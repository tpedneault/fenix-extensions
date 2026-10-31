//! The programs the pack runs, found on this machine: the `dotnet` CLI,
//! csharp-ls, netcoredbg and CSharpier. `dotnet tool install -g` puts
//! tools in `~/.dotnet/tools`, which an editor started from the desktop
//! may not have on its `PATH`, so it's looked in too; netcoredbg, which
//! isn't a .NET tool, is installed into the tools folder from Samsung's
//! release for this machine.

use fenix_extension_api::Command;
use fenix_lang_kit::{Host, Install, Version};

/// Where else to look: .NET's global tools, and where the SDK goes.
pub const HINTS: &[&str] = &["$HOME/.dotnet/tools", "%USERPROFILE%\\.dotnet\\tools", "$HOME/.dotnet", "/usr/local/share/dotnet", "/usr/share/dotnet", "/usr/lib/dotnet", "%ProgramFiles%\\dotnet"];

pub fn hints() -> Vec<String> {
    HINTS.iter().map(|h| h.to_string()).collect()
}

/// netcoredbg's release, and each file's SHA-256.
pub const NETCOREDBG: &str = "3.1.2-1054";
const NETCOREDBG_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "netcoredbg-linux-amd64.tar.gz:4f58ae90aacbccdf85dc7d6a0d8893796dd3750d6867fe31396a6b7bee09c9cb"),
    ("linux", "aarch64", "netcoredbg-linux-arm64.tar.gz:4dc745eecf694f2151ecf1635229a0cc97e9f00868b14916aea7785b0184cc05"),
    // Intel Macs only: the release has no Apple silicon build, and the
    // Intel one can't debug arm64 programs.
    ("macos", "x86_64", "netcoredbg-osx-amd64.tar.gz:a958c50be5fd3bec5804657fef49935cd819830e700797044519594fee17a1a1"),
    ("windows", "x86_64", "netcoredbg-win64.zip:09b4385fd556014a8a96df7a368d4937f4fc9e06f031365d41a288aabd2d78f9"),
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

    /// netcoredbg's release for it, into `/tools/netcoredbg`: none where
    /// there isn't one.
    pub fn netcoredbg(&self) -> Option<Install> {
        let (_, _, file) = NETCOREDBG_FILES.iter().find(|(os, arch, _)| *os == self.os && *arch == self.arch)?;
        let (name, sha) = file.split_once(':')?;
        let program = if self.os == "windows" { "netcoredbg.exe" } else { "netcoredbg" };
        Some(Install::new("netcoredbg", &format!("https://github.com/Samsung/netcoredbg/releases/download/{NETCOREDBG}/{name}")).label("netcoredbg").sha256(sha).strip(1).program(program))
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub dotnet: Option<String>,
    pub csharp_ls: Option<String>,
    pub netcoredbg: Option<String>,
    pub csharpier: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let hints = hints();
        let mut find = |name: &str| host.find_program(name, &hints);
        Tools { dotnet: find("dotnet"), csharp_ls: find("csharp-ls"), netcoredbg: find("netcoredbg"), csharpier: find("csharpier") }
    }

    /// `dotnet`, by its path when found.
    pub fn dotnet(&self) -> Command {
        Command::new(self.dotnet.as_deref().unwrap_or("dotnet"))
    }

    /// Installing (or bringing to the version given) a global .NET tool.
    /// `update` installs one that isn't there too.
    pub fn install_tool(&self, package: &str, version: Option<&str>) -> Command {
        let cmd = self.dotnet().args(["tool", "update", "-g", package]);
        match version {
            Some(v) => cmd.args(["--version", v]),
            None => cmd,
        }
    }
}

/// The newest csharp-ls an SDK can run: 0.21 and later are built for .NET
/// 10, 0.17 to 0.20 for .NET 9, 0.16 for .NET 8; `None` for the newest.
pub fn csharp_ls_for(sdk: Option<&Version>) -> Option<&'static str> {
    match sdk.map(Version::major) {
        Some(8) => Some("0.16.0"),
        Some(9) => Some("0.20.0"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    #[test]
    fn netcoredbg_is_installed_from_the_release_for_this_machine() {
        let linux = Platform::new("linux", "x86_64").netcoredbg().unwrap();
        assert_eq!(linux.url, "https://github.com/Samsung/netcoredbg/releases/download/3.1.2-1054/netcoredbg-linux-amd64.tar.gz");
        assert_eq!((linux.strip, linux.program.as_str()), (1, "netcoredbg"));
        assert_eq!(linux.sha256.as_deref().map(str::len), Some(64));
        let windows = Platform::new("windows", "x86_64").netcoredbg().unwrap();
        assert!(windows.url.ends_with("netcoredbg-win64.zip") && windows.program == "netcoredbg.exe");
        assert_eq!(Platform::new("macos", "aarch64").netcoredbg(), None, "no build for Apple silicon");
        assert_eq!(Platform::default().netcoredbg(), None);
    }

    #[test]
    fn tools_are_installed_with_dotnet_tool_for_the_sdk() {
        let t = Tools { dotnet: Some("/usr/bin/dotnet".into()), ..Tools::default() };
        assert_eq!(line(&t.install_tool("csharp-ls", csharp_ls_for(Version::parse("8.0.132").as_ref()))), "/usr/bin/dotnet tool update -g csharp-ls --version 0.16.0");
        assert_eq!(csharp_ls_for(Version::parse("9.0.306").as_ref()), Some("0.20.0"));
        assert_eq!(line(&Tools::default().install_tool("csharp-ls", csharp_ls_for(Version::parse("10.0.100").as_ref()))), "dotnet tool update -g csharp-ls");
        assert_eq!(csharp_ls_for(None), None);
    }
}
