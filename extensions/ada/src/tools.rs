//! The programs the pack runs, found on this machine: Alire (`alr`),
//! GNAT and gprbuild, GNATprove, and the Ada Language Server; and the
//! releases of Alire and the server it installs into its tools folder
//! when they aren't here already. GNAT and GNATprove come through Alire:
//! `alr install` puts them in `~/.alire/bin`, which is looked in, as are
//! GNAT's own install folders. Alire's toolchains
//! (`~/.local/share/alire/toolchains/gnat_native_<version>_<hash>`) are
//! named by a hash, so they're used through `alr`, not looked for.

use fenix_extension_api::Command;
use fenix_lang_kit::{Host, Install};

/// Where else to look: Alire's install prefix (`alr install`), the
/// user's own bin folders, GNAT's install folders (GNAT Pro's, the last
/// GNAT Community's, a GNAT FSF tarball's), Homebrew's, and Alire's
/// Windows installer's.
pub const HINTS: &[&str] = &[
    "$HOME/.alire/bin", "$HOME/.local/bin", "$HOME/bin", "/usr/gnat/bin", "/opt/gnat/bin", "/opt/GNAT/2021/bin", "$HOME/opt/GNAT/2021/bin", "/opt/homebrew/bin", "/usr/local/bin",
    "%USERPROFILE%\\.alire\\bin", "C:\\Program Files\\Alire\\bin", "C:\\GNAT\\2021\\bin",
];

/// Where `name` is looked for: where the pack installs it first, then
/// [`HINTS`].
pub fn hints(name: &str) -> Vec<String> {
    let own = match name {
        "alr" => Some("/tools/alr/bin"),
        "ada_language_server" => Some("/tools/als"),
        _ => None,
    };
    own.into_iter().map(str::to_string).chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// Alire's release, and each file's SHA-256, by OS and architecture as
/// `platform()` says them: a zip of `bin/alr` and its licence.
pub const ALIRE: &str = "2.1.1";
const ALIRE_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "alr-2.1.1-bin-x86_64-linux.zip:09c66bcd8c35dd4b97b72c3d9b76e44caa6964a2db35aba069f396f00f1f64c7"),
    ("linux", "aarch64", "alr-2.1.1-bin-aarch64-linux.zip:d76c93ad3dc631826144e10bdabc6b3bf98783805bebfd5e4a0e852dd524d812"),
    ("macos", "x86_64", "alr-2.1.1-bin-x86_64-macos.zip:d3e16cdfaf0cfb2da62853b79b62910189fdca9d5fddc5c3ac5974ffc7d9544b"),
    ("macos", "aarch64", "alr-2.1.1-bin-aarch64-macos.zip:2c4867bfff3b95ecd9d846df460a52983d2b0072808b341f8fa5d82494fb309e"),
    ("windows", "x86_64", "alr-2.1.1-bin-x86_64-windows.zip:863013b1f94da6f3b7d0d5a74022ac3370424eeea9a470ebdb33d188d61b9125"),
];

