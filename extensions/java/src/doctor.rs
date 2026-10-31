//! The project doctor's Java checks: the JDK (held to the release the
//! build asks for), the wrapper (and whether its Gradle runs on that
//! JDK), the Java the servers run on, JDT LS, java-debug, the Kotlin
//! server, google-java-format and ktlint. Each that isn't right has a
//! fix row.

use fenix_extension_api::{Check, Health, Section};
use fenix_lang_kit::{Fix, Program, Version};

use crate::project::{Build, Tool};
use crate::tools::{hints, JDTLS, JDTLS_JAVA, JRE};

/// The fix rows' commands, in the manifest.
pub const INSTALL_JDK: &str = "java.install-jdk";
pub const WRAPPER: &str = "java.wrapper";

/// The Gradle a wrapper makes when it's made again.
pub const GRADLE: &str = "9.8.2";
/// Maven's, for `mvnw`.
pub const MAVEN: &str = "3.9.11";

/// The newest Java each Gradle runs on (Gradle's compatibility matrix).
const GRADLE_JAVA: &[(&str, u32)] = &[("9.1", 25), ("8.14", 24), ("8.10", 23), ("8.8", 22), ("8.5", 21), ("8.3", 20), ("7.6", 19), ("7.5", 18), ("7.3", 17)];

/// The newest Java Gradle `version` runs on, when it's one the table
/// knows.
pub fn gradle_runs_on(version: &str) -> Option<u32> {
    let v = Version::parse(version)?;
    GRADLE_JAVA.iter().find(|(min, _)| Version::parse(min).is_some_and(|m| v >= m)).map(|(_, java)| *java)
}

/// The Temurin download page for a release.
pub fn jdk_page(release: Option<u32>) -> String {
    match release {
        Some(r) => format!("https://adoptium.net/temurin/releases/?version={r}"),
        None => "https://adoptium.net/temurin/releases/".into(),
    }
}

/// The JDK, held to the build's release: `javac`, since a runtime has
/// none.
pub fn programs(build: &Build) -> Vec<Program> {
    let jdk = hints(&[]).into_iter().fold(Program::new("javac", "JDK"), Program::hint).version_args(["-version"]);
    let jdk = match build.release {
        Some(r) => jdk.at_least(&r.to_string()),
        None => jdk,
    };
    let label = match build.release {
        Some(r) => format!("Get JDK {r}"),
        None => "Get a JDK".into(),
    };
    vec![jdk.fix(&label, INSTALL_JDK, Fix::Own)]
}

/// What the doctor's checks of its own know.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// The JDK's major version, as `javac` said it.
    pub jdk: Option<u32>,
    /// The `java` the servers run on, and its major version.
    pub java: Option<String>,
    pub java_major: Option<u32>,
    /// The tools folder's runtime is installed.
    pub jre: bool,
    /// Where the machine has no runtime to install.
    pub jre_here: bool,
    pub jdtls: bool,
    pub java_debug: bool,
    /// JDT LS: running, ready.
    pub running: bool,
    pub ready: bool,
    /// Why there's no JDT LS, when it couldn't start.
    pub missing: Option<String>,
    pub kotlin_lsp: Option<String>,
    pub kls: bool,
    pub gjf: Option<String>,
    pub ktlint: Option<String>,
    pub gradle: Option<String>,
    pub mvn: Option<String>,
}

fn ok(label: &str, detail: impl Into<String>) -> Check {
    Check::new(Section::Toolchain, label, Health::Ok, detail)
}

fn missing(label: &str, health: Health, detail: impl Into<String>, fix: &str, command: &str) -> Check {
    Check::new(Section::Toolchain, label, health, detail).fix(fix, command)
}

/// The wrapper: there and able to run, and its Gradle on the JDK.
pub fn wrapper(build: &Build, found: &Found) -> Option<Check> {
    let w = &build.wrapper;
    let check = |health, detail: String| Check::new(Section::Project, if build.tool == Tool::Maven { "Maven wrapper" } else { "Gradle wrapper" }, health, detail);
    match build.tool {
        Tool::None => None,
        Tool::Gradle => {
            let make = found.gradle.is_some() || (w.script && w.jar);
            let fix = |c: Check| if make { c.fix(&format!("Make the wrapper (Gradle {GRADLE})"), WRAPPER) } else { c };
            if !w.script || !w.jar {
                let what = if w.script { "gradle/wrapper/gradle-wrapper.jar isn't there" } else { "no gradlew: tasks run the Gradle on PATH" };
                return Some(fix(check(Health::Warn, what.into())));
            }
            let version = w.version.clone().unwrap_or_else(|| "its Gradle".into());
            match (w.version.as_deref().and_then(gradle_runs_on), found.jdk) {
                (Some(max), Some(jdk)) if jdk > max => Some(fix(check(Health::Bad, format!("Gradle {version} runs on Java {max} at most, and the JDK is {jdk}")))),
                _ => Some(check(Health::Ok, format!("Gradle {version} (gradlew)"))),
            }
        }
        Tool::Maven => {
            if w.script {
                Some(check(Health::Ok, format!("Maven {} (mvnw)", w.version.as_deref().unwrap_or("?"))))
            } else {
                let c = check(Health::Warn, "no mvnw: tasks run the mvn on PATH".into());
                Some(if found.mvn.is_some() { c.fix(&format!("Make the wrapper (Maven {MAVEN})"), WRAPPER) } else { c })
            }
        }
    }
}

