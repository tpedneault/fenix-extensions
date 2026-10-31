//! The project doctor's Lua checks, in a project with Lua in it: the Lua
//! that runs it (LuaJIT's in a LÖVE game or a Neovim config, where it's
//! only a suggestion), LuaRocks with a rockspec, the server, StyLua and
//! the linter when they're used, busted with specs, and LÖVE for a game.
//! Each that's missing has a fix row where there's one: the release for
//! this machine for the server, StyLua and selene; `luarocks install
//! --local` for busted and luacheck.

use fenix_lang_kit::{Fix, Program};

use crate::lint::LintWith;
use crate::project::Project;
use crate::tools::{hints, Platform, Tools, SERVER};

/// The fix rows' commands, in the manifest.
pub const INSTALL_BUSTED: &str = "lua.install-busted";
pub const INSTALL_LUACHECK: &str = "lua.install-luacheck";

/// `name`, looked for where the pack installs it and the managers do.
fn program(name: &str, label: &str) -> Program {
    hints(name).into_iter().fold(Program::new(name, label), Program::hint)
}

/// Its fix row installs the release of that name, when there's one for
/// this machine (`lua.install.<name>`).
fn release_fix(program: Program, label: &str, name: &str, platform: &Platform) -> Program {
    if platform.installs().iter().any(|i| i.name == name) {
        program.fix(label, &format!("lua.install.{name}"), Fix::Install(name.into()))
    } else {
        program
    }
}

/// Its fix row installs a rock with LuaRocks, when LuaRocks is here.
fn rock_fix(program: Program, rock: &str, command: &str, tools: &Tools) -> Program {
    match tools.install_rock(rock) {
        Some(cmd) => program.fix(&format!("Install {rock}"), command, Fix::Task(cmd)),
        None => program,
    }
}

/// The checks for `project`, with what's on this machine; `stylua` and
/// `linter` are what the settings make of it.
pub fn programs(project: &Project, tools: &Tools, platform: &Platform, stylua: bool, linter: Option<LintWith>) -> Vec<Program> {
    let mut out = Vec::new();
    if !project.lua {
        return out;
    }

    // LÖVE and Neovim bring their own LuaJIT: one here is only for
    // running a script by hand.
    let embedded = project.love || project.neovim;
    let name = if tools.lua.is_none() && tools.luajit.is_some() { "luajit" } else { "lua" };
    let lua = program(name, if name == "lua" { "Lua" } else { "LuaJIT" }).version_args(["-v"]);
    out.push(if embedded { lua.optional() } else { lua });
    if !project.rockspecs.is_empty() {
        out.push(program("luarocks", "LuaRocks").version_args(["--version"]));
    }

    out.push(release_fix(program(SERVER, "lua-language-server").optional(), "Install lua-language-server", SERVER, platform));
    if stylua {
        out.push(release_fix(program("stylua", "StyLua").optional(), "Install StyLua", "stylua", platform));
    }
    match linter {
        Some(LintWith::Selene) => out.push(release_fix(program("selene", "selene").optional(), "Install selene", "selene", platform)),
        Some(LintWith::Luacheck) => out.push(rock_fix(program("luacheck", "luacheck").optional(), "luacheck", INSTALL_LUACHECK, tools)),
        None => {}
    }
    if project.busted {
        out.push(rock_fix(program("busted", "busted").optional(), "busted", INSTALL_BUSTED, tools));
    }
    if project.love {
        out.push(program("love", "LÖVE").optional());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_extension_api::Health;
    use fenix_lang_kit::Output;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    fn row(label: &str, required: bool, fix: Option<&str>) -> (String, bool, Option<String>) {
        (label.into(), required, fix.map(str::to_string))
    }

    #[test]
    fn a_rock_gets_lua_luarocks_and_its_tools() {
        let project = Project { lua: true, rockspecs: vec!["orbit-1.0-1.rockspec".into()], busted: true, ..Project::default() };
        let tools = Tools { luarocks: Some("/usr/bin/luarocks".into()), ..Tools::default() };
        let p = programs(&project, &tools, &Platform::new("linux", "x86_64"), true, Some(LintWith::Luacheck));
        assert_eq!(
            rows(&p),
            [
                row("Lua", true, None),
                row("LuaRocks", true, None),
                row("lua-language-server", false, Some("lua.install.lua-language-server")),
                row("StyLua", false, Some("lua.install.stylua")),
                row("luacheck", false, Some(INSTALL_LUACHECK)),
                row("busted", false, Some(INSTALL_BUSTED)),
            ]
        );
        match &p[5].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(crate::tests::line(cmd), "/usr/bin/luarocks install --local busted"),
            other => panic!("{other:?}"),
        }
        assert_eq!(p[2].hints[0], "/tools/lua-language-server/bin");

        // What each says of its version.
        let c = p[0].evaluate(Some("/usr/bin/lua"), Some(&out(0, "", "Lua 5.1.5  Copyright (C) 1994-2012 Lua.org, PUC-Rio\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "5.1.5 (/usr/bin/lua)"), "5.1 says it on stderr");
        let c = p[1].evaluate(Some("/usr/bin/luarocks"), Some(&out(0, "/usr/bin/luarocks 3.8.0\nLuaRocks main command-line interface\n", "")));
        assert_eq!(c.detail, "3.8.0 (/usr/bin/luarocks)");
        let c = p[2].evaluate(Some("/tools/lua-language-server/bin/lua-language-server"), Some(&out(0, "3.19.2\n", "")));
        assert_eq!(c.detail, "3.19.2 (/tools/lua-language-server/bin/lua-language-server)");
        let c = p[3].evaluate(None, None);
        assert_eq!((c.health, c.fix.map(|f| f.1)), (Health::Warn, Some("lua.install.stylua".into())));
    }

    #[test]
    fn a_love_game_on_luajit_alone_needs_no_lua() {
        let project = Project { lua: true, love: true, main: true, ..Project::default() };
        let tools = Tools { luajit: Some("/usr/bin/luajit".into()), ..Tools::default() };
        let p = programs(&project, &tools, &Platform::new("macos", "x86_64"), false, Some(LintWith::Selene));
        assert_eq!(rows(&p), [row("LuaJIT", false, None), row("lua-language-server", false, Some("lua.install.lua-language-server")), row("selene", false, None), row("LÖVE", false, None)], "no selene for Intel Macs");
        let c = p[0].evaluate(Some("/usr/bin/luajit"), Some(&out(0, "LuaJIT 2.1.1727870382 -- Copyright (C) 2005-2023 Mike Pall. https://luajit.org/\n", "")));
        assert_eq!(c.health, Health::Ok);
        let c = p[3].evaluate(Some("/Applications/love.app/Contents/MacOS/love"), Some(&out(0, "LOVE 11.5 (Mysterious Mysteries)\n", "")));
        assert_eq!(c.detail, "11.5 (/Applications/love.app/Contents/MacOS/love)");
    }

    #[test]
    fn a_project_without_lua_is_not_checked() {
        assert!(programs(&Project::default(), &Tools::default(), &Platform::new("linux", "x86_64"), true, Some(LintWith::Selene)).is_empty());
        let p = programs(&Project { lua: true, ..Project::default() }, &Tools::default(), &Platform::new("windows", "aarch64"), false, None);
        assert_eq!(rows(&p), [row("Lua", true, None), row("lua-language-server", false, None)], "no release for this machine: no fix");
    }
}
