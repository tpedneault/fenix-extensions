//! Config and shell files in Fenix, on lang-kit: a language server for
//! each of YAML, JSON, TOML, Dockerfiles, shell scripts and Markdown,
//! given its settings (SchemaStore's schemas for YAML and JSON; ShellCheck
//! and shfmt for the Bash server), Prettier as the formatter for JSON,
//! hadolint on a saved Dockerfile, tasks that lint every script and
//! Dockerfile, and the project doctor's checks with their fixes.
//!
//! Fenix keeps one server of an extension's as a language's own, and the
//! rest beside the others: the Bash server is the language's own (in
//! place of Fenix's), the rest are added by name. A server is set for its
//! language whenever it's found and its setting is on; the doctor checks
//! only the kinds of file the project has ([`project`]).

pub mod doctor;
pub mod lint;
pub mod project;
pub mod schemas;
pub mod tools;

#[cfg(test)]
mod data;

use std::collections::HashMap;

use fenix_extension_api::{self as api, export, Command, Event, Extension, HttpRequest, Key, MouseEvent, Page};
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{Fenix, Host, Kit, Tasks};

use project::{Kind, Project};
use schemas::Schema;
use tools::{Platform, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Files whose saving changes what the project is: its Prettier config.
const PROJECT_FILES: &[&str] = &["package.json", ".prettierrc", ".prettierrc.json", ".prettierrc.yaml", ".prettierrc.yml", ".prettierrc.toml", "prettier.config.js", "prettier.config.mjs"];

/// How many files a task lints at once.
pub const TASK_FILES: usize = 200;

/// What the pack's settings say: each kind on or off, and the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub yaml: bool,
    pub json: bool,
    pub toml: bool,
    pub dockerfile: bool,
    pub shell: bool,
    pub markdown: bool,
    pub schemastore: bool,
    pub prettier: bool,
    pub hadolint: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { yaml: true, json: true, toml: true, dockerfile: true, shell: true, markdown: true, schemastore: true, prettier: true, hadolint: true }
    }
}

impl Settings {
    /// From Fenix: a setting that isn't `false` is on.
    pub fn read() -> Settings {
        let on = |key: &str| !api::setting(key).is_some_and(|v| v.trim().eq_ignore_ascii_case("false"));
        Settings {
            yaml: on("config.yaml"),
            json: on("config.json"),
            toml: on("config.toml"),
            dockerfile: on("config.dockerfile"),
            shell: on("config.shell"),
            markdown: on("config.markdown"),
            schemastore: on("config.schemastore"),
            prettier: on("config.prettier"),
            hadolint: on("config.hadolint"),
        }
    }

    pub fn on(&self, kind: Kind) -> bool {
        match kind {
            Kind::Yaml => self.yaml,
            Kind::Json => self.json,
            Kind::Toml => self.toml,
            Kind::Dockerfile => self.dockerfile,
            Kind::Shell => self.shell,
            Kind::Markdown => self.markdown,
        }
    }
}

/// The servers added beside the others, by name: their kind, and their
/// command for the program found.
pub fn added_servers(tools: &Tools, settings: &Settings) -> Vec<(&'static str, Kind, Option<Command>)> {
    let with = |found: &Option<String>, args: &[&str]| found.as_deref().map(|p| Command::new(p).args(args.iter().copied()));
    let rows = [
        ("yaml", Kind::Yaml, with(&tools.yaml, &["--stdio"])),
        ("json", Kind::Json, with(&tools.json, &["--stdio"])),
        ("toml", Kind::Toml, with(&tools.taplo, &["lsp", "stdio"])),
        ("dockerfile", Kind::Dockerfile, with(&tools.docker, &["--stdio"])),
        ("markdown", Kind::Markdown, with(&tools.marksman, &["server"])),
    ];
    rows.into_iter().map(|(name, kind, cmd)| (name, kind, cmd.filter(|_| settings.on(kind)))).collect()
}

