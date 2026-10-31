//! Ruby in Fenix, on lang-kit: tree-sitter-ruby's grammar for Fenix to
//! highlight, fold and indent with (`languages/ruby.toml`); Ruby LSP (or
//! Solargraph) as the language server; RuboCop, or Standard, as the
//! formatter and the linter on save; Bundler's, Rake's and the tests'
//! tasks; and the project doctor's checks with their fixes.
//!
//! What it knows of the project is read from its files ([`project`]),
//! and what's installed is found ([`tools`]); the rest is lang-kit's
//! [`Kit`], given this pack's checks, tasks and linter. Rake's tasks are
//! what `rake -T` says, asked in the background. A fix that installs a
//! gem, or a Gemfile saved, sets them again.

pub mod doctor;
pub mod project;
pub mod rubocop;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, MouseEvent, Page};
use fenix_lang_kit::exec::Runs;
use fenix_lang_kit::{Fenix, Host, Kit, Output};

use project::Project;
use rubocop::Style;
use tools::{Server, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language: its language file's.
const RUBY: &[&str] = &["ruby"];

/// Files whose saving changes what the project is.
const PROJECT_FILES: &[&str] = &["Gemfile", "Gemfile.lock", "gems.rb", "gems.locked", ".ruby-version", ".tool-versions", "Rakefile", "rakefile", ".standard.yml"];

/// The pack's own command: the project read again, Rake asked again.
pub const RELOAD: &str = "ruby.reload";

/// Where RuboCop lints (`ruby.lint`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LintMode {
    /// On save, unless Ruby LSP lints with the project's RuboCop itself.
    #[default]
    Auto,
    Save,
    Off,
}

impl LintMode {
    pub fn parse(text: &str) -> LintMode {
        match text.trim() {
            "save" => LintMode::Save,
            "off" => LintMode::Off,
            _ => LintMode::Auto,
        }
    }
}

/// What its settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `ruby.server`.
    pub server: Server,
    /// `ruby.lint`.
    pub lint: LintMode,
    /// `ruby.format`: RuboCop's (or Standard's) layout fixes as the
    /// formatter.
    pub format: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { server: Server::RubyLsp, lint: LintMode::Auto, format: true }
    }
}

impl Settings {
    pub fn read() -> Settings {
        let get = |key: &str| api::setting(key).unwrap_or_default();
        Settings { server: Server::parse(&get("ruby.server")), lint: LintMode::parse(&get("ruby.lint")), format: !get("ruby.format").trim().eq_ignore_ascii_case("false") }
    }
}

/// Whether RuboCop (or Standard) lints each file saved: not when Ruby LSP
/// is the server and the project's bundle has RuboCop, whose problems
/// Ruby LSP gives as you type.
pub fn lints_on_save(settings: &Settings, project: &Project, tools: &Tools) -> bool {
    match settings.lint {
        LintMode::Off => false,
        LintMode::Save => true,
        LintMode::Auto => {
            let ruby_lsp = tools.server(project, settings.server).is_some_and(|(s, _)| s == Server::RubyLsp);
            !(ruby_lsp && Style::of(project) == Style::RuboCop && project.has("rubocop"))
        }
    }
}

/// What the pack runs in the background.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Job {
    RakeTasks,
}

pub struct Ruby {
    pub kit: Kit,
    root: String,
    settings: Settings,
    read_settings: fn() -> Settings,
    project: Project,
    tools: Tools,
    jobs: Runs<Job>,
    /// What `rake -T` listed.
    rake_tasks: Vec<String>,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_formatter: Option<Command>,
}

impl Default for Ruby {
    fn default() -> Ruby {
        Ruby::new(Box::new(Fenix), PROJECT, Settings::read)
    }
}

