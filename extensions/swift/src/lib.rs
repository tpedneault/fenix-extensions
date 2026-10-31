//! Swift in Fenix, on lang-kit: sourcekit-lsp as the language server
//! (the toolchain's, or Xcode's through `xcrun`), swift-format or
//! SwiftFormat as the formatter, SwiftLint on save, SwiftPM's tasks, and
//! the project doctor's checks. Fenix has no Swift of its own: the
//! language file brings tree-sitter-swift's grammar and the pack's
//! queries for it.
//!
//! What it knows of the project is read from its files ([`project`]),
//! and what's installed is found ([`tools`]); the rest is lang-kit's
//! [`Kit`], given this pack's checks, tasks and linter. A fix that
//! installs a tool, `Package.swift` or a formatter's or SwiftLint's
//! config saved, or a setting changed, sets them again.

pub mod doctor;
pub mod lint;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, MouseEvent, Page};
use fenix_lang_kit::{find_version, Fenix, Host, Kit};

use project::Project;
use tools::{Formatter, LintWhen, Platform, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language: the language file's.
const SWIFT: &str = "swift";

/// Files whose saving changes what the project is.
const PROJECT_FILES: &[&str] = &["Package.swift", ".swift-format", ".swiftformat", ".swiftlint.yml", ".swiftlint.yaml"];

/// What the pack's settings say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub formatter: Formatter,
    pub swiftlint: LintWhen,
}

impl Settings {
    /// From Fenix.
    pub fn read() -> Settings {
        let setting = |key: &str| api::setting(key).unwrap_or_default();
        Settings { formatter: Formatter::parse(&setting("swift.formatter")), swiftlint: LintWhen::parse(&setting("swift.swiftlint")) }
    }
}

/// The formatter for `project`: `Auto` made swift-format or SwiftFormat.
pub fn formatter_for(project: &Project, settings: &Settings) -> Formatter {
    match settings.formatter {
        Formatter::Auto if project.swiftformat && !project.swift_format => Formatter::Lockwood,
        Formatter::Auto => Formatter::Apple,
        other => other,
    }
}

/// What Fenix runs on a Swift buffer, when it's found.
pub fn formatter(tools: &Tools, project: &Project, settings: &Settings) -> Option<Command> {
    match formatter_for(project, settings) {
        Formatter::Apple => tools.swift_format(),
        Formatter::Lockwood => tools.swiftformat(),
        _ => None,
    }
}

/// Whether SwiftLint lints `project`'s files as they're saved.
pub fn lints(project: &Project, settings: &Settings) -> bool {
    match settings.swiftlint {
        LintWhen::Always => true,
        LintWhen::Project => project.swiftlint,
        LintWhen::Off => false,
    }
}

pub struct Swift {
    pub kit: Kit,
    root: String,
    project: Project,
    tools: Tools,
    platform: Platform,
    settings: Settings,
    read_settings: fn() -> Settings,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_formatter: Option<Command>,
}

impl Default for Swift {
    fn default() -> Swift {
        let platform = api::platform();
        Swift::new(Box::new(Fenix), PROJECT, Settings::read, Platform::new(&platform.os, &platform.arch))
    }
}

