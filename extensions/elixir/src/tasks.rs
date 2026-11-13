//! The tasks (`SPC p t`): the `mix` tasks a project is worked with. Fetching
//! its dependencies, compiling, testing (all of it, or what failed last
//! time), checking the formatting, formatting; `mix phx.server` when
//! Phoenix is a dependency, `mix credo` when Credo is. `iex -S mix` isn't
//! one: a task's output is read, not typed into, so it opens in a
//! terminal ([`iex`]).

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::Project;
use crate::tools::Tools;

/// The tasks for `project` in folder `root`. The project's `.env` goes to
/// each.
pub fn tasks(project: &Project, tools: &Tools, root: &str) -> Tasks {
    let mut t = Tasks::default();
    if !project.is_elixir() {
        return t;
    }
    let mix = tools.mix();
    let mut add = |name: &str, args: &[&str]| t.add(name, mix.clone().args(args.iter().copied()));
    add("deps.get", &["deps.get"]);
    add("compile", &["compile"]);
    add("test", &["test"]);
    add("test --failed", &["test", "--failed"]);
    add("format --check-formatted", &["format", "--check-formatted"]);
    add("format", &["format"]);
    if project.credo {
        add("credo", &["credo"]);
    }
    if project.phoenix {
        add("phx.server", &["phx.server"]);
    }
    t.env_file(&format!("{root}/.env"))
}

/// `iex -S mix`: the project's code loaded into a shell, which is typed
/// into, so it runs in a terminal.
pub fn iex(tools: &Tools) -> Command {
    tools.iex().args(["-S", "mix"]).cwd("/project")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;
    use fenix_test_kit::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn a_mix_project_has_its_tasks() {
        let tools = Tools { mix: Some("/t/mix".into()), ..Tools::default() };
        let project = Project { mix: true, ..Project::default() };
        assert_eq!(
            table(&tasks(&project, &tools, "/nowhere")),
            rows(&[
                ("deps.get", "/t/mix deps.get"),
                ("compile", "/t/mix compile"),
                ("test", "/t/mix test"),
                ("test --failed", "/t/mix test --failed"),
                ("format --check-formatted", "/t/mix format --check-formatted"),
                ("format", "/t/mix format"),
            ])
        );
        let more = Project { mix: true, phoenix: true, credo: true, ..Project::default() };
        let names: Vec<String> = tasks(&more, &Tools::default(), "/nowhere").resolved().iter().map(|t| t.name.clone()).collect();
        assert_eq!(&names[6..], ["credo", "phx.server"], "when the project has them");
        assert_eq!(table(&tasks(&more, &Tools::default(), "/nowhere"))[2].1, "mix test", "mix by its name before it's found");
        assert!(tasks(&Project::default(), &tools, "/nowhere").tasks.is_empty(), "not a Mix project");
    }

    #[test]
    fn the_env_file_goes_to_each_task() {
        let dir = temp("tasks");
        std::fs::write(dir.join(".env"), "DATABASE_URL=ecto://localhost/dock_dev\n").unwrap();
        let t = tasks(&Project { mix: true, ..Project::default() }, &Tools::default(), &dir.to_string_lossy());
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("DATABASE_URL".to_string(), "ecto://localhost/dock_dev".to_string())]));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn iex_runs_mix_in_the_project() {
        let cmd = iex(&Tools { iex: Some("/t/iex".into()), ..Tools::default() });
        assert_eq!((line(&cmd).as_str(), cmd.cwd.as_deref()), ("/t/iex -S mix", Some("/project")));
    }
}
