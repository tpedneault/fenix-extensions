//! Python in Fenix, on lang-kit: basedpyright (or pyright) as the
//! language server with ruff's beside it (or ruff on save), debugpy as the
//! debug adapter with launches for the project's scripts, ruff as the
//! formatter, the tasks, the project doctor's checks, and the package
//! manager page, for uv's projects and pip's.
//!
//! What it knows of the project is read from its files ([`project`]),
//! and what's installed is found ([`tools`]); the rest is lang-kit's
//! [`Kit`], given this pack's package managers, checks, tasks and linter.
//! A fix that installs a tool, or a `pyproject.toml` saved, sets them
//! again.

pub mod debug;
pub mod doctor;
pub mod packages;
pub mod project;
pub mod provider;
pub mod ruff;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use std::cell::RefCell;
use std::rc::Rc;

use fenix_extension_api::serde_json::{json, Value};
use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, LaunchConfig, MouseEvent, Page};
use fenix_lang_kit::exec::Runs;
use fenix_lang_kit::{Fenix, Host, Kit, PackageManager};

use project::Project;
use ruff::Mode;
use tools::{Server, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language.
const PYTHON: &[&str] = &["python"];

/// What the test provider asks of the project's Python, for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// `python -m pytest --version`: whether pytest is installed, and which.
    Pytest,
}

/// Files whose saving changes what pytest's rootdirs and tests are.
const PYTEST_FILES: &[&str] = &["pytest.ini", "tox.ini", "setup.cfg", "setup.py", "pyproject.toml"];

/// Files whose saving changes what the project is.
const PROJECT_FILES: &[&str] = &["pyproject.toml", "uv.lock", "requirements.txt", "requirements-dev.txt", "manage.py", ".python-version"];

/// The package manager for `project`: uv's for a project it manages,
/// else the venv's pip.
pub fn manager(project: &Project, tools: &Tools, root: &str) -> Box<dyn PackageManager> {
    match &tools.uv {
        Some(_) if project.uv => Box::new(packages::Uv { uv: tools.uv(), root: root.into() }),
        _ => Box::new(packages::Pip::new(tools.uv.as_ref().map(|_| tools.uv()), tools.python(), root)),
    }
}

/// debugpy over stdio: uv brings it into the project's environment for
/// the session, so it needn't be installed there; else the Python's own.
pub fn adapter(project: &Project, tools: &Tools) -> Option<Command> {
    let args = ["python", "-m", "debugpy.adapter"];
    match (&tools.uv, &tools.python) {
        (Some(_), _) if project.uv => Some(tools.uv().args(["run", "--with", "debugpy"]).args(args)),
        (Some(_), None) => Some(tools.uv().args(["run", "--no-project", "--with", "debugpy"]).args(args)),
        (_, Some(_)) => Some(tools.python().args(args[1..].iter().copied())),
        (None, None) => None,
    }
}

/// What its settings say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// `python.server`.
    pub server: Server,
    /// `python.ruff`.
    pub ruff: Mode,
    /// `python.interpreter`: the Python you chose, if you did.
    pub interpreter: Option<String>,
}

impl Settings {
    pub fn read() -> Settings {
        let get = |key: &str| api::setting(key).unwrap_or_default();
        let interpreter = get("python.interpreter");
        Settings { server: Server::parse(&get("python.server")), ruff: Mode::parse(&get("python.ruff")), interpreter: Some(interpreter.trim().to_string()).filter(|i| !i.is_empty()) }
    }
}

/// What the language server is given when it asks
/// (`workspace/configuration`): the Python you chose, over the one Fenix
/// finds.
pub fn server_settings(interpreter: Option<&str>) -> Option<Value> {
    interpreter.map(|path| json!({ "python": { "pythonPath": path } }))
}

pub struct Python {
    pub kit: Kit,
    root: String,
    settings: Settings,
    project: Project,
    tools: Tools,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_ruff: Option<Command>,
    set_adapter: Option<Command>,
    set_formatter: Option<Command>,
    set_launches: Option<Vec<LaunchConfig>>,
    set_settings: Option<Value>,
    /// What the test provider reads of the project (Fenix 1.6).
    tests: provider::SharedFacts,
    /// `python -m pytest --version`, while it's asked.
    reading: Runs<Ask>,
    /// Activated: the kit has told Fenix the test runner once.
    started: bool,
}

impl Default for Python {
    fn default() -> Python {
        Python::new(Box::new(Fenix), PROJECT, Settings::read())
    }
}

