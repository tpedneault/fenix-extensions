//! Harper, the grammar and spelling server (`harper-ls`), found on this
//! machine and, when it isn't here, the release the pack installs into its
//! tools folder: pinned, one file for each OS and architecture, each held
//! to its SHA-256.
//!
//! The checksums are of the assets as they were downloaded from the
//! release on 13 November 2026: Harper's release publishes none beside
//! them (no `.sha256`, no `SHA256SUMS`), so these are what the file was
//! then, and Fenix refuses a download that isn't. See the README.

use fenix_extension_api::Command;
use fenix_lang_kit::{Host, Install};

/// What the server is called: its program, its folder in the tools folder
/// and, as `project::add_language_server` has it, its name.
pub const PROGRAM: &str = "harper-ls";
pub const SERVER: &str = "harper";

/// Where else to look: Cargo's, Homebrew's, the user's own bin folders,
/// Scoop's shims.
pub const HINTS: &[&str] = &["$HOME/.cargo/bin", "$HOME/.local/bin", "/opt/homebrew/bin", "/usr/local/bin", "%USERPROFILE%\\.cargo\\bin", "%USERPROFILE%\\scoop\\shims"];

/// Where Harper is looked for: its own folder under the tools folder
/// first, where the pack installs it, then [`HINTS`].
pub fn hints() -> Vec<String> {
    std::iter::once(format!("/tools/{SERVER}")).chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// The release, and each file's SHA-256, by OS and architecture as
/// `platform()` says them. Linux on x86-64 is the static (musl) build, so
/// it runs on any distribution whatever its C library; every archive holds
/// the one program at its top.
pub const HARPER: &str = "3.2.0";
const FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "harper-ls-x86_64-unknown-linux-musl.tar.gz:4b44c54ff6a0831901805584bb9d70a004b768b713af9725b65d13e1b369c138"),
    ("linux", "aarch64", "harper-ls-aarch64-unknown-linux-gnu.tar.gz:e2a13cd9bb91b505ef46bcbcafeb4f11b96d95e75b9cc0a156c680474c476804"),
    ("macos", "x86_64", "harper-ls-x86_64-apple-darwin.tar.gz:3ca19166a4ffef14c24b0396af920bbb11abd071e6724bfedbe1f2eccad3b76d"),
    ("macos", "aarch64", "harper-ls-aarch64-apple-darwin.tar.gz:9dd31b0cabfb040f658a8d595cc36520eb306a5a69ac37c7b828c36574b47ce9"),
    ("windows", "x86_64", "harper-ls-x86_64-pc-windows-msvc.zip:a9e370166ace4e92a592f954baa2fedf4528e6daf59e64a9973fc9f3d7d30d5e"),
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

    /// The program here: `harper-ls.exe` on Windows.
    pub fn exe(&self) -> String {
        if self.os == "windows" {
            format!("{PROGRAM}.exe")
        } else {
            PROGRAM.to_string()
        }
    }

    /// The file and SHA-256 for this machine.
    fn file(&self) -> Option<(&'static str, &'static str)> {
        let arch = if self.arch == "arm64" { "aarch64" } else { self.arch.as_str() };
        let (_, _, file) = FILES.iter().find(|(os, a, _)| *os == self.os && *a == arch)?;
        file.rsplit_once(':')
    }

    /// Harper's release for this machine, into `/tools/harper`; none for a
    /// machine it has no build for.
    pub fn install(&self) -> Option<Install> {
        let (file, sha) = self.file()?;
        let url = format!("https://github.com/Automattic/harper/releases/download/v{HARPER}/{file}");
        Some(Install::new(SERVER, &url).label(&format!("Harper {HARPER}")).sha256(sha).program(&self.exe()))
    }
}

/// `harper-ls` on stdio, by its path.
pub fn server(path: &str) -> Command {
    Command::new(path).arg("--stdio")
}

/// Harper, found: its full path.
pub fn find(host: &mut dyn Host) -> Option<String> {
    host.find_program(PROGRAM, &hints())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_lang_kit::archive::Kind;

    #[test]
    fn harper_is_installed_from_the_release_for_this_machine() {
        let linux = Platform::new("linux", "x86_64").install().unwrap();
        assert_eq!(linux.url, "https://github.com/Automattic/harper/releases/download/v3.2.0/harper-ls-x86_64-unknown-linux-musl.tar.gz");
        assert_eq!((linux.name.as_str(), linux.kind, linux.strip, linux.program.as_str()), ("harper", Kind::TarGz, 0, "harper-ls"));
        assert_eq!(linux.sha256.as_deref(), Some("4b44c54ff6a0831901805584bb9d70a004b768b713af9725b65d13e1b369c138"));

        let arm = Platform::new("linux", "arm64").install().unwrap();
        assert!(arm.url.ends_with("harper-ls-aarch64-unknown-linux-gnu.tar.gz"), "{}", arm.url);

        let mac = Platform::new("macos", "aarch64").install().unwrap();
        assert!(mac.url.ends_with("harper-ls-aarch64-apple-darwin.tar.gz"));
        assert!(Platform::new("macos", "x86_64").install().unwrap().url.ends_with("harper-ls-x86_64-apple-darwin.tar.gz"));

        let windows = Platform::new("windows", "x86_64").install().unwrap();
        assert_eq!((windows.kind, windows.program.as_str()), (Kind::Zip, "harper-ls.exe"));
        assert!(windows.url.ends_with("harper-ls-x86_64-pc-windows-msvc.zip"));
    }

    #[test]
    fn every_platform_has_a_full_checksum_and_a_machine_without_a_build_none() {
        for (os, arch) in [("linux", "x86_64"), ("linux", "aarch64"), ("macos", "x86_64"), ("macos", "aarch64"), ("windows", "x86_64")] {
            let i = Platform::new(os, arch).install().unwrap_or_else(|| panic!("{os} {arch}"));
            let sha = i.sha256.unwrap();
            assert!(sha.len() == 64 && sha.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{os} {arch}: {sha}");
            assert!(i.url.starts_with("https://github.com/Automattic/harper/releases/download/v"), "{}", i.url);
        }
        for (os, arch) in [("windows", "aarch64"), ("linux", "riscv64"), ("freebsd", "x86_64"), ("", "")] {
            assert!(Platform::new(os, arch).install().is_none(), "{os} {arch}");
        }
    }

    #[test]
    fn harper_is_looked_for_where_the_pack_puts_it_first() {
        assert_eq!(hints()[0], "/tools/harper");
        assert!(hints().contains(&"$HOME/.cargo/bin".to_string()));
        assert_eq!(server("/t/harper-ls").args, ["--stdio"]);
    }
}
