//! The programs the pack runs, found on this machine: Zig and ZLS, and
//! the releases of each it installs into its tools folder when they
//! aren't here already. Zig's version managers (zvm, zigup, anyzig),
//! Homebrew, Snap and winget put them in folders an editor started from
//! the desktop may not have on its `PATH`, so they're looked in too.

use fenix_extension_api::Command;
use fenix_lang_kit::archive::Kind;
use fenix_lang_kit::{Host, Install};

/// Where else to look: zvm's, zigup's and the user's own bin folders,
/// Homebrew's, Snap's, winget's links.
pub const HINTS: &[&str] = &[
    "$HOME/.zvm/bin", "$HOME/.zvm/self", "$HOME/.local/bin", "$HOME/bin", "/opt/homebrew/bin", "/usr/local/bin", "/snap/bin",
    "%USERPROFILE%\\.zvm\\bin", "%USERPROFILE%\\bin", "%LOCALAPPDATA%\\Microsoft\\WinGet\\Links",
];

/// Where `name` is looked for: its own folder under the tools folder
/// first, where the pack installs it, then [`HINTS`].
pub fn hints(name: &str) -> Vec<String> {
    std::iter::once(format!("/tools/{name}")).chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// ZLS's release, the one for Zig 0.17, and each file's SHA-256, by OS
/// and architecture as `platform()` says them.
pub const ZLS: &str = "0.17.0";
const ZLS_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "zls-x86_64-linux.tar.xz:dfa4514e1868803fdf2ae56832c88b9d400094d70ea6bb136b57bcc91af10258"),
    ("linux", "aarch64", "zls-aarch64-linux.tar.xz:0d314f3cab30da770328e1b8c84ab89bb308e56dbc26a85ac82429ff02b44873"),
    ("macos", "x86_64", "zls-x86_64-macos.tar.xz:0801481ebf0027620db6729968c069401c7b80f376d35f9f91f480d72c393523"),
    ("macos", "aarch64", "zls-aarch64-macos.tar.xz:2e451358b4f7e2d4b1fccb65db177dbb321c5a98e95f67e09aa72d6ecfb39d20"),
    ("windows", "x86_64", "zls-x86_64-windows.zip:b0e557b13032322f530a818ea4eb899a51a2e738808397a8cf3868fd56b73972"),
    ("windows", "aarch64", "zls-aarch64-windows.zip:98869d1d2e9532044a3bbb25be16a932041bdc2f422fe074cc499fd20d1b6f53"),
];

