//! The tasks (`SPC p t`), each run in the project's top folder, for the
//! build tools the project uses -- the first one's plain, any other's
//! prefixed with its name (`mill: compile`):
//!
//! | Tool | Tasks |
//! |---|---|
//! | sbt | `compile`, `test`, `run`, `clean`; `run <project>` and `test <project>` for each project `build.sbt` names; `scalafix` with sbt-scalafix |
//! | scala-cli | `compile`, `test`, `run`, `clean`, each on `.`; `scalafix` with a `.scalafix.conf` |
//! | Mill | `compile` and `test` of every module, `run <module>`, `clean`, `scalafix` with `ScalafixModule` |
//! | Gradle | `compileScala`, `compileTestScala`, `test`, `run` with `application`, `scaladoc`, `clean` |
//! | Maven | `compile`, `test`, `package`, `clean` |
//!
//! A REPL and a build run again as sources change aren't tasks: a task's
//! output is read, not typed into, so the REPL is a command in a terminal
//! ([`console`]), and watching is one run in the background ([`watch`]).
//! Running or testing the file you're in is a command too ([`run_file`]).

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::{Project, Tool};
use crate::tools::Tools;

/// The tool's program, by its full path when it was found, else by name.
pub fn program(tool: Tool, tools: &Tools) -> Command {
    let (found, name) = match tool {
        Tool::Sbt => (&tools.sbt, "sbt"),
        Tool::Mill => (&tools.mill, "mill"),
        Tool::ScalaCli => (&tools.scala_cli, "scala-cli"),
        Tool::Gradle => (&tools.gradle, "gradle"),
        Tool::Maven => (&tools.maven, "mvn"),
    };
    Command::new(found.as_deref().unwrap_or(name))
}

fn args(base: &Command, args: &[&str]) -> Command {
    base.clone().args(args.iter().map(|a| a.to_string()))
}

/// What a tool's tasks are, as `(name, arguments)` rows.
fn rows(tool: Tool, project: &Project) -> Vec<(String, Vec<String>)> {
    let row = |name: &str, a: &[&str]| (name.to_string(), a.iter().map(|a| a.to_string()).collect::<Vec<_>>());
    let mut rows = Vec::new();
    match tool {
        Tool::Sbt => {
            rows.extend([row("compile", &["compile"]), row("test", &["test"]), row("run", &["run"]), row("clean", &["clean"])]);
            for p in project.sbt_projects.iter().filter(|p| *p != "root") {
                rows.push((format!("run {p}"), vec![format!("{p}/run")]));
                rows.push((format!("test {p}"), vec![format!("{p}/test")]));
            }
            if project.scalafix == Some(Tool::Sbt) {
                rows.push(row("scalafix", &["scalafixAll"]));
            }
        }
        Tool::ScalaCli => {
            rows.extend([row("compile", &["compile", "."]), row("test", &["test", "."]), row("run", &["run", "."]), row("clean", &["clean", "."])]);
            if project.scalafix == Some(Tool::ScalaCli) {
                rows.push(row("scalafix", &["--power", "fix", "."]));
            }
        }
        Tool::Mill => {
            rows.extend([row("compile", &["__.compile"]), row("test", &["__.test"])]);
            if project.mill_root {
                rows.push(row("run", &["run"]));
            }
            for m in &project.mill_modules {
                rows.push((format!("run {m}"), vec![format!("{m}.run")]));
            }
            rows.push(row("clean", &["clean"]));
            if project.scalafix == Some(Tool::Mill) {
                rows.push(row("scalafix", &["__.fix"]));
            }
        }
        Tool::Gradle => {
            rows.extend([row("compileScala", &["compileScala"]), row("compileTestScala", &["compileTestScala"]), row("test", &["test"])]);
            if project.gradle_application {
                rows.push(row("run", &["run"]));
            }
            rows.extend([row("scaladoc", &["scaladoc"]), row("clean", &["clean"])]);
        }
        Tool::Maven => rows.extend([row("compile", &["compile"]), row("test", &["test"]), row("package", &["package"]), row("clean", &["clean"])]),
    }
    rows
}

/// The tasks for `project` in folder `root`, as `tools` found them. A
/// task's environment is the project's `.env`, and `JAVA_HOME` when a JDK
/// was chosen (`java_home`).
pub fn tasks(project: &Project, tools: &Tools, root: &str, java_home: Option<&str>) -> Tasks {
    let mut t = Tasks::default();
    for (i, tool) in project.tools.iter().enumerate() {
        let base = program(*tool, tools);
        for (name, a) in rows(*tool, project) {
            let name = if i == 0 { name } else { format!("{}: {name}", tool.name().to_lowercase()) };
            let mut cmd = base.clone().args(a);
            cmd.env = java_home.map(|h| vec![("JAVA_HOME".to_string(), h.to_string())]).unwrap_or_default();
            t.add(&name, cmd);
        }
    }
    t.env_file(&format!("{root}/.env"))
}

