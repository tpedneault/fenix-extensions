//! JavaScript and TypeScript in Fenix, on lang-kit: Fenix's own
//! typescript-language-server kept (the one found used, where Fenix
//! wouldn't look), with the CSS and HTML servers, Tailwind's,
//! ESLint's (given its settings) and Biome's beside it
//! (`language-server-add`, 0.8.5); js-debug as the debug adapter over a
//! socket (`debug-adapter-connect`, 0.8.4), a launch for each script;
//! Prettier or Biome as the formatter, ESLint on save where its server
//! isn't; `package.json`'s scripts as tasks, the dev server in the
//! background; the project doctor's checks; and the package manager page
//! for npm, pnpm, Yarn and bun.
//!
//! What it knows of the project is read from its files ([`project`]),
//! and what's installed is found ([`tools`]); the rest is lang-kit's
//! [`Kit`]. A fix that installs something, an install, or a
//! `package.json` saved, sets them again.

pub mod debug;
pub mod doctor;
pub mod lint;
pub mod packages;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use std::collections::BTreeMap;

use fenix_extension_api::serde_json::Value;
use fenix_extension_api::{self as api, export, Check, Command, Event, Extension, Key, LaunchConfig, MouseEvent, Page};
use fenix_lang_kit::{Fenix, Host, Kit, PackagesPage};

use project::{Manager, Project};
use tools::Tools;

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's names for what typescript-language-server serves.
pub const SCRIPTS: &[&str] = &["javascript", "typescript", "tsx"];

/// Files whose saving changes what the project is.
const PROJECT_FILES: &[&str] = &[
    "package.json", "package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lock", ".nvmrc", ".node-version", ".yarnrc.yml",
    "biome.json", "biome.jsonc", ".prettierrc", ".prettierrc.json", "eslint.config.js", "eslint.config.mjs", "eslint.config.ts", "tsconfig.json",
    "tailwind.config.js", "tailwind.config.ts",
];

/// The pack's own commands.
pub const DOCTOR: &str = "web.doctor";
pub const DEV: &str = "web.dev";
pub const DEV_STOP: &str = "web.dev-stop";

/// How ESLint runs (`web.eslint`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Eslint {
    /// vscode-eslint-language-server beside the TypeScript server; on save
    /// while it isn't installed.
    #[default]
    Server,
    /// On save, `--format json`.
    Save,
    Off,
}

impl Eslint {
    pub fn parse(text: &str) -> Eslint {
        match text.trim() {
            "save" => Eslint::Save,
            "off" => Eslint::Off,
            _ => Eslint::Server,
        }
    }

    /// Whether its server runs, with what's installed.
    pub fn serves(self, project: &Project, tools: &Tools) -> bool {
        self == Eslint::Server && project.eslint && tools.eslint_server.is_some()
    }
}

/// The servers set beside the others, by name: their languages, and
/// what runs them, for the project and tools as they are.
pub fn added_servers(project: &Project, tools: &Tools, eslint: Eslint, root: &str) -> Vec<(&'static str, &'static [&'static str], Option<Command>)> {
    let stdio = |path: &Option<String>| path.as_deref().map(|p| Command::new(p).arg("--stdio"));
    let when = |yes: bool, path: &Option<String>| if yes { stdio(path) } else { None };
    vec![
        ("css", &["css"], stdio(&tools.css_server)),
        ("html", &["html"], stdio(&tools.html_server)),
        ("eslint", SCRIPTS, when(eslint.serves(project, tools), &tools.eslint_server)),
        ("tailwindcss", &["html", "css", "javascript", "typescript", "tsx"], when(project.tailwind, &tools.tailwind_server)),
        ("biome", lint::BIOME_LANGUAGES, lint::biome_server(project, tools, root)),
    ]
}

pub struct Web {
    pub kit: Kit,
    root: String,
    eslint: Eslint,
    project: Project,
    tools: Tools,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    added: BTreeMap<&'static str, Option<Command>>,
    set_adapter: Option<Command>,
    set_formatter: Option<(&'static [&'static str], Command)>,
    set_eslint: Option<Value>,
    set_launches: Option<Vec<LaunchConfig>>,
    js_debug: bool,
    /// `web.install`'s task, and the dev server's.
    installing: Option<u64>,
    dev: Option<u64>,
}

