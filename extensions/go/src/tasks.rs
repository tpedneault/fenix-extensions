//! The tasks (`SPC p t`): build, run (each main package), test, vet,
//! lint with golangci-lint, format, tidy, and govulncheck, as each is
//! installed. The project's `.env` goes to each; a task of the same name
//! in `.fenix/tools.json` wins.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::Project;
use crate::tools::Tools;

/// `run`, or `run orbitctl` for `cmd/orbitctl` beside a main package at
/// the top.
fn run_name(main: &str, only: bool) -> String {
    match main.rsplit('/').next() {
        _ if only => "run".into(),
        Some(".") | None => "run".into(),
        Some(name) => format!("run {name}"),
    }
}

/// The tasks for `project` in folder `root`, with what's on this machine.
pub fn tasks(project: &Project, tools: &Tools, root: &str) -> Tasks {
    let go = tools.go();
    let mut t = Tasks::default();
    t.add("build", go.clone().args(["build", "./..."]));
    for main in &project.mains {
        let package = if main == "." { ".".to_string() } else { format!("./{main}") };
        t.add(&run_name(main, project.mains.len() == 1), go.clone().args(["run".to_string(), package]));
    }
    t.add("test", go.clone().args(["test", "./..."]));
    t.add("vet", go.clone().args(["vet", "./..."]));
    if let Some(lint) = &tools.golangci_lint {
        t.add("lint", Command::new(lint).args(["run", "./..."]));
    }
    match &tools.goimports {
        Some(goimports) => t.add("format", Command::new(goimports).args(["-w", "."])),
        None => t.add("format", go.clone().args(["fmt", "./..."])),
    }
    if project.module.is_some() {
        t.add("tidy", go.clone().args(["mod", "tidy"]));
    }
    if let Some(vulncheck) = &tools.govulncheck {
        t.add("vulncheck", Command::new(vulncheck).arg("./..."));
    }
    t.env_file(&format!("{root}/.env"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{tests::temp, GoMod};
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn a_module_with_its_tools_gets_them_all() {
        let project = Project { module: Some(GoMod::default()), mains: vec![".".into(), "cmd/orbitctl".into()], ..Project::default() };
        let tools = Tools { go: Some("go".into()), goimports: Some("/h/goimports".into()), golangci_lint: Some("/h/golangci-lint".into()), govulncheck: Some("/h/govulncheck".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&project, &tools, "/nowhere")),
            rows(&[
                ("build", "go build ./..."),
                ("run", "go run ."),
                ("run orbitctl", "go run ./cmd/orbitctl"),
                ("test", "go test ./..."),
                ("vet", "go vet ./..."),
                ("lint", "/h/golangci-lint run ./..."),
                ("format", "/h/goimports -w ."),
                ("tidy", "go mod tidy"),
                ("vulncheck", "/h/govulncheck ./..."),
            ])
        );
    }

    #[test]
    fn a_library_with_only_go_and_its_env() {
        let dir = temp("tasks");
        std::fs::write(dir.join(".env"), "ORBIT_ENV=dev\n").unwrap();
        let project = Project { mains: vec!["cmd/orbitctl".into()], ..Project::default() };
        let t = tasks(&project, &Tools::default(), &dir.to_string_lossy());
        assert_eq!(table(&t), rows(&[("build", "go build ./..."), ("run", "go run ./cmd/orbitctl"), ("test", "go test ./..."), ("vet", "go vet ./..."), ("format", "go fmt ./...")]), "one main package is run; no go.mod, no tidy");
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("ORBIT_ENV".to_string(), "dev".to_string())]));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
