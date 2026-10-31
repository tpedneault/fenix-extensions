//! The tasks (`SPC p t`): build, test, restore, clean and format for the
//! solution, then build, run and test for each of its projects (run for a
//! program, test for a test project, watch for a web project). A task of
//! the same name in `.fenix/tools.json` wins.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::{Project, Solution};
use crate::tools::Tools;

/// `dotnet <verb> [the solution]`.
fn whole(tools: &Tools, solution: &Solution, verb: &str) -> Command {
    let cmd = tools.dotnet().arg(verb);
    match solution.target() {
        Some(t) => cmd.arg(t),
        None => cmd,
    }
}

/// The task that builds `project`, a launch's `preLaunchTask`: its own,
/// or the solution's `build` when it's the only one.
pub fn build_name(solution: &Solution, project: &Project) -> String {
    if solution.projects.len() > 1 {
        format!("build {}", project.name)
    } else {
        "build".into()
    }
}

pub fn tasks(solution: &Solution, tools: &Tools) -> Tasks {
    let mut t = Tasks::default();
    if solution.projects.is_empty() {
        return t;
    }
    for verb in ["build", "test", "restore", "clean"] {
        if verb == "test" && !solution.projects.iter().any(|p| p.test) {
            continue;
        }
        t.add(verb, whole(tools, solution, verb));
    }
    // `dotnet format` changes the files themselves (it can't take one on
    // stdin), so it's a task, not the formatter.
    t.add("format", whole(tools, solution, "format"));
    if solution.tools_manifest {
        t.add("tool restore", tools.dotnet().args(["tool", "restore"]));
    }
    if solution.projects.len() > 1 {
        for p in &solution.projects {
            t.add(&format!("build {}", p.name), tools.dotnet().args(["build", &p.path]));
        }
    }
    for p in &solution.projects {
        if p.runnable() && !p.test {
            t.add(&format!("run {}", p.name), tools.dotnet().args(["run", "--project", &p.path]));
        }
        if p.web() {
            t.add(&format!("watch {}", p.name), tools.dotnet().args(["watch", "run", "--project", &p.path]));
        }
        if p.test {
            t.add(&format!("test {}", p.name), tools.dotnet().args(["test", &p.path]));
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{orbit, temp, write};
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    #[test]
    fn the_solution_then_each_project() {
        let dir = orbit("tasks");
        let s = Solution::read(&dir.to_string_lossy(), None);
        let tools = Tools { dotnet: Some("dotnet".into()), ..Tools::default() };
        let rows: Vec<(String, String)> = [
            ("build", "dotnet build Orbit.sln"),
            ("test", "dotnet test Orbit.sln"),
            ("restore", "dotnet restore Orbit.sln"),
            ("clean", "dotnet clean Orbit.sln"),
            ("format", "dotnet format Orbit.sln"),
            ("build Orbit.App", "dotnet build src/Orbit.App/Orbit.App.csproj"),
            ("build Orbit.Core", "dotnet build src/Orbit.Core/Orbit.Core.csproj"),
            ("build Orbit.Tests", "dotnet build tests/Orbit.Tests/Orbit.Tests.csproj"),
            ("build Orbit.Api", "dotnet build src/Orbit.Api/Orbit.Api.csproj"),
            ("build Orbit.Worker", "dotnet build src/Orbit.Worker/Orbit.Worker.csproj"),
            ("run Orbit.App", "dotnet run --project src/Orbit.App/Orbit.App.csproj"),
            ("test Orbit.Tests", "dotnet test tests/Orbit.Tests/Orbit.Tests.csproj"),
            ("run Orbit.Api", "dotnet run --project src/Orbit.Api/Orbit.Api.csproj"),
            ("watch Orbit.Api", "dotnet watch run --project src/Orbit.Api/Orbit.Api.csproj"),
            ("run Orbit.Worker", "dotnet run --project src/Orbit.Worker/Orbit.Worker.csproj"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        assert_eq!(table(&tasks(&s, &tools)), rows);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn one_project_and_its_tools() {
        let dir = temp("tasks-one");
        write(&dir, "App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>");
        write(&dir, ".config/dotnet-tools.json", "{\"version\":1,\"tools\":{}}");
        let s = Solution::read(&dir.to_string_lossy(), None);
        let names: Vec<String> = table(&tasks(&s, &Tools::default())).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["build", "restore", "clean", "format", "tool restore", "run App"], "no tests, no test task");
        assert!(tasks(&Solution::default(), &Tools::default()).tasks.is_empty(), "nothing to build");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
