//! Ada and SPARK in Fenix, on lang-kit: the grammar (its language files
//! and queries), AdaCore's Ada Language Server for Ada and for GNAT's
//! project files, which also formats (GNATformat, inside it), Alire's or
//! gprbuild's tasks, GNATprove's findings as problems, and the project
//! doctor's checks, each with a fix row that installs what's missing:
//! Alire's and the server's releases, and through Alire, GNAT, gprbuild
//! and GNATprove.
//!
//! What it knows of the project is read from its files ([`project`]),
//! and what's installed is found ([`tools`]). In an Alire crate the
//! server runs in the crate's environment (`alr exec`), where its
//! dependencies' project files and Alire's GNAT are. A fix that installs
//! a tool, a setting changed, or `alire.toml` or a project file saved
//! sets them again.

pub mod doctor;
pub mod project;
pub mod prove;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, MouseEvent, Page};
use fenix_lang_kit::serde_json::{json, Value};
use fenix_lang_kit::{Fenix, Host, Kit};

use project::Project;
use tools::{Platform, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's names for the languages, the language files': Ada, and GNAT's
/// project files.
const ADA: &str = "ada";
const GPR: &str = "gpr";

/// The proof level when the setting says none: GNATprove's levels go
/// from 0, the quickest, to 4.
pub const PROVE_LEVEL: u8 = 2;

/// What the pack's settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The project file chosen, under the project; empty for the crate's
    /// or the only one.
    pub project_file: String,
    /// GNATprove's `--level`.
    pub prove_level: u8,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { project_file: String::new(), prove_level: PROVE_LEVEL }
    }
}

impl Settings {
    /// From Fenix: a level that isn't 0 to 4 is the default.
    pub fn read() -> Settings {
        let level = api::setting("ada.proveLevel").and_then(|v| v.trim().parse::<u8>().ok()).filter(|l| *l <= 4).unwrap_or(PROVE_LEVEL);
        Settings { project_file: api::setting("ada.projectFile").unwrap_or_default(), prove_level: level }
    }
}

/// What the server is told (`ada.projectFile`, its own setting's name):
/// the project file, when there's one to use.
pub fn als_settings(gpr: Option<&str>) -> Option<Value> {
    gpr.map(|gpr| json!({"ada": {"projectFile": gpr}}))
}

pub struct Ada {
    pub kit: Kit,
    root: String,
    project: Project,
    tools: Tools,
    platform: Platform,
    settings: Settings,
    read_settings: fn() -> Settings,
    /// The project file the server and tasks use, or why there's none.
    gpr: Result<Option<String>, String>,
    /// What's set in Fenix, so it's set again only when it changes.
    set_servers: (Option<Command>, Option<Command>),
    set_settings: Option<Value>,
}

impl Default for Ada {
    fn default() -> Ada {
        let platform = api::platform();
        Ada::new(Box::new(Fenix), PROJECT, Settings::read, Platform::new(&platform.os, &platform.arch))
    }
}

