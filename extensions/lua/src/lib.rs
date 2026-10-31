//! Lua in Fenix, on lang-kit: lua-language-server as the language server,
//! given the Lua a LÖVE game or a Neovim config runs on; StyLua as the
//! formatter in a project that has its config; selene or luacheck on
//! save, as the project's config says; tasks for `main.lua`, LÖVE,
//! busted and LuaRocks; and the project doctor's checks with their fixes.
//!
//! Fenix highlights Lua itself and knows its comment, so the pack brings
//! no grammar. What it knows of the project is read from its files
//! ([`project`]), and what's installed is found ([`tools`]); the server,
//! StyLua and selene are installed from their releases by the doctor's
//! fix rows. A rockspec or a tool's config saved, a fix that installs a
//! tool, or a setting changed, sets them again.

pub mod doctor;
pub mod lint;
pub mod project;
pub mod tasks;
pub mod tools;

#[cfg(test)]
mod data;

use fenix_extension_api::{self as api, export, Command, Event, Extension, Key, MouseEvent, Page};
use fenix_lang_kit::serde_json::{json, Value};
use fenix_lang_kit::{Fenix, Host, Kit};

use lint::LintWith;
use project::Project;
use tools::{Platform, Tools};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language.
const LUA: &[&str] = &["lua"];

/// What formats a Lua file (`lua.formatter`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Formatter {
    /// StyLua in a project with its config, else the server.
    #[default]
    Auto,
    Stylua,
    Server,
}

/// What lints a saved Lua file (`lua.linter`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Linting {
    /// selene with a `selene.toml`, luacheck with a `.luacheckrc`.
    #[default]
    Auto,
    Selene,
    Luacheck,
    Off,
}

/// What the pack's settings say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub formatter: Formatter,
    pub linter: Linting,
}

impl Settings {
    /// From Fenix: anything not one of the choices is `auto`.
    pub fn read() -> Settings {
        let setting = |key: &str| api::setting(key).unwrap_or_default().trim().to_ascii_lowercase();
        Settings {
            formatter: match setting("lua.formatter").as_str() {
                "stylua" => Formatter::Stylua,
                "server" => Formatter::Server,
                _ => Formatter::Auto,
            },
            linter: match setting("lua.linter").as_str() {
                "selene" => Linting::Selene,
                "luacheck" => Linting::Luacheck,
                "off" => Linting::Off,
                _ => Linting::Auto,
            },
        }
    }

    /// Whether StyLua formats this project.
    pub fn stylua(&self, project: &Project) -> bool {
        match self.formatter {
            Formatter::Auto => project.stylua,
            Formatter::Stylua => true,
            Formatter::Server => false,
        }
    }

    /// Which linter runs on save in this project, if any.
    pub fn linter(&self, project: &Project) -> Option<LintWith> {
        match self.linter {
            Linting::Auto if project.selene => Some(LintWith::Selene),
            Linting::Auto if project.luacheck => Some(LintWith::Luacheck),
            Linting::Auto | Linting::Off => None,
            Linting::Selene => Some(LintWith::Selene),
            Linting::Luacheck => Some(LintWith::Luacheck),
        }
    }
}

/// StyLua, reading the buffer and finding its config from the file's
/// folder; none when the settings leave it to the server or it isn't
/// installed.
pub fn formatter(tools: &Tools, settings: &Settings, project: &Project) -> Option<Command> {
    let stylua = tools.stylua.as_deref().filter(|_| settings.stylua(project))?;
    Some(Command::new(stylua).args(["--stdin-filepath", "${file}", "-"]))
}

/// What the server is given: the definitions it has for LÖVE, busted and
/// the rest applied without asking (Fenix can't answer it), and LuaJIT
/// with Neovim's `vim` for a Neovim config or plugin. A `.luarc.json`
/// wins over these.
pub fn server_settings(project: &Project) -> Value {
    let mut lua = json!({"workspace": {"checkThirdParty": "ApplyInMemory"}});
    if let Some(runtime) = project.runtime() {
        lua["runtime"] = json!({"version": runtime});
    }
    if project.neovim {
        lua["diagnostics"] = json!({"globals": ["vim"]});
    }
    json!({ "Lua": lua })
}