/// The Ada Language Server's release (AdaCore's; a tag ending in 1 is a
/// release, not a pre-release): a tar.gz of the folder its VS Code
/// extension ships it in, `integration/vscode/ada/<arch>/<os>/`.
pub const ALS: &str = "2027.0.202610121";
const ALS_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "linux-x64:9715f019b1939c171829359367359cd295f0a4e9615d6fea080705972e220a70"),
    ("linux", "aarch64", "linux-arm64:00f1882bda7bb3c4b1d594643f597d508cd788337a0314065d00a0abe5eacba6"),
    ("macos", "x86_64", "darwin-x64:0eace238e9f88358bcc1dc2156fbb8c3311dc0b937ce20ae73d7e8c9337e62e3"),
    ("macos", "aarch64", "darwin-arm64:9f4a49446613fe196c661c63c3235db9633b027f976eaad6489ef042b305daec"),
    ("windows", "x86_64", "win32-x64:3ed7f85b674974fb81685e6bdcbe64824e4f564c675d5453f8e749717e1b6172"),
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

    /// `name` as a program here: `alr.exe` on Windows.
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

    /// Alire's release for this machine, into `/tools/alr`.
    pub fn alr(&self) -> Option<Install> {
        let (file, sha) = self.file(ALIRE_FILES)?;
        let url = format!("https://github.com/alire-project/alire/releases/download/v{ALIRE}/{file}");
        Some(Install::new("alr", &url).label(&format!("Alire {ALIRE}")).sha256(sha).program(&format!("bin/{}", self.exe("alr"))))
    }

    /// The server's release for this machine, into `/tools/als`: the five
    /// folders above it taken off. Its debug symbols come with it.
    pub fn als(&self) -> Option<Install> {
        let (target, sha) = self.file(ALS_FILES)?;
        let url = format!("https://github.com/AdaCore/ada_language_server/releases/download/{ALS}/als-{ALS}-{target}.tar.gz");
        Some(Install::new("als", &url).label(&format!("the Ada Language Server {ALS}")).sha256(sha).strip(5).program(&self.exe("ada_language_server")))
    }

    /// Every release there's one of for this machine.
    pub fn installs(&self) -> Vec<Install> {
        [self.alr(), self.als()].into_iter().flatten().collect()
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub alr: Option<String>,
    pub gnat: Option<String>,
    pub gprbuild: Option<String>,
    pub gprclean: Option<String>,
    pub gnatprove: Option<String>,
    pub als: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let mut find = |name: &str| host.find_program(name, &hints(name));
        Tools { alr: find("alr"), gnat: find("gnat"), gprbuild: find("gprbuild"), gprclean: find("gprclean"), gnatprove: find("gnatprove"), als: find("ada_language_server") }
    }

    /// `alr`, by its path when found, asked nothing (`-n`): it never
    /// waits on a question no one can answer.
    pub fn alr(&self) -> Command {
        Command::new(self.alr.as_deref().unwrap_or("alr")).arg("-n")
    }

    /// A program by its path when found, else by its name.
    pub fn named(&self, name: &str) -> Command {
        let found = match name {
            "gprbuild" => &self.gprbuild,
            "gprclean" => &self.gprclean,
            "gnatprove" => &self.gnatprove,
            _ => &None,
        };
        Command::new(found.as_deref().unwrap_or(name))
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        match name {
            "alr" => Some(&mut self.alr),
            "gnat" => Some(&mut self.gnat),
            "gprbuild" => Some(&mut self.gprbuild),
            "gnatprove" => Some(&mut self.gnatprove),
            "ada_language_server" => Some(&mut self.als),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_lang_kit::archive::Kind;

    #[test]
    fn each_tool_is_installed_from_the_release_for_this_machine() {
        let linux = Platform::new("linux", "x86_64");
        let alr = linux.alr().unwrap();
        assert_eq!(alr.url, "https://github.com/alire-project/alire/releases/download/v2.1.1/alr-2.1.1-bin-x86_64-linux.zip");
        assert_eq!((alr.kind, alr.strip, alr.program.as_str()), (Kind::Zip, 0, "bin/alr"));
        assert_eq!(alr.sha256.as_deref(), Some("09c66bcd8c35dd4b97b72c3d9b76e44caa6964a2db35aba069f396f00f1f64c7"));
        let als = linux.als().unwrap();
        assert_eq!(als.url, "https://github.com/AdaCore/ada_language_server/releases/download/2027.0.202610121/als-2027.0.202610121-linux-x64.tar.gz");
        assert_eq!((als.kind, als.strip, als.program.as_str()), (Kind::TarGz, 5, "ada_language_server"), "integration/vscode/ada/x64/linux/ taken off");

        let mac = Platform::new("macos", "arm64");
        assert!(mac.alr().unwrap().url.ends_with("alr-2.1.1-bin-aarch64-macos.zip") && mac.als().unwrap().url.ends_with("-darwin-arm64.tar.gz"));
        let windows = Platform::new("windows", "x86_64");
        let names: Vec<(String, String)> = windows.installs().into_iter().map(|i| (i.name, i.program)).collect();
        assert_eq!(names, [("alr".into(), "bin/alr.exe".into()), ("als".into(), "ada_language_server.exe".into())]);
        for i in mac.installs().into_iter().chain(Platform::new("linux", "aarch64").installs()).chain(Platform::new("macos", "x86_64").installs()) {
            assert_eq!(i.sha256.as_deref().map(str::len), Some(64), "{}", i.name);
        }
        assert!(Platform::new("windows", "aarch64").installs().is_empty() && Platform::default().installs().is_empty());
    }

    #[test]
    fn gnat_is_looked_for_where_alire_and_its_installers_put_it() {
        assert_eq!(hints("alr")[0], "/tools/alr/bin");
        assert_eq!(hints("ada_language_server")[0], "/tools/als");
        assert_eq!(hints("gnatprove")[0], "$HOME/.alire/bin", "alr install gnatprove");
        assert!(hints("gnat").contains(&"/opt/GNAT/2021/bin".to_string()));
        let t = Tools { gprbuild: Some("/t/gprbuild".into()), ..Tools::default() };
        assert_eq!((t.named("gprbuild").program.as_str(), t.named("gprclean").program.as_str()), ("/t/gprbuild", "gprclean"));
        assert_eq!((t.alr().program.as_str(), t.alr().args.as_slice()), ("alr", &["-n".to_string()][..]));
        let mut t = Tools::default();
        *t.slot("ada_language_server").unwrap() = Some("/t/als".into());
        assert_eq!(t.als.as_deref(), Some("/t/als"));
        assert!(t.slot("go").is_none());
    }
}
