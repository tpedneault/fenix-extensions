//! Scala in Fenix, on lang-kit: the grammar (its language file and
//! queries), Metals as the language server -- found where Coursier puts it,
//! or installed through Coursier -- run by the pack, so that it can answer
//! what Metals asks of its client and start its debug adapter; tasks for
//! sbt, scala-cli, Mill, Gradle and Maven; a REPL, a build watching its
//! sources and the file you're in run by scala-cli; and the project
//! doctor's checks. Metals formats, with the scalafmt `.scalafmt.conf` says,
//! and compiles: its diagnostics are the compiler's.
//!
//! What it knows of the project is read from its files ([`project`]), and
//! what's installed is found ([`tools`]). A fix that installs a tool, a
//! setting changed, or a build file saved sets everything again.

pub mod doctor;
pub mod lsp;
pub mod project;
pub mod tasks;
pub mod tools;
pub mod trust;

#[cfg(test)]
mod data;

#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

use api::{export, Command, Event, Extension, Key, MouseEvent, Page, PickerItem};
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{Fenix, Host, Kit};

use lsp::{Ask, Launch, Out, Proxy};
use doctor::Allowed;
use project::Project;
use trust::Decision;
use tools::{Platform, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language Metals serves.
pub(crate) const SERVED: &[&str] = &["scala"];

/// The files that, saved, make the pack read the project again.
pub const BUILD_FILES: &[&str] = &[
    "build.sbt", "build.properties", "plugins.sbt", "build.sc", "build.mill", "build.mill.scala", "project.scala", ".scalafmt.conf", ".scalafix.conf", "pom.xml", "build.gradle", "build.gradle.kts",
];

/// What the pack's settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `scala.jdk`: the JDK you chose, its home given to Metals and to the
    /// build's tasks as `JAVA_HOME`.
    pub jdk: Option<String>,
    /// `scala.auto_import`: `all`, `initial` or `off` -- Metals' own
    /// `autoImportBuilds`.
    pub auto_import: String,
    /// `scala.run_metals`: `always` runs Metals, importing the build, in every
    /// project without asking -- for someone who opens only their own code. Yours
    /// only: no project's `.fenix/settings.toml` can set it.
    pub always: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { jdk: None, auto_import: "all".into(), always: false }
    }
}

impl Settings {
    /// From Fenix: no JDK when the setting is empty, and an `auto_import`
    /// that isn't one Metals knows is `all`.
    pub fn read() -> Settings {
        let jdk = api::setting("scala.jdk").map(|j| j.trim().to_string()).filter(|j| !j.is_empty());
        let auto_import = api::setting("scala.auto_import").map(|a| a.trim().to_string()).filter(|a| ["all", "initial", "off"].contains(&a.as_str())).unwrap_or_else(|| "all".into());
        let always = api::setting("scala.run_metals").is_some_and(|v| v.trim() == "always");
        Settings { jdk, auto_import, always }
    }

    /// The chosen JDK's home: what the setting names -- `java`, its `bin`
    /// folder or the home itself -- as the home.
    pub fn jdk_home(&self) -> Option<String> {
        let path = self.jdk.as_deref()?.replace('\\', "/");
        let path = path.trim_end_matches('/');
        let file = path.rsplit('/').next().unwrap_or(path);
        let dir = if ["java", "javac", "java.exe", "javac.exe"].contains(&file) { path.rsplit_once('/').map(|d| d.0).unwrap_or(path) } else { path };
        Some(dir.strip_suffix("/bin").unwrap_or(dir).to_string())
    }
}

/// The keys that run `command` now, as the key scheme in use writes them
/// (`SPC u u`, or `C-k u u` with standard keys, or yours); `literal` in
/// Vim's when none does.
fn keys(command: &str, literal: &str) -> String {
    api::key_for(command).unwrap_or_else(|| literal.to_string())
}

/// Why Metals can't start without it.
fn metals_missing(has_coursier: bool, doctor: &str) -> String {
    match has_coursier {
        true => format!("Metals isn't installed: the project doctor ({doctor}) installs it, with Coursier"),
        false => format!("Metals isn't installed: the project doctor ({doctor}) installs Coursier, and then Metals"),
    }
}

pub struct Scala {
    pub kit: Kit,
    root: String,
    project: Project,
    tools: Tools,
    platform: Platform,
    settings: Settings,
    read_settings: fn() -> Settings,
    /// Metals, run by the pack.
    pub proxy: Proxy,
    /// `cs install metals`, as a task, while it runs.
    installing_metals: Option<u64>,
    /// The build run again as sources change, while it runs.
    watching: Option<u64>,
    /// The formatter set in Fenix, so it's set again only when it changes.
    set_formatter: Option<Command>,
    /// The pack's own data folder, where what you said of each project is kept.
    pub data: String,
    /// The project's full path, from Fenix's `initialize`.
    project_path: Option<String>,
    /// The question about a project's build, while it's open, and the paths
    /// asked this session.
    asking: Option<u64>,
    asked: Vec<String>,
}

impl Default for Scala {
    fn default() -> Scala {
        let platform = api::platform();
        Scala::new(Box::new(Fenix), PROJECT, Settings::read, Platform::new(&platform.os, &platform.arch))
    }
}