pub struct Lua {
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
    set_settings: Option<Value>,
}

impl Default for Lua {
    fn default() -> Lua {
        let platform = api::platform();
        Lua::new(Box::new(Fenix), PROJECT, Settings::read, Platform::new(&platform.os, &platform.arch))
    }
}

impl Lua {
    /// The pack for the project in `root`, talking to `host`, on
    /// `platform`.
    pub fn new(mut host: Box<dyn Host>, root: &str, read_settings: fn() -> Settings, platform: Platform) -> Lua {
        let tools = Tools::find(host.as_mut());
        let mut kit = Kit::with_host("lua", host);
        for install in platform.installs() {
            kit.installer.add(install);
        }
        let mut l = Lua { kit, root: root.into(), project: Project::read(root), tools, platform, settings: read_settings(), read_settings, set_server: None, set_formatter: None, set_settings: None };
        l.configure();
        l
    }

    /// The kit's tasks, checks and linter, for the project, tools and
    /// settings as they are.
    fn configure(&mut self) {
        let (project, tools) = (&self.project, &self.tools);
        let (stylua, linter) = (self.settings.stylua(project), self.settings.linter(project));
        self.kit.tasks = tasks::tasks(project, tools, &self.root, stylua, linter);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(project, tools, &self.platform, stylua, linter) {
            self.kit.doctor.program(program);
        }
        let linter = match (linter.filter(|_| project.lua), &tools.selene, &tools.luacheck) {
            (Some(LintWith::Selene), Some(selene), _) => Some(lint::selene(&Command::new(selene))),
            (Some(LintWith::Luacheck), _, Some(luacheck)) => Some(lint::luacheck(&Command::new(luacheck))),
            _ => None,
        };
        // One turned off: its problems taken off too.
        let gone: Vec<String> = self.kit.lint.linters.iter().map(|l| l.name.clone()).filter(|n| linter.as_ref().is_none_or(|l| l.name != *n)).collect();
        for name in gone {
            self.kit.lint.remove(self.kit.host.as_mut(), &name);
        }
        if let Some(l) = linter {
            self.kit.lint.add(l);
        }
    }

    /// The server, its settings and the formatter set in Fenix, where
    /// they changed. No server found leaves Fenix's own.
    fn apply(&mut self) {
        let server = self.tools.server.as_deref().map(Command::new);
        let formatter = formatter(&self.tools, &self.settings, &self.project);
        let settings = server_settings(&self.project);
        let languages: Vec<String> = LUA.iter().map(|l| l.to_string()).collect();
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
            // Started again: told its settings again.
            self.set_settings = None;
        }
        if self.set_settings.as_ref() != Some(&settings) {
            if let Err(why) = host.language_server_settings("lua", Some(&settings)) {
                host.log(&format!("settings for the server: {why}"));
            }
            self.set_settings = Some(settings);
        }
        if formatter != self.set_formatter {
            if let Err(why) = host.formatter(&languages, formatter.as_ref()) {
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
}

impl Extension for Lua {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = self.kit.run_command(id).unwrap_or_else(|| Err(format!("no command {id} here")));
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        // A rockspec or a tool's config saved, or the first Lua file.
        let saved = matches!(&event, Event::Saved(name) if Project::is_config(name) || (!self.project.lua && name.to_ascii_lowercase().ends_with(".lua")));
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("lua."));
        let installing = self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        if saved {
            self.reload();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.configure();
            if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
                self.kit.host.log(&format!("tasks: {why}"));
            }
            self.kit.doctor.run(self.kit.host.as_mut());
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

export!(Lua);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, write};
    use fenix_extension_api::Health;
    use fenix_lang_kit::host::mock::Shared;

    /// A command as a line, its program first.
    pub fn line(cmd: &Command) -> String {
        std::iter::once(cmd.program.clone()).chain(cmd.args.iter().cloned()).collect::<Vec<_>>().join(" ")
    }