/// A REPL, for the tool the project uses (or scala-cli's alone, in any
/// folder): typed into, so run in a terminal. None where the tool has
/// none.
pub fn console(project: &Project, tools: &Tools) -> Option<(String, Command)> {
    match project.primary() {
        Some(Tool::Sbt) => Some(("sbt console".into(), program(Tool::Sbt, tools).arg("console"))),
        Some(Tool::ScalaCli) => Some(("scala-cli repl".into(), args(&program(Tool::ScalaCli, tools), &["repl", "."]))),
        Some(Tool::Mill) => {
            let target = match (project.mill_root, project.mill_modules.first()) {
                (true, _) => "console".to_string(),
                (false, Some(m)) => format!("{m}.console"),
                (false, None) => return None,
            };
            Some(("mill console".into(), args(&program(Tool::Mill, tools), &["-i", &target])))
        }
        Some(Tool::Maven) => Some(("mvn scala:console".into(), args(&program(Tool::Maven, tools), &["scala:console"]))),
        Some(Tool::Gradle) => None,
        None => tools.scala_cli.as_ref().map(|_| ("scala-cli repl".into(), args(&program(Tool::ScalaCli, tools), &["repl"]))),
    }
}

/// The build run again each time a source changes, kept in the background:
/// sbt's `~compile`, Mill's and scala-cli's `--watch`.
pub fn watch(project: &Project, tools: &Tools) -> Option<(String, Command)> {
    match project.primary()? {
        Tool::Sbt => Some(("sbt ~compile".into(), program(Tool::Sbt, tools).arg("~compile"))),
        Tool::ScalaCli => Some(("scala-cli compile --watch".into(), args(&program(Tool::ScalaCli, tools), &["compile", ".", "--watch"]))),
        Tool::Mill => Some(("mill -w compile".into(), args(&program(Tool::Mill, tools), &["-w", "__.compile"]))),
        Tool::Gradle | Tool::Maven => None,
    }
}

/// Whether `file` is one scala-cli runs or tests by itself: a script, or a
/// source.
pub fn is_source(file: &str) -> bool {
    file.ends_with(".scala") || file.ends_with(".sc")
}

