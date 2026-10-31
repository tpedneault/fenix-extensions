//! Go in Fenix, on lang-kit: gopls as the language server, Delve's DAP
//! server as the debug adapter (over TCP), go vet or golangci-lint on
//! save, gofmt or goimports as the formatter, the tasks, the project
//! doctor's checks, and the package manager page for Go modules.
//!
//! What it knows of the project is read from its files ([`project`]),
//! and what's installed is found ([`tools`]); the rest is lang-kit's
//! [`Kit`], given this pack's package manager, checks, tasks and linter.
//! A fix that installs a tool, a `go.mod` saved or changed by a task, or
//! a setting changed, sets them again.

pub mod doctor;
pub mod lint;
pub mod packages;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, MouseEvent, Page};
use fenix_lang_kit::{find_version, Fenix, Host, Kit, PackageManager};

use project::Project;
use tools::{Formatter, LintWith, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language.
const GO: &[&str] = &["go"];

/// Files whose saving changes what the project is.
const PROJECT_FILES: &[&str] = &["go.mod", "go.work", ".golangci.yml", ".golangci.yaml", ".golangci.toml", ".golangci.json"];

/// What the pack's settings say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub linter: LintWith,
    pub formatter: Formatter,
    /// `go.delve_server`'s port: debug through the Delve running headless
    /// there, not one Fenix starts.
    pub delve_server: Option<u16>,
}

impl Settings {
    /// From Fenix.
    pub fn read() -> Settings {
        let setting = |key: &str| api::setting(key).unwrap_or_default();
        Settings {
            linter: LintWith::parse(&setting("go.linter")),
            formatter: Formatter::parse(&setting("go.formatter")),
            delve_server: parse_server(&setting("go.delve_server")).unwrap_or(None),
        }
    }
}

/// `go.delve_server`'s port: `2345`, `:2345`, `127.0.0.1:2345` or
/// `localhost:2345`. Fenix connects to an adapter on this machine only.
pub fn parse_server(text: &str) -> Result<Option<u16>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let (host, port) = text.rsplit_once(':').unwrap_or(("", text));
    if !matches!(host, "" | "127.0.0.1" | "localhost") {
        return Err(format!("{text}: Fenix connects to a debug adapter on 127.0.0.1 only"));
    }
    port.parse::<u16>().ok().filter(|p| *p > 0).map(Some).ok_or_else(|| format!("{text}: not a port"))
}

/// How Fenix talks to Delve: a command it starts, on the port it picks
/// (`${port}`), or none for one already running at `port`.
#[derive(Debug, Clone, PartialEq)]
pub struct Adapter {
    pub cmd: Option<Command>,
    pub port: u16,
}

/// Delve: `dlv dap` started for each session, listening where Fenix
/// says; or the headless Delve `go.delve_server` names.
pub fn adapter(tools: &Tools, settings: &Settings) -> Option<Adapter> {
    if let Some(port) = settings.delve_server {
        return Some(Adapter { cmd: None, port });
    }
    tools.dlv.as_ref().map(|dlv| Adapter { cmd: Some(Command::new(dlv).args(["dap", "--listen", "127.0.0.1:${port}"])), port: 0 })
}

/// The package manager for the project in `root`.
pub fn manager(tools: &Tools, root: &str) -> Box<dyn PackageManager> {
    Box::new(packages::GoModules { go: tools.go(), govulncheck: tools.govulncheck.as_deref().map(Command::new), root: root.into() })
}

/// `debug-adapter-connect`, which lang-kit's [`Host`] doesn't have.
pub type Connect = Box<dyn FnMut(&[String], Option<&Command>, u16) -> Result<(), String>>;

pub struct Go {
    pub kit: Kit,
    root: String,
    project: Project,
    tools: Tools,
    settings: Settings,
    read_settings: fn() -> Settings,
    connect: Connect,
    /// go.mod as it was read: a task that changes it (`go get`) reads
    /// the project again.
    go_mod: Option<String>,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_adapter: Option<Adapter>,
    set_formatter: Option<Command>,
}

impl Default for Go {
    fn default() -> Go {
        let connect: Connect = Box::new(|languages, cmd, port| {
            let languages: Vec<&str> = languages.iter().map(String::as_str).collect();
            api::project::debug_adapter_connect(&languages, cmd, port)
        });
        Go::new(Box::new(Fenix), PROJECT, Settings::read, connect, &api::platform().os)
    }
}

