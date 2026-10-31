//! Java and Kotlin in Fenix, on lang-kit: Eclipse JDT LS run by the pack
//! (it's Java's language server to Fenix, and passes the messages on),
//! java-debug through JDT LS, kotlin-language-server (or JetBrains'
//! kotlin-lsp), google-java-format and ktlint, Gradle's and Maven's
//! tasks, the project doctor's checks, and the package manager page.
//!
//! What it knows of the build is read from its files ([`project`]), and
//! what's installed is found ([`tools`]); JDT LS's messages go through
//! [`jdtls::Proxy`]; the rest is lang-kit's [`Kit`]. A fix that installs
//! a tool, a build file saved, or moving to another module's file sets
//! them again.

pub mod debug;
pub mod doctor;
pub mod format;
pub mod jdtls;
pub mod packages;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, MouseEvent, Page, PickerItem};
use fenix_lang_kit::exec::Runs;
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{Fenix, Host, Kit, Output, PackagesPage};

use jdtls::{Ask, Launch, MainClass, Out, Proxy};
use project::{Build, Tool};
use tools::{Platform, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's names for the languages.
const JAVA: &[&str] = &["java"];
const KOTLIN: &[&str] = &["kotlin"];

/// What Fenix is asked that lang-kit's [`Host`] doesn't ask: a program
/// whose stdout is bytes (JDT LS), its stdin, being a language server,
/// an adapter on a port, a formatter whose output is kept when it fails.
pub trait Wire {
    fn spawn_raw(&mut self, cmd: &Command) -> Result<u64, String>;
    fn write(&mut self, process: u64, data: &[u8]) -> Result<(), String>;
    fn lsp_send(&mut self, server: u64, message: &str);
    fn adapter_connect(&mut self, languages: &[&str], port: u16) -> Result<(), String>;
    /// The project's language servers started again: JDT LS once it's
    /// installed.
    fn restart_servers(&mut self);
    fn open_url(&mut self, url: &str) -> Result<(), String>;
    /// The formatter for `languages`, what it wrote taken even when it
    /// fails (0.8.7): ktlint's.
    fn formatter_kept(&mut self, languages: &[&str], cmd: Option<&Command>) -> Result<(), String>;
}

/// The real thing.
pub struct FenixWire;

impl Wire for FenixWire {
    fn spawn_raw(&mut self, cmd: &Command) -> Result<u64, String> {
        cmd.spawn_raw()
    }
    fn write(&mut self, process: u64, data: &[u8]) -> Result<(), String> {
        api::process::write(process, data)
    }
    fn lsp_send(&mut self, server: u64, message: &str) {
        api::host::lsp_send(server, message)
    }
    fn adapter_connect(&mut self, languages: &[&str], port: u16) -> Result<(), String> {
        api::project::debug_adapter_connect(languages, None, port)
    }
    fn restart_servers(&mut self) {
        api::project::restart_language_server()
    }
    fn open_url(&mut self, url: &str) -> Result<(), String> {
        api::open_url(url)
    }
    fn formatter_kept(&mut self, languages: &[&str], cmd: Option<&Command>) -> Result<(), String> {
        api::project::formatter_keeping_output(languages, cmd)
    }
}

/// The settings it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub ktlint_on_save: bool,
    pub dependency_check: bool,
    /// `java.jdk`: the JDK you chose, its home given to the build's
    /// tasks as `JAVA_HOME`.
    pub jdk: Option<String>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { ktlint_on_save: true, dependency_check: false, jdk: None }
    }
}

impl Settings {
    fn read() -> Settings {
        let on = |key: &str, default: bool| api::setting(key).map(|v| v.trim() == "true").unwrap_or(default);
        Settings { ktlint_on_save: on("java.ktlint_on_save", true), dependency_check: on("java.dependency_check", false), jdk: api::setting("java.jdk").map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) }
    }
}

/// A program the pack runs to learn something.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Job {
    /// `java -version`, of the Java the servers run on.
    Java,
    /// `javac -J-XshowSettings:properties -version`: the JDK's home.
    JdkHome,
    /// `gradle tasks --all`.
    GradleTasks,
}

/// A picker of the pack's, open.
enum Asking {
    Main(u64, Vec<MainClass>),
}

pub struct Java {
    pub kit: Kit,
    wire: Box<dyn Wire>,
    root: String,
    /// The project's full path, once Fenix has said it (`here`, JDT LS's
    /// `initialize`): where its wrapper is found.
    host_root: Option<String>,
    pub build: Build,
    tools: Tools,
    platform: Platform,
    settings: Settings,
    pub proxy: Proxy,
    pub found: doctor::Found,
    jdk_home: Option<String>,
    jobs: Runs<Job>,
    gradle_tasks: Vec<tasks::GradleTask>,
    /// The file you're in, under the project.
    focus: Option<String>,
    /// java-debug's port, once JDT LS said it.
    pub debug_port: Option<u16>,
    asking: Option<Asking>,
    /// Tools installed when last looked: one new restarts the servers.
    installed: Vec<String>,
    set_kotlin: Option<Command>,
    set_java_format: Option<Command>,
    set_kotlin_format: Option<Command>,
}

impl Default for Java {
    fn default() -> Java {
        let p = api::platform();
        Java::new(Box::new(Fenix), Box::new(FenixWire), PROJECT, Platform::new(&p.os, &p.arch), Settings::read())
    }
}

