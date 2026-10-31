//! C and C++ in Fenix, on lang-kit: clangd as the language server, told
//! where the build's `compile_commands.json` is; lldb-dap or GDB as the
//! debug adapter, and a launch for each CMake target; CMake's presets as
//! tasks; clang-format as the formatter and clang-tidy on save, with its
//! fixes; the
//! project doctor's checks; and the package manager page for vcpkg and
//! Conan.
//!
//! What it knows of the project is read from its files ([`project`],
//! [`presets`], [`fileapi`]), and what's installed is found ([`tools`]);
//! the rest is lang-kit's [`Kit`]. A fix that installs a tool, a
//! configure, or a `CMakeLists.txt` saved, sets them again.

pub mod doctor;
pub mod fileapi;
pub mod launch;
pub mod packages;
pub mod presets;
pub mod project;
pub mod tasks;
pub mod tidy;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, LaunchConfig, MouseEvent, Page, PickerItem, Problem};
use fenix_lang_kit::{Fenix, Host, Kit, PackageManager};

use fileapi::Executable;
use project::Project;
use tools::{Debugger, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's names for the languages.
const LANGUAGES: &[&str] = &["c", "cpp"];

/// Files whose saving changes what the project is.
const PROJECT_FILES: &[&str] = &["CMakeLists.txt", "CMakePresets.json", "CMakeUserPresets.json", "vcpkg.json", "vcpkg-configuration.json", "conanfile.txt", "conanfile.py", ".clangd"];

/// While there's no `compile_commands.json`, it's looked for this often:
/// a configure run from the task picker says nothing to the extension.
pub const WAIT_MILLIS: u32 = 5000;

/// What the pack sets in Fenix that lang-kit's `Host` has no call for
/// (0.8.6): its launches, `${cmakeTarget}`, and problems with their
/// fixes. Fenix's own host calls; written down in the tests.
pub trait More {
    fn launches(&mut self, launches: &[LaunchConfig]) -> Result<(), String>;
    fn variable(&mut self, name: &str, choices: &[PickerItem]) -> Result<(), String>;
    fn fixes(&mut self, path: &str, problems: &[Problem]) -> Result<(), String>;
}

/// Fenix's.
pub struct FenixMore;

impl More for FenixMore {
    fn launches(&mut self, launches: &[LaunchConfig]) -> Result<(), String> {
        api::debug::launches(launches)
    }

    fn variable(&mut self, name: &str, choices: &[PickerItem]) -> Result<(), String> {
        api::debug::variable(name, choices)
    }

    fn fixes(&mut self, path: &str, problems: &[Problem]) -> Result<(), String> {
        api::diagnostics::set(path, problems)
    }
}

/// The package manager for `project`: vcpkg's for a manifest, else
/// Conan's; none for neither.
pub fn manager(project: &Project, tools: &Tools, root: &str, build: Option<String>) -> Option<Box<dyn PackageManager>> {
    if project.vcpkg.is_some() {
        return Some(Box::new(packages::Vcpkg { vcpkg: tools.vcpkg(), root: root.into(), build }));
    }
    let conan = project.conan.as_ref()?;
    Some(Box::new(packages::Conan { conan: tools.conan(), root: root.into(), file: conan.file.clone() }))
}

/// clangd, with the folder of `compile_commands.json` when clangd
/// wouldn't find it itself (it looks in each file's folders and their
/// `build`). It's relative to the project, where Fenix starts it.
pub fn server(clangd: &str, database: Option<&str>, clangd_file: bool) -> Command {
    let cmd = Command::new(clangd);
    match database {
        Some(dir) if !clangd_file && dir != "." && dir != "build" => cmd.arg(format!("--compile-commands-dir={dir}")),
        _ => cmd,
    }
}

/// What the settings say.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub preset: Option<String>,
    pub debugger: Debugger,
    pub tidy_on_save: bool,
}

