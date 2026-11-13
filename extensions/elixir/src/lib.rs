//! Elixir in Fenix, on lang-kit: the grammars (their language files and
//! queries), Expert as the language server, `mix format` as the formatter,
//! the compiler's warnings and Credo's on save, `mix`'s tasks, and the
//! project doctor's checks, each with a fix row where there's something to
//! install.
//!
//! What it knows of the project is read from its files ([`project`]), and
//! what's installed is found ([`tools`]). A fix that installs a tool, a
//! setting changed, or `mix.exs` saved sets the tasks, checks and linters
//! again.

pub mod doctor;
pub mod lint;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

use api::{export, Command, Event, Extension, Key, MouseEvent, Page};
use fenix_lang_kit::{Fenix, Host, Kit};

use project::Project;
use tools::{Platform, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's names for the languages: the grammars'.
const ELIXIR: &[&str] = &["elixir"];
const HEEX: &[&str] = &["heex"];

/// Files whose saving changes what the project is.
const PROJECT_FILES: &[&str] = &["mix.exs", ".formatter.exs", ".credo.exs"];

/// What the pack's settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `mix compile --all-warnings` on a saved file.
    pub compiler_warnings: bool,
    /// Credo on a saved file, in a project that has it.
    pub credo: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { compiler_warnings: true, credo: true }
    }
}

impl Settings {
    /// From Fenix: a setting that isn't `false` is on.
    pub fn read() -> Settings {
        let on = |key: &str| !api::setting(key).is_some_and(|v| v.trim().eq_ignore_ascii_case("false"));
        Settings { compiler_warnings: on("elixir.compiler_warnings"), credo: on("elixir.credo") }
    }
}

/// `mix format` as Fenix runs it: the buffer's text on stdin, the
/// formatted text on stdout, in the project's folder so `.formatter.exs`
/// applies. `--no-compile` keeps Mix from printing a compile into the
/// text; `--stdin-filename` is how a plugin knows it's a `.heex`.
pub fn formatter(mix: &Command) -> Command {
    mix.clone().args(["format", "--no-compile", "--stdin-filename", "${file}", "-"])
}

pub struct Elixir {
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
    set_heex_formatter: Option<Command>,
}

impl Default for Elixir {
    fn default() -> Elixir {
        let platform = api::platform();
        Elixir::new(Box::new(Fenix), PROJECT, Settings::read, Platform::new(&platform.os, &platform.arch))
    }
}