/// What can be installed, by name.
const INSTALLS: &[&str] = &["jre", "jdtls", "java-debug", "kotlin-language-server", "google-java-format", "ktlint"];

impl Java {
    pub fn new(mut host: Box<dyn Host>, wire: Box<dyn Wire>, root: &str, platform: Platform, settings: Settings) -> Java {
        platform.hide_jre_java(fenix_lang_kit::Installer::new().tools.as_str());
        let tools = Tools::find(host.as_mut());
        let build = Build::read(root);
        let mut kit = Kit::with_host("java", host);
        for i in platform.jre().into_iter().chain([tools::jdtls(), tools::java_debug(), tools::kls(), platform.gjf(), tools::ktlint()]) {
            kit.installer.add(i);
        }
        let mut j = Java {
            kit,
            wire,
            root: root.into(),
            host_root: None,
            build,
            tools,
            platform,
            settings,
            proxy: Proxy::default(),
            found: doctor::Found::default(),
            jdk_home: None,
            jobs: Runs::new(),
            gradle_tasks: Vec::new(),
            focus: None,
            debug_port: None,
            asking: None,
            installed: Vec::new(),
            set_kotlin: None,
            set_java_format: None,
            set_kotlin_format: None,
        };
        j.installed = j.installed_now();
        j.tools.find_wrapper(j.kit.host.as_mut(), j.build.wrapper_name(), None);
        let here = j.kit.host.here().and_then(|(_, r)| r);
        j.learn_root(here);
        j.configure();
        j
    }

    /// The project's full path: its wrapper looked for there.
    fn learn_root(&mut self, root: Option<String>) {
        let Some(root) = root.filter(|r| self.host_root.as_ref() != Some(r)) else { return };
        self.host_root = Some(root);
        let mut tools = self.tools.clone();
        tools.find_wrapper(self.kit.host.as_mut(), self.build.wrapper_name(), self.host_root.as_deref());
        self.tools = tools;
    }

    fn installed_now(&self) -> Vec<String> {
        INSTALLS.iter().filter(|n| self.kit.installer.installed(n)).map(|n| n.to_string()).collect()
    }

    fn installed(&self, name: &str) -> bool {
        self.installed.iter().any(|n| n == name)
    }

