//! What every language pack's code extension shares (API 0.8.3, and the
//! later calls a pack may use through [`Host`]): the package manager
//! page, the doctor's checks, tasks, a server or an adapter installed
//! into the tools folder, a formatter and a linter. A
//! pack says what its language has -- a [`PackageManager`], [`Program`]s,
//! a [`Tasks`] table, [`Install`]s, [`Linter`]s -- and hands its
//! extension's calls to a [`Kit`], which does the rest.
//!
//! Every host call goes through [`Host`]: [`Fenix`] in the sandbox, a
//! test's own on the host, so all of it is tested natively.

pub mod archive;
pub mod cvss;
pub mod doctor;
pub mod exec;
pub mod host;
pub mod install;
pub mod lint;
pub mod packages;
pub mod page;
pub mod tasks;
pub mod version;

use std::collections::HashSet;

use fenix_extension_api::{Check, Command, Event, Key, MouseEvent, Page};

pub use doctor::{Doctor, Extra, Fix, Judge, Program};
pub use exec::{command, split_words, Output};
pub use host::{Fenix, Host};
pub use install::{Install, Installer, Use};
pub use lint::Files;
pub use lint::{compiler_lines, Lint, Linter};
pub use packages::{Advisory, Dependency, Finding, Found, PackageManager, Risk, Steps, Update};
pub use page::PackagesPage;
pub use tasks::Tasks;
pub use version::{find_version, Requirement, Version};

/// The JSON reader packs parse their managers' output with.
pub use serde_json;

/// The README's example, compiled.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct Readme;

/// A pack's language, wired up: give it the extension's calls.
///
/// Its commands, which the pack's manifest lists: `<prefix>.packages`
/// (the page), `<prefix>.doctor` (checks again), `<prefix>.install.<name>`
/// for each [`Install`], and each [`Program`]'s fix command.
pub struct Kit {
    pub prefix: String,
    pub host: Box<dyn Host>,
    pub packages: Option<PackagesPage>,
    pub doctor: Doctor,
    pub tasks: Tasks,
    pub installer: Installer,
    pub lint: Lint,
    pub formatters: Vec<(Vec<String>, Command)>,
    /// Fix rows' tasks running: the doctor checks again when one ends.
    fixing: HashSet<u64>,
}

impl Kit {
    /// A kit whose commands start with `prefix` (`python`), talking to
    /// Fenix.
    pub fn new(prefix: &str) -> Kit {
        Kit::with_host(prefix, Box::new(Fenix))
    }

    pub fn with_host(prefix: &str, host: Box<dyn Host>) -> Kit {
        Kit {
            prefix: prefix.into(),
            host,
            packages: None,
            doctor: Doctor::new(),
            tasks: Tasks::default(),
            installer: Installer::new(),
            lint: Lint::new(),
            formatters: Vec::new(),
            fixing: HashSet::new(),
        }
    }

    /// The package manager its page is for.
    pub fn packages(mut self, manager: Box<dyn PackageManager>) -> Kit {
        self.packages = Some(PackagesPage::new(manager));
        self
    }

    pub fn program(mut self, program: Program) -> Kit {
        self.doctor.program(program);
        self
    }

    pub fn tasks(mut self, tasks: Tasks) -> Kit {
        self.tasks = tasks;
        self
    }

    pub fn install(mut self, install: Install) -> Kit {
        self.installer.add(install);
        self
    }

    /// `cmd` formats `languages`' files (see [`lint::formatter`]).
    pub fn formatter(mut self, languages: &[&str], cmd: Command) -> Kit {
        self.formatters.push((languages.iter().map(|l| l.to_string()).collect(), cmd));
        self
    }

    pub fn linter(mut self, linter: Linter) -> Kit {
        self.lint.add(linter);
        self
    }

    /// The pack's own checks, worked out again by `checks` each time the
    /// doctor runs: its `<prefix>.doctor`, a fix or an install that ended.
    pub fn extra_checks(mut self, checks: impl FnMut() -> Vec<Check> + 'static) -> Kit {
        self.doctor.refresh = Some(Box::new(checks));
        self
    }