impl Elixir {
    /// The pack for the project in `root`, talking to `host`, on
    /// `platform`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings, platform: Platform) -> Elixir {
        let tools = Tools::find(host.as_mut());
        let mut kit = Kit::with_host("elixir", host);
        for install in platform.installs() {
            kit.installer.add(install);
        }
        let mut e = Elixir {
            kit,
            root: root.into(),
            project: Project::read(root),
            tools,
            platform,
            settings: read_settings(),
            read_settings,
            set_server: None,
            set_formatter: None,
            set_heex_formatter: None,
        };
        e.configure();
        e
    }

    /// The kit's tasks, checks and linters, for the project, tools and
    /// settings as they are.
    fn configure(&mut self) {
        self.kit.tasks = tasks::tasks(&self.project, &self.tools, &self.root);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(&self.project, &self.platform, &self.tools) {
            self.kit.doctor.program(program);
        }
        self.kit.lint.linters.clear();
        if let (true, Some(mix)) = (self.project.is_elixir(), &self.tools.mix) {
            let mix = Command::new(mix);
            if self.settings.compiler_warnings {
                self.kit.lint.add(lint::compile(&mix));
            }
            if self.settings.credo && self.project.credo {
                self.kit.lint.add(lint::credo(&mix));
            }
        }
    }

    /// The server and the formatters set in Fenix, where they changed.
    /// Nothing found leaves Elixir without them; HEEx has a formatter only
    /// when the project's `.formatter.exs` has the plugin that formats it.
    fn apply(&mut self) {
        let server = self.tools.expert.as_deref().map(|e| Command::new(e).arg("--stdio"));
        let mix = self.tools.mix.as_deref().map(Command::new);
        let formatter = mix.as_ref().map(formatter);
        let heex = formatter.clone().filter(|_| self.project.heex_formatter);
        let host = self.kit.host.as_mut();
        let languages = |names: &[&str]| -> Vec<String> { names.iter().map(|l| l.to_string()).collect() };
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages(ELIXIR), server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
        }
        if formatter != self.set_formatter {
            if let Err(why) = host.formatter(&languages(ELIXIR), formatter.as_ref()) {
                host.log(&format!("formatter: {why}"));
            }
            self.set_formatter = formatter;
        }
        if heex != self.set_heex_formatter {
            if let Err(why) = host.formatter(&languages(HEEX), heex.as_ref()) {
                host.log(&format!("heex formatter: {why}"));
            }
            self.set_heex_formatter = heex;
        }
    }

    /// The tools as they're found now; set again, and checked again,
    /// where that changed.
    fn find_again(&mut self) {
        let mut tools = Tools::find(self.kit.host.as_mut());
        // What the doctor found stands, so a fix's install is used.
        tools.expert = tools.expert.or_else(|| self.tools.expert.clone());
        if tools != self.tools {
            self.take(tools);
            self.kit.doctor.run(self.kit.host.as_mut());
        }
    }

    /// What the doctor found since: a tool a fix installed is used.
    fn follow(&mut self) {
        let mut tools = self.tools.clone();
        for program in &self.kit.doctor.programs {
            if let (Some(slot), Some(path)) = (tools.slot(&program.name), self.kit.doctor.found.get(&program.name)) {
                *slot = Some(path.clone());
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

    fn set_tasks(&mut self) {
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
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
}

impl Extension for Elixir {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            None if id == "elixir.iex" => {
                if !self.project.is_elixir() {
                    return Err("there's no mix.exs at the project's top".into());
                }
                if self.tools.iex.is_none() {
                    return Err("iex isn't installed (or isn't on PATH)".into());
                }
                api::host::run_in_terminal(&tasks::iex(&self.tools), "iex -S mix", false).map(|_| ())
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
        let saved = name.as_deref().is_some_and(|n| PROJECT_FILES.contains(&n));
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("elixir."));
        let installing = self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        if saved {
            self.reload();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.configure();
        }
        let installed = installing && !self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        if installed || task_ended {
            // What a fix put here, or a task (asdf's, mise's) did.
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

export!(Elixir);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;
    use api::Health;
    use fenix_test_kit::{Fake, Harness, Ran};

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        fenix_test_kit::line(cmd)
    }

    /// The pack for the project in `dir`, on `fake`, its tools folder in
    /// `dir` too.
    fn pack(dir: &std::path::Path, fake: &Fake, settings: fn() -> Settings) -> Harness<Elixir> {
        let mut e = Elixir::new(fake.host(), &dir.to_string_lossy(), settings, Platform::new("linux", "x86_64"));
        e.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
        Harness::wrap(fake.clone(), e)
    }

    /// The processes started, as lines, in order.
    fn spawned(fake: &Fake) -> Vec<String> {
        fake.spawned().into_iter().map(|(_, l)| l).collect()
    }

    /// A Mix project as `mix new` makes it, with `mix.exs` as `deps` says.
    fn project(name: &str, mix_exs: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(dir.join("mix.exs"), mix_exs).unwrap();
        std::fs::write(dir.join("lib/orbit.ex"), include_str!("samples/orbit.ex")).unwrap();
        dir
    }

    const PLAIN: &str = "defmodule Orbit.MixProject do\n  use Mix.Project\n\n  def project do\n    [app: :orbit, version: \"0.1.0\", elixir: \"~> 1.15\", deps: []]\n  end\nend\n";

    /// Elixir, Hex, Mix and Expert installed, saying their versions.
    fn installed() -> Fake {
        let fake = Fake::new();
        fake.program("mix", "/t/mix")
            .program("elixir", "/t/elixir")
            .program("iex", "/t/iex")
            .program("expert", "/t/expert")
            .on_spawn("elixir --short-version", Ran::ok("1.20.5\n"))
            .on_spawn("mix hex.info", Ran::ok(include_str!("samples/mix-hex-info.txt")))
            .on_spawn("expert --version", Ran::ok("0.2.3\n"));
        fake
    }

    #[test]
    fn expert_mix_format_and_the_mix_tasks_are_set() {
        let dir = project("activate", PLAIN);
        let fake = installed();
        let mut e = pack(&dir, &fake, Settings::default);
        e.activate().unwrap();
        let calls = fake.take();
        for want in [
            "tasks deps.get, compile, test, test --failed, format --check-formatted, format",
            "server elixir: /t/expert --stdio",
            "formatter elixir: /t/mix format --no-compile --stdin-filename ${file} -",
        ] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert!(!calls.iter().any(|c| c.starts_with("formatter heex")), "no HEEx plugin in .formatter.exs: {calls:?}");

        // The doctor's checks, each asked for its version.
        let spawned = spawned(&fake);
        for want in ["/t/elixir --short-version", "/t/mix hex.info", "/t/expert --version"] {
            assert!(spawned.iter().any(|l| l == want), "{want}: {spawned:?}");
        }
        let checks: Vec<(String, Health, String)> = fake.checks().iter().map(|c| (c.label.clone(), c.health, c.detail.clone())).collect();
        assert_eq!(checks, [("Elixir".into(), Health::Ok, "1.20.5 (/t/elixir)".into()), ("Hex".into(), Health::Ok, "2.2.1".into()), ("Expert".into(), Health::Ok, "0.2.3".into())]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_phoenix_project_has_its_server_task_and_formats_heex_with_its_plugin() {
        let dir = project("phoenix", include_str!("samples/mix.exs.phoenix"));
        std::fs::write(dir.join(".formatter.exs"), "[plugins: [Phoenix.LiveView.HTMLFormatter], inputs: [\"*.{heex,ex,exs}\"]]\n").unwrap();
        let fake = installed();
        let mut e = pack(&dir, &fake, Settings::default);
        e.activate().unwrap();
        let calls = fake.take();
        for want in ["tasks deps.get, compile, test, test --failed, format --check-formatted, format, credo, phx.server", "formatter heex: /t/mix format --no-compile --stdin-filename ${file} -"] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert_eq!(e.ext.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["mix compile", "credo"]);

        // mix.exs saved without Phoenix: the task goes, and so does Credo.
        std::fs::write(dir.join("mix.exs"), PLAIN).unwrap();
        e.event(Event::Saved("mix.exs".into()));
        assert!(fake.take().iter().any(|c| c == "tasks deps.get, compile, test, test --failed, format --check-formatted, format"));
        assert_eq!(e.ext.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["mix compile"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_saved_file_is_compiled_and_its_warnings_are_problems() {
        let dir = project("lint", PLAIN);
        let fake = installed();
        fake.on_spawn("/t/mix compile --all-warnings", Ran::ok("Compiling 2 files (.ex)\n").stderr(include_str!("samples/mix-compile-warnings.txt")));
        let mut e = pack(&dir, &fake, Settings::default);
        e.activate().unwrap();
        fake.take();
        let path = format!("{}/lib/orbit.ex", dir.display());
        fake.in_file(&path, "elixir", "defmodule Orbit do\nend\n").project("lint", &dir.to_string_lossy());
        e.saved(&path);
        let calls = fake.take();
        assert!(calls.contains(&"problems /project/lib/orbit.ex 3".to_string()), "{calls:?}");
        let problems = fake.problems("/project/lib/orbit.ex");
        assert_eq!((problems.len(), problems[0].start, problems[0].end, problems[0].source.as_deref()), (3, (10, 4), (10, 10), Some("mix compile")));
        let run = fake.process("/t/mix compile --all-warnings").expect("mix compile run");
        assert!(run.stdin_closed, "its stdin closed, so it doesn't wait");

        // The warnings fixed: the file is made clean.
        fake.on_spawn("/t/mix compile --all-warnings", Ran::ok("Compiling 1 file (.ex)\nGenerated orbit app\n"));
        e.saved(&path);
        assert!(fake.take().contains(&"problems /project/lib/orbit.ex 0".to_string()));

        // Off: no linter.
        e.ext.read_settings = || Settings { compiler_warnings: false, credo: false };
        e.event(Event::SettingChanged("elixir.compiler_warnings".into()));
        assert!(e.ext.kit.lint.linters.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn credo_runs_on_the_saved_file_when_the_project_has_it() {
        let dir = project("credo", include_str!("samples/mix.exs.phoenix"));
        let fake = installed();
        fake.on_spawn("/t/mix compile --all-warnings", Ran::ok(""));
        fake.on_spawn("/t/mix credo lib/orbit/crew.ex --format json", Ran::fails(10, "").stdout(include_str!("samples/credo.json")));
        let mut e = pack(&dir, &fake, Settings::default);
        e.activate().unwrap();
        fake.take();
        let path = format!("{}/lib/orbit/crew.ex", dir.display());
        fake.in_file(&path, "elixir", "defmodule Orbit.Crew do\nend\n").project("credo", &dir.to_string_lossy());
        e.saved(&path);
        assert!(fake.take().contains(&"problems /project/lib/orbit/crew.ex 3".to_string()));
        let problems = fake.problems("/project/lib/orbit/crew.ex");
        assert_eq!(problems.iter().filter(|p| p.source.as_deref() == Some("credo")).count(), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn iex_opens_in_a_terminal() {
        let dir = project("iex", PLAIN);
        let fake = installed();
        let mut e = pack(&dir, &fake, Settings::default);
        e.activate().unwrap();
        fake.take();
        e.command("elixir.iex").unwrap();
        let calls = fake.take();
        assert!(calls.len() == 1 && calls[0].starts_with("terminal ") && calls[0].ends_with(" iex -S mix: /t/iex -S mix"), "{calls:?}");

        // Not in a Mix project, or without iex: it says why.
        let empty = temp("iex-none");
        let mut none = pack(&empty, &Fake::new(), Settings::default);
        assert_eq!(none.command("elixir.iex"), Err("there's no mix.exs at the project's top".into()));
        std::fs::write(empty.join("mix.exs"), PLAIN).unwrap();
        none.event(Event::Saved("mix.exs".into()));
        assert_eq!(none.command("elixir.iex"), Err("iex isn't installed (or isn't on PATH)".into()));
        assert!(none.command("elixir.nothing").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&empty).unwrap();
    }

    #[test]
    fn without_expert_its_fix_row_installs_it_and_hexs_runs_a_task() {
        let dir = project("fix", PLAIN);
        let fake = Fake::new();
        fake.program("mix", "/t/mix").program("elixir", "/t/elixir").on_spawn("elixir --short-version", Ran::ok("1.18.3\n")).on_spawn("mix hex.info", Ran::fails(1, "** (Mix) The task \"hex.info\" could not be found"));
        let mut e = pack(&dir, &fake, Settings::default);
        e.activate().unwrap();
        let calls = fake.take();
        assert!(!calls.iter().any(|c| c.starts_with("server")), "nothing found, no server set: {calls:?}");
        let rows: Vec<(String, Health, Option<String>)> = fake.checks().iter().map(|c| (c.label.clone(), c.health, c.fix.as_ref().map(|f| f.1.clone()))).collect();
        assert_eq!(
            rows,
            [("Elixir".into(), Health::Ok, None), ("Hex".into(), Health::Warn, Some("elixir.install-hex".into())), ("Expert".into(), Health::Warn, Some("elixir.install.expert".into()))]
        );

        // Hex: a task, which `mix local.hex --force` is.
        e.command("elixir.install-hex").unwrap();
        let calls = fake.take();
        assert!(calls.iter().any(|c| c.contains("/t/mix local.hex --force")), "run as a task: {calls:?}");

        // Expert: its release for this machine, with no checksum to check.
        e.command("elixir.install.expert").unwrap();
        let download = fake.take().into_iter().find(|c| c.starts_with("download ")).unwrap();
        assert!(download.contains("https://github.com/expert-lsp/expert/releases/download/v0.2.3/expert_linux_amd64"), "{download}");
        assert!(!download.contains("sha256"), "unverified: {download}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An install takes minutes: how it ended is a notification, the
    /// newest for the tool in place of the last, saying what failed and
    /// offering it again.
    #[test]
    fn a_failed_install_of_expert_is_a_notification_that_offers_it_again() {
        let dir = temp("install-told");
        let fake = Fake::new();
        fake.on_http_error("https://*", "the server said 404");
        let mut e = pack(&dir, &fake, Settings::default);
        e.activate().unwrap();
        e.command("elixir.install.expert").unwrap();
        assert_eq!(fake.notifications(), ["Downloading Expert 0.2.3…".to_string()]);
        let shown = fake.showing();
        assert_eq!(shown.len(), 1, "{shown:?}");
        let n = &shown[0];
        assert_eq!((n.summary.as_str(), n.urgency, n.tag.as_deref()), ("Couldn't install Expert 0.2.3", api::Urgency::Normal, Some("install.expert")));
        assert_eq!((n.actions[0].label.as_str(), n.actions[0].command.as_str()), ("Try again", "elixir.install.expert"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mix_format_is_the_stdin_form() {
        let cmd = formatter(&Command::new("mix"));
        assert_eq!(line(&cmd), "mix format --no-compile --stdin-filename ${file} -");
    }
}
