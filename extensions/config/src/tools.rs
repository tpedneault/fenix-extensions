//! The programs the pack runs, found on this machine: Node's npm and the
//! servers it installs (yaml-language-server, vscode-json-language-server,
//! docker-langserver, bash-language-server), Prettier, and the single
//! programs the pack installs into its tools folder from their releases
//! when they aren't here already (Taplo, Marksman, shfmt, ShellCheck,
//! hadolint). `npm install -g`, `cargo install`, `go install`, Homebrew
//! and winget put programs in folders an editor started from the desktop
//! may not have on its `PATH`, so they're looked in too.

use fenix_extension_api::Command;
use fenix_lang_kit::{Host, Install};

/// Where else to look: npm's global folders and the other managers',
/// Cargo's and Go's, Homebrew's, winget's links.
pub const HINTS: &[&str] = &[
    "$HOME/.npm-global/bin", "$HOME/.local/share/pnpm", "$HOME/Library/pnpm", "$HOME/.bun/bin", "$HOME/.volta/bin",
    "$HOME/.local/bin", "$HOME/.cargo/bin", "$HOME/go/bin", "/opt/homebrew/bin", "/usr/local/bin",
    "%APPDATA%\\npm", "%LOCALAPPDATA%\\pnpm", "%LOCALAPPDATA%\\Volta\\bin", "%ProgramFiles%\\nodejs",
    "%USERPROFILE%\\.cargo\\bin", "%USERPROFILE%\\go\\bin", "%LOCALAPPDATA%\\Microsoft\\WinGet\\Links",
];

/// Where `name` is looked for: its own folder under the tools folder
/// first, where the pack installs it, then [`HINTS`].
pub fn hints(name: &str) -> Vec<String> {
    std::iter::once(format!("/tools/{name}")).chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// The language servers, by their programs.
pub const YAML_SERVER: &str = "yaml-language-server";
pub const JSON_SERVER: &str = "vscode-json-language-server";
pub const DOCKER_SERVER: &str = "docker-langserver";
pub const BASH_SERVER: &str = "bash-language-server";

/// What npm installs each server from.
pub const YAML_PACKAGE: &str = "yaml-language-server";
pub const JSON_PACKAGE: &str = "vscode-langservers-extracted";
pub const DOCKER_PACKAGE: &str = "dockerfile-language-server-nodejs";
pub const BASH_PACKAGE: &str = "bash-language-server";

/// The releases the pack installs, and each file's SHA-256, by OS and
/// architecture as `platform()` says them.
pub const TAPLO: &str = "0.10.0";
const TAPLO_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "taplo-linux-x86_64.gz:8fe196b894ccf9072f98d4e1013a180306e17d244830b03986ee5e8eabeb6156"),
    ("linux", "aarch64", "taplo-linux-aarch64.gz:033681d01eec8376c3fd38fa3703c79316f5e14bb013d859943b60a07bccdcc3"),
    ("macos", "x86_64", "taplo-darwin-x86_64.gz:898122cde3a0b1cd1cbc2d52d3624f23338218c91b5ddb71518236a4c2c10ef2"),
    ("macos", "aarch64", "taplo-darwin-aarch64.gz:713734314c3e71894b9e77513c5349835eefbd52908445a0d73b0c7dc469347d"),
    ("windows", "x86_64", "taplo-windows-x86_64.zip:1615eed140039bd58e7089109883b1c434de5d6de8f64a993e6e8c80ca57bdf9"),
];

pub const MARKSMAN: &str = "2026-02-08";
const MARKSMAN_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "marksman-linux-x64:be5098e8213219269c47fc0d916a66fa31ce0602ec967475c722260aabf26087"),
    ("linux", "aarch64", "marksman-linux-arm64:db8e124527f7f8048e3e6c91821b9c52ef173d92c01e47d221bf1337afd962fb"),
    // One for both: a universal binary.
    ("macos", "x86_64", "marksman-macos:6a801c17b5ac0dba69787c5282b3b3bd416e66c96253fae098d311c6bbd1833b"),
    ("macos", "aarch64", "marksman-macos:6a801c17b5ac0dba69787c5282b3b3bd416e66c96253fae098d311c6bbd1833b"),
    ("windows", "x86_64", "marksman.exe:a6d05beb08ebe41b0a9f09c98a438540421436fa5531424c22e0bb1d22529705"),
];

