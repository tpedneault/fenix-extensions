//! The tasks (`SPC p t`): Cabal's or Stack's -- build, test, and each
//! executable run, test suite tested and benchmark run, by the
//! components the packages' `.cabal` files (or `package.yaml`s) name --
//! or, in a folder of scripts, `runghc` on each file with a `main`; and
//! hlint over the project, when it lints and is installed. The
//! project's `.env` goes to each; a task of the same name in
//! `.fenix/tools.json` wins.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::{Kind, Project};
use crate::tools::{BuildTool, Tools};

/// The tasks for `project` in folder `root`, built by `tool`; `lint`,
/// whether hlint lints it.
pub fn tasks(project: &Project, tools: &Tools, tool: BuildTool, root: &str, lint: bool) -> Tasks {
    let mut t = Tasks::default();
    if !project.packages.is_empty() || project.cabal_project || project.stack {
        match tool {
            BuildTool::Stack => stack(&mut t, project, &tools.run("stack")),
            _ => cabal(&mut t, project, &tools.run("cabal")),
        }
    } else {
        let runghc = tools.run("runghc");
        for lone in project.lone.iter().filter(|l| l.main) {
            t.add(&format!("run {}", lone.file), runghc.clone().arg(lone.file.as_str()));
        }
    }
    if let (true, true, Some(hlint)) = (lint, project.is_haskell(), &tools.hlint) {
        t.add("hlint", Command::new(hlint).arg("."));
    }
    t.env_file(&format!("{root}/.env"))
}

/// Cabal's: `all` the project's packages built and tested, each component
/// by its kind and name (`exe:orbit`), which a project of several
/// packages takes as well as one.
fn cabal(t: &mut Tasks, project: &Project, cabal: &Command) {
    t.add("build", cabal.clone().args(["build", "all"]));
    t.add("test", cabal.clone().args(["test", "all"]));
    for (_, c) in project.components(Kind::Executable) {
        t.add(&format!("run {}", c.name), cabal.clone().args(["run", &format!("exe:{}", c.name)]));
    }
    for (_, c) in project.components(Kind::Test) {
        t.add(&format!("test {}", c.name), cabal.clone().args(["test", &format!("test:{}", c.name)]));
    }
    for (_, c) in project.components(Kind::Benchmark) {
        t.add(&format!("bench {}", c.name), cabal.clone().args(["bench", &format!("bench:{}", c.name)]));
    }
    t.add("update", cabal.clone().arg("update"));
    t.add("clean", cabal.clone().arg("clean"));
}

/// Stack's: an executable by its name (`stack run orbit`), a test suite
/// or a benchmark by its package's (`orbit:test:spec`).
fn stack(t: &mut Tasks, project: &Project, stack: &Command) {
    t.add("build", stack.clone().arg("build"));
    t.add("test", stack.clone().arg("test"));
    for (_, c) in project.components(Kind::Executable) {
        t.add(&format!("run {}", c.name), stack.clone().args(["run", c.name.as_str()]));
    }
    for kind in [Kind::Test, Kind::Benchmark] {
        let verb = if kind == Kind::Test { "test" } else { "bench" };
        for (p, c) in project.components(kind) {
            let Some(package) = &p.name else { continue };
            t.add(&format!("{verb} {}", c.name), stack.clone().args([verb, &format!("{package}:{}:{}", kind.target(), c.name)]));
        }
    }
    t.add("clean", stack.clone().arg("clean"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;
    use crate::project::{parse_cabal, parse_package_yaml, Lone};
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    fn cabal_project() -> Project {
        Project { packages: vec![parse_cabal(include_str!("samples/orbit.cabal"))], ..Project::default() }
    }

    #[test]
    fn cabal_runs_each_component() {
        let tools = Tools { cabal: Some("/g/cabal".into()), ormolu: Some("/g/ormolu".into()), hlint: Some("/g/hlint".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&cabal_project(), &tools, BuildTool::Cabal, "/nowhere", true)),
            rows(&[
                ("build", "/g/cabal build all"),
                ("test", "/g/cabal test all"),
                ("run orbit", "/g/cabal run exe:orbit"),
                ("run orbit-sim", "/g/cabal run exe:orbit-sim"),
                ("test orbit-test", "/g/cabal test test:orbit-test"),
                ("bench orbit-bench", "/g/cabal bench bench:orbit-bench"),
                ("update", "/g/cabal update"),
                ("clean", "/g/cabal clean"),
                ("hlint", "/g/hlint ."),
            ])
        );
        let bare = table(&tasks(&cabal_project(), &Tools::default(), BuildTool::Cabal, "/nowhere", true));
        assert_eq!(bare[0], ("build".into(), "cabal build all".into()), "by its name, before it's found");
        assert!(!bare.iter().any(|(n, _)| n == "hlint"), "not installed");
    }

    #[test]
    fn stack_runs_each_component_by_its_package() {
        let mut p = parse_package_yaml(include_str!("samples/package.yaml"));
        p.dir = String::new();
        let project = Project { stack: true, packages: vec![p], ..Project::default() };
        let tools = Tools { stack: Some("/g/stack".into()), fourmolu: Some("/g/fourmolu".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&project, &tools, BuildTool::Stack, "/nowhere", false)),
            rows(&[
                ("build", "/g/stack build"),
                ("test", "/g/stack test"),
                ("run orbit-exe", "/g/stack run orbit-exe"),
                ("run orbit-sim", "/g/stack run orbit-sim"),
                ("test orbit-test", "/g/stack test orbit:test:orbit-test"),
                ("clean", "/g/stack clean"),
            ])
        );
    }

    #[test]
    fn scripts_are_run_alone_with_the_env() {
        let dir = temp("tasks");
        std::fs::write(dir.join(".env"), "ORBIT_ENV=dev\n").unwrap();
        let project = Project { lone: vec![Lone { file: "Hello.hs".into(), main: true }, Lone { file: "Orbit.hs".into(), main: false }], ..Project::default() };
        let tools = Tools { runghc: Some("/g/runghc".into()), ormolu: Some("/g/ormolu".into()), ..Tools::default() };
        let t = tasks(&project, &tools, BuildTool::Cabal, &dir.to_string_lossy(), true);
        assert_eq!(table(&t), rows(&[("run Hello.hs", "/g/runghc Hello.hs")]));
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("ORBIT_ENV".to_string(), "dev".to_string())]));
        assert!(tasks(&Project::default(), &tools, BuildTool::Cabal, "/nowhere", true).tasks.is_empty(), "not a Haskell project");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
