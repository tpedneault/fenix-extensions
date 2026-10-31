//! PHP in Fenix, on lang-kit: tree-sitter-php's grammar (the language
//! file's), Intelephense or Phpactor as the language server, the
//! project's Pint or PHP CS Fixer as the formatter, PHPStan on save,
//! Composer's scripts and the project's tools as tasks, and the project
//! doctor's checks with their fixes.
//!
//! What it knows of the project is read from its files ([`project`]),
//! and what's installed is found ([`tools`]); the rest is lang-kit's
//! [`Kit`]. A fix or an install that ends, a task that changes the
//! project (`composer install`), `composer.json` or a tool's config saved,
//! or a setting changed, sets them again.

pub mod doctor;
pub mod format;
pub mod lint;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, MouseEvent, Page};
use fenix_lang_kit::{Fenix, Host, Kit};

use format::Formatter;
use project::Project;
use tools::{Server, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language, the language file's.
const PHP: &[&str] = &["php"];

/// Files whose saving changes what the project is.
const PROJECT_FILES: &[&str] = &[
    "composer.json", "composer.lock", "pint.json", ".php-cs-fixer.php", ".php-cs-fixer.dist.php",
    "phpstan.neon", "phpstan.neon.dist", "phpstan.dist.neon", "artisan",
];

/// What the pack's settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub server: Server,
    pub formatter: Formatter,
    /// PHPStan on save, where the project uses it.
    pub phpstan: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { server: Server::default(), formatter: Formatter::default(), phpstan: true }
    }
}

impl Settings {
    /// From Fenix.
    pub fn read() -> Settings {
        let setting = |key: &str| api::setting(key).unwrap_or_default();
        Settings {
            server: Server::parse(&setting("php.server")),
            formatter: Formatter::parse(&setting("php.formatter")),
            phpstan: !setting("php.phpstan").trim().eq_ignore_ascii_case("false"),
        }
    }
}

pub struct Php {
    pub kit: Kit,
    root: String,
    project: Project,
    tools: Tools,
    settings: Settings,
    read_settings: fn() -> Settings,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_formatter: Option<Command>,
}

impl Default for Php {
    fn default() -> Php {
        Php::new(Box::new(Fenix), PROJECT, Settings::read)
    }
}

