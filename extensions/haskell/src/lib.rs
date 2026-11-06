//! Haskell in Fenix, on lang-kit: tree-sitter-haskell's grammar for
//! Fenix to highlight, fold and indent with (`languages/haskell.toml`);
//! the Haskell Language Server, told which formatter is in use and
//! whether hlint runs beside it; ormolu or fourmolu as the formatter;
//! hlint on save, its rewrites as fixes; Cabal's or Stack's tasks, a
//! task each for the components the packages name; and the project
//! doctor's checks, with fixes that run ghcup or `cabal install`.
//!
//! What it knows of the project is read from its files ([`project`]),
//! and what's installed is found ([`tools`]); the rest is lang-kit's
//! [`Kit`], given this pack's checks, tasks and linter. A fix that
//! installs a tool, a `.cabal` file or the project's saved, or a setting
//! changed, sets them again.

pub mod doctor;
pub mod hlint;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, MouseEvent, Page};
use fenix_lang_kit::serde_json::{json, Value};
use fenix_lang_kit::{Fenix, Host, Kit};

use project::Project;
use tools::{BuildTool, Formatter, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language: its language file's.
const HASKELL: &str = "haskell";

/// Files whose saving changes what the project is.
const PROJECT_FILES: &[&str] = &["cabal.project", "cabal.project.local", "stack.yaml", "package.yaml", "fourmolu.yaml"];

/// The pack's own command: the project read again.
pub const RELOAD: &str = "haskell.reload";

/// What the pack's settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `haskell.formatter`.
    pub formatter: Formatter,
    /// `haskell.hlint`: hlint on a saved file.
    pub hlint: bool,
    /// `haskell.build_tool`.
    pub build_tool: BuildTool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { formatter: Formatter::Auto, hlint: true, build_tool: BuildTool::Auto }
    }
}

impl Settings {
    /// From Fenix.
    pub fn read() -> Settings {
        let get = |key: &str| api::setting(key).unwrap_or_default();
        Settings { formatter: Formatter::parse(&get("haskell.formatter")), hlint: !get("haskell.hlint").trim().eq_ignore_ascii_case("false"), build_tool: BuildTool::parse(&get("haskell.build_tool")) }
    }
}

/// What HLS is told when it asks (`workspace/configuration`, its section
/// `haskell`): the formatter in use, for its own formatting; and, when
/// the pack runs hlint, its own hlint off, so a hint isn't said twice.
pub fn hls_settings(format: Formatter, hlint: bool) -> Value {
    json!({"haskell": {"formattingProvider": format.program(), "plugin": {"hlint": {"globalOn": !hlint}}}})
}

pub struct Haskell {
    pub kit: Kit,
    root: String,
    project: Project,
    tools: Tools,
    settings: Settings,
    read_settings: fn() -> Settings,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_settings: Option<Value>,
    set_formatter: Option<Command>,
}

impl Default for Haskell {
    fn default() -> Haskell {
        Haskell::new(Box::new(Fenix), PROJECT, Settings::read)
    }
}