impl Scala {
    /// The pack for the project in `root`, talking to `host`, on
    /// `platform`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings, platform: Platform) -> Scala {
        let tools = Tools::find(host.as_mut());
        let mut kit = Kit::with_host("scala", host);
        for install in platform.installs() {
            kit.installer.add(install);
        }
        let mut s = Scala {
            kit,
            root: root.into(),
            project: Project::read(root),
            tools,
            platform,
            settings: read_settings(),
            read_settings,
            proxy: Proxy::default(),
            installing_metals: None,
            watching: None,
            set_formatter: None,
            data: "/data".into(),
            project_path: None,
            asking: None,
            asked: Vec::new(),
        };
        s.configure();
        s
    }

    /// The kit's tasks and checks, for the project, tools and settings as
    /// they are.
    fn configure(&mut self) {
        self.kit.tasks = tasks::tasks(&self.project, &self.tools, &self.root, self.settings.jdk_home().as_deref());
        self.kit.doctor.programs.clear();
        for program in doctor::programs(&self.project, &self.tools, &self.platform, self.settings.jdk.as_deref()) {
            self.kit.doctor.program(program);
        }
        let native = self.kit.installer.installed("scalafmt");
        let mut extra: Vec<api::Check> = doctor::formatting(&self.project, native, self.platform.scalafmt().is_some()).into_iter().collect();
        extra.extend(doctor::wrapper_checks(&self.project, &self.tools));
        extra.extend(doctor::build_check(&self.project, self.allowed()));
        self.kit.doctor.extra = extra;
    }

    /// The tasks set again.
    fn set_tasks(&mut self) {
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
        }
    }

    /// The native scalafmt, as the formatter on save: `editor.format_on_save`
    /// runs only an extension's formatter, and a native scalafmt formats only
    /// with the version it is, so there's one when the project's
    /// `.scalafmt.conf` pins that version and the pack has installed it. Metals
    /// formats the rest (the format command, any version).
    fn formatter(&self) -> Option<Command> {
        let pinned = self.project.scalafmt.as_ref().and_then(|s| s.version.as_deref()) == Some(tools::SCALAFMT);
        let install = self.kit.installer.get("scalafmt").filter(|_| pinned && self.kit.installer.installed("scalafmt"))?;
        Some(install.run_in("/tools").args(["--stdin", "--assume-filename", "${file}"]))
    }

    /// The formatter set in Fenix, where it changed.
    fn apply_formatter(&mut self) {
        let want = self.formatter();
        if want != self.set_formatter {
            let languages: Vec<String> = SERVED.iter().map(|l| l.to_string()).collect();
            if let Err(why) = self.kit.host.formatter(&languages, want.as_ref()) {
                self.kit.host.log(&format!("formatter: {why}"));
            }
            self.set_formatter = want;
            // The doctor's row for scalafmt says whether it's installed.
            self.configure();
            self.kit.doctor.run(self.kit.host.as_mut());
        }
    }

    /// Whether Metals may run in this project: you said Import, or `scala.run_metals`
    /// is `always`.
    fn allowed(&self) -> Allowed {
        if self.settings.always {
            return Allowed::Yes;
        }
        match self.project_path.as_deref().and_then(|p| trust::get(&self.data, p)) {
            Some(Decision::Allow) => Allowed::Yes,
            Some(Decision::Never) => Allowed::Never,
            None => Allowed::Unasked,
        }
    }

    /// What's said of the project, and Metals started or stopped for it.
    fn decide(&mut self, decision: Option<Decision>) -> Result<(), String> {
        let path = self.project_path.clone().or_else(|| self.kit.host.here().and_then(|h| h.1)).ok_or("open a Scala file first: the project isn't known yet")?;
        trust::set(&self.data, &path, decision)?;
        self.project_path = Some(path);
        self.configure();
        self.kit.doctor.run(self.kit.host.as_mut());
        self.restart_server();
        Ok(())
    }

    /// The question: Metals isn't started until it's answered Import.
    fn ask_allow(&mut self) {
        let Some(path) = self.project_path.clone().filter(|p| !self.asked.contains(p)) else { return };
        self.asked.push(path);
        let items = [
            PickerItem::new("Import", "import").detail("Run Metals here and import the build: its code runs"),
            PickerItem::new("Not now", "later").detail("No Metals in this project for now; asked again next time"),
            PickerItem::new("Never for this project", "never").detail("Scala: forget what's decided about this project undoes it"),
        ];
        self.asking = Some(self.kit.host.pick("Import this project's build? This runs its build definition (build.sbt / build.mill / ...), which is code from this project.", &items));
    }

    /// How Metals starts, or why it can't.
    fn launch(&self) -> Result<Launch, String> {
        let Some(metals) = &self.tools.metals else {
            return Err(metals_missing(self.tools.coursier.is_some(), &keys("project.doctor", "SPC p h")));
        };
        match self.allowed() {
            Allowed::Yes => {}
            Allowed::Unasked => return Err("Metals isn't started in this project until you allow it: it connects to the project's build, which runs the build's own code. Say Import (the project doctor, or Scala: allow Metals in this project)".into()),
            Allowed::Never => return Err("Metals isn't started in this project: you said Never (Scala: forget what's decided about this project undoes it)".into()),
        }
        Ok(Launch { command: Command::new(metals), auto_import: self.settings.auto_import.clone(), jdk_home: self.settings.jdk_home() })
    }

    /// Fenix's servers started again: Metals, once there's one, or with
    /// what it's told changed.
    fn restart_server(&mut self) {
        api::project::restart_language_server();
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

    /// What the doctor found since: a tool a fix installed is used.
    fn follow(&mut self) {
        let mut tools = self.tools.clone();
        for program in &self.kit.doctor.programs {
            // A project's wrapper was found and not asked: it stays what runs the tasks.
            if tools.is_wrapper(&program.name) {
                continue;
            }
            if let Some(slot) = tools.slot(&program.name) {
                *slot = self.kit.doctor.found.get(&program.name).cloned();
            }
        }
        self.take(tools);
        self.apply_formatter();
    }

    fn take(&mut self, tools: Tools) {
        if tools != self.tools {
            let metals_arrived = self.tools.metals.is_none() && tools.metals.is_some();
            self.tools = tools;
            self.configure();
            self.set_tasks();
            if metals_arrived && self.proxy.missing.is_some() {
                self.restart_server();
            }
        }
    }

    /// The project's files changed: read again, and checked again.
    fn reload(&mut self) {
        self.project = Project::read(&self.root);
        self.configure();
        self.set_tasks();
        self.kit.doctor.run(self.kit.host.as_mut());
    }

    fn act(&mut self, outs: Vec<Out>) {
        for o in outs {
            match o {
                Out::Spawn(cmd) => match api::host::spawn_raw(&cmd) {
                    Ok(p) => self.proxy.started(p),
                    Err(why) => {
                        self.kit.host.notify(&format!("Metals didn't start: {why}"));
                        let told = self.proxy.failed(&why);
                        self.act(told);
                    }
                },
                Out::Kill(p) => api::process::kill(p),
                Out::Write(bytes) => {
                    if let Some(p) = self.proxy.process {
                        if let Err(why) = api::process::write(p, &bytes) {
                            self.kit.host.log(&format!("metals: {why}"));
                        }
                    }
                }
                Out::Send(server, text) => api::host::lsp_send(server, &text),
                Out::Answer(ask, answer) => self.answered(ask, answer),
            }
        }
    }

    fn answered(&mut self, ask: Ask, answer: Result<Value, String>) {
        match (ask, answer) {
            (Ask::Debug { .. }, Ok(v)) => match lsp::debug_session(&v) {
                Some((name, port)) => match self.kit.host.debug_adapter_connect(&[SERVED[0].to_string()], None, port) {
                    Ok(()) => {
                        let start = keys("debug.start_or_continue", "SPC u u");
                        self.kit.host.notify(&format!("{name}: Metals is waiting for the debugger. {start}, then Scala: Metals' debug session"));
                    }
                    Err(why) => self.kit.host.log(&format!("debug adapter: {why}")),
                },
                None => self.kit.host.notify("Metals didn't say where its debug adapter is"),
            },
            (Ask::Import, Ok(_)) => self.kit.host.notify("Metals: importing the build"),
            (_, Err(why)) => self.kit.host.notify(&format!("Metals: {why}")),
        }
    }

    /// The file you're in, when it's one of Scala's.
    fn here(&mut self) -> Result<String, String> {
        let (path, _) = self.kit.host.here().ok_or("no file is open")?;
        if tasks::is_source(&path) {
            Ok(path)
        } else {
            Err(format!("{path} isn't a .scala or .sc file"))
        }
    }

    fn debug(&mut self, run_type: &str) -> Result<(), String> {
        let file = self.here()?;
        let outs = self.proxy.ask(Ask::Debug { file, run_type: run_type.into() })?;
        self.act(outs);
        Ok(())
    }

    fn own_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            doctor::INSTALL_METALS => {
                let cs = self.tools.coursier.clone().ok_or("Coursier isn't installed: install it first (its row in the project doctor)")?;
                let task = self.kit.host.run_task(&format!("Install Metals {}", tools::METALS), &tools::install_metals(&cs))?;
                self.installing_metals = Some(task);
                Ok(())
            }
            "scala.console" => {
                let (title, cmd) = tasks::console(&self.project, &self.tools).ok_or("there's no REPL for this project's build tool")?;
                api::host::run_in_terminal(&cmd.cwd(PROJECT), &title, false).map(|_| ())
            }
            "scala.watch" => {
                if let Some(task) = self.watching.take() {
                    self.kit.host.task_cancel(task);
                    self.kit.host.notify("Stopped watching");
                    return Ok(());
                }
                let (name, cmd) = tasks::watch(&self.project, &self.tools).ok_or("there's no watching build for this project's build tool")?;
                let task = self.kit.host.run_task_background(&name, &cmd.cwd(PROJECT))?;
                self.watching = Some(task);
                self.kit.host.notify(&format!("{name}: running in the background; the same command stops it"));
                Ok(())
            }
            "scala.run-file" | "scala.test-file" => {
                let verb = if id == "scala.run-file" { "run" } else { "test" };
                let file = self.here()?;
                if self.tools.scala_cli.is_none() {
                    return Err("scala-cli isn't installed (or isn't on PATH): `cs install scala-cli`".into());
                }
                let cmd = tasks::run_file(&self.tools, verb, &file);
                let name = format!("scala-cli {verb} {}", file.rsplit(['/', '\\']).next().unwrap_or(&file));
                self.kit.host.run_task(&name, &cmd).map(|_| ())
            }
            doctor::ALLOW => self.decide(Some(Decision::Allow)),
            "scala.forget-project" => {
                self.asked.clear();
                self.decide(None)
            }
            "scala.import-build" => {
                let outs = self.proxy.ask(Ask::Import)?;
                self.act(outs);
                Ok(())
            }
            "scala.debug-file" => self.debug("runOrTestFile"),
            "scala.debug-tests" => self.debug("testFile"),
            _ => Err(format!("no command {id} here")),
        }
    }
}

