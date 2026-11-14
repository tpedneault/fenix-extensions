//! The programs the pack runs, found on this machine -- texlab, the
//! language server; the builders (latexmk, Tectonic, or an engine:
//! pdflatex, xelatex, lualatex); the formatters (tex-fmt, latexindent);
//! chktex, texcount and synctex -- and the releases of the three it
//! installs into its tools folder when there isn't one: texlab, Tectonic
//! and tex-fmt. A TeX distribution is several gigabytes and the user's to
//! choose (TeX Live, MiKTeX, MacTeX): the pack installs none, and offers
//! Tectonic, one self-contained program, when it finds no builder.
//!
//! None of the three publishes a checksum file beside its releases (a
//! `.sha256` of each asset, `SHA256SUMS` and the like all answer 404), so
//! each SHA-256 below is the one computed from the asset downloaded from
//! the release's own URL (`README.md` says which tags). Fenix checks every
//! download against it.

use fenix_lang_kit::archive::Kind;
use fenix_lang_kit::{Host, Install};

/// Where else to look: the user's own bin folders, Homebrew's, Snap's, the
/// folders MacTeX and MiKTeX use, Scoop's and winget's links, and Cargo's.
pub const HINTS: &[&str] = &[
    "$HOME/.local/bin", "$HOME/bin", "$HOME/.cargo/bin", "/opt/homebrew/bin", "/usr/local/bin", "/snap/bin", "/Library/TeX/texbin",
    "%USERPROFILE%\\.cargo\\bin", "%USERPROFILE%\\scoop\\shims", "%LOCALAPPDATA%\\Microsoft\\WinGet\\Links", "%LOCALAPPDATA%\\Programs\\MiKTeX\\miktex\\bin\\x64", "%ProgramFiles%\\MiKTeX\\miktex\\bin\\x64",
];

