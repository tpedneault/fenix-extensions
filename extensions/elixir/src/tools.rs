//! The programs the pack runs, found on this machine: Mix and Elixir, and
//! Expert, the language server, with the release of it that the pack
//! installs into its tools folder when it isn't here already. Elixir
//! comes from a version manager as often as from the system (asdf, mise,
//! Homebrew), and these put it in folders an editor started from the
//! desktop may not have on its `PATH`, so they're looked in too.

use fenix_extension_api::Command;
use fenix_lang_kit::{Host, Install};

/// Where else to look: the version managers' shims, the user's own bin
/// folders, Homebrew's, the Windows installer's.
pub const HINTS: &[&str] = &[
    "$HOME/.asdf/shims", "$HOME/.local/share/mise/shims", "$HOME/.local/bin", "$HOME/bin", "/opt/homebrew/bin", "/usr/local/bin", "/usr/lib/elixir/bin",
    "%USERPROFILE%\\.asdf\\shims", "%USERPROFILE%\\AppData\\Local\\mise\\shims", "C:\\Program Files\\Elixir\\bin", "C:\\Program Files (x86)\\Elixir\\bin",
];

/// Where `name` is looked for: its own folder under the tools folder
/// first, where the pack installs it, then [`HINTS`].
pub fn hints(name: &str) -> Vec<String> {
    std::iter::once(format!("/tools/{name}")).chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// Expert's release. Expert is the Elixir project's official language
/// server (Lexical and Next LS, merged); its releases are one
/// self-extracting file per system, named `expert_<os>_<arch>`, on GitHub
/// (expert-lsp/expert).
///
/// **The checksums are unverified.** The release's `expert_checksums.txt`
/// lists each file's SHA-256, but it couldn't be fetched when this was
/// written, and a checksum isn't one to guess: each file's is `None`
/// until it's read from the release and put here. Fenix then downloads
/// without checking a hash; a test holds the table to whole checksums or
/// none.
pub const EXPERT: &str = "0.2.3";
const EXPERT_FILES: &[(&str, &str, &str, Option<&str>)] = &[
    ("linux", "x86_64", "expert_linux_amd64", None),
    ("linux", "aarch64", "expert_linux_arm64", None),
    ("macos", "x86_64", "expert_darwin_amd64", None),
    ("macos", "aarch64", "expert_darwin_arm64", None),
    ("windows", "x86_64", "expert_windows_amd64.exe", None),
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

    /// `name` as a program here: `expert.exe` on Windows.
    fn exe(&self, name: &str) -> String {
        if self.os == "windows" {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }

    /// Expert's release for this machine, a lone file into
    /// `/tools/expert`, as the language server for Elixir (`--stdio`).
    pub fn expert(&self) -> Option<Install> {
        let arch = if self.arch == "arm64" { "aarch64" } else { self.arch.as_str() };
        let (_, _, file, sha) = EXPERT_FILES.iter().find(|(os, a, _, _)| *os == self.os && *a == arch)?;
        let url = format!("https://github.com/expert-lsp/expert/releases/download/v{EXPERT}/{file}");
        let install = Install::new("expert", &url).label(&format!("Expert {EXPERT}")).program(&self.exe("expert"));
        let install = match sha {
            Some(sha) => install.sha256(sha),
            None => install,
        };
        Some(install.language_server(&["elixir"], ["--stdio"]))
    }

    /// Every release there's one of for this machine.
    pub fn installs(&self) -> Vec<Install> {
        self.expert().into_iter().collect()
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub mix: Option<String>,
    pub elixir: Option<String>,
    pub iex: Option<String>,
    pub expert: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let mut find = |name: &str| host.find_program(name, &hints(name));
        Tools { mix: find("mix"), elixir: find("elixir"), iex: find("iex"), expert: find("expert") }
    }

    /// Mix by its path when found, else by its name.
    pub fn mix(&self) -> Command {
        Command::new(self.mix.as_deref().unwrap_or("mix"))
    }

    /// `iex` by its path when found, else by its name.
    pub fn iex(&self) -> Command {
        Command::new(self.iex.as_deref().unwrap_or("iex"))
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        match name {
            "mix" => Some(&mut self.mix),
            "elixir" => Some(&mut self.elixir),
            "iex" => Some(&mut self.iex),
            "expert" => Some(&mut self.expert),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_lang_kit::archive::Kind;

    #[test]
    fn expert_is_installed_from_the_release_for_this_machine() {
        let linux = Platform::new("linux", "x86_64").expert().unwrap();
        assert_eq!(linux.url, "https://github.com/expert-lsp/expert/releases/download/v0.2.3/expert_linux_amd64");
        assert_eq!((linux.kind, linux.strip, linux.program.as_str(), linux.name.as_str()), (Kind::Plain, 0, "expert", "expert"), "a lone file, written as expert");
        assert_eq!(linux.label, "Expert 0.2.3");
        assert!(matches!(&linux.uses[..], [fenix_lang_kit::Use::LanguageServer { languages, cmd }] if languages == &["elixir"] && cmd.args == ["--stdio"] && cmd.program == "/tools/expert/expert"));

        assert!(Platform::new("macos", "arm64").expert().unwrap().url.ends_with("/expert_darwin_arm64"));
        let windows = Platform::new("windows", "x86_64").expert().unwrap();
        assert!(windows.url.ends_with("/expert_windows_amd64.exe") && windows.program == "expert.exe");
        assert!(Platform::new("linux", "riscv64").installs().is_empty() && Platform::default().installs().is_empty());
        assert!(Platform::new("windows", "aarch64").installs().is_empty(), "no Windows on Arm release");
    }

    /// A checksum is the release's or it's absent: never a made-up one,
    /// never a short one.
    #[test]
    fn a_checksum_is_whole_or_marked_unverified() {
        for (os, arch, file, sha) in EXPERT_FILES {
            let install = Platform::new(os, arch).expert().unwrap();
            assert!(install.url.ends_with(file));
            match sha {
                Some(sha) => assert!(sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()), "{file}: {sha}"),
                None => assert_eq!(install.sha256, None, "{file}: unverified, so none is sent"),
            }
        }
    }

    #[test]
    fn the_tools_are_looked_for_where_the_managers_put_them() {
        assert_eq!(hints("mix")[0], "/tools/mix");
        assert!(hints("mix").contains(&"$HOME/.asdf/shims".to_string()));
        assert_eq!((Tools::default().mix().program.as_str(), Tools::default().iex().program.as_str()), ("mix", "iex"));
        let mut t = Tools::default();
        *t.slot("expert").unwrap() = Some("/t/expert".into());
        assert_eq!(t.expert.as_deref(), Some("/t/expert"));
        assert!(t.slot("go").is_none());
    }
}