impl Ada {
    /// The pack for the project in `root`, talking to `host`, on
    /// `platform`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings, platform: Platform) -> Ada {
        let tools = Tools::find(host.as_mut());
        let mut kit = Kit::with_host("ada", host);
        for install in platform.installs() {
            kit.installer.add(install);
        }
        let mut a = Ada {
            kit,
            root: root.into(),
            project: Project::read(root),
            tools,
            platform,
            settings: read_settings(),
            read_settings,
            gpr: Ok(None),
            set_servers: (None, None),
            set_settings: None,
        };
        a.configure();
        a
    }

    fn gpr(&self) -> Option<&str> {
        self.gpr.as_ref().ok().and_then(|g| g.as_deref())
    }

    /// The proof, as the `prove` task runs it.
    fn prove(&self) -> Option<Command> {
        tasks::prove(&self.project, &self.tools, self.gpr(), self.settings.prove_level)
    }

    /// The kit's tasks, checks and proof, for the project, tools and
    /// settings as they are.
    fn configure(&mut self) {
        self.gpr = self.project.gpr(&self.settings.project_file, &self.root);
        self.kit.tasks = tasks::tasks(&self.project, &self.tools, self.gpr(), self.settings.prove_level, &self.root);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(&self.project, &self.tools, &self.platform) {
            self.kit.doctor.program(program);
        }
        self.kit.doctor.extra = doctor::project_file(&self.gpr).into_iter().collect();
        // Put in place of the one there, so what it found stays shown.
        match self.prove().filter(|_| self.project.spark) {
            Some(cmd) => self.kit.lint.add(prove::linter(cmd, self.project.clone())),
            None => self.kit.lint.remove(self.kit.host.as_mut(), prove::NAME),
        }
    }

    /// The server for `languages`' files, with `args`: in a crate, in
    /// its environment (`alr exec`) when there's Alire to run it.
    fn server(&self, args: &[&str]) -> Option<Command> {
        let als = self.tools.als.as_deref()?;
        let cmd = match (&self.project.alire, &self.tools.alr) {
            (Some(_), Some(_)) => self.tools.alr().args(["exec", "--", als]),
            _ => Command::new(als),
        };
        Some(cmd.args(args.iter().copied()))
    }

    /// The servers and their settings set in Fenix, where they changed.
    /// Nothing found leaves Ada without one.
    fn apply(&mut self) {
        let servers = (self.server(&[]), self.server(&["--language-gpr"]));
        let settings = servers.0.as_ref().and_then(|_| als_settings(self.gpr()));
        let host = self.kit.host.as_mut();
        if servers != self.set_servers {
            for (language, server, set) in [(ADA, &servers.0, &self.set_servers.0), (GPR, &servers.1, &self.set_servers.1)] {
                if server != set {
                    if let Err(why) = host.language_server(&[language.to_string()], server.as_ref()) {
                        host.log(&format!("language server for {language}: {why}"));
                    }
                }
            }
            self.set_servers = servers;
            self.set_settings = None;
        }
        if settings != self.set_settings {
            if let Err(why) = host.language_server_settings(ADA, settings.as_ref()) {
                host.log(&format!("settings for the Ada Language Server: {why}"));
            }
            self.set_settings = settings;
        }
    }

    /// The tasks set in Fenix.
    fn set_tasks(&mut self) {
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
            self.set_tasks();
            self.apply();
        }
    }

    /// The project's files changed: read again, and checked again.
    fn reload(&mut self) {
        self.project = Project::read(&self.root);
        self.configure();
        self.set_tasks();
        self.kit.doctor.run(self.kit.host.as_mut());
        self.apply();
    }

    /// Whether saving `name` (in the project) changes what it is: the
    /// crate's manifest or a project file at the top, a source that's
    /// new, or the first with `SPARK_Mode`.
    fn changes_project(&self, name: &str) -> bool {
        if name == "alire.toml" || (!name.contains('/') && name.to_ascii_lowercase().ends_with(".gpr")) {
            return true;
        }
        if !project::is_ada(name) {
            return false;
        }
        if !self.project.sources.iter().any(|s| s == name) && self.project.sources.len() < project::SOURCES {
            return true;
        }
        !self.project.spark && std::fs::read_to_string(format!("{}/{name}", self.root)).is_ok_and(|t| t.to_ascii_lowercase().contains("spark_mode"))
    }

    /// The pack's own commands: the proof now, and the fix rows lang-kit
    /// leaves to it.
    fn command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "ada.prove" => {
                if self.project.alire.is_none() && self.gpr().is_none() {
                    return Err(match &self.gpr {
                        Err(why) => why.clone(),
                        Ok(_) => "there's no project file (.gpr) or alire.toml here to prove".into(),
                    });
                }
                // Not SPARK by what the pack read: proved all the same.
                if !self.kit.lint.linters.iter().any(|l| l.name == prove::NAME) {
                    let cmd = self.prove().ok_or("there's no project file to prove")?;
                    self.kit.lint.add(prove::linter(cmd, self.project.clone()));
                }
                let file = self.gpr().unwrap_or("alire.toml").to_string();
                self.kit.lint.run(self.kit.host.as_mut(), prove::NAME, &file)
            }
            doctor::INSTALL_GNAT => self.kit.host.run_task("Install GNAT and gprbuild", &doctor::alr_install(&self.tools, &["gnat_native", "gprbuild"])?).map(|_| ()),
            doctor::INSTALL_GNATPROVE => self.kit.host.run_task("Install GNATprove", &doctor::alr_install(&self.tools, &["gnatprove"])?).map(|_| ()),
            _ => Err(format!("no command {id} here")),
        }
    }
}

