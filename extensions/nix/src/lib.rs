//! Nix in Fenix, on lang-kit: the grammar (its language file and queries),
//! nixd or nil as the language server, nixfmt, alejandra or nixpkgs-fmt as
//! the formatter, statix and deadnix on save, the tasks of a flake or a
//! plain `default.nix`, the pickers of a flake's outputs, and the project
//! doctor's checks.
//!
//! Nothing is installed: Nix's tools come from Nix, so the pack looks for
//! them on `PATH` and in the profiles ([`tools`]), and the only thing it
//! will do about one that isn't there is run it through the project's dev
//! shell or `nix shell nixpkgs#...`, as a task you start from a doctor
//! row ([`doctor`]). What it knows of the project is read from its files
//! ([`project`]); which server and formatter it uses is [`choose`]'s.

pub mod calls;
pub mod choose;
pub mod doctor;
pub mod flake;
pub mod lint;
pub mod project;
pub mod tasks;
pub mod tools;
pub mod trust;

#[cfg(test)]
mod data;

use std::cell::RefCell;
use std::rc::Rc;

use fenix_extension_api::{export, Check, Command, Event, Extension, Key, MouseEvent, Page, PickerItem};
use fenix_lang_kit::exec::Runs;
use fenix_lang_kit::{Fenix, Host, Kit};

use choose::{Runner, Settings};
use flake::Action;
use project::Project;
use tools::Tools;

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language.
const LANGUAGES: &[&str] = &["nix"];

/// Your setting for where the server comes from (`path` or `nixpkgs`).
pub const RUNNER_SETTING: &str = "nix.server_runner";

/// The pack's own folder under `/data`, where the projects you've allowed
/// the dev shell for are kept.
pub const DATA: &str = "/data/nix";

/// The command that stops using a project's dev shell for the server.
pub const FORGET: &str = doctor::SERVER_FORGET;

pub struct Nix {
    pub kit: Kit,
    root: String,
    project: Project,
    tools: Tools,
    system: Option<String>,
    settings: Settings,
    /// Where the allowed projects are kept.
    data: String,
    /// The project's full path, once Fenix has said it (a file in it is
    /// focused); the dev shell is only ever used for a project in `data`.
    full_root: Option<String>,
    dev_shell: bool,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_formatter: Option<Command>,
    /// The doctor's rows that aren't a program's, worked out by `configure`.
    extras: Rc<RefCell<Vec<Check>>>,
    /// What the linters last found, so one doesn't wipe out the other's.
    found: lint::Shared,
    /// `nix flake show --json`, for the picker of what `Action` chooses.
    shows: Runs<Action>,
    picking: Option<(u64, Action)>,
    /// The task that tries the server through Nix, and the runner to keep
    /// when it works.
    trying: Option<(u64, Runner)>,
}

impl Default for Nix {
    fn default() -> Nix {
        Nix::new(Box::new(Fenix), PROJECT)
    }
}

impl Nix {
    /// The pack for the project in `root`, talking to `host`.
    pub fn new(host: Box<dyn Host>, root: &str) -> Nix {
        Nix::with_data(host, root, DATA)
    }

    /// The same, keeping what you've allowed in `data`.
    pub fn with_data(mut host: Box<dyn Host>, root: &str, data: &str) -> Nix {
        let full_root = host.here().and_then(|(_, r)| r).map(|r| trust::normal(&r));
        let dev_shell = full_root.as_deref().is_some_and(|r| trust::contains(data, r));
        let tools = Tools::find(host.as_mut());
        let platform = host.platform();
        let extras: Rc<RefCell<Vec<Check>>> = Rc::default();
        let shared = extras.clone();
        let kit = Kit::with_host("nix", host).extra_checks(move || shared.borrow().clone());
        let mut nix = Nix {
            kit,
            root: root.into(),
            project: Project::read(root),
            tools,
            system: tools::system(&platform.os, &platform.arch),
            settings: Settings::read(),
            data: data.into(),
            full_root,
            dev_shell,
            set_server: None,
            set_formatter: None,
            extras,
            found: lint::Shared::default(),
            shows: Runs::new(),
            picking: None,
            trying: None,
        };
        nix.configure();
        nix
    }

    /// The settings in force here: yours, and the dev shell too for a
    /// project you've allowed it for. A project's own files never say so.
    fn active(&self) -> Settings {
        Settings { runner: if self.dev_shell { Runner::DevShell } else { self.settings.runner }, ..self.settings.clone() }
    }

    /// Where the project is, as Fenix says it, once a file in it is focused;
    /// whether that changed what's in force.
    fn learn_root(&mut self) -> bool {
        let Some(root) = self.kit.host.here().and_then(|(_, r)| r).map(|r| trust::normal(&r)) else { return false };
        if self.full_root.as_deref() == Some(root.as_str()) {
            return false;
        }
        let dev_shell = trust::contains(&self.data, &root);
        self.full_root = Some(root);
        let changed = dev_shell != self.dev_shell;
        self.dev_shell = dev_shell;
        changed
    }

