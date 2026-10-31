//! netcoredbg as the debug adapter (`netcoredbg --interpreter=vscode`, DAP
//! over stdio), and what it's told to debug.
//!
//! Each program of the solution is a launch the pack gives as it runs
//! (`launches-set`, 0.8.6): `dotnet <its Debug dll>` in its own folder, in
//! Development with its launch profile's variables and URL, as `dotnet
//! run` would, its project built first (`preLaunchTask`). Once `dotnet
//! test` holds its test host for the debugger (`VSTEST_HOST_DEBUG=1`), a
//! launch attaching to that process is offered beside them; any other
//! process is the attach launch file's, `${pickProcess}`.

use fenix_extension_api::{Command, LaunchConfig};
use fenix_lang_kit::serde_json;

use crate::project::{Project, Solution};
use crate::tasks;

/// Fenix's name for the language.
const CSHARP: &[&str] = &["csharp"];

/// The adapter.
pub fn adapter(netcoredbg: &str) -> Command {
    Command::new(netcoredbg).arg("--interpreter=vscode")
}

/// `project` from its Debug build: its dll as the program, which netcoredbg
/// runs with `dotnet` itself. Given `dotnet` as the program and the dll as
/// its argument, netcoredbg ran `dotnet.exe` as the app ("The library
/// 'hostpolicy.dll' required to execute the application was not found"),
/// and the session never started. Stopped at its start or not.
pub fn launch(solution: &Solution, project: &Project, stop_at_entry: bool) -> LaunchConfig {
    let name = if stop_at_entry { format!(".NET: {}, stopped at its start", project.name) } else { format!(".NET: {}", project.name) };
    let dir = if project.dir().is_empty() { "${project}".to_string() } else { format!("${{project}}/{}", project.dir()) };
    let mut l = LaunchConfig::launch(&name, CSHARP)
        .program(&format!("${{project}}/{}", project.dll()))
        .cwd(&dir)
        .attributes(&serde_json::json!({"justMyCode": true, "stopAtEntry": stop_at_entry}))
        .pre_launch_task(&tasks::build_name(solution, project));
    for (k, v) in project.launch_env() {
        l = l.env(&k, &v);
    }
    l
}

/// What `SPC u u` offers besides the launch files: each program, then
/// each stopped at its start, and the tests held for the debugger
/// (`held`, the test host's process) when there are. Programs only with a
/// `dotnet` to run them.
pub fn launches(solution: &Solution, dotnet: Option<&str>, held: Option<u32>) -> Vec<LaunchConfig> {
    let mut out = Vec::new();
    if dotnet.is_some() {
        let programs: Vec<&Project> = solution.projects.iter().filter(|p| p.runnable()).collect();
        for stop in [false, true] {
            out.extend(programs.iter().map(|p| launch(solution, p, stop)));
        }
    }
    if let Some(pid) = held {
        out.push(LaunchConfig::attach(&format!(".NET: the tests dotnet test holds (process {pid})"), CSHARP).attributes(&serde_json::json!({"processId": pid})));
    }
    out
}

/// `dotnet test` with the test host held until a debugger attaches.
pub fn test_held(dotnet: &Command, target: Option<&str>) -> Command {
    let mut cmd = dotnet.clone().arg("test");
    if let Some(t) = target {
        cmd = cmd.arg(t);
    }
    cmd.env.extend([("VSTEST_HOST_DEBUG".to_string(), "1".to_string()), ("MSBUILDTERMINALLOGGER".into(), "off".into()), ("DOTNET_NOLOGO".into(), "1".into())]);
    cmd
}

/// The test host's process in `dotnet test`'s line `Process Id: 24793,
/// Name: dotnet`.
pub fn test_host(line: &str) -> Option<u32> {
    let rest = line.trim().strip_prefix("Process Id:")?;
    rest.split(',').next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{orbit, temp, write};
    use fenix_extension_api::LaunchRequest;

    #[test]
    fn each_program_is_a_launch_built_first_and_run_as_dotnet_run_would() {
        let dir = orbit("launches");
        let s = Solution::read(&dir.to_string_lossy(), None);
        let all = launches(&s, Some("/usr/bin/dotnet"), None);
        let names: Vec<&str> = all.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, [".NET: Orbit.App", ".NET: Orbit.Api", ".NET: Orbit.Worker", ".NET: Orbit.App, stopped at its start", ".NET: Orbit.Api, stopped at its start", ".NET: Orbit.Worker, stopped at its start"], "not the library, nor VSTest's tests");
        let app = &all[0];
        assert_eq!((app.request, app.program.as_deref(), app.args.is_empty(), app.cwd.as_deref()), (LaunchRequest::Launch, Some("${project}/src/Orbit.App/bin/Debug/net8.0/Orbit.App.dll"), true, Some("${project}/src/Orbit.App")), "the dll, which netcoredbg runs with dotnet");
        assert_eq!(app.pre_launch_task.as_deref(), Some("build Orbit.App"), "a task of tasks.rs's");
        assert_eq!(app.languages, ["csharp"]);
        let attributes: serde_json::Value = serde_json::from_str(&all[3].attributes).unwrap();
        assert_eq!(attributes, serde_json::json!({"justMyCode": true, "stopAtEntry": true}));
        let api = &all[1];
        assert!(api.env.contains(&("ASPNETCORE_URLS".into(), "http://localhost:5189".into())), "{:?}", api.env);
        assert!(api.env.contains(&("ASPNETCORE_ENVIRONMENT".into(), "Development".into())));
        assert!(launches(&s, None, None).is_empty(), "no dotnet: nothing to run");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn one_project_is_built_by_the_build_task_and_runs_in_the_project() {
        let dir = temp("launch-one");
        write(&dir, "App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>");
        let s = Solution::read(&dir.to_string_lossy(), None);
        let l = launch(&s, &s.projects[0], false);
        assert_eq!((l.pre_launch_task.as_deref(), l.cwd.as_deref(), l.program.as_deref()), (Some("build"), Some("${project}"), Some("${project}/bin/Debug/net8.0/App.dll")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_tests_held_are_attached_to_by_their_process() {
        let held = launches(&Solution::default(), Some("dotnet"), Some(24793));
        assert_eq!(held.len(), 1);
        assert_eq!((held[0].request, held[0].program.as_deref()), (LaunchRequest::Attach, None));
        assert_eq!(held[0].attributes, r#"{"processId":24793}"#, "a number, as netcoredbg takes it");
        assert_eq!(crate::tests::line(&adapter("/t/netcoredbg")), "/t/netcoredbg --interpreter=vscode");
    }

    #[test]
    fn the_test_host_waiting_says_its_process() {
        let out = include_str!("samples/vstest-debug.txt");
        let pids: Vec<u32> = out.lines().filter_map(test_host).collect();
        assert_eq!(pids, [24793, 24793], "said twice");
        let cmd = test_held(&Command::new("dotnet"), Some("tests/Orbit.Tests/Orbit.Tests.csproj"));
        assert_eq!(crate::tests::line(&cmd), "dotnet test tests/Orbit.Tests/Orbit.Tests.csproj");
        assert_eq!(cmd.env[0], ("VSTEST_HOST_DEBUG".to_string(), "1".to_string()));
        assert_eq!(test_host("Process Id: x, Name: dotnet"), None);
    }
}