pub struct Cpp {
    pub kit: Kit,
    root: String,
    settings: Settings,
    project: Project,
    tools: Tools,
    /// The compilation database's folder, and the reply CMake last wrote:
    /// what changes when the project's configured.
    database: Option<String>,
    reply: Option<String>,
    programs: Vec<Executable>,
    more: Box<dyn More>,
    /// clang-tidy's fixes, to set once it has said its problems.
    fixes: tidy::Fixes,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_adapter: Option<Command>,
    set_formatter: Option<Command>,
    set_launches: Option<Vec<LaunchConfig>>,
    configuring: Option<u64>,
    waiting: Option<u64>,
}

impl Default for Cpp {
    fn default() -> Cpp {
        let setting = |k: &str| api::setting(k).filter(|v| !v.trim().is_empty());
        let settings = Settings {
            preset: setting("cpp.preset"),
            debugger: Debugger::parse(&setting("cpp.debugger").unwrap_or_default()),
            tidy_on_save: setting("cpp.tidy_on_save").is_none_or(|v| v != "false"),
        };
        Cpp::new(Box::new(Fenix), Box::new(FenixMore), PROJECT, settings, &api::platform().os)
    }
}

impl Cpp {
    /// The pack for the project in `root`, talking to `host` and `more`,
    /// on `os`.
    pub fn new(mut host: Box<dyn Host>, more: Box<dyn More>, root: &str, settings: Settings, os: &str) -> Cpp {
        let mut tools = Tools::find(host.as_mut());
        tools.os = os.into();
        let kit = Kit::with_host("cpp", host);
        let mut c = Cpp {
            kit,
            root: root.into(),
            settings,
            project: Project::default(),
            tools,
            database: None,
            reply: None,
            programs: Vec::new(),
            more,
            fixes: tidy::Fixes::default(),
            set_server: None,
            set_adapter: None,
            set_formatter: None,
            set_launches: None,
            configuring: None,
            waiting: None,
        };
        c.read();
        c.configure();
        c
    }

    fn preset(&self) -> Option<&str> {
        self.settings.preset.as_deref()
    }

    /// The project's files, what its build wrote, and the query left for
    /// CMake's next configure.
    fn read(&mut self) {
        self.project = Project::read(&self.root);
        if !self.project.is_cpp() {
            return;
        }
        let root = std::path::Path::new(&self.root);
        for dir in self.project.build_dirs() {
            if let Err(why) = fileapi::ask(root, &dir) {
                self.kit.host.log(&format!("CMake's file API: {why}"));
            }
        }
        self.read_build();
    }

    /// What the build wrote: `compile_commands.json` and CMake's reply.
    /// Whether it changed.
    fn read_build(&mut self) -> bool {
        let database = self.project.database(&self.root, self.preset());
        let build = self.project.build_dir(self.preset());
        let reply = build.as_deref().and_then(|b| fileapi::index(std::path::Path::new(&self.root), b));
        if database == self.database && reply == self.reply {
            return false;
        }
        self.programs = match build.as_deref() {
            Some(b) => fileapi::executables(std::path::Path::new(&self.root), b).unwrap_or_else(|why| {
                self.kit.host.log(&format!("CMake's file API: {why}"));
                Vec::new()
            }),
            None => Vec::new(),
        };
        self.database = database;
        self.reply = reply;
        true
    }

    /// The kit's tasks, checks, linter and package manager, for the
    /// project and tools as they are.
    fn configure(&mut self) {
        let (project, tools, preset) = (&self.project, &self.tools, self.settings.preset.as_deref());
        self.kit.tasks = tasks::tasks(project, tools, preset, &self.programs);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(project, tools, preset, self.settings.debugger) {
            self.kit.doctor.program(program);
        }
        self.kit.doctor.extra = doctor::database(project, tools, &self.root, preset).into_iter().collect();
        match (&tools.clang_tidy, &self.database) {
            (Some(tidy), Some(db)) if self.settings.tidy_on_save && project.is_cpp() => self.kit.lint.add(tidy::linter(tidy, db, &self.root, self.fixes.clone())),
            _ => self.kit.lint.linters.clear(),
        }
        let manager = manager(project, tools, &self.root, project.build_dir(preset));
        match (self.kit.packages.as_mut(), manager) {
            (Some(page), Some(m)) => page.manager = m,
            (None, Some(m)) => self.kit.packages = Some(fenix_lang_kit::PackagesPage::new(m)),
            (_, None) => {}
        }
    }

