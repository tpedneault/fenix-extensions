//! Zig in Fenix, on lang-kit: the grammar (its language file and
//! queries), ZLS as the language server, told which Zig to use, `zig fmt`
//! as the formatter, `zig ast-check` on save, `zig build`'s steps as
//! tasks, and the project doctor's checks, each with a fix row that
//! installs the release for this machine.
//!
//! What it knows of the project is read from its files ([`project`]),
//! and what's installed is found ([`tools`]). `zig build -l` runs the
//! project's `build.zig` to list its steps, so it runs in the background
//! when the pack starts and when `build.zig` or `build.zig.zon` is saved;
//! until it answers, the tasks are `zig init`'s. A fix that installs a
//! tool, a setting changed, or the Zig found changing sets them again.

pub mod doctor;
pub mod lint;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, MouseEvent, Page};
use fenix_lang_kit::exec::Runs;
use fenix_lang_kit::serde_json::{json, Value};
use fenix_lang_kit::{find_version, Fenix, Host, Kit, Version};

use project::Project;
use tasks::Step;
use tools::{Platform, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language, the grammar's.
const ZIG: &[&str] = &["zig"];

/// Files whose saving changes what the project is.
const PROJECT_FILES: &[&str] = &["build.zig", "build.zig.zon"];

/// What the pack's settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `zig ast-check` on a saved file.
    pub ast_check: bool,
    /// `zig build -l`'s steps as tasks.
    pub build_steps: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { ast_check: true, build_steps: true }
    }
}

impl Settings {
    /// From Fenix: a setting that isn't `false` is on.
    pub fn read() -> Settings {
        let on = |key: &str| !api::setting(key).is_some_and(|v| v.trim().eq_ignore_ascii_case("false"));
        Settings { ast_check: on("zig.ast_check"), build_steps: on("zig.build_steps") }
    }
}

/// What ZLS is told when it asks (`workspace/configuration`, a section
/// `zls.<field>` each): the Zig found, which may be in the tools folder,
/// not on its `PATH`.
pub fn zls_settings(zig: Option<&str>) -> Value {
    match zig {
        Some(zig) => json!({"zls": {"zig_exe_path": zig}}),
        None => json!({"zls": {}}),
    }
}

pub struct Zig {
    pub kit: Kit,
    root: String,
    project: Project,
    tools: Tools,
    platform: Platform,
    settings: Settings,
    read_settings: fn() -> Settings,
    /// `zig build -l` running, and the steps it last listed.
    listing: Runs<()>,
    steps: Option<Vec<Step>>,
    /// The Zig found's version, as the doctor last read it: ZLS is held
    /// to it.
    zig_version: Option<Version>,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_settings: Option<Value>,
    set_formatter: Option<Command>,
}

impl Default for Zig {
    fn default() -> Zig {
        let platform = api::platform();
        Zig::new(Box::new(Fenix), PROJECT, Settings::read, Platform::new(&platform.os, &platform.arch))
    }
}

