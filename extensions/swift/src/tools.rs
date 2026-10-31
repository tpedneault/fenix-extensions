//! The programs the pack runs, found on this machine: Swift and what its
//! toolchain has beside it (sourcekit-lsp, and swift-format since Swift
//! 6), SwiftFormat and SwiftLint. swiftly, the toolchains' installers,
//! Xcode and Homebrew put them in folders an editor started from the
//! desktop may not have on its `PATH`, so they're looked in too, and in
//! the folder of the `swift` found first: its sourcekit-lsp is the one
//! that matches it. On macOS `xcrun` finds Xcode's.
//!
//! The pack installs only SwiftLint, from its release, into its tools
//! folder; Swift itself is a toolchain the doctor says how to get.

use fenix_extension_api::Command;
use fenix_lang_kit::{Host, Install};

/// Where else to look: swiftly's (`~/.swiftly/bin` on macOS, under
/// `~/.local/share` on Linux), the tarballs' usual homes, the command
/// line tools' and Xcode's, Homebrew's, and Mint's.
pub const HINTS: &[&str] = &[
    "$HOME/.swiftly/bin", "$HOME/.local/share/swiftly/bin", "/usr/bin", "/usr/local/bin", "/opt/swift/usr/bin", "/usr/share/swift/usr/bin",
    "/Library/Developer/CommandLineTools/usr/bin", "/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin",
    "/opt/homebrew/bin", "$HOME/.mint/bin",
];

/// Where `name` is looked for: its own folder under the tools folder
/// first, where the pack installs it, then the folder `swift` was found
/// in, then [`HINTS`].
pub fn hints(name: &str, swift: Option<&str>) -> Vec<String> {
    let beside = swift.and_then(|s| s.replace('\\', "/").rsplit_once('/').map(|(dir, _)| dir.to_string()));
    std::iter::once(format!("/tools/{name}")).chain(beside).chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// Where Swift comes from, for each OS: swift.org's pages say how
/// (swiftly on Linux and macOS, Xcode, the Windows installer).
pub fn install_page(os: &str) -> &'static str {
    match os {
        "macos" => "https://www.swift.org/install/macos/",
        "windows" => "https://www.swift.org/install/windows/",
        _ => "https://www.swift.org/install/linux/",
    }
}

/// SwiftLint's release the pack installs, and each file's SHA-256, by OS
/// and architecture as `platform()` says them.
pub const SWIFTLINT: &str = "0.65.1";
const SWIFTLINT_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "swiftlint_linux_amd64.zip:caeed6f4a679c35539ffaf124f6c4ab4a8416917f7d8796279dc52b74026059d"),
    ("linux", "aarch64", "swiftlint_linux_arm64.zip:9ffa52f478e6d8eb485d37d14715ffac90abc81c58f3370d598bf75be05605f8"),
    // One for both: a universal binary.
    ("macos", "x86_64", "portable_swiftlint.zip:c1e429b0599cf1b516f369a2d9ec04eaf0e436f3c12b637df8851fa52ff694d0"),
    ("macos", "aarch64", "portable_swiftlint.zip:c1e429b0599cf1b516f369a2d9ec04eaf0e436f3c12b637df8851fa52ff694d0"),
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

    /// SwiftLint's release for this machine, into `/tools/swiftlint`: a
    /// zip with the program at its top. Linux's links Swift's own
    /// libcurl and libxml2, which a toolchain needs too.
    pub fn swiftlint(&self) -> Option<Install> {
        let arch = if self.arch == "arm64" { "aarch64" } else { self.arch.as_str() };
        let (_, _, file) = SWIFTLINT_FILES.iter().find(|(os, a, _)| *os == self.os && *a == arch)?;
        let (file, sha) = file.split_once(':')?;
        let url = format!("https://github.com/realm/SwiftLint/releases/download/{SWIFTLINT}/{file}");
        Some(Install::new("swiftlint", &url).label(&format!("SwiftLint {SWIFTLINT}")).sha256(sha).program("swiftlint"))
    }
}