impl Go {
    /// The pack for the project in `root`, talking to `host`, on `os`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings, connect: Connect, os: &str) -> Go {
        let mut tools = Tools::find(host.as_mut());
        tools.os = os.into();
        let kit = Kit::with_host("go", host).packages(manager(&tools, root));
        let mut g = Go {
            kit,
            root: root.into(),
            project: Project::read(root),
            tools,
            settings: read_settings(),
            read_settings,
            connect,
            go_mod: std::fs::read_to_string(format!("{root}/go.mod")).ok(),
            set_server: None,
            set_adapter: None,
            set_formatter: None,
        };
        g.configure();
        g
    }

    /// The kit's tasks, checks and linter, for the project, tools and
    /// settings as they are.
    fn configure(&mut self) {
        let (project, tools) = (&self.project, &self.tools);
        self.kit.tasks = tasks::tasks(project, tools, &self.root);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(project, tools) {
            self.kit.doctor.program(program);
        }
        let linter = match (self.settings.linter, &tools.golangci_lint) {
            (LintWith::Off, _) => None,
            (LintWith::GolangciLint, Some(golangci)) => Some(lint::golangci(&Command::new(golangci), tools.golangci_v1)),
            _ => Some(lint::vet(&tools.go())),
        };
        let name = linter.as_ref().map(|l| l.name.clone());
        self.kit.lint.linters.retain(|l| Some(&l.name) == name.as_ref());
        if let Some(l) = linter {
            self.kit.lint.add(l);
        }
        if let Some(page) = self.kit.packages.as_mut() {
            page.manager = manager(tools, &self.root);
        }
    }

    /// The server, the adapter and the formatter set in Fenix, where they
    /// changed. Nothing found leaves Fenix's own.
    fn apply(&mut self) {
        let server = self.tools.gopls.as_deref().map(Command::new);
        let adapter = adapter(&self.tools, &self.settings);
        let formatter = self.tools.formatter(self.settings.formatter);
        let languages: Vec<String> = GO.iter().map(|l| l.to_string()).collect();
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
        }
        if adapter != self.set_adapter {
            let done = match &adapter {
                Some(a) => (self.connect)(&languages, a.cmd.as_ref(), a.port),
                None => host.debug_adapter(&languages, None),
            };
            if let Err(why) = done {
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

    /// What the doctor found since: a tool a fix installed is used, and
    /// golangci-lint's version says its flags.
    fn follow(&mut self) {
        let checked = |name: &str| self.kit.doctor.programs.iter().any(|p| p.name == name);
        let found = |name: &str| self.kit.doctor.found.get(name).cloned();
        let mut tools = self.tools.clone();
        for (name, slot) in [("go", &mut tools.go), ("gopls", &mut tools.gopls), ("dlv", &mut tools.dlv), ("govulncheck", &mut tools.govulncheck), ("golangci-lint", &mut tools.golangci_lint)] {
            if checked(name) {
                *slot = found(name);
            }
        }
        if let Some(check) = self.kit.doctor.checks().into_iter().find(|c| c.label == "golangci-lint") {
            if let Some(v) = find_version(&check.detail) {
                tools.golangci_v1 = v.major() < 2;
            }
        }
        if tools != self.tools {
            self.tools = tools;
            self.configure();
            if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
                self.kit.host.log(&format!("tasks: {why}"));
            }
            self.apply();
        }
    }

    /// The project's files changed: read again, and checked again.
    fn reload(&mut self) {
        self.project = Project::read(&self.root);
        self.go_mod = std::fs::read_to_string(format!("{}/go.mod", self.root)).ok();
        self.configure();
        let host = self.kit.host.as_mut();
        if let Err(why) = self.kit.tasks.apply(host) {
            host.log(&format!("tasks: {why}"));
        }
        self.kit.doctor.run(host);
        self.apply();
    }
}