impl Zig {
    /// The pack for the project in `root`, talking to `host`, on
    /// `platform`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings, platform: Platform) -> Zig {
        let tools = Tools::find(host.as_mut());
        let mut kit = Kit::with_host("zig", host);
        for install in platform.installs() {
            kit.installer.add(install);
        }
        let mut z = Zig {
            kit,
            root: root.into(),
            project: Project::read(root),
            tools,
            platform,
            settings: read_settings(),
            read_settings,
            listing: Runs::new(),
            steps: None,
            zig_version: None,
            set_server: None,
            set_settings: None,
            set_formatter: None,
        };
        z.configure();
        z
    }

    /// The kit's tasks, checks and linter, for the project, tools and
    /// settings as they are.
    fn configure(&mut self) {
        let steps = self.steps.as_deref().filter(|_| self.settings.build_steps);
        self.kit.tasks = tasks::tasks(&self.project, &self.tools, steps, &self.root);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(&self.project, self.zig_version.as_ref(), &self.platform) {
            self.kit.doctor.program(program);
        }
        self.kit.lint.linters.clear();
        if let (true, Some(zig)) = (self.settings.ast_check, &self.tools.zig) {
            self.kit.lint.add(lint::ast_check(&Command::new(zig)));
        }
    }

    /// The server, its settings and the formatter set in Fenix, where they
    /// changed. Nothing found leaves Zig without them.
    fn apply(&mut self) {
        let server = self.tools.zls.as_deref().map(Command::new);
        let formatter = self.tools.zig.as_deref().map(|zig| Command::new(zig).args(["fmt", "--stdin"]));
        let languages: Vec<String> = ZIG.iter().map(|l| l.to_string()).collect();
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
            self.set_settings = None;
        }
        let settings = self.set_server.as_ref().map(|_| zls_settings(self.tools.zig.as_deref()));
        if settings.is_some() && settings != self.set_settings {
            if let Err(why) = host.language_server_settings("zig", settings.as_ref()) {
                host.log(&format!("settings for ZLS: {why}"));
            }
            self.set_settings = settings;
        }
        if formatter != self.set_formatter {
            if let Err(why) = host.formatter(&languages, formatter.as_ref()) {
                host.log(&format!("formatter: {why}"));
            }
            self.set_formatter = formatter;
        }
    }

    /// `zig build -l` started again, for a project with `build.zig`, when
    /// there's Zig and the setting is on.
    fn list_steps(&mut self) {
        let host = self.kit.host.as_mut();
        self.listing.kill_all(host);
        let Some(zig) = self.tools.zig.as_deref().filter(|_| self.project.build && self.settings.build_steps) else { return };
        if let Err(why) = self.listing.start(host, &Command::new(zig).args(["build", "-l"]), ()) {
            host.log(&format!("zig build -l: {why}"));
        }
    }

    /// What `zig build -l` said: its steps as tasks. A `build.zig` that
    /// doesn't build keeps the steps it had, and says why.
    fn listed(&mut self, out: &fenix_lang_kit::Output) {
        let steps = tasks::parse_steps(&out.stdout);
        if !out.ok() || steps.is_empty() {
            self.kit.host.log(&format!("zig build -l: {}", out.why()));
            return;
        }
        self.steps = Some(steps);
        self.configure();
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
        }
    }

    /// The tools as they're found now; set again, and checked again,
    /// where that changed.
    fn find_again(&mut self) {
        let tools = Tools::find(self.kit.host.as_mut());
        if tools != self.tools {
            self.take(tools);
            self.kit.doctor.run(self.kit.host.as_mut());
        }
    }

    /// What the doctor found since: a tool a fix installed is used, and
    /// ZLS is checked again against the Zig it read.
    fn follow(&mut self) {
        let mut tools = self.tools.clone();
        for program in &self.kit.doctor.programs {
            if let Some(slot) = tools.slot(&program.name) {
                *slot = self.kit.doctor.found.get(&program.name).cloned();
            }
        }
        self.take(tools);
        let said = self.kit.doctor.checks().into_iter().find(|c| c.label == "Zig").map(|c| find_version(&c.detail));
        if let Some(version) = said {
            if version != self.zig_version {
                self.zig_version = version;
                self.configure();
                self.kit.doctor.run(self.kit.host.as_mut());
            }
        }
    }

    fn take(&mut self, tools: Tools) {
        if tools != self.tools {
            let zig_changed = tools.zig != self.tools.zig;
            self.tools = tools;
            self.configure();
            if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
                self.kit.host.log(&format!("tasks: {why}"));
            }
            self.apply();
            if zig_changed {
                self.list_steps();
            }
        }
    }

    /// The project's files changed: read again, and checked again.
    fn reload(&mut self) {
        self.project = Project::read(&self.root);
        self.configure();
        let host = self.kit.host.as_mut();
        if let Err(why) = self.kit.tasks.apply(host) {
            host.log(&format!("tasks: {why}"));
        }
        self.kit.doctor.run(host);
        self.apply();
        self.list_steps();
    }
}

