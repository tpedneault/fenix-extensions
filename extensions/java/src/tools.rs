//! The programs the pack runs, and the releases it installs into the
//! tools folder, each checked against its SHA-256: a Java runtime for the
//! servers (Temurin's JRE 25, which JDT LS needs), Eclipse JDT LS, the
//! java-debug plugin, kotlin-language-server, google-java-format and
//! ktlint.
//!
//! The project's JDK is found by `javac` -- the tools folder's runtime has
//! none, and Fenix looks there first -- in `java.jdk`, then `JAVA_HOME`,
//! then `PATH`.

use fenix_extension_api::Command;
use fenix_lang_kit::{Host, Install};

/// Eclipse JDT LS: the milestone, its file, its SHA-256, and the launcher
/// jar in it.
pub const JDTLS: &str = "1.63.0";
const JDTLS_FILE: &str = "jdt-language-server-1.63.0-202610191322.tar.gz";
const JDTLS_SHA256: &str = "b1f2abe0b3cccb462cf64fb4a5587c7d09df60cf8be7748b152cbd89d018bdc4";
pub const JDTLS_LAUNCHER: &str = "plugins/org.eclipse.equinox.launcher_1.8.0.v20260804-1928.jar";
/// The Java JDT LS runs on, at least.
pub const JDTLS_JAVA: u32 = 25;

/// java-debug's plugin for JDT LS, from Maven Central.
pub const JAVA_DEBUG: &str = "0.53.1";
const JAVA_DEBUG_SHA256: &str = "4f4778d452a6a0665536f43ce4e32403a24be6593336b80dc85a322912859e24";

/// kotlin-language-server's release (a zip of jars, any machine).
pub const KLS: &str = "1.3.13";
const KLS_SHA256: &str = "4fe7d71d087b307c7869036171bd9d8c6a4284cd7c25b89098b0a24eb2d9b6d2";

pub const KTLINT: &str = "1.8.0";
const KTLINT_SHA256: &str = "a3fd620207d5c40da6ca789b95e7f823c54e854b7fade7f613e91096a3706d75";

/// google-java-format: its native build for a machine that has one, else
/// its jar.
pub const GJF: &str = "1.37.0";
const GJF_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "google-java-format_linux-x86-64:881821ffd1a52b175886c4ebeeacce2ba5b5d7326b52d32a18d384d8a783bd77"),
    ("linux", "aarch64", "google-java-format_linux-arm64:e8c1c4ae92f5a86367500d78ff0d09ad12bf8ad213fa35b372468fcabf281bd3"),
    ("macos", "aarch64", "google-java-format_darwin-arm64:657cf1011c8dfb7e0dac08516db02b701436e59600b961077f83b89b69cc3495"),
    ("windows", "x86_64", "google-java-format_windows-x86-64.exe:48260bed87f6830bae44a7a27f66ce98f7d9fb245f500021bd7ae16c9e2daa06"),
];
const GJF_JAR_SHA256: &str = "834b2a0c38cb774953322a84b5ca3f2f40dd3156650b3cd44d3b744345962f7a";

/// Temurin's JRE 25 for each machine.
pub const JRE: &str = "25.0.5+8";
const JRE_FILES: &[(&str, &str, &str)] = &[
    ("linux", "x86_64", "OpenJDK25U-jre_x64_linux_hotspot_25.0.5_8.tar.gz:6fac98f0c8202efc3135a9e9e42aa61963ec42d1c5cbb40caf231e7b14a24ef6"),
    ("linux", "aarch64", "OpenJDK25U-jre_aarch64_linux_hotspot_25.0.5_8.tar.gz:963931723891302ebb2687178532c3d793d71e2577cd058061c7efb396140a86"),
    ("macos", "x86_64", "OpenJDK25U-jre_x64_mac_hotspot_25.0.5_8.tar.gz:806902c796c670969b7b27df73b8f0fe0924a61bd65083aa53064115e49cab2d"),
    ("macos", "aarch64", "OpenJDK25U-jre_aarch64_mac_hotspot_25.0.5_8.tar.gz:72ebd76b502c849e88dea39afae51baf5f82f2b5b1b6389c28ea707b20451bce"),
    ("windows", "x86_64", "OpenJDK25U-jre_x64_windows_hotspot_25.0.5_8.zip:da78d54a2fcdbba1e92d5e3f07dd30236f96297891c4a21a781c3f38f5c0beae"),
];