impl Haskell {
    /// The pack for the project in `root`, talking to `host`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings) -> Haskell {
        let tools = Tools::find(host.as_mut());
        let kit = Kit::with_host("haskell", host);
        let mut h = Haskell { kit, root: root.into(), project: Project::read(root), tools, settings: read_settings(), read_settings, set_server: None, set_settings: None, set_formatter: None };
        h.configure();
        h
    }

    /// The build tool and the formatter, as the settings and the project
    /// make them.
    fn chosen(&self) -> (BuildTool, Formatter) {
        (Tools::build_tool(self.settings.build_tool, self.project.stack_project()), self.tools.formatter(self.settings.formatter, self.project.fourmolu_yaml))
    }

    /// Whether hlint lints saved files: it's on and installed.
    fn lints(&self) -> bool {
        self.settings.hlint && self.tools.hlint.is_some()
    }

    /// The kit's tasks, checks and linter, for the project, tools and
    /// settings as they are.
    fn configure(&mut self) {
        let (tool, format) = self.chosen();
        self.kit.tasks = tasks::tasks(&self.project, &self.tools, tool, &self.root, self.settings.hlint);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(&self.project, &self.tools, tool, format, self.settings.hlint) {
            self.kit.doctor.program(program);
        }
        self.kit.lint.linters.clear();
        if let (true, Some(hlint)) = (self.settings.hlint, &self.tools.hlint) {
            self.kit.lint.add(hlint::linter(&Command::new(hlint)));
        }
    }

    /// The server, its settings and the formatter set in Fenix, where they
    /// changed. Nothing found sets nothing: a grammar's language has no
    /// server of Fenix's own.
    fn apply(&mut self) {
        let (_, format) = self.chosen();
        let server = self.tools.server();
        let formatter = self.tools.format_command(format);
        let settings = server.as_ref().map(|_| hls_settings(if formatter.is_some() { format } else { Formatter::Off }, self.lints()));
        let languages = [HASKELL.to_string()];
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
            self.set_settings = None;
        }
        if settings.is_some() && settings != self.set_settings {
            if let Err(why) = host.language_server_settings(HASKELL, settings.as_ref()) {
                host.log(&format!("settings for HLS: {why}"));
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

    /// What the doctor found since: a tool a fix installed is used.
    fn follow(&mut self) {
        let mut tools = self.tools.clone();
        for program in &self.kit.doctor.programs {
            if let Some(slot) = tools.slot(&program.name) {
                *slot = self.kit.doctor.found.get(&program.name).cloned();
            }
        }
        self.take(tools);
    }

    fn take(&mut self, tools: Tools) {
        if tools != self.tools {
            self.tools = tools;
            self.configure();
            if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
                self.kit.host.log(&format!("tasks: {why}"));
            }
            self.apply();
        }
    }

    /// The tools as they're found now: what a task (ghcup's, `cabal
    /// install`'s) put there.
    fn find_again(&mut self) {
        let tools = Tools::find(self.kit.host.as_mut());
        if tools != self.tools {
            self.take(tools);
            self.kit.doctor.run(self.kit.host.as_mut());
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
    }
}

impl Extension for Haskell {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            None if id == RELOAD => {
                self.reload();
                Ok(())
            }
            None if id == doctor::GET_GHCUP => {
                self.kit.host.notify(&format!("ghcup installs GHC, Cabal, Stack and HLS: {}", tools::GHCUP_PAGE));
                self.kit.host.open_url(tools::GHCUP_PAGE)
            }
            None => Err(format!("no command {id} here")),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        let name = match &event {
            Event::Saved(name) => Some(name.replace('\\', "/")),
            _ => None,
        };
        // A package's or the project's file; a file at the top, without one.
        let saved = name.as_deref().is_some_and(|n| {
            let file = n.rsplit('/').next().unwrap_or_default();
            PROJECT_FILES.contains(&file) || file.ends_with(".cabal") || (self.project.packages.is_empty() && !n.contains('/') && n.ends_with(".hs"))
        });
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("haskell."));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        if saved {
            self.reload();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.reload();
        }
        if task_ended {
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

export!(Haskell);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{cabal_project, temp};
    use fenix_extension_api::Health;
    use fenix_lang_kit::host::mock::Shared;

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    fn found(host: &Shared, programs: &[(&str, &str)]) {
        host.0.borrow_mut().programs.extend(programs.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    }

    fn pack(dir: &std::path::Path, host: &Shared, settings: fn() -> Settings) -> Haskell {
        Haskell::new(Box::new(host.clone()), &dir.to_string_lossy(), settings)
    }

    fn said(h: &mut Haskell, process: u64, text: &str, code: i32) {
        for l in text.lines() {
            h.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: l.into() }));
        }
        h.handle(Event::ProcessExit((process, Some(code))));
    }

    /// The id a program was spawned with, by the end of its command line.
    fn spawned(calls: &[String], ending: &str) -> u64 {
        calls.iter().find_map(|c| c.strip_prefix("spawn ")?.split_once(' ').filter(|(_, l)| l.ends_with(ending))?.0.parse().ok()).unwrap_or_else(|| panic!("{ending}: {calls:?}"))
    }

    const GHCUP: &[(&str, &str)] = &[("ghc", "/g/ghc"), ("cabal", "/g/cabal"), ("ghcup", "/g/ghcup"), ("haskell-language-server-wrapper", "/g/haskell-language-server-wrapper"), ("hlint", "/c/hlint"), ("ormolu", "/c/ormolu")];

    #[test]
    fn a_cabal_package_gets_its_server_formatter_tasks_and_checks() {
        let dir = cabal_project("activate");
        let host = Shared::default();
        found(&host, GHCUP);
        let mut h = pack(&dir, &host, Settings::default);
        h.activate().unwrap();
        let calls = host.take();
        assert_eq!(calls[0], "tasks build, test, run orbit, run orbit-sim, test orbit-test, bench orbit-bench, update, clean, hlint");
        for want in ["server haskell: /g/haskell-language-server-wrapper --lsp", "formatter haskell: /c/ormolu --stdin-input-file ${file}"] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert_eq!(host.0.borrow().settings["haskell"], json!({"haskell": {"formattingProvider": "ormolu", "plugin": {"hlint": {"globalOn": false}}}}), "HLS's own hlint off: the pack's runs");
        assert_eq!(h.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["hlint"]);

        said(&mut h, spawned(&calls, "/g/ghc --numeric-version"), "9.10.2", 0);
        said(&mut h, spawned(&calls, "/g/cabal --numeric-version"), "3.14.2.0", 0);
        said(&mut h, spawned(&calls, "/g/haskell-language-server-wrapper --numeric-version"), "2.11.0.0", 0);
        said(&mut h, spawned(&calls, "/c/hlint --numeric-version"), "3.10", 0);
        said(&mut h, spawned(&calls, "/c/ormolu --version"), "ormolu 0.7.7.0 UNKNOWN UNKNOWN", 0);
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(checks, [("GHC".into(), Health::Ok), ("Cabal".into(), Health::Ok), ("Haskell Language Server".into(), Health::Ok), ("hlint".into(), Health::Ok), ("ormolu".into(), Health::Ok)]);

        // A Haskell file saved: hlint on it, its rewrites offered.
        host.take();
        host.0.borrow_mut().here = Some((format!("{}/src/Hints.hs", dir.display()), Some(dir.to_string_lossy().into_owned())));
        h.file_saved(&format!("{}/src/Hints.hs", dir.display()));
        let id = spawned(&host.take(), "/c/hlint --json --no-exit-code src/Hints.hs");
        said(&mut h, id, include_str!("samples/hlint.json"), 0);
        assert_eq!(host.take(), ["problems /project/src/Hints.hs 4 (4 fixes)"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_fix_runs_ghcup_and_what_it_installed_is_used() {
        let dir = cabal_project("fix");
        let host = Shared::default();
        found(&host, &[("ghc", "/g/ghc"), ("cabal", "/g/cabal"), ("ghcup", "/g/ghcup")]);
        let mut h = pack(&dir, &host, Settings::default);
        h.activate().unwrap();
        let calls = host.take();
        assert!(!calls.iter().any(|c| c.starts_with("server") || c.starts_with("formatter")), "nothing found, nothing set: {calls:?}");
        assert!(h.kit.lint.linters.is_empty(), "no hlint");
        h.run_command(doctor::INSTALL_HLS).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").and_then(|c| c.split_once(' ')).filter(|(_, rest)| rest.ends_with("/g/ghcup install hls recommended")).map(|(id, _)| id.parse::<u64>().unwrap()));
        found(&host, &[("haskell-language-server-wrapper", "/g/haskell-language-server-wrapper")]);
        h.handle(Event::TaskFinished((task.expect("installed as a task"), true)));
        let calls = host.take();
        assert!(calls.contains(&"server haskell: /g/haskell-language-server-wrapper --lsp".to_string()), "{calls:?}");
        assert_eq!(host.0.borrow().settings["haskell"]["haskell"]["formattingProvider"], "none", "no ormolu: HLS's own");

        // hlint installed by cabal: on at once.
        h.run_command(doctor::INSTALL_HLINT).unwrap();
        assert!(host.take().iter().any(|c| c.ends_with("/g/cabal install hlint --install-method=copy --overwrite-policy=always")));
        found(&host, &[("hlint", "/c/hlint")]);
        h.handle(Event::TaskFinished((99, true)));
        assert_eq!(h.kit.lint.linters.len(), 1);
        assert_eq!(host.0.borrow().settings["haskell"]["haskell"]["plugin"]["hlint"]["globalOn"], false);
        assert!(h.run_command("haskell.nothing").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_ghcup_the_fix_opens_its_page() {
        let dir = cabal_project("ghcup");
        let host = Shared::default();
        let mut h = pack(&dir, &host, Settings::default);
        h.activate().unwrap();
        host.take();
        h.run_command(doctor::GET_GHCUP).unwrap();
        assert_eq!(host.take().last().map(String::as_str), Some("url https://www.haskell.org/ghcup/install/"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stack_and_fourmolu_where_the_project_says() {
        let dir = temp("stack");
        std::fs::write(dir.join("package.yaml"), include_str!("samples/package.yaml")).unwrap();
        let host = Shared::default();
        found(&host, &[("stack", "/s/stack"), ("ormolu", "/c/ormolu"), ("fourmolu", "/c/fourmolu"), ("haskell-language-server-wrapper", "/g/hls")]);
        let mut h = pack(&dir, &host, Settings::default);
        h.activate().unwrap();
        let calls = host.take();
        assert!(calls.iter().any(|c| c.starts_with("tasks build, test, run orbit-exe, run orbit-sim, test orbit-test")), "package.yaml alone: Stack's, {calls:?}");
        assert_eq!(line(&h.kit.tasks.tasks[0].cmd), "/s/stack build");
        assert!(calls.contains(&"formatter haskell: /c/ormolu --stdin-input-file ${file}".to_string()), "{calls:?}");
        assert!(h.kit.doctor.programs.iter().any(|p| p.label == "Stack"));

        std::fs::write(dir.join("fourmolu.yaml"), "indentation: 2\n").unwrap();
        h.handle(Event::Saved("fourmolu.yaml".into()));
        assert!(host.take().contains(&"formatter haskell: /c/fourmolu --stdin-input-file ${file}".to_string()));
        assert_eq!(host.0.borrow().settings["haskell"]["haskell"]["formattingProvider"], "fourmolu");

        // Cabal asked for, and formatting and hlint off.
        h.read_settings = || Settings { build_tool: BuildTool::Cabal, formatter: Formatter::Off, hlint: false };
        h.handle(Event::SettingChanged("haskell.build_tool".into()));
        let calls = host.take();
        assert!(calls.contains(&"formatter haskell: ".to_string()), "taken away: {calls:?}");
        assert_eq!(line(&h.kit.tasks.tasks[0].cmd), "cabal build all");
        assert_eq!(host.0.borrow().settings["haskell"]["haskell"]["plugin"]["hlint"]["globalOn"], true, "the pack's hlint off: HLS's on");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_a_cabal_file_reads_the_project_again() {
        let dir = temp("reload");
        let host = Shared::default();
        found(&host, GHCUP);
        let mut h = pack(&dir, &host, Settings::default);
        h.activate().unwrap();
        assert!(h.kit.doctor.programs.is_empty() && h.kit.tasks.tasks.is_empty(), "not Haskell yet");
        host.take();
        std::fs::write(dir.join("hello.cabal"), "cabal-version: 3.0\nname: hello\nexecutable hello\n  main-is: Main.hs\n").unwrap();
        h.handle(Event::Saved("hello.cabal".into()));
        assert!(host.take().iter().any(|c| c == "tasks build, test, run hello, update, clean, hlint"));
        assert_eq!(h.kit.doctor.programs.len(), 5);
        h.run_command(RELOAD).unwrap();
        assert!(host.take().iter().any(|c| c.starts_with("doctor") || c.starts_with("spawn")), "checked again when you say");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