impl Extension for Go {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            // Tidy without go.mod's check: go says what's wrong.
            None if id == doctor::TIDY => self.kit.host.run_task("go mod tidy", &self.tools.go().args(["mod", "tidy"])).map(|_| ()),
            None => Err(format!("no command {id} here")),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        let saved = matches!(&event, Event::Saved(name) if PROJECT_FILES.iter().any(|f| name.replace('\\', "/").rsplit('/').next() == Some(f)));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("go."));
        self.kit.handle(&event);
        if saved || (task_ended && std::fs::read_to_string(format!("{}/go.mod", self.root)).ok() != self.go_mod) {
            self.reload();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.configure();
            self.apply();
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

export!(Go);

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

    fn module(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::write(dir.join("go.mod"), "module example.com/orbit\n\ngo 1.22\n\nrequire golang.org/x/text v0.3.5\n").unwrap();
        std::fs::write(dir.join("main.go"), "package main\n\nfunc main() {}\n").unwrap();
        dir
    }

    fn defaults() -> Settings {
        Settings::default()
    }

    fn headless() -> Settings {
        Settings { delve_server: Some(2345), ..Settings::default() }
    }

    /// The pack, its `debug-adapter-connect` calls written down with the
    /// host's.
    fn pack(dir: &std::path::Path, host: &Shared, settings: fn() -> Settings) -> Go {
        let calls = host.clone();
        let connect: Connect = Box::new(move |languages, cmd, port| {
            calls.0.borrow_mut().calls.push(format!("connect {}: {} :{port}", languages.join(","), cmd.map(line).unwrap_or_default()));
            Ok(())
        });
        let mut g = Go::new(Box::new(host.clone()), &dir.to_string_lossy(), settings, connect, "linux");
        if let Some(page) = g.kit.packages.as_mut() {
            page.cache_dir = None;
        }
        g
    }

    fn said(g: &mut Go, process: u64, text: &str) {
        g.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: text.into() }));
        g.handle(Event::ProcessExit((process, Some(0))));
    }

    fn found(host: &Shared, programs: &[(&str, &str)]) {
        host.0.borrow_mut().programs.extend(programs.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    }

    #[test]
    fn a_module_gets_its_server_adapter_tasks_and_checks() {
        let dir = module("activate");
        let host = Shared::default();
        found(&host, &[("go", "/usr/local/go/bin/go"), ("gofmt", "/usr/local/go/bin/gofmt"), ("gopls", "/h/go/bin/gopls"), ("dlv", "/h/go/bin/dlv")]);
        let mut g = pack(&dir, &host, defaults);
        g.activate().unwrap();
        let calls = host.take();
        assert_eq!(calls[0], "tasks build, run, test, vet, format, tidy");
        assert!(calls.contains(&"spawn 1 /usr/local/go/bin/go version".to_string()), "{calls:?}");
        assert!(calls.contains(&"spawn 2 /usr/local/go/bin/go mod tidy -diff".to_string()), "{calls:?}");
        assert!(calls.contains(&"server go: /h/go/bin/gopls".to_string()));
        assert!(calls.contains(&"connect go: /h/go/bin/dlv dap --listen 127.0.0.1:${port} :0".to_string()), "{calls:?}");
        assert!(calls.contains(&"formatter go: /usr/local/go/bin/gofmt".to_string()), "no goimports: gofmt");
        assert_eq!(line(&host.0.borrow().tasks[1].cmd), "/usr/local/go/bin/go run .");

        said(&mut g, 1, "go version go1.24.7 linux/amd64");
        said(&mut g, 2, "");
        said(&mut g, 3, "golang.org/x/tools/gopls v0.24.0");
        said(&mut g, 4, "Delve Debugger\nVersion: 1.27.2");
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(checks, [("Go".into(), Health::Ok), ("Modules".into(), Health::Ok), ("gopls".into(), Health::Ok), ("Delve".into(), Health::Ok), ("govulncheck".into(), Health::Warn)]);
        assert_eq!(host.0.borrow().checks[4].fix, Some(("Install govulncheck".into(), doctor::INSTALL_GOVULNCHECK.into())));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_tool_installed_by_its_fix_row_is_used() {
        let dir = module("fix");
        std::fs::write(dir.join(".golangci.yml"), "version: \"2\"\n").unwrap();
        let host = Shared::default();
        found(&host, &[("go", "/g/go"), ("gofmt", "/g/gofmt")]);
        let mut g = pack(&dir, &host, defaults);
        g.activate().unwrap();
        assert!(!host.take().iter().any(|c| c.starts_with("connect") || c.starts_with("adapter")), "no Delve: Fenix's own");
        g.run_command(doctor::INSTALL_DELVE).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").and_then(|c| c.split_once(' ')).filter(|(_, rest)| rest.ends_with("/g/go install github.com/go-delve/delve/cmd/dlv@latest")).map(|(id, _)| id.parse::<u64>().unwrap()));
        let task = task.expect("installed as a task");
        found(&host, &[("dlv", "/h/go/bin/dlv"), ("golangci-lint", "/h/go/bin/golangci-lint")]);
        g.handle(Event::TaskFinished((task, true)));
        let calls = host.take();
        assert!(calls.contains(&"connect go: /h/go/bin/dlv dap --listen 127.0.0.1:${port} :0".to_string()), "{calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("tasks ")), "the tasks follow");
        assert_eq!(g.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["golangci-lint"]);

        // golangci-lint 1.x says so, and its flags are 1.x's.
        let spawned = calls.iter().find_map(|c| c.strip_prefix("spawn ").and_then(|c| c.split_once(' ')).filter(|(_, rest)| rest.ends_with("golangci-lint --version")).map(|(id, _)| id.parse::<u64>().unwrap()));
        let ids: Vec<u64> = calls.iter().filter_map(|c| c.strip_prefix("spawn ").and_then(|c| c.split_once(' ')).map(|(id, _)| id.parse().unwrap())).collect();
        for id in ids {
            let text = if Some(id) == spawned { "golangci-lint has version v1.64.8 built with go1.24.1 from 8b37f141 on 2025-03-17T20:41:53Z" } else { "x 1.0.0" };
            said(&mut g, id, text);
        }
        host.0.borrow_mut().here = Some((format!("{}/internal/store/store.go", dir.display()), Some(dir.to_string_lossy().into_owned())));
        g.handle(Event::Saved("store.go".into()));
        let calls = host.take();
        assert!(calls.iter().any(|c| c.ends_with("/h/go/bin/golangci-lint run --out-format json --print-issued-lines=false ./internal/store")), "{calls:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn go_mod_changed_reads_the_project_again() {
        let dir = module("reload");
        let host = Shared::default();
        found(&host, &[("go", "/g/go")]);
        let mut g = pack(&dir, &host, defaults);
        g.activate().unwrap();
        host.take();
        std::fs::remove_file(dir.join("main.go")).unwrap();
        std::fs::create_dir_all(dir.join("cmd/orbitctl")).unwrap();
        std::fs::write(dir.join("cmd/orbitctl/main.go"), "package main\n").unwrap();
        std::fs::write(dir.join("go.mod"), "module example.com/orbit\n\ngo 1.23\n").unwrap();
        g.handle(Event::Saved("go.mod".into()));
        let calls = host.take();
        assert!(calls.contains(&"tasks build, run, test, vet, format, tidy".to_string()), "{calls:?}");
        assert_eq!(line(&host.0.borrow().tasks[1].cmd), "/g/go run ./cmd/orbitctl");
        assert!(calls.iter().any(|c| c.starts_with("spawn")), "checked again");

        // A task (the page's go get) that changes go.mod: read again too.
        g.handle(Event::TaskFinished((99, true)));
        assert!(!host.take().iter().any(|c| c.starts_with("tasks")), "go.mod is as it was");
        std::fs::write(dir.join("go.mod"), "module example.com/orbit\n\ngo 1.23\n\nrequire golang.org/x/text v0.43.0\n").unwrap();
        g.handle(Event::TaskFinished((100, true)));
        assert!(host.take().iter().any(|c| c.starts_with("tasks")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_headless_delve_is_connected_to() {
        let dir = module("headless");
        let host = Shared::default();
        found(&host, &[("go", "/g/go"), ("dlv", "/h/dlv"), ("gofmt", "/g/gofmt"), ("goimports", "/h/goimports")]);
        let mut g = pack(&dir, &host, defaults);
        g.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"formatter go: /h/goimports -srcdir ${file}".to_string()), "{calls:?}");
        g.read_settings = headless;
        g.handle(Event::SettingChanged("go.delve_server".into()));
        assert!(host.take().contains(&"connect go:  :2345".to_string()), "no command: the one running");
        g.read_settings = || Settings { linter: LintWith::Off, formatter: Formatter::Gofmt, delve_server: None };
        g.handle(Event::SettingChanged("go.formatter".into()));
        let calls = host.take();
        assert!(calls.contains(&"formatter go: /g/gofmt".to_string()) && calls.contains(&"connect go: /h/dlv dap --listen 127.0.0.1:${port} :0".to_string()), "{calls:?}");
        assert!(g.kit.lint.linters.is_empty(), "off");
        assert!(g.run_command("go.nothing").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn delve_server_is_a_local_port() {
        assert_eq!(parse_server(""), Ok(None));
        assert_eq!(parse_server("2345"), Ok(Some(2345)));
        assert_eq!(parse_server(":4000"), Ok(Some(4000)));
        assert_eq!(parse_server("127.0.0.1:2345"), Ok(Some(2345)));
        assert_eq!(parse_server("localhost:2345"), Ok(Some(2345)));
        assert!(parse_server("build.lan:2345").unwrap_err().contains("127.0.0.1 only"));
        assert!(parse_server("localhost:x").is_err() && parse_server("0").is_err());
        assert_eq!(adapter(&Tools::default(), &defaults()), None);
    }

    #[test]
    fn without_go_mod_tidy_still_runs() {
        let dir = temp("bare");
        let host = Shared::default();
        let mut g = pack(&dir, &host, defaults);
        assert_eq!(g.kit.packages.as_ref().map(|p| p.manager.name().to_string()).as_deref(), Some("go"));
        g.activate().unwrap();
        host.take();
        g.run_command(doctor::TIDY).unwrap();
        assert!(host.take().iter().any(|c| c.ends_with("go mod tidy: go mod tidy")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