/// Where else to look for a JDK: the setting, then `JAVA_HOME`, then
/// where Linux distributions and macOS keep one.
pub const HINTS: &[&str] = &["${setting:java.jdk}", "$JAVA_HOME/bin", "%JAVA_HOME%\\bin", "/usr/lib/jvm/default-java/bin", "/usr/local/opt/openjdk/bin", "/opt/homebrew/opt/openjdk/bin"];

/// Where Gradle and Maven are kept when they aren't on `PATH`: SDKMAN!'s.
pub const BUILD_HINTS: &[&str] = &["$HOME/.sdkman/candidates/gradle/current/bin", "$HOME/.sdkman/candidates/maven/current/bin", "/opt/gradle/bin", "/opt/maven/bin"];

pub fn hints(extra: &[&str]) -> Vec<String> {
    HINTS.iter().chain(extra).map(|h| h.to_string()).collect()
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

    /// `java` (`java.exe`) in a runtime's `bin`.
    pub fn exe(&self, name: &str) -> String {
        if self.windows() {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }

    /// Temurin's JRE for it, into `/tools/jre`: none where there isn't
    /// one.
    pub fn jre(&self) -> Option<Install> {
        let (_, _, file) = JRE_FILES.iter().find(|(os, arch, _)| *os == self.os && *arch == self.arch)?;
        let (name, sha) = file.split_once(':')?;
        let java = self.jre_bin("java");
        let tag = JRE.replace('+', "%2B");
        Some(Install::new("jre", &format!("https://github.com/adoptium/temurin25-binaries/releases/download/jdk-{tag}/{name}")).label(&format!("Temurin JRE {JRE}")).sha256(sha).strip(1).program(&java))
    }

    /// The runtime's `java`, under the tools folder, as the pack names it
    /// once it's installed: `fenix-java`, so that looking for `java` finds
    /// the project's (kotlin-language-server's Kotlin can't run on Java
    /// 25) and this one is only what the pack asks for.
    pub fn jre_java(&self) -> String {
        format!("/tools/jre/{}", self.jre_bin("fenix-java"))
    }

    /// `bin/<name>` in the runtime, for this machine.
    pub fn jre_bin(&self, name: &str) -> String {
        match self.os.as_str() {
            "macos" => format!("Contents/Home/bin/{name}"),
            _ => format!("bin/{}", self.exe(name)),
        }
    }

    /// The runtime's `java` renamed `fenix-java`, in the tools folder at
    /// `tools`, when it's `java` (just installed, or installed again).
    pub fn hide_jre_java(&self, tools: &str) {
        let (from, to) = (format!("{tools}/jre/{}", self.jre_bin("java")), format!("{tools}/jre/{}", self.jre_bin("fenix-java")));
        if std::path::Path::new(&from).is_file() {
            let _ = std::fs::remove_file(&to);
            let _ = std::fs::rename(from, to);
        }
    }

    /// google-java-format: native here, else its jar (run by `java`).
    pub fn gjf(&self) -> Install {
        match GJF_FILES.iter().find(|(os, arch, _)| *os == self.os && *arch == self.arch).and_then(|(_, _, f)| f.split_once(':')) {
            Some((name, sha)) => Install::new("google-java-format", &format!("https://github.com/google/google-java-format/releases/download/v{GJF}/{name}"))
                .label(&format!("google-java-format {GJF}"))
                .sha256(sha)
                .program(&self.exe("google-java-format")),
            None => Install::new("google-java-format", &format!("https://github.com/google/google-java-format/releases/download/v{GJF}/google-java-format-{GJF}-all-deps.jar"))
                .label(&format!("google-java-format {GJF}"))
                .sha256(GJF_JAR_SHA256)
                .program("google-java-format.jar"),
        }
    }
}

/// Eclipse JDT LS, into `/tools/jdtls`.
pub fn jdtls() -> Install {
    Install::new("jdtls", &format!("https://download.eclipse.org/jdtls/milestones/{JDTLS}/{JDTLS_FILE}")).label(&format!("Eclipse JDT LS {JDTLS}")).sha256(JDTLS_SHA256).program("bin/jdtls")
}

/// java-debug's plugin, into `/tools/java-debug`.
pub fn java_debug() -> Install {
    let file = format!("com.microsoft.java.debug.plugin-{JAVA_DEBUG}.jar");
    Install::new("java-debug", &format!("https://repo1.maven.org/maven2/com/microsoft/java/com.microsoft.java.debug.plugin/{JAVA_DEBUG}/{file}")).label(&format!("java-debug {JAVA_DEBUG}")).sha256(JAVA_DEBUG_SHA256).program(&file)
}

/// kotlin-language-server, into `/tools/kotlin-language-server`.
pub fn kls() -> Install {
    Install::new("kotlin-language-server", &format!("https://github.com/fwcd/kotlin-language-server/releases/download/{KLS}/server.zip"))
        .label(&format!("kotlin-language-server {KLS}"))
        .sha256(KLS_SHA256)
        .strip(1)
        .program(&format!("lib/server-{KLS}.jar"))
}

/// ktlint: one file, a jar with a shell script before it -- run by `java
/// -jar`, so the same on every machine.
pub fn ktlint() -> Install {
    Install::new("ktlint", &format!("https://github.com/pinterest/ktlint/releases/download/{KTLINT}/ktlint")).label(&format!("ktlint {KTLINT}")).sha256(KTLINT_SHA256).program("ktlint")
}

/// What was found, by full path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    /// The project's JDK's `javac`.
    pub javac: Option<String>,
    /// The project's `java`: `java.jdk`'s, else `JAVA_HOME`'s or `PATH`'s.
    /// The tools folder's runtime is `fenix-java`, so it isn't this.
    pub java: Option<String>,
    /// The tools folder's runtime, `fenix-java`: JDT LS runs on it.
    pub jre: Option<String>,
    pub gradle: Option<String>,
    pub mvn: Option<String>,
    /// JetBrains' `kotlin-lsp`, when it's installed (Homebrew's, or a
    /// link on `PATH` to its script).
    pub kotlin_lsp: Option<String>,
    pub google_java_format: Option<String>,
    pub ktlint: Option<String>,
    /// JDT LS's own script, in the tools folder: where it is says the
    /// tools folder's full path.
    pub jdtls: Option<String>,
    /// The project's `gradlew` or `mvnw`, by its full path: Fenix runs a
    /// path it found, not `./gradlew`. On Windows, `gradlew.bat` and
    /// `mvnw.cmd` (0.8.7).
    pub wrapper: Option<String>,
}