    /// A file was saved, by its path (the export `file_saved`, 0.8.4, with
    /// `contributes.events` `saved`): linted. Once a pack gives these,
    /// `Event::Saved`'s names aren't linted too.
    pub fn file_saved(&mut self, path: &str) {
        self.lint.file_saved(self.host.as_mut(), path);
    }

    /// When the extension starts: its tasks and formatters set, what it
    /// installed used, and the checks made. What Fenix refuses (a program
    /// the manifest doesn't name) goes to its output page, and the rest
    /// goes on.
    pub fn activate(&mut self) -> Result<(), String> {
        let host = self.host.as_mut();
        if !self.tasks.tasks.is_empty() {
            if let Err(why) = self.tasks.apply(host) {
                host.log(&format!("tasks: {why}"));
            }
        }
        for (languages, cmd) in &self.formatters {
            if let Err(why) = host.formatter(languages, Some(cmd)) {
                host.log(&format!("formatter {}: {why}", cmd.program));
            }
        }
        self.installer.apply_installed(host);
        self.doctor.run(host);
        Ok(())
    }

    /// One of its commands run; `None` when `id` isn't one of its, so the
    /// pack's own can be tried.
    pub fn run_command(&mut self, id: &str) -> Option<Result<(), String>> {
        let host = self.host.as_mut();
        let own = id.strip_prefix(&self.prefix).and_then(|r| r.strip_prefix('.'));
        match own {
            Some("packages") => {
                let page = self.packages.as_mut()?;
                page.open(host);
                return Some(Ok(()));
            }
            Some("doctor") => {
                self.doctor.run(host);
                return Some(Ok(()));
            }
            Some(rest) => {
                if let Some(name) = rest.strip_prefix("install.") {
                    return Some(self.installer.start(host, name));
                }
            }
            None => {}
        }
        let (program, fix) = self.doctor.fix_for(id)?;
        Some(match fix {
            Fix::Task(cmd) => {
                let label = program.fix.as_ref().map(|f| f.0.clone()).unwrap_or_default();
                host.run_task(&label, cmd).map(|t| {
                    self.fixing.insert(t);
                })
            }
            Fix::Install(name) => {
                let name = name.clone();
                self.installer.start(host, &name)
            }
            Fix::Own => return None,
        })
    }

    /// An event: whether it was for the kit. A `saved` file is linted and
    /// still given back as not its, for the pack to see too.
    pub fn handle(&mut self, event: &Event) -> bool {
        let host = self.host.as_mut();
        if let Some(page) = self.packages.as_mut() {
            if page.handle(host, event) {
                return true;
            }
        }
        if self.doctor.handle(host, event) || self.lint.handle(host, event) {
            return true;
        }
        match self.installer.handle(host, event) {
            install::Handled::No => {}
            install::Handled::Yes => return true,
            install::Handled::Ended(_, result) => {
                if result.is_ok() {
                    self.doctor.run(host);
                }
                return true;
            }
        }
        match event {
            Event::TaskFinished((id, _)) if self.fixing.remove(id) => {
                self.doctor.run(host);
                true
            }
            Event::Saved(name) => {
                self.lint.saved(host, name);
                false
            }
            _ => false,
        }
    }

    fn page_of(&mut self, page: u64) -> Option<&mut PackagesPage> {
        self.packages.as_mut().filter(|p| p.page == Some(page))
    }

    /// Its page `page`, when it's the kit's, `width` by `height`.
    pub fn page(&mut self, page: u64, width: u32, height: u32) -> Option<Page> {
        self.page_of(page).map(|p| p.page(width, height))
    }

    /// A key on its page `page`: whether it was the kit's.
    pub fn page_key(&mut self, page: u64, key: Key) -> bool {
        let Some(p) = self.packages.as_mut().filter(|p| p.page == Some(page)) else { return false };
        p.key(self.host.as_mut(), key.into());
        true
    }