impl Python {
    /// The pack for the project in `root`, talking to `host`.
    pub fn new(mut host: Box<dyn Host>, root: &str, settings: Settings) -> Python {
        let tools = Tools::find(host.as_mut());
        let project = Project::read(root);
        let tests = Rc::new(RefCell::new(provider::Facts { root: root.into(), ..provider::Facts::default() }));
        let kit = Kit::with_host("python", host).packages(manager(&project, &tools, root)).tests(Box::new(provider::Provider::new(tests.clone())));
        let mut p = Python { kit, root: root.into(), settings, project, tools, set_server: None, set_ruff: None, set_adapter: None, set_formatter: None, set_launches: None, set_settings: None, tests, reading: Runs::new(), started: false };
        p.configure();
        p
    }

    /// The kit's tasks, checks and linter, for the project and tools as
    /// they are.
    fn configure(&mut self) {
        let (project, tools) = (&self.project, &self.tools);
        self.kit.tasks = tasks::tasks(project, tools, self.settings.server, &self.root);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(project, tools, self.settings.server, self.settings.ruff) {
            self.kit.doctor.program(program);
        }
        match ruff::ruff(project, tools).filter(|_| self.settings.ruff == Mode::Save) {
            Some(r) => self.kit.lint.add(ruff::linter(&r)),
            None => self.kit.lint.linters.retain(|l| l.name != "ruff"),
        }
        if let Some(page) = self.kit.packages.as_mut() {
            page.manager = manager(project, tools, &self.root);
        }
        self.share();
    }

    /// How pytest is run, as the `test` task chooses: through uv for a
    /// project uv manages (or where there's no other Python), else the
    /// environment's Python.
    fn pytest_runs_in_uv(&self) -> bool {
        self.tools.uv.is_some() && (self.project.uv || self.tools.python.is_none())
    }

    /// The test provider's facts as they are now, and Fenix told the
    /// runner where it changed.
    fn share(&mut self) {
        let in_uv = self.pytest_runs_in_uv();
        let uv = self.tools.uv.clone().filter(|_| in_uv);
        let with_pytest = in_uv && !self.project.has("pytest");
        // uv's lock says which pytest the project has; for the environment's
        // Python `python -m pytest --version` says it.
        let locked = (in_uv && !with_pytest).then(|| std::fs::read_to_string(format!("{}/uv.lock", self.root)).ok().and_then(|l| provider::locked_version(&l))).flatten();
        let mut f = self.tests.borrow_mut();
        if f.uv != uv || f.with_pytest != with_pytest || f.python != self.tools.python {
            f.missing = false;
            f.pytest = None;
        }
        if in_uv {
            f.pytest = locked;
            f.missing = false;
        }
        f.uv = uv;
        f.with_pytest = with_pytest;
        f.python = self.tools.python.clone();
        drop(f);
        if self.started {
            self.kit.tests_runner_changed();
        }
    }

    /// `python -m pytest --version` in the project's Python, when that's
    /// what runs the tests: the provider's runner is `pytest <version>`, or
    /// says pytest isn't installed.
    fn read_pytest(&mut self) {
        if self.pytest_runs_in_uv() || self.tools.python.is_none() {
            return;
        }
        let cmd = self.tools.python().args(["-m", "pytest", "--version"]).cwd(PROJECT);
        let host = self.kit.host.as_mut();
        self.reading.kill_all(host);
        if let Err(why) = self.reading.start(host, &cmd, Ask::Pytest) {
            host.log(&format!("pytest --version: {why}"));
        }
    }

    /// An event for the pack itself: pytest's version.
    fn own(&mut self, event: &Event) -> bool {
        if !self.reading.owns(event) {
            return false;
        }
        if let Some((Ask::Pytest, out)) = self.reading.event(event) {
            let text = out.text();
            {
                let mut f = self.tests.borrow_mut();
                match provider::pytest_version(&text) {
                    Some(v) => {
                        f.pytest = Some(v);
                        f.missing = false;
                    }
                    None if text.contains("No module named") => {
                        f.pytest = None;
                        f.missing = true;
                    }
                    None => {}
                }
            }
            self.share();
        }
        true
    }