/// Prettier, for JSON; and YAML and Markdown in a project with a Prettier
/// config of its own. None in a Biome project, whose Biome formats JSON
/// (the Web pack's).
pub fn formatter(tools: &Tools, settings: &Settings, project: &Project) -> Option<(Vec<String>, Command)> {
    let prettier = tools.prettier.as_deref().filter(|_| settings.prettier && !project.biome)?;
    let mut languages = Vec::new();
    if settings.json {
        languages.push("json".to_string());
    }
    if project.prettier {
        languages.extend([(settings.yaml, "yaml"), (settings.markdown, "markdown")].into_iter().filter(|(on, _)| *on).map(|(_, l)| l.to_string()));
    }
    (!languages.is_empty()).then(|| (languages, Command::new(prettier).args(["--stdin-filepath", "${file}"])))
}

/// The tasks: ShellCheck on every script, hadolint on every Dockerfile.
pub fn tasks(project: &Project, tools: &Tools, settings: &Settings) -> Tasks {
    let mut t = Tasks::default();
    if let (true, Some(shellcheck)) = (settings.shell && !project.scripts.is_empty(), &tools.shellcheck) {
        t.add("shellcheck: every script", Command::new(shellcheck).args(project.scripts.iter().take(TASK_FILES).cloned()));
    }
    if let (true, Some(hadolint)) = (settings.dockerfile && !project.dockerfiles.is_empty(), &tools.hadolint) {
        t.add("hadolint: every Dockerfile", Command::new(hadolint).args(project.dockerfiles.iter().take(TASK_FILES).cloned()));
    }
    t
}

pub struct Config {
    pub kit: Kit,
    root: String,
    project: Project,
    tools: Tools,
    platform: Platform,
    settings: Settings,
    read_settings: fn() -> Settings,
    /// SchemaStore's catalog: where it's kept, what it gave, and the
    /// request fetching it.
    pub cache: Option<String>,
    schemas: Vec<Schema>,
    fetching: Option<u64>,
    /// What's set in Fenix, so it's set again only when it changes.
    set_added: HashMap<&'static str, Command>,
    set_bash: Option<Command>,
    set_formatter: Option<(Vec<String>, Command)>,
    set_settings: HashMap<&'static str, Value>,
}

impl Default for Config {
    fn default() -> Config {
        let platform = api::platform();
        Config::new(Box::new(Fenix), PROJECT, Settings::read, Platform::new(&platform.os, &platform.arch))
    }
}