    /// The init scripts, where `${tools}` names them for Gradle.
    fn write_init_scripts(&self) {
        let dir = format!("{}/gradle", self.kit.installer.tools);
        let _ = std::fs::create_dir_all(&dir);
        for (name, text) in [("dependencies.init.gradle", packages::DEPENDENCIES_INIT), ("dependency-check.init.gradle", packages::DEPENDENCY_CHECK_INIT)] {
            let path = format!("{dir}/{name}");
            if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
                let _ = std::fs::write(&path, text);
            }
        }
    }

    /// Gradle's or Maven's program: the wrapper, else the one installed.
    fn runner(&self) -> Option<Command> {
        let mut cmd = self.build.runner(self.tools.wrapper.as_deref(), self.tools.gradle.as_deref(), self.tools.mvn.as_deref())?;
        cmd.cwd = Some(PROJECT.into());
        if let Some(home) = self.settings.jdk.as_ref().and(self.jdk_home.as_ref()) {
            cmd.env.push(("JAVA_HOME".into(), home.clone()));
        }
        Some(cmd)
    }

    /// What the doctor's own checks know, now.
    fn found(&self) -> doctor::Found {
        doctor::Found {
            java: self.tools.java.clone(),
            jre: self.installed("jre"),
            jre_here: self.platform.jre().is_some(),
            jdtls: self.installed("jdtls"),
            java_debug: self.installed("java-debug"),
            running: self.proxy.process.is_some(),
            ready: self.proxy.ready,
            missing: self.proxy.missing.clone(),
            kotlin_lsp: self.tools.kotlin_lsp.clone(),
            kls: self.installed("kotlin-language-server"),
            gjf: self.tools.google_java_format.clone().or_else(|| self.installed("google-java-format").then(|| format!("/tools/google-java-format/{}", self.platform.gjf().program))),
            ktlint: self.tools.ktlint.clone().or_else(|| self.installed("ktlint").then(|| "/tools/ktlint/ktlint".into())),
            gradle: self.tools.gradle.clone(),
            mvn: self.tools.mvn.clone(),
            ..self.found.clone()
        }
    }

    /// The module of the file you're in.
    fn module(&self) -> Option<&project::Module> {
        self.focus.as_deref().and_then(|f| self.build.module_of(f))
    }

    /// The kit's tasks, checks, linter and package manager, for the build
    /// and tools as they are.
    fn configure(&mut self) {
        self.write_init_scripts();
        let runner = self.runner();
        self.kit.tasks = tasks::tasks(&self.build, runner.as_ref(), &self.gradle_tasks);
        self.kit.doctor.programs.clear();
        for p in doctor::programs(&self.build) {
            self.kit.doctor.program(p);
        }
        self.found = self.found();
        self.kit.doctor.extra = doctor::checks(&self.build, &self.found);
        let ktlint = format::ktlint(self.installed("ktlint"), self.tools.ktlint.as_deref().filter(|_| !self.installed("ktlint")), self.tools.java.as_deref());
        match (ktlint, self.settings.ktlint_on_save && self.build.kotlin()) {
            (Some(k), true) => self.kit.lint.add(format::ktlint_linter(k)),
            _ => self.kit.lint.linters.clear(),
        }
        match runner {
            Some(r) if self.build.tool != Tool::None => {
                let mut manager = packages::JavaPackages::new(&self.build, r, &self.root, self.settings.dependency_check);
                manager.add_to = self.module().map(|m| m.file.clone()).filter(|f| !f.is_empty());
                match self.kit.packages.as_mut() {
                    Some(page) => page.manager = Box::new(manager),
                    None => self.kit.packages = Some(PackagesPage::new(Box::new(manager))),
                }
            }
            _ => {}
        }
    }

    /// The Kotlin server and the formatters set in Fenix, where they
    /// changed.
    fn apply(&mut self) {
        let java = self.tools.java.clone();
        let kotlin = match (&self.tools.kotlin_lsp, self.installed("kotlin-language-server"), &java) {
            (Some(k), _, _) => Some(Command::new(k).arg("--stdio")),
            (None, true, Some(java)) => Some(tools::kls_command(java)),
            _ => None,
        };
        let installed_gjf = self.installed("google-java-format").then(|| format!("/tools/google-java-format/{}", self.platform.gjf().program));
        let java_format = format::google_java_format(installed_gjf.as_deref(), self.tools.google_java_format.as_deref().filter(|_| installed_gjf.is_none()), java.as_deref());
        let ktlint = format::ktlint(self.installed("ktlint"), self.tools.ktlint.as_deref().filter(|_| !self.installed("ktlint")), java.as_deref());
        let kotlin_format = ktlint.as_ref().map(format::ktlint_format);
        let languages = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        let host = self.kit.host.as_mut();
        if kotlin != self.set_kotlin {
            if let Err(why) = host.language_server(&languages(KOTLIN), kotlin.as_ref()) {
                host.log(&format!("kotlin server: {why}"));
            }
            self.set_kotlin = kotlin;
        }
        if java_format != self.set_java_format {
            if let Err(why) = host.formatter(&languages(JAVA), java_format.as_ref()) {
                host.log(&format!("formatter: {why}"));
            }
            self.set_java_format = java_format;
        }
        if kotlin_format != self.set_kotlin_format {
            if let Err(why) = self.wire.formatter_kept(KOTLIN, kotlin_format.as_ref()) {
                host.log(&format!("formatter: {why}"));
            }
            self.set_kotlin_format = kotlin_format;
        }
    }

    fn refresh(&mut self) {
        self.configure();
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
        }
        self.apply();
    }

    /// What the doctor and the installer found since: a tool a fix
    /// installed is used, and JDT LS, once there's one to run, started.
    fn follow(&mut self) {
        let mut tools = self.tools.clone();
        if let Some(javac) = self.kit.doctor.found.get("javac") {
            tools.javac = Some(javac.clone());
        }
        let now = self.installed_now();
        let new: Vec<String> = now.iter().filter(|n| !self.installed.contains(n)).cloned().collect();
        if !new.is_empty() {
            self.platform.hide_jre_java(&self.kit.installer.tools);
            tools = Tools::find(self.kit.host.as_mut());
            tools.wrapper = self.tools.wrapper.clone();
        }
        let jdk = self.kit.doctor.checks().iter().find(|c| c.label == "JDK").and_then(|c| tools::java_major(&c.detail));
        if tools != self.tools || !new.is_empty() || jdk != self.found.jdk {
            let java_changed = tools.java != self.tools.java;
            self.tools = tools;
            self.installed = now;
            self.found.jdk = jdk;
            if java_changed {
                self.found.java_major = None;
                self.check_java();
            }
            self.refresh();
            self.kit.doctor.run(self.kit.host.as_mut());
            if new.iter().any(|n| n == "jdtls" || n == "jre" || n == "java-debug") {
                self.wire.restart_servers();
            }
        }
    }

    /// `java -version` of the Java the servers run on, and the JDK's home.
    fn check_java(&mut self) {
        let host = self.kit.host.as_mut();
        if let Some(java) = &self.tools.java {
            let _ = self.jobs.start(host, &Command::new(java).arg("-version"), Job::Java);
        }
        if let (Some(javac), None) = (&self.tools.javac, self.tools.jdk_home()) {
            let _ = self.jobs.start(host, &Command::new(javac).args(["-J-XshowSettings:properties", "-version"]), Job::JdkHome);
        }
    }

    /// Gradle's own list of tasks, in the background.
    fn list_gradle_tasks(&mut self) {
        if self.build.tool != Tool::Gradle {
            return;
        }
        self.jobs.kill_where(self.kit.host.as_mut(), |j| *j == Job::GradleTasks);
        if let Some(r) = self.runner() {
            let cmd = r.args(["tasks", "--all", "--console=plain", "-q"]);
            if let Err(why) = self.jobs.start(self.kit.host.as_mut(), &cmd, Job::GradleTasks) {
                self.kit.host.log(&format!("gradle tasks: {why}"));
            }
        }
    }

    fn job_done(&mut self, job: Job, out: Output) {
        match job {
            Job::Java => {
                self.found.java_major = tools::java_major(&out.text());
            }
            Job::JdkHome => self.jdk_home = tools::java_home(&out.text()),
            Job::GradleTasks => {
                if out.ok() {
                    self.gradle_tasks = tasks::parse_gradle_tasks(&out.stdout);
                } else {
                    self.kit.host.log(&format!("gradle tasks: {}", out.why()));
                }
            }
        }
        self.refresh();
        self.kit.doctor.run(self.kit.host.as_mut());
    }

    /// How JDT LS starts here, or why it can't.
    pub fn launch(&self) -> Result<Launch, String> {
        if !self.installed("jdtls") {
            return Err("Eclipse JDT LS isn't installed: the project doctor (SPC p d) installs it".into());
        }
        let java = if self.installed("jre") {
            self.platform.jre_java()
        } else {
            match (&self.tools.java, self.found.java_major) {
                (Some(_), Some(m)) if !jdtls::runs_jdtls(Some(m)) => return Err(format!("JDT LS runs on Java {} or later, and this is Java {m}: the project doctor installs one", tools::JDTLS_JAVA)),
                (Some(j), _) => j.clone(),
                (None, _) => return Err("no java to run JDT LS: the project doctor installs one".into()),
            }
        };
        let home = self.tools.jdk_home().or_else(|| self.jdk_home.clone());
        let release = self.build.release.or(self.found.jdk);
        Ok(Launch {
            java,
            config: jdtls::config_dir(&self.platform.os, &self.platform.arch),
            tools: self.tools.tools_dir(),
            java_debug: self.installed("java-debug").then(|| format!("/tools/java-debug/{}", tools::java_debug().program)),
            jdk: release.zip(home),
        })
    }

    fn act(&mut self, outs: Vec<Out>) {
        for o in outs {
            match o {
                Out::Spawn(cmd) => match self.wire.spawn_raw(&cmd) {
                    Ok(p) => self.proxy.started(p),
                    Err(why) => {
                        self.kit.host.notify(&format!("JDT LS didn't start: {why}"));
                        self.proxy.missing = Some(why);
                    }
                },
                Out::Write(bytes) => {
                    if let Some(p) = self.proxy.process {
                        if let Err(why) = self.wire.write(p, &bytes) {
                            self.kit.host.log(&format!("jdtls: {why}"));
                        }
                    }
                }
                Out::Send(server, text) => self.wire.lsp_send(server, &text),
                Out::Ready => {
                    self.refresh();
                    self.kit.doctor.run(self.kit.host.as_mut());
                    if self.installed("java-debug") {
                        if let Ok(outs) = self.proxy.ask(Ask::DebugSession) {
                            self.act(outs);
                        }
                    }
                }
                Out::Answer(ask, answer) => self.answered(ask, answer),
            }
        }
    }

    fn answered(&mut self, ask: Ask, answer: Result<Value, String>) {
        match (ask, answer) {
            (Ask::DebugSession, Ok(port)) => {
                let Some(port) = port.as_u64().and_then(|p| u16::try_from(p).ok()) else { return };
                self.debug_port = Some(port);
                if let Err(why) = self.wire.adapter_connect(JAVA, port) {
                    self.kit.host.log(&format!("java-debug: {why}"));
                }
            }
            (Ask::MainClasses, Ok(v)) => {
                let mains = jdtls::main_classes(&v);
                let here = self.focus.as_ref().map(|f| format!("{}/{f}", self.proxy.root.as_deref().unwrap_or(&self.root)));
                match mains.iter().find(|m| m.file.is_some() && m.file == here) {
                    Some(m) => self.debug_main(m.clone()),
                    None if mains.len() == 1 => self.debug_main(mains[0].clone()),
                    None if mains.is_empty() => self.kit.host.notify("JDT LS found no main class"),
                    None => {
                        let items: Vec<PickerItem> = mains.iter().map(|m| PickerItem::new(m.main.clone(), m.main.clone()).detail(m.project.clone().unwrap_or_default())).collect();
                        let id = self.kit.host.pick("Debug which main class", &items);
                        self.asking = Some(Asking::Main(id, mains));
                    }
                }
            }
            (Ask::Classpath(m), Ok(v)) => {
                let (module_path, class_path) = jdtls::classpath(&v);
                let java = self.tools.java.clone().unwrap_or_else(|| "java".into());
                let mut cmd = debug::held(&java, &m.main, &module_path, &class_path, self.platform.windows());
                cmd.cwd = Some(PROJECT.into());
                self.run_held(&format!("debug {}", m.main.rsplit('.').next().unwrap_or(&m.main)), &cmd);
            }
            (ask, Err(why)) => self.kit.host.notify(&format!("JDT LS: {why} ({ask:?})")),
        }
    }

    /// A program held for the debugger, as a task: the session attaches.
    fn run_held(&mut self, name: &str, cmd: &Command) {
        let host = self.kit.host.as_mut();
        match host.run_task(name, cmd) {
            Ok(_) => host.notify(&format!("{name}: waiting for the debugger on port {}. SPC u u, then Java: attach", debug::PORT)),
            Err(why) => host.notify(&format!("{name}: {why}")),
        }
    }

    /// Debugging `m`: by Gradle's `run` when it's an application's own
    /// main class (Gradle builds the Kotlin JDT LS doesn't), else as JDT
    /// LS says to run it.
    fn debug_main(&mut self, m: MainClass) {
        let app = self.build.modules.iter().find(|x| x.application && x.main_class.as_deref() == Some(m.main.as_str())).cloned();
        if let (Tool::Gradle, Some(app), Some(runner)) = (self.build.tool, app, self.runner()) {
            if !app.spring_boot {
                return self.run_held(&format!("debug {}", app.task("run")), &debug::gradle_run(&runner, &app));
            }
        }
        match self.proxy.ask(Ask::Classpath(m)) {
            Ok(outs) => self.act(outs),
            Err(why) => self.kit.host.notify(&why),
        }
    }

    fn own_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            doctor::INSTALL_JDK => {
                let page = doctor::jdk_page(self.build.release);
                self.wire.open_url(&page)?;
                self.kit.host.notify(&format!("A JDK: {page}"));
                Ok(())
            }
            doctor::WRAPPER => {
                let cmd = doctor::wrapper_command(&self.build, &self.found, self.tools.wrapper.as_deref()).ok_or("no Gradle or Maven to make the wrapper with")?;
                let cmd = Command { cwd: Some(PROJECT.into()), ..cmd };
                self.kit.host.run_task("wrapper", &cmd).map(|_| ())
            }
            "java.debug-main" => {
                if self.proxy.ready {
                    let outs = self.proxy.ask(Ask::MainClasses)?;
                    self.act(outs);
                    return Ok(());
                }
                // JDT LS not there yet: Gradle's application.
                let app = self.module().filter(|m| m.application).or_else(|| self.build.modules.iter().find(|m| m.application)).cloned();
                match (self.build.tool, app, self.runner()) {
                    (Tool::Gradle, Some(app), Some(r)) => {
                        self.run_held(&format!("debug {}", app.task("run")), &debug::gradle_run(&r, &app));
                        Ok(())
                    }
                    _ => Err(self.proxy.missing.clone().unwrap_or_else(|| "JDT LS is still reading the project".into())),
                }
            }
            "java.debug-tests" => {
                let runner = self.runner().ok_or("no Gradle or Maven here")?;
                let module = self.module().filter(|m| m.tests).cloned();
                let class = self.focus.as_deref().filter(|f| f.contains("src/test/")).and_then(debug::class_of);
                let cmd = debug::tests_held(&self.build, &runner, module.as_ref(), class.as_deref()).ok_or("no Gradle or Maven here")?;
                self.run_held("debug tests", &cmd);
                Ok(())
            }
            "java.reload" => {
                self.reload();
                Ok(())
            }
            _ => Err(format!("no command {id} here")),
        }
    }

    /// The build's files changed: read again, its tasks listed again, and
    /// checked again.
    fn reload(&mut self) {
        self.build = Build::read(&self.root);
        let root = self.host_root.take();
        self.learn_root(root);
        self.refresh();
        self.list_gradle_tasks();
        self.kit.doctor.run(self.kit.host.as_mut());
    }

    fn focused(&mut self, path: &str) {
        let wrapper = self.tools.wrapper.clone();
        let here = self.kit.host.here().and_then(|(_, r)| r);
        self.learn_root(here);
        if self.tools.wrapper != wrapper {
            self.refresh();
        }
        let path = path.replace('\\', "/");
        let rel = path.strip_prefix(&format!("{PROJECT}/")).or_else(|| path.strip_prefix(&format!("{}/", self.root))).or_else(|| self.proxy.root.as_ref().and_then(|r| path.strip_prefix(&format!("{r}/")))).unwrap_or(&path).to_string();
        let before = self.module().map(|m| m.name.clone());
        self.focus = Some(rel);
        if self.module().map(|m| m.name.clone()) != before {
            self.configure();
        }
    }

    /// An event for the pack's own: whether it was.
    fn own_event(&mut self, event: &Event) -> bool {
        if self.jobs.owns(event) {
            if let Some((job, out)) = self.jobs.event(event) {
                self.job_done(job, out);
            }
            return true;
        }
        match event {
            Event::ProcessBytes((p, bytes)) if self.proxy.process == Some(*p) => {
                let outs = self.proxy.from_jdtls(bytes);
                self.act(outs);
                true
            }
            Event::ProcessOutput(o) if self.proxy.process == Some(o.process) => true,
            Event::ProcessExit((p, code)) if self.proxy.process == Some(*p) => {
                let outs = self.proxy.exited();
                self.act(outs);
                self.debug_port = None;
                self.kit.host.notify(&format!("JDT LS stopped ({})", code.map(|c| c.to_string()).unwrap_or_else(|| "killed".into())));
                self.refresh();
                self.kit.doctor.run(self.kit.host.as_mut());
                true
            }
            Event::Picked((id, value)) => match self.asking.take() {
                Some(Asking::Main(a, mains)) if a == *id => {
                    if let Some(m) = value.as_ref().and_then(|v| mains.iter().find(|m| &m.main == v)) {
                        self.debug_main(m.clone());
                    }
                    true
                }
                other => {
                    self.asking = other;
                    false
                }
            },
            Event::FocusChanged(Some(path)) => {
                self.focused(path);
                false
            }
            Event::SettingChanged(key) if key.starts_with("java.") => {
                #[cfg(target_family = "wasm")]
                {
                    self.settings = Settings::read();
                    self.tools = Tools::find(self.kit.host.as_mut());
                }
                self.refresh();
                true
            }
            _ => false,
        }
    }
}

