//! The tasks (`SPC p t`). In an Alire crate, `alr`'s: build, run (each
//! executable when it has several), test, update and clean. Else, with a
//! project file, gprbuild's: build (and each main on its own, when it
//! lists several) and clean. With SPARK, `prove`: GNATprove at the level
//! `ada.proveLevel` says. `alr` runs with `-n`, so it never waits on a
//! question. The project's `.env` goes to each.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::project::Project;
use crate::tools::Tools;

/// GNATprove on the project `gpr` (`None`: the crate's, which `alr exec
/// -P1` puts first), at `level`, on every core.
pub fn prove(project: &Project, tools: &Tools, gpr: Option<&str>, level: u8) -> Option<Command> {
    let args = [format!("--level={level}"), "-j0".to_string()];
    if project.alire.is_some() {
        // The crate's GNATprove (a dependency) is in alr's environment,
        // by its name; else the one found.
        let gnatprove = if project.alire.as_ref().is_some_and(|c| c.gnatprove) { Command::new("gnatprove") } else { tools.named("gnatprove") };
        return Some(tools.alr().args(["exec", "-P1", "--", gnatprove.program.as_str()]).args(args));
    }
    Some(tools.named("gnatprove").args(["-P", gpr?]).args(args))
}

/// The tasks for `project` in folder `root`, with its project file `gpr`
/// and the proof level.
pub fn tasks(project: &Project, tools: &Tools, gpr: Option<&str>, level: u8, root: &str) -> Tasks {
    let mut t = Tasks::default();
    if let Some(c) = &project.alire {
        let alr = tools.alr();
        t.add("build", alr.clone().arg("build"));
        match c.executables.as_slice() {
            [] | [_] => t.add("run", alr.clone().arg("run")),
            several => {
                for exe in several {
                    t.add(&format!("run {exe}"), alr.clone().args(["run", exe.as_str()]));
                }
            }
        }
        t.add("test", alr.clone().arg("test"));
        t.add("update", alr.clone().arg("update"));
        t.add("clean", alr.clone().arg("clean"));
    } else if let Some(gpr) = gpr {
        t.add("build", tools.named("gprbuild").args(["-P", gpr]));
        let mains = std::fs::read_to_string(format!("{root}/{gpr}")).map(|text| crate::project::mains(&text)).unwrap_or_default();
        if mains.len() > 1 {
            for main in &mains {
                t.add(&format!("build {main}"), tools.named("gprbuild").args(["-P", gpr, main.as_str()]));
            }
        }
        t.add("clean", tools.named("gprclean").args(["-P", gpr]));
    }
    if project.spark {
        if let Some(cmd) = prove(project, tools, gpr, level) {
            t.add("prove", cmd);
        }
    }
    t.env_file(&format!("{root}/.env"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{tests::temp, Crate};
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn a_crate_is_alires_to_build_run_and_prove() {
        let crate_ = Crate::parse(include_str!("samples/alire.toml"));
        let project = Project { alire: Some(crate_), spark: true, ..Project::default() };
        let tools = Tools { alr: Some("/t/alr".into()), gnatprove: Some("/h/.alire/bin/gnatprove".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&project, &tools, Some("orbit.gpr"), 2, "/nowhere")),
            rows(&[
                ("build", "/t/alr -n build"),
                ("run", "/t/alr -n run"),
                ("test", "/t/alr -n test"),
                ("update", "/t/alr -n update"),
                ("clean", "/t/alr -n clean"),
                ("prove", "/t/alr -n exec -P1 -- /h/.alire/bin/gnatprove --level=2 -j0"),
            ])
        );
        // GNATprove a dependency: the crate's own, and an executable each.
        let c = Crate::parse("name = \"ground\"\nexecutables = [\"ground\", \"replay\"]\n[[depends-on]]\ngnatprove = \"^16\"\n");
        let project = Project { alire: Some(c), spark: true, ..Project::default() };
        let t = table(&tasks(&project, &Tools::default(), None, 0, "/nowhere"));
        assert_eq!(&t[1..3], rows(&[("run ground", "alr -n run ground"), ("run replay", "alr -n run replay")]));
        assert_eq!(t.last().unwrap().1, "alr -n exec -P1 -- gnatprove --level=0 -j0");
    }

    #[test]
    fn a_project_file_is_gprbuilds_with_each_main() {
        let dir = temp("tasks");
        std::fs::write(dir.join("ground.gpr"), "project Ground is\n   for Main use (\"ground.adb\", \"replay.adb\");\nend Ground;\n").unwrap();
        std::fs::write(dir.join(".env"), "ORBIT_ENV=dev\n").unwrap();
        let root = dir.to_string_lossy().into_owned();
        let tools = Tools { gprbuild: Some("/g/bin/gprbuild".into()), gprclean: Some("/g/bin/gprclean".into()), ..Tools::default() };
        let project = Project { gprs: vec!["ground.gpr".into()], spark: true, ..Project::default() };
        let t = tasks(&project, &tools, Some("ground.gpr"), 1, &root);
        assert_eq!(
            table(&t),
            rows(&[
                ("build", "/g/bin/gprbuild -P ground.gpr"),
                ("build ground.adb", "/g/bin/gprbuild -P ground.gpr ground.adb"),
                ("build replay.adb", "/g/bin/gprbuild -P ground.gpr replay.adb"),
                ("clean", "/g/bin/gprclean -P ground.gpr"),
                ("prove", "gnatprove -P ground.gpr --level=1 -j0"),
            ])
        );
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("ORBIT_ENV".to_string(), "dev".to_string())]));
        // Several project files and none chosen: nothing to build.
        let project = Project { gprs: vec!["a.gpr".into(), "b.gpr".into()], spark: true, ..Project::default() };
        assert!(tasks(&project, &tools, None, 1, &root).tasks.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