impl Config {
    /// The pack for the project in `root`, talking to `host`, on
    /// `platform`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings, platform: Platform) -> Config {
        let tools = Tools::find(host.as_mut());
        let mut kit = Kit::with_host("config", host);
        for install in platform.installs() {
            kit.installer.add(install);
        }
        let mut c = Config {
            kit,
            root: root.into(),
            project: Project::read(root),
            tools,
            platform,
            settings: read_settings(),
            read_settings,
            cache: Some(schemas::CACHE.into()),
            schemas: Vec::new(),
            fetching: None,
            set_added: HashMap::new(),
            set_bash: None,
            set_formatter: None,
            set_settings: HashMap::new(),
        };
        c.configure();
        c
    }

    /// The kit's tasks, checks and linter, for the project, tools and
    /// settings as they are.
    fn configure(&mut self) {
        self.kit.tasks = tasks(&self.project, &self.tools, &self.settings);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(&self.project, &self.tools, &self.settings, &self.platform) {
            self.kit.doctor.program(program);
        }
        self.kit.lint.linters.clear();
        if let (true, Some(hadolint)) = (self.settings.dockerfile && self.settings.hadolint, &self.tools.hadolint) {
            self.kit.lint.add(lint::hadolint(&Command::new(hadolint)));
        }
    }

    /// The servers, their settings and the formatter set in Fenix, where
    /// they changed.
    fn apply(&mut self) {
        let host = self.kit.host.as_mut();
        for (name, kind, cmd) in added_servers(&self.tools, &self.settings) {
            if self.set_added.get(name) == cmd.as_ref() {
                continue;
            }
            let languages = [kind.language().to_string()];
            if let Err(why) = host.add_language_server(name, &languages, cmd.as_ref()) {
                host.log(&format!("language server {name}: {why}"));
            }
            match cmd {
                Some(cmd) => self.set_added.insert(name, cmd),
                None => self.set_added.remove(name),
            };
            // Started again: told its settings again.
            self.set_settings.remove(name);
        }

        // The Bash server, in place of Fenix's own.
        let bash = self.tools.bash.as_deref().filter(|_| self.settings.shell).map(|p| Command::new(p).arg("start"));
        if bash != self.set_bash {
            if let Err(why) = host.language_server(&["bash".to_string()], bash.as_ref()) {
                host.log(&format!("language server bash: {why}"));
            }
            self.set_bash = bash;
            self.set_settings.remove("bash");
        }

        let schemas: &[Schema] = if self.settings.schemastore { &self.schemas } else { &[] };
        let wanted: [(&'static str, bool, Value); 3] = [
            ("yaml", self.set_added.contains_key("yaml"), schemas::yaml_settings(self.settings.schemastore)),
            ("json", self.set_added.contains_key("json"), schemas::json_settings(schemas)),
            ("bash", self.set_bash.is_some(), schemas::bash_settings(self.tools.shellcheck.as_deref(), self.tools.shfmt.as_deref())),
        ];
        for (name, running, settings) in wanted {
            if !running || self.set_settings.get(name) == Some(&settings) {
                continue;
            }
            if let Err(why) = host.language_server_settings(name, Some(&settings)) {
                host.log(&format!("settings for {name}: {why}"));
            }
            self.set_settings.insert(name, settings);
        }

        let formatter = formatter(&self.tools, &self.settings, &self.project);
        if formatter != self.set_formatter {
            let done = match &formatter {
                Some((languages, cmd)) => host.formatter(languages, Some(cmd)),
                None => self.set_formatter.as_ref().map_or(Ok(()), |(languages, _)| host.formatter(languages, None)),
            };
            if let Err(why) = done {
                host.log(&format!("formatter: {why}"));
            }
            self.set_formatter = formatter;
        }
    }

    /// The tools as they're found now; set again where that changed.
    fn find_again(&mut self) {
        let tools = Tools::find(self.kit.host.as_mut());
        self.take(tools);
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

    /// SchemaStore's catalog: what was kept, and fetched again when it's
    /// a week old or there's none (or `again`).
    fn schemas(&mut self, again: bool) {
        if !(self.settings.json && self.settings.schemastore) || self.fetching.is_some() {
            return;
        }
        let kept = self.cache.as_deref().and_then(|c| std::fs::read_to_string(c).ok()).and_then(|t| schemas::read_cache(&t));
        let now = self.kit.host.now();
        let fresh = kept.as_ref().is_some_and(|(at, _)| !schemas::stale(*at, now));
        if let Some((_, list)) = kept {
            self.schemas = list;
        }
        if fresh && !again {
            return;
        }
        match self.kit.host.http(&HttpRequest::get(schemas::CATALOG)) {
            Ok(id) => self.fetching = Some(id),
            Err(why) => self.kit.host.log(&format!("SchemaStore's catalog: {why}")),
        }
    }

    /// The catalog's answer: kept, and given to the JSON server.
    fn fetched(&mut self, answer: &Result<api::HttpResponse, String>) {
        self.fetching = None;
        let parsed = match answer {
            Ok(r) if (200..300).contains(&r.status) => schemas::parse_catalog(&String::from_utf8_lossy(&r.body)),
            Ok(r) => Err(format!("SchemaStore said {}", r.status)),
            Err(why) => Err(why.clone()),
        };
        match parsed {
            Ok(list) => {
                let now = self.kit.host.now();
                if let Some(cache) = &self.cache {
                    if let Some(dir) = std::path::Path::new(cache).parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    if let Err(why) = std::fs::write(cache, schemas::cache_text(now, &list)) {
                        self.kit.host.log(&format!("{cache}: {why}"));
                    }
                }
                self.schemas = list;
                self.apply();
            }
            Err(why) => self.kit.host.log(&format!("SchemaStore's catalog: {why}")),
        }
    }
}

impl Extension for Config {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.schemas(false);
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            None if id == "config.schemas" => {
                if !(self.settings.json && self.settings.schemastore) {
                    return Err("JSON or SchemaStore is turned off (config.json, config.schemastore)".into());
                }
                self.schemas(true);
                Ok(())
            }
            None => Err(format!("no command {id} here")),
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        if let Event::HttpResponse((id, answer)) = &event {
            if self.fetching == Some(*id) {
                return self.fetched(answer);
            }
        }
        let name = match &event {
            Event::Saved(name) => Some(name.replace('\\', "/").rsplit('/').next().unwrap_or_default().to_string()),
            _ => None,
        };
        // A config file saved, or the first of a kind.
        let saved = name.as_deref().is_some_and(|n| PROJECT_FILES.contains(&n) || Kind::of(n).is_some_and(|k| !self.project.has(k)));
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("config."));
        let installing = self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        if saved {
            self.reload();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.configure();
            self.kit.doctor.run(self.kit.host.as_mut());
            self.schemas(false);
            self.apply();
        }
        let installed = installing && !self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        if installed || task_ended {
            // What a fix, or one of its install commands, put here.
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

export!(Config);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, write};
    use fenix_extension_api::{Health, HttpResponse};
    use fenix_lang_kit::host::mock::Shared;

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    fn found(host: &Shared, programs: &[(&str, &str)]) {
        host.0.borrow_mut().programs.extend(programs.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    }

    fn pack(dir: &std::path::Path, host: &Shared, settings: fn() -> Settings) -> Config {
        let mut c = Config::new(Box::new(host.clone()), &dir.to_string_lossy(), settings, Platform::new("linux", "x86_64"));
        c.cache = Some(dir.join(".cache/schemastore.json").to_string_lossy().into_owned());
        c.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
        c
    }

    fn said(c: &mut Config, process: u64, text: &str) {
        c.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: text.into() }));
        c.handle(Event::ProcessExit((process, Some(0))));
    }

    const EVERYTHING: &[(&str, &str)] = &[
        ("npm", "/n/npm"),
        (tools::YAML_SERVER, "/n/yaml-language-server"),
        (tools::JSON_SERVER, "/n/vscode-json-language-server"),
        (tools::DOCKER_SERVER, "/n/docker-langserver"),
        (tools::BASH_SERVER, "/n/bash-language-server"),
        ("taplo", "/t/taplo"),
        ("marksman", "/t/marksman"),
        ("shellcheck", "/t/shellcheck"),
        ("shfmt", "/t/shfmt"),
        ("hadolint", "/t/hadolint"),
        ("prettier", "/n/prettier"),
    ];

    #[test]
    fn every_server_is_set_with_its_settings() {
        let dir = temp("activate");
        write(&dir, &["README.md", "docs/a.md", ".github/workflows/ci.yml", "package.json", "Cargo.toml", "Dockerfile", "scripts/release.sh"]);
        let host = Shared::default();
        found(&host, EVERYTHING);
        let mut c = pack(&dir, &host, Settings::default);
        c.activate().unwrap();
        let calls = host.take();
        for want in [
            "add yaml yaml: /n/yaml-language-server --stdio",
            "add json json: /n/vscode-json-language-server --stdio",
            "add toml toml: /t/taplo lsp stdio",
            "add dockerfile dockerfile: /n/docker-langserver --stdio",
            "add markdown markdown: /t/marksman server",
            "server bash: /n/bash-language-server start",
            "formatter json: /n/prettier --stdin-filepath ${file}",
            "tasks shellcheck: every script, hadolint: every Dockerfile",
        ] {
            assert!(calls.iter().any(|c| c.starts_with(want)), "{want}: {calls:?}");
        }
        let settings = host.0.borrow().settings.clone();
        assert_eq!(settings["yaml"]["yaml"]["schemaStore"]["enable"], true);
        assert_eq!(settings["bash"]["bashIde"]["shellcheckPath"], "/t/shellcheck");
        assert_eq!(settings["json"]["json"]["schemas"].as_array().unwrap().len(), 1, "the catalog isn't here yet");
        assert_eq!(line(&host.0.borrow().tasks[0].cmd), "/t/shellcheck scripts/release.sh");

        // The catalog comes: kept, and the JSON server given it.
        let id = calls.iter().find_map(|c| c.strip_prefix("http ")?.strip_suffix(" GET https://www.schemastore.org/api/json/catalog.json")?.parse::<u64>().ok()).expect("the catalog asked for");
        let catalog = r#"{"schemas": [{"name": "package.json", "fileMatch": ["package.json"], "url": "https://www.schemastore.org/package.json"}]}"#;
        c.handle(Event::HttpResponse((id, Ok(HttpResponse::new(200, catalog.as_bytes().to_vec())))));
        assert_eq!(host.0.borrow().settings["json"]["json"]["schemas"].as_array().unwrap().len(), 2);
        let kept = std::fs::read_to_string(dir.join(".cache/schemastore.json")).unwrap();
        assert!(kept.contains("package.json"), "{kept}");
        assert!(host.take().iter().all(|c| !c.starts_with("add ")), "no server started again");

        // Checks for each kind the project has.
        let labels: Vec<String> = c.kit.doctor.programs.iter().map(|p| p.label.clone()).collect();
        assert_eq!(labels, ["YAML server", "JSON server", "Taplo (TOML)", "Dockerfile server", "hadolint", "Bash server", "ShellCheck", "shfmt", "Marksman (Markdown)", "Prettier (formats JSON)"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_kept_catalog_is_used_until_its_a_week_old() {
        let dir = temp("cache");
        write(&dir, &["package.json"]);
        std::fs::create_dir_all(dir.join(".cache")).unwrap();
        let list = [Schema { files: vec!["tsconfig*.json".into()], url: "https://www.schemastore.org/tsconfig.json".into() }];
        std::fs::write(dir.join(".cache/schemastore.json"), schemas::cache_text(1_000, &list)).unwrap();
        let host = Shared::default();
        found(&host, &[(tools::JSON_SERVER, "/n/json")]);
        host.0.borrow_mut().now = 1_000 + 60;
        let mut c = pack(&dir, &host, Settings::default);
        c.activate().unwrap();
        assert!(!host.take().iter().any(|c| c.starts_with("http")), "fresh");
        assert_eq!(host.0.borrow().settings["json"]["json"]["schemas"].as_array().unwrap().len(), 2);
        c.run_command("config.schemas").unwrap();
        assert!(host.take().iter().any(|c| c.starts_with("http")), "fetched again when asked");
        // A failed fetch keeps what was kept.
        let id = c.fetching.unwrap();
        c.handle(Event::HttpResponse((id, Ok(HttpResponse::new(503, Vec::new())))));
        assert_eq!(c.schemas.len(), 1);
        assert!(host.take().iter().any(|c| c.contains("SchemaStore said 503")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_turn_servers_and_prettier_off() {
        let dir = temp("settings");
        write(&dir, &["a.yaml", "b.json", ".prettierrc"]);
        let host = Shared::default();
        found(&host, EVERYTHING);
        let mut c = pack(&dir, &host, Settings::default);
        c.activate().unwrap();
        assert!(host.take().contains(&"formatter json,yaml,markdown: /n/prettier --stdin-filepath ${file}".to_string()), "a Prettier config: YAML and Markdown too");
        c.read_settings = || Settings { yaml: false, prettier: false, shell: false, ..Settings::default() };
        c.handle(Event::SettingChanged("config.yaml".into()));
        let calls = host.take();
        assert!(calls.contains(&"add yaml yaml: ".to_string()) || calls.iter().any(|c| c.starts_with("add yaml yaml") && !c.contains("--stdio")), "taken away: {calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("server bash") && !c.contains("start")), "Fenix's own Bash server back: {calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("formatter json,yaml,markdown") && !c.contains("prettier")), "no formatter: {calls:?}");
        assert!(c.run_command("config.nothing").is_err());

        let biome = Project { biome: true, ..Project::default() };
        let tools = Tools { prettier: Some("/n/prettier".into()), ..Tools::default() };
        assert_eq!(formatter(&tools, &Settings::default(), &biome), None, "Biome formats it");
        assert_eq!(formatter(&tools, &Settings::default(), &Project::default()).map(|(l, _)| l), Some(vec!["json".to_string()]));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_tool_installed_by_its_fix_row_is_used() {
        let dir = temp("fix");
        write(&dir, &["Cargo.toml", "Dockerfile"]);
        let host = Shared::default();
        found(&host, &[("npm", "/n/npm")]);
        let mut c = pack(&dir, &host, Settings::default);
        c.activate().unwrap();
        host.take();
        assert_eq!(c.kit.doctor.programs.iter().find(|p| p.name == "taplo").and_then(|p| p.fix.clone()).map(|f| f.1), Some("config.install.taplo".into()));
        c.run_command(doctor::INSTALL_DOCKER).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").and_then(|c| c.split_once(' ')).filter(|(_, rest)| rest.ends_with("/n/npm install -g dockerfile-language-server-nodejs")).map(|(id, _)| id.parse::<u64>().unwrap()));
        found(&host, &[(tools::DOCKER_SERVER, "/n/docker-langserver"), ("hadolint", "/t/hadolint")]);
        c.handle(Event::TaskFinished((task.expect("installed as a task"), true)));
        let calls = host.take();
        assert!(calls.contains(&"add dockerfile dockerfile: /n/docker-langserver --stdio".to_string()), "{calls:?}");
        assert_eq!(c.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["hadolint"]);

        // A Dockerfile saved: hadolint on it.
        host.0.borrow_mut().here = Some((format!("{}/Dockerfile", dir.display()), Some(dir.to_string_lossy().into_owned())));
        c.file_saved(&format!("{}/Dockerfile", dir.display()));
        let calls = host.take();
        assert!(calls.iter().any(|c| c.ends_with("/t/hadolint --format json --no-fail Dockerfile")), "{calls:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_first_file_of_a_kind_saved_checks_its_tools() {
        let dir = temp("first");
        write(&dir, &["Cargo.toml"]);
        let host = Shared::default();
        let mut c = pack(&dir, &host, Settings::default);
        c.activate().unwrap();
        assert!(!c.kit.doctor.programs.iter().any(|p| p.name == "shellcheck"));
        write(&dir, &["install.sh"]);
        c.handle(Event::Saved("install.sh".into()));
        assert!(c.kit.doctor.programs.iter().any(|p| p.name == "shellcheck"));
        for (id, line) in host.take().iter().filter_map(|c| c.strip_prefix("spawn ")?.split_once(' ')).map(|(id, l)| (id.parse::<u64>().unwrap(), l.to_string())) {
            said(&mut c, id, if line.contains("shellcheck") { "version: 0.11.0" } else { "1.0.0" });
        }
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        assert!(checks.contains(&("ShellCheck".into(), Health::Warn)), "{checks:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