impl Tools {
    pub fn find(host: &mut dyn Host) -> Tools {
        let jdk = hints(&[]);
        let build = hints(BUILD_HINTS);
        let none: Vec<String> = Vec::new();
        Tools {
            javac: host.find_program("javac", &jdk),
            java: host.find_program("java", &jdk),
            gradle: host.find_program("gradle", &build),
            mvn: host.find_program("mvn", &build),
            kotlin_lsp: host.find_program("kotlin-lsp", &none),
            google_java_format: host.find_program("google-java-format", &none),
            ktlint: host.find_program("ktlint", &none),
            jdtls: host.find_program("jdtls", &none),
            jre: host.find_program("fenix-java", &none),
            wrapper: None,
        }
    }

    /// The wrapper `name` (`gradlew`) in the project, its full path: the
    /// hint `./gradlew` is looked at under the project before `PATH`, and
    /// on Windows finds `gradlew.bat` (0.8.7). With the project's full
    /// path known (`root`), one found outside it isn't it.
    pub fn find_wrapper(&mut self, host: &mut dyn Host, name: Option<&str>, root: Option<&str>) {
        let norm = |p: &str| p.replace('\\', "/").to_ascii_lowercase();
        self.wrapper = name.and_then(|name| host.find_program(name, &[wrapper_hint(name)])).filter(|p| root.is_none_or(|r| norm(p).starts_with(&format!("{}/", norm(r).trim_end_matches('/')))));
    }

