//! C# and .NET in Fenix, on lang-kit: csharp-ls as the language server,
//! netcoredbg as the debug adapter and a launch for each program, the
//! build on save as the linter where csharp-ls isn't,
//! CSharpier as the formatter, the tasks of the solution and its
//! projects, the project doctor's checks, and NuGet on the package
//! manager page.
//!
//! What it knows of the solution is read from its files ([`project`]),
//! and what's installed is found ([`tools`]); the rest is lang-kit's
//! [`Kit`]. A fix that installs a tool, a project file saved, or moving
//! to another project's file sets them again.

pub mod debug;
pub mod doctor;
pub mod msbuild;
pub mod packages;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, LaunchConfig, MouseEvent, Page};
use fenix_lang_kit::{find_version, Fenix, Host, Kit, Version};

use project::{Project, Solution};
use tools::{Platform, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language.
const CSHARP: &[&str] = &["csharp"];

/// Files whose saving changes what the solution is.
const SOLUTION_FILES: &[&str] = &["global.json", "Directory.Build.props", "Directory.Packages.props", "dotnet-tools.json", "launchSettings.json"];

/// Sets the launches `SPC u u` offers (`launches-set`, 0.8.6), which
/// lang-kit's `Host` doesn't have: the host's own in Fenix, recorded in
/// the tests.
pub type Launches = Box<dyn FnMut(&[LaunchConfig]) -> Result<(), String>>;

/// Whether a saved file's project is built for its problems: as the
/// setting says, else when there's no csharp-ls, whose own come as Fenix
/// asks for them (pull diagnostics, 0.8.6).
pub fn builds_on_save(setting: Option<bool>, tools: &Tools) -> bool {
    setting.unwrap_or(tools.csharp_ls.is_none())
}

/// The setting, read: none when it isn't set.
fn lint_setting() -> Option<bool> {
    api::setting("dotnet.lint_on_save").filter(|v| !v.trim().is_empty()).map(|v| v != "false")
}

/// CSharpier as the formatter: the project's (its tools manifest's, run
/// by `dotnet`), else the one installed. The buffer on stdin, `${file}`
/// its path, so the project's `.csharpierrc` and ignore file are used.
pub fn formatter(solution: &Solution, tools: &Tools) -> Option<Command> {
    let args = ["format", "--write-stdout", "--stdin-path", "${file}"];
    match (&solution.csharpier, &tools.dotnet, &tools.csharpier) {
        // CSharpier before 1.0 had no `format`.
        (Some(v), Some(_), _) if !v.starts_with("0.") => Some(tools.dotnet().arg("csharpier").args(args)),
        (_, _, Some(c)) => Some(Command::new(c).args(args)),
        _ => None,
    }
}

/// The language server: csharp-ls, given the solution.
pub fn server(solution: &Solution, tools: &Tools) -> Option<Command> {
    let ls = Command::new(tools.csharp_ls.as_deref()?);
    Some(match &solution.file {
        Some(f) => ls.args(["--solution", f]),
        None => ls,
    })
}

/// A `dotnet test` held for the debugger.
struct Testing {
    process: u64,
    output: String,
}

pub struct Dotnet {
    pub kit: Kit,
    root: String,
    pub solution: Solution,
    tools: Tools,
    platform: Platform,
    lint_on_save: Option<bool>,
    /// The project of the file you're in.
    focus: Option<String>,
    /// The last program you were in.
    program: Option<String>,
    /// The test host `dotnet test` holds for the debugger.
    held: Option<u32>,
    testing: Option<Testing>,
    launches: Launches,
    /// Builds of the project to debug: the doctor checks again after.
    building: Vec<u64>,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_adapter: Option<Command>,
    set_formatter: Option<Command>,
    set_launches: Option<Vec<LaunchConfig>>,
}

impl Default for Dotnet {
    fn default() -> Dotnet {
        let p = api::platform();
        Dotnet::new(Box::new(Fenix), PROJECT, Platform::new(&p.os, &p.arch), lint_setting(), Box::new(api::debug::launches))
    }
}

impl Dotnet {
    /// The pack for the project in `root`, talking to `host`.
    pub fn new(mut host: Box<dyn Host>, root: &str, platform: Platform, lint_on_save: Option<bool>, launches: Launches) -> Dotnet {
        let tools = Tools::find(host.as_mut());
        let solution = Solution::read(root, None);
        let kit = Kit::with_host("dotnet", host).packages(Box::new(packages::Nuget { dotnet: tools.dotnet(), target: None, projects: Vec::new(), add_to: None, root: root.into(), central: false }));
        let mut d = Dotnet {
            kit,
            root: root.into(),
            solution,
            tools,
            platform,
            lint_on_save,
            focus: None,
            program: None,
            held: None,
            testing: None,
            launches,
            building: Vec::new(),
            set_server: None,
            set_adapter: None,
            set_formatter: None,
            set_launches: None,
        };
        d.configure();
        d
    }

    /// The SDK's version as the doctor last found it.
    fn sdk(&self) -> Option<Version> {
        self.kit.doctor.checks().iter().find(|c| c.label == ".NET SDK").and_then(|c| find_version(&c.detail))
    }

    /// The project the doctor says is built or not: the program you were
    /// in last, else the first.
    pub fn target(&self) -> Option<&Project> {
        let runnable = |name: &Option<String>| name.as_deref().and_then(|n| self.solution.by_name(n)).filter(|p| p.runnable());
        runnable(&self.program).or_else(|| self.solution.projects.iter().find(|p| p.runnable() && !p.test))
    }

    /// The project packages are added to: the one you're in, else the
    /// first that isn't a test project.
    fn add_to(&self) -> Option<&Project> {
        self.focus.as_deref().and_then(|n| self.solution.by_name(n)).or_else(|| self.solution.projects.iter().find(|p| !p.test)).or_else(|| self.solution.projects.first())
    }

    /// The kit's tasks, checks, linter and package manager, for the
    /// solution and tools as they are.
    fn configure(&mut self) {
        let (s, tools) = (&self.solution, &self.tools);
        self.kit.tasks = tasks::tasks(s, tools);
        let netcoredbg = self.platform.netcoredbg();
        self.kit.doctor.programs.clear();
        for program in doctor::programs(s, tools, self.sdk().as_ref(), netcoredbg.is_some()) {
            self.kit.doctor.program(program);
        }
        if let Some(install) = netcoredbg {
            self.kit.installer.add(install);
        }
        self.kit.doctor.extra = doctor::built(&self.root, self.target()).into_iter().collect();
        match (&tools.dotnet, builds_on_save(self.lint_on_save, tools)) {
            (Some(_), true) => self.kit.lint.add(msbuild::linter(&tools.dotnet(), s)),
            _ => self.kit.lint.linters.clear(),
        }
        let manager = packages::Nuget {
            dotnet: tools.dotnet(),
            target: s.target(),
            projects: s.projects.iter().map(|p| (p.name.clone(), p.path.clone())).collect(),
            add_to: self.add_to().map(|p| p.path.clone()),
            root: self.root.clone(),
            central: std::path::Path::new(&self.root).join("Directory.Packages.props").is_file(),
        };
        if let Some(page) = self.kit.packages.as_mut() {
            page.manager = Box::new(manager);
        }
    }

    /// The server, the adapter, the formatter and the launches set in
    /// Fenix, where they changed.
    fn apply(&mut self) {
        let server = server(&self.solution, &self.tools);
        let adapter = self.tools.netcoredbg.as_deref().map(debug::adapter);
        let formatter = formatter(&self.solution, &self.tools);
        let launches = debug::launches(&self.solution, self.tools.dotnet.as_deref(), self.held);
        if self.set_launches.as_ref() != Some(&launches) {
            if let Err(why) = (self.launches)(&launches) {
                self.kit.host.log(&format!("launches: {why}"));
            }
            self.set_launches = Some(launches);
        }
        let languages: Vec<String> = CSHARP.iter().map(|l| l.to_string()).collect();
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
    }

    /// The kit's again, its tasks set, and the rest applied.
    fn refresh(&mut self) {
        self.configure();
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
        }
        self.apply();
    }

    /// What the doctor found since: a tool a fix installed is used, and
    /// the SDK's version picks the csharp-ls to install.
    fn follow(&mut self, sdk: Option<Version>) {
        let checked = |name: &str| self.kit.doctor.programs.iter().any(|p| p.name == name);
        let found = |name: &str| self.kit.doctor.found.get(name).cloned();
        let mut tools = self.tools.clone();
        for (name, slot) in [("dotnet", &mut tools.dotnet), ("csharp-ls", &mut tools.csharp_ls), ("netcoredbg", &mut tools.netcoredbg), ("csharpier", &mut tools.csharpier)] {
            if checked(name) {
                *slot = found(name);
            }
        }
        if tools != self.tools || self.sdk() != sdk {
            self.tools = tools;
            self.refresh();
        }
    }

    /// The solution's files changed: read again, and checked again.
    fn reload(&mut self) {
        self.solution = Solution::read(&self.root, self.solution.file.as_deref());
        self.refresh();
        self.kit.doctor.run(self.kit.host.as_mut());
    }

    /// The file you're in is `path` (under `/project`): its project's
    /// the one packages are added to, and the doctor's to build.
    fn focused(&mut self, path: &str) {
        let path = path.replace('\\', "/");
        let rel = path.strip_prefix(&format!("{PROJECT}/")).or_else(|| path.strip_prefix(&format!("{}/", self.root))).unwrap_or(&path);
        let Some((p, runnable)) = self.solution.project_of(rel).map(|p| (p.name.clone(), p.runnable())) else { return };
        if self.focus.as_deref() != Some(&p) {
            if runnable {
                self.program = Some(p.clone());
            }
            self.focus = Some(p);
            self.configure();
        }
    }

    /// The test host held, offered to attach to; none once it's gone.
    fn hold(&mut self, pid: Option<u32>) {
        self.held = pid;
        self.apply();
        if let Some(pid) = pid {
            self.kit.host.notify(&format!("dotnet test holds process {pid} for the debugger: SPC u u, then .NET: the tests"));
        }
    }

    /// The test project you're in, else the solution's tests, run held
    /// for the debugger.
    fn debug_tests(&mut self) -> Result<(), String> {
        let tests: Vec<&Project> = self.solution.projects.iter().filter(|p| p.test).collect();
        if tests.is_empty() {
            return Err("there's no test project to debug".into());
        }
        let target = match self.focus.as_deref().and_then(|n| tests.iter().find(|p| p.name == n)) {
            Some(p) => Some(p.path.clone()),
            None if tests.len() == 1 => Some(tests[0].path.clone()),
            None => self.solution.target(),
        };
        let host = self.kit.host.as_mut();
        if let Some(t) = self.testing.take() {
            host.kill(t.process);
        }
        let process = host.spawn(&debug::test_held(&self.tools.dotnet(), target.as_deref()))?;
        host.close_stdin(process);
        self.testing = Some(Testing { process, output: String::new() });
        host.notify("dotnet test: building, then waiting for the debugger");
        Ok(())
    }

    fn build_target(&mut self) -> Result<(), String> {
        let p = self.target().ok_or("no project to build")?;
        let (name, cmd) = (format!("build {}", p.name), self.tools.dotnet().args(["build", &p.path]));
        let task = self.kit.host.run_task(&name, &cmd)?;
        self.building.push(task);
        Ok(())
    }

    fn own_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            doctor::INSTALL_SDK => {
                let page = doctor::sdk_page(&self.solution);
                #[cfg(target_family = "wasm")]
                api::open_url(&page)?;
                self.kit.host.notify(&format!("The .NET SDK: {page}"));
                Ok(())
            }
            doctor::BUILD => self.build_target(),
            "dotnet.debug-tests" => self.debug_tests(),
            _ => Err(format!("no command {id} here")),
        }
    }

    /// An event for the pack's own: whether it was.
    fn own_event(&mut self, event: &Event) -> bool {
        match event {
            Event::ProcessOutput(o) if self.testing.as_ref().is_some_and(|t| t.process == o.process) => {
                let pid = debug::test_host(&o.line);
                if let Some(t) = self.testing.as_mut() {
                    t.output.push_str(&o.line);
                    t.output.push('\n');
                }
                if pid.is_some() && self.held.is_none() {
                    self.hold(pid);
                }
                true
            }
            Event::ProcessExit((p, _)) if self.testing.as_ref().is_some_and(|t| t.process == *p) => {
                let t = self.testing.take().map(|t| t.output).unwrap_or_default();
                self.hold(None);
                self.kit.host.open_text_beside("dotnet test", &t, None);
                true
            }
            Event::TaskFinished((t, _)) if self.building.contains(t) => {
                self.building.retain(|b| b != t);
                self.configure();
                self.kit.doctor.run(self.kit.host.as_mut());
                true
            }
            Event::FocusChanged(Some(path)) => {
                self.focused(path);
                false
            }
            Event::SettingChanged(key) if key == "dotnet.lint_on_save" => {
                #[cfg(target_family = "wasm")]
                {
                    self.lint_on_save = lint_setting();
                }
                self.configure();
                true
            }
            _ => false,
        }
    }
}