    /// The servers, the adapter, the formatter and the launches set in
    /// Fenix, where they changed. These are found, not installed by the
    /// kit, so they're the pack's to set; nothing found leaves Fenix's own.
    fn apply(&mut self) {
        let server = self.tools.pick_server(self.settings.server).map(|(_, path)| Command::new(path).arg("--stdio"));
        let adapter = adapter(&self.project, &self.tools);
        let found_ruff = ruff::ruff(&self.project, &self.tools);
        let formatter = found_ruff.as_ref().map(ruff::formatter);
        let ruff_server = found_ruff.as_ref().filter(|_| self.settings.ruff == Mode::Server).map(ruff::server);
        let languages: Vec<String> = PYTHON.iter().map(|l| l.to_string()).collect();
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
        }
        if ruff_server != self.set_ruff {
            if let Err(why) = host.add_language_server(ruff::SERVER, &languages, ruff_server.as_ref()) {
                host.log(&format!("ruff server: {why}"));
            }
            self.set_ruff = ruff_server;
        }
        let settings = server_settings(self.settings.interpreter.as_deref());
        if settings != self.set_settings {
            if let Err(why) = host.language_server_settings("python", settings.as_ref()) {
                host.log(&format!("language server settings: {why}"));
            }
            self.set_settings = settings;
        }
        let launches = debug::launches(&self.project);
        if self.set_launches.as_ref() != Some(&launches) {
            if let Err(why) = host.launches(&launches) {
                host.log(&format!("launches: {why}"));
            }
            self.set_launches = Some(launches);
        }
        if adapter != self.set_adapter {
            if let Err(why) = host.debug_adapter(&languages, adapter.as_ref()) {
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

    /// What the doctor found since: a tool a fix installed is used.
    fn follow(&mut self) {
        let checked = |name: &str| self.kit.doctor.programs.iter().any(|p| p.name == name);
        let found = |name: &str| self.kit.doctor.found.get(name).cloned();
        let mut tools = self.tools.clone();
        for (name, slot) in [("uv", &mut tools.uv), ("ruff", &mut tools.ruff), (Server::Basedpyright.program(), &mut tools.basedpyright), (Server::Pyright.program(), &mut tools.pyright)] {
            if checked(name) {
                *slot = found(name);
            }
        }
        if tools != self.tools {
            self.tools = tools;
            self.configure();
            if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
                self.kit.host.log(&format!("tasks: {why}"));
            }
            self.apply();
            self.read_pytest();
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
        self.read_pytest();
    }
}

impl Extension for Python {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.started = true;
        self.apply();
        self.follow();
        self.read_pytest();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = self.kit.run_command(id).unwrap_or_else(|| Err(format!("no command {id} here")));
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        if self.own(&event) {
            return;
        }
        let saved = matches!(&event, Event::Saved(name) if PROJECT_FILES.iter().any(|f| name.replace('\\', "/").rsplit('/').next() == Some(f)));
        // What pytest's rootdirs and tests are is in these files.
        if matches!(&event, Event::Saved(name) if PYTEST_FILES.iter().any(|f| name.replace('\\', "/").rsplit('/').next() == Some(f))) {
            self.tests.borrow_mut().generation += 1;
        }
        self.kit.handle(&event);
        if saved {
            self.reload();
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

    fn file_saved(&mut self, path: &str) {
        self.kit.file_saved(path);
    }
}

export!(Python);

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

    fn uv_project(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::write(dir.join("pyproject.toml"), "[project]\nname = \"orbit\"\nrequires-python = \">=3.11\"\ndependencies = [\"fastapi\"]\n\n[project.scripts]\norbit = \"orbit:main\"\n").unwrap();
        std::fs::write(dir.join("uv.lock"), "version = 1\n").unwrap();
        dir
    }

    fn pack(dir: &std::path::Path, host: &Shared) -> Python {
        pack_with(dir, host, Settings::default())
    }

    fn pack_with(dir: &std::path::Path, host: &Shared, settings: Settings) -> Python {
        let mut p = Python::new(Box::new(host.clone()), &dir.to_string_lossy(), settings);
        if let Some(page) = p.kit.packages.as_mut() {
            page.cache_dir = None;
        }
        p
    }

    fn said(p: &mut Python, process: u64, text: &str) {
        p.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: text.into() }));
        p.handle(Event::ProcessExit((process, Some(0))));
    }

    #[test]
    fn a_uv_project_gets_its_server_adapter_tasks_and_checks() {
        let dir = uv_project("activate");
        let host = Shared::default();
        host.0.borrow_mut().programs.extend([("uv".to_string(), "/u/uv".to_string()), ("basedpyright-langserver".into(), "/u/basedpyright-langserver".into())]);
        let mut p = pack(&dir, &host);
        p.activate().unwrap();
        let calls = host.take();
        assert_eq!(calls[0], "tasks run, test, lint, format, typecheck, sync");
        assert!(calls.contains(&"spawn 1 /u/uv python find --show-version".to_string()), "{calls:?}");
        assert!(calls.contains(&"spawn 3 /u/uv sync --locked --dry-run --offline".to_string()), "{calls:?}");
        assert!(calls.contains(&"server python: /u/basedpyright-langserver --stdio".to_string()));
        assert!(calls.contains(&"adapter python: /u/uv run --with debugpy python -m debugpy.adapter".to_string()));
        assert!(!calls.iter().any(|c| c.starts_with("formatter")), "no ruff, no formatter: Fenix's own stays");
        assert_eq!(line(&host.0.borrow().tasks[0].cmd), "/u/uv run orbit");

        // Each says what it found, and the checks are set.
        said(&mut p, 1, "3.12.7");
        said(&mut p, 2, "uv 0.5.11");
        said(&mut p, 3, "Would make no changes");
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(checks, [("Python".into(), Health::Ok), ("uv".into(), Health::Ok), ("Environment".into(), Health::Ok), ("Language server".into(), Health::Ok), ("ruff".into(), Health::Warn)]);
        assert_eq!(host.0.borrow().checks[4].fix, Some(("Install ruff".into(), doctor::INSTALL_RUFF.into())));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ruff_installed_by_its_fix_row_is_its_server_and_formats() {
        let dir = uv_project("fix");
        let host = Shared::default();
        host.0.borrow_mut().programs.insert("uv".into(), "/u/uv".into());
        let mut p = pack(&dir, &host);
        p.activate().unwrap();
        assert!(!host.take().iter().any(|c| c.starts_with("add ruff")), "no ruff, no server");
        p.run_command(doctor::INSTALL_RUFF).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").and_then(|c| c.split_once(' ')).filter(|(_, rest)| rest.ends_with("/u/uv tool install ruff")).map(|(id, _)| id.parse::<u64>().unwrap()));
        let task = task.expect("installed as a task");
        host.0.borrow_mut().programs.insert("ruff".into(), "/u/ruff".into());
        p.handle(Event::TaskFinished((task, true)));
        let calls = host.take();
        assert!(calls.contains(&"formatter python: /u/ruff format --stdin-filename ${file} -".to_string()), "{calls:?}");
        assert!(calls.contains(&"add ruff python: /u/ruff server".to_string()), "beside basedpyright: {calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("tasks ")), "the tasks use it too");
        assert!(p.kit.lint.linters.is_empty(), "the server lints");

        // Nothing changed: nothing set again.
        p.handle(Event::Saved("app.py".into()));
        assert!(!host.take().iter().any(|c| c.starts_with("add") || c.starts_with("formatter") || c.starts_with("launches")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ruff_on_save_lints_the_saved_file_with_its_fixes() {
        let dir = uv_project("save");
        let host = Shared::default();
        host.0.borrow_mut().programs.extend([("uv".to_string(), "/u/uv".to_string()), ("ruff".into(), "/u/ruff".into())]);
        let mut p = pack_with(&dir, &host, Settings { ruff: Mode::Save, ..Settings::default() });
        p.activate().unwrap();
        assert!(!host.take().iter().any(|c| c.starts_with("add ruff")), "no server on save");
        assert_eq!(p.kit.lint.linters.len(), 1);
        p.file_saved(&format!("{}/src/orbit/app.py", dir.display()));
        host.0.borrow_mut().here = Some((format!("{}/src/orbit/app.py", dir.display()), Some(dir.to_string_lossy().into_owned())));
        p.file_saved(&format!("{}/src/orbit/app.py", dir.display()));
        let calls = host.take();
        let id: u64 = calls.iter().find_map(|c| c.strip_prefix("spawn ")?.strip_suffix(" /u/ruff check --output-format json --no-fix --force-exclude src/orbit/app.py")?.parse().ok()).expect("linted");
        for line in include_str!("samples/ruff-check.json").lines() {
            p.handle(Event::ProcessOutput(api::ProcessOutput { process: id, stderr: false, line: line.into() }));
        }
        p.handle(Event::ProcessExit((id, Some(1))));
        assert_eq!(host.take(), ["problems /project/src/orbit/app.py 4 (1 fixes)"]);
        assert_eq!(host.0.borrow().fixes["/project/src/orbit/app.py"][0].fixes[0].title, "Remove unused import: `os`");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_scripts_are_launches_and_the_interpreter_goes_to_the_server() {
        let dir = uv_project("launches");
        let host = Shared::default();
        host.0.borrow_mut().programs.extend([("uv".to_string(), "/u/uv".to_string()), ("basedpyright-langserver".into(), "/u/bp".into())]);
        let mut p = pack_with(&dir, &host, Settings { interpreter: Some("/opt/py/bin/python3.13".into()), ..Settings::default() });
        p.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"launches Python: orbit (script)".to_string()), "{calls:?}");
        assert!(calls.contains(&r#"settings python: {"python":{"pythonPath":"/opt/py/bin/python3.13"}}"#.to_string()), "{calls:?}");
        assert!(host.0.borrow().launches[0].attributes.contains("from orbit import main"));
        // A script added: the launches set again.
        std::fs::write(dir.join("pyproject.toml"), "[project]\nname = \"orbit\"\n\n[project.scripts]\norbit = \"orbit:main\"\nadmin = \"orbit.admin:run\"\n").unwrap();
        p.handle(Event::Saved("pyproject.toml".into()));
        assert!(host.take().contains(&"launches Python: admin (script), Python: orbit (script)".to_string()));
        assert_eq!(server_settings(None), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_pyproject_reads_the_project_again() {
        let dir = uv_project("reload");
        let host = Shared::default();
        host.0.borrow_mut().programs.extend([("uv".to_string(), "/u/uv".to_string()), ("ruff".into(), "/u/ruff".into())]);
        let mut p = pack(&dir, &host);
        p.activate().unwrap();
        assert!(host.take().contains(&"formatter python: /u/ruff format --stdin-filename ${file} -".to_string()));
        std::fs::write(dir.join("pyproject.toml"), "[project]\nname = \"orbit\"\n\n[dependency-groups]\ndev = [\"ruff\", \"pytest\"]\n").unwrap();
        p.handle(Event::Saved("pyproject.toml".into()));
        let calls = host.take();
        assert!(calls.contains(&"tasks test, lint, format, typecheck, sync".to_string()), "{calls:?}");
        assert!(calls.contains(&"formatter python: /u/uv run ruff format --stdin-filename ${file} -".to_string()), "the project's ruff now: {calls:?}");
        assert_eq!(line(&host.0.borrow().tasks[0].cmd), "/u/uv run pytest");
        assert!(calls.iter().any(|c| c.starts_with("spawn")), "checked again");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_uv_its_pip_and_python_are_used() {
        let dir = temp("pip");
        std::fs::write(dir.join("requirements.txt"), "flask\n").unwrap();
        let host = Shared::default();
        host.0.borrow_mut().programs.insert("python3".into(), "/usr/bin/python3".into());
        let mut p = pack(&dir, &host);
        assert_eq!(p.kit.packages.as_ref().map(|page| page.manager.name().to_string()).as_deref(), Some("pip"));
        p.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"adapter python: /usr/bin/python3 -m debugpy.adapter".to_string()), "{calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("server")), "nothing found: Fenix's pyright stays");
        assert!(p.run_command("python.nothing").is_err());
        assert!(!calls.iter().any(|c| c.starts_with("settings")), "no interpreter chosen: Fenix's own");
        assert!(adapter(&Project::default(), &Tools::default()).is_none());
        let only_uv = Tools { uv: Some("uv".into()), ..Tools::default() };
        assert_eq!(adapter(&Project::default(), &only_uv).map(|c| line(&c)).as_deref(), Some("uv run --no-project --with debugpy python -m debugpy.adapter"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The id of the spawn whose command line is `line`.
    fn spawned(calls: &[String], line: &str) -> u64 {
        calls.iter().find_map(|c| c.strip_prefix("spawn ")?.split_once(' ').filter(|(_, rest)| *rest == line).map(|(id, _)| id.parse().unwrap())).unwrap_or_else(|| panic!("no spawn of {line}: {calls:?}"))
    }

    fn ended(p: &mut Python, process: u64, stdout: &str, stderr: &str, code: i32) {
        for (text, stderr) in [(stdout, false), (stderr, true)] {
            if !text.is_empty() {
                p.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr, line: text.into() }));
            }
        }
        p.handle(Event::ProcessExit((process, Some(code))));
    }

    /// What the test provider last told Fenix: its runner and its problem.
    fn told(host: &Shared) -> Option<(String, Option<String>)> {
        host.0.borrow().test_providers.last().map(|(_, r, p)| (r.clone(), p.clone()))
    }

    /// The runner is `pytest` once Python's `pytest --version` says which, and
    /// again when it says pytest isn't there; Fenix is told only what changed.
    #[test]
    fn the_test_provider_says_pytest_with_its_version_and_why_it_cant_run() {
        let dir = temp("pytest-version");
        std::fs::write(dir.join("requirements.txt"), "flask\npytest\n").unwrap();
        let host = Shared::default();
        host.0.borrow_mut().programs.insert("python3".into(), "/usr/bin/python3".into());
        let mut p = pack(&dir, &host);
        p.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"test-provider python: pytest".to_string()), "{calls:?}");
        assert_eq!(calls.iter().position(|c| c.starts_with("tasks ")), Some(0), "the tasks stay first: the provider comes after");
        let version = spawned(&calls, "/usr/bin/python3 -m pytest --version");
        ended(&mut p, version, "pytest 9.2.0", "", 0);
        assert_eq!(host.take(), ["test-provider python: pytest 9.2.0"]);
        assert_eq!(p.tests.borrow().pytest.as_deref(), Some("9.2.0"));
        // Asked again, the same: Fenix isn't told again.
        p.reload();
        let again = spawned(&host.take(), "/usr/bin/python3 -m pytest --version");
        ended(&mut p, again, "pytest 9.2.0", "", 0);
        assert!(!host.take().iter().any(|c| c.starts_with("test-provider")));
        // Python without pytest: the problem, until it's there.
        p.reload();
        let none = spawned(&host.take(), "/usr/bin/python3 -m pytest --version");
        ended(&mut p, none, "", "/usr/bin/python3: No module named pytest", 1);
        let (runner, problem) = told(&host).unwrap();
        assert_eq!(runner, "pytest");
        assert!(problem.is_some_and(|why| why.starts_with("pytest isn't installed in this environment")));
        p.reload();
        let back = spawned(&host.take(), "/usr/bin/python3 -m pytest --version");
        ended(&mut p, back, "pytest 8.3.1", "", 0);
        assert_eq!(told(&host), Some(("pytest 8.3.1".into(), None)));
        // What says pytest's rootdirs and tests is saved: the provider reads its files again.
        let generation = p.tests.borrow().generation;
        p.handle(Event::Saved("services/api/pytest.ini".into()));
        assert_eq!(p.tests.borrow().generation, generation + 1);
        p.handle(Event::Saved("app.py".into()));
        assert_eq!(p.tests.borrow().generation, generation + 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A project uv manages is run by uv, which brings pytest when the
    /// project doesn't ask for it; the lock says which pytest.
    #[test]
    fn a_uv_project_runs_its_tests_through_uv_and_the_lock_says_which_pytest() {
        let dir = temp("pytest-uv");
        std::fs::write(dir.join("pyproject.toml"), "[project]\nname = \"orbit\"\n\n[dependency-groups]\ndev = [\"pytest\"]\n").unwrap();
        std::fs::write(dir.join("uv.lock"), "version = 1\n\n[[package]]\nname = \"pytest\"\nversion = \"9.2.0\"\n").unwrap();
        let host = Shared::default();
        host.0.borrow_mut().programs.extend([("uv".to_string(), "/u/uv".to_string()), ("python3".into(), "/usr/bin/python3".into())]);
        let mut p = pack(&dir, &host);
        p.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"test-provider python: pytest 9.2.0".to_string()), "{calls:?}");
        assert!(!calls.iter().any(|c| c.contains("-m pytest --version")), "uv runs it: nothing to ask");
        let f = p.tests.borrow().clone();
        assert_eq!((f.uv.as_deref(), f.with_pytest), (Some("/u/uv"), false));
        // Without pytest in the project, uv brings it for the run.
        std::fs::write(dir.join("pyproject.toml"), "[project]\nname = \"orbit\"\n").unwrap();
        p.handle(Event::Saved("pyproject.toml".into()));
        let f = p.tests.borrow().clone();
        assert_eq!((f.uv.as_deref(), f.with_pytest, f.pytest), (Some("/u/uv"), true, None));
        assert_eq!(told(&host), Some(("pytest".into(), None)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_python_or_uv_the_test_provider_says_so() {
        let dir = temp("pytest-nopython");
        let host = Shared::default();
        let mut p = pack(&dir, &host);
        p.activate().unwrap();
        let (runner, problem) = told(&host).unwrap();
        assert_eq!(runner, "pytest");
        assert!(problem.is_some_and(|why| why.starts_with("Python isn't installed")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