    fn found(host: &Shared, programs: &[(&str, &str)]) {
        host.0.borrow_mut().programs.extend(programs.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    }

    fn pack(dir: &std::path::Path, host: &Shared, settings: fn() -> Settings) -> Lua {
        let mut l = Lua::new(Box::new(host.clone()), &dir.to_string_lossy(), settings, Platform::new("linux", "x86_64"));
        l.kit.installer.tools = dir.join(".tools").to_string_lossy().into_owned();
        l
    }

    fn said(l: &mut Lua, process: u64, text: &str) {
        l.handle(Event::ProcessOutput(api::ProcessOutput { process, stderr: false, line: text.into() }));
        l.handle(Event::ProcessExit((process, Some(0))));
    }

    /// Every check answered as `answer` says for its command line.
    fn answer(l: &mut Lua, host: &Shared, answer: impl Fn(&str) -> &'static str) {
        let spawned: Vec<(u64, String)> = host.take().iter().filter_map(|c| c.strip_prefix("spawn ")?.split_once(' ')).map(|(id, l)| (id.parse().unwrap(), l.to_string())).collect();
        for (id, cmd) in spawned {
            said(l, id, answer(&cmd));
        }
    }

    const EVERYTHING: &[(&str, &str)] = &[
        ("lua", "/usr/bin/lua"),
        ("luarocks", "/usr/bin/luarocks"),
        (tools::SERVER, "/t/lua-language-server"),
        ("stylua", "/t/stylua"),
        ("selene", "/t/selene"),
        ("busted", "/h/.luarocks/bin/busted"),
    ];

    #[test]
    fn a_rock_gets_its_server_formatter_linter_tasks_and_checks() {
        let dir = temp("activate");
        write(&dir, &["orbit-1.0-1.rockspec", "src/orbit.lua", "spec/orbit_spec.lua", "stylua.toml", "selene.toml"]);
        let host = Shared::default();
        found(&host, EVERYTHING);
        let mut l = pack(&dir, &host, Settings::default);
        l.activate().unwrap();
        let calls = host.0.borrow().calls.clone();
        for want in [
            "tasks test, luarocks make, luarocks test, lint, format",
            "server lua: /t/lua-language-server",
            "settings lua: {\"Lua\":{\"workspace\":{\"checkThirdParty\":\"ApplyInMemory\"}}}",
            "formatter lua: /t/stylua --stdin-filepath ${file} -",
            "spawn 1 /usr/bin/lua -v",
        ] {
            assert!(calls.iter().any(|c| c == want), "{want}: {calls:?}");
        }
        assert_eq!(l.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["selene"]);
        answer(&mut l, &host, |cmd| match cmd {
            c if c.ends_with("lua -v") => "Lua 5.4.6  Copyright (C) 1994-2023 Lua.org, PUC-Rio",
            c if c.contains("luarocks") => "/usr/bin/luarocks 3.11.1",
            c if c.contains("lua-language-server") => "3.19.2",
            c if c.contains("stylua") => "stylua 2.6.0",
            c if c.contains("selene") => "selene 0.31.0",
            _ => "2.2.0",
        });
        let checks: Vec<(String, Health)> = host.0.borrow().checks.iter().map(|c| (c.label.clone(), c.health)).collect();
        let labels: Vec<&str> = checks.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(labels, ["Lua", "LuaRocks", "lua-language-server", "StyLua", "selene", "busted"]);
        assert!(checks.iter().all(|c| c.1 == Health::Ok), "{checks:?}");

        // A Lua file saved: selene on it.
        host.0.borrow_mut().here = Some((format!("{}/src/orbit.lua", dir.display()), Some(dir.to_string_lossy().into_owned())));
        host.take();
        l.file_saved(&format!("{}/src/orbit.lua", dir.display()));
        assert!(host.take().iter().any(|c| c.ends_with("/t/selene --display-style=json2 --no-summary src/orbit.lua")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_neovim_config_gets_luajit_and_vim_and_the_servers_formatting() {
        let dir = temp("nvim");
        write(&dir, &["init.lua=vim.g.mapleader = ' '\n", "lua/plugins/ui.lua", ".luacheckrc"]);
        let host = Shared::default();
        found(&host, &[(tools::SERVER, "/m/lua-language-server"), ("stylua", "/m/stylua"), ("luacheck", "/m/luacheck")]);
        let mut l = pack(&dir, &host, Settings::default);
        l.activate().unwrap();
        let calls = host.take();
        assert!(!calls.iter().any(|c| c.starts_with("formatter")), "no stylua.toml: the server formats");
        let settings = host.0.borrow().settings["lua"].clone();
        assert_eq!(settings["Lua"]["runtime"]["version"], "LuaJIT");
        assert_eq!(settings["Lua"]["diagnostics"]["globals"][0], "vim");
        assert_eq!(l.kit.lint.linters.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["luacheck"], "its .luacheckrc");
        assert!(l.kit.doctor.programs.iter().find(|p| p.name == "lua").is_some_and(|p| !p.required), "Neovim brings its own");

        // StyLua asked for: it formats, and is checked.
        l.read_settings = || Settings { formatter: Formatter::Stylua, linter: Linting::Off };
        l.handle(Event::SettingChanged("lua.formatter".into()));
        let calls = host.take();
        assert!(calls.contains(&"formatter lua: /m/stylua --stdin-filepath ${file} -".to_string()), "{calls:?}");
        assert!(calls.iter().any(|c| c == "tasks format"), "{calls:?}");
        assert!(l.kit.lint.linters.is_empty() && l.kit.doctor.programs.iter().any(|p| p.name == "stylua"));
        l.read_settings = || Settings { formatter: Formatter::Server, ..Settings::default() };
        l.handle(Event::SettingChanged("lua.formatter".into()));
        assert!(host.take().contains(&"formatter lua: ".to_string()), "given back to the server");
        assert!(l.run_command("lua.nothing").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_tool_installed_by_its_fix_row_is_used() {
        let dir = temp("fix");
        write(&dir, &["main.lua", "spec/main_spec.lua"]);
        let host = Shared::default();
        found(&host, &[("lua", "/usr/bin/lua"), ("luarocks", "/usr/bin/luarocks")]);
        let mut l = pack(&dir, &host, Settings::default);
        l.activate().unwrap();
        let calls = host.take();
        assert!(!calls.iter().any(|c| c.starts_with("server")), "no server found: Fenix's own");
        assert!(calls.contains(&"tasks run, test".to_string()), "{calls:?}");
        l.run_command(doctor::INSTALL_BUSTED).unwrap();
        let task = host.take().iter().find_map(|c| c.strip_prefix("task ").and_then(|c| c.split_once(' ')).filter(|(_, rest)| rest.ends_with("/usr/bin/luarocks install --local busted")).map(|(id, _)| id.parse::<u64>().unwrap()));
        found(&host, &[("busted", "/h/.luarocks/bin/busted")]);
        l.handle(Event::TaskFinished((task.expect("installed as a task"), true)));
        assert!(host.take().iter().any(|c| c == "tasks run, test"));
        assert_eq!(line(&host.0.borrow().tasks[1].cmd), "/h/.luarocks/bin/busted");

        // The server's fix row downloads its release, checked.
        let row = l.kit.doctor.programs.iter().find(|p| p.name == tools::SERVER).and_then(|p| p.fix.clone()).map(|f| f.1).unwrap();
        l.run_command(&row).unwrap();
        let calls = host.take();
        assert!(calls.iter().any(|c| c.starts_with("download ") && c.contains("lua-language-server-3.19.2-linux-x64.tar.gz") && c.ends_with(" 93ad326b10f6c00911dd8c450fa098332dbd8f9f45d1052f5636a2d54479b947")), "{calls:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_first_lua_file_saved_makes_it_a_lua_project() {
        let dir = temp("first");
        write(&dir, &["README.md"]);
        let host = Shared::default();
        let mut l = pack(&dir, &host, Settings::default);
        l.activate().unwrap();
        assert!(l.kit.doctor.programs.is_empty() && l.kit.tasks.tasks.is_empty());
        write(&dir, &["main.lua", "conf.lua"]);
        l.handle(Event::Saved("main.lua".into()));
        assert_eq!(l.kit.doctor.programs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["Lua", "lua-language-server", "LÖVE"]);
        assert_eq!(l.kit.tasks.tasks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["love"]);
        assert_eq!(host.0.borrow().settings["lua"]["Lua"]["runtime"]["version"], "LuaJIT", "LÖVE runs LuaJIT");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
