//! The tasks (`SPC p t`): `zig build` and each of its steps (`zig build
//! -l` lists them; `test` and `run` until it has), or, in a project
//! without `build.zig`, `zig run` and `zig test` on each Zig file at its
//! top; and `zig fmt` on it all. The project's `.env` goes to each.

use fenix_lang_kit::Tasks;

use crate::project::Project;
use crate::tools::Tools;

/// A step `zig build -l` listed: its name, and whether it's the default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub name: String,
    pub description: String,
    pub default: bool,
}

/// What `zig build -l` printed: a step a line, its name first, then
/// `(default)` for the default, then what it does.
pub fn parse_steps(text: &str) -> Vec<Step> {
    text.lines()
        .filter(|l| l.starts_with(' '))
        .filter_map(|l| {
            let mut rest = l.trim();
            let name = rest.split_whitespace().next()?;
            rest = rest[name.len()..].trim_start();
            let default = rest.starts_with("(default)");
            let description = rest.trim_start_matches("(default)").trim().to_string();
            Some(Step { name: name.to_string(), description, default })
        })
        .collect()
}

/// The tasks for `project` in folder `root`, with `steps` as `zig build
/// -l` said them (none yet: `test` and `run`, as `zig init` makes).
pub fn tasks(project: &Project, tools: &Tools, steps: Option<&[Step]>, root: &str) -> Tasks {
    let zig = tools.zig();
    let mut t = Tasks::default();
    if project.build {
        t.add("build", zig.clone().arg("build"));
        let names: Vec<&str> = match steps {
            Some(steps) => steps.iter().map(|s| s.name.as_str()).collect(),
            None => vec!["test", "run"],
        };
        // `install` is what `zig build` does; `uninstall` is rarely meant.
        for name in names.into_iter().filter(|n| !matches!(*n, "install" | "uninstall")) {
            t.add(&format!("build {name}"), zig.clone().args(["build", name]));
        }
    } else {
        for lone in &project.lone {
            if lone.main {
                t.add(&format!("run {}", lone.file), zig.clone().args(["run", lone.file.as_str()]));
            }
            t.add(&format!("test {}", lone.file), zig.clone().args(["test", lone.file.as_str()]));
        }
    }
    if project.is_zig() {
        t.add("fmt", zig.clone().args(["fmt", "."]));
    }
    t.env_file(&format!("{root}/.env"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{tests::temp, Lone};
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn zig_build_l_lists_the_steps() {
        let steps = parse_steps(include_str!("samples/zig-build-l.txt"));
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["install", "uninstall", "run", "test", "docs", "bench-orbit"]);
        assert_eq!(steps[0], Step { name: "install".into(), description: "Copy build artifacts to prefix path".into(), default: true });
        assert_eq!((steps[5].default, steps[5].description.as_str()), (false, "Benchmark the orbit propagator"));
        assert!(parse_steps("error: no build.zig file found, in the current directory or any parent directories\n").is_empty());
    }

    #[test]
    fn a_build_has_each_of_its_steps() {
        let project = Project { build: true, ..Project::default() };
        let tools = Tools { zig: Some("/t/zig".into()), ..Tools::default() };
        let steps = parse_steps(include_str!("samples/zig-build-l.txt"));
        assert_eq!(
            table(&tasks(&project, &tools, Some(&steps), "/nowhere")),
            rows(&[("build", "/t/zig build"), ("build run", "/t/zig build run"), ("build test", "/t/zig build test"), ("build docs", "/t/zig build docs"), ("build bench-orbit", "/t/zig build bench-orbit"), ("fmt", "/t/zig fmt .")])
        );
        assert_eq!(table(&tasks(&project, &Tools::default(), None, "/nowhere")), rows(&[("build", "zig build"), ("build test", "zig build test"), ("build run", "zig build run"), ("fmt", "zig fmt .")]), "before the steps are listed");
    }

    #[test]
    fn lone_files_are_run_and_tested_with_the_env() {
        let dir = temp("tasks");
        std::fs::write(dir.join(".env"), "ORBIT_ENV=dev\n").unwrap();
        let project = Project { lone: vec![Lone { file: "hello.zig".into(), main: true }, Lone { file: "math.zig".into(), main: false }], ..Project::default() };
        let t = tasks(&project, &Tools::default(), None, &dir.to_string_lossy());
        assert_eq!(table(&t), rows(&[("run hello.zig", "zig run hello.zig"), ("test hello.zig", "zig test hello.zig"), ("test math.zig", "zig test math.zig"), ("fmt", "zig fmt .")]));
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("ORBIT_ENV".to_string(), "dev".to_string())]));
        assert!(tasks(&Project::default(), &Tools::default(), None, "/nowhere").tasks.is_empty(), "not a Zig project");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
