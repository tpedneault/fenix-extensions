//! The programs the pack runs, found on this machine, and the one it
//! installs into its tools folder when there isn't one: Coursier.
//!
//! Metals isn't a release binary. Its maintainers' route is Coursier --
//! `cs install metals` writes a small launcher that runs it from
//! Coursier's cache -- so the pack looks for a Metals that's there
//! (`PATH`, Coursier's bin folder, its own tools folder) and, where there
//! isn't one, offers two fix rows: Coursier, and then Metals through it. An
//! editor started from the desktop may lack the folders those put things
//! in, so the pack looks in them, not only on `PATH`.

use fenix_extension_api::Command;
use fenix_lang_kit::archive::Kind;
use fenix_lang_kit::{Host, Install};

/// Metals: the stable release `cs install` is asked for, so every machine
/// has the one the pack was tried with. Metals 1.6 runs on Java 17 or later
/// (its classes are Java 17's).
pub const METALS: &str = "1.6.9";

/// The oldest Java Metals runs on.
pub const METALS_JAVA: u32 = 17;

/// Coursier's release, and the SHA-256 of each file. Coursier publishes
/// no checksum file with its releases, so these are the hashes of the
/// files as they were downloaded from the release (see the README).
pub const COURSIER: &str = "2.1.26";
const COURSIER_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "cs-x86_64-pc-linux.gz:348e37bc2a8c706640e6b032c4551a0e055a7f1537485b4a91540e9d3598ec6d"),
    ("linux", "aarch64", "cs-aarch64-pc-linux.gz:6c358ac223332c8981eb86417d3faf9338a5ef6c95d1879a58162b24c510025c"),
    ("macos", "x86_64", "cs-x86_64-apple-darwin.gz:c8045f36fadec075c8647b979d1028435935b0ecd43efa1387cd286c3926a421"),
    ("macos", "aarch64", "cs-aarch64-apple-darwin.gz:8e4d36aa2565276f262af073f0999399e4c2f920f2fe436bb0ae67665eb505e9"),
    ("windows", "x86_64", "cs-x86_64-pc-win32.zip:d23f4ba6afa7f092135b485303b0a020358fbb314abf9c6dc2576df3e2abd941"),
];

/// scalafmt's native build, the release and the SHA-256 of each file. A native
/// scalafmt formats only with the version it is (`.scalafmt.conf` must pin
/// it), and starts in milliseconds, so it can format on every save. The
/// release has no checksum file either: these are the files as downloaded.
pub const SCALAFMT: &str = "3.11.5";
const SCALAFMT_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "scalafmt-x86_64-pc-linux.zip:7b0e44cc193eeff06daa14b394dc0489d8eaa4537eb945522f7f267641ed8c2b"),
    ("linux", "aarch64", "scalafmt-aarch64-pc-linux.zip:691576a60e00f4bca150d251395196a382f0828697a9b1b4c9c44815eaea80bc"),
    ("macos", "x86_64", "scalafmt-x86_64-apple-darwin.zip:33d012e18f322ac9855496bb0f6c6528f5d880d8ba225e5031ec24fff6963394"),
    ("macos", "aarch64", "scalafmt-aarch64-apple-darwin.zip:8ccd72f826dd33b5c7ab5e0e01944044d041aa8c3c49cbda3caac31930483d5d"),
    ("windows", "x86_64", "scalafmt-x86_64-pc-win32.zip:d420e6a7205cd87520dc09160e3c420dac7a9782150054b1c42cf2e6c98030db"),
];

/// Where else to look: Coursier's bin folder on each system (and
/// `COURSIER_BIN_DIR`), the user's own, Homebrew's, SDKMAN!'s and
/// Chocolatey's.
pub const HINTS: &[&str] = &[
    "$COURSIER_BIN_DIR",
    "$HOME/.local/share/coursier/bin",
    "$HOME/Library/Application Support/Coursier/bin",
    "%LOCALAPPDATA%\\Coursier\\data\\bin",
    "$HOME/.local/bin",
    "$HOME/bin",
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/snap/bin",
    "$HOME/.sdkman/candidates/sbt/current/bin",
    "$HOME/.sdkman/candidates/mill/current/bin",
    "$HOME/.sdkman/candidates/scalacli/current/bin",
    "%ProgramData%\\chocolatey\\bin",
];