impl Ruby {
    /// The pack for the project in `root`, talking to `host`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings) -> Ruby {
        let tools = Tools::find(host.as_mut());
        let kit = Kit::with_host("ruby", host);
        let mut r = Ruby { kit, root: root.into(), settings: read_settings(), read_settings, project: Project::read(root), tools, jobs: Runs::new(), rake_tasks: Vec::new(), set_server: None, set_formatter: None };
        r.configure();
        r
    }

    /// The kit's tasks, checks and linter, for the project and tools as
    /// they are.
    fn configure(&mut self) {
        let (project, tools) = (&self.project, &self.tools);
        self.kit.tasks = tasks::tasks(project, tools, &self.rake_tasks, &self.root);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(project, tools, &self.settings) {
            self.kit.doctor.program(program);
        }
        self.kit.lint.linters.clear();
        if let (true, Some((style, cmd))) = (lints_on_save(&self.settings, project, tools), rubocop::command(project, tools)) {
            self.kit.lint.add(rubocop::linter(style, &cmd));
        }
    }

    /// The server and the formatter set in Fenix, where they changed.
    /// Nothing found sets nothing: a grammar's language has no server of
    /// Fenix's own.
    fn apply(&mut self) {
        let server = self.tools.server(&self.project, self.settings.server).map(|(_, cmd)| cmd);
        let formatter = rubocop::command(&self.project, &self.tools).filter(|_| self.settings.format).map(|(_, cmd)| rubocop::formatter(&cmd));
        let languages: Vec<String> = RUBY.iter().map(|l| l.to_string()).collect();
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
        }
        if formatter != self.set_formatter {
            // RuboCop exits 1 when what's left isn't layout: its text is kept.
            if let Err(why) = host.formatter_keeping_output(&languages, formatter.as_ref()) {
                host.log(&format!("formatter: {why}"));
            }
            self.set_formatter = formatter;
        }
    }

    /// Rake's own list of tasks, in the background.
    fn list_rake_tasks(&mut self) {
        self.jobs.kill_where(self.kit.host.as_mut(), |j| *j == Job::RakeTasks);
        let Some(rake) = tasks::rake(&self.project, &self.tools).filter(|_| self.project.rakefile) else {
            self.rake_tasks.clear();
            return;
        };
        if let Err(why) = self.jobs.start(self.kit.host.as_mut(), &rake.arg("-T"), Job::RakeTasks) {
            self.kit.host.log(&format!("rake -T: {why}"));
        }
    }

    fn job_done(&mut self, job: Job, out: Output) {
        match job {
            Job::RakeTasks if out.ok() => self.rake_tasks = tasks::parse_rake(&out.stdout),
            Job::RakeTasks => {
                self.kit.host.log(&format!("rake -T: {}", out.why()));
                return;
            }
        }
        self.kit.tasks = tasks::tasks(&self.project, &self.tools, &self.rake_tasks, &self.root);
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
        }
    }

    /// What the doctor found since: a gem a fix installed is used.
    fn follow(&mut self) {
        let mut tools = self.tools.clone();
        for program in &self.kit.doctor.programs {
            if let Some(slot) = tools.slot(&program.name) {
                *slot = self.kit.doctor.found.get(&program.name).cloned();
            }
        }
        if tools != self.tools {
            let rake_before = tasks::rake(&self.project, &self.tools);
            self.tools = tools;
            self.configure();
            if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
                self.kit.host.log(&format!("tasks: {why}"));
            }
            self.apply();
            if tasks::rake(&self.project, &self.tools) != rake_before {
                self.list_rake_tasks();
            }
        }
    }

    /// The project's files changed: read again, checked again, and Rake
    /// asked again.
    fn reload(&mut self) {
        self.project = Project::read(&self.root);
        self.list_rake_tasks();
        self.configure();
        let host = self.kit.host.as_mut();
        if let Err(why) = self.kit.tasks.apply(host) {
            host.log(&format!("tasks: {why}"));
        }
        self.kit.doctor.run(host);
        self.apply();
    }
}