impl Swift {
    /// The pack for the project in `root`, talking to `host`, on
    /// `platform`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings, platform: Platform) -> Swift {
        let tools = Tools::find(host.as_mut(), &platform.os);
        let mut kit = Kit::with_host("swift", host);
        if let Some(install) = platform.swiftlint() {
            kit.installer.add(install);
        }
        let mut s = Swift { kit, root: root.into(), project: Project::read(root), tools, platform, settings: read_settings(), read_settings, set_server: None, set_formatter: None };
        s.configure();
        s
    }

    /// The kit's tasks, checks and linter, for the project, tools and
    /// settings as they are.
    fn configure(&mut self) {
        let (project, tools, settings) = (&self.project, &self.tools, &self.settings);
        let which = formatter_for(project, settings);
        let lint = lints(project, settings);
        let format = formatter(tools, project, settings).map(|cmd| tasks::in_place(&cmd, which == Formatter::Apple));
        self.kit.tasks = tasks::tasks(project, tools, &self.root, format, lint);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(project, tools, &self.platform, (which == Formatter::Apple, which == Formatter::Lockwood, lint)) {
            self.kit.doctor.program(program);
        }
        self.kit.lint.linters.clear();
        if let (true, Some(swiftlint)) = (lint, &tools.swiftlint) {
            self.kit.lint.add(lint::swiftlint(&Command::new(swiftlint)));
        }
    }

    /// The server and the formatter set in Fenix, where they changed.
    /// Nothing found leaves Fenix's own.
    fn apply(&mut self) {
        let server = self.tools.server();
        let formatter = formatter(&self.tools, &self.project, &self.settings);
        let languages = [SWIFT.to_string()];
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

    /// What the doctor found since: a tool a fix installed is used, and
    /// Swift's version says whether it has `swift format`.
    fn follow(&mut self) {
        let mut tools = self.tools.clone();
        for program in &self.kit.doctor.programs {
            if let Some(slot) = tools.slot(&program.name) {
                *slot = self.kit.doctor.found.get(&program.name).cloned();
            }
        }
        if let Some(check) = self.kit.doctor.checks().into_iter().find(|c| c.label == "Swift") {
            if let Some(v) = find_version(&check.detail) {
                tools.swift_major = Some(v.major());
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

impl Extension for Swift {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            None if id == doctor::GET_SWIFT => {
                let page = tools::install_page(&self.platform.os);
                self.kit.host.notify(&format!("Swift: {page}"));
                self.kit.host.open_url(page)
            }
            None => Err(format!("no command {id} here")),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        let name = match &event {
            Event::Saved(name) => Some(name.replace('\\', "/").rsplit('/').next().unwrap_or_default().to_string()),
            _ => None,
        };
        // A config saved, or the first Swift file in a folder that wasn't
        // Swift's.
        let saved = name.as_deref().is_some_and(|n| PROJECT_FILES.contains(&n) || (n.ends_with(".swift") && !self.project.is_swift()));
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("swift."));
        self.kit.handle(&event);
        if saved {
            self.reload();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.reload();
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

export!(Swift);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, write, PACKAGE};
    use fenix_extension_api::Health;
    use fenix_lang_kit::host::mock::Shared;

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    fn found(host: &Shared, programs: &[(&str, &str)]) {
        host.0.borrow_mut().programs.extend(programs.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    }

    fn pack(dir: &std::path::Path, host: &Shared, settings: fn() -> Settings, os: &str) -> Swift {
        let mut s = Swift::new(Box::new(host.clone()), &dir.to_string_lossy(), settings, Platform::new(os, "aarch64"));
        s.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
        s
    }

    fn said(s: &mut Swift, process: u64, text: &str) {
        for line in text.lines() {
            s.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: line.into() }));
        }
        s.handle(Event::ProcessExit((process, Some(0))));
    }

    /// Each program the doctor asked, answered by `answer` from its line.
    fn answer(s: &mut Swift, calls: &[String], answer: impl Fn(&str) -> &'static str) {
        for (id, line) in calls.iter().filter_map(|c| c.strip_prefix("spawn ")?.split_once(' ')).map(|(id, l)| (id.parse::<u64>().unwrap(), l.to_string())) {
            said(s, id, answer(&line));
        }
    }

    fn package(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        write(&dir, &[("Package.swift", PACKAGE), ("Sources/OrbitCtl/main.swift", "print(\"orbit\")\n"), ("Tests/OrbitKitTests/OrbitKitTests.swift", "import Testing\n")]);
        dir
    }

    #[test]
    fn a_package_gets_its_server_formatter_tasks_and_checks() {
        let dir = package("activate");
        let host = Shared::default();
        found(&host, &[("swift", "/h/.local/share/swiftly/bin/swift"), ("sourcekit-lsp", "/h/.local/share/swiftly/bin/sourcekit-lsp")]);
        let mut s = pack(&dir, &host, Settings::default, "linux");
        s.activate().unwrap();
        let calls = host.take();
        assert_eq!(calls[0], "tasks build, build release, run orbitctl, run orbit-server, test, resolve, update, format");
        assert!(calls.contains(&"server swift: /h/.local/share/swiftly/bin/sourcekit-lsp".to_string()), "{calls:?}");
        assert!(calls.contains(&"formatter swift: /h/.local/share/swiftly/bin/swift format format --assume-filename ${file}".to_string()), "{calls:?}");
        assert!(calls.contains(&"spawn 1 /h/.local/share/swiftly/bin/swift --version".to_string()), "{calls:?}");
        assert_eq!(line(&host.0.borrow().tasks[7].cmd), "/h/.local/share/swiftly/bin/swift format format --in-place --recursive Package.swift Sources Tests");

        // Swift 5: no `swift format`, so no formatter.
        answer(&mut s, &calls, |l| if l.ends_with("swift --version") { "Swift version 5.10.1 (swift-5.10.1-RELEASE)" } else { "" });
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(checks, [("Swift".into(), Health::Ok), ("sourcekit-lsp".into(), Health::Ok), ("swift-format".into(), Health::Warn)]);
        let calls = host.take();
        assert!(calls.contains(&"formatter swift: ".to_string()), "taken away: {calls:?}");
        assert!(!host.0.borrow().tasks.iter().any(|t| t.name == "format"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn swiftlint_installed_by_its_fix_row_lints_on_save() {
        let dir = package("swiftlint");
        write(&dir, &[(".swiftlint.yml", "opt_in_rules: [force_unwrapping]\n")]);
        let host = Shared::default();
        found(&host, &[("swift", "/usr/bin/swift"), ("swift-format", "/usr/bin/swift-format")]);
        let mut s = pack(&dir, &host, Settings::default, "linux");
        s.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"formatter swift: /usr/bin/swift-format format --assume-filename ${file}".to_string()), "{calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("server")), "no server found: Fenix's own");
        assert!(s.kit.lint.linters.is_empty());
        let row = s.kit.doctor.programs.iter().find(|p| p.name == "swiftlint").and_then(|p| p.fix.clone());
        assert_eq!(row.map(|f| f.1), Some(doctor::INSTALL_SWIFTLINT.into()));

        // Its fix row downloads the release, checked against its SHA-256.
        s.run_command(doctor::INSTALL_SWIFTLINT).unwrap();
        let calls = host.take();
        assert!(calls.iter().any(|c| c.contains("https://github.com/realm/SwiftLint/releases/download/0.65.1/swiftlint_linux_arm64.zip") && c.ends_with("9ffa52f478e6d8eb485d37d14715ffac90abc81c58f3370d598bf75be05605f8")), "{calls:?}");

        // Found once it's there: the doctor checks again when it ends.
        found(&host, &[("swiftlint", "/t/swiftlint/swiftlint")]);
        s.run_command("swift.doctor").unwrap();
        assert!(host.take().iter().any(|c| c.starts_with("tasks ") && c.contains("lint")));
        assert_eq!(s.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["swiftlint"]);
        host.0.borrow_mut().here = Some((format!("{}/Sources/OrbitCtl/main.swift", dir.display()), Some(dir.to_string_lossy().into_owned())));
        s.file_saved(&format!("{}/Sources/OrbitCtl/main.swift", dir.display()));
        let calls = host.take();
        assert!(calls.iter().any(|c| c.ends_with("/t/swiftlint/swiftlint lint --reporter json --quiet --force-exclude Sources/OrbitCtl/main.swift")), "{calls:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_and_configs_choose_the_formatter_and_linter() {
        let dir = package("settings");
        write(&dir, &[(".swiftformat", "--indent 4\n")]);
        let host = Shared::default();
        found(&host, &[("swift", "/usr/bin/swift"), ("swiftformat", "/h/swiftformat"), ("swiftlint", "/h/swiftlint")]);
        let mut s = pack(&dir, &host, Settings::default, "linux");
        s.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"formatter swift: /h/swiftformat --stdinpath ${file} --quiet".to_string()), "a .swiftformat: SwiftFormat; {calls:?}");
        assert!(s.kit.lint.linters.is_empty(), "no .swiftlint.yml");
        assert_eq!(s.kit.doctor.programs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["Swift", "sourcekit-lsp", "SwiftFormat"]);

        s.read_settings = || Settings { formatter: Formatter::Apple, swiftlint: LintWhen::Always };
        s.handle(Event::SettingChanged("swift.formatter".into()));
        let calls = host.take();
        assert!(calls.contains(&"formatter swift: /usr/bin/swift format format --assume-filename ${file}".to_string()), "{calls:?}");
        assert_eq!(s.kit.lint.linters.len(), 1, "always");
        assert_eq!(s.kit.doctor.programs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["Swift", "sourcekit-lsp", "swift-format", "SwiftLint"]);

        s.read_settings = || Settings { formatter: Formatter::Off, swiftlint: LintWhen::Off };
        s.handle(Event::SettingChanged("swift.swiftlint".into()));
        assert!(host.take().contains(&"formatter swift: ".to_string()));
        assert!(s.kit.lint.linters.is_empty());
        assert!(s.run_command("swift.nothing").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn on_macos_xcodes_server_is_used_and_swift_org_says_how_to_get_swift() {
        let dir = temp("macos");
        let host = Shared::default();
        found(&host, &[("xcrun", "/usr/bin/xcrun"), ("brew", "/opt/homebrew/bin/brew")]);
        let mut s = pack(&dir, &host, Settings::default, "macos");
        s.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"server swift: /usr/bin/xcrun sourcekit-lsp".to_string()), "{calls:?}");
        assert!(s.kit.doctor.programs.is_empty() && !calls.iter().any(|c| c.starts_with("tasks")), "not a Swift project: nothing to check or run");

        // The first Swift file saved makes it one.
        write(&dir, &[("hello.swift", "print(\"hi\")\n")]);
        s.handle(Event::Saved("hello.swift".into()));
        assert_eq!(s.kit.doctor.programs.iter().map(|p| (p.label.as_str(), p.name.as_str())).collect::<Vec<_>>(), [("Swift", "swift"), ("sourcekit-lsp", "xcrun"), ("swift-format", "swift-format")]);
        assert!(!s.kit.doctor.programs[0].required, "no package to build");
        host.take();
        s.run_command(doctor::GET_SWIFT).unwrap();
        assert_eq!(host.take(), ["notify Swift: https://www.swift.org/install/macos/", "url https://www.swift.org/install/macos/"]);
        s.run_command(doctor::INSTALL_SWIFT_FORMAT).unwrap();
        assert!(host.take().iter().any(|c| c.ends_with("/opt/homebrew/bin/brew install swift-format")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