impl Extension for Dotnet {
    fn activate(&mut self) -> Result<(), String> {
        let sdk = self.sdk();
        self.kit.activate()?;
        self.apply();
        self.follow(sdk);
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let sdk = self.sdk();
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            None => self.own_command(id),
        };
        self.follow(sdk);
        done
    }

    fn handle(&mut self, event: Event) {
        let sdk = self.sdk();
        if self.own_event(&event) {
            return;
        }
        let saved = match &event {
            Event::Saved(name) => {
                let file = name.replace('\\', "/");
                let file = file.rsplit('/').next().unwrap_or(&file).to_string();
                SOLUTION_FILES.contains(&file.as_str()) || file.ends_with(".sln") || file.ends_with(".slnx") || project::PROJECT_EXTENSIONS.iter().any(|x| file.ends_with(&format!(".{x}")))
            }
            _ => false,
        };
        self.kit.handle(&event);
        if saved {
            self.reload();
        }
        self.follow(sdk);
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

export!(Dotnet);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{orbit, write};
    use fenix_extension_api::Health;
    use fenix_lang_kit::host::mock::Shared;

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    fn host() -> Shared {
        let host = Shared::default();
        host.0.borrow_mut().programs.extend([("dotnet", "/usr/bin/dotnet"), ("csharp-ls", "/h/.dotnet/tools/csharp-ls"), ("netcoredbg", "/tools/netcoredbg/netcoredbg")].map(|(a, b)| (a.to_string(), b.to_string())));
        host
    }

    /// The pack, its launches written down with the host's calls
    /// (`launches <name>; <name>`).
    fn pack_with(dir: &std::path::Path, host: &Shared, lint: Option<bool>) -> Dotnet {
        let calls = host.clone();
        let launches: Launches = Box::new(move |l| {
            calls.0.borrow_mut().calls.push(format!("launches {}", l.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join("; ")));
            Ok(())
        });
        let mut d = Dotnet::new(Box::new(host.clone()), &dir.to_string_lossy(), Platform::new("linux", "x86_64"), lint, launches);
        if let Some(page) = d.kit.packages.as_mut() {
            page.cache_dir = None;
        }
        d
    }

    fn pack(dir: &std::path::Path, host: &Shared) -> Dotnet {
        pack_with(dir, host, Some(true))
    }

    fn said(d: &mut Dotnet, process: u64, text: &str, code: i32) {
        for l in text.lines() {
            d.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: l.into() }));
        }
        d.handle(Event::ProcessExit((process, Some(code))));
    }

    fn spawned(calls: &[String], what: &str) -> u64 {
        calls.iter().find_map(|c| c.strip_prefix("spawn ").filter(|c| c.contains(what)).and_then(|c| c.split(' ').next()?.parse().ok())).unwrap_or_else(|| panic!("{what}: {calls:?}"))
    }

    #[test]
    fn a_solution_gets_its_server_adapter_tasks_and_checks() {
        let dir = orbit("activate");
        let host = host();
        let mut d = pack(&dir, &host);
        d.activate().unwrap();
        let calls = host.take();
        assert!(calls[0].starts_with("tasks build, test, restore, clean, format, build Orbit.App"), "{calls:?}");
        assert!(calls.contains(&"server csharp: /h/.dotnet/tools/csharp-ls --solution Orbit.sln".to_string()), "{calls:?}");
        assert!(calls.contains(&"adapter csharp: /tools/netcoredbg/netcoredbg --interpreter=vscode".to_string()), "{calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("launches .NET: Orbit.App; .NET: Orbit.Api; .NET: Orbit.Worker; .NET: Orbit.App, stopped")), "{calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("formatter")), "no CSharpier: csharp-ls formats");
        let sdk = spawned(&calls, "/usr/bin/dotnet --version");
        let server = spawned(&calls, "csharp-ls --version");
        let debugger = spawned(&calls, "netcoredbg --version");
        said(&mut d, sdk, "8.0.132", 0);
        said(&mut d, server, "csharp-ls, 0.16.0.0", 0);
        said(&mut d, debugger, "NET Core debugger 3.1.2-1 (83214c3, Release)\n\nCopyright (c) 2020 Samsung Electronics Co., LTD", 0);
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(checks, [(".NET SDK".into(), Health::Ok), ("Language server".into(), Health::Ok), ("Debugger".into(), Health::Ok), ("CSharpier".into(), Health::Warn), ("Debugging".into(), Health::Warn)]);
        assert_eq!(host.0.borrow().checks[4].fix, Some(("Build Orbit.App".into(), doctor::BUILD.into())));
        assert_eq!(d.sdk(), Version::parse("8.0.132"));
        // The SDK known: csharp-ls's fix is the one it runs.
        match d.kit.doctor.fix_for(doctor::INSTALL_SERVER) {
            Some((_, fenix_lang_kit::Fix::Task(cmd))) => assert!(line(cmd).ends_with("--version 0.16.0")),
            other => panic!("{other:?}"),
        }

        // Its build fix, and the checks after.
        host.take();
        d.run_command(doctor::BUILD).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").filter(|c| c.ends_with("dotnet build src/Orbit.App/Orbit.App.csproj")).and_then(|c| c.split(' ').next()?.parse::<u64>().ok())).unwrap();
        write(&dir, "src/Orbit.App/bin/Debug/net8.0/Orbit.App.dll", "MZ");
        d.handle(Event::TaskFinished((task, true)));
        assert!(host.take().iter().any(|c| c.contains("--version")), "checked again");
        assert_eq!(d.kit.doctor.extra[0].health, Health::Ok);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_file_youre_in_says_what_the_doctor_builds_and_where_packages_go() {
        let dir = orbit("focus");
        let host = host();
        let mut d = pack(&dir, &host);
        d.activate().unwrap();
        host.take();
        d.handle(Event::FocusChanged(Some("/project/src/Orbit.Api/Program.cs".into())));
        assert!(!host.take().iter().any(|c| c.starts_with("adapter") || c.starts_with("launches")), "each program has its launch already");
        // A library's file: the program stays.
        d.handle(Event::FocusChanged(Some("/project/src/Orbit.Core/Orbits.cs".into())));
        assert_eq!(d.target().map(|p| p.name.as_str()), Some("Orbit.Api"), "the program you were in last");
        let page = d.kit.packages.as_ref().unwrap();
        assert_eq!(line(&page.manager.add("Polly")), "/usr/bin/dotnet add src/Orbit.Core/Orbit.Core.csproj package Polly");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn with_csharp_ls_its_problems_are_asked_for_and_the_build_on_save_is_off() {
        let dir = orbit("pull");
        let host = host();
        let mut d = pack_with(&dir, &host, None);
        assert!(d.kit.lint.linters.is_empty(), "csharp-ls is there");
        host.0.borrow_mut().here = Some((format!("{}/src/Orbit.Core/Class1.cs", dir.display()), Some(dir.to_string_lossy().into_owned())));
        d.activate().unwrap();
        host.take();
        d.handle(Event::Saved("Class1.cs".into()));
        assert!(!host.take().iter().any(|c| c.contains("dotnet build")));
        // Without it, the build is where problems come from.
        let none = Tools { dotnet: Some("dotnet".into()), ..Tools::default() };
        assert!(builds_on_save(None, &none));
        assert!(!builds_on_save(Some(false), &none), "the setting wins");
        assert!(builds_on_save(Some(true), &Tools { csharp_ls: Some("csharp-ls".into()), ..none }));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tests_held_for_the_debugger_are_attached_to() {
        let dir = orbit("tests");
        let host = host();
        let mut d = pack(&dir, &host);
        d.activate().unwrap();
        host.take();
        d.run_command("dotnet.debug-tests").unwrap();
        let calls = host.take();
        let test = spawned(&calls, "/usr/bin/dotnet test tests/Orbit.Tests/Orbit.Tests.csproj");
        for l in include_str!("samples/vstest-debug.txt").lines() {
            d.handle(Event::ProcessOutput(api::ProcessOutput { process: test, stderr: false, line: l.into() }));
        }
        let calls = host.take();
        assert!(calls.iter().any(|c| c.starts_with("launches ") && c.ends_with("; .NET: the tests dotnet test holds (process 24793)")), "{calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("adapter")), "the adapter stays as it was");
        assert!(calls.iter().any(|c| c.starts_with("notify dotnet test holds process 24793")));
        d.handle(Event::ProcessExit((test, Some(1))));
        let calls = host.take();
        assert!(calls.iter().any(|c| c.starts_with("launches ") && !c.contains("the tests")), "gone with it: {calls:?}");
        assert!(calls.contains(&"text dotnet test".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn csharpier_installed_by_its_fix_formats_and_saving_a_project_reads_it_again() {
        let dir = orbit("csharpier");
        let host = host();
        let mut d = pack(&dir, &host);
        d.activate().unwrap();
        host.take();
        d.run_command(doctor::INSTALL_CSHARPIER).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").filter(|c| c.ends_with("dotnet tool update -g csharpier")).and_then(|c| c.split(' ').next()?.parse::<u64>().ok())).expect("installed as a task");
        host.0.borrow_mut().programs.insert("csharpier".into(), "/h/.dotnet/tools/csharpier".into());
        d.handle(Event::TaskFinished((task, true)));
        assert!(host.take().contains(&"formatter csharp: /h/.dotnet/tools/csharpier format --write-stdout --stdin-path ${file}".to_string()));

        // The project's own, from its manifest, once saved.
        write(&dir, ".config/dotnet-tools.json", "{\"version\":1,\"isRoot\":true,\"tools\":{\"csharpier\":{\"version\":\"1.3.0\",\"commands\":[\"csharpier\"]}}}");
        d.handle(Event::Saved("dotnet-tools.json".into()));
        let calls = host.take();
        assert!(calls.contains(&"formatter csharp: /usr/bin/dotnet csharpier format --write-stdout --stdin-path ${file}".to_string()), "{calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("tasks ") && c.contains("tool restore")));

        // A saved C# file is built, and its problems shown.
        host.0.borrow_mut().here = Some((format!("{}/src/Orbit.Core/Class1.cs", dir.display()), Some(dir.to_string_lossy().into_owned())));
        d.handle(Event::Saved("Class1.cs".into()));
        let build = spawned(&host.take(), "dotnet build src/Orbit.Core/Orbit.Core.csproj");
        said(&mut d, build, &include_str!("samples/build-warn.txt").replace("/home/u/orbit", &dir.to_string_lossy()), 0);
        assert_eq!(host.0.borrow().problems["/project/src/Orbit.Core/Class1.cs"].len(), 2);
        assert!(formatter(&Solution { csharpier: Some("0.30.6".into()), ..Solution::default() }, &Tools { dotnet: Some("dotnet".into()), ..Tools::default() }).is_none(), "CSharpier 0.x has no format");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
