//! OCaml in Fenix, on lang-kit: tree-sitter-ocaml's grammars for Fenix
//! to highlight, fold and indent with -- one for implementations (`.ml`,
//! `languages/ocaml.toml`) and one for interfaces (`.mli`,
//! `languages/ocaml_interface.toml`); ocaml-lsp-server as the language
//! server for both; ocamlformat as the formatter, in a project with an
//! `.ocamlformat`; dune's tasks, a `dune exec` for each executable its
//! `dune` files name; debugging bytecode with earlybird; and the project
//! doctor's checks, with fixes that run `opam install`.
//!
//! What it knows of the project is read from its files ([`project`]),
//! and what's installed is found ([`tools`]): directly, or through `opam
//! exec` in the project's switch once the doctor has seen it answer
//! there. The rest is lang-kit's [`Kit`], given this pack's checks and
//! tasks. A fix that installs a tool, a `dune` file or the project's
//! saved, or a setting changed, sets them again.

pub mod debug;
pub mod doctor;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Health, Key, LaunchConfig, MouseEvent, Page};
use fenix_lang_kit::{Fenix, Host, Kit};

use project::Project;
use tools::Tools;

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's names for the languages: its language files'.
const OCAML: &[&str] = &["ocaml", "ocaml_interface"];

/// Files whose saving changes what the project is.
const PROJECT_FILES: &[&str] = &["dune", "dune-project", "dune-workspace", ".ocamlformat"];

/// The pack's own command: the project read again.
pub const RELOAD: &str = "ocaml.reload";

/// What the pack's settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `ocaml.ocamlformat`: ocamlformat formats a project with an
    /// `.ocamlformat`.
    pub ocamlformat: bool,
    /// `ocaml.debug`: earlybird, and a launch for each bytecode
    /// executable.
    pub debug: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { ocamlformat: true, debug: true }
    }
}

impl Settings {
    /// From Fenix: a setting that isn't `false` is on.
    pub fn read() -> Settings {
        let on = |key: &str| !api::setting(key).is_some_and(|v| v.trim().eq_ignore_ascii_case("false"));
        Settings { ocamlformat: on("ocaml.ocamlformat"), debug: on("ocaml.debug") }
    }
}

/// What Fenix formats an OCaml buffer with: it on stdin (`-`), `${file}`
/// its name, for whether it's an interface and for the `.ocamlformat`
/// that applies.
pub fn formatter(ocamlformat: &Command) -> Command {
    ocamlformat.clone().args(["--name", "${file}", "-"])
}

pub struct OCaml {
    pub kit: Kit,
    root: String,
    project: Project,
    tools: Tools,
    settings: Settings,
    read_settings: fn() -> Settings,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_formatter: Option<Command>,
    set_adapter: Option<Command>,
    set_launches: Option<Vec<LaunchConfig>>,
}

impl Default for OCaml {
    fn default() -> OCaml {
        OCaml::new(Box::new(Fenix), PROJECT, Settings::read)
    }
}