/// The checks the pack makes itself: what runs JDT LS and the rest.
pub fn checks(build: &Build, found: &Found) -> Vec<Check> {
    let mut out: Vec<Check> = wrapper(build, found).into_iter().collect();
    let runtime = "Server runtime";
    out.push(if found.jre {
        ok(runtime, format!("Temurin JRE {JRE}, in the tools folder"))
    } else {
        match (&found.java, found.java_major) {
            (Some(j), Some(m)) if m >= JDTLS_JAVA => ok(runtime, format!("Java {m} ({j})")),
            (j, m) => {
                let had = match (j, m) {
                    (Some(j), Some(m)) => format!("Java {m} ({j})"),
                    (Some(j), None) => j.clone(),
                    _ => "no java".into(),
                };
                let detail = format!("JDT LS {JDTLS} runs on Java {JDTLS_JAVA} or later; this is {had}");
                if found.jre_here {
                    missing(runtime, Health::Bad, detail, &format!("Install Temurin JRE {JRE}"), "java.install.jre")
                } else {
                    Check::new(Section::Toolchain, runtime, Health::Bad, detail)
                }
            }
        }
    });
    let server = "Language server";
    out.push(match (found.jdtls, found.running, found.ready, &found.missing) {
        (false, ..) => missing(server, Health::Bad, "Eclipse JDT LS isn't installed", &format!("Install JDT LS {JDTLS}"), "java.install.jdtls"),
        (true, _, true, _) => ok(server, format!("Eclipse JDT LS {JDTLS}, ready")),
        (true, true, false, _) => ok(server, format!("Eclipse JDT LS {JDTLS}, reading the project")),
        (true, false, _, Some(why)) => Check::new(Section::Toolchain, server, Health::Warn, why.clone()),
        (true, false, _, None) => ok(server, format!("Eclipse JDT LS {JDTLS}, when a Java file is opened")),
    });
    out.push(if found.java_debug {
        ok("Debugger", format!("java-debug {}", crate::tools::JAVA_DEBUG))
    } else {
        missing("Debugger", Health::Warn, "java-debug isn't installed: JDT LS can't debug", &format!("Install java-debug {}", crate::tools::JAVA_DEBUG), "java.install.java-debug")
    });
    out.push(match &found.gjf {
        Some(p) => ok("google-java-format", p.clone()),
        None => missing("google-java-format", Health::Warn, "not installed: JDT LS formats Java", &format!("Install google-java-format {}", crate::tools::GJF), "java.install.google-java-format"),
    });
    if build.kotlin() {
        out.push(match (&found.kotlin_lsp, found.kls) {
            (Some(p), _) => ok("Kotlin server", format!("kotlin-lsp ({p})")),
            (None, true) if found.java_major.is_some_and(|m| m >= 25) => Check::new(Section::Toolchain, "Kotlin server", Health::Warn, format!("kotlin-language-server {} doesn't run on Java {} (its Kotlin can't read the version): set java.jdk to a JDK 24 or older, or install kotlin-lsp", crate::tools::KLS, found.java_major.unwrap_or_default())),
            (None, true) => ok("Kotlin server", format!("kotlin-language-server {}", crate::tools::KLS)),
            (None, false) => missing("Kotlin server", Health::Warn, "neither kotlin-lsp nor kotlin-language-server is installed", &format!("Install kotlin-language-server {}", crate::tools::KLS), "java.install.kotlin-language-server"),
        });
        out.push(match &found.ktlint {
            Some(p) => ok("ktlint", p.clone()),
            None => missing("ktlint", Health::Warn, "not installed: Kotlin isn't formatted or linted on save", &format!("Install ktlint {}", crate::tools::KTLINT), "java.install.ktlint"),
        });
    }
    out
}

