//! What the project is, read from its files: its rockspecs, the tools'
//! configs at its top (StyLua's, selene's, luacheck's, busted's,
//! `.luarc.json`), whether it's a LÖVE game or a Neovim config or
//! plugin, and whether it has Lua files at all, found by walking it (not
//! into rocks, builds or dot folders, and only so far). A project without
//! any gets no checks and no tasks.

use std::path::Path;

/// How deep the walk goes, and how many entries it reads at most.
pub const DEPTH: usize = 5;
pub const ENTRIES: usize = 10_000;

/// Folders never walked into: rocks installed into the project, builds.
const SKIP: &[&str] = &["lua_modules", "node_modules", "target", "build", "dist", "out"];

/// StyLua's config files.
const STYLUA: &[&str] = &["stylua.toml", ".stylua.toml"];

/// What was read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// Whether it has a Lua file, a rockspec or a `.luarc.json`.
    pub lua: bool,
    /// The rockspecs at its top, sorted.
    pub rockspecs: Vec<String>,
    pub stylua: bool,
    pub selene: bool,
    pub luacheck: bool,
    /// A `.luarc.json`: the server reads it, and it wins over the pack's
    /// settings.
    pub luarc: bool,
    /// busted's `.busted`, or a `spec` folder.
    pub busted: bool,
    /// `main.lua` with `conf.lua` beside it, or `love.` callbacks in it.
    pub love: bool,
    /// An `init.lua` that speaks to `vim`, or a `lua` folder beside a
    /// `plugin` folder or an `init.lua`.
    pub neovim: bool,
    /// `main.lua` at the top: what `run` runs.
    pub main: bool,
}

impl Project {
    pub fn read(root: &str) -> Project {
        let top = Path::new(root);
        let is_file = |name: &str| top.join(name).is_file();
        let is_dir = |name: &str| top.join(name).is_dir();
        let text = |name: &str| std::fs::read_to_string(top.join(name)).unwrap_or_default();
        let mut rockspecs: Vec<String> = std::fs::read_dir(top)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".rockspec") && top.join(n).is_file())
            .collect();
        rockspecs.sort();
        let main = is_file("main.lua");
        let love = main && (is_file("conf.lua") || text("main.lua").contains("function love."));
        let neovim = text("init.lua").contains("vim.") || (is_dir("lua") && (is_dir("plugin") || is_file("init.lua")));
        let luarc = is_file(".luarc.json") || is_file(".luarc.jsonc");
        let mut budget = ENTRIES;
        let lua = luarc || !rockspecs.is_empty() || has_lua(top, 0, &mut budget);
        Project {
            lua,
            rockspecs,
            stylua: STYLUA.iter().any(|f| is_file(f)),
            selene: is_file("selene.toml"),
            luacheck: is_file(".luacheckrc"),
            luarc,
            busted: is_file(".busted") || is_dir("spec"),
            love,
            neovim,
            main,
        }
    }

    /// The Lua the server reads it as, when the project says: LÖVE and
    /// Neovim run LuaJIT.
    pub fn runtime(&self) -> Option<&'static str> {
        (self.love || self.neovim).then_some("LuaJIT")
    }

    /// Whether a file of this name, saved, changes what the project is.
    pub fn is_config(name: &str) -> bool {
        let name = name.replace('\\', "/");
        let name = name.rsplit('/').next().unwrap_or_default();
        name.ends_with(".rockspec") || STYLUA.contains(&name) || matches!(name, "selene.toml" | ".luacheckrc" | ".busted" | ".luarc.json" | ".luarc.jsonc" | "main.lua" | "conf.lua" | "init.lua")
    }
}

/// Whether there's a `.lua` file under `dir`, `depth` down.
fn has_lua(dir: &Path, depth: usize, budget: &mut usize) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    let mut folders = Vec::new();
    for entry in entries.flatten() {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if depth + 1 < DEPTH && !name.starts_with('.') && !SKIP.contains(&name.as_str()) {
                folders.push(entry.path());
            }
        } else if name.to_ascii_lowercase().ends_with(".lua") {
            return true;
        }
    }
    folders.sort();
    folders.iter().any(|f| has_lua(f, depth + 1, budget))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for a test.
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-lua-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Files written, each `path` or `path=text`.
    pub fn write(dir: &Path, files: &[&str]) {
        for f in files {
            let (path, text) = f.split_once('=').unwrap_or((f, ""));
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    #[test]
    fn a_rock_with_its_tools_configs_is_read() {
        let dir = temp("rock");
        write(&dir, &["orbit-1.0-1.rockspec", "orbit-scm-1.rockspec", "src/orbit.lua", "spec/orbit_spec.lua", "stylua.toml", "selene.toml", ".luacheckrc"]);
        let p = Project::read(&dir.to_string_lossy());
        assert_eq!(p.rockspecs, ["orbit-1.0-1.rockspec", "orbit-scm-1.rockspec"]);
        assert!(p.lua && p.stylua && p.selene && p.luacheck && p.busted);
        assert!(!p.love && !p.neovim && !p.main && !p.luarc);
        assert_eq!(p.runtime(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn love_games_and_neovim_configs_are_known() {
        let dir = temp("love");
        write(&dir, &["main.lua=function love.draw()\nend\n", ".stylua.toml"]);
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.love && p.main && p.stylua && !p.neovim, "{p:?}");
        assert_eq!(p.runtime(), Some("LuaJIT"));
        std::fs::remove_dir_all(&dir).unwrap();

        let dir = temp("nvim");
        write(&dir, &["init.lua=vim.opt.number = true\n", "lua/config/keys.lua"]);
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.neovim && p.lua && !p.love && !p.main);
        std::fs::remove_dir_all(&dir).unwrap();

        let dir = temp("plugin");
        write(&dir, &["lua/orbit/init.lua", "plugin/orbit.lua", ".luarc.json"]);
        let p = Project::read(&dir.to_string_lossy());
        assert!(p.neovim && p.luarc);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_project_without_lua_is_not_one_and_rocks_are_not_walked() {
        let dir = temp("none");
        write(&dir, &["Cargo.toml", "src/main.rs", "lua_modules/share/lua/5.4/x.lua", ".git/hooks/x.lua", "main.lua.txt"]);
        assert!(!Project::read(&dir.to_string_lossy()).lua);
        write(&dir, &["scripts/deep/er/still/build.lua"]);
        assert!(Project::read(&dir.to_string_lossy()).lua, "five down");
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(Project::is_config("/project/orbit-1.0-1.rockspec") && Project::is_config("selene.toml") && Project::is_config("sub\\.luacheckrc"));
        assert!(!Project::is_config("src/orbit.lua") && !Project::is_config("Cargo.toml"));
    }
}