impl OCaml {
    /// The pack for the project in `root`, talking to `host`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings) -> OCaml {
        let tools = Tools::find(host.as_mut());
        let kit = Kit::with_host("ocaml", host);
        let mut o = OCaml { kit, root: root.into(), project: Project::read(root), tools, settings: read_settings(), read_settings, set_server: None, set_formatter: None, set_adapter: None, set_launches: None };
        o.configure();
        o
    }

    /// Whether earlybird debugs: it's on and there.
    fn debugs(&self) -> bool {
        self.settings.debug && debug::adapter(&self.tools).is_some()
    }

    /// The kit's tasks and checks, for the project, tools and settings as
    /// they are.
    fn configure(&mut self) {
        self.kit.tasks = tasks::tasks(&self.project, &self.tools, &self.root, self.debugs());
        self.kit.doctor.programs.clear();
        for program in doctor::programs(&self.project, &self.tools, self.settings.ocamlformat, self.settings.debug) {
            self.kit.doctor.program(program);
        }
    }

    /// The server, the formatter, the adapter and its launches set in
    /// Fenix, where they changed. Nothing found sets nothing: a grammar's
    /// language has no server of Fenix's own.
    fn apply(&mut self) {
        let server = self.tools.command("ocamllsp");
        let formatter = self.tools.command("ocamlformat").filter(|_| self.settings.ocamlformat && self.project.ocamlformat).map(|c| formatter(&c));
        let adapter = debug::adapter(&self.tools).filter(|_| self.settings.debug);
        let launches = adapter.as_ref().map(|_| debug::launches(&self.project));
        let languages: Vec<String> = OCAML.iter().map(|l| l.to_string()).collect();
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
        }
        if formatter != self.set_formatter {
            if let Err(why) = host.formatter(&languages, formatter.as_ref()) {
                host.log(&format!("formatter: {why}"));
            }
            self.set_formatter = formatter;
        }
        if adapter != self.set_adapter {
            if let Err(why) = host.debug_adapter(&languages[..1], adapter.as_ref()) {
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
    }

    /// What the doctor found since: a tool a fix installed is used, and
    /// one that answered through `opam exec` runs that way.
    fn follow(&mut self) {
        let mut tools = self.tools.clone();
        for program in &self.kit.doctor.programs {
            if let Some(slot) = tools.slot(&program.name) {
                *slot = self.kit.doctor.found.get(&program.name).cloned();
            }
        }
        let checks = self.kit.doctor.checks();
        for program in self.kit.doctor.programs.iter().filter(|p| p.name == "opam") {
            let (Some(tool), Some(check)) = (tools::by_label(&program.label), checks.iter().find(|c| c.label == program.label)) else { continue };
            if check.health == Health::Ok {
                tools.in_switch.insert(tool.program.to_string());
            } else {
                tools.in_switch.remove(tool.program);
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

    /// The tools as they're found now: what a task (`opam install`'s)
    /// put there; checked again, for what's in the switch.
    fn find_again(&mut self) {
        let mut tools = Tools::find(self.kit.host.as_mut());
        tools.in_switch = self.tools.in_switch.clone();
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

impl Extension for OCaml {
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
            None if id == doctor::GET_OPAM => {
                self.kit.host.notify(&format!("opam installs OCaml, dune and the rest: {}", tools::OPAM_PAGE));
                self.kit.host.open_url(tools::OPAM_PAGE)
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
        // dune's files, anywhere; an `.opam` file; a file at the top,
        // without dune.
        let saved = name.as_deref().is_some_and(|n| {
            let file = n.rsplit('/').next().unwrap_or_default();
            PROJECT_FILES.contains(&file) || file.ends_with(".opam") || (!self.project.is_dune() && !n.contains('/') && n.ends_with(".ml"))
        });
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("ocaml."));
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

export!(OCaml);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{dune_project, temp};
    use fenix_lang_kit::host::mock::Shared;

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    fn found(host: &Shared, programs: &[(&str, &str)]) {
        host.0.borrow_mut().programs.extend(programs.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    }

    fn pack(dir: &std::path::Path, host: &Shared, settings: fn() -> Settings) -> OCaml {
        OCaml::new(Box::new(host.clone()), &dir.to_string_lossy(), settings)
    }

    fn said(o: &mut OCaml, process: u64, text: &str, code: i32) {
        for l in text.lines() {
            o.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: l.into() }));
        }
        o.handle(Event::ProcessExit((process, Some(code))));
    }

    /// The id a program was spawned with, by the end of its command line.
    fn spawned(calls: &[String], ending: &str) -> u64 {
        calls.iter().find_map(|c| c.strip_prefix("spawn ")?.split_once(' ').filter(|(_, l)| l.ends_with(ending))?.0.parse().ok()).unwrap_or_else(|| panic!("{ending}: {calls:?}"))
    }

    #[test]
    fn in_opams_switch_each_tool_runs_through_opam_once_its_seen() {
        let dir = dune_project("switch");
        let host = Shared::default();
        found(&host, &[("opam", "/o/opam")]);
        let mut o = pack(&dir, &host, Settings::default);
        o.activate().unwrap();
        let calls = host.take();
        assert!(calls[0].starts_with("tasks build, test, exec orbit, exec sim, exec replay, fmt, clean, opam: install dependencies"), "{calls:?}");
        assert_eq!(line(&o.kit.tasks.tasks[0].cmd), "/o/opam exec -- dune build");
        assert!(!calls.iter().any(|c| c.starts_with("server") || c.starts_with("formatter") || c.starts_with("adapter")), "not yet seen in the switch: {calls:?}");

        said(&mut o, spawned(&calls, "/o/opam --version"), "2.3.0", 0);
        said(&mut o, spawned(&calls, "/o/opam exec -- ocaml -vnum"), "5.3.0", 0);
        said(&mut o, spawned(&calls, "/o/opam exec -- dune --version"), "3.20.2", 0);
        said(&mut o, spawned(&calls, "/o/opam exec -- ocamllsp --version"), "1.23.0", 0);
        said(&mut o, spawned(&calls, "/o/opam exec -- ocamlformat --version"), "0.27.0", 0);
        said(&mut o, spawned(&calls, "/o/opam exec -- ocamlearlybird --version"), "1.3.4", 0);
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert!(checks.iter().all(|(_, h)| *h == Health::Ok), "{checks:?}");
        let calls = host.take();
        for want in [
            "server ocaml,ocaml_interface: /o/opam exec -- ocamllsp",
            "formatter ocaml,ocaml_interface: /o/opam exec -- ocamlformat --name ${file} -",
            "adapter ocaml: /o/opam exec -- ocamlearlybird debug",
            "launches OCaml: debug orbit",
        ] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert!(o.kit.tasks.tasks.iter().any(|t| t.name == "build bin/main.bc"), "the launch's task");
        assert_eq!(host.0.borrow().launches[0].pre_launch_task.as_deref(), Some("build bin/main.bc"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_fix_runs_opam_install_and_what_it_installed_is_used() {
        let dir = dune_project("fix");
        let host = Shared::default();
        found(&host, &[("opam", "/o/opam"), ("dune", "/s/dune"), ("ocaml", "/s/ocaml")]);
        let mut o = pack(&dir, &host, Settings::default);
        o.activate().unwrap();
        let calls = host.take();
        said(&mut o, spawned(&calls, "/o/opam --version"), "2.3.0", 0);
        said(&mut o, spawned(&calls, "/s/ocaml -vnum"), "5.3.0", 0);
        said(&mut o, spawned(&calls, "/s/dune --version"), "3.20.2", 0);
        said(&mut o, spawned(&calls, "/o/opam exec -- ocamllsp --version"), "", 1);
        said(&mut o, spawned(&calls, "/o/opam exec -- ocamlformat --version"), "0.27.0", 0);
        said(&mut o, spawned(&calls, "/o/opam exec -- ocamlearlybird --version"), "", 1);
        let lsp = host.0.borrow().checks.iter().find(|c| c.label == "ocaml-lsp-server").cloned().unwrap();
        assert_eq!((lsp.health, lsp.fix.map(|f| f.1)), (Health::Warn, Some(doctor::INSTALL_LSP.into())));
        o.run_command(doctor::INSTALL_LSP).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").and_then(|c| c.split_once(' ')).filter(|(_, rest)| rest.ends_with("/o/opam install ocaml-lsp-server --yes")).map(|(id, _)| id.parse::<u64>().unwrap()));
        // The fix's task ends: checked again, and seen in the switch.
        o.handle(Event::TaskFinished((task.expect("installed as a task"), true)));
        let calls = host.take();
        said(&mut o, spawned(&calls, "/o/opam exec -- ocamllsp --version"), "1.23.0", 0);
        assert!(host.take().contains(&"server ocaml,ocaml_interface: /o/opam exec -- ocamllsp".to_string()));

        // Installed some other way, on PATH: used by its path.
        found(&host, &[("ocamllsp", "/s/ocamllsp")]);
        o.handle(Event::TaskFinished((99, true)));
        assert!(host.take().contains(&"server ocaml,ocaml_interface: /s/ocamllsp".to_string()));
        assert!(o.run_command("ocaml.nothing").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_opam_the_fix_opens_its_page() {
        let dir = dune_project("page");
        let host = Shared::default();
        let mut o = pack(&dir, &host, Settings::default);
        o.activate().unwrap();
        host.take();
        o.run_command(doctor::GET_OPAM).unwrap();
        assert_eq!(host.take().last().map(String::as_str), Some("url https://opam.ocaml.org/doc/Install.html"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ocamlformat_only_where_the_project_has_its_config() {
        let dir = dune_project("format");
        std::fs::remove_file(dir.join(".ocamlformat")).unwrap();
        let host = Shared::default();
        found(&host, &[("dune", "/s/dune"), ("ocaml", "/s/ocaml"), ("ocamllsp", "/s/ocamllsp"), ("ocamlformat", "/s/ocamlformat")]);
        let mut o = pack(&dir, &host, Settings::default);
        o.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"server ocaml,ocaml_interface: /s/ocamllsp".to_string()), "{calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("formatter")), "no .ocamlformat: {calls:?}");
        assert!(!o.kit.doctor.programs.iter().any(|p| p.label == "ocamlformat"));

        std::fs::write(dir.join(".ocamlformat"), "profile = default\n").unwrap();
        o.handle(Event::Saved(".ocamlformat".into()));
        assert!(host.take().contains(&"formatter ocaml,ocaml_interface: /s/ocamlformat --name ${file} -".to_string()));

        // Off: taken away; and no debugging.
        o.read_settings = || Settings { ocamlformat: false, debug: false };
        o.handle(Event::SettingChanged("ocaml.ocamlformat".into()));
        assert!(host.take().contains(&"formatter ocaml,ocaml_interface: ".to_string()));
        assert!(!o.kit.doctor.programs.iter().any(|p| p.label == "earlybird"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_a_dune_file_reads_the_project_again() {
        let dir = temp("reload");
        let host = Shared::default();
        found(&host, &[("dune", "/s/dune")]);
        let mut o = pack(&dir, &host, Settings::default);
        o.activate().unwrap();
        assert!(o.kit.tasks.tasks.is_empty() && o.kit.doctor.programs.is_empty(), "not OCaml yet");
        host.take();
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("dune-project"), "(lang dune 3.0)\n").unwrap();
        std::fs::write(dir.join("bin/dune"), "(executable (public_name hello) (name main))\n").unwrap();
        o.handle(Event::Saved("bin/dune".into()));
        assert!(host.take().iter().any(|c| c == "tasks build, test, exec hello, fmt, clean"));
        assert!(o.kit.doctor.programs.iter().any(|p| p.label == "dune"));
        o.run_command(RELOAD).unwrap();
        assert!(host.take().iter().any(|c| c.starts_with("spawn") || c.starts_with("doctor")), "checked again when you say");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