    /// The kit's tasks, checks and linters, for the project, tools and
    /// settings as they are.
    fn configure(&mut self) {
        self.kit.tasks = tasks::tasks(&self.project, &self.tools);
        let decision = choose::formatter(&self.settings, &self.project, &self.tools);
        let (programs, extra) = doctor::checks(&self.project, &self.tools, &self.active(), &decision);
        self.kit.doctor.programs.clear();
        for program in programs {
            self.kit.doctor.program(program);
        }
        *self.extras.borrow_mut() = extra;
        self.kit.lint.linters.clear();
        match (self.settings.statix, self.tools.get("statix")) {
            (true, Some(statix)) => self.kit.lint.add(lint::statix(&Command::new(statix), &self.found)),
            _ => self.found.forget("statix"),
        }
        match (self.settings.deadnix, self.tools.get("deadnix")) {
            (true, Some(deadnix)) => self.kit.lint.add(lint::deadnix(&Command::new(deadnix), self.settings.deadnix_arguments, &self.found)),
            _ => self.found.forget("deadnix"),
        }
    }

    /// The server and the formatter set in Fenix, where they changed.
    /// Nothing found leaves Nix files without them.
    fn apply(&mut self) {
        let server = choose::server(&self.active(), &self.project, &self.tools).map(|s| s.command);
        let decision = choose::formatter(&self.settings, &self.project, &self.tools);
        let formatter = choose::formatter_command(decision.want, &self.tools);
        let host = self.kit.host.as_mut();
        let languages: Vec<String> = LANGUAGES.iter().map(|l| l.to_string()).collect();
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
    }