impl Default for Web {
    fn default() -> Web {
        let eslint = Eslint::parse(&api::setting("web.eslint").unwrap_or_default());
        Web::new(Box::new(Fenix), PROJECT, eslint)
    }
}

impl Web {
    /// The pack for the project in `root`, talking to `host`.
    pub fn new(mut host: Box<dyn Host>, root: &str, eslint: Eslint) -> Web {
        let tools = Tools::find(host.as_mut());
        let kit = Kit::with_host("web", host).install(debug::install());
        let mut w = Web {
            kit,
            root: root.into(),
            eslint,
            project: Project::read(root),
            tools,
            set_server: None,
            added: BTreeMap::new(),
            set_adapter: None,
            set_formatter: None,
            set_eslint: None,
            set_launches: None,
            js_debug: false,
            installing: None,
            dev: None,
        };
        w.configure();
        w
    }

    /// The package manager its page uses.
    fn manager(&self) -> packages::Js {
        let m = self.project.manager;
        packages::Js { manager: m, program: self.tools.manager(m), npm: self.tools.npm.as_ref().map(|_| self.tools.npm()), project: self.project.clone(), root: self.root.clone() }
    }

    /// The pack's own checks.
    fn checks(&self) -> Vec<Check> {
        let (p, root) = (&self.project, self.root.as_str());
        let mut out: Vec<Check> = doctor::node_modules(p, root).into_iter().collect();
        if p.eslint && self.eslint != Eslint::Off {
            out.push(doctor::local(p, root, "ESLint", "eslint"));
        }
        if p.biome {
            out.push(doctor::local(p, root, "Biome", "@biomejs/biome"));
        } else if p.has("prettier") {
            out.push(doctor::local(p, root, "Prettier", "prettier"));
        }
        out.push(doctor::js_debug(self.js_debug.then_some(debug::VERSION)));
        out
    }

    /// The kit's tasks, checks, linter and package manager, for the
    /// project and tools as they are.
    fn configure(&mut self) {
        self.js_debug = self.kit.installer.installed(debug::NAME);
        self.kit.tasks = tasks::tasks(&self.project, &self.tools);
        // Checked in a project of its own only: Node isn't another's.
        self.kit.doctor.programs.clear();
        self.kit.doctor.extra.clear();
        if self.project.package_json {
            for program in doctor::programs(&self.project, &self.tools, self.eslint == Eslint::Server) {
                self.kit.doctor.program(program);
            }
            self.kit.doctor.extra = self.checks();
        }
        // On save, or while its server isn't there.
        let on_save = self.eslint == Eslint::Save || (self.eslint == Eslint::Server && !self.eslint.serves(&self.project, &self.tools));
        match lint::eslint(&self.project, &self.tools, &self.root).filter(|_| on_save) {
            Some(e) => self.kit.lint.add(lint::linter(&e)),
            None => self.kit.lint.linters.retain(|l| l.name != "eslint"),
        }
        if self.project.package_json {
            let m = Box::new(self.manager());
            match self.kit.packages.as_mut() {
                Some(page) => page.manager = m,
                None => self.kit.packages = Some(PackagesPage::new(m)),
            }
        }
    }