/// Making the wrapper again: `gradle wrapper` (the wrapper's own, when
/// it runs), `mvn wrapper:wrapper` -- the script-only one, so nothing
/// binary.
pub fn wrapper_command(build: &Build, found: &Found, wrapper: Option<&str>) -> Option<fenix_extension_api::Command> {
    use fenix_extension_api::Command;
    match build.tool {
        Tool::Gradle => {
            let runner = build.runner(wrapper, found.gradle.as_deref(), None)?;
            Some(runner.args(["wrapper", "--gradle-version", GRADLE]))
        }
        Tool::Maven => Some(Command::new(found.mvn.as_deref()?).args(["-B", "-N", "org.apache.maven.plugins:maven-wrapper-plugin:3.3.4:wrapper", "-Dtype=only-script", &format!("-Dmaven={MAVEN}")])),
        Tool::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{maven, orbit};
    use crate::tests::line;

    fn labels(c: &[Check]) -> Vec<(String, Health, Option<String>)> {
        c.iter().map(|c| (c.label.clone(), c.health, c.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    #[test]
    fn gradles_java() {
        assert_eq!(gradle_runs_on("8.14.3"), Some(24));
        assert_eq!(gradle_runs_on("9.8.2"), Some(25));
        assert_eq!(gradle_runs_on("8.5"), Some(21));
        assert_eq!(gradle_runs_on("6.9"), None);
        assert_eq!(jdk_page(Some(21)), "https://adoptium.net/temurin/releases/?version=21");
    }

    #[test]
    fn the_jdk_is_held_to_the_builds_release() {
        let dir = orbit("doctor-jdk");
        let b = crate::project::Build::read(&dir.to_string_lossy());
        let p = &programs(&b)[0];
        assert_eq!((p.name.as_str(), p.label.as_str()), ("javac", "JDK"));
        assert!(p.hints.contains(&"${setting:java.jdk}".to_string()));
        let out = fenix_lang_kit::Output { code: Some(0), stdout: "javac 17.0.9\n".into(), stderr: String::new() };
        let c = p.evaluate(Some("/usr/bin/javac"), Some(&out));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "17.0.9, but the project needs >=21 (/usr/bin/javac)"));
        assert_eq!(p.fix.as_ref().map(|f| f.0.as_str()), Some("Get JDK 21"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn each_check_and_its_fix() {
        let dir = orbit("doctor");
        let b = crate::project::Build::read(&dir.to_string_lossy());
        let mut found = Found { jdk: Some(21), java: Some("/usr/bin/java".into()), java_major: Some(21), jre_here: true, ..Found::default() };
        assert_eq!(
            labels(&checks(&b, &found)),
            [
                ("Gradle wrapper".into(), Health::Ok, None),
                ("Server runtime".into(), Health::Bad, Some("java.install.jre".into())),
                ("Language server".into(), Health::Bad, Some("java.install.jdtls".into())),
                ("Debugger".into(), Health::Warn, Some("java.install.java-debug".into())),
                ("google-java-format".into(), Health::Warn, Some("java.install.google-java-format".into())),
                ("Kotlin server".into(), Health::Warn, Some("java.install.kotlin-language-server".into())),
                ("ktlint".into(), Health::Warn, Some("java.install.ktlint".into())),
            ]
        );
        assert_eq!(checks(&b, &found)[1].detail, "JDT LS 1.63.0 runs on Java 25 or later; this is Java 21 (/usr/bin/java)");
        // All there.
        found = Found { jdk: Some(25), jre: true, jdtls: true, java_debug: true, running: true, ready: true, kls: true, gjf: Some("/tools/google-java-format/google-java-format".into()), ktlint: Some("/tools/ktlint/ktlint".into()), ..found };
        let c = checks(&b, &found);
        assert!(c[1..].iter().all(|c| c.health == Health::Ok), "{:?}", labels(&c));
        // Gradle 8.14.3 doesn't run on 25.
        assert_eq!((c[0].health, c[0].detail.as_str()), (Health::Bad, "Gradle 8.14.3 runs on Java 24 at most, and the JDK is 25"));
        assert_eq!(c[0].fix.as_ref().map(|f| f.1.as_str()), Some(WRAPPER));
        assert_eq!(line(&wrapper_command(&b, &found, Some("./gradlew")).unwrap()), "./gradlew wrapper --gradle-version 9.8.2");
        // kotlin-lsp, when it's there, is the one.
        found.kotlin_lsp = Some("/opt/homebrew/bin/kotlin-lsp".into());
        assert!(checks(&b, &found).iter().any(|c| c.detail == "kotlin-lsp (/opt/homebrew/bin/kotlin-lsp)"));
        std::fs::remove_dir_all(&dir).unwrap();

        let dir = maven("doctor-maven");
        let b = crate::project::Build::read(&dir.to_string_lossy());
        let c = checks(&b, &Found::default());
        assert_eq!((c[0].label.as_str(), c[0].detail.as_str()), ("Maven wrapper", "Maven 3.9.11 (mvnw)"));
        assert!(!c.iter().any(|c| c.label == "ktlint"), "no Kotlin here");
        assert_eq!(c[1].fix, None, "no runtime to install on this machine");
        assert_eq!(wrapper_command(&b, &Found::default(), None), None, "no mvn");
        assert_eq!(line(&wrapper_command(&b, &Found { mvn: Some("/opt/maven/bin/mvn".into()), ..Found::default() }, None).unwrap()), "/opt/maven/bin/mvn -B -N org.apache.maven.plugins:maven-wrapper-plugin:3.3.4:wrapper -Dtype=only-script -Dmaven=3.9.11");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