    fn set_tasks(&mut self) {
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
        }
    }

    /// Everything worked out again, set, and checked again.
    fn refresh(&mut self) {
        self.configure();
        self.set_tasks();
        self.apply();
        self.kit.doctor.run(self.kit.host.as_mut());
    }

    /// The tools as they're found now; everything set again where that
    /// changed (the profile got a server).
    fn find_again(&mut self) {
        let tools = Tools::find(self.kit.host.as_mut());
        if tools != self.tools {
            self.tools = tools;
            self.refresh();
        }
    }

    /// The project's files changed: read again.
    fn reload(&mut self) {
        self.project = Project::read(&self.root);
        self.refresh();
    }

    fn say(&mut self, message: &str) {
        self.kit.host.notify(message);
    }

    /// The server's name the fix rows are about.
    fn server_name(&self) -> &'static str {
        choose::candidates(self.settings.server)[0]
    }

    /// Tries the server through Nix as a task you can watch: the dev shell
    /// is built, or the server downloaded, in the task pane. When it works
    /// the runner is kept (`nix.server_runner`) and the server set.
    fn try_server(&mut self, runner: Runner) -> Result<(), String> {
        let name = self.server_name();
        let cmd = choose::try_server(runner, name, &self.project, &self.tools).ok_or_else(|| match runner {
            Runner::DevShell => "this project has no flake.nix or shell.nix, or Nix isn't installed".to_string(),
            _ => "Nix isn't installed".to_string(),
        })?;
        let (task, what) = match runner {
            Runner::DevShell => (format!("{name} in the dev shell"), "the project's dev shell (it's built first)"),
            _ => (format!("{name} from nixpkgs"), "nixpkgs (it's downloaded the first time)"),
        };
        if runner == Runner::DevShell && self.full_root.is_none() {
            return Err("open a file of the project first: the choice is kept for this project alone".into());
        }
        let id = self.kit.host.run_task(&task, &cmd)?;
        self.trying = Some((id, runner));
        self.say(&format!("Trying {name} through {what}: see Task Output"));
        Ok(())
    }

    fn tried(&mut self, runner: Runner, ok: bool) {
        let name = self.server_name();
        if !ok {
            let where_ = if runner == Runner::DevShell { "the project's dev shell has no" } else { "nixpkgs couldn't give" };
            self.say(&format!("{where_} {name}: see Task Output"));
            return;
        }
        if runner == Runner::DevShell {
            // This project alone, kept here and not in a settings file a repository could ship.
            let kept = self.full_root.clone().ok_or_else(|| "the project isn't known".to_string()).and_then(|r| trust::add(&self.data, &r));
            match kept {
                Ok(()) => {
                    self.dev_shell = true;
                    self.refresh();
                    self.say(&format!("{name} will be started through this project's dev shell (SPC p h can stop that)"));
                }
                Err(why) => self.say(&format!("{name} works in the dev shell, but it couldn't be remembered: {why}")),
            }
            return;
        }
        match crate::calls::set_setting(RUNNER_SETTING, "nixpkgs") {
            Ok(()) => {
                self.settings = Settings::read();
                self.refresh();
                self.say(&format!("{name} will be started through nix shell nixpkgs#{name} ({RUNNER_SETTING} = nixpkgs)"));
            }
            Err(why) => self.say(&format!("{name} works through Nix, but {RUNNER_SETTING} couldn't be set: {why}")),
        }
    }

    /// `nix flake show --json`, then a picker of what it lists.
    fn show(&mut self, action: Action) -> Result<(), String> {
        if !self.project.flake {
            return Err("this project has no flake.nix".into());
        }
        if self.tools.nix.is_none() {
            return Err("Nix isn't installed: SPC m d says how".into());
        }
        let cmd = self.tools.command("nix").args(["flake", "show", "--json", "--no-write-lock-file"]);
        self.shows.start(self.kit.host.as_mut(), &cmd, action)?;
        self.say("Reading the flake's outputs (nix flake show --json); it may fetch the flake's inputs");
        Ok(())
    }

    fn shown(&mut self, action: Action, out: fenix_lang_kit::Output) {
        if !out.ok() {
            self.say(&format!("nix flake show failed: {}", out.why()));
            return;
        }
        let json = match flake::read(&out.stdout) {
            Ok(json) => json,
            Err(why) => return self.say(&why),
        };
        let targets = flake::targets(&json, self.system.as_deref(), action);
        if targets.is_empty() {
            return self.say("The flake has no outputs of that kind for this machine");
        }
        let items: Vec<PickerItem> = targets.iter().map(|t| PickerItem::new(&t.label, &t.installable).detail(&t.detail)).collect();
        let id = self.kit.host.pick(action.title(), &items);
        self.picking = Some((id, action));
    }

    fn chosen(&mut self, action: Action, installable: &str) {
        let nix = self.tools.command("nix");
        match action {
            Action::Build => {
                if let Err(why) = self.kit.host.run_task(&format!("build {installable}"), &nix.arg("build").arg(installable)) {
                    self.say(&format!("build: {why}"));
                }
            }
            Action::Run => {
                if let Err(why) = self.kit.host.run_task(&format!("run {installable}"), &nix.arg("run").arg(installable)) {
                    self.say(&format!("run: {why}"));
                }
            }
            Action::Develop => {
                let cmd = if installable == "." { nix.arg("develop") } else { nix.arg("develop").arg(installable) };
                if let Err(why) = calls::terminal(&cmd, &format!("nix develop {installable}")) {
                    self.say(&format!("nix develop: {why}"));
                }
            }
        }
    }

    /// The default dev shell in a terminal: a flake's, or `shell.nix`'s.
    fn develop(&mut self) -> Result<(), String> {
        let (cmd, title) = if self.project.flake && self.tools.nix.is_some() {
            (self.tools.command("nix").arg("develop"), "nix develop")
        } else if self.project.shell_nix && self.tools.nix_shell.is_some() {
            (self.tools.command("nix-shell"), "nix-shell")
        } else {
            return Err("no flake.nix or shell.nix to enter, or Nix isn't installed".into());
        };
        calls::terminal(&cmd, title).map(|_| ())
    }
}

impl Extension for Nix {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        if self.learn_root() {
            self.refresh();
        }
        match id {
            doctor::INSTALL_HELP => {
                self.kit.host.open_text_beside("Installing Nix", &doctor::install_help(), None);
                Ok(())
            }
            doctor::SERVER_DEV_SHELL => self.try_server(Runner::DevShell),
            doctor::SERVER_NIXPKGS => self.try_server(Runner::Nixpkgs),
            doctor::SERVER_FORGET => {
                let root = self.full_root.clone().ok_or("open a file of the project first")?;
                trust::remove(&self.data, &root)?;
                self.dev_shell = false;
                self.refresh();
                Ok(())
            }
            "nix.flake.build" => self.show(Action::Build),
            "nix.flake.run" => self.show(Action::Run),
            "nix.flake.develop" => self.show(Action::Develop),
            "nix.develop" => self.develop(),
            _ => {
                if id == "nix.doctor" {
                    // Something installed since: found and used.
                    self.find_again();
                }
                self.kit.run_command(id).unwrap_or_else(|| Err(format!("no command {id} here")))
            }
        }
    }

    fn handle(&mut self, event: Event) {
        if self.shows.owns(&event) {
            if let Some((action, out)) = self.shows.event(&event) {
                self.shown(action, out);
            }
            return;
        }
        match &event {
            Event::Picked((id, value)) if self.picking.is_some_and(|(p, _)| p == *id) => {
                let (_, action) = self.picking.take().unwrap();
                if let Some(installable) = value {
                    self.chosen(action, installable);
                }
                return;
            }
            Event::TaskFinished((id, ok)) if self.trying.is_some_and(|(t, _)| t == *id) => {
                let (_, runner) = self.trying.take().unwrap();
                self.tried(runner, *ok);
                return;
            }
            _ => {}
        }
        let moved = matches!(&event, Event::FocusChanged(_) | Event::Saved(_)) && self.learn_root();
        let project_file = matches!(&event, Event::Saved(name) if project::FILES.contains(&name.as_str()));
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("nix."));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        if project_file {
            self.reload();
        } else if moved {
            self.refresh();
        }
        if setting {
            self.settings = Settings::read();
            self.refresh();
        }
        if task_ended {
            // What a task put in a profile (nix profile install).
            self.find_again();
        }
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