    /// The servers, the adapter and the formatter set in Fenix, where they
    /// changed; nothing found leaves Fenix's own.
    fn apply(&mut self) {
        let host = self.kit.host.as_mut();
        let languages = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<String>>();

        // Fenix's typescript-language-server, by the path it was found at
        // (npm's prefix, Volta's), which Fenix's PATH may not have.
        let server = self.tools.tsserver.as_deref().map(|p| Command::new(p).arg("--stdio"));
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages(SCRIPTS), server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
        }
        for (name, langs, cmd) in added_servers(&self.project, &self.tools, self.eslint, &self.root) {
            if self.added.get(name).cloned().flatten() == cmd {
                continue;
            }
            if let Err(why) = host.add_language_server(name, &languages(langs), cmd.as_ref()) {
                host.log(&format!("{name} server: {why}"));
            }
            self.added.insert(name, cmd);
        }
        // What ESLint's server asks for; it fails on nothing.
        let settings = self.eslint.serves(&self.project, &self.tools).then(|| lint::eslint_settings(&self.project, &self.root));
        if settings != self.set_eslint {
            if let Err(why) = host.language_server_settings("eslint", settings.as_ref()) {
                host.log(&format!("eslint settings: {why}"));
            }
            self.set_eslint = settings;
        }
        let launches = debug::launches(&self.project);
        if self.set_launches.as_ref() != Some(&launches) {
            if let Err(why) = host.launches(&launches) {
                host.log(&format!("launches: {why}"));
            }
            self.set_launches = Some(launches);
        }

        let adapter = self.js_debug.then(|| debug::adapter(self.tools.node()));
        if adapter != self.set_adapter {
            let done = match &adapter {
                Some(cmd) => host.debug_adapter_connect(&languages(debug::LANGUAGES), Some(cmd), 0),
                None => host.debug_adapter(&languages(debug::LANGUAGES), None),
            };
            if let Err(why) = done {
                host.log(&format!("debug adapter: {why}"));
            }
            self.set_adapter = adapter;
        }

        let formatter = lint::formatter(&self.project, &self.tools, &self.root);
        if formatter != self.set_formatter {
            // Biome's languages aren't Prettier's: those it had go back.
            if let Some((old, _)) = &self.set_formatter {
                let gone: Vec<&str> = old.iter().copied().filter(|l| formatter.as_ref().is_none_or(|(new, _)| !new.contains(l))).collect();
                if !gone.is_empty() {
                    if let Err(why) = host.formatter(&languages(&gone), None) {
                        host.log(&format!("formatter: {why}"));
                    }
                }
            }
            if let Some((langs, cmd)) = &formatter {
                if let Err(why) = host.formatter(&languages(langs), Some(cmd)) {
                    host.log(&format!("formatter: {why}"));
                }
            }
            self.set_formatter = formatter;
        }
    }

    /// The tasks and the rest set again.
    fn refresh(&mut self) {
        self.configure();
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
        }
        self.apply();
    }

    /// What changed since: a program a fix installed (the doctor found
    /// it), or js-debug installed. The programs are looked for again.
    fn follow(&mut self) {
        let checked: Vec<(String, Option<String>)> = self.kit.doctor.programs.iter().map(|p| (p.name.clone(), self.kit.doctor.found.get(&p.name).cloned())).collect();
        let t = &self.tools;
        let had = |name: &str| -> Option<&Option<String>> {
            Some(match name {
                "node" => &t.node,
                tools::TSSERVER => &t.tsserver,
                tools::CSS_SERVER => &t.css_server,
                tools::ESLINT_SERVER => &t.eslint_server,
                tools::TAILWIND_SERVER => &t.tailwind_server,
                n if n == self.project.manager.program() => match self.project.manager {
                    Manager::Npm => &t.npm,
                    Manager::Pnpm => &t.pnpm,
                    Manager::Yarn | Manager::Berry => &t.yarn,
                    Manager::Bun => &t.bun,
                },
                _ => return None,
            })
        };
        let changed = checked.iter().any(|(name, found)| had(name).is_some_and(|h| h != found));
        let installed = self.kit.installer.installed(debug::NAME) != self.js_debug;
        if changed {
            self.tools = Tools::find(self.kit.host.as_mut());
        }
        if changed || installed {
            self.refresh();
            if installed {
                self.kit.doctor.run(self.kit.host.as_mut());
            }
        }
    }

    /// The project's files changed: read again, and checked again.
    fn reload(&mut self) {
        self.project = Project::read(&self.root);
        self.refresh();
        self.kit.doctor.run(self.kit.host.as_mut());
    }

    /// `web.install`: the manager's install, as a task; the project read
    /// again when it ends.
    fn install(&mut self) -> Result<(), String> {
        if !self.project.package_json {
            return Err("there's no package.json here".into());
        }
        let cmd = self.tools.manager(self.project.manager).arg("install");
        self.installing = Some(self.kit.host.run_task("install", &cmd)?);
        Ok(())
    }

    /// `web.dev`: the dev script (or `start`) in the background, so the
    /// browser launch has a server and the task pane stays free; when it's
    /// running, its output.
    fn dev(&mut self) -> Result<(), String> {
        if let Some(task) = self.dev {
            return self.kit.host.task_show(task);
        }
        let script = ["dev", "start", "serve"].into_iter().find(|s| self.project.scripts.iter().any(|(k, _)| k == s)).ok_or("package.json has no dev or start script")?;
        let cmd = self.tools.manager(self.project.manager).args(["run", script]);
        self.dev = Some(self.kit.host.run_task_background(script, &cmd)?);
        Ok(())
    }

    fn dev_stop(&mut self) -> Result<(), String> {
        let task = self.dev.take().ok_or("the dev server isn't running")?;
        self.kit.host.task_cancel(task);
        Ok(())
    }

    /// An event that's the pack's own.
    fn own(&mut self, event: &Event) -> bool {
        match event {
            Event::TaskFinished((id, _)) if self.installing == Some(*id) => {
                self.installing = None;
                self.reload();
                true
            }
            Event::TaskFinished((id, _)) if self.dev == Some(*id) => {
                self.dev = None;
                true
            }
            _ => false,
        }
    }
}