/// Where a JDK is looked for: the setting, then `JAVA_HOME`, then where
/// Linux distributions and macOS keep one -- as the Java pack looks.
pub const JDK_HINTS: &[&str] = &["${setting:scala.jdk}", "$JAVA_HOME/bin", "%JAVA_HOME%\\bin", "/usr/lib/jvm/default-java/bin", "/usr/local/opt/openjdk/bin", "/opt/homebrew/opt/openjdk/bin"];

/// Where `name` is looked for: its own folder under the tools folder first,
/// where the pack installs it, then [`HINTS`].
pub fn hints(name: &str) -> Vec<String> {
    std::iter::once(format!("/tools/{name}")).chain(HINTS.iter().map(|h| h.to_string())).collect()
}

/// Coursier's: `cs` (or `coursier`) is in its own folder, `/tools/coursier`.
pub fn coursier_hints() -> Vec<String> {
    hints("coursier")
}

/// Where the project's own wrapper script, then the rest, are looked for.
pub fn hints_with(first: &[&str], name: &str) -> Vec<String> {
    first.iter().map(|h| h.to_string()).chain(hints(name)).collect()
}

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

    pub fn windows(&self) -> bool {
        self.os == "windows"
    }

    /// Coursier's release for this machine, into `/tools/coursier`: a
    /// native launcher, `cs` (`cs.exe` from Windows' zip), that fetches the
    /// rest. None where there's no release: Windows on ARM.
    pub fn coursier(&self) -> Option<Install> {
        let arch = if self.arch == "arm64" { "aarch64" } else { self.arch.as_str() };
        let (_, _, file) = COURSIER_FILES.iter().find(|(os, a, _)| *os == self.os && *a == arch)?;
        let (name, sha) = file.split_once(':')?;
        let url = format!("https://github.com/coursier/coursier/releases/download/v{COURSIER}/{name}");
        let install = Install::new("coursier", &url).label(&format!("Coursier {COURSIER}")).sha256(sha);
        Some(match self.windows() {
            true => install.kind(Kind::Zip).program("cs-x86_64-pc-win32.exe"),
            false => install.program("cs"),
        })
    }

    /// scalafmt's native release for this machine, into `/tools/scalafmt`: the
    /// program alone in the zip. None where there's no release: Windows on ARM.
    pub fn scalafmt(&self) -> Option<Install> {
        let arch = if self.arch == "arm64" { "aarch64" } else { self.arch.as_str() };
        let (_, _, file) = SCALAFMT_FILES.iter().find(|(os, a, _)| *os == self.os && *a == arch)?;
        let (name, sha) = file.split_once(':')?;
        let url = format!("https://github.com/scalameta/scalafmt/releases/download/v{SCALAFMT}/{name}");
        let program = if self.windows() { "scalafmt.exe" } else { "scalafmt" };
        Some(Install::new("scalafmt", &url).label(&format!("scalafmt {SCALAFMT}")).sha256(sha).kind(Kind::Zip).program(program))
    }

    /// Every release there's one of for this machine.
    pub fn installs(&self) -> Vec<Install> {
        self.coursier().into_iter().chain(self.scalafmt()).collect()
    }
}