impl Php {
    /// The pack for the project in `root`, talking to `host`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings) -> Php {
        let tools = Tools::find(host.as_mut());
        let kit = Kit::with_host("php", host).install(tools::phpactor_install());
        let mut p = Php { kit, root: root.into(), project: Project::read(root), tools, settings: read_settings(), read_settings, set_server: None, set_formatter: None };
        p.configure();
        p
    }

    /// The kit's tasks, checks and linter, for the project, tools and
    /// settings as they are.
    fn configure(&mut self) {
        self.tools.phpactor_phar = self.kit.installer.installed("phpactor");
        let (project, tools) = (&self.project, &self.tools);
        self.kit.tasks = tasks::tasks(project, tools, &self.root);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(project, tools, self.settings.server, tools.phpactor_phar) {
            self.kit.doctor.program(program);
        }
        // Worked out again each time the doctor runs: a composer install
        // changes vendor.
        let root = self.root.clone();
        let phar = (self.settings.server == Server::Phpactor && tools.phpactor_phar).then(|| doctor::phpactor(true));
        self.kit.doctor.refresh = Some(Box::new(move || doctor::vendor(&Project::read(&root), &root).into_iter().chain(phar.clone()).collect()));
        self.kit.lint.linters.clear();
        if let (true, Some(phpstan)) = (self.settings.phpstan && project.uses_phpstan(), &tools.phpstan) {
            self.kit.lint.add(lint::phpstan(&Command::new(phpstan)));
        }
    }

    /// The server and the formatter set in Fenix, where they changed.
    fn apply(&mut self) {
        let server = self.tools.server(self.settings.server);
        let formatter = format::formatter(&self.project, &self.tools, self.settings.formatter);
        let languages: Vec<String> = PHP.iter().map(|l| l.to_string()).collect();
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

    /// The tools as they're found now, and the PHAR as installed.
    fn find_again(&mut self) {
        let mut tools = Tools::find(self.kit.host.as_mut());
        tools.phpactor_phar = self.kit.installer.installed("phpactor");
        if tools != self.tools {
            self.take(tools);
            self.kit.doctor.run(self.kit.host.as_mut());
        }
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

    /// The project's files changed: read again, and checked again.
    fn reload(&mut self) {
        self.project = Project::read(&self.root);
        self.tools = Tools::find(self.kit.host.as_mut());
        self.configure();
        let host = self.kit.host.as_mut();
        if let Err(why) = self.kit.tasks.apply(host) {
            host.log(&format!("tasks: {why}"));
        }
        self.kit.doctor.run(host);
        self.apply();
    }
}

impl Extension for Php {
    fn activate(&mut self) -> Result<(), String> {
        self.configure();
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            // vendor's fix, when no tool's row has it.
            None if id == doctor::INSTALL => self.kit.host.run_task("composer install", &doctor::composer_install(&self.tools)).map(|_| ()),
            None => Err(format!("no command {id} here")),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        let saved = matches!(&event, Event::Saved(name) if PROJECT_FILES.iter().any(|f| name.replace('\\', "/").rsplit('/').next() == Some(f)));
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("php."));
        let installing = self.kit.installer.is_running("phpactor");
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        // A task may have installed the project's tools (composer install).
        if saved || (task_ended && Project::read(&self.root) != self.project) {
            self.reload();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.configure();
            self.kit.doctor.run(self.kit.host.as_mut());
            self.apply();
        }
        if (installing && !self.kit.installer.is_running("phpactor")) || task_ended {
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

export!(Php);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, write, COMPOSER};
    use fenix_extension_api::Health;
    use fenix_lang_kit::host::mock::Shared;

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    fn found(host: &Shared, programs: &[(&str, &str)]) {
        host.0.borrow_mut().programs.extend(programs.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    }

    fn pack(dir: &std::path::Path, host: &Shared, settings: fn() -> Settings) -> Php {
        let mut p = Php::new(Box::new(host.clone()), &dir.to_string_lossy(), settings);
        p.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
        p
    }

    fn said(p: &mut Php, process: u64, text: &str) {
        p.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: text.into() }));
        p.handle(Event::ProcessExit((process, Some(0))));
    }

    /// Every program spawned, answered: `answer` by what it is.
    fn answer(p: &mut Php, calls: &[String], answer: impl Fn(&str) -> &'static str) {
        for (id, line) in calls.iter().filter_map(|c| c.strip_prefix("spawn ")?.split_once(' ')).map(|(id, l)| (id.parse::<u64>().unwrap(), l.to_string())) {
            said(p, id, answer(&line));
        }
    }

    fn laravel(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        write(&dir, &[("composer.json", COMPOSER), ("composer.lock", include_str!("samples/composer.lock")), ("artisan", ""), ("pint.json", "{\"preset\": \"laravel\"}"), ("phpstan.neon", ""), ("vendor/bin/pint", ""), ("vendor/bin/phpstan", ""), ("vendor/composer/installed.json", include_str!("samples/installed.json"))]);
        dir
    }

    const EVERYTHING: &[(&str, &str)] = &[
        ("php", "/usr/bin/php"),
        ("composer", "/c/composer"),
        ("npm", "/n/npm"),
        (tools::INTELEPHENSE, "/n/intelephense"),
        ("pint", "/p/vendor/bin/pint"),
        ("phpstan", "/p/vendor/bin/phpstan"),
    ];

    #[test]
    fn a_laravel_app_gets_its_server_formatter_linter_tasks_and_checks() {
        let dir = laravel("activate");
        let host = Shared::default();
        found(&host, EVERYTHING);
        let mut p = pack(&dir, &host, Settings::default);
        p.activate().unwrap();
        let calls = host.take();
        assert_eq!(calls[0], "tasks test, analyse, format, serve, install, update, outdated, audit");
        assert!(calls.contains(&"server php: /n/intelephense --stdio".to_string()), "{calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("formatter php: /usr/bin/php -r ") && c.ends_with("-- /p/vendor/bin/pint --quiet")), "{calls:?}");
        assert_eq!(p.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["phpstan"]);

        answer(&mut p, &calls, |line| match line {
            l if l.ends_with("php --version") => "PHP 8.3.6 (cli) (built: Oct 19 2026 15:16:58) (NTS)",
            l if l.contains("composer") => "Composer version 2.8.12 2025-09-19 13:41:59",
            l if l.contains("pint") => "Laravel Pint 1.18.3",
            _ => "PHPStan - PHP Static Analysis Tool 2.1.31",
        });
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(checks, [("PHP".into(), Health::Ok), ("Composer".into(), Health::Ok), ("Intelephense".into(), Health::Ok), ("Pint".into(), Health::Ok), ("PHPStan".into(), Health::Ok), ("vendor".into(), Health::Ok)]);

        // A file saved: PHPStan on it.
        host.0.borrow_mut().here = Some((format!("{}/app/Orbit.php", dir.display()), Some(dir.to_string_lossy().into_owned())));
        p.file_saved(&format!("{}/app/Orbit.php", dir.display()));
        assert!(host.take().iter().any(|c| c.ends_with("/p/vendor/bin/phpstan analyse --error-format=json --no-progress --no-interaction --memory-limit=1G app/Orbit.php")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn intelephense_installed_by_its_fix_row_is_used() {
        let dir = laravel("fix");
        let host = Shared::default();
        found(&host, &[("php", "/usr/bin/php"), ("npm", "/n/npm")]);
        let mut p = pack(&dir, &host, Settings::default);
        p.activate().unwrap();
        assert!(!host.take().iter().any(|c| c.starts_with("server")), "nothing found: Fenix's own");
        p.run_command(doctor::INSTALL_INTELEPHENSE).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").and_then(|c| c.split_once(' ')).filter(|(_, rest)| rest.ends_with("/n/npm install -g intelephense")).map(|(id, _)| id.parse::<u64>().unwrap()));
        found(&host, &[(tools::INTELEPHENSE, "/n/intelephense")]);
        p.handle(Event::TaskFinished((task.expect("installed as a task"), true)));
        assert!(host.take().contains(&"server php: /n/intelephense --stdio".to_string()));

        // composer install, from vendor's row: the project's tools come.
        std::fs::remove_dir_all(dir.join("vendor")).unwrap();
        p.reload();
        assert!(p.kit.lint.linters.is_empty(), "no PHPStan yet");
        p.run_command(doctor::INSTALL).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").and_then(|c| c.split_once(' ')).filter(|(_, rest)| rest.ends_with("composer install")).map(|(id, _)| id.parse::<u64>().unwrap()));
        write(&dir, &[("vendor/bin/phpstan", "")]);
        found(&host, &[("phpstan", "/p/vendor/bin/phpstan")]);
        p.handle(Event::TaskFinished((task.expect("composer install as a task"), true)));
        assert_eq!(p.kit.lint.linters.len(), 1, "PHPStan, installed");
        assert!(host.take().iter().any(|c| c.starts_with("spawn ") && c.ends_with("/p/vendor/bin/phpstan --version")), "checked again");
        assert!(p.run_command("php.nothing").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn phpactor_is_installed_and_run_by_php() {
        let dir = temp("phpactor");
        write(&dir, &[("index.php", "<?php echo 1;\n")]);
        let host = Shared::default();
        found(&host, &[("php", "/usr/bin/php")]);
        let mut p = pack(&dir, &host, || Settings { server: Server::Phpactor, formatter: Formatter::Server, phpstan: false });
        p.activate().unwrap();
        host.take();
        assert_eq!(p.kit.doctor.programs.iter().find(|p| p.name == "phpactor").and_then(|p| p.fix.clone()).map(|f| f.1), Some(doctor::INSTALL_PHPACTOR.into()));
        host.0.borrow_mut().files.insert(tools::phpactor_install().url, b"#!/usr/bin/env php\n<?php\n".to_vec());
        p.run_command(doctor::INSTALL_PHPACTOR).unwrap();
        let calls = host.take();
        let download = calls.iter().find_map(|c| c.strip_prefix("download ").and_then(|c| c.split(' ').next()?.parse::<u64>().ok())).expect("downloaded");
        assert!(calls.iter().any(|c| c.contains(tools::PHPACTOR_SHA256)), "checked against its SHA-256: {calls:?}");
        p.handle(Event::HttpResponse((download, Ok(api::HttpResponse::new(200, Vec::new())))));
        let step = |calls: &[String]| calls.iter().find_map(|c| c.strip_prefix("timer ")?.strip_suffix(" 1")?.parse::<u64>().ok());
        let mut calls = host.take();
        while let Some(t) = step(&calls) {
            p.handle(Event::Timer(t));
            calls = host.take();
        }
        assert!(p.kit.installer.installed("phpactor"));
        assert!(calls.contains(&"server php: /usr/bin/php ${tools}/phpactor/phpactor.phar language-server".to_string()), "{calls:?}");
        assert!(!p.kit.doctor.programs.iter().any(|p| p.name == "phpactor"), "its own check now");
        answer(&mut p, &calls, |_| "PHP 8.3.6 (cli)");
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert!(checks.contains(&("Phpactor".into(), Health::Ok)), "{checks:?}");

        // Off: no server.
        p.read_settings = || Settings { server: Server::Off, ..Settings::default() };
        p.handle(Event::SettingChanged("php.server".into()));
        assert!(host.take().iter().any(|c| c == "server php: "), "taken away");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
