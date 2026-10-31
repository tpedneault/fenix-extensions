//! Rust in Fenix, on lang-kit: rust-analyzer as the language server,
//! lldb-dap or CodeLLDB as the debug adapter (a launch for each binary and
//! example, built first; launch files that have cargo build what they
//! debug), clippy on save with its suggestions as fixes, rustfmt as the
//! formatter, the tasks from Cargo.toml's targets, the project doctor's
//! checks, and the package manager page.
//!
//! What it knows of the project is read from its files ([`project`]) and
//! from `cargo metadata --no-deps`, run when it starts and when
//! `Cargo.toml` is saved; what's installed is found ([`tools`]). The rest
//! is lang-kit's [`Kit`], given cargo, the checks and the tasks. A fix
//! that installs a tool, or a project file saved, sets them again.

pub mod clippy;
pub mod debug;
pub mod doctor;
pub mod packages;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use std::collections::{BTreeSet, HashMap};

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, LaunchConfig, MouseEvent, Page};
use fenix_lang_kit::exec::Runs;
use fenix_lang_kit::{Fenix, Host, Kit};

use packages::{Cargo, Latest};
use project::{Metadata, Project};
use tools::{Debugger, Tools};

/// What the pack asks cargo and rustc, for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// `cargo metadata --no-deps`: the targets and the packages.
    Metadata,
    /// `rustc --print sysroot`: where rustc's lldb formatters are.
    Sysroot,
}

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language.
const RUST: &[&str] = &["rust"];

/// Files whose saving changes what the project is.
const PROJECT_FILES: &[&str] = &["Cargo.toml", "rust-toolchain.toml", "rust-toolchain", "rustfmt.toml", ".rustfmt.toml"];

/// An HTTP GET of the crates.io index, its id: `api`'s in Fenix, a test's
/// own in tests.
pub type Get = Box<dyn FnMut(&str) -> Result<u64, String>>;

/// A setting's value by its key.
pub type Setting = Box<dyn Fn(&str) -> Option<String>>;

/// rustfmt on the buffer, for the project's edition: rustfmt reads stdin
/// and writes stdout, and takes the edition from `rustfmt.toml` or its
/// command line, never from Cargo.toml.
pub fn formatter(rustfmt: &str, edition: Option<&str>) -> Command {
    let cmd = Command::new(rustfmt);
    match edition {
        Some(e) => cmd.args(["--edition", e]),
        None => cmd,
    }
}

pub struct Rust {
    pub kit: Kit,
    root: String,
    project: Project,
    metadata: Option<Metadata>,
    tools: Tools,
    setting: Setting,
    get: Get,
    /// cargo's metadata and rustc's sysroot, while they're asked.
    reading: Runs<Ask>,
    sysroot: Option<String>,
    /// The machine runs Windows: a program's `.exe`.
    windows: bool,
    /// The index's answers to come, by request, and the crates asked.
    fetching: HashMap<u64, String>,
    fetched: BTreeSet<String>,
    latest: Latest,
    /// rust-analyzer being added by its fix row: the server is started
    /// again when it's done.
    adding_server: Option<u64>,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_adapter: Option<(Command, bool)>,
    set_formatter: Option<Command>,
    set_launches: Option<Vec<LaunchConfig>>,
}

impl Default for Rust {
    fn default() -> Rust {
        let get: Get = Box::new(|url| api::HttpRequest::get(url).send());
        Rust::new(Box::new(Fenix), PROJECT, Box::new(api::setting), get)
    }
}

impl Rust {
    /// The pack for the project in `root`, talking to `host`.
    pub fn new(mut host: Box<dyn Host>, root: &str, setting: Setting, get: Get) -> Rust {
        let tools = Tools::find(host.as_mut());
        let windows = host.platform().os == "windows";
        let project = Project::read(root);
        let latest = Latest::default();
        let mut cargo = Cargo::new(tools.cargo(), root);
        cargo.latest = latest.clone();
        let kit = Kit::with_host("rust", host).packages(Box::new(cargo));
        let mut r = Rust {
            kit,
            root: root.into(),
            project,
            metadata: None,
            tools,
            setting,
            get,
            reading: Runs::new(),
            sysroot: None,
            windows,
            fetching: HashMap::new(),
            fetched: BTreeSet::new(),
            latest,
            adding_server: None,
            set_server: None,
            set_adapter: None,
            set_formatter: None,
            set_launches: None,
        };
        r.configure();
        r
    }