    /// The tools folder's full path, from a program found in it (JDT LS's
    /// script, the runtime's `java`, google-java-format): what a program
    /// the pack starts is told, since `${tools}` is filled in only in a
    /// command's arguments.
    pub fn tools_dir(&self) -> Option<String> {
        const IN: &[&str] = &["/jdtls/bin/jdtls", "/jre/bin/fenix-java", "/jre/bin/fenix-java.exe", "/jre/Contents/Home/bin/fenix-java", "/google-java-format/google-java-format", "/google-java-format/google-java-format.exe"];
        [&self.jdtls, &self.jre, &self.google_java_format].into_iter().flatten().find_map(|p| {
            let p = p.replace('\\', "/");
            IN.iter().find_map(|tail| p.strip_suffix(tail)).map(str::to_string)
        })
    }

    /// The JDK's home, from its `javac`: `<home>/bin/javac`. None for one
    /// on `PATH` by a link (`/usr/bin/javac`), whose home only it knows
    /// (`java.home`, which `javac -J-XshowSettings:properties` says).
    pub fn jdk_home(&self) -> Option<String> {
        let javac = self.javac.as_deref()?.replace('\\', "/");
        let bin = javac.rsplit_once('/')?.0;
        let home = bin.strip_suffix("/bin")?;
        (!["/usr", "/usr/local", ""].contains(&home)).then(|| home.to_string())
    }
}

/// The hint that finds the project's own wrapper `name` (0.8.7).
pub fn wrapper_hint(name: &str) -> String {
    format!("./{name}")
}

/// `java.home` in what `javac -J-XshowSettings:properties -version` says.
pub fn java_home(text: &str) -> Option<String> {
    text.lines().find_map(|l| l.trim().strip_prefix("java.home = ")).map(|h| h.trim().replace('\\', "/"))
}

/// The major version in what `java -version` (`javac -version`) says:
/// `openjdk version "21.0.2"`, `javac 17.0.9`, `java version "1.8.0_402"`.
pub fn java_major(text: &str) -> Option<u32> {
    let v = fenix_lang_kit::find_version(text)?;
    match v.major() {
        1 => Some(v.number(1) as u32),
        m => Some(m as u32),
    }
}