/// `scala-cli run <file>` or `scala-cli test <file>`: the file you're in, a
/// script or a single source, with its `//> using` directives, in any folder.
pub fn run_file(tools: &Tools, verb: &str, file: &str) -> Command {
    program(Tool::ScalaCli, tools).args([verb.to_string(), file.to_string()]).cwd("/project")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{temp, write};
    use fenix_test_kit::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    fn sbt() -> Project {
        Project { tools: vec![Tool::Sbt], sbt_projects: vec!["root".into(), "core".into(), "cli".into()], ..Project::default() }
    }

    #[test]
    fn an_sbt_build_has_its_tasks_and_a_run_and_test_for_each_project() {
        let tools = Tools { sbt: Some("/t/sbt".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&sbt(), &tools, "/nowhere", None)),
            rows(&[
                ("compile", "/t/sbt compile"),
                ("test", "/t/sbt test"),
                ("run", "/t/sbt run"),
                ("clean", "/t/sbt clean"),
                ("run core", "/t/sbt core/run"),
                ("test core", "/t/sbt core/test"),
                ("run cli", "/t/sbt cli/run"),
                ("test cli", "/t/sbt cli/test"),
            ])
        );
        let with_fix = Project { scalafix: Some(Tool::Sbt), ..sbt() };
        assert_eq!(table(&tasks(&with_fix, &tools, "/nowhere", None)).last().unwrap(), &("scalafix".to_string(), "/t/sbt scalafixAll".to_string()));
    }

    #[test]
    fn scala_cli_runs_on_the_folder() {
        let p = Project { tools: vec![Tool::ScalaCli], scalafix: Some(Tool::ScalaCli), ..Project::default() };
        assert_eq!(
            table(&tasks(&p, &Tools::default(), "/nowhere", None)),
            rows(&[("compile", "scala-cli compile ."), ("test", "scala-cli test ."), ("run", "scala-cli run ."), ("clean", "scala-cli clean ."), ("scalafix", "scala-cli --power fix .")])
        );
    }

    #[test]
    fn mill_runs_every_module_and_a_run_for_each() {
        let p = Project { tools: vec![Tool::Mill], mill_modules: vec!["core".into(), "app".into()], scalafix: Some(Tool::Mill), ..Project::default() };
        let tools = Tools { mill: Some("/p/mill".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&p, &tools, "/nowhere", None)),
            rows(&[("compile", "/p/mill __.compile"), ("test", "/p/mill __.test"), ("run core", "/p/mill core.run"), ("run app", "/p/mill app.run"), ("clean", "/p/mill clean"), ("scalafix", "/p/mill __.fix")])
        );
        let root = Project { tools: vec![Tool::Mill], mill_root: true, ..Project::default() };
        assert!(table(&tasks(&root, &tools, "/nowhere", None)).contains(&("run".to_string(), "/p/mill run".to_string())));
    }

    #[test]
    fn gradle_and_maven_have_the_scala_ones_and_a_second_tool_is_prefixed() {
        let p = Project { tools: vec![Tool::Gradle, Tool::Maven], gradle_application: true, ..Project::default() };
        let t = table(&tasks(&p, &Tools::default(), "/nowhere", None));
        assert_eq!(
            t,
            rows(&[
                ("compileScala", "gradle compileScala"),
                ("compileTestScala", "gradle compileTestScala"),
                ("test", "gradle test"),
                ("run", "gradle run"),
                ("scaladoc", "gradle scaladoc"),
                ("clean", "gradle clean"),
                ("maven: compile", "mvn compile"),
                ("maven: test", "mvn test"),
                ("maven: package", "mvn package"),
                ("maven: clean", "mvn clean"),
            ])
        );
        let p = Project { tools: vec![Tool::Gradle], ..Project::default() };
        assert!(!table(&tasks(&p, &Tools::default(), "/nowhere", None)).iter().any(|(n, _)| n == "run"), "no application plugin, no run");
        assert!(tasks(&Project::default(), &Tools::default(), "/nowhere", None).tasks.is_empty(), "no tool, no tasks");
    }

    #[test]
    fn the_env_and_the_chosen_jdk_go_to_each() {
        let dir = temp("tasks-env");
        write(&dir, ".env", "SBT_OPTS=-Xmx2g\n");
        let t = tasks(&sbt(), &Tools::default(), &dir.to_string_lossy(), Some("/usr/lib/jvm/java-21"));
        let env = [("SBT_OPTS".to_string(), "-Xmx2g".to_string()), ("JAVA_HOME".to_string(), "/usr/lib/jvm/java-21".to_string())];
        assert!(t.resolved().iter().all(|t| t.cmd.env == env), "{:?}", t.resolved()[0].cmd.env);
        let none = tasks(&sbt(), &Tools::default(), "/nowhere", None);
        assert!(none.resolved().iter().all(|t| t.cmd.env.is_empty()), "no JDK chosen: the environment's own");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_repl_is_a_terminal_and_watching_is_one_background_task() {
        let tools = Tools { sbt: Some("/t/sbt".into()), scala_cli: Some("/t/scala-cli".into()), mill: Some("/t/mill".into()), ..Tools::default() };
        let c = |p: &Project| console(p, &tools).map(|(t, c)| (t, line(&c)));
        assert_eq!(c(&sbt()), Some(("sbt console".into(), "/t/sbt console".into())));
        let cli = Project { tools: vec![Tool::ScalaCli], ..Project::default() };
        assert_eq!(c(&cli), Some(("scala-cli repl".into(), "/t/scala-cli repl .".into())));
        let mill = Project { tools: vec![Tool::Mill], mill_modules: vec!["core".into()], ..Project::default() };
        assert_eq!(c(&mill), Some(("mill console".into(), "/t/mill -i core.console".into())));
        let mvn = Project { tools: vec![Tool::Maven], ..Project::default() };
        assert_eq!(c(&mvn).map(|c| c.1), Some("mvn scala:console".into()));
        assert_eq!(c(&Project { tools: vec![Tool::Gradle], ..Project::default() }), None, "Gradle has no Scala console");
        assert_eq!(c(&Project::default()), Some(("scala-cli repl".into(), "/t/scala-cli repl".into())), "a bare REPL, anywhere scala-cli is");
        assert_eq!(console(&Project::default(), &Tools::default()), None);

        let w = |p: &Project| watch(p, &tools).map(|(t, c)| (t, line(&c)));
        assert_eq!(w(&sbt()), Some(("sbt ~compile".into(), "/t/sbt ~compile".into())));
        assert_eq!(w(&cli).map(|c| c.1), Some("/t/scala-cli compile . --watch".into()));
        assert_eq!(w(&mill).map(|c| c.1), Some("/t/mill -w __.compile".into()));
        assert_eq!(w(&mvn), None);
    }

    #[test]
    fn a_script_or_a_source_runs_and_tests_with_scala_cli() {
        let tools = Tools { scala_cli: Some("/t/scala-cli".into()), ..Tools::default() };
        let c = run_file(&tools, "run", "/home/u/orbit/scripts/seed.sc");
        assert_eq!((line(&c), c.cwd.as_deref()), ("/t/scala-cli run /home/u/orbit/scripts/seed.sc".to_string(), Some("/project")));
        assert_eq!(line(&run_file(&Tools::default(), "test", "/p/OrbitSuite.scala")), "scala-cli test /p/OrbitSuite.scala");
        assert!(is_source("a/b/Main.scala") && is_source("x.sc") && !is_source("build.sbt") && !is_source("README.md"));
    }
}