pub const SHFMT: &str = "3.14.1";
const SHFMT_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "shfmt_v3.14.1_linux_amd64:76e77641faa025814b77f153b29796b8e6fa2fca03e0c76a691608b86c7ea7bf"),
    ("linux", "aarch64", "shfmt_v3.14.1_linux_arm64:5f2db09dae91fca848f7adbdd014632e921a383863a2ad7e0450ad3aba0c6489"),
    ("macos", "x86_64", "shfmt_v3.14.1_darwin_amd64:d33eee0da0f92835b3562e9767a05cee7e4eaeef47daa03bfd09da17b4b590a6"),
    ("macos", "aarch64", "shfmt_v3.14.1_darwin_arm64:b7c872db63553ccffc7253aba3ed7d4885a27d83f1ba567b1138c6315a5847e5"),
    ("windows", "x86_64", "shfmt_v3.14.1_windows_amd64.exe:13629ce28442ca80b6b5a819f7574ab39e1c28c6e26734ca816c9714e04851df"),
];

pub const SHELLCHECK: &str = "0.11.0";
const SHELLCHECK_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "shellcheck-v0.11.0.linux.x86_64.tar.xz:8c3be12b05d5c177a04c29e3c78ce89ac86f1595681cab149b65b97c4e227198"),
    ("linux", "aarch64", "shellcheck-v0.11.0.linux.aarch64.tar.xz:12b331c1d2db6b9eb13cfca64306b1b157a86eb69db83023e261eaa7e7c14588"),
    ("macos", "x86_64", "shellcheck-v0.11.0.darwin.x86_64.tar.xz:3c89db4edcab7cf1c27bff178882e0f6f27f7afdf54e859fa041fca10febe4c6"),
    ("macos", "aarch64", "shellcheck-v0.11.0.darwin.aarch64.tar.xz:56affdd8de5527894dca6dc3d7e0a99a873b0f004d7aabc30ae407d3f48b0a79"),
    ("windows", "x86_64", "shellcheck-v0.11.0.zip:8a4e35ab0b331c85d73567b12f2a444df187f483e5079ceffa6bda1faa2e740e"),
];