    fn debugger(&self) -> Debugger {
        Debugger::parse(&(self.setting)("rust.debugger").unwrap_or_default())
    }

    /// The project's files' folder as cargo has it: the package the
    /// project's Cargo.toml is, in its workspace; else the workspace's.
    fn project_root(&self) -> Option<String> {
        let m = self.metadata.as_ref()?;
        let own = self.project.name.as_ref().and_then(|name| m.packages.iter().find(|p| &p.name == name));
        let dir = own.and_then(|p| p.manifest_path.replace('\\', "/").rsplit_once('/').map(|(dir, _)| dir.to_string()));
        dir.or_else(|| Some(m.workspace_root.clone()).filter(|r| !r.is_empty()))
    }

    fn workspace(&self) -> bool {
        self.project.workspace || self.metadata.as_ref().is_some_and(Metadata::several)
    }

    /// The kit's tasks, checks and package manager, and clippy, for the
    /// project and tools as they are.
    fn configure(&mut self) {
        let (project, metadata, tools) = (&self.project, self.metadata.as_ref(), &self.tools);
        self.kit.tasks = tasks::tasks(metadata, project.workspace, tools);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(project, metadata, tools, self.debugger()) {
            self.kit.doctor.program(program);
        }
        let on = (self.setting)("rust.clippy").is_none_or(|v| v.trim() != "false");
        if on && project.cargo {
            let workspace_root = metadata.map(|m| m.workspace_root.clone()).unwrap_or_default();
            let project_root = self.project_root().unwrap_or_else(|| workspace_root.clone());
            self.kit.lint.add(clippy::linter(self.tools.cargo(), self.workspace(), &workspace_root, &project_root));
        } else {
            self.kit.lint.remove(self.kit.host.as_mut(), clippy::NAME);
        }
        let add_to = match (project.name.as_ref(), metadata) {
            (None, Some(m)) if m.several() => m.packages.first().map(|p| p.name.clone()),
            _ => None,
        };
        if let Some(page) = self.kit.packages.as_mut() {
            let mut cargo = Cargo::new(tools.cargo(), &self.root);
            cargo.latest = self.latest.clone();
            cargo.outdated_tool = tools.cargo_outdated.is_some();
            cargo.audit_tool = tools.cargo_audit.is_some();
            cargo.add_to = add_to;
            page.manager = Box::new(cargo);
        }
    }

