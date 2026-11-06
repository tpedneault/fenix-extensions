//! The tasks (`SPC p t`), each run in the buf workspace's folder: `buf
//! build`, `buf lint`, `buf format -w`, `buf breaking` against the default
//! branch, `buf generate` for each template (`buf.gen.yaml`, and those
//! named beside it, `--template`), and `buf dep update` when `buf.yaml`
//! has deps. A project without buf has `protoc`'s check of its files when
//! there's protoc. Their output is buf's `file:line:col:message`, which
//! Fenix's quickfix list reads. The project's `.env` goes to each.

use fenix_lang_kit::Tasks;

use crate::project::Project;
use crate::tools::Tools;

/// The tasks for `project` in folder `root`, comparing with `against`'s
/// branch (the setting's, else the default branch).
pub fn tasks(project: &Project, tools: &Tools, against: Option<&str>, root: &str) -> Tasks {
    let mut t = Tasks::default();
    let cwd = project.cwd();
    if project.buf_dir.is_some() {
        let buf = tools.buf();
        t.add("build", buf.clone().arg("build"));
        t.add("lint", buf.clone().arg("lint"));
        t.add("format", buf.clone().args(["format", "-w"]));
        if let Some(branch) = against {
            t.add(&format!("breaking against {branch}"), buf.clone().args(["breaking".to_string(), "--against".to_string(), project.against(branch)]));
        }
        for template in &project.templates {
            match &template.name {
                None => t.add("generate", buf.clone().arg("generate")),
                Some(name) => t.add(&format!("generate {name}"), buf.clone().args(["generate", "--template", template.file.as_str()])),
            }
        }
        if project.buf.as_ref().is_some_and(|b| !b.deps.is_empty()) {
            t.add("dep update", buf.clone().args(["dep", "update"]));
        }
        t = t.cwd(&cwd);
    } else if let (Some(protoc), false) = (&tools.protoc, project.protos.is_empty()) {
        // Each file compiled, what it makes written to the pack's own
        // folder: protoc's own check.
        let cmd = fenix_extension_api::Command::new(protoc).args(["-I", ".", "--descriptor_set_out=${data}/protoc-check.binpb"]).args(project.protos.iter().cloned());
        t.add("protoc check", cmd);
        t = t.cwd(crate::PROJECT);
    }
    t.env_file(&format!("{root}/.env"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{tests::temp, BufYaml, Template};
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd), t.cmd.cwd.clone().unwrap_or_default())).collect()
    }

    #[test]
    fn a_workspace_builds_lints_formats_compares_and_generates() {
        let project = Project {
            buf_dir: Some("api".into()),
            buf: Some(BufYaml { version: "v2".into(), modules: vec!["proto".into()], deps: vec!["buf.build/googleapis/googleapis".into()] }),
            templates: vec![Template::parse("buf.gen.yaml", include_str!("samples/buf.gen.yaml")), Template::parse("buf.gen.docs.yaml", "version: v2\n")],
            default_branch: Some("main".into()),
            protos: Vec::new(),
        };
        let tools = Tools { buf: Some("/t/buf".into()), ..Tools::default() };
        let rows = table(&tasks(&project, &tools, Some("main"), "/nowhere"));
        let names: Vec<(&str, &str)> = rows.iter().map(|(n, l, _)| (n.as_str(), l.as_str())).collect();
        assert_eq!(
            names,
            [
                ("build", "/t/buf build"),
                ("lint", "/t/buf lint"),
                ("format", "/t/buf format -w"),
                ("breaking against main", "/t/buf breaking --against ../.git#branch=main,subdir=api"),
                ("generate", "/t/buf generate"),
                ("generate docs", "/t/buf generate --template buf.gen.docs.yaml"),
                ("dep update", "/t/buf dep update"),
            ]
        );
        assert!(rows.iter().all(|(_, _, cwd)| cwd == "/project/api"), "in the workspace's folder");
        let none = table(&tasks(&project, &Tools::default(), None, "/nowhere"));
        assert!(!none.iter().any(|(n, _, _)| n.starts_with("breaking")), "no repository, nothing to compare with");
        assert_eq!(none[0].1, "buf build", "buf by its name until it's found");
    }

    #[test]
    fn without_buf_protoc_checks_the_files_with_the_env() {
        let dir = temp("tasks");
        std::fs::write(dir.join(".env"), "BUF_TOKEN=x\n").unwrap();
        let project = Project { protos: vec!["a.proto".into(), "b/b.proto".into()], ..Project::default() };
        let tools = Tools { protoc: Some("/t/protoc".into()), ..Tools::default() };
        let t = tasks(&project, &tools, Some("main"), &dir.to_string_lossy());
        assert_eq!(table(&t)[0].1, "/t/protoc -I . --descriptor_set_out=${data}/protoc-check.binpb a.proto b/b.proto");
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("BUF_TOKEN".to_string(), "x".to_string())]));
        assert!(tasks(&project, &Tools::default(), None, "/nowhere").tasks.is_empty(), "no protoc, no buf: nothing");
        assert!(tasks(&Project::default(), &tools, None, "/nowhere").tasks.is_empty(), "not a Protobuf project");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
