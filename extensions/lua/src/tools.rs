//! The programs the pack runs, found on this machine: Lua or LuaJIT,
//! LuaRocks and what it installs (busted, luacheck), LÖVE, and the
//! programs the pack installs into its tools folder from their releases
//! when they aren't here already (lua-language-server, StyLua, selene).
//! LuaRocks' `--local`, Cargo, Homebrew, winget, Scoop and Neovim's Mason
//! put programs in folders an editor started from the desktop may not
//! have on its `PATH`, so they're looked in too.

use fenix_extension_api::Command;
use fenix_lang_kit::{Host, Install};

/// Where else to look: LuaRocks' `--local` tree, Mason's (Neovim's
/// installer of servers and formatters), Cargo's, Homebrew's, LÖVE's app
/// and its Windows installer's folder, winget's links and Scoop's shims.
pub const HINTS: &[&str] = &[
    "$HOME/.luarocks/bin", "$HOME/.local/share/nvim/mason/bin", "$HOME/.local/bin", "$HOME/.cargo/bin", "/opt/homebrew/bin", "/usr/local/bin",
    "/Applications/love.app/Contents/MacOS", "%APPDATA%\\luarocks\\bin", "%LOCALAPPDATA%\\nvim-data\\mason\\bin", "%USERPROFILE%\\.cargo\\bin",
    "%ProgramFiles%\\LOVE", "%LOCALAPPDATA%\\Microsoft\\WinGet\\Links", "%USERPROFILE%\\scoop\\shims",
];

/// The language server, by its program.
pub const SERVER: &str = "lua-language-server";

/// Where `name` is looked for: its own folder under the tools folder
/// first, where the pack installs it (the server's `bin`), then [`HINTS`].
pub fn hints(name: &str) -> Vec<String> {
    let own = if name == SERVER { format!("/tools/{SERVER}/bin") } else { format!("/tools/{name}") };
    std::iter::once(own).chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// The releases the pack installs, and each file's SHA-256, by OS and
/// architecture as `platform()` says them.
pub const LUALS: &str = "3.19.2";
const LUALS_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "linux-x64.tar.gz:93ad326b10f6c00911dd8c450fa098332dbd8f9f45d1052f5636a2d54479b947"),
    ("linux", "aarch64", "linux-arm64.tar.gz:17b8e401a874440adeb649e5024c364f2bb9d15156c74a40e56d1ae9a1b2e4f2"),
    ("macos", "x86_64", "darwin-x64.tar.gz:6f2174ece5ec63bcd9d5b22b2c902445c1cd9045fafe48c62602448ebf4c09cc"),
    ("macos", "aarch64", "darwin-arm64.tar.gz:79149584320a23bd5fcd5bc31f47a1975acd8a4560a0e89036bb1016fa61687f"),
    ("windows", "x86_64", "win32-x64.zip:4a3dc851b3b31e7b75a23f645d1ba4a2137579a55f5fa4edbb17a3e2a6254044"),
];

pub const STYLUA: &str = "2.6.0";
const STYLUA_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "stylua-linux-x86_64.zip:a485f72bbfa22e4642e77a84b5edc2611a5cab7477b34e2e709dd8f78e919895"),
    ("linux", "aarch64", "stylua-linux-aarch64.zip:8e67539e5fdf7a81cd03cf592b0df18d3bb96ee17a5ea5d9bf67b86387b903b0"),
    ("macos", "x86_64", "stylua-macos-x86_64.zip:90779aa873837b2dff8eb3361bd27950303914c24265a59cae0a0d0629d71585"),
    ("macos", "aarch64", "stylua-macos-aarch64.zip:dab9b66d340ca5ffc8225211f1dcf89b84783e09b8cbec8c518497eba81f3c42"),
    ("windows", "x86_64", "stylua-windows-x86_64.zip:fd13b4014e3b779c244bbc03f2c282e357f15bc526de6a6fa5ecbc447538313c"),
];