/// Zig itself: the wheels Zig's own project puts on PyPI (`pip install
/// ziglang`), each a zip holding `ziglang/zig` and its `lib`. There's none
/// for Windows at 0.17.0.
pub const ZIG: &str = "0.17.0";
const ZIG_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "fe/6f/e1c380fe61db36cb464d5d4d18c557338812500986885373e2550d4ebf6b/ziglang-0.17.0-py3-none-manylinux_2_12_x86_64.manylinux2010_x86_64.musllinux_1_1_x86_64.whl:97079827064a7492cce5541ceb438ae8003ce1e54e399676da2d62fb7a5dc08b"),
    ("linux", "aarch64", "02/21/abee2f0524f54fe3e8783748b9df79bb07d4b955e6b465677d1cfbda335c/ziglang-0.17.0-py3-none-manylinux_2_17_aarch64.manylinux2014_aarch64.musllinux_1_1_aarch64.whl:d2804f94f54f7fcf8169f4b212a619ad688a4e2e6592e976f0cf2e58e2cd8af3"),
    ("macos", "x86_64", "e2/52/4f8a8f157d994482e501bb37e01f4883990b9848b9d8b809fe95e97aac6d/ziglang-0.17.0-py3-none-macosx_13_0_x86_64.whl:4d910f0e363ddb60a6b8ddb55235617797a8ccbcb6c5c7132a949d9f833eea42"),
    ("macos", "aarch64", "37/fa/54dd03b21fa790a85e1de39eeda9703386fe519490598abdb38894a1c3b3/ziglang-0.17.0-py3-none-macosx_13_0_arm64.whl:87274af689c4128051b26c80f6f676757e0a81c2fdc137fd8b5679e928e4bc4f"),
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

    /// `name` as a program here: `zls.exe` on Windows.
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
        file.rsplit_once(':')
    }

    /// ZLS's release for this machine, into `/tools/zls`: the program at
    /// the archive's top, beside its README and licence.
    pub fn zls(&self) -> Option<Install> {
        let (file, sha) = self.file(ZLS_FILES)?;
        let url = format!("https://github.com/zigtools/zls/releases/download/{ZLS}/{file}");
        Some(Install::new("zls", &url).label(&format!("ZLS {ZLS}")).sha256(sha).program(&self.exe("zls")))
    }

    /// Zig's wheel for this machine, into `/tools/zig`: `ziglang/` taken
    /// off, so `zig` and its `lib` are at the top.
    pub fn zig(&self) -> Option<Install> {
        let (path, sha) = self.file(ZIG_FILES)?;
        let url = format!("https://files.pythonhosted.org/packages/{path}");
        Some(Install::new("zig", &url).label(&format!("Zig {ZIG}")).sha256(sha).kind(Kind::Zip).strip(1).program(&self.exe("zig")))
    }

    /// Every release there's one of for this machine.
    pub fn installs(&self) -> Vec<Install> {
        [self.zig(), self.zls()].into_iter().flatten().collect()
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub zig: Option<String>,
    pub zls: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let mut find = |name: &str| host.find_program(name, &hints(name));
        Tools { zig: find("zig"), zls: find("zls") }
    }

    /// Zig by its path when found, else by its name.
    pub fn zig(&self) -> Command {
        Command::new(self.zig.as_deref().unwrap_or("zig"))
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        match name {
            "zig" => Some(&mut self.zig),
            "zls" => Some(&mut self.zls),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tool_is_installed_from_the_release_for_this_machine() {
        let linux = Platform::new("linux", "x86_64");
        let zls = linux.zls().unwrap();
        assert_eq!(zls.url, "https://github.com/zigtools/zls/releases/download/0.17.0/zls-x86_64-linux.tar.xz");
        assert_eq!((zls.kind, zls.strip, zls.program.as_str()), (Kind::TarXz, 0, "zls"), "no folder at the archive's top");
        let zig = linux.zig().unwrap();
        assert!(zig.url.starts_with("https://files.pythonhosted.org/packages/fe/6f/") && zig.url.ends_with("_x86_64.whl"), "{}", zig.url);
        assert_eq!((zig.kind, zig.strip, zig.program.as_str()), (Kind::Zip, 1, "zig"), "a wheel is a zip");
        assert_eq!(zig.sha256.as_deref(), Some("97079827064a7492cce5541ceb438ae8003ce1e54e399676da2d62fb7a5dc08b"));

        let mac = Platform::new("macos", "arm64");
        assert!(mac.zig().unwrap().url.ends_with("macosx_13_0_arm64.whl") && mac.zls().unwrap().url.ends_with("zls-aarch64-macos.tar.xz"));
        for i in mac.installs().into_iter().chain(Platform::new("linux", "aarch64").installs()) {
            assert_eq!(i.sha256.as_deref().map(str::len), Some(64), "{}", i.name);
        }

        let windows = Platform::new("windows", "x86_64");
        let names: Vec<(String, String, Kind)> = windows.installs().into_iter().map(|i| (i.name, i.program, i.kind)).collect();
        assert_eq!(names, [("zls".into(), "zls.exe".into(), Kind::Zip)], "no Zig wheel for Windows");
        assert!(Platform::new("linux", "riscv64").installs().is_empty() && Platform::default().installs().is_empty());
    }

    #[test]
    fn zig_is_looked_for_where_the_managers_put_it() {
        assert_eq!(hints("zig")[0], "/tools/zig");
        assert!(hints("zls").contains(&"$HOME/.zvm/bin".to_string()));
        assert_eq!(Tools::default().zig().program, "zig");
        let mut t = Tools::default();
        *t.slot("zls").unwrap() = Some("/t/zls".into());
        assert_eq!(t.zls.as_deref(), Some("/t/zls"));
        assert!(t.slot("go").is_none());
    }
}