impl Extension for Zig {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.list_steps();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            None if id == "zig.steps" => {
                if !self.project.build {
                    return Err("there's no build.zig at the project's top".into());
                }
                if self.tools.zig.is_none() {
                    return Err("Zig isn't installed (or isn't on PATH)".into());
                }
                self.list_steps();
                Ok(())
            }
            None => Err(format!("no command {id} here")),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        if self.listing.owns(&event) {
            if let Some(((), out)) = self.listing.event(&event) {
                self.listed(&out);
            }
            return;
        }
        let name = match &event {
            Event::Saved(name) => Some(name.replace('\\', "/")),
            _ => None,
        };
        // build.zig or its manifest saved; a file at the top, without one.
        let saved = name.as_deref().is_some_and(|n| PROJECT_FILES.contains(&n) || (!self.project.build && !n.contains('/') && n.ends_with(".zig")));
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("zig."));
        let installing = self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        if saved {
            self.reload();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.configure();
            if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
                self.kit.host.log(&format!("tasks: {why}"));
            }
            self.apply();
            self.list_steps();
        }
        let installed = installing && !self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        if installed || task_ended {
            // What a fix put here, or a task (zvm's, zigup's) did.
            self.find_again();
        }
        self.follow();
    }

    fn file_saved(&mut self, path: &str) {
        self.kit.file_saved(path);
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

export!(Zig);

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

    fn found(host: &Shared, programs: &[(&str, &str)]) {
        host.0.borrow_mut().programs.extend(programs.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    }

    fn pack(dir: &std::path::Path, host: &Shared, settings: fn() -> Settings) -> Zig {
        let mut z = Zig::new(Box::new(host.clone()), &dir.to_string_lossy(), settings, Platform::new("linux", "x86_64"));
        z.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
        z
    }

    fn said(z: &mut Zig, process: u64, text: &str, code: i32) {
        for l in text.lines() {
            z.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: l.into() }));
        }
        z.handle(Event::ProcessExit((process, Some(code))));
    }

    /// The processes started, by id, each as a line.
    fn spawned(calls: &[String]) -> Vec<(u64, String)> {
        calls.iter().filter_map(|c| c.strip_prefix("spawn ")?.split_once(' ')).map(|(id, l)| (id.parse().unwrap(), l.to_string())).collect()
    }

    /// A project as `zig init` makes it.
    fn build(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("build.zig"), "const std = @import(\"std\");\n\npub fn build(b: *std.Build) void {\n    _ = b;\n}\n").unwrap();
        std::fs::write(dir.join("build.zig.zon"), include_str!("samples/build.zig.zon")).unwrap();
        std::fs::write(dir.join("src/main.zig"), "pub fn main() !void {}\n").unwrap();
        dir
    }

    #[test]
    fn zls_zig_fmt_and_the_build_steps_are_set() {
        let dir = build("activate");
        let host = Shared::default();
        found(&host, &[("zig", "/t/zig"), ("zls", "/t/zls")]);
        let mut z = pack(&dir, &host, Settings::default);
        z.activate().unwrap();
        let calls = host.take();
        for want in ["tasks build, build test, build run, fmt", "server zig: /t/zls", "formatter zig: /t/zig fmt --stdin"] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert_eq!(host.0.borrow().settings["zig"]["zls"]["zig_exe_path"], "/t/zig", "ZLS told which Zig");

        // The doctor and zig build -l, answered.
        let procs = spawned(&calls);
        let lines: Vec<&str> = procs.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(lines, ["/t/zig version", "/t/zls --version", "/t/zig build -l"]);
        said(&mut z, procs[2].0, include_str!("samples/zig-build-l.txt"), 0);
        assert!(host.take().contains(&"tasks build, build run, build test, build docs, build bench-orbit, fmt".to_string()));
        said(&mut z, procs[0].0, "0.17.0", 0);
        said(&mut z, procs[1].0, "0.16.0", 0);

        // Zig said its version: ZLS is checked again against it.
        let again = spawned(&host.take());
        assert_eq!(again.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(), ["/t/zig version", "/t/zls --version"]);
        said(&mut z, again[0].0, "0.17.0", 0);
        said(&mut z, again[1].0, "0.16.0", 0);
        let checks: Vec<(String, Health, String)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health, c.detail.clone())).collect();
        assert_eq!(checks[0], ("Zig".into(), Health::Ok, "0.17.0 (/t/zig)".into()));
        assert_eq!(checks[1], ("ZLS".into(), Health::Warn, "0.16.0, but Zig is 0.17.0: ZLS goes with the Zig of its own version".into()));
        assert!(spawned(&host.take()).is_empty(), "checked once more, not again and again");

        // A build.zig that doesn't build keeps the steps.
        z.handle(Event::Saved("build.zig".into()));
        let listing = spawned(&host.take()).into_iter().find(|(_, l)| l.ends_with("build -l")).unwrap();
        said(&mut z, listing.0, "build.zig:4:9: error: expected ';' after statement", 1);
        let calls = host.take();
        assert!(calls.iter().any(|c| c.starts_with("log zig build -l: build.zig:4:9")), "{calls:?}");
        assert!(z.kit.tasks.tasks.iter().any(|t| t.name == "build bench-orbit"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_saved_zig_file_is_ast_checked() {
        let dir = build("lint");
        let host = Shared::default();
        found(&host, &[("zig", "/t/zig")]);
        let mut z = pack(&dir, &host, Settings::default);
        z.activate().unwrap();
        host.take();
        host.0.borrow_mut().here = Some((format!("{}/src/main.zig", dir.display()), Some(dir.to_string_lossy().into_owned())));
        z.file_saved(&format!("{}/src/main.zig", dir.display()));
        let check = spawned(&host.take()).into_iter().find(|(_, l)| l == "/t/zig ast-check src/main.zig").expect("ast-check run");
        z.handle(Event::ProcessOutput(api::ProcessOutput { process: check.0, stderr: true, line: "src/main.zig:1:22: error: expected ';' after statement".into() }));
        z.handle(Event::ProcessExit((check.0, Some(1))));
        let problems = host.0.borrow().problems.values().next().cloned().unwrap();
        assert_eq!((problems.len(), problems[0].start), (1, (0, 21)));

        // Off: no linter; and no listing of steps.
        z.read_settings = || Settings { ast_check: false, build_steps: false };
        z.handle(Event::SettingChanged("zig.ast_check".into()));
        assert!(z.kit.lint.linters.is_empty());
        assert!(!spawned(&host.take()).iter().any(|(_, l)| l.ends_with("build -l")));
        assert_eq!(z.kit.tasks.tasks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["build", "build test", "build run", "fmt"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_zig_its_fix_row_installs_it_and_its_used() {
        let dir = temp("fix");
        std::fs::write(dir.join("hello.zig"), "pub fn main() void {}\n").unwrap();
        let host = Shared::default();
        let mut z = pack(&dir, &host, Settings::default);
        z.activate().unwrap();
        let calls = host.take();
        assert!(!calls.iter().any(|c| c.starts_with("server") || c.starts_with("formatter")), "nothing found, nothing set: {calls:?}");
        assert_eq!(z.kit.doctor.programs.iter().map(|p| p.fix.clone().map(|f| f.1)).collect::<Vec<_>>(), [Some("zig.install.zig".into()), Some("zig.install.zls".into())]);
        z.run_command("zig.install.zig").unwrap();
        let download = host.take().into_iter().find(|c| c.starts_with("download ")).unwrap();
        assert!(download.contains("https://files.pythonhosted.org/packages/fe/6f/") && download.ends_with(" 97079827064a7492cce5541ceb438ae8003ce1e54e399676da2d62fb7a5dc08b"), "{download}");
        assert!(z.run_command("zig.steps").is_err(), "no build.zig");
        assert!(z.run_command("zig.nothing").is_err());

        // Installed some other way, by a task: found and used.
        found(&host, &[("zig", "/h/.zvm/bin/zig"), ("zls", "/h/.zvm/bin/zls")]);
        z.handle(Event::TaskFinished((99, true)));
        let calls = host.take();
        for want in ["server zig: /h/.zvm/bin/zls", "formatter zig: /h/.zvm/bin/zig fmt --stdin", "tasks run hello.zig, test hello.zig, fmt"] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert_eq!(z.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["zig ast-check"]);

        // A new file at the top: its tasks.
        std::fs::write(dir.join("orbit.zig"), "test \"x\" {}\n").unwrap();
        z.handle(Event::Saved("orbit.zig".into()));
        assert!(z.kit.tasks.tasks.iter().any(|t| t.name == "test orbit.zig"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