/// selene's releases: Linux and Windows on x86_64, macOS on arm64 only.
pub const SELENE: &str = "0.31.0";
const SELENE_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "selene-0.31.0-linux.zip:dac452422747999ec4919bbb8bb52992b66aae533b60022bf005669de8616671"),
    ("macos", "aarch64", "selene-0.31.0-macos.zip:67f644e57e14ccb74a0c272bc44af0dc7909d8bdff58e4e59bb3524717da5741"),
    ("windows", "x86_64", "selene-0.31.0-windows.zip:c5d5d087daa8e38bd71680b2202a407e5d4bc00fd584a648dec17ef9b29a2b73"),
];

/// The machine, as `platform()` says it (`linux`, `x86_64`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Platform {
    pub os: String,
    pub arch: String,
}

impl Platform {
    pub fn new(os: &str, arch: &str) -> Platform {
        Platform { os: os.into(), arch: arch.into() }
    }

    /// `name` as a program here: `stylua.exe` on Windows.
    fn exe(&self, name: &str) -> String {
        if self.os == "windows" {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }

    /// The file and SHA-256 for this machine in `files`.
    fn file<'a>(&self, files: &'a [(&str, &str, &str)]) -> Option<(&'a str, &'a str)> {
        let arch = if self.arch == "arm64" { "aarch64" } else { self.arch.as_str() };
        let (_, _, file) = files.iter().find(|(os, a, _)| *os == self.os && *a == arch)?;
        file.split_once(':')
    }

    /// lua-language-server's release for this machine, into
    /// `/tools/lua-language-server`: the server in `bin`, beside the Lua
    /// it's written in, which it finds from there.
    pub fn server(&self) -> Option<Install> {
        let (file, sha) = self.file(LUALS_FILES)?;
        let url = format!("https://github.com/LuaLS/lua-language-server/releases/download/{LUALS}/lua-language-server-{LUALS}-{file}");
        Some(Install::new(SERVER, &url).label(&format!("lua-language-server {LUALS}")).sha256(sha).program(&self.exe(&format!("bin/{SERVER}"))))
    }

    /// StyLua's release: a zip with the program alone.
    pub fn stylua(&self) -> Option<Install> {
        let (file, sha) = self.file(STYLUA_FILES)?;
        let url = format!("https://github.com/JohnnyMorganz/StyLua/releases/download/v{STYLUA}/{file}");
        Some(Install::new("stylua", &url).label(&format!("StyLua {STYLUA}")).sha256(sha).program(&self.exe("stylua")))
    }

    /// selene's release: a zip with the program alone.
    pub fn selene(&self) -> Option<Install> {
        let (file, sha) = self.file(SELENE_FILES)?;
        let url = format!("https://github.com/Kampfkarren/selene/releases/download/{SELENE}/{file}");
        Some(Install::new("selene", &url).label(&format!("selene {SELENE}")).sha256(sha).program(&self.exe("selene")))
    }

    /// Every release there's one of for this machine.
    pub fn installs(&self) -> Vec<Install> {
        [self.server(), self.stylua(), self.selene()].into_iter().flatten().collect()
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub lua: Option<String>,
    pub luajit: Option<String>,
    pub luarocks: Option<String>,
    pub server: Option<String>,
    pub stylua: Option<String>,
    pub selene: Option<String>,
    pub luacheck: Option<String>,
    pub busted: Option<String>,
    pub love: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let mut find = |name: &str| host.find_program(name, &hints(name));
        Tools {
            lua: find("lua"),
            luajit: find("luajit"),
            luarocks: find("luarocks"),
            server: find(SERVER),
            stylua: find("stylua"),
            selene: find("selene"),
            luacheck: find("luacheck"),
            busted: find("busted"),
            love: find("love"),
        }
    }

    /// What runs a script: Lua, else LuaJIT, else `lua` as Fenix finds it.
    pub fn interpreter(&self) -> Command {
        Command::new(self.lua.as_deref().or(self.luajit.as_deref()).unwrap_or("lua"))
    }

