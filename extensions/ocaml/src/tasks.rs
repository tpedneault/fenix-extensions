//! The tasks (`SPC p t`) of a dune project: build, test (and each folder
//! with tests), `dune exec` each executable its `dune` files name, `dune
//! fmt`, clean, and the `.opam` files' dependencies installed; with
//! earlybird, each bytecode executable built to debug. In a folder of
//! scripts, `ocaml` runs each file at its top. The project's `.env` goes
//! to each; a task of the same name in `.fenix/tools.json` wins.

use fenix_lang_kit::Tasks;

use crate::project::{Executable, Project};
use crate::tools::Tools;

/// The task that builds `exe`'s bytecode, which a launch runs first.
pub fn bytecode_task(exe: &Executable) -> String {
    format!("build {}", exe.path("bc"))
}

/// The tasks for `project` in folder `root`; `debug` when earlybird
/// debugs its bytecode executables.
pub fn tasks(project: &Project, tools: &Tools, root: &str, debug: bool) -> Tasks {
    let mut t = Tasks::default();
    if project.is_dune() {
        let dune = tools.run("dune");
        t.add("build", dune.clone().arg("build"));
        t.add("test", dune.clone().arg("test"));
        if project.test_dirs.len() > 1 {
            for dir in project.test_dirs.iter().filter(|d| !d.is_empty()) {
                t.add(&format!("test {dir}"), dune.clone().args(["test", dir.as_str()]));
            }
        }
        for exe in &project.executables {
            t.add(&format!("exec {}", exe.label()), dune.clone().args(["exec", &exe.exec_target()]));
        }
        if debug {
            for exe in project.executables.iter().filter(|e| e.byte) {
                t.add(&bytecode_task(exe), dune.clone().args(["build", &format!("./{}", exe.path("bc"))]));
            }
        }
        t.add("fmt", dune.clone().arg("fmt"));
        t.add("clean", dune.clone().arg("clean"));
    } else {
        let ocaml = tools.run("ocaml");
        for lone in &project.lone {
            t.add(&format!("run {}", lone.file), ocaml.clone().arg(lone.file.as_str()));
        }
    }
    if let (false, Some(opam)) = (project.opam_files.is_empty(), &tools.opam) {
        t.add("opam: install dependencies", fenix_extension_api::Command::new(opam).args(["install", ".", "--deps-only", "--with-test", "--yes"]));
    }
    t.env_file(&format!("{root}/.env"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::{dune_project, temp};
    use crate::project::Lone;
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn dune_execs_each_executable() {
        let dir = dune_project("tasks");
        std::fs::create_dir_all(dir.join("test")).unwrap();
        std::fs::write(dir.join("test/dune"), "(test (name test_orbit))\n").unwrap();
        let project = Project::read(&dir.to_string_lossy());
        let tools = Tools { opam: Some("/o/opam".into()), dune: Some("/s/dune".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&project, &tools, "/nowhere", true)),
            rows(&[
                ("build", "/s/dune build"),
                ("test", "/s/dune test"),
                ("test bin", "/s/dune test bin"),
                ("test test", "/s/dune test test"),
                ("exec orbit", "/s/dune exec orbit"),
                ("exec sim", "/s/dune exec ./bin/sim.exe"),
                ("exec replay", "/s/dune exec ./bin/replay.exe"),
                ("build bin/main.bc", "/s/dune build ./bin/main.bc"),
                ("fmt", "/s/dune fmt"),
                ("clean", "/s/dune clean"),
                ("opam: install dependencies", "/o/opam install . --deps-only --with-test --yes"),
            ])
        );
        let opam_only = Tools { opam: Some("/o/opam".into()), ..Tools::default() };
        assert_eq!(table(&tasks(&project, &opam_only, "/nowhere", false))[0], ("build".to_string(), "/o/opam exec -- dune build".to_string()), "dune through opam");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scripts_are_run_alone_with_the_env() {
        let dir = temp("tasks-lone");
        std::fs::write(dir.join(".env"), "ORBIT_ENV=dev\n").unwrap();
        let project = Project { lone: vec![Lone { file: "hello.ml".into() }], ..Project::default() };
        let tools = Tools { ocaml: Some("/n/ocaml".into()), ..Tools::default() };
        let t = tasks(&project, &tools, &dir.to_string_lossy(), true);
        assert_eq!(table(&t), rows(&[("run hello.ml", "/n/ocaml hello.ml")]));
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("ORBIT_ENV".to_string(), "dev".to_string())]));
        assert!(tasks(&Project::default(), &tools, "/nowhere", true).tasks.is_empty(), "not an OCaml project");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