    /// The server, the adapter, the formatter and the launches set in
    /// Fenix, where they changed; nothing found leaves Fenix's own. Not in
    /// a project that isn't C or C++ (an Arduino sketch is the Arduino
    /// extension's).
    fn apply(&mut self) {
        let programs: &[Executable] = if self.project.is_cpp() { &self.programs } else { &[] };
        let names: Vec<String> = self.kit.tasks.resolved().into_iter().map(|t| t.name).collect();
        let launches = launch::launches(programs, &names);
        if self.set_launches.as_ref() != Some(&launches) {
            if let Err(why) = self.more.launches(&launches) {
                self.kit.host.log(&format!("launches: {why}"));
            }
            if let Err(why) = self.more.variable(launch::VARIABLE, &launch::choices(programs)) {
                self.kit.host.log(&format!("${{{}}}: {why}", launch::VARIABLE));
            }
            self.set_launches = Some(launches);
        }
        let cpp = self.project.is_cpp();
        let server = self.tools.clangd.as_deref().filter(|_| cpp).map(|c| server(c, self.database.as_deref(), self.project.clangd_db.is_some()));
        let adapter = self.tools.adapter(self.settings.debugger).map(|(_, c)| c).filter(|_| cpp);
        let formatter = self.tools.clang_format.as_deref().filter(|_| cpp).map(tidy::formatter);
        let languages: Vec<String> = LANGUAGES.iter().map(|l| l.to_string()).collect();
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
        }
        if adapter != self.set_adapter {
            if let Err(why) = host.debug_adapter(&languages, adapter.as_ref()) {
                host.log(&format!("debug adapter: {why}"));
            }
            self.set_adapter = adapter;
        }
        if formatter != self.set_formatter {
            if let Err(why) = host.formatter(&languages, formatter.as_ref()) {
                host.log(&format!("formatter: {why}"));
            }
            self.set_formatter = formatter;
        }
        // No database yet: looked for again in a while.
        if cpp && self.database.is_none() && self.project.database_dir(self.settings.preset.as_deref()).is_some() && self.waiting.is_none() {
            self.waiting = Some(host.set_timer(WAIT_MILLIS));
        }
    }

    /// The tasks and the rest set again.
    fn refresh(&mut self) {
        self.configure();
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
        }
        self.apply();
    }

    /// What the doctor found since: a tool a fix installed is used.
    fn follow(&mut self) {
        let checked = |name: &str| self.kit.doctor.programs.iter().any(|p| p.name == name);
        let found = |name: &str| self.kit.doctor.found.get(name).cloned();
        let mut tools = self.tools.clone();
        for (name, slot) in [("cmake", &mut tools.cmake), ("ninja", &mut tools.ninja), ("clangd", &mut tools.clangd), ("clang-format", &mut tools.clang_format), ("clang-tidy", &mut tools.clang_tidy), ("conan", &mut tools.conan)] {
            if checked(name) {
                *slot = found(name);
            }
        }
        if tools.lldb_dap.is_none() && checked("lldb-dap") {
            tools.lldb_dap = found("lldb-dap");
        }
        if tools != self.tools {
            self.tools = tools;
            self.refresh();
        }
    }

    /// The project's files changed: read again, and checked again.
    fn reload(&mut self) {
        self.read();
        self.configure();
        let host = self.kit.host.as_mut();
        if let Err(why) = self.kit.tasks.apply(host) {
            host.log(&format!("tasks: {why}"));
        }
        self.kit.doctor.run(host);
        self.apply();
    }

    /// The build may have been configured: when it was, what it wrote is
    /// used, and the doctor checks again.
    fn rescan(&mut self) {
        if self.read_build() {
            self.configure();
            let host = self.kit.host.as_mut();
            if let Err(why) = self.kit.tasks.apply(host) {
                host.log(&format!("tasks: {why}"));
            }
            self.kit.doctor.run(host);
            self.apply();
        }
    }

    /// `cpp.configure`: the configure task, run; the build read when it ends.
    fn configure_now(&mut self) -> Result<(), String> {
        let task = self.kit.tasks.resolved().into_iter().find(|t| t.name == "configure").ok_or("there's no CMake build to configure")?;
        self.configuring = Some(self.kit.host.run_task("configure", &task.cmd)?);
        Ok(())
    }

    /// An event that's the pack's own.
    fn own(&mut self, event: &Event) -> bool {
        match event {
            Event::TaskFinished((id, _)) if self.configuring == Some(*id) => {
                self.configuring = None;
                self.rescan();
                true
            }
            Event::Timer(id) if self.waiting == Some(*id) => {
                self.waiting = None;
                self.rescan();
                self.apply();
                true
            }
            _ => false,
        }
    }
}

