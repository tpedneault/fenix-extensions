//! The programs the pack runs, found on this machine: Terraform and
//! terraform-ls, and the release of terraform-ls it installs into its
//! tools folder when there isn't one. Terraform itself is HashiCorp's to
//! install (a package manager, `tfenv`, or a download), so the pack looks
//! where those put it, not only on `PATH`: an editor started from the
//! desktop may not have the folder.

use fenix_extension_api::Command;
use fenix_lang_kit::archive::Kind;
use fenix_lang_kit::{Host, Install};

/// Where else to look: Homebrew's, the user's own bin folders, tfenv's
/// and tenv's shims, winget's links and Chocolatey's.
pub const HINTS: &[&str] = &[
    "$HOME/.local/bin", "$HOME/bin", "$HOME/.tfenv/bin", "$HOME/.tenv/bin", "/opt/homebrew/bin", "/usr/local/bin", "/snap/bin",
    "%USERPROFILE%\\bin", "%LOCALAPPDATA%\\Microsoft\\WinGet\\Links", "%ProgramData%\\chocolatey\\bin",
];

/// Where `name` is looked for: its own folder under the tools folder
/// first, where the pack installs it, then [`HINTS`].
pub fn hints(name: &str) -> Vec<String> {
    std::iter::once(format!("/tools/{name}")).chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// terraform-ls's release, and each file's SHA-256, by OS and
/// architecture as `platform()` says them. HashiCorp's own releases: the
/// `terraform-ls_<version>_SHA256SUMS` file beside them, signed with their
/// key, holds the same sums.
pub const LS: &str = "0.39.1";
const LS_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "linux_amd64:fe2a158e4a1a8ec8047dae9e06c439b468412aa579929ba28b2e1258cedef9c0"),
    ("linux", "aarch64", "linux_arm64:a72a6d9f72580cfc30cc06d3c973fd5f55790f62e3ddb195998435070d9af36d"),
    ("macos", "x86_64", "darwin_amd64:25814afdbec9163de4ad8db58960195b9fc65ddc5405a5df7163bbdbd08b5325"),
    ("macos", "aarch64", "darwin_arm64:07748aa989fc3b9f6e4538ff83bc2e607ebc20c7587ce6556ae555d631e70b1b"),
    ("windows", "x86_64", "windows_amd64:bb8523b2e109c2ad719dbf6245c4be035386e322cb44e8f01c74b1caa4680efb"),
    ("windows", "aarch64", "windows_arm64:0b6fe711414f29eeaaa09509fcb073e1f3269705e2de87a8b8ce7e77bc43fb6c"),
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

    /// `name` as a program here: `terraform-ls.exe` on Windows.
    fn exe(&self, name: &str) -> String {
        if self.os == "windows" {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }

    /// terraform-ls's release for this machine, into `/tools/terraform-ls`:
    /// the program at the zip's top, beside its licence.
    pub fn terraform_ls(&self) -> Option<Install> {
        let arch = if self.arch == "arm64" { "aarch64" } else { self.arch.as_str() };
        let (_, _, file) = LS_FILES.iter().find(|(os, a, _)| *os == self.os && *a == arch)?;
        let (target, sha) = file.split_once(':')?;
        let url = format!("https://releases.hashicorp.com/terraform-ls/{LS}/terraform-ls_{LS}_{target}.zip");
        Some(Install::new("terraform-ls", &url).label(&format!("terraform-ls {LS}")).sha256(sha).kind(Kind::Zip).program(&self.exe("terraform-ls")))
    }

    /// Every release there's one of for this machine.
    pub fn installs(&self) -> Vec<Install> {
        self.terraform_ls().into_iter().collect()
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub terraform: Option<String>,
    pub terraform_ls: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let mut find = |name: &str| host.find_program(name, &hints(name));
        Tools { terraform: find("terraform"), terraform_ls: find("terraform-ls") }
    }

    /// Terraform by its path when found, else by its name.
    pub fn terraform(&self) -> Command {
        Command::new(self.terraform.as_deref().unwrap_or("terraform"))
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        match name {
            "terraform" => Some(&mut self.terraform),
            "terraform-ls" => Some(&mut self.terraform_ls),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_is_installed_from_the_release_for_this_machine() {
        let linux = Platform::new("linux", "x86_64").terraform_ls().unwrap();
        assert_eq!(linux.url, "https://releases.hashicorp.com/terraform-ls/0.39.1/terraform-ls_0.39.1_linux_amd64.zip");
        assert_eq!((linux.kind, linux.strip, linux.program.as_str()), (Kind::Zip, 0, "terraform-ls"), "no folder at the zip's top");
        assert_eq!(linux.sha256.as_deref(), Some("fe2a158e4a1a8ec8047dae9e06c439b468412aa579929ba28b2e1258cedef9c0"));
        assert_eq!(linux.label, "terraform-ls 0.39.1");

        let mac = Platform::new("macos", "arm64").terraform_ls().unwrap();
        assert!(mac.url.ends_with("terraform-ls_0.39.1_darwin_arm64.zip"), "{}", mac.url);
        let windows = Platform::new("windows", "x86_64").terraform_ls().unwrap();
        assert_eq!((windows.program.as_str(), windows.url.ends_with("_windows_amd64.zip")), ("terraform-ls.exe", true));
        for (os, arch) in [("linux", "x86_64"), ("linux", "aarch64"), ("macos", "x86_64"), ("macos", "aarch64"), ("windows", "x86_64"), ("windows", "aarch64")] {
            let i = Platform::new(os, arch).terraform_ls().unwrap_or_else(|| panic!("{os} {arch}"));
            assert_eq!(i.sha256.as_deref().map(str::len), Some(64), "{os} {arch}");
            assert!(i.url.starts_with("https://releases.hashicorp.com/terraform-ls/"), "{}", i.url);
        }
        assert!(Platform::new("linux", "riscv64").installs().is_empty() && Platform::default().installs().is_empty());
    }

    #[test]
    fn terraform_is_looked_for_where_the_managers_put_it() {
        assert_eq!(hints("terraform-ls")[0], "/tools/terraform-ls");
        assert!(hints("terraform").contains(&"$HOME/.tfenv/bin".to_string()));
        assert_eq!(Tools::default().terraform().program, "terraform");
        let mut t = Tools::default();
        *t.slot("terraform-ls").unwrap() = Some("/t/terraform-ls".into());
        assert_eq!(t.terraform_ls.as_deref(), Some("/t/terraform-ls"));
        assert!(t.slot("go").is_none());
    }
}