    /// LuaRocks, found or by its name.
    pub fn luarocks(&self) -> Command {
        Command::new(self.luarocks.as_deref().unwrap_or("luarocks"))
    }

    /// A rock installed into your own tree (`~/.luarocks`), which needs
    /// no administrator; none without LuaRocks.
    pub fn install_rock(&self, rock: &str) -> Option<Command> {
        self.luarocks.as_ref().map(|l| Command::new(l).args(["install", "--local", rock]))
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        Some(match name {
            "lua" => &mut self.lua,
            "luajit" => &mut self.luajit,
            "luarocks" => &mut self.luarocks,
            SERVER => &mut self.server,
            "stylua" => &mut self.stylua,
            "selene" => &mut self.selene,
            "luacheck" => &mut self.luacheck,
            "busted" => &mut self.busted,
            "love" => &mut self.love,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;
    use fenix_lang_kit::archive::Kind;

    #[test]
    fn each_tool_is_installed_from_the_release_for_this_machine() {
        let linux = Platform::new("linux", "x86_64");
        let server = linux.server().unwrap();
        assert_eq!(server.url, "https://github.com/LuaLS/lua-language-server/releases/download/3.19.2/lua-language-server-3.19.2-linux-x64.tar.gz");
        assert_eq!((server.kind, server.strip, server.program.as_str()), (Kind::TarGz, 0, "bin/lua-language-server"), "the archive is the folder");
        let stylua = linux.stylua().unwrap();
        assert_eq!(stylua.url, "https://github.com/JohnnyMorganz/StyLua/releases/download/v2.6.0/stylua-linux-x86_64.zip");
        assert_eq!((stylua.kind, stylua.program.as_str()), (Kind::Zip, "stylua"));

        let windows = Platform::new("windows", "x86_64");
        let names: Vec<(String, String, Kind)> = windows.installs().into_iter().map(|i| (i.name, i.program, i.kind)).collect();
        assert_eq!(
            names,
            [
                ("lua-language-server".into(), "bin/lua-language-server.exe".into(), Kind::Zip),
                ("stylua".into(), "stylua.exe".into(), Kind::Zip),
                ("selene".into(), "selene.exe".into(), Kind::Zip),
            ]
        );
        for i in Platform::new("macos", "arm64").installs() {
            assert_eq!(i.sha256.as_deref().map(str::len), Some(64), "{}", i.name);
        }
        assert_eq!(Platform::new("macos", "aarch64").selene().map(|i| i.url).as_deref(), Some("https://github.com/Kampfkarren/selene/releases/download/0.31.0/selene-0.31.0-macos.zip"));
        assert_eq!(Platform::new("macos", "x86_64").installs().len(), 2, "selene's macOS release is for arm64");
        assert_eq!(Platform::new("linux", "aarch64").installs().len(), 2);
        assert!(Platform::new("windows", "aarch64").installs().is_empty() && Platform::default().installs().is_empty());
    }

    #[test]
    fn rocks_are_installed_into_your_own_tree() {
        let t = Tools { luarocks: Some("/usr/bin/luarocks".into()), ..Tools::default() };
        assert_eq!(t.install_rock("busted").map(|c| line(&c)).as_deref(), Some("/usr/bin/luarocks install --local busted"));
        assert!(Tools::default().install_rock("busted").is_none(), "no LuaRocks");
        assert_eq!(hints(SERVER)[0], "/tools/lua-language-server/bin");
        assert_eq!(hints("stylua")[0], "/tools/stylua");
        assert!(hints("luacheck").contains(&"$HOME/.luarocks/bin".to_string()));
        assert_eq!(line(&Tools { luajit: Some("/usr/bin/luajit".into()), ..Tools::default() }.interpreter()), "/usr/bin/luajit");
        assert_eq!(line(&Tools::default().interpreter()), "lua");
    }
}
