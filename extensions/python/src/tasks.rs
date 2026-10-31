//! The tasks (`SPC p t`): run, test, lint, format, typecheck, and sync
//! for a project uv manages. Each runs the project's own tool when it
//! asks for one (`uv run pytest`), else one uv brings for the run (`uv
//! run --with pytest`), else what's installed. The project's `.env` goes
//! to each; a task of the same name in `.fenix/tools.json` wins.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::Project;
use crate::tools::{Server, Tools};

/// How the project's Python and its tools are run.
struct Runner<'a> {
    project: &'a Project,
    tools: &'a Tools,
    uv: bool,
}

impl Runner<'_> {
    /// Python, in the project's environment.
    fn python(&self) -> Command {
        if self.uv {
            self.tools.uv().args(["run", "python"])
        } else {
            self.tools.python()
        }
    }

    /// `tool` (a package of that name), in the project's environment: the
    /// project's, else one for the run.
    fn tool(&self, tool: &str) -> Command {
        match (self.uv, self.project.has(tool)) {
            (true, true) => self.tools.uv().args(["run", tool]),
            (true, false) => self.tools.uv().args(["run", "--with", tool, tool]),
            (false, _) => self.tools.python().args(["-m", tool]),
        }
    }

    /// ruff, which needs nothing of the project's environment.
    fn ruff(&self) -> Option<Command> {
        if self.project.has("ruff") {
            return Some(self.tool("ruff"));
        }
        if let Some(ruff) = &self.tools.ruff {
            return Some(Command::new(ruff));
        }
        self.tools.uv.as_ref().map(|_| self.tools.uv().args(["tool", "run", "ruff"]))
    }

    fn run(&self) -> Option<Command> {
        let p = self.project;
        if let (true, Some(script)) = (self.uv, p.scripts.first()) {
            return Some(self.tools.uv().args(["run", script]));
        }
        if p.django {
            return Some(self.python().args(["manage.py", "runserver"]));
        }
        if let Some(module) = &p.main_module {
            return Some(self.python().args(["-m", module]));
        }
        p.entry.as_ref().map(|e| self.python().arg(e))
    }

    fn test(&self) -> Command {
        if self.project.django && !self.project.has("pytest-django") {
            self.python().args(["manage.py", "test"])
        } else {
            self.tool("pytest")
        }
    }

    fn typecheck(&self, server: Server) -> Option<Command> {
        if self.project.has("mypy") {
            return Some(self.tool("mypy").arg("."));
        }
        let checker = [server, server.other()].into_iter().find(|s| self.project.has(s.package())).unwrap_or(server);
        if self.uv {
            return Some(self.tool(checker.package()));
        }
        self.tools.pick_server(server).map(|(s, _)| Command::new(s.package()))
    }
}

/// The tasks for `project` in folder `root`, with what's on this machine.
pub fn tasks(project: &Project, tools: &Tools, server: Server, root: &str) -> Tasks {
    // uv runs a project it manages, and one there's no other Python for.
    let uv = tools.uv.is_some() && (project.uv || tools.python.is_none());
    let r = Runner { project, tools, uv };
    let mut t = Tasks::default();
    if let Some(run) = r.run() {
        t.add("run", run);
    }
    t.add("test", r.test());
    if let Some(ruff) = r.ruff() {
        t.add("lint", ruff.clone().args(["check", "."]));
        t.add("format", ruff.args(["format", "."]));
    }
    if let Some(check) = r.typecheck(server) {
        t.add("typecheck", check);
    }
    if project.uv && tools.uv.is_some() {
        t.add("sync", tools.uv().arg("sync"));
    }
    t.env_file(&format!("{root}/.env"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{tests::temp, Dep};
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn dep(name: &str, group: Option<&str>) -> Dep {
        Dep { name: name.into(), group: group.map(str::to_string) }
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn a_uv_project_runs_its_own_tools() {
        let project = Project { uv: true, locked: true, scripts: vec!["orbit".into()], deps: vec![dep("fastapi", None), dep("pytest", Some("dev")), dep("ruff", Some("dev")), dep("mypy", Some("dev"))], ..Project::default() };
        let tools = Tools { uv: Some("uv".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&project, &tools, Server::Basedpyright, "/nowhere")),
            rows(&[("run", "uv run orbit"), ("test", "uv run pytest"), ("lint", "uv run ruff check ."), ("format", "uv run ruff format ."), ("typecheck", "uv run mypy ."), ("sync", "uv sync")])
        );
    }

    #[test]
    fn what_a_uv_project_doesnt_ask_for_uv_brings() {
        let project = Project { uv: true, main_module: Some("star_map".into()), ..Project::default() };
        let tools = Tools { uv: Some("uv".into()), ruff: Some("/r/ruff".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&project, &tools, Server::Pyright, "/nowhere")),
            rows(&[("run", "uv run python -m star_map"), ("test", "uv run --with pytest pytest"), ("lint", "/r/ruff check ."), ("format", "/r/ruff format ."), ("typecheck", "uv run --with pyright pyright"), ("sync", "uv sync")])
        );
    }

    #[test]
    fn a_django_project_without_uv_runs_manage_py_and_its_env() {
        let dir = temp("tasks");
        std::fs::write(dir.join(".env"), "DJANGO_DEBUG=1\n").unwrap();
        let project = Project { django: true, entry: Some("main.py".into()), deps: vec![dep("django", None)], ..Project::default() };
        let tools = Tools { python: Some("/usr/bin/python3".into()), basedpyright: Some("/b/basedpyright-langserver".into()), ..Tools::default() };
        let t = tasks(&project, &tools, Server::Basedpyright, &dir.to_string_lossy());
        assert_eq!(
            table(&t),
            rows(&[("run", "/usr/bin/python3 manage.py runserver"), ("test", "/usr/bin/python3 manage.py test"), ("typecheck", "basedpyright")]),
            "no ruff to lint with"
        );
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("DJANGO_DEBUG".to_string(), "1".to_string())]));
        std::fs::remove_dir_all(&dir).unwrap();
        // pytest-django: pytest runs Django's tests.
        let project = Project { django: true, deps: vec![dep("pytest-django", Some("dev"))], ..project };
        assert_eq!(table(&tasks(&project, &tools, Server::Basedpyright, "/nowhere"))[1].1, "/usr/bin/python3 -m pytest");
    }

    #[test]
    fn a_folder_of_scripts_with_only_uv_is_run_by_uv() {
        let project = Project { entry: Some("app.py".into()), ..Project::default() };
        let tools = Tools { uv: Some("uv".into()), ..Tools::default() };
        let t = table(&tasks(&project, &tools, Server::Basedpyright, "/nowhere"));
        assert_eq!(t[0], ("run".to_string(), "uv run python app.py".to_string()));
        assert_eq!(t[2], ("lint".to_string(), "uv tool run ruff check .".to_string()));
        assert!(!t.iter().any(|(n, _)| n == "sync"), "not uv's project");
    }
}
