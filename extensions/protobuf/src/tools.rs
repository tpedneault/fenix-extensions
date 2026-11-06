//! The programs the pack runs, found on this machine: buf (the language
//! server, the formatter, lint, breaking, generate and build), grpcurl
//! (the gRPC client), protoc and protols; and the GitHub releases of buf,
//! grpcurl and protoc it installs into its tools folder when they aren't
//! here already. `go install`, Homebrew, Scoop and winget put them in
//! folders an editor started from the desktop may not have on its `PATH`,
//! so they're looked in too.

use fenix_extension_api::Command;
use fenix_lang_kit::archive::Kind;
use fenix_lang_kit::{Host, Install};

/// Where else to look: Go's, the user's own bin folders, Homebrew's,
/// Scoop's and winget's.
pub const HINTS: &[&str] = &[
    "$HOME/go/bin", "$HOME/.local/bin", "$HOME/bin", "$HOME/.cargo/bin", "/opt/homebrew/bin", "/usr/local/bin", "/snap/bin",
    "%USERPROFILE%\\go\\bin", "%USERPROFILE%\\scoop\\shims", "%USERPROFILE%\\.cargo\\bin", "%LOCALAPPDATA%\\Microsoft\\WinGet\\Links",
];

/// Where `name` is looked for: its own folder under the tools folder
/// first, where the pack installs it, then [`HINTS`].
pub fn hints(name: &str) -> Vec<String> {
    std::iter::once(format!("/tools/{name}")).chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// buf's release: the program itself, a file for each machine, as its
/// `sha256.txt` gives them.
pub const BUF: &str = "1.75.0";
const BUF_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "buf-Linux-x86_64:fad1e38d6909bbb583a4da81d8c02a01046e2cd663bde7e337dd24277393764b"),
    ("linux", "aarch64", "buf-Linux-aarch64:00eb190aa82df17f393e05b04889cc60a0b13b3259d336e21a87d5f23ab4fd26"),
    ("macos", "x86_64", "buf-Darwin-x86_64:ff6d0979143c661d59178a032c57be883b2ac03dc9bf7d00ffca222b0354bd46"),
    ("macos", "aarch64", "buf-Darwin-arm64:f71b009ebd8530b12fd85b060c6c245e15d6e5110ccc8da48d8b6fa71e59330b"),
    ("windows", "x86_64", "buf-Windows-x86_64.exe:de35b288e7962ac72aefee5048aff80551a4f0e6ac865a21f838693461ac8b8d"),
    ("windows", "aarch64", "buf-Windows-arm64.exe:57fdfac407c0538707a7d8f7ddc13fcd8770e3c58dcffe7d55e7d8f1804cca61"),
];

/// grpcurl's release, as its `grpcurl_<v>_checksums.txt` gives them: a
/// tar.gz (a zip on Windows) with the program at its top.
pub const GRPCURL: &str = "1.9.4";
const GRPCURL_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "linux_x86_64.tar.gz:97e13d58d2733a0e62cd2571d1d5f0c02823f0d25282f08bddedf1ad9c5d1736"),
    ("linux", "aarch64", "linux_arm64.tar.gz:ad66227d90631da5428b4a5ccf28d63846f0f15649d8b2367df044a59edbb617"),
    ("macos", "x86_64", "osx_x86_64.tar.gz:0c1b24a82097862027af6abe88f362db73e4846859a3ceda2dd466e4dc971a06"),
    ("macos", "aarch64", "osx_arm64.tar.gz:e0df111350acf8ee38f453f4e97e0474cfe1a987ee0c368ebe5383e6a4bccf25"),
    ("windows", "x86_64", "windows_x86_64.zip:412dbe98dc6f3fddfbdc15db061768495d3b4c6ee24de998c83c36f79857680d"),
    ("windows", "aarch64", "windows_arm64.zip:f42f6646bc259989cc8e8b7ecb3b987456ea24dd103361bcabb61534f2c77189"),
];