export!(Nix);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;
    use fenix_extension_api::Health;
    use fenix_test_kit::{Fake, Harness, Ran};

    /// The pack for the project in `dir`, on `fake`.
    fn pack(dir: &std::path::Path, fake: &Fake) -> Harness<Nix> {
        pack_with(dir, fake, &dir.join(".nix-data"))
    }

    /// The same, keeping the projects you've allowed in `data`.
    fn pack_with(dir: &std::path::Path, fake: &Fake, data: &std::path::Path) -> Harness<Nix> {
        let nix = Nix::with_data(fake.host(), &dir.to_string_lossy(), &data.to_string_lossy());
        Harness::wrap(fake.clone(), nix)
    }

    /// A file of the project in `dir` is the one you're in, so Fenix says
    /// where the project is.
    fn focus(fake: &Fake, dir: &std::path::Path) {
        fake.in_file(&format!("{}/flake.nix", dir.display()), "nix", "{ }\n").project("p", &dir.to_string_lossy());
    }

    /// The processes started, as lines, in order.
    fn spawned(fake: &Fake) -> Vec<String> {
        fake.spawned().into_iter().map(|(_, l)| l).collect()
    }

    /// Every tool installed in `/n`, each saying its version.
    fn installed() -> Fake {
        let fake = Fake::new();
        fake.platform("linux", "x86_64");
        for name in tools::NAMES {
            fake.program(name, &format!("/n/{name}"));
        }
        fake.on_spawn("/n/nix --version", Ran::ok("nix (Nix) 2.24.9\n"))
            .on_spawn("/n/nix config show experimental-features", Ran::ok("nix-command flakes\n"))
            .on_spawn("/n/nixd --version", Ran::ok("nixd 2.6.4\n"))
            .on_spawn("/n/nil --version", Ran::ok("nil 2025-06-13\n"))
            .on_spawn("/n/nixfmt --version", Ran::ok("nixfmt 0.6.0\n"))
            .on_spawn("/n/alejandra --version", Ran::ok("alejandra 4.0.0\n"))
            .on_spawn("/n/nixpkgs-fmt --version", Ran::ok("nixpkgs-fmt 1.3.0\n"))
            .on_spawn("/n/statix --version", Ran::ok("statix 0.5.8\n"))
            .on_spawn("/n/deadnix --version", Ran::ok("deadnix 1.3.1\n"))
            .on_spawn("/n/nixos-rebuild --version", Ran::ok("24.11.20250101.abc\n"))
            .on_spawn("/n/home-manager --version", Ran::ok("24.11\n"));
        fake
    }

    /// Only Nix installed, saying its version and that flakes are on.
    fn with_nix(fake: &Fake) {
        fake.program("nix", "/n/nix").on_spawn("/n/nix --version", Ran::ok("nix (Nix) 2.24.9\n")).on_spawn("/n/nix config show experimental-features", Ran::ok("nix-command flakes\n"));
    }

    /// A flake project.
    fn flake_project(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::write(dir.join("flake.nix"), include_str!("samples/flake.nix")).unwrap();
        dir
    }

    fn checks(fake: &Fake) -> Vec<(String, Health)> {
        fake.checks().iter().map(|c| (c.label.clone(), c.health)).collect()
    }

    #[test]
    fn the_server_the_formatter_the_tasks_and_the_linters_are_set() {
        let dir = flake_project("activate");
        let fake = installed();
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        let calls = fake.take();
        for want in ["tasks check, build, run, fmt, flake show, flake update (rewrites flake.lock)", "server nix: /n/nixd", "formatter nix: /n/nixfmt"] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert_eq!(t.ext.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["statix", "deadnix"]);
        let rows = checks(&fake);
        for (label, health) in [("Nix", Health::Ok), ("Flakes", Health::Ok), ("Language server (nixd)", Health::Ok), ("Formatter (nixfmt)", Health::Ok), ("statix", Health::Ok), ("deadnix", Health::Ok)] {
            assert!(rows.contains(&(label.to_string(), health)), "{label}: {rows:?}");
        }
        let nix = fake.checks().into_iter().find(|c| c.label == "Nix").unwrap();
        assert_eq!(nix.detail, "2.24.9 (/n/nix)");
        // Asking versions is all it spawns: nothing is built, downloaded or run through Nix.
        assert!(spawned(&fake).iter().all(|l| l.ends_with("--version") || l.ends_with("experimental-features")), "{:?}", spawned(&fake));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_nix_it_still_sets_what_is_installed_and_says_so() {
        let dir = flake_project("no-nix");
        let fake = Fake::new();
        fake.platform("linux", "x86_64");
        fake.program("nixd", "/h/.nix-profile/bin/nixd").program("alejandra", "/h/bin/alejandra");
        fake.on_spawn("/h/.nix-profile/bin/nixd --version", Ran::ok("nixd 2.6.4\n"));
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c == "server nix: /h/.nix-profile/bin/nixd"), "{calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("formatter")), "the flake names nixfmt, which isn't here: {calls:?}");
        let rows = fake.checks();
        let nix = rows.iter().find(|c| c.label == "Nix").unwrap();
        assert_eq!(nix.health, Health::Warn, "not a failure: the pack works without it");
        assert_eq!(nix.fix.as_ref().map(|f| f.1.as_str()), Some("nix.install-help"));
        let without = rows.iter().find(|c| c.label == "Without Nix").unwrap();
        assert!(without.detail.contains("still work"), "{}", without.detail);
        assert!(!rows.iter().any(|c| c.label == "Flakes"));
        // The fix row's text is shown beside the editor, not downloaded.
        t.command("nix.install-help").unwrap();
        assert!(fake.take().iter().any(|c| c.starts_with("text-beside Installing Nix") || c.contains("Installing Nix")), "{:?}", fake.calls());
        assert!(fake.requests().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_flakes_own_formatter_wins_over_what_happens_to_be_installed() {
        let dir = temp("formatter");
        std::fs::write(dir.join("flake.nix"), "{ outputs = { nixpkgs, ... }: { formatter.x86_64-linux = nixpkgs.legacyPackages.x86_64-linux.alejandra; }; }\n").unwrap();
        let fake = Fake::new();
        fake.platform("linux", "x86_64");
        fake.program("nixfmt", "/n/nixfmt").on_spawn("/n/nixfmt --version", Ran::ok("nixfmt 0.6.0\n"));
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        assert!(!fake.take().iter().any(|c| c.starts_with("formatter")), "nixfmt isn't the project's formatter");
        let missing = fake.checks().into_iter().find(|c| c.label == "Formatter (alejandra)").unwrap();
        assert_eq!(missing.health, Health::Warn);

        // It's installed after: found at the next check, and used.
        fake.program("alejandra", "/n/alejandra").on_spawn("/n/alejandra --version", Ran::ok("alejandra 4.0.0\n"));
        t.command("nix.doctor").unwrap();
        assert!(fake.take().iter().any(|c| c == "formatter nix: /n/alejandra --quiet -"));
        // A setting picks another on purpose.
        t.set_setting("nix.formatter", Some("nixfmt"));
        assert!(fake.take().iter().any(|c| c == "formatter nix: /n/nixfmt"));
        t.set_setting("nix.formatter", Some("off"));
        assert!(fake.take().iter().any(|c| c == "formatter nix: "), "{:?}", fake.calls());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nixd_is_preferred_nil_is_the_fallback_and_a_setting_picks() {
        let dir = flake_project("server-pref");
        let fake = Fake::new();
        fake.platform("linux", "x86_64");
        fake.program("nil", "/n/nil");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        assert!(fake.take().iter().any(|c| c == "server nix: /n/nil"));
        fake.program("nixd", "/n/nixd");
        t.command("nix.doctor").unwrap();
        assert!(fake.take().iter().any(|c| c == "server nix: /n/nixd"), "nixd, once it's there");
        t.set_setting("nix.server", Some("nil"));
        assert!(fake.take().iter().any(|c| c == "server nix: /n/nil"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_project_with_a_flake_and_no_server_starts_nothing_through_nix_by_itself() {
        let dir = flake_project("quiet");
        let fake = Fake::new();
        fake.platform("linux", "x86_64");
        with_nix(&fake);
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        t.event(Event::Saved("flake.nix".into()));
        let calls = fake.calls();
        assert!(!calls.iter().any(|c| c.starts_with("server") || c.starts_with("task ") || c.starts_with("terminal")), "{calls:?}");
        assert!(spawned(&fake).iter().all(|l| !l.contains("develop") && !l.contains("nixpkgs#")), "{:?}", spawned(&fake));
        let row = fake.checks().into_iter().find(|c| c.label == "Language server (nixd)").unwrap();
        assert_eq!((row.health, row.fix.map(|f| f.1)), (Health::Warn, Some("nix.server.dev-shell".into())));
        let other = fake.checks().into_iter().find(|c| c.label == "Server from nixpkgs").unwrap();
        assert_eq!(other.fix.map(|f| f.1), Some("nix.server.nixpkgs".into()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_dev_shell_fix_is_a_visible_task_and_when_it_works_this_project_alone_is_remembered() {
        let dir = flake_project("dev-shell");
        let other = flake_project("dev-shell-other");
        let fake = Fake::new();
        fake.platform("linux", "x86_64");
        with_nix(&fake);
        fake.on_task("nixd in the dev shell", true);
        focus(&fake, &dir);
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        t.command("nix.server.dev-shell").unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.ends_with("nixd in the dev shell: /n/nix develop --no-write-lock-file -c nixd --version")), "a task in the task pane: {calls:?}");
        assert!(calls.iter().any(|c| c == "server nix: /n/nix develop --no-write-lock-file -c nixd"), "{calls:?}");
        assert_eq!(fake.setting_value("nix.server_runner"), None, "not a setting: a project's own settings file could say the same");
        assert_eq!(trust::list(&dir.join(".nix-data").to_string_lossy()), [dir.to_string_lossy().to_string()]);
        let row = fake.checks().into_iter().find(|c| c.label == "Language server (nixd)").unwrap();
        assert_eq!(row.health, Health::Ok);
        assert!(row.detail.contains("nix develop -c nixd"), "{}", row.detail);
        assert_eq!(row.fix.map(|f| f.1), Some("nix.server.forget".into()));

        // The next start, in the same project: used from the start.
        let again = Fake::new();
        again.platform("linux", "x86_64");
        with_nix(&again);
        focus(&again, &dir);
        let mut t2 = pack(&dir, &again);
        t2.activate().unwrap();
        assert!(again.take().iter().any(|c| c == "server nix: /n/nix develop --no-write-lock-file -c nixd"));

        // Another project, the same data: nothing. A project is its full path.
        let fake3 = Fake::new();
        fake3.platform("linux", "x86_64");
        with_nix(&fake3);
        focus(&fake3, &other);
        let mut t3 = pack_with(&other, &fake3, &dir.join(".nix-data"));
        t3.activate().unwrap();
        t3.event(Event::Saved("flake.nix".into()));
        let calls = fake3.take();
        assert!(!calls.iter().any(|c| c.starts_with("server")), "{calls:?}");
        assert!(spawned(&fake3).iter().all(|l| !l.contains("develop")));
        // Focus moves to the allowed project in the same window: it's used there.
        focus(&fake3, &dir);
        t3.event(Event::FocusChanged(Some("flake.nix".into())));
        assert!(fake3.take().iter().any(|c| c == "server nix: /n/nix develop --no-write-lock-file -c nixd"), "the choice is for that path");

        // Forgotten.
        t2.command("nix.server.forget").unwrap();
        assert!(trust::list(&dir.join(".nix-data").to_string_lossy()).is_empty());
        assert!(again.take().iter().any(|c| c == "server nix: "), "no server again");
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&other).unwrap();
    }

    #[test]
    fn a_dev_shell_without_the_server_changes_nothing() {
        let dir = flake_project("dev-shell-fails");
        let fake = Fake::new();
        fake.platform("linux", "x86_64");
        with_nix(&fake);
        fake.on_task("nixd in the dev shell", false);
        focus(&fake, &dir);
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        t.command("nix.server.dev-shell").unwrap();
        let calls = fake.take();
        assert!(!calls.iter().any(|c| c.starts_with("server")), "{calls:?}");
        assert!(trust::list(&dir.join(".nix-data").to_string_lossy()).is_empty());
        assert!(calls.iter().any(|c| c.contains("has no nixd")), "{calls:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_dev_shell_needs_to_know_which_project_before_it_runs_anything() {
        let dir = flake_project("dev-shell-unknown");
        let fake = Fake::new();
        fake.platform("linux", "x86_64");
        with_nix(&fake);
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        assert!(t.command("nix.server.dev-shell").unwrap_err().contains("open a file of the project"));
        assert!(!fake.take().iter().any(|c| c.starts_with("task ")), "no task, since nothing could be remembered");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_project_that_asks_for_the_dev_shell_in_its_own_settings_starts_nothing() {
        // A cloned repository: .fenix/settings.toml says dev-shell, and Fenix applies a
        // project's settings as the setting's value, with no question asked.
        let dir = flake_project("clone");
        std::fs::create_dir_all(dir.join(".fenix")).unwrap();
        std::fs::write(dir.join(".fenix/settings.toml"), "[nix]\nserver_runner = \"dev-shell\"\n").unwrap();
        let fake = Fake::new();
        fake.platform("linux", "x86_64");
        with_nix(&fake);
        fake.setting("nix.server_runner", "dev-shell");
        focus(&fake, &dir);
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        t.event(Event::FocusChanged(Some("flake.nix".into())));
        t.event(Event::Saved("flake.nix".into()));
        t.saved(&format!("{}/flake.nix", dir.display()));
        t.set_setting("nix.server_runner", Some("dev-shell"));
        let calls = fake.calls();
        assert!(!calls.iter().any(|c| c.starts_with("server") || c.starts_with("task ") || c.starts_with("terminal")), "{calls:?}");
        assert!(spawned(&fake).iter().all(|l| !l.contains("develop") && !l.contains("nix-shell") && !l.contains("nixpkgs#")), "{:?}", spawned(&fake));
        let row = fake.checks().into_iter().find(|c| c.label == "Language server (nixd)").unwrap();
        assert_eq!(row.health, Health::Warn, "still missing, with its fix rows");
        // The same for nix-shell.
        std::fs::remove_file(dir.join("flake.nix")).unwrap();
        std::fs::write(dir.join("shell.nix"), "{ }").unwrap();
        t.event(Event::Saved("shell.nix".into()));
        assert!(!fake.calls().iter().any(|c| c.starts_with("server")));
        // nixpkgs is yours alone, and runs nothing of the project's.
        assert!(trust::list(&dir.join(".nix-data").to_string_lossy()).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_fresh_clone_spawns_nothing_through_nix_on_opening_or_saving() {
        let dir = flake_project("fresh");
        let fake2 = Fake::new();
        fake2.platform("linux", "x86_64");
        with_nix(&fake2);
        focus(&fake2, &dir);
        let mut t = pack(&dir, &fake2);
        t.activate().unwrap();
        for event in [Event::FocusChanged(Some("flake.nix".into())), Event::Saved("flake.nix".into()), Event::Saved("flake.lock".into())] {
            t.event(event);
        }
        t.saved(&format!("{}/flake.nix", dir.display()));
        let started = spawned(&fake2);
        assert!(started.iter().all(|l| l.ends_with("--version") || l.ends_with("experimental-features")), "only versions are asked: {started:?}");
        assert!(!fake2.calls().iter().any(|c| c.starts_with("task ") || c.starts_with("terminal")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_nixpkgs_fix_is_a_visible_task_that_downloads_and_is_kept_when_it_works() {
        let dir = temp("nixpkgs");
        std::fs::write(dir.join("default.nix"), "{ }\n").unwrap();
        let fake = Fake::new();
        fake.platform("linux", "x86_64");
        with_nix(&fake);
        fake.on_task("nixd from nixpkgs", true);
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        let row = fake.checks().into_iter().find(|c| c.label == "Language server (nixd)").unwrap();
        assert_eq!(row.fix.map(|f| f.1), Some("nix.server.nixpkgs".into()), "no flake, no dev shell to try");
        assert!(t.command("nix.server.dev-shell").is_err(), "there's no dev shell here");
        t.command("nix.server.nixpkgs").unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.ends_with("nixd from nixpkgs: /n/nix shell nixpkgs#nixd -c nixd --version")), "{calls:?}");
        assert!(calls.iter().any(|c| c == "server nix: /n/nix shell nixpkgs#nixd -c nixd"), "{calls:?}");
        assert_eq!(fake.setting_value("nix.server_runner").as_deref(), Some("nixpkgs"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_saved_file_is_checked_by_statix_and_deadnix_and_their_problems_shown() {
        let dir = flake_project("lint");
        let fake = installed();
        fake.on_spawn("/n/statix check -o json -- flake.nix", Ran::exits(1).stdout(include_str!("samples/statix-lint.json")));
        fake.on_spawn("/n/deadnix -o json -- flake.nix", Ran::ok(include_str!("samples/deadnix.json")));
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        let path = format!("{}/flake.nix", dir.display());
        fake.in_file(&path, "nix", "{ }\n").project("lint", &dir.to_string_lossy());
        t.saved(&path);
        let problems = fake.problems("/project/flake.nix");
        let sources: Vec<(&str, usize)> = ["statix", "deadnix"].iter().map(|s| (*s, problems.iter().filter(|p| p.source.as_deref() == Some(*s)).count())).collect();
        assert_eq!(sources, [("statix", 3), ("deadnix", 13)], "{problems:?}");
        let fixed = problems.iter().filter(|p| !p.fixes.is_empty()).count();
        assert_eq!(fixed, 3, "statix's come with fixes");
        let process = fake.process("/n/statix check -o json -- flake.nix").expect("statix ran");
        assert!(process.stdin_closed);

        // Fixed: clean again.
        fake.on_spawn("/n/statix check -o json -- flake.nix", Ran::ok(""));
        fake.on_spawn("/n/deadnix -o json -- flake.nix", Ran::ok(""));
        t.saved(&path);
        assert!(fake.problems("/project/flake.nix").is_empty());

        // Off: no linter of that name.
        t.set_setting("nix.statix", Some("false"));
        t.set_setting("nix.deadnix", Some("false"));
        assert!(t.ext.kit.lint.linters.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_the_linters_there_are_none_and_a_row_says_so() {
        let dir = flake_project("no-lint");
        let fake = Fake::new();
        fake.platform("linux", "x86_64");
        with_nix(&fake);
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        assert!(t.ext.kit.lint.linters.is_empty());
        let statix = fake.checks().into_iter().find(|c| c.label == "statix").unwrap();
        assert_eq!(statix.health, Health::Warn);
        fake.program("statix", "/n/statix");
        t.event(Event::TaskFinished((9, true)));
        assert_eq!(t.ext.kit.lint.linters.len(), 1, "found after a task ended (nix profile install)");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_flakes_outputs_are_listed_from_nix_flake_show_and_picked() {
        let dir = flake_project("pickers");
        let fake = installed();
        fake.on_spawn("/n/nix flake show --json --no-write-lock-file", Ran::ok(include_str!("samples/flake-show.json")));
        fake.on_pick("Flake output to build", "packages.docs").on_pick("Flake output to run", "apps.serve").on_pick("Dev shell to enter", "devShells.ci");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        t.command("nix.flake.build").unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.contains("Flake output to build: packages.default, packages.docs, checks.fmt")), "{calls:?}");
        assert!(calls.iter().any(|c| c.ends_with("build .#docs: /n/nix build .#docs")), "{calls:?}");

        t.command("nix.flake.run").unwrap();
        assert!(fake.take().iter().any(|c| c.ends_with("run .#serve: /n/nix run .#serve")));

        t.command("nix.flake.develop").unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.contains("nix develop .#ci") && c.contains("/n/nix develop .#ci")), "a terminal, not a task: {calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("task ")), "{calls:?}");
        // The lock file isn't written by looking.
        assert!(fake.process("/n/nix flake show --json --no-write-lock-file").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_default_dev_shell_is_one_key_and_a_picker_cancelled_runs_nothing() {
        let dir = flake_project("develop");
        let fake = installed();
        fake.on_spawn("/n/nix flake show --json --no-write-lock-file", Ran::ok(include_str!("samples/flake-show.json")));
        fake.on_pick_nothing("Flake output to build");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        t.command("nix.develop").unwrap();
        assert!(fake.take().iter().any(|c| c.contains("/n/nix develop") && c.contains("terminal")));
        t.command("nix.flake.build").unwrap();
        assert!(!fake.take().iter().any(|c| c.starts_with("task ")), "Esc runs nothing");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_flake_that_wont_show_says_why_and_a_project_without_one_refuses() {
        let dir = flake_project("show-fails");
        let fake = installed();
        fake.on_spawn("/n/nix flake show --json --no-write-lock-file", Ran::fails(1, "error: flake.lock is out of date\n"));
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        fake.take();
        t.command("nix.flake.run").unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.starts_with("notify") && c.contains("nix flake show failed: error: flake.lock is out of date")), "{calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("pick")));
        std::fs::remove_dir_all(&dir).unwrap();

        let dir = temp("show-none");
        std::fs::write(dir.join("default.nix"), "{ }").unwrap();
        let fake = installed();
        let mut t = pack(&dir, &fake);
        assert_eq!(t.command("nix.flake.build").unwrap_err(), "this project has no flake.nix");
        assert!(t.command("nix.develop").is_err(), "no shell.nix or flake.nix");
        assert!(t.command("nix.nothing").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_tasks_follow_the_shape_of_the_project() {
        let fake = installed();
        let dir = temp("shape-none");
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        assert!(!fake.take().iter().any(|c| c.starts_with("tasks")), "an empty folder has none");

        // A flake appears.
        std::fs::write(dir.join("flake.nix"), "{ }").unwrap();
        t.event(Event::Saved("flake.nix".into()));
        assert!(fake.take().iter().any(|c| c == "tasks check, build, run, flake show, flake update (rewrites flake.lock)"));
        std::fs::remove_dir_all(&dir).unwrap();

        // A NixOS flake with a home-manager one.
        let dir = temp("shape-nixos");
        std::fs::write(dir.join("flake.nix"), "{ outputs = _: { nixosConfigurations.a = 1; homeConfigurations.b = 2; }; }").unwrap();
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        let tasks: Vec<String> = fake.tasks().iter().map(|t| t.name.clone()).collect();
        assert_eq!(tasks, ["check", "build", "run", "flake show", "nixos build", "nixos dry-build", "home-manager build", "flake update (rewrites flake.lock)"]);
        assert!(!tasks.iter().any(|t| t.contains("switch")));
        std::fs::remove_dir_all(&dir).unwrap();

        // A plain default.nix.
        let dir = temp("shape-plain");
        std::fs::write(dir.join("default.nix"), "{ }").unwrap();
        let mut t = pack(&dir, &fake);
        t.activate().unwrap();
        let tasks: Vec<String> = fake.tasks().iter().map(|t| t.name.clone()).collect();
        assert_eq!(tasks, ["build", "shell (build only)"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