pub const HADOLINT: &str = "2.15.1";
const HADOLINT_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "hadolint-linux-x86_64:c7187db94eeeeca956519a6af171adc31453941a1e777961f6e680f697c8c507"),
    ("linux", "aarch64", "hadolint-linux-arm64:f6198ef8090f404dbb771abfee086eb8c48ac177f30da7fd3510aca35b344b5d"),
    ("macos", "x86_64", "hadolint-macos-x86_64:ffe9bb18b23d5ed1eae50237aecdbb523d016e96da0bd4e7aa432040acfc3fde"),
    ("macos", "aarch64", "hadolint-macos-arm64:5c09f3213f8e40406abe048233d985eebef336d4a6a20021be47fadb6cf480a2"),
    ("windows", "x86_64", "hadolint-windows-x86_64.exe:01d927294962b5387f9ead4f18679158452be4f17c765ad0bdffe5264b9c7b0a"),
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

    /// `name` as a program here: `taplo.exe` on Windows.
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

    /// Taplo's release for this machine, into `/tools/taplo`: a lone
    /// program gzipped, or zipped on Windows.
    pub fn taplo(&self) -> Option<Install> {
        let (file, sha) = self.file(TAPLO_FILES)?;
        let url = format!("https://github.com/tamasfe/taplo/releases/download/{TAPLO}/{file}");
        Some(Install::new("taplo", &url).label(&format!("Taplo {TAPLO}")).sha256(sha).program(&self.exe("taplo")))
    }

    /// Marksman's release: the program itself.
    pub fn marksman(&self) -> Option<Install> {
        let (file, sha) = self.file(MARKSMAN_FILES)?;
        let url = format!("https://github.com/artempyanykh/marksman/releases/download/{MARKSMAN}/{file}");
        Some(Install::new("marksman", &url).label(&format!("Marksman {MARKSMAN}")).sha256(sha).program(&self.exe("marksman")))
    }

    /// shfmt's release: the program itself.
    pub fn shfmt(&self) -> Option<Install> {
        let (file, sha) = self.file(SHFMT_FILES)?;
        let url = format!("https://github.com/mvdan/sh/releases/download/v{SHFMT}/{file}");
        Some(Install::new("shfmt", &url).label(&format!("shfmt {SHFMT}")).sha256(sha).program(&self.exe("shfmt")))
    }

    /// ShellCheck's release: a folder in a tar.xz, or a zip with the
    /// program at its top on Windows.
    pub fn shellcheck(&self) -> Option<Install> {
        let (file, sha) = self.file(SHELLCHECK_FILES)?;
        let url = format!("https://github.com/koalaman/shellcheck/releases/download/v{SHELLCHECK}/{file}");
        let strip = if self.os == "windows" { 0 } else { 1 };
        Some(Install::new("shellcheck", &url).label(&format!("ShellCheck {SHELLCHECK}")).sha256(sha).strip(strip).program(&self.exe("shellcheck")))
    }

    /// hadolint's release: the program itself.
    pub fn hadolint(&self) -> Option<Install> {
        let (file, sha) = self.file(HADOLINT_FILES)?;
        let url = format!("https://github.com/hadolint/hadolint/releases/download/v{HADOLINT}/{file}");
        Some(Install::new("hadolint", &url).label(&format!("hadolint {HADOLINT}")).sha256(sha).program(&self.exe("hadolint")))
    }

    /// Every release there's one of for this machine.
    pub fn installs(&self) -> Vec<Install> {
        [self.taplo(), self.marksman(), self.shfmt(), self.shellcheck(), self.hadolint()].into_iter().flatten().collect()
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub npm: Option<String>,
    pub yaml: Option<String>,
    pub json: Option<String>,
    pub docker: Option<String>,
    pub bash: Option<String>,
    pub taplo: Option<String>,
    pub marksman: Option<String>,
    pub shfmt: Option<String>,
    pub shellcheck: Option<String>,
    pub hadolint: Option<String>,
    pub prettier: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let mut find = |name: &str| host.find_program(name, &hints(name));
        Tools {
            npm: find("npm"),
            yaml: find(YAML_SERVER),
            json: find(JSON_SERVER),
            docker: find(DOCKER_SERVER),
            bash: find(BASH_SERVER),
            taplo: find("taplo"),
            marksman: find("marksman"),
            shfmt: find("shfmt"),
            shellcheck: find("shellcheck"),
            hadolint: find("hadolint"),
            prettier: find("prettier"),
        }
    }

    /// Installing npm packages globally; none without npm.
    pub fn install_global(&self, packages: &[&str]) -> Option<Command> {
        self.npm.as_ref().map(|npm| Command::new(npm).args(["install", "-g"]).args(packages.iter().copied()))
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        Some(match name {
            "npm" => &mut self.npm,
            YAML_SERVER => &mut self.yaml,
            JSON_SERVER => &mut self.json,
            DOCKER_SERVER => &mut self.docker,
            BASH_SERVER => &mut self.bash,
            "taplo" => &mut self.taplo,
            "marksman" => &mut self.marksman,
            "shfmt" => &mut self.shfmt,
            "shellcheck" => &mut self.shellcheck,
            "hadolint" => &mut self.hadolint,
            "prettier" => &mut self.prettier,
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
        let taplo = linux.taplo().unwrap();
        assert_eq!(taplo.url, "https://github.com/tamasfe/taplo/releases/download/0.10.0/taplo-linux-x86_64.gz");
        assert_eq!((taplo.kind, taplo.program.as_str()), (Kind::Gz, "taplo"));
        let shellcheck = linux.shellcheck().unwrap();
        assert_eq!((shellcheck.kind, shellcheck.strip, shellcheck.program.as_str()), (Kind::TarXz, 1, "shellcheck"));
        let marksman = linux.marksman().unwrap();
        assert_eq!((marksman.kind, marksman.program.as_str()), (Kind::Plain, "marksman"), "written as marksman, not marksman-linux-x64");

        let windows = Platform::new("windows", "x86_64");
        let names: Vec<(String, String, Kind)> = windows.installs().into_iter().map(|i| (i.name, i.program, i.kind)).collect();
        assert_eq!(
            names,
            [
                ("taplo".into(), "taplo.exe".into(), Kind::Zip),
                ("marksman".into(), "marksman.exe".into(), Kind::Plain),
                ("shfmt".into(), "shfmt.exe".into(), Kind::Plain),
                ("shellcheck".into(), "shellcheck.exe".into(), Kind::Zip),
                ("hadolint".into(), "hadolint.exe".into(), Kind::Plain),
            ]
        );
        assert_eq!(windows.shellcheck().unwrap().strip, 0, "the zip has the program at its top");
        for i in Platform::new("macos", "arm64").installs() {
            assert_eq!(i.sha256.as_deref().map(str::len), Some(64), "{}", i.name);
        }
        assert_eq!(Platform::new("macos", "aarch64").installs().len(), 5);
        assert!(Platform::new("windows", "aarch64").installs().is_empty() && Platform::default().installs().is_empty());
    }

    #[test]
    fn the_node_servers_are_installed_with_npm() {
        let t = Tools { npm: Some("/n/npm".into()), ..Tools::default() };
        assert_eq!(t.install_global(&[YAML_PACKAGE]).map(|c| line(&c)).as_deref(), Some("/n/npm install -g yaml-language-server"));
        assert!(Tools::default().install_global(&[JSON_PACKAGE]).is_none(), "no npm");
        assert_eq!(hints("taplo")[0], "/tools/taplo");
        assert!(hints("shfmt").contains(&"$HOME/go/bin".to_string()));
    }
}