impl Extension for Web {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match id {
            doctor::INSTALL => self.install(),
            // node_modules may have changed since: read again too.
            DOCTOR => {
                self.reload();
                Ok(())
            }
            DEV => self.dev(),
            DEV_STOP => self.dev_stop(),
            _ => self.kit.run_command(id).unwrap_or_else(|| Err(format!("no command {id} here"))),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        if self.own(&event) {
            return;
        }
        // A fix's task ended: the doctor checks again, node_modules too.
        if matches!(event, Event::TaskFinished(_)) && self.project.package_json {
            self.kit.doctor.extra = self.checks();
        }
        let saved = matches!(&event, Event::Saved(name) if PROJECT_FILES.iter().any(|f| name.replace('\\', "/").rsplit('/').next() == Some(f)));
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

export!(Web);

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

    fn vite_project(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::write(dir.join("package.json"), crate::project::tests::PACKAGE).unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
        std::fs::write(dir.join(".prettierrc"), "{}\n").unwrap();
        dir
    }

    fn pack(dir: &std::path::Path, host: &Shared, eslint: Eslint) -> Web {
        let mut w = Web::new(Box::new(host.clone()), &dir.to_string_lossy(), eslint);
        w.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
        if let Some(page) = w.kit.packages.as_mut() {
            page.cache_dir = None;
        }
        w.configure();
        w
    }