/// Where `name` is looked for: its own folder under the tools folder
/// first, where the pack installs it, then [`HINTS`].
pub fn hints(name: &str) -> Vec<String> {
    std::iter::once(format!("/tools/{name}")).chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// texlab's release (latex-lsp/texlab, tag `v5.26.0`).
pub const TEXLAB: &str = "5.26.0";
/// Tectonic's release (tectonic-typesetting/tectonic, tag `tectonic@0.17.0`).
pub const TECTONIC: &str = "0.17.0";
/// tex-fmt's release (WGUNDERWOOD/tex-fmt, tag `v0.5.7`).
pub const TEXFMT: &str = "0.5.7";

/// `(os, arch, file, sha256)`, by OS and architecture as `platform()` says
/// them.
type Files = &'static [(&'static str, &'static str, &'static str, &'static str)];

const TEXLAB_FILES: Files = &[
    ("linux", "x86_64", "texlab-x86_64-linux.tar.gz", "8697bd5e479d4584b14b7eed5c320c80ec4e1d91ebefbb6801e6bf38e9971300"),
    ("linux", "aarch64", "texlab-aarch64-linux.tar.gz", "a85cdfcd22454b8d8550f4b0f0620c45ab51760f302fac7a12bc18a890f70f8c"),
    ("macos", "x86_64", "texlab-x86_64-macos.tar.gz", "6091611f756b28e1a57612b130c196df4b0bb6e22dde5cf5d890578513397daf"),
    ("macos", "aarch64", "texlab-aarch64-macos.tar.gz", "af7972ffd230711ba04ada9b69cc32ce9111d9196ba69538062872faefdbee56"),
    ("windows", "x86_64", "texlab-x86_64-windows.zip", "cb028d44c3d2b85d36a2ed52d41a0ff43a341b1f04c500c56c4524c4eb72b316"),
    ("windows", "aarch64", "texlab-aarch64-windows.zip", "99b215e9a44169eb8d786c33484b963055e1c3dd40f68e14fcc47ae1c84e92c1"),
];
const TECTONIC_FILES: Files = &[
    ("linux", "x86_64", "tectonic-0.17.0-x86_64-unknown-linux-musl.tar.gz", "8533d07f9ccbd7a65824b9e0459041bca34af1eb33daba48f59215593753a3b7"),
    ("linux", "aarch64", "tectonic-0.17.0-aarch64-unknown-linux-musl.tar.gz", "b10954a95404f3ab2328d2fa59a5ebab8e657f893fab096f98be8db7c0c979b8"),
    ("macos", "x86_64", "tectonic-0.17.0-x86_64-apple-darwin.tar.gz", "7c90ef5b6ddb1eb1937e4337add5237b79338e4b9676459fa91187d24d6cdf80"),
    ("macos", "aarch64", "tectonic-0.17.0-aarch64-apple-darwin.tar.gz", "a3f1cac7c5678f01661a92212f58480ae3b0634115d880dbc59e2953ded45667"),
    ("windows", "x86_64", "tectonic-0.17.0-x86_64-pc-windows-msvc.zip", "f61ce51f0b0ade1015b7de7ef368541c5424e9756ecbd0d7af97d6d48030845f"),
];
const TEXFMT_FILES: Files = &[
    ("linux", "x86_64", "tex-fmt-x86_64-linux.tar.gz", "d0e30b9b811fdc1e66a9328d5c95e9d8e585019288ca39c07f643e0796bcc4a3"),
    ("linux", "aarch64", "tex-fmt-aarch64-linux.tar.gz", "ad5a6c8a84f61de711f2d7659191cd7e813ad1881b384abed60df420e1eabdd1"),
    ("macos", "x86_64", "tex-fmt-x86_64-macos.tar.gz", "e39311252562a3608474639e182782ca51f319fa50a8abfca9cb09e59ed7f7ac"),
    ("macos", "aarch64", "tex-fmt-aarch64-macos.tar.gz", "f88ad74c446d6dcfec9657ef600d61f65f0ccdcf8401e2909bb3b093d23fff80"),
    ("windows", "x86_64", "tex-fmt-x86_64-windows.zip", "807b6df5bef297eb8c44f79562bf9917b2e87f41e8e9c6b7f27e2df3f8cc9c98"),
    ("windows", "aarch64", "tex-fmt-aarch64-windows.zip", "d3ce79d3cecf1b6a66c0978125b924cace80904c0d55bf75dc96bebdca5fc978"),
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

    /// `name` as a program here: `texlab.exe` on Windows.
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

    /// texlab for this machine: the program alone in its archive.
    pub fn texlab(&self) -> Option<Install> {
        let (file, sha) = self.file(TEXLAB_FILES)?;
        let url = format!("https://github.com/latex-lsp/texlab/releases/download/v{TEXLAB}/{file}");
        let kind = if file.ends_with(".zip") { Kind::Zip } else { Kind::TarGz };
        Some(Install::new("texlab", &url).label(&format!("texlab {TEXLAB}")).sha256(sha).kind(kind).program(&self.exe("texlab")))
    }

    /// Tectonic for this machine (the static build on Linux): the program
    /// alone in its archive. None for Windows on ARM, which it has no release for.
    pub fn tectonic(&self) -> Option<Install> {
        let (file, sha) = self.file(TECTONIC_FILES)?;
        let url = format!("https://github.com/tectonic-typesetting/tectonic/releases/download/tectonic%40{TECTONIC}/{file}");
        let kind = if file.ends_with(".zip") { Kind::Zip } else { Kind::TarGz };
        Some(Install::new("tectonic", &url).label(&format!("Tectonic {TECTONIC}")).sha256(sha).kind(kind).program(&self.exe("tectonic")))
    }

    /// tex-fmt for this machine: the program alone in its archive.
    pub fn tex_fmt(&self) -> Option<Install> {
        let (file, sha) = self.file(TEXFMT_FILES)?;
        let url = format!("https://github.com/WGUNDERWOOD/tex-fmt/releases/download/v{TEXFMT}/{file}");
        let kind = if file.ends_with(".zip") { Kind::Zip } else { Kind::TarGz };
        Some(Install::new("tex-fmt", &url).label(&format!("tex-fmt {TEXFMT}")).sha256(sha).kind(kind).program(&self.exe("tex-fmt")))
    }

    /// Every release there's one of for this machine.
    pub fn installs(&self) -> Vec<Install> {
        [self.texlab(), self.tectonic(), self.tex_fmt()].into_iter().flatten().collect()
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub texlab: Option<String>,
    pub tectonic: Option<String>,
    pub latexmk: Option<String>,
    pub pdflatex: Option<String>,
    pub xelatex: Option<String>,
    pub lualatex: Option<String>,
    pub tex_fmt: Option<String>,
    pub latexindent: Option<String>,
    pub chktex: Option<String>,
    pub texcount: Option<String>,
    pub synctex: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let mut find = |name: &str| host.find_program(name, &hints(name));
        Tools {
            texlab: find("texlab"),
            tectonic: find("tectonic"),
            latexmk: find("latexmk"),
            pdflatex: find("pdflatex"),
            xelatex: find("xelatex"),
            lualatex: find("lualatex"),
            tex_fmt: find("tex-fmt"),
            latexindent: find("latexindent"),
            chktex: find("chktex"),
            texcount: find("texcount"),
            synctex: find("synctex"),
        }
    }

    /// The engine `name` (`pdflatex`, `xelatex`, `lualatex`) as found.
    pub fn engine(&self, name: &str) -> Option<&str> {
        match name {
            "pdflatex" => self.pdflatex.as_deref(),
            "xelatex" => self.xelatex.as_deref(),
            "lualatex" => self.lualatex.as_deref(),
            _ => None,
        }
    }

    /// Whether anything here can build a document.
    pub fn builds(&self) -> bool {
        self.latexmk.is_some() || self.tectonic.is_some() || self.pdflatex.is_some() || self.xelatex.is_some() || self.lualatex.is_some()
    }

    /// The program found, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        Some(match name {
            "texlab" => &mut self.texlab,
            "tectonic" => &mut self.tectonic,
            "latexmk" => &mut self.latexmk,
            "pdflatex" => &mut self.pdflatex,
            "xelatex" => &mut self.xelatex,
            "lualatex" => &mut self.lualatex,
            "tex-fmt" => &mut self.tex_fmt,
            "latexindent" => &mut self.latexindent,
            "chktex" => &mut self.chktex,
            "texcount" => &mut self.texcount,
            "synctex" => &mut self.synctex,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [(&str, &str); 6] = [("linux", "x86_64"), ("linux", "aarch64"), ("macos", "x86_64"), ("macos", "aarch64"), ("windows", "x86_64"), ("windows", "aarch64")];

    #[test]
    fn each_tool_is_installed_from_the_release_for_this_machine() {
        let t = Platform::new("linux", "x86_64").texlab().unwrap();
        assert_eq!(t.url, "https://github.com/latex-lsp/texlab/releases/download/v5.26.0/texlab-x86_64-linux.tar.gz");
        assert_eq!((t.kind, t.strip, t.program.as_str()), (Kind::TarGz, 0, "texlab"), "the program alone at the archive's top");
        assert_eq!(t.sha256.as_deref(), Some("8697bd5e479d4584b14b7eed5c320c80ec4e1d91ebefbb6801e6bf38e9971300"));
        assert_eq!(t.label, "texlab 5.26.0");

        let tectonic = Platform::new("linux", "aarch64").tectonic().unwrap();
        assert!(tectonic.url.ends_with("/tectonic%400.17.0/tectonic-0.17.0-aarch64-unknown-linux-musl.tar.gz"), "{}", tectonic.url);
        let win = Platform::new("windows", "x86_64").tectonic().unwrap();
        assert_eq!((win.kind, win.program.as_str()), (Kind::Zip, "tectonic.exe"));
        assert!(Platform::new("windows", "aarch64").tectonic().is_none(), "no release for Windows on ARM");

        let fmt = Platform::new("macos", "arm64").tex_fmt().unwrap();
        assert!(fmt.url.ends_with("/v0.5.7/tex-fmt-aarch64-macos.tar.gz"), "{}", fmt.url);
        assert_eq!(Platform::new("windows", "aarch64").tex_fmt().unwrap().program, "tex-fmt.exe");
    }

    #[test]
    fn every_platform_has_its_installs_with_a_checksum_and_the_hosts_the_manifest_names() {
        for (os, arch) in ALL {
            let installs = Platform::new(os, arch).installs();
            let names: Vec<&str> = installs.iter().map(|i| i.name.as_str()).collect();
            if (os, arch) == ("windows", "aarch64") {
                assert_eq!(names, ["texlab", "tex-fmt"], "no Tectonic there");
            } else {
                assert_eq!(names, ["texlab", "tectonic", "tex-fmt"], "{os} {arch}");
            }
            for i in installs {
                let sha = i.sha256.as_deref().unwrap_or_else(|| panic!("{os} {arch} {}: a checksum", i.name));
                assert!(sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{os} {arch} {}", i.name);
                assert!(i.url.starts_with("https://github.com/") && i.url.contains("/releases/download/"), "{}", i.url);
            }
        }
        assert!(Platform::new("linux", "riscv64").installs().is_empty() && Platform::default().installs().is_empty());
    }

    #[test]
    fn no_two_files_share_a_name_or_a_checksum_within_a_tool() {
        for files in [TEXLAB_FILES, TECTONIC_FILES, TEXFMT_FILES] {
            for (i, a) in files.iter().enumerate() {
                for b in &files[i + 1..] {
                    assert_ne!(a.2, b.2);
                    assert_ne!(a.3, b.3, "{} and {} are different files", a.2, b.2);
                }
            }
        }
    }

    #[test]
    fn programs_are_looked_for_where_the_pack_installs_them_and_tex_lives() {
        assert_eq!(hints("texlab")[0], "/tools/texlab");
        assert!(hints("latexmk").contains(&"/Library/TeX/texbin".to_string()));
        let mut t = Tools::default();
        assert!(!t.builds());
        *t.slot("tex-fmt").unwrap() = Some("/t/tex-fmt".into());
        assert_eq!(t.tex_fmt.as_deref(), Some("/t/tex-fmt"));
        assert!(!t.builds(), "a formatter doesn't build");
        *t.slot("xelatex").unwrap() = Some("/t/xelatex".into());
        assert!(t.builds());
        assert_eq!((t.engine("xelatex"), t.engine("pdflatex"), t.engine("latex")), (Some("/t/xelatex"), None, None));
        assert!(t.slot("go").is_none());
    }
}