impl Extension for Ruby {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.list_rake_tasks();
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
            None => Err(format!("no command {id} here")),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        if self.jobs.owns(&event) {
            if let Some((job, out)) = self.jobs.event(&event) {
                self.job_done(job, out);
            }
            return;
        }
        let name = match &event {
            Event::Saved(name) => Some(name.replace('\\', "/").rsplit('/').next().unwrap_or_default().to_string()),
            _ => None,
        };
        // The project's files, or a Rake file of tasks (`lib/tasks/*.rake`).
        let saved = name.as_deref().is_some_and(|n| PROJECT_FILES.contains(&n) || n.ends_with(".rake"));
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("ruby."));
        self.kit.handle(&event);
        if saved {
            self.reload();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.configure();
            self.kit.doctor.run(self.kit.host.as_mut());
            self.apply();
        }
        self.follow();
    }

    fn file_saved(&mut self, path: &str) {
        // `Gemfile.lock` is named like the Gemfile, and isn't Ruby.
        if !(path.ends_with(".lock") || path.ends_with(".locked") || path.ends_with(".json")) {
            self.kit.file_saved(path);
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

export!(Ruby);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{gem_project, temp};
    use fenix_extension_api::Health;
    use fenix_lang_kit::host::mock::Shared;

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    fn found(host: &Shared, programs: &[(&str, &str)]) {
        host.0.borrow_mut().programs.extend(programs.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    }

    fn pack(dir: &std::path::Path, host: &Shared, settings: fn() -> Settings) -> Ruby {
        Ruby::new(Box::new(host.clone()), &dir.to_string_lossy(), settings)
    }

    fn said(r: &mut Ruby, process: u64, text: &str, code: i32) {
        for line in text.lines() {
            r.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: line.into() }));
        }
        r.handle(Event::ProcessExit((process, Some(code))));
    }

    /// The id a program was spawned with, by the end of its command line.
    fn spawned(calls: &[String], ending: &str) -> u64 {
        calls.iter().find_map(|c| c.strip_prefix("spawn ")?.split_once(' ').filter(|(_, l)| l.ends_with(ending))?.0.parse().ok()).unwrap_or_else(|| panic!("{ending}: {calls:?}"))
    }

    const MACHINE: &[(&str, &str)] = &[("ruby", "/r/ruby"), ("gem", "/r/gem"), ("bundle", "/r/bundle"), ("ruby-lsp", "/r/ruby-lsp")];

    #[test]
    fn a_bundled_gem_gets_its_server_formatter_tasks_and_checks() {
        let dir = gem_project("activate");
        let host = Shared::default();
        found(&host, MACHINE);
        let mut r = pack(&dir, &host, Settings::default);
        r.activate().unwrap();
        let calls = host.take();
        assert_eq!(calls[0], "tasks bundle install, test, rubocop, rubocop: correct");
        assert!(calls.contains(&"server ruby: /r/ruby-lsp".to_string()), "{calls:?}");
        assert!(calls.contains(&"formatter ruby: /r/bundle exec rubocop --fix-layout --format quiet --stderr --stdin ${file} (kept)".to_string()), "{calls:?}");
        assert!(r.kit.lint.linters.is_empty(), "Ruby LSP lints with the project's RuboCop");

        // Each says what it found, and the checks are set.
        said(&mut r, spawned(&calls, "/r/ruby -e print RUBY_VERSION"), "3.4.7", 0);
        said(&mut r, spawned(&calls, "/r/bundle --version"), "Bundler version 4.0.18", 0);
        said(&mut r, spawned(&calls, "/r/bundle check --dry-run"), "The following gems are missing\n * rubocop (1.92.2)", 1);
        said(&mut r, spawned(&calls, "/r/ruby-lsp --version"), "0.26.11", 0);
        said(&mut r, spawned(&calls, "/r/bundle exec rubocop --version"), "", 7);
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(checks, [("Ruby".into(), Health::Ok), ("Bundler".into(), Health::Ok), ("Gems".into(), Health::Warn), ("Language server".into(), Health::Ok), ("RuboCop".into(), Health::Warn)]);
        assert_eq!(host.0.borrow().checks[2].fix, Some(("Install the gems (bundle install)".into(), doctor::BUNDLE_INSTALL.into())));

        // Rake says its tasks: rows of their own.
        said(&mut r, spawned(&calls, "/r/bundle exec rake -T"), include_str!("samples/rake-T.txt"), 0);
        let calls = host.take();
        assert!(calls.iter().any(|c| c.starts_with("tasks ") && c.contains("rake build, rake build:checksum") && c.ends_with("rake test")), "{calls:?}");
        assert_eq!(line(&host.0.borrow().tasks.iter().find(|t| t.name == "rake release").unwrap().cmd), "/r/bundle exec rake release");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_server_installed_by_its_fix_row_is_set() {
        let dir = temp("fix");
        let host = Shared::default();
        found(&host, &[("ruby", "/r/ruby"), ("gem", "/r/gem"), ("rubocop", "/g/rubocop")]);
        let mut r = pack(&dir, &host, Settings::default);
        r.activate().unwrap();
        let calls = host.take();
        assert!(!calls.iter().any(|c| c.starts_with("server")), "nothing found, nothing set: {calls:?}");
        assert!(!calls.iter().any(|c| c.contains("rake -T")), "no Rakefile");
        assert_eq!(r.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["rubocop"], "no Ruby LSP: RuboCop on save");
        r.run_command(doctor::INSTALL_SERVER).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").and_then(|c| c.split_once(' ')).filter(|(_, rest)| rest.ends_with("/r/gem install ruby-lsp")).map(|(id, _)| id.parse::<u64>().unwrap()));
        found(&host, &[("ruby-lsp", "/g/ruby-lsp")]);
        r.handle(Event::TaskFinished((task.expect("installed as a task"), true)));
        let calls = host.take();
        assert!(calls.contains(&"server ruby: /g/ruby-lsp".to_string()), "{calls:?}");
        assert_eq!(r.kit.lint.linters.len(), 1, "no bundle with RuboCop in it: still on save");

        // A Ruby file saved: RuboCop on it, its corrections offered.
        host.0.borrow_mut().here = Some((format!("{}/lib/orbit.rb", dir.display()), Some(dir.to_string_lossy().into_owned())));
        r.file_saved(&format!("{}/lib/orbit.rb", dir.display()));
        let calls = host.take();
        let id = spawned(&calls, "/g/rubocop --format json --force-exclusion lib/orbit.rb");
        said(&mut r, id, include_str!("samples/rubocop.json"), 1);
        assert_eq!(host.take(), ["problems /project/lib/orbit.rb 6 (5 fixes)"]);
        // The lock isn't Ruby, though it's named like the Gemfile.
        r.file_saved(&format!("{}/Gemfile.lock", dir.display()));
        assert!(host.take().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_choose_the_server_and_turn_formatting_and_linting_off() {
        let dir = gem_project("settings");
        let host = Shared::default();
        found(&host, MACHINE);
        found(&host, &[("solargraph", "/g/solargraph")]);
        let mut r = pack(&dir, &host, Settings::default);
        r.activate().unwrap();
        host.take();
        r.read_settings = || Settings { server: Server::Solargraph, lint: LintMode::Auto, format: false };
        r.handle(Event::SettingChanged("ruby.server".into()));
        let calls = host.take();
        assert!(calls.contains(&"server ruby: /g/solargraph stdio".to_string()), "{calls:?}");
        assert!(calls.contains(&"formatter ruby:  (kept)".to_string()), "taken away: {calls:?}");
        assert_eq!(r.kit.lint.linters.len(), 1, "Solargraph doesn't run the project's RuboCop: on save");
        r.read_settings = || Settings { lint: LintMode::Off, ..Settings::default() };
        r.handle(Event::SettingChanged("ruby.lint".into()));
        assert!(r.kit.lint.linters.is_empty());
        assert!(r.run_command("ruby.nothing").is_err());
        assert_eq!(LintMode::parse("save"), LintMode::Save);
        assert_eq!(LintMode::parse(""), LintMode::Auto);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_the_gemfile_reads_the_project_again() {
        let dir = temp("reload");
        let host = Shared::default();
        found(&host, MACHINE);
        found(&host, &[("rake", "/r/rake")]);
        let mut r = pack(&dir, &host, Settings::default);
        r.activate().unwrap();
        assert!(!r.kit.doctor.programs.iter().any(|p| p.label == "Bundler"));
        host.take();
        std::fs::write(dir.join("Gemfile"), "source \"https://rubygems.org\"\ngem \"rake\"\n").unwrap();
        std::fs::write(dir.join("Rakefile"), "task :hello\n").unwrap();
        r.handle(Event::Saved("Gemfile".into()));
        let calls = host.take();
        assert!(calls.contains(&"tasks bundle install".to_string()), "{calls:?}");
        assert!(r.kit.doctor.programs.iter().any(|p| p.label == "Bundler"), "checked again");
        assert!(!r.kit.doctor.programs.iter().any(|p| p.label == "Gems"), "no lock to check them against yet");
        let id = spawned(&calls, "/r/rake -T");
        // A Rake file saved: asked again, the one before stopped.
        r.handle(Event::Saved("lib/tasks/db.rake".into()));
        let calls = host.take();
        assert!(calls.contains(&format!("kill {id}")), "{calls:?}");
        said(&mut r, spawned(&calls, "/r/rake -T"), "rake hello  # Say hello\n", 0);
        assert!(host.take().contains(&"tasks bundle install, rake hello".to_string()));
        r.run_command(RELOAD).unwrap();
        assert!(host.take().iter().any(|c| c.ends_with("/r/rake -T")), "asked again when you say");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