    fn said(w: &mut Web, process: u64, text: &str) {
        w.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: text.into() }));
        w.handle(Event::ProcessExit((process, Some(0))));
    }

    #[test]
    fn a_vite_project_gets_its_servers_tasks_and_checks() {
        let dir = vite_project("activate");
        let host = Shared::default();
        host.0.borrow_mut().programs.extend(
            [("node", "/n/node"), ("pnpm", "/n/pnpm"), ("npm", "/n/npm"), ("prettier", "/g/prettier"), (tools::TSSERVER, "/g/tsls"), (tools::CSS_SERVER, "/g/css"), (tools::HTML_SERVER, "/g/html"), (tools::TAILWIND_SERVER, "/g/tw")]
                .map(|(k, v)| (k.to_string(), v.to_string())),
        );
        let mut w = pack(&dir, &host, Eslint::Save);
        w.activate().unwrap();
        let calls = host.take();
        assert_eq!(calls[0], "tasks dev, build, test, lint, install");
        assert!(calls.contains(&"server javascript,typescript,tsx: /g/tsls --stdio".to_string()), "{calls:?}");
        for c in ["add css css: /g/css --stdio", "add html html: /g/html --stdio", "add tailwindcss html,css,javascript,typescript,tsx: /g/tw --stdio"] {
            assert!(calls.contains(&c.to_string()), "{c}: {calls:?}");
        }
        assert!(!calls.iter().any(|c| c.starts_with("add eslint")), "ESLint lints on save");
        assert!(!calls.iter().any(|c| c.starts_with("add json")), "JSON is fenix.config's");
        assert!(calls.contains(&"formatter javascript,typescript,tsx,css,html,json: /g/prettier --stdin-filepath ${file}".to_string()), "{calls:?}");
        assert!(!calls.iter().any(|c| c.starts_with("connect")), "js-debug isn't installed yet");
        assert!(calls.contains(&"spawn 1 /n/node --version".to_string()), "{calls:?}");
        assert_eq!(line(&host.0.borrow().tasks[0].cmd), "/n/pnpm run dev");

        said(&mut w, 1, "v22.22.0");
        said(&mut w, 2, "9.15.1");
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert_eq!(
            checks,
            [
                ("Node".into(), Health::Ok),
                ("Package manager".into(), Health::Ok),
                ("TypeScript server".into(), Health::Ok),
                ("CSS and HTML servers".into(), Health::Ok),
                ("Tailwind CSS server".into(), Health::Ok),
                ("node_modules".into(), Health::Warn),
                ("ESLint".into(), Health::Warn),
                ("Debugger".into(), Health::Warn),
            ]
        );
        assert_eq!(host.0.borrow().checks[7].fix, Some(("Install js-debug".into(), doctor::INSTALL_JS_DEBUG.into())));

        // Installed outside Fenix: web.doctor sees it.
        std::fs::create_dir_all(dir.join("node_modules/.pnpm")).unwrap();
        std::fs::write(dir.join("node_modules/.pnpm/lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
        w.run_command(DOCTOR).unwrap();
        let spawned: Vec<u64> = host.take().iter().filter_map(|c| c.strip_prefix("spawn ")?.split(' ').next()?.parse().ok()).collect();
        for id in spawned {
            said(&mut w, id, "9.15.1");
        }
        assert_eq!(host.0.borrow().checks.iter().find(|c| c.label == "node_modules").map(|c| c.health), Some(Health::Ok));

        // Nothing changed: nothing set again.
        w.handle(Event::Saved("src/App.tsx".into()));
        assert!(!host.take().iter().any(|c| c.starts_with("server") || c.starts_with("add") || c.starts_with("formatter")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn web_install_runs_the_managers_install_and_reads_the_project_again() {
        let dir = vite_project("install");
        let host = Shared::default();
        host.0.borrow_mut().programs.extend([("node".to_string(), "/n/node".to_string()), ("pnpm".into(), "/n/pnpm".into())]);
        let mut w = pack(&dir, &host, Eslint::Save);
        w.activate().unwrap();
        host.take();
        w.run_command(doctor::INSTALL).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").and_then(|c| c.split_once(" install: /n/pnpm install")).map(|(id, _)| id.parse::<u64>().unwrap())).expect("a task");
        // What it installed: ESLint, linting on save now.
        std::fs::create_dir_all(dir.join("node_modules/eslint/bin")).unwrap();
        std::fs::write(dir.join("node_modules/eslint/package.json"), r#"{"version": "9.39.1", "bin": {"eslint": "./bin/eslint.js"}}"#).unwrap();
        std::fs::write(dir.join("node_modules/eslint/bin/eslint.js"), "").unwrap();
        w.handle(Event::TaskFinished((task, true)));
        assert!(host.take().iter().any(|c| c.starts_with("spawn")), "checked again");
        assert_eq!(w.kit.lint.linters.len(), 1);
        host.0.borrow_mut().here = Some((format!("{}/src/App.tsx", dir.display()), Some(dir.to_string_lossy().into_owned())));
        w.handle(Event::Saved("App.tsx".into()));
        assert!(host.take().iter().any(|c| c.ends_with("/n/node node_modules/eslint/bin/eslint.js --format json src/App.tsx")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn js_debug_installed_by_its_fix_row_is_connected_to() {
        let dir = vite_project("debug");
        let host = Shared::default();
        host.0.borrow_mut().programs.insert("node".into(), "/n/node".into());
        let mut w = pack(&dir, &host, Eslint::Save);
        w.activate().unwrap();
        host.take();
        w.run_command(doctor::INSTALL_JS_DEBUG).unwrap();
        let calls = host.take();
        assert!(calls.iter().any(|c| c.starts_with("download ") && c.contains(&format!(" {} -> ", debug::url())) && c.ends_with(debug::SHA256)), "{calls:?}");
        // As the kit leaves it once it's unpacked.
        let tools = dir.join(".tools/js-debug");
        std::fs::create_dir_all(tools.join("src")).unwrap();
        std::fs::write(tools.join("src/dapDebugServer.js"), "").unwrap();
        std::fs::write(tools.join(fenix_lang_kit::install::MARKER), debug::url()).unwrap();
        w.handle(Event::Timer(999));
        let calls = host.take();
        let node: u64 = calls.iter().rev().find_map(|c| c.strip_prefix("spawn ")?.strip_suffix(" /n/node --version")?.parse().ok()).expect("checked again");
        said(&mut w, node, "v22.22.0");
        assert!(calls.contains(&"connect javascript,typescript,tsx: /n/node ${tools}/js-debug/src/dapDebugServer.js ${port} 127.0.0.1 :0".to_string()), "{calls:?}");
        assert_eq!(host.0.borrow().checks.iter().find(|c| c.label == "Debugger").map(|c| c.health), Some(Health::Ok));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn eslints_server_is_added_when_asked_and_biome_takes_the_formatter() {
        let dir = vite_project("eslint");
        std::fs::write(dir.join("biome.json"), "{}\n").unwrap();
        let host = Shared::default();
        host.0.borrow_mut().programs.extend([("biome".to_string(), "/g/biome".to_string()), (tools::ESLINT_SERVER.into(), "/g/eslint-ls".into()), ("prettier".into(), "/g/prettier".into())]);
        let mut w = pack(&dir, &host, Eslint::Server);
        w.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"add eslint javascript,typescript,tsx: /g/eslint-ls --stdio".to_string()), "{calls:?}");
        assert!(calls.contains(&"formatter javascript,typescript,tsx,css,json: /g/biome format --stdin-file-path=${file}".to_string()), "{calls:?}");
        assert!(calls.contains(&"add biome javascript,typescript,tsx,css,json: /g/biome lsp-proxy".to_string()), "Biome's server lints too: {calls:?}");
        assert!(w.kit.lint.linters.is_empty(), "the server lints");
        let settings = host.0.borrow().settings["eslint"].clone();
        assert_eq!((settings["validate"].as_str(), settings["packageManager"].as_str(), settings["experimental"]["useFlatConfig"].as_bool()), (Some("on"), Some("pnpm"), Some(false)), "what it reads without a null");
        // biome.json gone: Prettier's, and html back to it.
        std::fs::remove_file(dir.join("biome.json")).unwrap();
        w.handle(Event::Saved("biome.json".into()));
        let calls = host.take();
        assert!(calls.contains(&"formatter javascript,typescript,tsx,css,html,json: /g/prettier --stdin-filepath ${file}".to_string()), "{calls:?}");
        // No ESLint or Prettier in the project now: its server and the
        // formatter taken away.
        std::fs::remove_file(dir.join(".prettierrc")).unwrap();
        std::fs::write(dir.join("package.json"), r#"{"name": "plain", "scripts": {"start": "node server.js"}}"#).unwrap();
        w.handle(Event::Saved("package.json".into()));
        let calls = host.take();
        assert!(calls.contains(&"add eslint javascript,typescript,tsx: ".to_string()), "{calls:?}");
        assert!(calls.contains(&"settings eslint: ".to_string()), "its settings taken away: {calls:?}");
        assert!(calls.contains(&"formatter javascript,typescript,tsx,css,html,json: ".to_string()), "no Prettier now: {calls:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_eslints_server_eslint_runs_on_save_and_each_script_is_a_launch() {
        let dir = vite_project("eslint-save");
        std::fs::create_dir_all(dir.join("node_modules/eslint/bin")).unwrap();
        std::fs::write(dir.join("node_modules/eslint/package.json"), r#"{"version": "9.39.1", "bin": {"eslint": "./bin/eslint.js"}}"#).unwrap();
        std::fs::write(dir.join("node_modules/eslint/bin/eslint.js"), "").unwrap();
        let host = Shared::default();
        host.0.borrow_mut().programs.insert("node".into(), "/n/node".into());
        let mut w = pack(&dir, &host, Eslint::Server);
        w.activate().unwrap();
        let calls = host.take();
        assert_eq!(w.kit.lint.linters.len(), 1, "no server installed: on save");
        assert!(!calls.iter().any(|c| c.starts_with("settings eslint")), "{calls:?}");
        assert!(calls.contains(&"launches Node: pnpm run dev, Node: pnpm run build, Node: pnpm run test, Node: pnpm run lint".to_string()), "{calls:?}");
        w.file_saved("/project/src/App.tsx");
        assert!(host.take().iter().any(|c| c.ends_with("/n/node node_modules/eslint/bin/eslint.js --format json src/App.tsx")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_dev_server_runs_in_the_background() {
        let dir = vite_project("dev");
        let host = Shared::default();
        host.0.borrow_mut().programs.insert("pnpm".into(), "/n/pnpm".into());
        let mut w = pack(&dir, &host, Eslint::Off);
        w.activate().unwrap();
        host.take();
        w.run_command(DEV).unwrap();
        let calls = host.take();
        let id: u64 = calls.iter().find_map(|c| c.strip_prefix("background ")?.strip_suffix(" dev: /n/pnpm run dev")?.parse().ok()).expect("started");
        w.run_command(DEV).unwrap();
        assert_eq!(host.take(), [format!("show {id}")], "running: its output");
        w.run_command(DEV_STOP).unwrap();
        assert_eq!(host.take(), [format!("cancel {id}")]);
        assert!(w.run_command(DEV_STOP).is_err());
        w.run_command(DEV).unwrap();
        let again: u64 = host.take().iter().find_map(|c| c.strip_prefix("background ")?.split(' ').next()?.parse().ok()).unwrap();
        w.handle(Event::TaskFinished((again, false)));
        assert!(w.dev.is_none(), "it ended on its own");
        assert!(w.run_command("web.nothing").is_err());
        assert_eq!(Eslint::parse("save"), Eslint::Save);
        assert_eq!(Eslint::parse(""), Eslint::Server);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn outside_a_js_project_only_the_servers_for_its_files_are_set() {
        let dir = temp("plain");
        let host = Shared::default();
        host.0.borrow_mut().programs.insert(tools::CSS_SERVER.into(), "/g/css".into());
        let mut w = pack(&dir, &host, Eslint::Save);
        assert!(w.kit.packages.is_none(), "no package.json, no page");
        w.activate().unwrap();
        let calls = host.take();
        assert!(calls.contains(&"add css css: /g/css --stdio".to_string()));
        assert!(!calls.iter().any(|c| c.starts_with("tasks") || c.starts_with("spawn") || c.starts_with("add eslint") || c.starts_with("add tailwindcss") || c.starts_with("formatter")), "{calls:?}");
        assert!(w.run_command(doctor::INSTALL).is_err());
        assert!(w.run_command(DEV).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
