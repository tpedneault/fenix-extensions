//! The tasks (`SPC p t`). Gradle's: build, test, clean and check for the
//! build, then each project's run and test, and -- once `gradle tasks
//! --all` has said what there is -- the build's own tasks besides
//! (`bootRun`, `bootJar`, a group of its own). Maven's: its lifecycle's
//! phases, and each module's test and package, and `spring-boot:run`
//! for a Spring Boot module. A task of the same name in
//! `.fenix/tools.json` wins.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::{Build, Tool};

/// Gradle's own groups, whose tasks the table has already or doesn't
/// want (`help`, `build setup`, the compile tasks of `other`).
const GRADLE_GROUPS: &[&str] = &["application", "build", "build setup", "distribution", "documentation", "help", "verification", "other", "ide", "publishing", "rules"];

/// The tasks of Gradle's groups worth a row.
const GRADLE_KEPT: &[&str] = &["bootRun", "bootJar", "bootBuildImage", "bootTestRun", "installDist", "javadoc", "jar", "assemble"];

/// Maven's lifecycle, as tasks.
const MAVEN_PHASES: &[&str] = &["clean", "compile", "test", "package", "verify", "install"];

/// A task `gradle tasks --all` lists: its group, its path (`app:run`, or
/// `build` for the root), and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GradleTask {
    pub group: String,
    pub path: String,
    pub description: String,
}

/// `gradle tasks --all`'s report: each group's heading, its rule of
/// dashes, then `path - description` lines until a blank one.
pub fn parse_gradle_tasks(text: &str) -> Vec<GradleTask> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut group: Option<String> = None;
    for (i, line) in lines.iter().enumerate() {
        let next_is_rule = lines.get(i + 1).is_some_and(|n| !n.is_empty() && n.chars().all(|c| c == '-'));
        if let Some(heading) = line.strip_suffix(" tasks").filter(|_| next_is_rule) {
            group = Some(heading.to_ascii_lowercase());
            continue;
        }
        if line.trim().is_empty() || line.chars().all(|c| c == '-') {
            if line.trim().is_empty() {
                group = None;
            }
            continue;
        }
        let Some(g) = &group else { continue };
        let (path, description) = line.split_once(" - ").map(|(p, d)| (p.trim(), d.trim())).unwrap_or((line.trim(), ""));
        if path.contains(' ') || path.is_empty() {
            continue;
        }
        out.push(GradleTask { group: g.clone(), path: path.to_string(), description: description.to_string() });
    }
    out
}

/// A task's row name: `run`, `run app`, `bootRun api`.
fn named(task: &str, module: &str) -> String {
    if module.is_empty() {
        task.to_string()
    } else {
        format!("{task} {module}")
    }
}

