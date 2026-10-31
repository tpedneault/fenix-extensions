//! Debugging, with java-debug: JDT LS starts it (`vscode.java.
//! startDebugSession`) and says its port, which Fenix connects to
//! (`debug-adapter-connect`). A launch file can't carry a classpath --
//! java-debug's launch needs one, and only JDT LS knows it -- so every
//! session attaches: to a program the pack starts held for the debugger
//! (`-agentlib:jdwp=...,suspend=y`, on port 5005), to tests Gradle or
//! Maven hold the same way (`--debug-jvm`, `-Dmaven.surefire.debug`), or
//! to a JVM you started yourself.

use fenix_extension_api::Command;

use crate::project::{Build, Module, Tool};

/// The port a program held for the debugger waits on: Gradle's and
/// Maven's own.
pub const PORT: u16 = 5005;

/// The JVM's agent, held at its start until the debugger attaches.
pub fn agent() -> String {
    format!("-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:{PORT}")
}

/// `main` run by `java` held for the debugger, with JDT LS's module path
/// and classpath.
pub fn held(java: &str, main: &str, module_path: &[String], class_path: &[String], windows: bool) -> Command {
    let sep = if windows { ";" } else { ":" };
    let mut cmd = Command::new(java).arg(agent());
    if !module_path.is_empty() {
        cmd = cmd.args(["--module-path".to_string(), module_path.join(sep)]);
    }
    if !class_path.is_empty() {
        cmd = cmd.args(["-cp".to_string(), class_path.join(sep)]);
    }
    cmd.arg(main)
}

/// Gradle's `run`, held: for an application's own main class, where
/// Gradle builds what JDT LS doesn't (Kotlin).
pub fn gradle_run(runner: &Command, module: &Module) -> Command {
    runner.clone().args([module.task("run"), "--debug-jvm".into()])
}

/// A class's name from its file: `app/src/test/java/orbit/app/AppTest.java`
/// is `orbit.app.AppTest`.
pub fn class_of(rel: &str) -> Option<String> {
    let rel = rel.replace('\\', "/");
    let (_, rest) = ["/java/", "/kotlin/"].iter().find_map(|d| rel.split_once(&format!("src/main{d}")).or_else(|| rel.split_once(&format!("src/test{d}"))))?;
    let stem = rest.rsplit_once('.').map(|(s, _)| s)?;
    Some(stem.replace('/', "."))
}

/// The tests held for the debugger: the class `class` of `module`, or
/// all of its, or the build's.
pub fn tests_held(build: &Build, runner: &Command, module: Option<&Module>, class: Option<&str>) -> Option<Command> {
    match build.tool {
        Tool::Gradle => {
            let task = module.map(|m| m.task("test")).unwrap_or_else(|| "test".into());
            let cmd = runner.clone().args([task, "--debug-jvm".into()]);
            Some(match class {
                Some(c) => cmd.args(["--tests".to_string(), c.to_string()]),
                None => cmd,
            })
        }
        Tool::Maven => {
            let mut cmd = runner.clone().arg("-B");
            if let Some(m) = module.filter(|m| !m.dir.is_empty()) {
                cmd = cmd.args(["-pl".to_string(), m.dir.clone(), "-am".into()]);
            }
            cmd = cmd.args(["test", "-Dmaven.surefire.debug", "-Dsurefire.failIfNoSpecifiedTests=false"]);
            Some(match class {
                Some(c) => cmd.arg(format!("-Dtest={}", c.rsplit('.').next().unwrap_or(c))),
                None => cmd,
            })
        }
        Tool::None => None,
    }
}

/// What a held program says once it's waiting: `Listening for transport
/// dt_socket at address: 5005`.
pub fn listening(line: &str) -> bool {
    line.contains("Listening for transport dt_socket")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{maven, orbit};
    use crate::tests::line;

    #[test]
    fn a_main_class_is_held_for_the_debugger() {
        let cmd = held("/tools/jre/bin/java", "orbit.app.App", &[], &["/p/app/bin/main".into(), "/m2/guava.jar".into()], false);
        assert_eq!(line(&cmd), "/tools/jre/bin/java -agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:5005 -cp /p/app/bin/main:/m2/guava.jar orbit.app.App");
        let cmd = held("java", "app/orbit.App", &["C:/m/a.jar".into()], &["C:/p/bin".into(), "C:/m2/g.jar".into()], true);
        assert_eq!(line(&cmd), "java -agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:5005 --module-path C:/m/a.jar -cp C:/p/bin;C:/m2/g.jar app/orbit.App");
        assert!(listening("Listening for transport dt_socket at address: 5005"));
        assert_eq!(class_of("app/src/test/java/orbit/app/AppTest.java").as_deref(), Some("orbit.app.AppTest"));
        assert_eq!(class_of("core/src/main/kotlin/orbit/core/Orbits.kt").as_deref(), Some("orbit.core.Orbits"));
        assert_eq!(class_of("README.md"), None);
    }

    #[test]
    fn tests_are_held_by_gradle_and_maven() {
        let dir = orbit("debug-gradle");
        let b = Build::read(&dir.to_string_lossy());
        let runner = b.runner(Some(crate::tests::wrapper(&b)), None, None).unwrap();
        let app = b.modules.iter().find(|m| m.name == "app");
        assert_eq!(line(&tests_held(&b, &runner, app, Some("orbit.app.AppTest")).unwrap()), "./gradlew :app:test --debug-jvm --tests orbit.app.AppTest");
        assert_eq!(line(&tests_held(&b, &runner, None, None).unwrap()), "./gradlew test --debug-jvm");
        assert_eq!(line(&gradle_run(&runner, app.unwrap())), "./gradlew :app:run --debug-jvm");
        std::fs::remove_dir_all(&dir).unwrap();

        let dir = maven("debug-maven");
        let b = Build::read(&dir.to_string_lossy());
        let runner = b.runner(Some(crate::tests::wrapper(&b)), None, None).unwrap();
        let core = b.modules.iter().find(|m| m.dir == "core");
        assert_eq!(line(&tests_held(&b, &runner, core, Some("orbit.core.OrbitsTest")).unwrap()), "./mvnw -B -pl core -am test -Dmaven.surefire.debug -Dsurefire.failIfNoSpecifiedTests=false -Dtest=OrbitsTest");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