/// The formatter to use (`swift.formatter`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Formatter {
    /// SwiftFormat in a project with a `.swiftformat` (and no
    /// `.swift-format`), else swift-format.
    #[default]
    Auto,
    /// swift-format, the toolchain's (swiftlang's).
    Apple,
    /// SwiftFormat, Nick Lockwood's.
    Lockwood,
    Off,
}

impl Formatter {
    pub fn parse(text: &str) -> Formatter {
        match text.trim().to_ascii_lowercase().as_str() {
            "swift-format" | "swift format" => Formatter::Apple,
            "swiftformat" => Formatter::Lockwood,
            "off" | "none" | "false" => Formatter::Off,
            _ => Formatter::Auto,
        }
    }
}

/// When SwiftLint runs on a saved file (`swift.swiftlint`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LintWhen {
    /// In a project with a `.swiftlint.yml`.
    #[default]
    Project,
    /// With its own rules where there's none.
    Always,
    Off,
}

impl LintWhen {
    pub fn parse(text: &str) -> LintWhen {
        match text.trim().to_ascii_lowercase().as_str() {
            "always" | "true" => LintWhen::Always,
            "off" | "never" | "false" => LintWhen::Off,
            _ => LintWhen::Project,
        }
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub swift: Option<String>,
    pub sourcekit_lsp: Option<String>,
    pub swift_format: Option<String>,
    pub swiftformat: Option<String>,
    pub swiftlint: Option<String>,
    pub xcrun: Option<String>,
    pub brew: Option<String>,
    /// Swift's major version, once the doctor has asked: 6 and later
    /// have `swift format`.
    pub swift_major: Option<u64>,
    /// `platform()`'s `os`: `linux`, `macos`, `windows`.
    pub os: String,
}

impl Tools {
    pub fn find(host: &mut dyn Host, os: &str) -> Tools {
        let swift = host.find_program("swift", &hints("swift", None));
        let mut find = |name: &str| host.find_program(name, &hints(name, swift.as_deref()));
        Tools {
            sourcekit_lsp: find("sourcekit-lsp"),
            swift_format: find("swift-format"),
            swiftformat: find("swiftformat"),
            swiftlint: find("swiftlint"),
            xcrun: if os == "macos" { find("xcrun") } else { None },
            brew: if os == "macos" { find("brew") } else { None },
            swift,
            swift_major: None,
            os: os.into(),
        }
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        Some(match name {
            "swift" => &mut self.swift,
            "sourcekit-lsp" => &mut self.sourcekit_lsp,
            "swift-format" => &mut self.swift_format,
            "swiftformat" => &mut self.swiftformat,
            "swiftlint" => &mut self.swiftlint,
            _ => return None,
        })
    }

    /// The language server: sourcekit-lsp found, else Xcode's through
    /// `xcrun` on macOS. None leaves Fenix's own.
    pub fn server(&self) -> Option<Command> {
        match (&self.sourcekit_lsp, &self.xcrun) {
            (Some(lsp), _) => Some(Command::new(lsp)),
            (None, Some(xcrun)) => Some(Command::new(xcrun).arg("sourcekit-lsp")),
            (None, None) => None,
        }
    }

    /// swift-format on the buffer, its config found from the file's
    /// folder up: the program found, else Swift 6's `swift format`.
    pub fn swift_format(&self) -> Option<Command> {
        let args = ["format", "--assume-filename", "${file}"];
        match (&self.swift_format, &self.swift) {
            (Some(f), _) => Some(Command::new(f).args(args)),
            (None, Some(swift)) if self.swift_major.is_none_or(|m| m >= 6) => Some(Command::new(swift).arg("format").args(args)),
            _ => None,
        }
    }