/// `cs install` for Metals: into the pack's tools folder (`${tools}`),
/// where it's found again, as a task so its progress is seen.
pub fn install_metals(cs: &str) -> Command {
    Command::new(cs).args(["install".to_string(), "--install-dir".into(), "${tools}/metals".into(), format!("metals:{METALS}")])
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    /// The JDK's `javac`: a runtime has none, and Metals needs a JDK.
    pub javac: Option<String>,
    pub coursier: Option<String>,
    pub metals: Option<String>,
    pub scala_cli: Option<String>,
    pub sbt: Option<String>,
    /// Mill: the project's `./mill` wrapper, else its own.
    pub mill: Option<String>,
    pub gradle: Option<String>,
    pub maven: Option<String>,
    /// Which of Mill, Gradle and Maven were found as the project's own wrapper
    /// (`./mill`, `./gradlew`, `./mvnw`), not on `PATH`: a file of the project's,
    /// which runs when you run a task and is never run to ask its version.
    pub wrapped: Vec<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let jdk: Vec<String> = JDK_HINTS.iter().map(|h| h.to_string()).collect();
        let javac = host.find_program("javac", &jdk);
        let coursier = host.find_program("cs", &coursier_hints()).or_else(|| host.find_program("coursier", &coursier_hints()));
        let mut find = |name: &str, first: &[&str]| host.find_program(name, &hints_with(first, name));
        let mut tools = Tools { javac, coursier, metals: find("metals", &[]), scala_cli: find("scala-cli", &[]), sbt: find("sbt", &[]), ..Tools::default() };
        // The project's wrapper, for the tasks you run, is looked for -- never
        // run: if finding it with its folder named gives something other than
        // finding the program without, it's the project's file.
        for (name, wrapper) in [("mill", "./mill"), ("gradle", "./gradlew"), ("mvn", "./mvnw")] {
            let plain = find(name, &[]);
            let task = find(name, &[wrapper]);
            if task != plain {
                tools.wrapped.push(name.to_string());
            }
            match name {
                "mill" => tools.mill = task,
                "gradle" => tools.gradle = task,
                _ => tools.maven = task,
            }
        }
        tools
    }

    /// Whether `name` (`mill`, `gradle`, `mvn`) is the project's own wrapper.
    pub fn is_wrapper(&self, name: &str) -> bool {
        self.wrapped.iter().any(|w| w == name)
    }

    /// The slot each program found goes in, by its name.
    pub fn slot(&mut self, name: &str) -> Option<&mut Option<String>> {
        match name {
            "javac" => Some(&mut self.javac),
            "cs" | "coursier" => Some(&mut self.coursier),
            "metals" => Some(&mut self.metals),
            "scala-cli" => Some(&mut self.scala_cli),
            "sbt" => Some(&mut self.sbt),
            "mill" => Some(&mut self.mill),
            "gradle" => Some(&mut self.gradle),
            "mvn" => Some(&mut self.maven),
            _ => None,
        }
    }

    /// A JDK's home from its `javac`: `/usr/lib/jvm/java-21` for
    /// `/usr/lib/jvm/java-21/bin/javac`.
    pub fn jdk_home(&self) -> Option<String> {
        let javac = self.javac.as_deref()?.replace('\\', "/");
        let bin = javac.rsplit_once('/')?.0;
        bin.strip_suffix("/bin").map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coursier_is_installed_from_the_release_for_this_machine() {
        let linux = Platform::new("linux", "x86_64").coursier().unwrap();
        assert_eq!(linux.url, "https://github.com/coursier/coursier/releases/download/v2.1.26/cs-x86_64-pc-linux.gz");
        assert_eq!((linux.kind, linux.strip, linux.program.as_str(), linux.name.as_str()), (Kind::Gz, 0, "cs", "coursier"), "a lone file, written as cs");
        assert_eq!(linux.sha256.as_deref(), Some("348e37bc2a8c706640e6b032c4551a0e055a7f1537485b4a91540e9d3598ec6d"));
        assert_eq!(linux.label, "Coursier 2.1.26");

        let arm = Platform::new("linux", "aarch64").coursier().unwrap();
        assert!(arm.url.ends_with("/cs-aarch64-pc-linux.gz"), "{}", arm.url);
        let mac = Platform::new("macos", "arm64").coursier().unwrap();
        assert!(mac.url.ends_with("/cs-aarch64-apple-darwin.gz"), "{}", mac.url);
        let intel = Platform::new("macos", "x86_64").coursier().unwrap();
        assert!(intel.url.ends_with("/cs-x86_64-apple-darwin.gz"), "{}", intel.url);
        let windows = Platform::new("windows", "x86_64").coursier().unwrap();
        assert_eq!((windows.kind, windows.program.as_str()), (Kind::Zip, "cs-x86_64-pc-win32.exe"));
        assert!(windows.url.ends_with("/cs-x86_64-pc-win32.zip"), "{}", windows.url);

        for (os, arch) in [("linux", "x86_64"), ("linux", "aarch64"), ("macos", "x86_64"), ("macos", "aarch64"), ("windows", "x86_64")] {
            let i = Platform::new(os, arch).coursier().unwrap_or_else(|| panic!("{os} {arch}"));
            assert_eq!(i.sha256.as_deref().map(str::len), Some(64), "{os} {arch}");
            assert!(i.url.starts_with("https://github.com/coursier/coursier/releases/download/v2.1.26/"), "{}", i.url);
        }
        assert!(Platform::new("windows", "aarch64").installs().is_empty(), "neither has a Windows on ARM release");
        assert!(Platform::new("linux", "riscv64").installs().is_empty() && Platform::default().installs().is_empty());
    }

    #[test]
    fn scalafmt_is_installed_from_its_native_release_for_this_machine() {
        let linux = Platform::new("linux", "x86_64").scalafmt().unwrap();
        assert_eq!(linux.url, "https://github.com/scalameta/scalafmt/releases/download/v3.11.5/scalafmt-x86_64-pc-linux.zip");
        assert_eq!((linux.kind, linux.strip, linux.program.as_str(), linux.name.as_str(), linux.label.as_str()), (Kind::Zip, 0, "scalafmt", "scalafmt", "scalafmt 3.11.5"));
        assert_eq!(linux.sha256.as_deref(), Some("7b0e44cc193eeff06daa14b394dc0489d8eaa4537eb945522f7f267641ed8c2b"));
        assert_eq!(Platform::new("windows", "x86_64").scalafmt().unwrap().program, "scalafmt.exe");
        assert!(Platform::new("macos", "arm64").scalafmt().unwrap().url.ends_with("/scalafmt-aarch64-apple-darwin.zip"));
        for (os, arch) in [("linux", "x86_64"), ("linux", "aarch64"), ("macos", "x86_64"), ("macos", "aarch64"), ("windows", "x86_64")] {
            let i = Platform::new(os, arch).scalafmt().unwrap_or_else(|| panic!("{os} {arch}"));
            assert_eq!(i.sha256.as_deref().map(str::len), Some(64), "{os} {arch}");
            assert!(i.url.starts_with("https://github.com/scalameta/scalafmt/releases/download/v3.11.5/"), "{}", i.url);
            assert_eq!(Platform::new(os, arch).installs().len(), 2, "Coursier and scalafmt");
        }
    }

    #[test]
    fn metals_is_installed_by_coursier_into_the_tools_folder() {
        let cmd = install_metals("/tools/coursier/cs");
        assert_eq!(fenix_test_kit::line(&cmd), "/tools/coursier/cs install --install-dir ${tools}/metals metals:1.6.9");
    }

    #[test]
    fn programs_are_looked_for_where_the_managers_put_them() {
        assert_eq!(hints("metals")[0], "/tools/metals");
        assert!(hints("metals").contains(&"$HOME/.local/share/coursier/bin".to_string()));
        assert!(hints("scala-cli").contains(&"%LOCALAPPDATA%\\Coursier\\data\\bin".to_string()));
        assert_eq!(hints_with(&["./mill"], "mill")[0], "./mill");
        assert!(JDK_HINTS.contains(&"${setting:scala.jdk}"));
        let mut t = Tools::default();
        *t.slot("metals").unwrap() = Some("/t/metals".into());
        assert_eq!(t.metals.as_deref(), Some("/t/metals"));
        assert!(t.slot("go").is_none());
        let t = Tools { javac: Some("/usr/lib/jvm/java-21/bin/javac".into()), ..Tools::default() };
        assert_eq!(t.jdk_home().as_deref(), Some("/usr/lib/jvm/java-21"));
        let t = Tools { javac: Some("C:\\jdk\\bin\\javac.exe".into()), ..Tools::default() };
        assert_eq!(t.jdk_home().as_deref(), Some("C:/jdk"));
        assert_eq!(Tools { javac: Some("javac".into()), ..Tools::default() }.jdk_home(), None);
    }
}