/// protoc's release: a zip with `bin/protoc` and the well-known types'
/// `include/`. The release has no checksums of its own; these are its
/// files'. There's no Windows on Arm build.
pub const PROTOC: &str = "36.2";
const PROTOC_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "linux-x86_64:121f6c7afe1d4d0e3ea6aab9432038599250134cbf4474cb1167d2c7decd4278"),
    ("linux", "aarch64", "linux-aarch_64:8b8f18bd2b30346efbc698dd5a73dd7c805f3ef8380f6dfc95c768f3f1852f6a"),
    ("macos", "x86_64", "osx-x86_64:228cc7add4616cc14ca5e80dee83209d44449a7aee95a914ae748fa374efb078"),
    ("macos", "aarch64", "osx-aarch_64:9cd98a532c5c5e0c4161314de0225de27e4c8a323917b6ea7b1b714d3ae23466"),
    ("windows", "x86_64", "win64:f0c128dc0d8492eceece83bb459a4c0e316764b929ffbf1aa416357fd644edd3"),
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

    /// `name` as a program here: `buf.exe` on Windows.
    pub fn exe(&self, name: &str) -> String {
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

    /// buf for this machine, into `/tools/buf`: the program, downloaded
    /// as it is.
    pub fn buf(&self) -> Option<Install> {
        let (file, sha) = self.file(BUF_FILES)?;
        let url = format!("https://github.com/bufbuild/buf/releases/download/v{BUF}/{file}");
        Some(Install::new("buf", &url).label(&format!("buf {BUF}")).sha256(sha).kind(Kind::Plain).program(&self.exe("buf")))
    }

    /// grpcurl for this machine, into `/tools/grpcurl`.
    pub fn grpcurl(&self) -> Option<Install> {
        let (file, sha) = self.file(GRPCURL_FILES)?;
        let url = format!("https://github.com/fullstorydev/grpcurl/releases/download/v{GRPCURL}/grpcurl_{GRPCURL}_{file}");
        Some(Install::new("grpcurl", &url).label(&format!("grpcurl {GRPCURL}")).sha256(sha).program(&self.exe("grpcurl")))
    }

    /// protoc for this machine, into `/tools/protoc`: `bin/protoc` and
    /// its `include/`.
    pub fn protoc(&self) -> Option<Install> {
        let (file, sha) = self.file(PROTOC_FILES)?;
        let url = format!("https://github.com/protocolbuffers/protobuf/releases/download/v{PROTOC}/protoc-{PROTOC}-{file}.zip");
        Some(Install::new("protoc", &url).label(&format!("protoc {PROTOC}")).sha256(sha).program(&format!("bin/{}", self.exe("protoc"))))
    }

    /// Every release there's one of for this machine.
    pub fn installs(&self) -> Vec<Install> {
        [self.buf(), self.grpcurl(), self.protoc()].into_iter().flatten().collect()
    }
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub buf: Option<String>,
    pub grpcurl: Option<String>,
    pub protoc: Option<String>,
    pub protols: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let mut find = |name: &str| host.find_program(name, &hints(name));
        Tools { buf: find("buf"), grpcurl: find("grpcurl"), protoc: find("protoc"), protols: find("protols") }
    }

    /// buf by its path when found, else by its name.
    pub fn buf(&self) -> Command {
        Command::new(self.buf.as_deref().unwrap_or("buf"))
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        match name {
            "buf" => Some(&mut self.buf),
            "grpcurl" => Some(&mut self.grpcurl),
            "protoc" => Some(&mut self.protoc),
            "protols" => Some(&mut self.protols),
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
        let buf = linux.buf().unwrap();
        assert_eq!(buf.url, "https://github.com/bufbuild/buf/releases/download/v1.75.0/buf-Linux-x86_64");
        assert_eq!((buf.kind, buf.program.as_str()), (Kind::Plain, "buf"), "the program itself, written as buf");
        assert_eq!(buf.sha256.as_deref(), Some("fad1e38d6909bbb583a4da81d8c02a01046e2cd663bde7e337dd24277393764b"));
        let grpcurl = linux.grpcurl().unwrap();
        assert_eq!(grpcurl.url, "https://github.com/fullstorydev/grpcurl/releases/download/v1.9.4/grpcurl_1.9.4_linux_x86_64.tar.gz");
        assert_eq!((grpcurl.kind, grpcurl.strip, grpcurl.program.as_str()), (Kind::TarGz, 0, "grpcurl"));
        let protoc = linux.protoc().unwrap();
        assert_eq!(protoc.url, "https://github.com/protocolbuffers/protobuf/releases/download/v36.2/protoc-36.2-linux-x86_64.zip");
        assert_eq!((protoc.kind, protoc.program.as_str()), (Kind::Zip, "bin/protoc"));

        let mac = Platform::new("macos", "arm64");
        assert!(mac.buf().unwrap().url.ends_with("buf-Darwin-arm64") && mac.grpcurl().unwrap().url.ends_with("_osx_arm64.tar.gz") && mac.protoc().unwrap().url.ends_with("osx-aarch_64.zip"));
        for p in [Platform::new("linux", "aarch64"), Platform::new("macos", "x86_64"), Platform::new("windows", "x86_64"), mac] {
            for i in p.installs() {
                assert_eq!(i.sha256.as_deref().map(str::len), Some(64), "{} on {:?}", i.name, p);
            }
        }
        let windows = Platform::new("windows", "x86_64");
        let rows: Vec<(String, String, Kind)> = windows.installs().into_iter().map(|i| (i.name, i.program, i.kind)).collect();
        assert_eq!(rows, [("buf".into(), "buf.exe".into(), Kind::Plain), ("grpcurl".into(), "grpcurl.exe".into(), Kind::Zip), ("protoc".into(), "bin/protoc.exe".into(), Kind::Zip)]);
        assert_eq!(Platform::new("windows", "aarch64").installs().len(), 2, "no protoc for Windows on Arm");
        assert!(Platform::new("linux", "riscv64").installs().is_empty() && Platform::default().installs().is_empty());
    }

    #[test]
    fn tools_are_looked_for_where_go_and_the_managers_put_them() {
        assert_eq!(hints("buf")[0], "/tools/buf");
        assert!(hints("grpcurl").contains(&"$HOME/go/bin".to_string()));
        assert_eq!(Tools::default().buf().program, "buf");
        let mut t = Tools::default();
        *t.slot("grpcurl").unwrap() = Some("/t/grpcurl".into());
        assert_eq!(t.grpcurl.as_deref(), Some("/t/grpcurl"));
        assert!(t.slot("zig").is_none());
    }
}