impl Extension for Java {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.check_java();
        self.list_gradle_tasks();
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
        if self.own_event(&event) {
            return;
        }
        let saved = match &event {
            Event::Saved(name) => {
                let file = name.replace('\\', "/");
                let file = file.rsplit('/').next().unwrap_or(&file).to_string();
                project::BUILD_FILES.contains(&file.as_str())
            }
            _ => false,
        };
        self.kit.handle(&event);
        if saved {
            self.reload();
        }
        self.follow();
    }

    fn lsp_message(&mut self, server: u64, message: &str) {
        let launch = self.launch();
        let outs = self.proxy.from_fenix(server, message, launch.as_ref().map_err(String::as_str));
        self.act(outs);
        let wrapper = self.tools.wrapper.clone();
        self.learn_root(self.proxy.root.clone());
        if self.tools.wrapper != wrapper {
            self.refresh();
        }
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

export!(Java);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::orbit;
    use fenix_extension_api::Health;
    use fenix_lang_kit::host::mock::Shared;
    use fenix_lang_kit::serde_json::json;

    /// The wrapper as the samples' tests spell it.
    pub fn wrapper(b: &Build) -> &'static str {
        if b.tool == Tool::Maven {
            "./mvnw"
        } else {
            "./gradlew"
        }
    }

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    /// The wire, written down in the mock's calls.
    struct TestWire(Shared);

    impl Wire for TestWire {
        fn spawn_raw(&mut self, cmd: &Command) -> Result<u64, String> {
            let mut m = self.0 .0.borrow_mut();
            m.next += 1;
            let id = m.next;
            m.calls.push(format!("raw {id} {}", line(cmd)));
            Ok(id)
        }
        fn write(&mut self, process: u64, data: &[u8]) -> Result<(), String> {
            let mut b = data.to_vec();
            let msg = jdtls::unframe(&mut b).join("");
            self.0 .0.borrow_mut().calls.push(format!("write {process} {msg}"));
            Ok(())
        }
        fn lsp_send(&mut self, server: u64, message: &str) {
            self.0 .0.borrow_mut().calls.push(format!("lsp {server} {message}"));
        }
        fn adapter_connect(&mut self, languages: &[&str], port: u16) -> Result<(), String> {
            self.0 .0.borrow_mut().calls.push(format!("connect {}: {port}", languages.join(",")));
            Ok(())
        }
        fn restart_servers(&mut self) {
            self.0 .0.borrow_mut().calls.push("restart servers".into());
        }
        fn open_url(&mut self, url: &str) -> Result<(), String> {
            self.0 .0.borrow_mut().calls.push(format!("open {url}"));
            Ok(())
        }
        fn formatter_kept(&mut self, languages: &[&str], cmd: Option<&Command>) -> Result<(), String> {
            self.0 .0.borrow_mut().calls.push(format!("formatter {} (kept): {}", languages.join(","), cmd.map(line).unwrap_or_default()));
            Ok(())
        }
    }

    fn host() -> Shared {
        let host = Shared::default();
        host.0.borrow_mut().programs.extend([("javac", "/usr/lib/jvm/temurin-21/bin/javac"), ("java", "/usr/lib/jvm/temurin-21/bin/java"), ("gradle", "/opt/gradle/bin/gradle")].map(|(a, b)| (a.to_string(), b.to_string())));
        host
    }

    /// The pack on `dir`, its tools folder `tools`: what's named in
    /// `installed` is there.
    fn pack(dir: &std::path::Path, tools: &std::path::Path, host: &Shared, installed: &[&str]) -> Java {
        host.0.borrow_mut().programs.insert("gradlew".into(), format!("{}/gradlew", dir.display()));
        host.0.borrow_mut().here = Some((format!("{}/build.gradle.kts", dir.display()), Some(dir.to_string_lossy().into_owned())));
        let mut j = Java::new(Box::new(host.clone()), Box::new(TestWire(host.clone())), &dir.to_string_lossy(), Platform::new("linux", "x86_64"), Settings::default());
        j.kit.installer.tools = tools.to_string_lossy().into_owned();
        for name in installed {
            let i = j.kit.installer.get(name).unwrap().clone();
            let d = tools.join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(fenix_lang_kit::install::MARKER), &i.url).unwrap();
        }
        j.installed = j.installed_now();
        if let Some(page) = j.kit.packages.as_mut() {
            page.cache_dir = None;
        }
        j.configure();
        j
    }

    fn said(j: &mut Java, process: u64, text: &str, code: i32) {
        for l in text.lines() {
            j.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: l.into() }));
        }
        j.handle(Event::ProcessExit((process, Some(code))));
    }

    fn spawned(calls: &[String], what: &str) -> u64 {
        calls.iter().find_map(|c| c.strip_prefix("spawn ").filter(|c| c.contains(what)).and_then(|c| c.split(' ').next()?.parse().ok())).unwrap_or_else(|| panic!("{what}: {calls:?}"))
    }

    fn init(root: &str) -> String {
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "rootUri": format!("file://{root}"), "capabilities": {} } }).to_string()
    }

    #[test]
    fn a_gradle_build_gets_its_tasks_checks_and_kotlin_server() {
        let dir = orbit("activate");
        let tools = project::tests::temp("tools-activate");
        let host = host();
        let mut j = pack(&dir, &tools, &host, &["kotlin-language-server"]);
        j.activate().unwrap();
        let calls = host.take();
        assert!(calls[0].starts_with("tasks build, test, clean, check, run app, test app, test core"), "{calls:?}");
        let kls = calls.iter().find(|c| c.starts_with("server kotlin:")).unwrap();
        assert!(kls.ends_with("-cp ${tools}/kotlin-language-server/lib/* org.javacs.kt.MainKt"), "{kls}");
        assert!(!calls.iter().any(|c| c.starts_with("formatter java")), "no google-java-format: JDT LS formats");
        let javac = spawned(&calls, "javac -version");
        let java = spawned(&calls, "temurin-21/bin/java -version");
        let gradle_tasks = spawned(&calls, "gradlew tasks --all --console=plain -q");
        said(&mut j, javac, "javac 21.0.12", 0);
        said(&mut j, java, "openjdk version \"21.0.12\" 2026-07-15", 0);
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(checks[0], ("JDK".into(), Health::Ok));
        assert_eq!(checks[1], ("Gradle wrapper".into(), Health::Ok));
        assert_eq!(checks[2], ("Server runtime".into(), Health::Bad), "JDT LS needs 25");
        assert!(j.launch().unwrap_err().contains("Eclipse JDT LS isn't installed"));
        // Gradle's own tasks, once it says them.
        host.take();
        said(&mut j, gradle_tasks, include_str!("samples/gradle-tasks-all.txt"), 0);
        let calls = host.take();
        assert!(calls.iter().any(|c| c.starts_with("tasks ") && c.contains("installDist app")), "{calls:?}");
        // The init scripts, where `${tools}` names them.
        assert_eq!(std::fs::read_to_string(tools.join("gradle/dependencies.init.gradle")).unwrap(), packages::DEPENDENCIES_INIT);
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&tools).unwrap();
    }

    #[test]
    fn jdtls_is_started_through_the_pack_and_java_debug_connected() {
        let dir = orbit("jdtls");
        let tools = project::tests::temp("tools-jdtls");
        let host = host();
        host.0.borrow_mut().programs.insert("jdtls".into(), format!("{}/jdtls/bin/jdtls", tools.display()));
        host.0.borrow_mut().programs.insert("fenix-java".into(), format!("{}/jre/bin/fenix-java", tools.display()));
        let mut j = pack(&dir, &tools, &host, &["jre", "jdtls", "java-debug"]);
        j.activate().unwrap();
        host.take();
        let root = dir.to_string_lossy().into_owned();
        j.lsp_message(3, &init(&root));
        let calls = host.take();
        let raw = calls.iter().find(|c| c.starts_with("raw ")).unwrap();
        assert!(raw.starts_with("raw ") && raw.contains("/tools/jre/bin/fenix-java -Declipse.application=org.eclipse.jdt.ls.core.id1"), "{raw}");
        let process: u64 = raw.split(' ').nth(1).unwrap().parse().unwrap();
        let write = calls.iter().find(|c| c.starts_with(&format!("write {process} "))).unwrap();
        assert!(write.contains(&format!("\"bundles\":[\"{}/java-debug/com.microsoft.java.debug.plugin-0.53.1.jar\"]", tools.display())), "{write}");
        assert!(write.contains("\"JavaSE-21\"") && write.contains("/usr/lib/jvm/temurin-21"), "{write}");

        // JDT LS answers, and once it's read the project, java-debug's
        // port is asked for and connected to.
        j.handle(Event::ProcessBytes((process, include_bytes!("samples/jdtls-stdout.bin").to_vec())));
        let calls = host.take();
        assert!(calls.iter().any(|c| c.starts_with("lsp 3 ") && c.contains("executeCommandProvider")), "{calls:?}");
        let ask = calls.iter().find(|c| c.starts_with("write ") && c.contains("vscode.java.startDebugSession")).unwrap_or_else(|| panic!("{calls:?}"));
        let id = ask.split("\"id\":\"").nth(1).unwrap().split('"').next().unwrap().to_string();
        j.handle(Event::ProcessBytes((process, jdtls::frame(&json!({ "jsonrpc": "2.0", "id": id, "result": 42067 }).to_string()))));
        assert_eq!(host.take(), ["connect java: 42067"]);

        // A main class to debug: JDT LS's, held for the debugger.
        j.handle(Event::FocusChanged(Some(format!("{root}/core/src/main/kotlin/orbit/core/Orbits.kt"))));
        host.take();
        j.run_command("java.debug-main").unwrap();
        let ask = host.take().into_iter().find(|c| c.contains("vscode.java.resolveMainClass")).unwrap();
        let id = ask.split("\"id\":\"").nth(1).unwrap().split('"').next().unwrap().to_string();
        let mains = json!([{ "mainClass": "orbit.tools.Seed", "projectName": "core", "filePath": format!("{root}/core/src/main/java/orbit/tools/Seed.java") }, { "mainClass": "orbit.app.App", "projectName": "app", "filePath": format!("{root}/app/src/main/java/orbit/app/App.java") }]);
        j.handle(Event::ProcessBytes((process, jdtls::frame(&json!({ "jsonrpc": "2.0", "id": id, "result": mains }).to_string()))));
        let calls = host.take();
        let pick = calls.iter().find_map(|c| c.strip_prefix("pick ")).unwrap();
        assert_eq!(pick.split_once(' ').unwrap().1, "Debug which main class: orbit.tools.Seed, orbit.app.App");
        let pick: u64 = pick.split(' ').next().unwrap().parse().unwrap();
        // The application's own main class: Gradle's run, held.
        j.handle(Event::Picked((pick, Some("orbit.app.App".into()))));
        let calls = host.take();
        assert!(calls.iter().any(|c| c.ends_with(&format!("debug :app:run: {root}/gradlew :app:run --debug-jvm"))), "{calls:?}");
        // Another: as JDT LS says to run it.
        j.answered(Ask::MainClasses, Ok(json!([mains[0].clone()])));
        let ask = host.take().into_iter().find(|c| c.contains("vscode.java.resolveClasspath")).unwrap();
        assert!(ask.contains("\"arguments\":[\"orbit.tools.Seed\",\"core\"]"), "{ask}");
        j.answered(Ask::Classpath(jdtls::main_classes(&json!([mains[0].clone()]))[0].clone()), Ok(json!([[], [format!("{root}/core/bin/main"), "/m2/guava.jar"]])));
        let calls = host.take();
        assert!(calls.iter().any(|c| c.contains("debug Seed: ") && c.ends_with(&format!("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:5005 -cp {root}/core/bin/main:/m2/guava.jar orbit.tools.Seed"))), "{calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("notify debug Seed: waiting for the debugger on port 5005")));

        // JDT LS ends.
        j.handle(Event::ProcessExit((process, Some(13))));
        assert!(host.take().contains(&"notify JDT LS stopped (13)".to_string()));
        assert_eq!(j.debug_port, None);
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&tools).unwrap();
    }

    #[test]
    fn without_jdtls_fenix_is_answered_and_installing_it_restarts_the_server() {
        let dir = orbit("no-jdtls");
        let tools = project::tests::temp("tools-no-jdtls");
        let host = host();
        let mut j = pack(&dir, &tools, &host, &[]);
        j.activate().unwrap();
        host.take();
        j.lsp_message(3, &init(&dir.to_string_lossy()));
        let calls = host.take();
        assert!(calls.len() == 1 && calls[0].starts_with("lsp 3 ") && calls[0].contains("\"capabilities\":{}"), "{calls:?}");
        assert_eq!(j.found().missing.as_deref(), Some("Eclipse JDT LS isn't installed: the project doctor (SPC p d) installs it"));
        // Its fix row installs it; once it's there, the server starts again.
        let i = j.kit.installer.get("jdtls").unwrap().clone();
        std::fs::create_dir_all(tools.join("jdtls")).unwrap();
        std::fs::write(tools.join("jdtls").join(fenix_lang_kit::install::MARKER), &i.url).unwrap();
        j.run_command("java.doctor").unwrap();
        assert!(host.take().contains(&"restart servers".to_string()));
        // Its JDK: the download page.
        j.run_command(doctor::INSTALL_JDK).unwrap();
        assert!(host.take().contains(&"open https://adoptium.net/temurin/releases/?version=21".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&tools).unwrap();
    }

    #[test]
    fn tests_are_held_and_formatters_set_once_installed() {
        let dir = orbit("tests");
        let tools = project::tests::temp("tools-tests");
        let host = host();
        let mut j = pack(&dir, &tools, &host, &["google-java-format", "ktlint"]);
        j.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"formatter java: /tools/google-java-format/google-java-format -".to_string()), "{calls:?}");
        assert!(calls.contains(&"formatter kotlin (kept): /usr/lib/jvm/temurin-21/bin/java -jar ${tools}/ktlint/ktlint --format --stdin --stdin-path=${file} --log-level=none".to_string()), "ktlint's text kept when it fails: {calls:?}");
        j.handle(Event::FocusChanged(Some("/project/app/src/test/java/orbit/app/AppTest.java".into())));
        j.run_command("java.debug-tests").unwrap();
        let calls = host.take();
        assert!(calls.iter().any(|c| c.ends_with(&format!("debug tests: {}/gradlew :app:test --debug-jvm --tests orbit.app.AppTest", dir.display()))), "{calls:?}");
        // Packages go to the module you're in.
        let page = j.kit.packages.as_ref().unwrap();
        assert!(line(&page.manager.add("org.slf4j:slf4j-api")).ends_with("-Pfenix.file=app/build.gradle.kts -Pfenix.coords=org.slf4j:slf4j-api"));
        // A saved Kotlin file is linted by ktlint.
        host.0.borrow_mut().here = Some((format!("{}/core/src/main/kotlin/orbit/core/Orbits.kt", dir.display()), Some(dir.to_string_lossy().into_owned())));
        j.handle(Event::Saved("Orbits.kt".into()));
        assert!(host.take().iter().any(|c| c.ends_with("-jar ${tools}/ktlint/ktlint --reporter=json --relative --log-level=none core/src/main/kotlin/orbit/core/Orbits.kt")));
        // A build file saved: read again, and Gradle asked again.
        j.handle(Event::Saved("build.gradle.kts".into()));
        assert!(host.take().iter().any(|c| c.contains("gradlew tasks --all")));
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&tools).unwrap();
    }
}