impl Extension for Cpp {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match id {
            doctor::CONFIGURE => self.configure_now(),
            _ => self.kit.run_command(id).unwrap_or_else(|| Err(format!("no command {id} here"))),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        if self.own(&event) {
            return;
        }
        let saved = match &event {
            Event::Saved(name) => Some(PROJECT_FILES.iter().any(|f| name.replace('\\', "/").rsplit('/').next() == Some(f))),
            _ => None,
        };
        self.kit.handle(&event);
        // clang-tidy said its problems: with their fixes.
        let fixed: Vec<(String, Vec<Problem>)> = self.fixes.borrow_mut().drain(..).collect();
        for (file, problems) in fixed {
            if let Err(why) = self.more.fixes(&format!("{PROJECT}/{file}"), &problems) {
                self.kit.host.log(&format!("clang-tidy's fixes: {why}"));
            }
        }
        match saved {
            Some(true) => self.reload(),
            // Any file saved: a configure since is noticed.
            Some(false) => self.rescan(),
            None => {}
        }
        self.follow();
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        self.kit.page(page, width, height).unwrap_or_default()
    }

    fn page_key(&mut self, page: u64, key: Key) {
        self.kit.page_key(page, key);
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        self.kit.page_mouse(page, &event);
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        self.kit.page_state(page)
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        self.kit.page_restore(page, kind, state)
    }

    fn page_closed(&mut self, page: u64) {
        self.kit.page_closed(page);
    }
}