impl Extension for Ada {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        // The doctor reads the project again first: a source may say
        // SPARK_Mode now.
        if id == "ada.doctor" {
            self.project = Project::read(&self.root);
            self.configure();
            self.set_tasks();
            self.apply();
        }
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            None => self.command(id),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        let saved = match &event {
            Event::Saved(name) => self.changes_project(&name.replace('\\', "/")),
            _ => false,
        };
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("ada."));
        let installing = self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        if saved {
            self.reload();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.reload();
        }
        let installed = installing && !self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        if installed || task_ended {
            // What a fix put here: Alire's release, or what alr installed.
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

export!(Ada);

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

    fn pack(dir: &std::path::Path, host: &Shared, settings: fn() -> Settings) -> Ada {
        let mut a = Ada::new(Box::new(host.clone()), &dir.to_string_lossy(), settings, Platform::new("linux", "x86_64"));
        a.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
        a
    }

    fn said(a: &mut Ada, process: u64, text: &str, code: i32) {
        for l in text.lines() {
            a.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: l.into() }));
        }
        a.handle(Event::ProcessExit((process, Some(code))));
    }

    /// The processes started, by id, each as a line.
    fn spawned(calls: &[String]) -> Vec<(u64, String)> {
        calls.iter().filter_map(|c| c.strip_prefix("spawn ")?.split_once(' ')).map(|(id, l)| (id.parse().unwrap(), l.to_string())).collect()
    }

    /// A crate as `alr init --bin` makes it, with SPARK in it.
    fn crate_(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("alire.toml"), include_str!("samples/alire.toml")).unwrap();
        std::fs::write(dir.join("orbit.gpr"), include_str!("samples/orbit.gpr")).unwrap();
        std::fs::write(dir.join("src/orbit.adb"), "with Altitudes;\n\nprocedure Orbit is\nbegin\n   null;\nend Orbit;\n").unwrap();
        std::fs::write(dir.join("src/altitudes.ads"), "package Altitudes\n  with SPARK_Mode\nis\nend Altitudes;\n").unwrap();
        std::fs::write(dir.join("src/altitudes.adb"), "package body Altitudes\n  with SPARK_Mode\nis\nend Altitudes;\n").unwrap();
        dir
    }

    #[test]
    fn in_a_crate_the_server_runs_in_alires_environment() {
        let dir = crate_("crate");
        let host = Shared::default();
        found(&host, &[("alr", "/t/alr"), ("ada_language_server", "/t/als"), ("gnatprove", "/h/.alire/bin/gnatprove")]);
        let mut a = pack(&dir, &host, Settings::default);
        a.activate().unwrap();
        let calls = host.take();
        for want in ["tasks build, run, test, update, clean, prove", "server ada: /t/alr -n exec -- /t/als", "server gpr: /t/alr -n exec -- /t/als --language-gpr", "settings ada: {\"ada\":{\"projectFile\":\"orbit.gpr\"}}"] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert!(!calls.iter().any(|c| c.starts_with("formatter")), "the server formats");

        // The doctor: Alire, its GNAT (none selected), the server, GNATprove.
        let procs = spawned(&calls);
        let lines: Vec<&str> = procs.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(lines, ["/t/alr --version", "/t/alr -n version", "/t/als --version", "/h/.alire/bin/gnatprove --version"]);
        said(&mut a, procs[0].0, "alr 2.1.1", 0);
        said(&mut a, procs[1].0, include_str!("samples/alr-version.txt"), 0);
        said(&mut a, procs[2].0, "ALS version: 2027.0.202610121 ()", 0);
        said(&mut a, procs[3].0, "FSF 16.1.0\nWhy3 for gnatprove version 1.8.2+git", 0);
        let checks: Vec<(String, Health, String)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health, c.detail.clone())).collect();
        assert_eq!(checks[1], ("GNAT (Alire's)".into(), Health::Bad, "Alire has no GNAT selected: alr builds nothing until one is".into()));
        assert_eq!(checks[2].2, "2027.0.202610121 (/t/als)");
        a.run_command(doctor::SELECT_TOOLCHAIN).unwrap();
        assert!(host.take().iter().any(|c| c.ends_with("Select Alire's GNAT and gprbuild: /t/alr -n toolchain --select gnat_native gprbuild")));

        // The proof's findings, on the sources they're in.
        a.run_command("ada.prove").unwrap();
        let run = spawned(&host.take()).into_iter().next().unwrap();
        assert_eq!(run.1, "/t/alr -n exec -P1 -- /h/.alire/bin/gnatprove --level=2 -j0 --output=oneline -k");
        said(&mut a, run.0, include_str!("samples/gnatprove.txt"), 1);
        let problems = host.0.borrow().problems.clone();
        assert_eq!(problems["/project/src/altitudes.adb"].len(), 3);
        assert_eq!(problems["/project/beacons.ads"].len(), 3, "not among the sources: by its name");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_level_and_the_project_file_are_settings() {
        let dir = temp("settings");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("orbit.gpr"), include_str!("samples/orbit.gpr")).unwrap();
        std::fs::write(dir.join("tools.gpr"), "project Tools is\nend Tools;\n").unwrap();
        std::fs::write(dir.join("src/orbit.adb"), "procedure Orbit with SPARK_Mode is\nbegin\n   null;\nend Orbit;\n").unwrap();
        let host = Shared::default();
        found(&host, &[("gnat", "/g/gnat"), ("gprbuild", "/g/gprbuild"), ("gnatprove", "/g/gnatprove"), ("ada_language_server", "/t/als")]);
        let mut a = pack(&dir, &host, Settings::default);
        a.activate().unwrap();
        let calls = host.take();
        assert!(calls.iter().any(|c| c == "server ada: /t/als"), "{calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("settings")), "no project file to tell it of");
        assert_eq!(a.kit.doctor.extra.iter().map(|c| c.detail.as_str()).collect::<Vec<_>>(), ["orbit.gpr, tools.gpr: choose one with ada.projectFile"]);
        assert!(a.run_command("ada.prove").is_err());

        a.read_settings = || Settings { project_file: "orbit.gpr".into(), prove_level: 4 };
        a.handle(Event::SettingChanged("ada.projectFile".into()));
        let calls = host.take();
        assert!(calls.iter().any(|c| c == "tasks build, clean, prove"), "{calls:?}");
        assert!(calls.iter().any(|c| c == "settings ada: {\"ada\":{\"projectFile\":\"orbit.gpr\"}}"), "{calls:?}");
        assert!(a.kit.doctor.extra.is_empty());
        assert_eq!(line(&a.kit.tasks.tasks.iter().find(|t| t.name == "prove").unwrap().cmd), "/g/gnatprove -P orbit.gpr --level=4 -j0");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_the_tools_the_fix_rows_install_them() {
        let dir = temp("fix");
        std::fs::write(dir.join("orbit.gpr"), include_str!("samples/orbit.gpr")).unwrap();
        std::fs::write(dir.join("orbit.adb"), "procedure Orbit is\nbegin\n   null;\nend Orbit;\n").unwrap();
        let host = Shared::default();
        let mut a = pack(&dir, &host, Settings::default);
        a.activate().unwrap();
        let calls = host.take();
        assert!(!calls.iter().any(|c| c.starts_with("server")), "nothing found, nothing set: {calls:?}");
        let fixes: Vec<Option<String>> = a.kit.doctor.programs.iter().map(|p| p.fix.clone().map(|f| f.1)).collect();
        assert_eq!(fixes, [Some(doctor::INSTALL_GNAT.into()), Some(doctor::INSTALL_GNAT.into()), Some("ada.install.alr".into()), Some("ada.install.als".into())]);
        assert!(a.run_command(doctor::INSTALL_GNAT).unwrap_err().contains("Alire isn't installed"));
        a.run_command("ada.install.alr").unwrap();
        let download = host.take().into_iter().find(|c| c.starts_with("download ")).unwrap();
        assert!(download.contains("https://github.com/alire-project/alire/releases/download/v2.1.1/alr-2.1.1-bin-x86_64-linux.zip") && download.ends_with(" 09c66bcd8c35dd4b97b72c3d9b76e44caa6964a2db35aba069f396f00f1f64c7"), "{download}");
        assert!(a.run_command("ada.nothing").is_err());

        // Alire found (as a fix would leave it): it installs GNAT, and
        // GNAT found then is used.
        found(&host, &[("alr", "/h/.alire/alr")]);
        a.handle(Event::TaskFinished((99, true)));
        a.run_command(doctor::INSTALL_GNAT).unwrap();
        let task = host.take().into_iter().find(|c| c.starts_with("task ")).unwrap();
        assert!(task.ends_with("Install GNAT and gprbuild: /h/.alire/alr -n install gnat_native gprbuild"), "{task}");
        found(&host, &[("gprbuild", "/h/.alire/bin/gprbuild")]);
        a.handle(Event::TaskFinished((100, true)));
        assert!(host.take().iter().any(|c| c == "tasks build, clean"));
        assert_eq!(line(&a.kit.tasks.tasks[0].cmd), "/h/.alire/bin/gprbuild -P orbit.gpr");

        // SPARK_Mode in a source saved: a SPARK project now.
        std::fs::write(dir.join("orbit.adb"), "procedure Orbit with SPARK_Mode is\nbegin\n   null;\nend Orbit;\n").unwrap();
        a.handle(Event::Saved("orbit.adb".into()));
        assert!(a.kit.tasks.tasks.iter().any(|t| t.name == "prove"));
        assert!(a.kit.doctor.programs.iter().any(|p| p.label == "GNATprove"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