    /// The server, the adapter and the formatter set in Fenix, where they
    /// changed. Nothing found leaves Fenix's own.
    fn apply(&mut self) {
        let server = self.tools.rust_analyzer.as_deref().map(Command::new);
        let adapter = self.tools.debugger(self.debugger()).map(|(d, path)| debug::adapter(d, path));
        let launches = self.metadata.as_ref().map(|m| debug::launches(m, self.windows, self.sysroot.as_deref()));
        let edition = if self.project.rustfmt_edition { None } else { self.project.edition.clone().or_else(|| self.metadata.as_ref().and_then(Metadata::edition)).or(Some("2021".into())) };
        let formatter = self.tools.rustfmt.as_deref().map(|r| formatter(r, edition.as_deref()));
        let languages: Vec<String> = RUST.iter().map(|l| l.to_string()).collect();
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
        }
        if adapter != self.set_adapter {
            let done = match &adapter {
                Some((cmd, true)) => host.debug_adapter_connect(&languages, Some(cmd), 0),
                Some((cmd, false)) => host.debug_adapter(&languages, Some(cmd)),
                None => host.debug_adapter(&languages, None),
            };
            if let Err(why) = done {
                host.log(&format!("debug adapter: {why}"));
            }
            self.set_adapter = adapter;
        }
        if launches.is_some() && launches != self.set_launches {
            if let Err(why) = host.launches(launches.as_deref().unwrap_or_default()) {
                host.log(&format!("launches: {why}"));
            }
            self.set_launches = launches;
        }
        if formatter != self.set_formatter {
            if let Err(why) = host.formatter(&languages, formatter.as_ref()) {
                host.log(&format!("formatter: {why}"));
            }
            self.set_formatter = formatter;
        }
    }

    /// What's installed looked for again (a fix ran): what changed is used.
    fn follow(&mut self) {
        let tools = Tools::find(self.kit.host.as_mut());
        if tools != self.tools {
            self.tools = tools;
            self.configure();
            if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
                self.kit.host.log(&format!("tasks: {why}"));
            }
            self.apply();
            self.fetch_latest();
        }
    }

    /// `cargo metadata --no-deps`, for the targets and the packages.
    fn read_metadata(&mut self) {
        if !self.project.cargo {
            return;
        }
        let cmd = self.tools.cargo().args(["metadata", "--format-version", "1", "--no-deps"]).cwd(PROJECT);
        let host = self.kit.host.as_mut();
        self.reading.kill_where(host, |a| *a == Ask::Metadata);
        if let Err(why) = self.reading.start(host, &cmd, Ask::Metadata) {
            host.log(&format!("cargo metadata: {why}"));
        }
    }

    /// `rustc --print sysroot`, in the project (its toolchain's), for the
    /// launches' formatters.
    fn read_sysroot(&mut self) {
        if !self.project.cargo {
            return;
        }
        let rustc = Tools::run(self.kit.doctor.found.get("rustc").map(String::as_str), "rustc").args(["--print", "sysroot"]).cwd(PROJECT);
        let host = self.kit.host.as_mut();
        self.reading.kill_where(host, |a| *a == Ask::Sysroot);
        if let Err(why) = self.reading.start(host, &rustc, Ask::Sysroot) {
            host.log(&format!("rustc --print sysroot: {why}"));
        }
    }

    /// The newest of each crates.io crate the project asks for, from the
    /// sparse index, for the page: unless cargo-outdated says them.
    fn fetch_latest(&mut self) {
        if self.tools.cargo_outdated.is_some() {
            return;
        }
        let Some(m) = &self.metadata else { return };
        for name in m.crates_io_deps() {
            if self.fetched.insert(name.clone()) {
                match (self.get)(&packages::index_url(&name)) {
                    Ok(id) => {
                        self.fetching.insert(id, name);
                    }
                    Err(why) => self.kit.host.log(&format!("crates.io index: {why}")),
                }
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
        self.read_metadata();
    }

    /// An event for the pack itself: cargo's metadata, the index's
    /// answers, a fix of its own done.
    fn own(&mut self, event: &Event) -> bool {
        if self.reading.owns(event) {
            match self.reading.event(event) {
                Some((Ask::Sysroot, out)) => {
                    let sysroot = out.stdout.lines().map(str::trim).find(|l| !l.is_empty());
                    match sysroot.filter(|_| out.ok()) {
                        Some(path) => {
                            self.sysroot = Some(path.to_string());
                            self.apply();
                        }
                        None => self.kit.host.log(&format!("rustc --print sysroot: {}", doctor::error_line(&out))),
                    }
                }
                Some((Ask::Metadata, out)) => match Metadata::parse(&out) {
                    Ok(m) => {
                        self.metadata = Some(m);
                        self.configure();
                        let host = self.kit.host.as_mut();
                        if let Err(why) = self.kit.tasks.apply(host) {
                            host.log(&format!("tasks: {why}"));
                        }
                        // rust-version, from every package.
                        self.kit.doctor.run(host);
                        self.apply();
                        self.fetch_latest();
                    }
                    Err(why) => self.kit.host.log(&format!("cargo metadata: {why}")),
                },
                None => {}
            }
            return true;
        }
        match event {
            Event::HttpResponse((id, result)) if self.fetching.contains_key(id) => {
                let name = self.fetching.remove(id).unwrap_or_default();
                match result {
                    Ok(r) if r.status == 200 => {
                        if let Some(v) = packages::newest(&String::from_utf8_lossy(&r.body)) {
                            self.latest.borrow_mut().insert(name, v);
                        }
                    }
                    Ok(r) => self.kit.host.log(&format!("crates.io index: {name}: HTTP {}", r.status)),
                    Err(why) => self.kit.host.log(&format!("crates.io index: {name}: {why}")),
                }
                true
            }
            Event::TaskFinished((id, _)) if self.adding_server == Some(*id) => {
                self.adding_server = None;
                // The same command set again starts the server again.
                self.set_server = None;
                self.follow();
                self.apply();
                self.kit.doctor.run(self.kit.host.as_mut());
                true
            }
            Event::SettingChanged(key) if key == "rust.clippy" || key == "rust.debugger" => {
                self.configure();
                self.apply();
                true
            }
            _ => false,
        }
    }

    /// The pack's own commands: clippy now, and the fix rows lang-kit
    /// leaves to it.
    fn command(&mut self, id: &str) -> Result<(), String> {
        if id == "rust.clippy" {
            if !self.project.cargo {
                return Err("there's no Cargo.toml here".into());
            }
            // Off on save, it's linted now all the same.
            if !self.kit.lint.linters.iter().any(|l| l.name == clippy::NAME) {
                let workspace_root = self.metadata.as_ref().map(|m| m.workspace_root.clone()).unwrap_or_default();
                let project_root = self.project_root().unwrap_or_else(|| workspace_root.clone());
                self.kit.lint.add(clippy::linter(self.tools.cargo(), self.workspace(), &workspace_root, &project_root));
            }
            return self.kit.lint.run(self.kit.host.as_mut(), clippy::NAME, "Cargo.toml");
        }
        let host = self.kit.host.as_mut();
        match id {
            doctor::INSTALL_RUST_ANALYZER => {
                let cmd = self.tools.add_component("rust-analyzer").ok_or("rustup isn't installed: rust-analyzer comes with it")?;
                self.adding_server = Some(host.run_task("Install rust-analyzer", &cmd)?);
                Ok(())
            }
            doctor::INSTALL_LLDB_DAP => {
                host.open_text_beside("Installing lldb-dap", doctor::LLDB_DAP_HELP, None);
                Ok(())
            }
            _ => Err(format!("no command {id} here")),
        }
    }
}

impl Extension for Rust {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.read_metadata();
        self.read_sysroot();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            None => self.command(id),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        if self.own(&event) {
            return;
        }
        let finished = matches!(event, Event::TaskFinished(_));
        let saved = match &event {
            Event::Saved(name) => Some(name.replace('\\', "/")),
            _ => None,
        };
        self.kit.handle(&event);
        if let Some(name) = saved {
            let file = name.rsplit('/').next().unwrap_or(&name).to_string();
            if PROJECT_FILES.contains(&file.as_str()) {
                self.reload();
                if file.starts_with("rust-toolchain") {
                    self.read_sysroot();
                }
            }
        }
        if finished {
            self.follow();
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

    fn file_saved(&mut self, path: &str) {
        self.kit.file_saved(path);
    }
}

export!(Rust);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, METADATA};
    use fenix_extension_api::{Health, HttpResponse, ProcessOutput};
    use fenix_lang_kit::host::mock::Shared;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    fn workspace(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::write(dir.join("Cargo.toml"), "[workspace]\nmembers = [\"orbit\", \"orbit-core\"]\n\n[workspace.package]\nedition = \"2021\"\n").unwrap();
        dir
    }

    /// The pack, its host, and the index's URLs it asked for.
    fn pack(dir: &std::path::Path, host: &Shared, clippy: &'static str) -> (Rust, Rc<RefCell<Vec<String>>>) {
        let asked = Rc::new(RefCell::new(Vec::new()));
        let log = asked.clone();
        let get: Get = Box::new(move |url| {
            log.borrow_mut().push(url.to_string());
            Ok(1000 + log.borrow().len() as u64)
        });
        let setting: Setting = Box::new(move |key| (key == "rust.clippy").then(|| clippy.to_string()));
        let mut r = Rust::new(Box::new(host.clone()), &dir.to_string_lossy(), setting, get);
        if let Some(page) = r.kit.packages.as_mut() {
            page.cache_dir = None;
        }
        (r, asked)
    }

    fn said(r: &mut Rust, process: u64, text: &str, code: i32) {
        r.handle(Event::ProcessOutput(ProcessOutput { process, stderr: false, line: text.into() }));
        r.handle(Event::ProcessExit((process, Some(code))));
    }

    fn spawned(calls: &[String], what: &str) -> u64 {
        calls.iter().find_map(|c| c.strip_prefix("spawn ").filter(|c| c.ends_with(what)).and_then(|c| c.split(' ').next()?.parse().ok())).unwrap_or_else(|| panic!("{what}: {calls:?}"))
    }

    fn rustup_host() -> Shared {
        let host = Shared::default();
        host.0.borrow_mut().programs.extend(
            [("cargo", "/c/cargo"), ("rustup", "/c/rustup"), ("rustc", "/c/rustc"), ("rustfmt", "/c/rustfmt"), ("rust-analyzer", "/c/rust-analyzer"), ("lldb-dap-18", "/usr/bin/lldb-dap-18")].map(|(a, b)| (a.to_string(), b.to_string())),
        );
        host
    }

    #[test]
    fn a_workspace_gets_its_server_adapter_formatter_tasks_and_checks() {
        let dir = workspace("activate");
        let host = rustup_host();
        let (mut r, asked) = pack(&dir, &host, "true");
        r.activate().unwrap();
        let calls = host.take();
        assert_eq!(calls[0], "tasks build, test, clippy, fmt", "before cargo has said the targets");
        assert!(calls.contains(&"server rust: /c/rust-analyzer".to_string()), "{calls:?}");
        assert!(calls.contains(&"adapter rust: /usr/bin/lldb-dap-18".to_string()));
        assert!(calls.contains(&"formatter rust: /c/rustfmt --edition 2021".to_string()));
        spawned(&calls, "/c/rustc --version");
        let metadata = spawned(&calls, "/c/cargo metadata --format-version 1 --no-deps");
        assert_eq!(line(&host.0.borrow().tasks[0].cmd), "/c/cargo build --workspace");

        // cargo says the targets: the tasks, rust-version, the index.
        said(&mut r, metadata, METADATA, 0);
        let calls = host.take();
        assert!(calls.contains(&"tasks build, run orbit, run probe, example demo, build orbit, build probe, build example demo, test, clippy, fmt".to_string()), "{calls:?}");
        assert!(calls.contains(&"launches Rust: debug orbit, Rust: debug probe, Rust: debug example demo".to_string()), "{calls:?}");
        assert_eq!(host.0.borrow().launches[0].pre_launch_task.as_deref(), Some("build orbit"));
        assert_eq!(*asked.borrow(), ["https://index.crates.io/2/cc", "https://index.crates.io/cl/ap/clap", "https://index.crates.io/sm/al/smallvec", "https://index.crates.io/ti/me/time"]);
        let index = include_str!("samples/index-smallvec.jsonl");
        r.handle(Event::HttpResponse((1003, Ok(HttpResponse::new(200, index.as_bytes().to_vec())))));
        r.handle(Event::HttpResponse((1004, Err("timed out".into()))));
        assert_eq!(r.latest.borrow().get("smallvec").map(String::as_str), Some("1.16.4"));
        assert_eq!(host.take(), ["log crates.io index: time: timed out"]);
        let rustc = spawned(&calls, "/c/rustc --version");
        let checks: Vec<String> = calls.iter().filter(|c| c.starts_with("spawn")).cloned().collect();
        assert!(checks.iter().any(|c| c.ends_with("/c/cargo audit --version")), "{checks:?}");
        said(&mut r, rustc, "rustc 1.65.0 (897e37553 2022-11-02)", 0);
        let rust = r.kit.doctor.programs.iter().find(|p| p.label == "Rust").unwrap();
        assert_eq!(rust.wants.as_ref().map(|w| w.to_string()).as_deref(), Some(">=1.70"), "orbit's rust-version");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_a_rust_file_runs_clippy_and_cargo_toml_reads_again() {
        let dir = workspace("save");
        let host = rustup_host();
        let (mut r, _) = pack(&dir, &host, "true");
        r.activate().unwrap();
        let metadata = spawned(&host.take(), "--no-deps");
        said(&mut r, metadata, METADATA, 0);
        host.take();
        host.0.borrow_mut().here = Some((format!("{}/orbit-core/src/lib.rs", dir.display()), Some(dir.to_string_lossy().into_owned())));
        r.handle(Event::Saved("lib.rs".into()));
        let calls = host.take();
        let clippy = spawned(&calls, "/c/cargo clippy --message-format json --all-targets --workspace");
        for l in include_str!("samples/cargo-clippy.json").lines() {
            r.handle(Event::ProcessOutput(ProcessOutput { process: clippy, stderr: false, line: l.into() }));
        }
        r.handle(Event::ProcessExit((clippy, Some(0))));
        let calls = host.take();
        assert!(calls.contains(&"problems /project/orbit-core/src/lib.rs 1 (1 fixes)".to_string()), "the workspace is the project: {calls:?}");

        // Cargo.toml saved: read again, cargo asked again.
        std::fs::write(dir.join("rustfmt.toml"), "edition = \"2024\"\n").unwrap();
        r.handle(Event::Saved("Cargo.toml".into()));
        let calls = host.take();
        assert!(calls.contains(&"formatter rust: /c/rustfmt".to_string()), "rustfmt.toml says the edition now: {calls:?}");
        spawned(&calls, "--no-deps");
        assert!(!calls.iter().any(|c| c.contains("clippy --message")), "not linted");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rust_analyzers_fix_starts_the_server_again() {
        let dir = workspace("fix");
        let host = rustup_host();
        let (mut r, _) = pack(&dir, &host, "false");
        r.activate().unwrap();
        host.take();
        r.run_command(doctor::INSTALL_RUST_ANALYZER).unwrap();
        let calls = host.take();
        let task = calls.iter().find_map(|c| c.strip_prefix("task ").and_then(|c| c.split_once(' ')).filter(|(_, rest)| rest.ends_with("/c/rustup component add rust-analyzer")).map(|(id, _)| id.parse::<u64>().unwrap()));
        r.handle(Event::TaskFinished((task.expect("a task"), true)));
        let calls = host.take();
        assert!(calls.contains(&"server rust: /c/rust-analyzer".to_string()), "set again, so started again: {calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("spawn")), "checked again");

        // A fix that installs cargo-audit: the page audits with it, a task.
        r.run_command(doctor::INSTALL_CARGO_AUDIT).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").and_then(|c| c.split(' ').next()?.parse::<u64>().ok())).unwrap();
        host.0.borrow_mut().programs.insert("cargo-audit".into(), "/c/cargo-audit".into());
        r.handle(Event::TaskFinished((task, true)));
        assert!(host.take().iter().any(|c| c.starts_with("tasks ") && c.ends_with("audit")));
        assert_eq!(r.kit.packages.as_ref().and_then(|p| p.manager.audit()).map(|c| line(&c)).as_deref(), Some("/c/cargo audit --json"));

        // lldb-dap's row says how; clippy now, though it's off on save.
        r.run_command(doctor::INSTALL_LLDB_DAP).unwrap();
        assert_eq!(host.take(), ["text Installing lldb-dap"]);
        r.run_command("rust.clippy").unwrap();
        assert!(host.take().iter().any(|c| c.ends_with("clippy --message-format json --all-targets --workspace")));
        assert!(r.run_command("rust.nothing").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_anything_installed_fenixs_own_stay() {
        let dir = temp("bare");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"orbit\"\nedition = \"2024\"\n").unwrap();
        let host = Shared::default();
        let (mut r, _) = pack(&dir, &host, "true");
        r.activate().unwrap();
        let calls = host.take();
        assert!(!calls.iter().any(|c| c.starts_with("server") || c.starts_with("adapter") || c.starts_with("formatter")), "{calls:?}");
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(checks[0], ("Rust".into(), Health::Bad));
        assert!(r.kit.doctor.programs.iter().filter(|p| p.label != "lldb-dap").all(|p| p.fix.is_none()), "nothing to install with");
        assert!(r.run_command("rust.clippy").is_ok(), "clippy now");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn codelldb_is_connected_to_and_the_launches_load_rusts_formatters() {
        let dir = workspace("codelldb");
        let host = Shared::default();
        host.0.borrow_mut().programs.extend([("cargo".to_string(), "/c/cargo".to_string()), ("rustc".into(), "/c/rustc".into()), ("codelldb".into(), "/opt/codelldb/adapter/codelldb".into()), ("lldb-dap".into(), "/usr/bin/lldb-dap".into())]);
        let get: Get = Box::new(|_| Ok(1));
        let setting: Setting = Box::new(|key| (key == "rust.debugger").then(|| "codelldb".to_string()));
        let mut r = Rust::new(Box::new(host.clone()), &dir.to_string_lossy(), setting, get);
        r.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"connect rust: /opt/codelldb/adapter/codelldb --port ${port} :0".to_string()), "{calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("adapter")), "not lldb-dap");
        let sysroot = spawned(&calls, "/c/rustc --print sysroot");
        let metadata = spawned(&calls, "--no-deps");
        said(&mut r, metadata, METADATA, 0);
        assert!(!host.0.borrow().launches[0].attributes.contains("initCommands"), "rustc hasn't said where they are");
        said(&mut r, sysroot, "/home/u/.rustup/toolchains/1.82.0-x86_64-unknown-linux-gnu", 0);
        assert!(host.take().iter().any(|c| c.starts_with("launches ")), "set again");
        assert!(host.0.borrow().launches[0].attributes.contains("1.82.0-x86_64-unknown-linux-gnu/lib/rustlib/etc/lldb_lookup.py"));
        let checks: Vec<String> = r.kit.doctor.programs.iter().map(|p| p.label.clone()).collect();
        assert!(checks.contains(&"CodeLLDB".to_string()), "{checks:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