/// kotlin-language-server, run by `java` with its jars.
pub fn kls_command(java: &str) -> Command {
    Command::new(java).args(["-Xss2m", &format!("-DkotlinLanguageServer.version={KLS}"), "-cp", "${tools}/kotlin-language-server/lib/*", "org.javacs.kt.MainKt"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_release_is_the_one_for_this_machine() {
        let linux = Platform::new("linux", "x86_64");
        let jre = linux.jre().unwrap();
        assert_eq!(jre.url, "https://github.com/adoptium/temurin25-binaries/releases/download/jdk-25.0.5%2B8/OpenJDK25U-jre_x64_linux_hotspot_25.0.5_8.tar.gz");
        assert_eq!((jre.strip, jre.program.as_str()), (1, "bin/java"));
        assert_eq!(linux.jre_java(), "/tools/jre/bin/fenix-java");
        let mac = Platform::new("macos", "aarch64");
        assert_eq!(mac.jre().unwrap().program, "Contents/Home/bin/java");
        assert_eq!(mac.jre_java(), "/tools/jre/Contents/Home/bin/fenix-java");
        let windows = Platform::new("windows", "x86_64");
        assert!(windows.jre().unwrap().url.ends_with(".zip") && windows.jre_java().ends_with("bin/fenix-java.exe"));
        assert_eq!(Platform::new("windows", "aarch64").jre(), None);
        assert_eq!(linux.gjf().program, "google-java-format");
        assert!(windows.gjf().program.ends_with(".exe"));
        assert_eq!(Platform::new("macos", "x86_64").gjf().program, "google-java-format.jar", "no native build: the jar");
        for i in [jdtls(), java_debug(), kls(), ktlint(), linux.gjf(), jre] {
            assert_eq!(i.sha256.as_deref().map(str::len), Some(64), "{}", i.name);
        }
        assert_eq!(jdtls().url, "https://download.eclipse.org/jdtls/milestones/1.63.0/jdt-language-server-1.63.0-202610191322.tar.gz");
        assert_eq!(kls().program, "lib/server-1.3.13.jar");
    }

    #[test]
    fn the_jdk_and_its_home() {
        assert_eq!(java_major("openjdk version \"21.0.12.1\" 2026-08-18"), Some(21));
        assert_eq!(java_major("javac 17.0.9"), Some(17));
        assert_eq!(java_major("java version \"1.8.0_402\""), Some(8));
        assert_eq!(java_home("    java.class.path = \n    java.home = /usr/lib/jvm/java-21-openjdk-amd64\n"), Some("/usr/lib/jvm/java-21-openjdk-amd64".into()));
        let t = |javac: &str| Tools { javac: Some(javac.into()), ..Tools::default() }.jdk_home();
        assert_eq!(t("/usr/lib/jvm/temurin-21/bin/javac"), Some("/usr/lib/jvm/temurin-21".into()));
        assert_eq!(t("C:\\Program Files\\Java\\jdk-21\\bin\\javac.exe"), Some("C:/Program Files/Java/jdk-21".into()));
        assert_eq!(t("/usr/bin/javac"), None, "a link: its home is javac's to say");
        let tools = Tools { jdtls: Some("/home/u/.local/share/fenix/tools/fenix.java/jdtls/bin/jdtls".into()), ..Tools::default() };
        assert_eq!(tools.tools_dir().as_deref(), Some("/home/u/.local/share/fenix/tools/fenix.java"));
        let windows = Tools { jre: Some("C:\\Users\\u\\AppData\\fenix\\tools\\fenix.java\\jre\\bin\\fenix-java.exe".into()), ..Tools::default() };
        assert_eq!(windows.tools_dir().as_deref(), Some("C:/Users/u/AppData/fenix/tools/fenix.java"));
        assert_eq!(Tools { java: Some("/usr/bin/java".into()), ..Tools::default() }.tools_dir(), None);
        // Its runtime's java, renamed once it's installed.
        let dir = crate::project::tests::temp("hide-jre");
        crate::project::tests::write(&dir, "jre/bin/java", "ELF");
        Platform::new("linux", "x86_64").hide_jre_java(&dir.to_string_lossy());
        assert!(dir.join("jre/bin/fenix-java").is_file() && !dir.join("jre/bin/java").exists());
        Platform::new("linux", "x86_64").hide_jre_java(&dir.to_string_lossy());
        assert!(dir.join("jre/bin/fenix-java").is_file());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_wrapper_is_the_projects_and_a_bat_on_windows() {
        use fenix_lang_kit::host::mock::Mock;
        let mut host = Mock::default();
        let mut t = Tools::default();
        assert_eq!(wrapper_hint("gradlew"), "./gradlew");
        host.programs.insert("gradlew".into(), "C:\\Users\\u\\orbit\\gradlew.bat".into());
        t.find_wrapper(&mut host, Some("gradlew"), Some("c:/Users/u/orbit"));
        assert_eq!(t.wrapper.as_deref(), Some("C:\\Users\\u\\orbit\\gradlew.bat"), "the drive's case aside");
        host.programs.insert("gradlew".into(), "/usr/local/bin/gradlew".into());
        t.find_wrapper(&mut host, Some("gradlew"), Some("/home/u/orbit"));
        assert_eq!(t.wrapper, None, "not the project's");
        host.programs.insert("mvnw".into(), "/home/u/orbit/mvnw".into());
        t.find_wrapper(&mut host, Some("mvnw"), None);
        assert_eq!(t.wrapper.as_deref(), Some("/home/u/orbit/mvnw"), "the project's path not known yet: the hint found it");
        t.find_wrapper(&mut host, Some("mvnw"), Some("/home/u/orbit-two"));
        assert_eq!(t.wrapper, None, "a folder beside it isn't it");
        t.find_wrapper(&mut host, None, Some("/home/u/orbit"));
        assert_eq!(t.wrapper, None, "no build");
    }
}
