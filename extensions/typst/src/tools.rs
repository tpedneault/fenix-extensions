//! The programs the pack runs, found on this machine -- tinymist, the
//! language server; typst, the compiler; typstyle, the formatter -- and the
//! releases of each it installs into its tools folder when there isn't one.
//!
//! None of the three publishes a checksum file beside its releases (a
//! `.sha256` of each asset, `SHA256SUMS` and the like all answer 404), so
//! each SHA-256 below is the one computed from the asset downloaded from
//! the release's own URL (`README.md` says which tags and how). Fenix
//! checks every download against it.

use fenix_extension_api::Command;
use fenix_lang_kit::archive::Kind;
use fenix_lang_kit::{Host, Install};

/// Where else to look: Cargo's, the user's own bin folders, Homebrew's,
/// Snap's, Scoop's and winget's links.
pub const HINTS: &[&str] = &[
    "$HOME/.local/bin", "$HOME/bin", "$HOME/.cargo/bin", "/opt/homebrew/bin", "/usr/local/bin", "/snap/bin",
    "%USERPROFILE%\\.cargo\\bin", "%USERPROFILE%\\scoop\\shims", "%LOCALAPPDATA%\\Microsoft\\WinGet\\Links",
];

/// Where `name` is looked for: its own folder under the tools folder
/// first, where the pack installs it, then [`HINTS`].
pub fn hints(name: &str) -> Vec<String> {
    std::iter::once(format!("/tools/{name}")).chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// tinymist's release (Myriad-Dreamin/tinymist, tag `v0.15.12`). Its binary
/// bundles Typst 0.15.1 and typstyle, so it needs no `typst` to serve.
pub const TINYMIST: &str = "0.15.12";
/// typst's release (typst/typst, tag `v0.15.1`).
pub const TYPST: &str = "0.15.1";
/// typstyle's release (Enter-tainer/typstyle, tag `v0.15.2`).
pub const TYPSTYLE: &str = "0.15.2";

/// `(os, arch, file, sha256)`, by OS and architecture as `platform()` says
/// them.
type Files = &'static [(&'static str, &'static str, &'static str, &'static str)];

const TINYMIST_FILES: Files = &[
    ("linux", "x86_64", "tinymist-linux-x64", "59cd0441dc6789a5715a4e7417ebdb492be07ca3d4ebdf40ade976bea1c1d1ff"),
    ("linux", "aarch64", "tinymist-linux-arm64", "e85e7eafdc5436638a98c71d88a77f08942de3cc261ec0bcca2672ff85cc8655"),
    ("macos", "x86_64", "tinymist-darwin-x64", "1682fa2afbae1a77d8ddad1f4d67a5254d92bbd68004a7c1925693185c0ff928"),
    ("macos", "aarch64", "tinymist-darwin-arm64", "92f1d92e7822be822a054aa29a97b4099439ee907c6676034a5002d572ae2f67"),
    ("windows", "x86_64", "tinymist-win32-x64.exe", "c7a2ad09d2f25f18e09f203deda20de1e086b3134fe70a5d163b720e97524af4"),
    ("windows", "aarch64", "tinymist-win32-arm64.exe", "226d9f069041117fd5cbd83babc12d60943246c9c0b0d06e935a52b18a9eacdb"),
];
const TYPST_FILES: Files = &[
    ("linux", "x86_64", "typst-x86_64-unknown-linux-musl.tar.xz", "a6d077d0a95eed5a2eba715b2dae06be954f624ccbf85758a03f389ded33118c"),
    ("linux", "aarch64", "typst-aarch64-unknown-linux-musl.tar.xz", "5aa8d74a3d906e60ea12a66ac2f37f8eef1b14cbad7182a745e393a10c23dcee"),
    ("macos", "x86_64", "typst-x86_64-apple-darwin.tar.xz", "7f9fdd9584866245de9a79e0add8f9236fae6f40a8a45e2c4771ccc14db4e0fa"),
    ("macos", "aarch64", "typst-aarch64-apple-darwin.tar.xz", "48f62ed034aa3a7978309579ac6ca00045e2ef0da73114e8af27cfd8e74dc05a"),
    ("windows", "x86_64", "typst-x86_64-pc-windows-msvc.zip", "19ce3551153c2fe7ee9fa2f95208310c8f4d3209fedb699e0333faf8913f6736"),
    ("windows", "aarch64", "typst-aarch64-pc-windows-msvc.zip", "4ab28e1b71ec3184d38d580ab797f499b6770d952b6b19167be5cea5c2662e14"),
];
const TYPSTYLE_FILES: Files = &[
    ("linux", "x86_64", "typstyle-x86_64-unknown-linux-gnu", "ea790059cd05016a6f4282386ab7b4fc92f83faa0f5126d8216df2562d7596fd"),
    ("linux", "aarch64", "typstyle-aarch64-unknown-linux-gnu", "4021f09180d04b804abfd6f5def3b09a0c63ecbb4d40e0d9b4ddd98e7b388f12"),
    ("macos", "x86_64", "typstyle-x86_64-apple-darwin", "8a02e72953d2ff183fed7753b8a19e2c372fbbcff49266d2b8fd703577ed5f89"),
    ("macos", "aarch64", "typstyle-aarch64-apple-darwin", "b3f7d55bf06f2b190a05ade3357ed62206aef5dabf12d205d158bc2bb203c975"),
    ("windows", "x86_64", "typstyle-x86_64-pc-windows-msvc.exe", "ec620da653e620968020768443bbe51acc96c65aa3fc15cd9f3a7610f2fbf782"),
    ("windows", "aarch64", "typstyle-aarch64-pc-windows-msvc.exe", "5eb62b46d1651cbcb8d504703995d07a02e7f9d57ade089de3988ae9246f5d24"),
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

    /// `name` as a program here: `typst.exe` on Windows.
    fn exe(&self, name: &str) -> String {
        if self.os == "windows" {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }

    fn file(&self, files: Files) -> Option<(&'static str, &'static str)> {
        let arch = if self.arch == "arm64" { "aarch64" } else { self.arch.as_str() };
        files.iter().find(|(os, a, _, _)| *os == self.os && *a == arch).map(|(_, _, f, s)| (*f, *s))
    }

    /// tinymist for this machine: the program itself, a file of its own.
    pub fn tinymist(&self) -> Option<Install> {
        let (file, sha) = self.file(TINYMIST_FILES)?;
        let url = format!("https://github.com/Myriad-Dreamin/tinymist/releases/download/v{TINYMIST}/{file}");
        Some(Install::new("tinymist", &url).label(&format!("tinymist {TINYMIST}")).sha256(sha).kind(Kind::Plain).program(&self.exe("tinymist")))
    }

    /// typst for this machine: its program in a folder of the archive's name.
    pub fn typst(&self) -> Option<Install> {
        let (file, sha) = self.file(TYPST_FILES)?;
        let url = format!("https://github.com/typst/typst/releases/download/v{TYPST}/{file}");
        let kind = if file.ends_with(".zip") { Kind::Zip } else { Kind::TarXz };
        Some(Install::new("typst", &url).label(&format!("Typst {TYPST}")).sha256(sha).kind(kind).strip(1).program(&self.exe("typst")))
    }

    /// typstyle for this machine: the program itself.
    pub fn typstyle(&self) -> Option<Install> {
        let (file, sha) = self.file(TYPSTYLE_FILES)?;
        let url = format!("https://github.com/Enter-tainer/typstyle/releases/download/v{TYPSTYLE}/{file}");
        Some(Install::new("typstyle", &url).label(&format!("typstyle {TYPSTYLE}")).sha256(sha).kind(Kind::Plain).program(&self.exe("typstyle")))
    }

    /// Every release there's one of for this machine.
    pub fn installs(&self) -> Vec<Install> {
        [self.tinymist(), self.typst(), self.typstyle()].into_iter().flatten().collect()
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub tinymist: Option<String>,
    pub typst: Option<String>,
    pub typstyle: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let mut find = |name: &str| host.find_program(name, &hints(name));
        Tools { tinymist: find("tinymist"), typst: find("typst"), typstyle: find("typstyle") }
    }

    /// Typst by its path when found, else by its name.
    pub fn typst(&self) -> Command {
        Command::new(self.typst.as_deref().unwrap_or("typst"))
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        match name {
            "tinymist" => Some(&mut self.tinymist),
            "typst" => Some(&mut self.typst),
            "typstyle" => Some(&mut self.typstyle),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [(&str, &str); 6] = [("linux", "x86_64"), ("linux", "aarch64"), ("macos", "x86_64"), ("macos", "aarch64"), ("windows", "x86_64"), ("windows", "aarch64")];

    #[test]
    fn each_tool_is_installed_from_the_release_for_this_machine() {
        let t = Platform::new("linux", "x86_64").tinymist().unwrap();
        assert_eq!(t.url, "https://github.com/Myriad-Dreamin/tinymist/releases/download/v0.15.12/tinymist-linux-x64");
        assert_eq!((t.kind, t.strip, t.program.as_str()), (Kind::Plain, 0, "tinymist"), "a lone file, written as the program");
        assert_eq!(t.sha256.as_deref(), Some("59cd0441dc6789a5715a4e7417ebdb492be07ca3d4ebdf40ade976bea1c1d1ff"));
        assert_eq!(t.label, "tinymist 0.15.12");

        let ty = Platform::new("linux", "aarch64").typst().unwrap();
        assert!(ty.url.ends_with("/v0.15.1/typst-aarch64-unknown-linux-musl.tar.xz"), "{}", ty.url);
        assert_eq!((ty.kind, ty.strip, ty.program.as_str()), (Kind::TarXz, 1, "typst"), "inside a folder of the archive's name");
        let win = Platform::new("windows", "x86_64").typst().unwrap();
        assert_eq!((win.kind, win.program.as_str()), (Kind::Zip, "typst.exe"));

        let style = Platform::new("macos", "arm64").typstyle().unwrap();
        assert!(style.url.ends_with("/v0.15.2/typstyle-aarch64-apple-darwin"), "{}", style.url);
        assert_eq!(Platform::new("windows", "aarch64").typstyle().unwrap().program, "typstyle.exe");
    }

    #[test]
    fn every_platform_has_all_three_with_a_checksum_and_the_hosts_the_manifest_names() {
        for (os, arch) in ALL {
            let installs = Platform::new(os, arch).installs();
            assert_eq!(installs.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["tinymist", "typst", "typstyle"], "{os} {arch}");
            for i in installs {
                let sha = i.sha256.as_deref().unwrap_or_else(|| panic!("{os} {arch} {}: a checksum", i.name));
                assert!(sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{os} {arch} {}", i.name);
                assert!(i.url.starts_with("https://github.com/") && i.url.contains("/releases/download/v"), "{}", i.url);
            }
        }
        assert!(Platform::new("linux", "riscv64").installs().is_empty() && Platform::default().installs().is_empty());
    }

    #[test]
    fn no_two_files_share_a_name_or_a_checksum_within_a_tool() {
        for files in [TINYMIST_FILES, TYPST_FILES, TYPSTYLE_FILES] {
            for (i, a) in files.iter().enumerate() {
                for b in &files[i + 1..] {
                    assert_ne!(a.2, b.2);
                    assert_ne!(a.3, b.3, "{} and {} are different files", a.2, b.2);
                }
            }
        }
    }

    #[test]
    fn programs_are_looked_for_where_the_pack_installs_them() {
        assert_eq!(hints("typst")[0], "/tools/typst");
        assert!(hints("tinymist").contains(&"$HOME/.cargo/bin".to_string()));
        assert_eq!(Tools::default().typst().program, "typst");
        let mut t = Tools::default();
        *t.slot("typst").unwrap() = Some("/t/typst".into());
        assert_eq!(t.typst().program, "/t/typst");
        assert!(t.slot("go").is_none());
    }
}