    /// SwiftFormat on the buffer, its `.swiftformat` found from the file's
    /// path.
    pub fn swiftformat(&self) -> Option<Command> {
        self.swiftformat.as_ref().map(|f| Command::new(f).args(["--stdinpath", "${file}", "--quiet"]))
    }

    /// Homebrew's `brew install <formula>`, on macOS.
    pub fn brew_install(&self, formula: &str) -> Option<Command> {
        self.brew.as_ref().map(|b| Command::new(b).args(["install", formula]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;
    use fenix_lang_kit::archive::Kind;

    #[test]
    fn swiftlint_is_installed_from_the_release_for_this_machine() {
        let linux = Platform::new("linux", "x86_64").swiftlint().unwrap();
        assert_eq!(linux.url, "https://github.com/realm/SwiftLint/releases/download/0.65.1/swiftlint_linux_amd64.zip");
        assert_eq!((linux.kind, linux.strip, linux.program.as_str()), (Kind::Zip, 0, "swiftlint"));
        assert!(Platform::new("linux", "arm64").swiftlint().unwrap().url.ends_with("swiftlint_linux_arm64.zip"));
        let mac = Platform::new("macos", "aarch64").swiftlint().unwrap();
        assert_eq!(mac.sha256, Platform::new("macos", "x86_64").swiftlint().unwrap().sha256, "a universal binary");
        assert_eq!(mac.sha256.as_deref().map(str::len), Some(64));
        assert!(Platform::new("windows", "x86_64").swiftlint().is_none() && Platform::default().swiftlint().is_none());
    }

    #[test]
    fn sourcekit_lsp_is_looked_for_beside_swift() {
        assert_eq!(hints("sourcekit-lsp", Some("/opt/swift-6.2/usr/bin/swift"))[..2], ["/tools/sourcekit-lsp".to_string(), "/opt/swift-6.2/usr/bin".to_string()]);
        assert!(hints("swift", None).contains(&"$HOME/.local/share/swiftly/bin".to_string()));
        let mut t = Tools { xcrun: Some("/usr/bin/xcrun".into()), ..Tools::default() };
        assert_eq!(t.server().map(|c| line(&c)).as_deref(), Some("/usr/bin/xcrun sourcekit-lsp"), "Xcode's");
        t.sourcekit_lsp = Some("/h/.swiftly/bin/sourcekit-lsp".into());
        assert_eq!(t.server().map(|c| line(&c)).as_deref(), Some("/h/.swiftly/bin/sourcekit-lsp"));
        assert_eq!(Tools::default().server(), None);
        assert_eq!(install_page("macos"), "https://www.swift.org/install/macos/");
    }

    #[test]
    fn swift_format_is_its_own_or_swift_6s() {
        let mut t = Tools { swift: Some("/usr/bin/swift".into()), ..Tools::default() };
        assert_eq!(t.swift_format().map(|c| line(&c)).as_deref(), Some("/usr/bin/swift format format --assume-filename ${file}"));
        t.swift_major = Some(5);
        assert_eq!(t.swift_format(), None, "Swift 5 has none");
        t.swift_format = Some("/b/swift-format".into());
        assert_eq!(t.swift_format().map(|c| line(&c)).as_deref(), Some("/b/swift-format format --assume-filename ${file}"));
        t.swiftformat = Some("/opt/homebrew/bin/swiftformat".into());
        assert_eq!(t.swiftformat().map(|c| line(&c)).as_deref(), Some("/opt/homebrew/bin/swiftformat --stdinpath ${file} --quiet"));
        assert_eq!((Formatter::parse("SwiftFormat"), Formatter::parse("swift-format"), Formatter::parse("off"), Formatter::parse("")), (Formatter::Lockwood, Formatter::Apple, Formatter::Off, Formatter::Auto));
        assert_eq!((LintWhen::parse("always"), LintWhen::parse("off"), LintWhen::parse("project")), (LintWhen::Always, LintWhen::Off, LintWhen::Project));
    }
}