/// The table, for `build` run by `runner` (the wrapper, or Gradle's or
/// Maven's own); `found`, what `gradle tasks --all` said, when it has.
pub fn tasks(build: &Build, runner: Option<&Command>, found: &[GradleTask]) -> Tasks {
    let mut t = Tasks::default();
    let Some(runner) = runner else { return t };
    let run = |args: &[&str]| runner.clone().args(args.iter().map(|a| a.to_string()));
    match build.tool {
        Tool::Gradle => {
            for task in ["build", "test", "clean", "check"] {
                t.add(task, run(&[task]));
            }
            let several = build.modules.iter().filter(|m| !m.file.is_empty()).count() > 1;
            for m in &build.modules {
                if m.application {
                    let task = if m.spring_boot { "bootRun" } else { "run" };
                    t.add(&named(task, &m.name), run(&[&m.task(task)]));
                }
                if m.tests && several && !m.name.is_empty() {
                    t.add(&named("test", &m.name), run(&[&m.task("test")]));
                }
            }
            for f in found {
                let (module, task) = f.path.rsplit_once(':').unwrap_or(("", &f.path));
                let own_group = !GRADLE_GROUPS.contains(&f.group.as_str());
                if !(own_group || GRADLE_KEPT.contains(&task)) {
                    continue;
                }
                let name = named(task, &module.replace(':', "/"));
                if !t.tasks.iter().any(|x| x.name == name) {
                    let path = if module.is_empty() { task.to_string() } else { format!(":{module}:{task}") };
                    t.add(&name, run(&[&path]));
                }
            }
        }
        Tool::Maven => {
            for phase in MAVEN_PHASES {
                t.add(phase, run(&["-B", phase]));
            }
            t.add("clean install", run(&["-B", "clean", "install"]));
            let modules: Vec<_> = build.modules.iter().filter(|m| !m.name.is_empty()).collect();
            for m in &modules {
                if m.tests {
                    t.add(&named("test", &m.name), run(&["-B", "-pl", &m.dir, "-am", "test", "-Dsurefire.failIfNoSpecifiedTests=false"]));
                }
                t.add(&named("package", &m.name), run(&["-B", "-pl", &m.dir, "-am", "package"]));
            }
            for m in &build.modules {
                if m.spring_boot {
                    let cmd = if m.dir.is_empty() { run(&["spring-boot:run"]) } else { run(&["-pl", &m.dir, "spring-boot:run"]) };
                    t.add(&named("spring-boot:run", &m.name), cmd);
                } else if m.main_class.is_some() {
                    let cmd = if m.dir.is_empty() { run(&["compile", "exec:java"]) } else { run(&["-pl", &m.dir, "-am", "compile", "exec:java"]) };
                    t.add(&named("exec:java", &m.name), cmd);
                }
            }
        }
        Tool::None => {}
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{maven, orbit};
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(r: &[(&str, &str)]) -> Vec<(String, String)> {
        r.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn gradle_tasks_report_is_read_group_by_group() {
        let found = parse_gradle_tasks(include_str!("samples/gradle-tasks-all.txt"));
        assert!(found.contains(&GradleTask { group: "application".into(), path: "app:run".into(), description: "Runs this project as a JVM application".into() }));
        assert!(found.iter().any(|t| t.group == "other" && t.path == "core:mainClasses" && t.description.is_empty()));
        assert!(found.iter().any(|t| t.group == "help" && t.path == "dependencyUpdates"));
        assert!(!found.iter().any(|t| t.path.contains("Problems") || t.path.starts_with("BUILD")));
        assert_eq!(found.iter().filter(|t| t.group == "verification").count(), 5);
    }

    #[test]
    fn the_gradle_build_and_each_project() {
        let dir = orbit("tasks");
        let b = Build::read(&dir.to_string_lossy());
        let runner = b.runner(Some(crate::tests::wrapper(&b)), None, None);
        let found = parse_gradle_tasks(include_str!("samples/gradle-tasks-all.txt"));
        let mut expect = rows(&[("build", "./gradlew build"), ("test", "./gradlew test"), ("clean", "./gradlew clean"), ("check", "./gradlew check"), ("run app", "./gradlew :app:run"), ("test app", "./gradlew :app:test"), ("test core", "./gradlew :core:test")]);
        assert_eq!(table(&tasks(&b, runner.as_ref(), &[])), expect);
        expect.extend(rows(&[("assemble app", "./gradlew :app:assemble"), ("assemble core", "./gradlew :core:assemble"), ("jar app", "./gradlew :app:jar"), ("jar core", "./gradlew :core:jar"), ("installDist app", "./gradlew :app:installDist"), ("javadoc app", "./gradlew :app:javadoc"), ("javadoc core", "./gradlew :core:javadoc")]));
        assert_eq!(table(&tasks(&b, runner.as_ref(), &found)), expect);

        // A group of the build's own, and Spring Boot's.
        let own = parse_gradle_tasks("Orbit tasks\n-----------\ndeploy - Sends it up\n\nApplication tasks\n-----------------\nbootRun - Runs this project as a Spring Boot application.\n");
        let names: Vec<String> = table(&tasks(&b, runner.as_ref(), &own)).into_iter().map(|r| r.0).collect();
        assert!(names.contains(&"deploy".to_string()) && names.contains(&"bootRun".to_string()), "{names:?}");
        assert!(tasks(&b, None, &found).tasks.is_empty(), "nothing to run them with");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn maven_phases_and_each_module() {
        let dir = maven("tasks-maven");
        let b = Build::read(&dir.to_string_lossy());
        let runner = b.runner(Some(crate::tests::wrapper(&b)), None, None);
        let t = table(&tasks(&b, runner.as_ref(), &[]));
        let expect = rows(&[
            ("clean", "./mvnw -B clean"),
            ("compile", "./mvnw -B compile"),
            ("test", "./mvnw -B test"),
            ("package", "./mvnw -B package"),
            ("verify", "./mvnw -B verify"),
            ("install", "./mvnw -B install"),
            ("clean install", "./mvnw -B clean install"),
            ("test orbit-core", "./mvnw -B -pl core -am test -Dsurefire.failIfNoSpecifiedTests=false"),
            ("package orbit-core", "./mvnw -B -pl core -am package"),
            ("package orbit-api", "./mvnw -B -pl api -am package"),
            ("spring-boot:run orbit-api", "./mvnw -pl api spring-boot:run"),
        ]);
        assert_eq!(t, expect);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