export!(Cpp);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;
    use fenix_extension_api::Health;
    use fenix_lang_kit::host::mock::Shared;

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    fn cmake_project(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::write(dir.join("CMakeLists.txt"), "cmake_minimum_required(VERSION 3.25)\nproject(orbit CXX)\n").unwrap();
        std::fs::write(dir.join("CMakePresets.json"), include_str!("samples/CMakePresets.json")).unwrap();
        dir
    }

    fn settings() -> Settings {
        Settings { tidy_on_save: true, ..Settings::default() }
    }

    /// [`More`]'s calls, written down with the host's.
    struct Written(Shared);

    impl More for Written {
        fn launches(&mut self, launches: &[LaunchConfig]) -> Result<(), String> {
            self.0 .0.borrow_mut().calls.push(format!("launches {}", launches.iter().map(|l| format!("{} ({})", l.name, l.pre_launch_task.as_deref().unwrap_or("-"))).collect::<Vec<_>>().join(", ")));
            Ok(())
        }

        fn variable(&mut self, name: &str, choices: &[PickerItem]) -> Result<(), String> {
            self.0 .0.borrow_mut().calls.push(format!("variable {name}: {}", choices.iter().map(|c| c.value.as_str()).collect::<Vec<_>>().join(", ")));
            Ok(())
        }

        fn fixes(&mut self, path: &str, problems: &[Problem]) -> Result<(), String> {
            self.0 .0.borrow_mut().calls.push(format!("fixes {path}: {}", problems.iter().map(|p| p.fixes.len().to_string()).collect::<Vec<_>>().join(" ")));
            Ok(())
        }
    }

    fn pack(dir: &std::path::Path, host: &Shared) -> Cpp {
        let mut c = Cpp::new(Box::new(host.clone()), Box::new(Written(host.clone())), &dir.to_string_lossy(), settings(), "linux");
        if let Some(page) = c.kit.packages.as_mut() {
            page.cache_dir = None;
        }
        c
    }

    fn said(c: &mut Cpp, process: u64, text: &str) {
        c.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: text.into() }));
        c.handle(Event::ProcessExit((process, Some(0))));
    }

    fn found(host: &Shared, programs: &[(&str, &str)]) {
        host.0.borrow_mut().programs.extend(programs.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    }

    #[test]
    fn a_cmake_project_gets_its_server_adapter_tasks_and_checks() {
        let dir = cmake_project("activate");
        let host = Shared::default();
        found(&host, &[("cmake", "/usr/bin/cmake"), ("ninja", "/usr/bin/ninja"), ("g++", "/usr/bin/g++"), ("clangd", "/usr/bin/clangd"), ("lldb-dap", "/usr/lib/llvm-18/bin/lldb-dap"), ("clang-format", "/usr/bin/clang-format")]);
        let mut c = pack(&dir, &host);
        assert!(dir.join("build/debug").join(fileapi::QUERY).is_file() && dir.join("build/release").join(fileapi::QUERY).is_file(), "CMake is asked for its code model");
        c.activate().unwrap();
        let calls = host.take();
        assert_eq!(calls[0], "tasks configure, clean, build, test, configure release, build release");
        assert!(calls.contains(&"server c,cpp: /usr/bin/clangd".to_string()), "no database yet: {calls:?}");
        assert!(calls.contains(&"adapter c,cpp: /usr/lib/llvm-18/bin/lldb-dap".to_string()));
        assert!(calls.contains(&"formatter c,cpp: /usr/bin/clang-format --assume-filename=${file}".to_string()));
        assert!(calls.contains(&"spawn 1 /usr/bin/g++ --version".to_string()), "{calls:?}");
        let timer = calls.iter().find_map(|c| c.strip_prefix("timer ")?.strip_suffix(" 5000")?.parse::<u64>().ok());

        // Each says its version; then the checks, the database's last.
        for (i, text) in ["g++ (GCC) 13.3.0", "cmake version 3.28.3", "1.11.1", "clangd version 18.1.3", "clang-format version 18.1.3"].iter().enumerate() {
            said(&mut c, i as u64 + 1, text);
        }
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(
            checks,
            [
                ("Compiler".into(), Health::Ok),
                ("CMake".into(), Health::Ok),
                ("Ninja".into(), Health::Ok),
                ("clangd".into(), Health::Ok),
                ("clang-format".into(), Health::Ok),
                ("clang-tidy".into(), Health::Warn),
                ("Debugger".into(), Health::Ok),
                ("compile_commands.json".into(), Health::Warn),
            ]
        );
        assert_eq!(host.0.borrow().checks[7].fix, Some(("Configure".into(), doctor::CONFIGURE.into())));

        // Configured by its fix row: the database and the programs are used.
        host.take();
        c.run_command(doctor::CONFIGURE).unwrap();
        let task = host.take().iter().find_map(|l| l.strip_prefix("task ")?.split_once(" configure: ").filter(|(_, cmd)| *cmd == "/usr/bin/cmake --preset debug").and_then(|(id, _)| id.parse::<u64>().ok())).expect("configured as a task");
        std::fs::write(dir.join("build/debug/compile_commands.json"), "[]").unwrap();
        fileapi::tests::reply(&dir, "build/debug");
        c.handle(Event::TaskFinished((task, true)));
        let calls = host.take();
        assert!(calls.contains(&"server c,cpp: /usr/bin/clangd --compile-commands-dir=build/debug".to_string()), "{calls:?}");
        assert!(calls.iter().any(|l| l.starts_with("tasks ") && l.ends_with("run orbit_app, run orbit_tests")), "{calls:?}");
        assert!(calls.iter().any(|l| l.starts_with("spawn")), "checked again");
        // The timer finds nothing new, and isn't set again.
        if let Some(t) = timer {
            c.handle(Event::Timer(t));
            assert!(!host.take().iter().any(|l| l.starts_with("timer") || l.starts_with("server")));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn each_target_is_a_launch_once_cmake_says_it() {
        let dir = cmake_project("debug");
        let host = Shared::default();
        found(&host, &[("gdb", "/usr/bin/gdb")]);
        let mut c = pack(&dir, &host);
        c.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"adapter c,cpp: /usr/bin/gdb --interpreter=dap".to_string()));
        assert!(calls.contains(&"launches ".to_string()) && calls.contains(&"variable cmakeTarget: ".to_string()), "none yet: {calls:?}");
        // Configured: saving any file notices the reply.
        fileapi::tests::reply(&dir, "build/debug");
        c.handle(Event::Saved("main.cpp".into()));
        let calls = host.take();
        assert!(calls.contains(&"launches CMake: orbit_app (build orbit_app), CMake: orbit_tests (build orbit_tests)".to_string()), "{calls:?}");
        assert!(calls.contains(&"variable cmakeTarget: /home/user/work/cpp-pack/sample/build/debug/bin/orbit, /home/user/work/cpp-pack/sample/build/debug/orbit_tests".to_string()), "{calls:?}");
        let tasks = host.0.borrow().tasks.iter().map(|t| (t.name.clone(), line(&t.cmd))).collect::<Vec<_>>();
        assert!(tasks.contains(&("build orbit_app".into(), "cmake --build --preset debug --target orbit_app".into())), "{tasks:?}");
        assert!(tasks.iter().any(|t| t.0 == "run orbit_tests"));
        // The same again: not set again.
        c.handle(Event::Saved("main.cpp".into()));
        assert!(!host.take().iter().any(|l| l.starts_with("launches")));
        assert!(!dir.join(".fenix/tools.json").exists(), "the project's own launch is left alone");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn clang_tidy_lints_once_theres_a_database() {
        let dir = cmake_project("tidy");
        std::fs::create_dir_all(dir.join("build/debug")).unwrap();
        std::fs::write(dir.join("build/debug/compile_commands.json"), "[]").unwrap();
        let host = Shared::default();
        found(&host, &[("clang-tidy", "/usr/bin/clang-tidy")]);
        let mut c = pack(&dir, &host);
        c.activate().unwrap();
        host.take();
        host.0.borrow_mut().here = Some((format!("{}/src/orbit.cpp", dir.display()), Some(dir.to_string_lossy().into_owned())));
        c.handle(Event::Saved("orbit.cpp".into()));
        let spawn = host.take().iter().find_map(|l| l.strip_prefix("spawn ")?.split_once(' ').filter(|(_, cmd)| *cmd == "/usr/bin/clang-tidy -p build/debug --quiet --export-fixes=build/debug/fenix-tidy-src_orbit.cpp.yaml src/orbit.cpp").map(|(id, _)| id.parse::<u64>().unwrap())).expect("linted");
        for l in include_str!("samples/tidy-broken.txt").replace("broken.cpp", "orbit.cpp").lines() {
            c.handle(Event::ProcessOutput(api::ProcessOutput { process: spawn, stderr: false, line: l.into() }));
        }
        c.handle(Event::ProcessExit((spawn, Some(1))));
        assert_eq!(host.0.borrow().problems["/project/src/orbit.cpp"].len(), 2);
        assert!(!host.take().iter().any(|l| l.starts_with("fixes")), "it wrote none");

        // With the fixes it wrote: set with them, after its problems.
        c.handle(Event::Saved("orbit.cpp".into()));
        let calls = host.take();
        let spawn = calls.iter().find_map(|l| l.strip_prefix("spawn ")?.split_once(' ').map(|(id, _)| id.parse::<u64>().unwrap())).expect("linted");
        assert!(calls.iter().any(|l| l.ends_with("--export-fixes=build/debug/fenix-tidy-src_orbit.cpp.yaml src/orbit.cpp")), "{calls:?}");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/orbit.cpp"), include_str!("samples/tidy-fixes.cpp")).unwrap();
        std::fs::write(dir.join("build/debug/fenix-tidy-src_orbit.cpp.yaml"), include_str!("samples/tidy-fixes.yaml")).unwrap();
        for l in include_str!("samples/tidy-fixes.txt").lines() {
            c.handle(Event::ProcessOutput(api::ProcessOutput { process: spawn, stderr: false, line: l.into() }));
        }
        c.handle(Event::ProcessExit((spawn, Some(0))));
        let calls = host.take();
        assert_eq!(calls.last().map(String::as_str), Some("fixes /project/src/orbit.cpp: 1 1 1"), "{calls:?}");

        // Saving CMakePresets.json reads the project again.
        std::fs::write(dir.join("CMakePresets.json"), r#"{"version": 3, "configurePresets": [{"name": "dev", "binaryDir": "${sourceDir}/out"}]}"#).unwrap();
        c.handle(Event::Saved("CMakePresets.json".into()));
        let calls = host.take();
        assert!(calls.contains(&"tasks configure, clean, build, test".to_string()), "{calls:?}");
        assert_eq!(line(&host.0.borrow().tasks[0].cmd), "cmake --preset dev -DCMAKE_EXPORT_COMPILE_COMMANDS=ON");
        assert!(c.kit.lint.linters.is_empty(), "no database in out yet");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_project_that_isnt_cs_is_left_alone() {
        let dir = temp("other");
        std::fs::write(dir.join("blink.ino"), "").unwrap();
        std::fs::write(dir.join("main.cpp"), "").unwrap();
        let host = Shared::default();
        found(&host, &[("clangd", "/usr/bin/clangd"), ("clang-format", "/usr/bin/clang-format")]);
        let mut c = pack(&dir, &host);
        c.activate().unwrap();
        let calls = host.take();
        assert!(!calls.iter().any(|l| l.starts_with("server") || l.starts_with("formatter") || l.starts_with("tasks") || l.starts_with("timer")), "{calls:?}");
        assert!(c.kit.packages.is_none());
        assert!(c.run_command("cpp.nothing").is_err());
        assert!(c.run_command(doctor::CONFIGURE).is_err());
        assert_eq!(line(&server("clangd", Some("build"), false)), "clangd", "clangd finds build/ itself");
        assert_eq!(line(&server("clangd", Some("out"), true)), "clangd", ".clangd says where");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_conan_project_gets_its_page() {
        let dir = temp("conan-page");
        std::fs::write(dir.join("conanfile.txt"), "[requires]\nhello/1.0\n").unwrap();
        std::fs::write(dir.join("main.cpp"), "").unwrap();
        let host = Shared::default();
        found(&host, &[("conan", "/u/conan")]);
        let mut c = pack(&dir, &host);
        assert_eq!(c.kit.packages.as_ref().map(|p| p.manager.name()), Some("conan"));
        c.activate().unwrap();
        assert!(host.take().contains(&"spawn 2 /u/conan profile path default".to_string()));
        c.run_command("cpp.packages").unwrap();
        let calls = host.take();
        assert!(calls.iter().any(|l| l.ends_with("/u/conan graph info . --format=json")), "{calls:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