    /// A click or the wheel on its page `page`: whether it was the kit's.
    pub fn page_mouse(&mut self, page: u64, event: &MouseEvent) -> bool {
        let Some(p) = self.packages.as_mut().filter(|p| p.page == Some(page)) else { return false };
        p.mouse(self.host.as_mut(), &event.into());
        true
    }

    pub fn page_state(&mut self, page: u64) -> Option<String> {
        self.page_of(page).map(|p| p.state())
    }

    /// Its page opened again (`page_restore`): whether it was the kit's.
    pub fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if kind != page::KIND {
            return false;
        }
        let Some(p) = self.packages.as_mut() else { return false };
        p.restore(self.host.as_mut(), page, state);
        true
    }

    pub fn page_closed(&mut self, page: u64) {
        if let Some(p) = self.packages.as_mut() {
            p.closed(self.host.as_mut(), page);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::mock::Shared;
    use fenix_extension_api::{Health, HttpResponse, Section};

    fn kit(dir: &std::path::Path) -> (Kit, Shared) {
        let shared = Shared::default();
        let mut kit = Kit::with_host("python", Box::new(shared.clone()))
            .packages(Box::new(packages::tests::Mock::default()))
            .program(Program::new("python3", "Python").at_least("3.9"))
            .program(Program::new("uv", "uv").fix("Install uv", "python.install-uv", Fix::Task(command("pipx install uv"))))
            .program(Program::new("server", "Language server").version_args(Vec::<String>::new()).fix("Install the server", "python.install-server", Fix::Install("server".into())))
            .program(Program::new("ruff", "ruff").optional().fix("Install ruff", "python.ruff", Fix::Own))
            .tasks(Tasks::table(&[("test", "uv run pytest")]))
            .install(Install::new("server", "https://x/server-1.0.tar.gz").strip(1).program("bin/server").language_server(&["python"], ["--stdio"]))
            .formatter(&["python"], command("ruff format --stdin-filename ${file} -"))
            .linter(Linter::new("ruff", &["py"], |f| Command::new("ruff").args(["check", f]), |o, f| Ok(compiler_lines(&o.stdout, f, fenix_extension_api::Severity::Warning))));
        kit.installer.tools = dir.to_string_lossy().into_owned();
        if let Some(p) = kit.packages.as_mut() {
            p.cache_dir = None;
        }
        (kit, shared)
    }

    #[test]
    fn a_pack_is_its_kit_and_a_few_lines() {
        let dir = archive::tests::temp("kit");
        let (mut kit, host) = kit(&dir);
        host.0.borrow_mut().programs.insert("python3".into(), "/usr/bin/python3".into());
        kit.activate().unwrap();
        assert_eq!(
            host.0.borrow_mut().take(),
            ["tasks test", "formatter python: ruff format --stdin-filename ${file} -", "spawn 1 /usr/bin/python3 --version", "close 1", "timer 2 5000"]
        );
        kit.handle(&Event::ProcessOutput(fenix_extension_api::ProcessOutput { process: 1, stderr: false, line: "Python 3.12.0".into() }));
        assert!(kit.handle(&Event::ProcessExit((1, Some(0)))));
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(checks, [("Python".into(), Health::Ok), ("uv".into(), Health::Bad), ("Language server".into(), Health::Bad), ("ruff".into(), Health::Warn)]);
        assert_eq!(host.0.borrow().checks[1].section, Section::Toolchain);

        // A fix row's task, and the checks made again after it.
        assert_eq!(kit.run_command("python.install-uv"), Some(Ok(())));
        assert_eq!(host.0.borrow_mut().take().last().map(String::as_str), Some("task 3 Install uv: pipx install uv"));
        host.0.borrow_mut().programs.insert("uv".into(), "/bin/uv".into());
        assert!(kit.handle(&Event::TaskFinished((3, true))));
        assert!(host.0.borrow_mut().take().contains(&"spawn 4 /usr/bin/python3 --version".to_string()));
        // The pack's own fix, and commands not the kit's, are the pack's.
        assert_eq!(kit.run_command("python.ruff"), None);
        assert_eq!(kit.run_command("python.something"), None);
        assert_eq!(kit.run_command("go.packages"), None);

        // An install from its fix row, then checked again.
        host.0.borrow_mut().files.insert("https://x/server-1.0.tar.gz".into(), archive::tests::tar_gz_bytes());
        assert_eq!(kit.run_command("python.install-server"), Some(Ok(())));
        let download = host.0.borrow_mut().take().iter().find_map(|c| c.strip_prefix("download ").and_then(|c| c.split(' ').next()?.parse::<u64>().ok())).unwrap();
        assert!(kit.handle(&Event::HttpResponse((download, Ok(HttpResponse::new(200, Vec::new()))))));
        // Unpacked a step a timer.
        let step = |calls: &[String]| calls.iter().find_map(|c| c.strip_prefix("timer ")?.strip_suffix(" 1")?.parse::<u64>().ok());
        let mut calls = host.0.borrow_mut().take();
        while let Some(t) = step(&calls) {
            assert!(kit.handle(&Event::Timer(t)));
            calls = host.0.borrow_mut().take();
        }
        assert!(calls.contains(&"server python: /tools/server/bin/server --stdio".to_string()) && calls.iter().any(|c| c.starts_with("spawn")), "{calls:?}");
        assert_eq!(kit.run_command("python.install.server"), Some(Ok(())), "installed again by its own command");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_page_and_the_linter_go_through_the_kit() {
        let dir = archive::tests::temp("kit-page");
        let (mut kit, host) = kit(&dir);
        assert_eq!(kit.run_command("python.packages"), Some(Ok(())));
        let page = host.0.borrow_mut().take().iter().find_map(|c| c.strip_prefix("page ").and_then(|c| c.split(' ').next()?.parse::<u64>().ok())).unwrap();
        assert!(kit.page(page, 80, 12).is_some() && kit.page(page + 100, 80, 12).is_none());
        assert!(kit.page_key(page, Key::Char('2')));
        assert_eq!(kit.page_state(page).as_deref(), Some("1"));
        assert!(!kit.page_key(page + 100, Key::Char('2')));
        kit.page(page, 80, 12);
        let mods = fenix_extension_api::KeyMods { shift: false, ctrl: false, alt: false, meta: false };
        let click = MouseEvent { line: 1, column: 2, cell: 2, action: fenix_extension_api::MouseAction::Press(fenix_extension_api::MousePress { button: fenix_extension_api::MouseButton::Left, clicks: 1 }), mods };
        assert!(kit.page_mouse(page, &click) && !kit.page_mouse(page + 100, &click));
        assert_eq!(kit.page_state(page).as_deref(), Some("0"), "All clicked");
        kit.page_closed(page);
        assert!(kit.page(page, 80, 12).is_none());
        assert!(kit.page_restore(50, page::KIND, "2") && !kit.page_restore(51, "other", ""));
        assert!(kit.page(50, 80, 12).is_some());

        host.0.borrow_mut().here = Some(("/home/u/p/app.py".into(), Some("/home/u/p".into())));
        host.0.borrow_mut().take();
        assert!(!kit.handle(&Event::Saved("app.py".into())), "the pack sees it too");
        assert_eq!(host.0.borrow_mut().take()[0].split_once(' ').map(|(_, c)| c.split_once(' ').unwrap().1.to_string()).as_deref(), Some("ruff check app.py"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_kits_doctor_works_out_the_packs_own_checks_again() {
        let dir = archive::tests::temp("kit-extra");
        let marker = dir.join("built");
        let seen = marker.clone();
        let host = Shared::default();
        let mut kit = Kit::with_host("dotnet", Box::new(host.clone())).extra_checks(move || {
            let health = if seen.exists() { Health::Ok } else { Health::Warn };
            vec![Check::new(Section::Project, "Built", health, "")]
        });
        kit.activate().unwrap();
        assert_eq!(host.0.borrow().checks[0].health, Health::Warn);
        std::fs::write(&marker, "").unwrap();
        assert_eq!(kit.run_command("dotnet.doctor"), Some(Ok(())));
        assert_eq!(host.0.borrow().checks[0].health, Health::Ok, "dotnet.doctor looked again");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_saved_files_path_is_linted_and_the_later_calls_reach_the_host() {
        let dir = archive::tests::temp("kit-saved");
        let (mut kit, host) = kit(&dir);
        kit.file_saved("/project/src/app.py");
        assert!(host.take().iter().any(|c| c.ends_with("ruff check src/app.py")));
        let h = kit.host.as_mut();
        assert_eq!(h.platform().os, "linux");
        h.add_language_server("ruff", &["python".to_string()], Some(&command("ruff server"))).unwrap();
        h.language_server_settings("ruff", Some(&serde_json::json!({"lint": {"preview": true}}))).unwrap();
        h.launches(&[fenix_extension_api::LaunchConfig::launch("Python: orbit", &["python"])]).unwrap();
        h.launch_variable("target", &[fenix_extension_api::PickerItem::new("orbit", "orbit")]).unwrap();
        assert_eq!(h.run_task_background("dev", &command("uv run fastapi dev")), Ok(2));
        assert_eq!(
            host.take(),
            ["add ruff python: ruff server", "settings ruff: {\"lint\":{\"preview\":true}}", "launches Python: orbit", "variable target: orbit", "background 2 dev: uv run fastapi dev"]
        );
        assert_eq!(host.0.borrow().settings["ruff"]["lint"]["preview"], true);
        // What a host that hasn't them does.
        struct Old;
        impl Host for Old {
            fn spawn(&mut self, _: &Command) -> Result<u64, String> { Ok(1) }
            fn close_stdin(&mut self, _: u64) {}
            fn kill(&mut self, _: u64) {}
            fn run_task(&mut self, _: &str, _: &Command) -> Result<u64, String> { Ok(1) }
            fn download(&mut self, _: &fenix_extension_api::HttpRequest, _: &str, _: Option<&str>) -> Result<u64, String> { Ok(1) }
            fn set_executable(&mut self, _: &str) -> Result<(), String> { Ok(()) }
            fn find_program(&mut self, _: &str, _: &[String]) -> Option<String> { None }
            fn set_timer(&mut self, _: u32) -> u64 { 1 }
            fn prompt(&mut self, _: &str, _: &str) -> u64 { 1 }
            fn pick(&mut self, _: &str, _: &[fenix_extension_api::PickerItem]) -> u64 { 1 }
            fn notify(&mut self, _: &str) {}
            fn log(&mut self, _: &str) {}
            fn open_page(&mut self, _: &str, _: &str) -> u64 { 1 }
            fn refresh(&mut self, _: u64) {}
            fn open_text_beside(&mut self, _: &str, _: &str, _: Option<&str>) {}
            fn doctor(&mut self, _: &[Check]) {}
            fn tasks(&mut self, _: &[fenix_extension_api::Task]) -> Result<(), String> { Ok(()) }
            fn language_server(&mut self, _: &[String], _: Option<&Command>) -> Result<(), String> { Ok(()) }
            fn debug_adapter(&mut self, _: &[String], _: Option<&Command>) -> Result<(), String> { Ok(()) }
            fn formatter(&mut self, _: &[String], _: Option<&Command>) -> Result<(), String> { Ok(()) }
            fn diagnostics(&mut self, _: &str, _: &[fenix_extension_api::Problem]) -> Result<(), String> { Ok(()) }
            fn here(&mut self) -> Option<(String, Option<String>)> { None }
            fn now(&mut self) -> u64 { 0 }
        }
        let mut old = Old;
        assert!(old.add_language_server("x", &[], None).is_err() && old.http(&fenix_extension_api::HttpRequest::get("https://x")).is_err());
        assert_eq!(old.diagnostics_with_fixes("/project/a.py", &[]), Ok(()), "the problems, without their fixes");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