impl Extension for Scala {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            None => self.own_command(id),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        match &event {
            Event::Picked((id, value)) if self.asking == Some(*id) => {
                self.asking = None;
                let said = match value.as_deref() {
                    Some("import") => Some(Decision::Allow),
                    Some("never") => Some(Decision::Never),
                    _ => None,
                };
                if let Some(d) = said {
                    if let Err(why) = self.decide(Some(d)) {
                        self.kit.host.notify(&format!("Scala: {why}"));
                    }
                }
                return;
            }
            Event::ProcessBytes((p, bytes)) if self.proxy.process == Some(*p) => {
                let outs = self.proxy.from_metals(bytes);
                self.act(outs);
                return;
            }
            // Metals' stderr: its own log, not the pack's.
            Event::ProcessOutput(o) if self.proxy.process == Some(o.process) => return,
            Event::ProcessExit((p, code)) if self.proxy.process == Some(*p) => {
                let outs = self.proxy.exited();
                self.act(outs);
                self.kit.host.notify(&format!("Metals stopped ({})", code.map(|c| c.to_string()).unwrap_or_else(|| "killed".into())));
                return;
            }
            _ => {}
        }
        // A source saved in a folder that wasn't a Scala project's, or a build
        // file: read again.
        let reread = matches!(&event, Event::Saved(name) if {
            let name = name.replace('\\', "/");
            let file = name.rsplit('/').next().unwrap_or(&name);
            BUILD_FILES.contains(&file) || (!self.project.is_scala() && tasks::is_source(file))
        });
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("scala."));
        let installing = self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        let task_ended = match &event {
            Event::TaskFinished((id, _)) => Some(*id),
            _ => None,
        };
        self.kit.handle(&event);
        if reread {
            self.reload();
        }
        if setting {
            let old = self.settings.clone();
            self.settings = (self.read_settings)();
            self.configure();
            self.set_tasks();
            self.kit.doctor.run(self.kit.host.as_mut());
            // What Metals is started with changed: it starts again.
            if self.settings != old && self.proxy.process.is_some() {
                self.restart_server();
            }
        }
        let installed = installing && !self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        if let Some(id) = task_ended {
            if self.watching == Some(id) {
                self.watching = None;
            }
            if self.installing_metals == Some(id) {
                self.installing_metals = None;
            }
        }
        if installed || task_ended.is_some() {
            // What a fix put here, or a task did.
            self.find_again();
        }
        self.follow();
    }

    fn lsp_message(&mut self, server: u64, message: &str) {
        if let Some(root) = lsp::initialize_root(message) {
            if self.project_path.as_deref() != Some(&root) {
                self.project_path = Some(root);
                self.configure();
                self.kit.doctor.run(self.kit.host.as_mut());
            }
        }
        let launch = self.launch();
        if lsp::initialize_root(message).is_some() && self.tools.metals.is_some() && self.allowed() == Allowed::Unasked {
            self.ask_allow();
        }
        let outs = self.proxy.from_fenix(server, message, launch.as_ref().map_err(String::as_str));
        self.act(outs);
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

export!(Scala);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, write};
    use api::Health;
    use fenix_test_kit::{Fake, Harness, Ran};

    /// The pack for the project in `dir`, on `fake`, its tools folder in
    /// `dir` too.
    fn pack(dir: &std::path::Path, fake: &Fake, settings: fn() -> Settings) -> Harness<Scala> {
        let mut s = Scala::new(fake.host(), &dir.to_string_lossy(), settings, Platform::new("linux", "x86_64"));
        s.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
        s.data = dir.join(".data").to_string_lossy().into_owned();
        Harness::wrap(fake.clone(), s)
    }

    /// The processes started, as lines, in order.
    fn spawned(fake: &Fake) -> Vec<String> {
        fake.spawned().into_iter().map(|(_, l)| l).collect()
    }

    /// An sbt project with a scalafmt configuration.
    fn sbt(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        write(&dir, "build.sbt", include_str!("samples/build.sbt"));
        write(&dir, "project/build.properties", "sbt.version=1.12.15\n");
        write(&dir, ".scalafmt.conf", "version = \"3.8.3\"\nrunner.dialect = scala3\n");
        dir
    }

    /// A JDK, Metals, sbt and scala-cli installed, saying their versions as
    /// they printed them.
    fn installed() -> Fake {
        let fake = Fake::new();
        fake.program("javac", "/usr/lib/jvm/java-21/bin/javac")
            .program("cs", "/t/cs")
            .program("metals", "/t/metals")
            .program("sbt", "/t/sbt")
            .program("scala-cli", "/t/scala-cli")
            .on_spawn("javac -version", Ran::ok(include_str!("samples/javac-version.txt")))
            .on_spawn("cs version", Ran::ok(include_str!("samples/cs-version.txt")))
            .on_spawn("metals --version", Ran::ok(include_str!("samples/metals-version.txt")))
            .on_spawn("sbt --version", Ran::ok(include_str!("samples/sbt-version.txt")))
            .on_spawn("scala-cli version", Ran::ok(include_str!("samples/scala-cli-version.txt")));
        fake
    }

    /// Settings that run Metals without asking, for the tests of what Metals does.
    fn always() -> Settings {
        Settings { always: true, ..Settings::default() }
    }

    fn init(root: &str) -> String {
        format!(r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"processId":7,"rootUri":"file://{root}","capabilities":{{}}}}}}"#)
    }

    #[test]
    fn the_tasks_and_the_checks_are_set_and_metals_is_left_to_the_pack_to_serve() {
        let dir = sbt("activate");
        let fake = installed();
        let mut s = pack(&dir, &fake, Settings::default);
        s.activate().unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c == "tasks compile, test, run, clean, run core, test core, run cli, test cli, run docs, test docs"), "{calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("server") || c.starts_with("formatter")), "Metals is the pack's, and formats through the language server: {calls:?}");
        for want in ["/usr/lib/jvm/java-21/bin/javac -version", "/t/metals --version"] {
            assert!(spawned(&fake).iter().any(|l| l == want), "{want}: {:?}", spawned(&fake));
        }
        assert!(!spawned(&fake).iter().any(|l| l.starts_with("/t/sbt")), "sbt reads the project's own files: found, not run");
        assert!(!spawned(&fake).iter().any(|l| l.starts_with("/t/cs") || l.starts_with("/t/scala-cli")), "Coursier isn't asked when Metals is there; scala-cli isn't this project's");
        let checks: Vec<(String, Health, String)> = fake.checks().iter().map(|c| (c.label.clone(), c.health, c.detail.clone())).collect();
        assert_eq!(
            checks,
            [
                ("JDK".into(), Health::Ok, "21.0.12.1 (/usr/lib/jvm/java-21/bin/javac)".into()),
                ("Metals".into(), Health::Ok, "1.6.9 (/t/metals)".into()),
                ("sbt".into(), Health::Ok, "/t/sbt".into()),
                ("scalafmt".into(), Health::Ok, "3.8.3, dialect scala3 (.scalafmt.conf): formatted by Metals, which runs that version; on save only 3.11.5's native scalafmt can".into()),
                ("Metals and the build".into(), Health::Warn, "Metals isn't started until you allow it: it connects to this project's build and imports it, which runs the build's own code (build.sbt, build.mill, ...)".into()),
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The fix rows: Coursier from its release, then Metals through it.
    #[test]
    fn without_metals_the_doctor_offers_coursier_and_then_metals() {
        let dir = sbt("fix");
        let fake = Fake::new();
        fake.program("javac", "/usr/lib/jvm/java-21/bin/javac").on_spawn("javac -version", Ran::ok("javac 21.0.12.1\n"));
        let mut s = pack(&dir, &fake, Settings::default);
        s.activate().unwrap();
        let rows: Vec<(String, Health, Option<String>)> = fake.checks().iter().map(|c| (c.label.clone(), c.health, c.fix.as_ref().map(|f| f.1.clone()))).collect();
        assert_eq!(
            rows,
            [
                ("JDK".to_string(), Health::Ok, None),
                ("Coursier".into(), Health::Warn, Some("scala.install.coursier".into())),
                ("Metals".into(), Health::Warn, None),
                ("sbt".into(), Health::Bad, None),
                ("scalafmt".into(), Health::Ok, None),
                ("Metals and the build".into(), Health::Warn, Some("scala.allow-project".into())),
            ]
        );
        assert_eq!(s.command("scala.install-metals"), Err("Coursier isn't installed: install it first (its row in the project doctor)".into()));

        // The install plan: the release for this machine, checked by its SHA-256.
        s.command("scala.install.coursier").unwrap();
        let download = fake.take().into_iter().find(|c| c.starts_with("download ")).unwrap();
        assert!(download.contains("https://github.com/coursier/coursier/releases/download/v2.1.26/cs-x86_64-pc-linux.gz") && download.ends_with(" 348e37bc2a8c706640e6b032c4551a0e055a7f1537485b4a91540e9d3598ec6d"), "{download}");
        assert!(s.command("scala.nothing").is_err());

        // Installed: found (the pack looks in its own folder), and Metals has a fix row.
        fake.program("cs", "/tools/coursier/cs").on_spawn("cs version", Ran::ok("2.1.26\n"));
        s.event(Event::TaskFinished((99, true)));
        let again: Vec<(String, Option<String>)> = fake.checks().iter().map(|c| (c.label.clone(), c.fix.as_ref().map(|f| f.1.clone()))).collect();
        assert!(again.contains(&("Metals".to_string(), Some("scala.install-metals".to_string()))), "{again:?}");
        assert!(again.contains(&("Coursier".to_string(), None)), "Coursier is fine now: {again:?}");

        // Metals through it, as a task you can watch.
        fake.take();
        s.command("scala.install-metals").unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.ends_with("Install Metals 1.6.9: /tools/coursier/cs install --install-dir ${tools}/metals metals:1.6.9") && c.starts_with("task ")), "{calls:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn coursier_is_installed_from_the_release_for_each_platform() {
        for (os, arch, file, sha) in [
            ("linux", "x86_64", "cs-x86_64-pc-linux.gz", "348e37bc"),
            ("linux", "aarch64", "cs-aarch64-pc-linux.gz", "6c358ac2"),
            ("macos", "x86_64", "cs-x86_64-apple-darwin.gz", "c8045f36"),
            ("macos", "aarch64", "cs-aarch64-apple-darwin.gz", "8e4d36aa"),
            ("windows", "x86_64", "cs-x86_64-pc-win32.zip", "d23f4ba6"),
        ] {
            let dir = temp(&format!("plan-{os}-{arch}"));
            let fake = Fake::new();
            let mut s = Scala::new(fake.host(), &dir.to_string_lossy(), Settings::default, Platform::new(os, arch));
            s.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
            let mut s = Harness::wrap(fake.clone(), s);
            s.command("scala.install.coursier").unwrap();
            let download = fake.take().into_iter().find(|c| c.starts_with("download ")).unwrap_or_else(|| panic!("{os} {arch}"));
            assert!(download.contains(&format!("/v2.1.26/{file}")) && download.contains(&format!(" {sha}")) || download.split_whitespace().last().is_some_and(|h| h.starts_with(sha)), "{os} {arch}: {download}");
            std::fs::remove_dir_all(&dir).unwrap();
        }
        // None for Windows on ARM: no row, no command.
        let dir = temp("plan-windows-arm");
        let fake = Fake::new();
        let mut s = Harness::wrap(fake.clone(), Scala::new(fake.host(), &dir.to_string_lossy(), Settings::default, Platform::new("windows", "aarch64")));
        assert!(s.command("scala.install.coursier").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Metals through the pack: started on `initialize`, told what its client
    /// is, answered where Fenix can't, and Fenix given the rest -- with what
    /// Metals really said (src/samples/metals-stdout.bin).
    #[test]
    fn metals_is_run_by_the_pack_and_its_questions_answered() {
        let dir = sbt("serve");
        let fake = installed();
        let raw = include_bytes!("samples/metals-stdout.bin");
        fake.on_spawn("/t/metals", Ran::running("").raw(raw.to_vec()));
        let mut s = pack(&dir, &fake, || Settings { jdk: Some("/opt/jdk21/bin/java".into()), auto_import: "initial".into(), always: true });
        s.activate().unwrap();
        fake.take();

        s.ext.lsp_message(4, &init("/home/u/orbit"));
        s.settle();
        let spawn = spawned(&fake);
        assert_eq!(spawn.last().unwrap(), "/t/metals", "{spawn:?}");
        let process = fake.process("/t/metals").unwrap();
        assert!(process.raw);
        assert_eq!(process.cmd.env, [("JAVA_HOME".to_string(), "/opt/jdk21".to_string())], "the JDK you chose");
        let stdin = String::from_utf8_lossy(&process.stdin).into_owned();
        assert!(stdin.contains("\"method\":\"initialize\"") && stdin.contains("\"isHttpEnabled\":false") && stdin.contains("\"debuggingProvider\":true"), "{stdin}");
        // What Metals asked of its client, answered: its configuration, its questions.
        assert!(stdin.contains(r#""result":[{"autoImportBuilds":"initial","javaHome":"/opt/jdk21"}]"#), "{stdin}");
        assert!(stdin.contains(r#""result":{"title":"Run anyway"}"#) && stdin.contains(r#""result":{"title":"Not now"}"#), "{stdin}");
        // And the rest is Fenix's.
        let calls = fake.take();
        let told: Vec<&String> = calls.iter().filter(|c| c.starts_with("lsp 4 ")).collect();
        assert!(told.iter().any(|c| c.contains("\"documentFormattingProvider\":true")), "initialize's answer");
        assert!(told.iter().any(|c| c.contains("textDocument/publishDiagnostics")));
        assert!(!told.iter().any(|c| c.contains("workspace/configuration") || c.contains("Run anyway")));

        // What Fenix sends goes through.
        s.ext.lsp_message(4, r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{}}"#);
        assert!(String::from_utf8_lossy(&fake.process("/t/metals").unwrap().stdin).contains("textDocument/didOpen"));
        // Metals going away is said.
        let p = fake.process("/t/metals").unwrap().id;
        s.event(Event::ProcessExit((p, Some(1))));
        assert!(fake.notifications().iter().any(|n| n == "Metals stopped (1)"), "{:?}", fake.notifications());
        assert_eq!(s.ext.proxy.process, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_metals_fenix_is_answered_and_installing_it_starts_the_server_again() {
        let dir = sbt("no-server");
        let fake = Fake::new();
        let mut s = pack(&dir, &fake, always);
        s.activate().unwrap();
        fake.take();
        s.ext.lsp_message(4, &init("/home/u/orbit"));
        let told = fake.take();
        assert!(told.iter().any(|c| c.starts_with("lsp 4 ") && c.contains("\"capabilities\":{}") && c.contains("Metals (not running)")), "{told:?}");
        s.ext.lsp_message(4, r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{}}"#);
        let told = fake.take();
        assert!(told.iter().any(|c| c.starts_with("lsp 4 ") && c.contains("Metals isn't installed: the project doctor (SPC p h) installs Coursier, and then Metals")), "{told:?}");
        // With Coursier, the message says Metals is what to install.
        fake.program("cs", "/t/cs");
        s.event(Event::TaskFinished((5, true)));
        s.ext.lsp_message(4, r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/hover","params":{}}"#);
        assert!(fake.take().iter().any(|c| c.starts_with("lsp 4 ") && c.contains("the project doctor (SPC p h) installs it, with Coursier")));

        // Metals arrives (a fix's task ended): Fenix starts the server again.
        fake.program("metals", "/t/metals").on_spawn("metals --version", Ran::ok("metals 1.6.9\n"));
        s.event(Event::TaskFinished((6, true)));
        assert!(fake.take().iter().any(|c| c == "server-restart"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_metals_that_cant_be_started_is_said_and_fenix_isnt_left_waiting() {
        let dir = sbt("no-start");
        let fake = installed();
        fake.on_spawn_error("/t/metals", "permission denied");
        let mut s = pack(&dir, &fake, always);
        s.activate().unwrap();
        fake.take();
        s.ext.lsp_message(4, &init("/home/u/orbit"));
        let calls = fake.take();
        assert!(fake.notifications().iter().any(|n| n == "Metals didn't start: permission denied"), "{:?}", fake.notifications());
        assert!(calls.iter().any(|c| c.starts_with("lsp 4 ") && c.contains("\"id\":1") && c.contains("permission denied")), "{calls:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A project that's just been cloned: its wrappers, its build, its `.bloop` and
    /// `.bsp` are its own code, and nothing of them runs, or is imported, before you say so.
    fn cloned(name: &str) -> std::path::PathBuf {
        let dir = sbt(name);
        for file in ["mill", "sbt", "gradlew", "mvnw", "millw"] {
            write(&dir, file, "#!/bin/sh\necho ran > ran-$0\n");
        }
        write(&dir, "build.mill", include_str!("samples/build.mill"));
        write(&dir, "project.scala", "//> using scala 3.7.3\n");
        write(&dir, ".bloop/orbit.json", "{}");
        write(&dir, ".bsp/scala-cli.json", "{\"argv\": [\"/tmp/evil\", \"bsp\"]}");
        write(&dir, ".fenix/settings.toml", "[scala]\nrun_metals = \"always\"\njdk = \"/tmp/evil-jdk\"\nauto_import = \"all\"\n");
        dir
    }

    fn project_files_found(fake: &Fake, dir: &std::path::Path) {
        let d = dir.display();
        fake.program("javac", "/usr/lib/jvm/java-21/bin/javac").program("metals", "/t/metals").program("cs", "/t/cs").program("scala-cli", "/t/scala-cli");
        // What the project has of its own, as finding a program there says it.
        fake.program("mill", &format!("{d}/mill")).program("sbt", &format!("{d}/sbt")).program("gradle", &format!("{d}/gradlew")).program("mvn", &format!("{d}/mvnw"));
        fake.on_spawn("javac -version", Ran::ok("javac 21.0.12.1\n")).on_spawn("metals --version", Ran::ok("metals 1.6.9\n")).on_spawn("cs version", Ran::ok("2.1.26\n")).on_spawn("scala-cli version", Ran::ok("Scala CLI version: 1.19.0\n"));
    }

    #[test]
    fn opening_focusing_and_saving_in_a_cloned_project_runs_nothing_of_its_own_and_imports_nothing() {
        let dir = cloned("cloned");
        let fake = Fake::new();
        project_files_found(&fake, &dir);
        let mut s = pack(&dir, &fake, Settings::default);
        s.activate().unwrap();
        let root = "/home/u/cloned";
        s.ext.lsp_message(4, &init(root));
        s.settle();
        s.event(Event::FocusChanged(Some(format!("{}/src/Main.scala", dir.display()))));
        s.event(Event::Saved("build.sbt".into()));
        s.event(Event::Saved("src/Main.scala".into()));
        s.event(Event::TaskFinished((3, true)));
        s.ext.lsp_message(4, r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{}}"#);
        s.ext.lsp_message(4, r#"{"jsonrpc":"2.0","id":9,"method":"textDocument/hover","params":{}}"#);

        let ran = spawned(&fake);
        assert!(!ran.iter().any(|l| l.starts_with(&dir.to_string_lossy().to_string())), "a file of the project's ran: {ran:?}");
        assert!(ran.iter().all(|l| ["javac", "/usr/lib/jvm/java-21/bin/javac", "/t/metals --version", "/t/cs", "/t/scala-cli"].iter().any(|ok| l.starts_with(ok))), "{ran:?}");
        let calls = fake.calls();
        assert!(!calls.iter().any(|c| c.starts_with("spawn-raw")), "Metals runs the build's own code when it connects: {calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("background") || c.starts_with("task ") || c.starts_with("terminal")), "{calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("pick ") && c.contains("Import this project's build? This runs its build definition (build.sbt / build.mill / ...), which is code from this project.: Import, Not now, Never for this project")), "{calls:?}");
        // Fenix is answered, and told why there's no Metals; the project's own settings file changed nothing.
        assert!(calls.iter().any(|c| c.starts_with("lsp 4 ") && c.contains("Metals isn't started in this project until you allow it")), "{calls:?}");
        assert_eq!(fake.calls().iter().filter(|c| c.starts_with("pick ")).count(), 1, "asked once");
        let row = fake.checks().into_iter().find(|c| c.label == "Metals and the build").unwrap();
        assert_eq!((row.health, row.fix.map(|f| f.1)), (Health::Warn, Some("scala.allow-project".into())));
        assert!(!dir.join(".data").exists(), "nothing was remembered");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Import is kept for that project's path only; Never is kept and undone by a command.
    #[test]
    fn what_you_say_is_kept_for_that_path_and_never_can_be_undone() {
        let dir = cloned("said");
        let fake = Fake::new();
        fake.on_spawn("/t/metals", Ran::running(""));
        project_files_found(&fake, &dir);
        let mut s = pack(&dir, &fake, Settings::default);
        s.activate().unwrap();
        fake.on_pick("Import this project's build", "Import");
        s.ext.lsp_message(4, &init("/home/u/orbit"));
        s.settle();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c == "server-restart"), "Fenix starts the server again: {calls:?}");
        assert_eq!(trust::get(&s.ext.data, "/home/u/orbit"), Some(trust::Decision::Allow));
        assert_eq!(trust::get(&s.ext.data, "/home/u/orbit-copy"), None);
        // Started again: Metals runs, with no more questions.
        s.ext.lsp_message(4, &init("/home/u/orbit"));
        s.settle();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.starts_with("spawn-raw") && c.ends_with(" /t/metals")), "{calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("pick ")));
        assert_eq!(fake.checks().into_iter().find(|c| c.label == "Metals and the build").unwrap().health, Health::Ok);

        // A later session of the pack, the same project: no question.
        let mut later = pack(&dir, &fake, Settings::default);
        later.activate().unwrap();
        fake.take();
        later.ext.lsp_message(4, &init("/home/u/orbit"));
        later.settle();
        assert!(fake.take().iter().any(|c| c.starts_with("spawn-raw")));

        // A copy at another path asks again, and runs nothing.
        let mut copy = pack(&dir, &fake, Settings::default);
        copy.activate().unwrap();
        fake.take();
        copy.ext.lsp_message(4, &init("/home/u/orbit-copy"));
        copy.settle();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.starts_with("pick ")) && !calls.iter().any(|c| c.starts_with("spawn-raw")), "{calls:?}");

        // Never: kept, so Metals isn't started and it isn't asked again; a command undoes it.
        let never = temp("said-never");
        let mut n = pack(&dir, &fake, Settings::default);
        n.ext.data = never.to_string_lossy().into_owned();
        n.activate().unwrap();
        fake.on_pick("Import this project's build", "Never for this project");
        n.ext.lsp_message(4, &init("/home/u/never"));
        n.settle();
        assert_eq!(trust::get(&n.ext.data, "/home/u/never"), Some(trust::Decision::Never));
        fake.take();
        n.ext.lsp_message(4, &init("/home/u/never"));
        n.settle();
        n.ext.lsp_message(4, r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{}}"#);
        let calls = fake.take();
        assert!(!calls.iter().any(|c| c.starts_with("pick ") || c.starts_with("spawn-raw")), "{calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("lsp 4 ") && c.contains("you said Never")), "{calls:?}");
        assert!(fake.checks().iter().any(|c| c.label == "Metals and the build" && c.detail.contains("you said Never")));
        n.command("scala.forget-project").unwrap();
        assert_eq!(trust::get(&n.ext.data, "/home/u/never"), None);
        fake.on_pick("Import this project's build", "Not now");
        n.ext.lsp_message(4, &init("/home/u/never"));
        n.settle();
        assert!(fake.take().iter().any(|c| c.starts_with("pick ")), "asked again");
        // Not now keeps nothing, and the question isn't repeated in the same session.
        assert_eq!(trust::get(&n.ext.data, "/home/u/never"), None);
        n.ext.lsp_message(4, &init("/home/u/never"));
        assert!(!fake.take().iter().any(|c| c.starts_with("pick ")));
        // The command that allows, from the doctor's row.
        n.command("scala.allow-project").unwrap();
        assert_eq!(trust::get(&n.ext.data, "/home/u/never"), Some(trust::Decision::Allow));
        std::fs::remove_dir_all(&never).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_your_own_setting_runs_metals_without_asking() {
        let dir = cloned("always");
        let fake = Fake::new();
        fake.on_spawn("/t/metals", Ran::running(""));
        project_files_found(&fake, &dir);
        // scala.run_metals is yours: read from Fenix's own settings, which a project's .fenix/settings.toml doesn't reach for it.
        let mut s = pack(&dir, &fake, always);
        s.activate().unwrap();
        fake.take();
        s.ext.lsp_message(4, &init("/home/u/mine"));
        s.settle();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.starts_with("spawn-raw")) && !calls.iter().any(|c| c.starts_with("pick ")), "{calls:?}");
        assert_eq!(trust::get(&s.ext.data, "/home/u/mine"), None, "nothing is kept for it");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn debugging_asks_metals_for_its_adapter_and_connects_to_it() {
        let dir = sbt("debug");
        let fake = installed();
        fake.on_spawn("/t/metals", Ran::running(""));
        let mut s = pack(&dir, &fake, always);
        s.activate().unwrap();
        s.ext.lsp_message(4, &init("/home/u/orbit"));
        s.settle();
        fake.take();
        fake.in_file("/home/u/orbit/src/main/scala/My Main.scala", "scala", "@main def run(): Unit = ()");

        s.command("scala.debug-file").unwrap();
        let p = fake.process("/t/metals").unwrap();
        let stdin = String::from_utf8_lossy(&p.stdin).into_owned();
        let asked = stdin.lines().rfind(|l| l.contains("debug-adapter-start")).unwrap().to_string();
        assert!(asked.contains(r#""arguments":[{"path":"file:///home/u/orbit/src/main/scala/My%20Main.scala","runType":"runOrTestFile"}]"#), "{asked}");
        let id = asked.split("\"id\":\"").nth(1).unwrap().split('"').next().unwrap().to_string();
        // As Metals answered it.
        let answer = lsp::frame(&format!(r#"{{"jsonrpc":"2.0","id":"{id}","result":{{"name":"run","uri":"tcp://127.0.0.1:45007"}}}}"#));
        s.event(Event::ProcessBytes((p.id, answer)));
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.starts_with("connect scala") && c.ends_with(":45007")), "{calls:?}");
        assert!(fake.notifications().iter().any(|n| n == "run: Metals is waiting for the debugger. SPC u u, then Scala: Metals' debug session"), "{:?}", fake.notifications());
        // The tests of the file, and a refusal said.
        s.command("scala.debug-tests").unwrap();
        let stdin = String::from_utf8_lossy(&fake.process("/t/metals").unwrap().stdin).into_owned();
        assert!(stdin.contains(r#""runType":"testFile""#));
        let id = stdin.lines().rfind(|l| l.contains("testFile")).unwrap().split("\"id\":\"").nth(1).unwrap().split('"').next().unwrap().to_string();
        s.event(Event::ProcessBytes((p.id, lsp::frame(&format!(r#"{{"jsonrpc":"2.0","id":"{id}","error":{{"code":-32603,"message":"No test classes found"}}}}"#)))));
        assert!(fake.notifications().iter().any(|n| n == "Metals: No test classes found"));
        // Not a Scala file, or no Metals.
        fake.in_file("/home/u/orbit/README.md", "md", "# orbit");
        assert_eq!(s.command("scala.debug-file"), Err("/home/u/orbit/README.md isn't a .scala or .sc file".into()));
        let mut none = pack(&dir, &Fake::new(), Settings::default);
        assert!(none.command("scala.debug-file").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_repl_a_watching_build_and_the_file_youre_in() {
        let dir = sbt("commands");
        let fake = installed();
        let mut s = pack(&dir, &fake, Settings::default);
        s.activate().unwrap();
        fake.take();

        s.command("scala.console").unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.starts_with("terminal ") && c.ends_with(" sbt console: /t/sbt console")), "{calls:?}");

        // Watching: one background task; the same command stops it.
        s.command("scala.watch").unwrap();
        let calls = fake.take();
        let started = calls.iter().find(|c| c.starts_with("background ")).unwrap();
        assert!(started.ends_with("sbt ~compile: /t/sbt ~compile"), "{started}");
        s.command("scala.watch").unwrap();
        assert!(fake.take().iter().any(|c| c.starts_with("cancel ")), "stopped");
        s.command("scala.watch").unwrap();
        let id: u64 = fake.take().iter().find(|c| c.starts_with("background ")).unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
        s.event(Event::TaskFinished((id, false)));
        s.command("scala.watch").unwrap();
        assert!(fake.take().iter().any(|c| c.starts_with("background ")), "one that ended is started again, not stopped");

        // The file you're in, with scala-cli.
        fake.in_file("/home/u/orbit/scripts/seed.sc", "sc", "println(1)");
        s.command("scala.run-file").unwrap();
        s.command("scala.test-file").unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.ends_with("scala-cli run seed.sc: /t/scala-cli run /home/u/orbit/scripts/seed.sc")), "{calls:?}");
        assert!(calls.iter().any(|c| c.ends_with("scala-cli test seed.sc: /t/scala-cli test /home/u/orbit/scripts/seed.sc")), "{calls:?}");
        fake.in_file("/home/u/orbit/build.sbt", "sbt", "");
        assert_eq!(s.command("scala.run-file"), Err("/home/u/orbit/build.sbt isn't a .scala or .sc file".into()), "scala-cli runs sources and scripts");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_saved_build_file_reads_the_project_again_and_a_setting_changes_what_metals_is_given() {
        let dir = temp("reload");
        let fake = installed();
        let mut s = pack(&dir, &fake, always);
        s.activate().unwrap();
        assert!(!fake.take().iter().any(|c| c.starts_with("tasks")), "not a Scala project");
        assert!(fake.checks().iter().all(|c| c.label != "sbt"));

        write(&dir, "build.sbt", "lazy val root = (project in file(\".\"))\n");
        s.event(Event::Saved("build.sbt".into()));
        assert!(fake.take().iter().any(|c| c == "tasks compile, test, run, clean"));
        assert!(fake.checks().iter().any(|c| c.label == "sbt"));
        // A source saved in a project that is Scala's already changes nothing.
        write(&dir, "src/main/scala/Main.scala", "object Main\n");
        s.event(Event::Saved("src/main/scala/Main.scala".into()));
        assert!(!fake.take().iter().any(|c| c.starts_with("tasks")));

        // A setting: the JDK's home goes to the tasks, and Metals, running, starts again.
        fake.on_spawn("/t/metals", Ran::running(""));
        s.ext.lsp_message(4, &init("/home/u/orbit"));
        s.settle();
        fake.take();
        fake.state_mut().settings.insert("scala.jdk".into(), "/opt/jdk21".into());
        s.ext.read_settings = || Settings { jdk: Some("/opt/jdk21".into()), auto_import: "off".into(), always: true };
        s.event(Event::SettingChanged("scala.jdk".into()));
        let calls = fake.take();
        assert!(calls.iter().any(|c| c == "server-restart"), "{calls:?}");
        assert!(s.ext.kit.tasks.resolved().iter().all(|t| t.cmd.env == [("JAVA_HOME".to_string(), "/opt/jdk21".to_string())]));
        // Another extension's setting is none of its business.
        s.event(Event::SettingChanged("java.jdk".into()));
        assert!(!fake.take().iter().any(|c| c == "server-restart"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `editor.format_on_save` runs only an extension's formatter: the pack has one where it can.
    #[test]
    fn the_native_scalafmt_formats_on_save_when_the_project_pins_its_version() {
        let dir = sbt("formatter");
        write(&dir, ".scalafmt.conf", "version = \"3.11.5\"\nrunner.dialect = scala3\n");
        let fake = installed();
        let mut s = pack(&dir, &fake, Settings::default);
        s.activate().unwrap();
        let calls = fake.take();
        assert!(!calls.iter().any(|c| c.starts_with("formatter")), "not installed yet: {calls:?}");
        let row = fake.checks().into_iter().find(|c| c.label == "scalafmt").unwrap();
        assert_eq!((row.health, row.fix.map(|f| f.1)), (Health::Warn, Some("scala.install.scalafmt".into())));

        // The install plan: its native release, checked by its SHA-256.
        s.command("scala.install.scalafmt").unwrap();
        let download = fake.take().into_iter().find(|c| c.starts_with("download ")).unwrap();
        assert!(download.contains("https://github.com/scalameta/scalafmt/releases/download/v3.11.5/scalafmt-x86_64-pc-linux.zip") && download.ends_with(" 7b0e44cc193eeff06daa14b394dc0489d8eaa4537eb945522f7f267641ed8c2b"), "{download}");

        // Installed (the marker an install leaves): set as the formatter, once.
        let install = s.ext.kit.installer.get("scalafmt").unwrap().clone();
        write(&dir, ".tools/scalafmt/.lang-kit-install", &install.url);
        s.event(Event::TaskFinished((7, true)));
        let calls = fake.take();
        assert_eq!(calls.iter().filter(|c| c.starts_with("formatter")).collect::<Vec<_>>(), ["formatter scala: /tools/scalafmt/scalafmt --stdin --assume-filename ${file}"]);
        s.event(Event::TaskFinished((8, true)));
        assert!(!fake.take().iter().any(|c| c.starts_with("formatter")), "set once");
        assert_eq!(fake.checks().into_iter().find(|c| c.label == "scalafmt").unwrap().health, Health::Ok);

        // The project moves to another version: Metals alone formats it, and the formatter is taken away.
        write(&dir, ".scalafmt.conf", "version = \"3.9.1\"\n");
        s.event(Event::Saved(".scalafmt.conf".into()));
        assert!(fake.take().iter().any(|c| c == "formatter scala: "));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_jdk_setting_is_a_home_whatever_it_names() {
        let home = |s: &str| Settings { jdk: Some(s.into()), ..Settings::default() }.jdk_home();
        assert_eq!(home("/usr/lib/jvm/java-21/bin/java").as_deref(), Some("/usr/lib/jvm/java-21"));
        assert_eq!(home("/usr/lib/jvm/java-21/bin").as_deref(), Some("/usr/lib/jvm/java-21"));
        assert_eq!(home("/usr/lib/jvm/java-21/").as_deref(), Some("/usr/lib/jvm/java-21"));
        assert_eq!(home("C:\\jdk\\bin\\javac.exe").as_deref(), Some("C:/jdk"));
        assert_eq!(Settings::default().jdk_home(), None);
    }

    /// An install takes minutes: how it ended is a notification that offers it again.
    #[test]
    fn a_failed_install_of_coursier_is_a_notification_that_offers_it_again() {
        let dir = temp("install-told");
        let fake = Fake::new();
        fake.on_http_error("https://*", "its SHA-256 isn't the release's");
        let mut s = pack(&dir, &fake, Settings::default);
        s.activate().unwrap();
        s.command("scala.install.coursier").unwrap();
        assert_eq!(fake.notifications(), [format!("Downloading Coursier {}…", tools::COURSIER)]);
        let shown = fake.showing();
        assert_eq!(shown.len(), 1, "{shown:?}");
        let n = &shown[0];
        assert_eq!((n.summary.clone(), n.urgency, n.tag.as_deref()), (format!("Couldn't install Coursier {}", tools::COURSIER), api::Urgency::Normal, Some("install.coursier")));
        assert!(n.body.starts_with("Its download from https://github.com/coursier/coursier/releases/download/") && n.body.ends_with("failed: its SHA-256 isn't the release's."), "{}", n.body);
        assert_eq!((n.actions[0].label.as_str(), n.actions[0].command.as_str()), ("Try again", "scala.install.coursier"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
