//! The project doctor's Ada checks. In an Alire crate: Alire, and the
//! GNAT it has selected (`alr version`'s `tool #1 gnat:`), with fix rows
//! that install Alire's release and select its GNAT and gprbuild
//! (`alr toolchain --select gnat_native gprbuild`). Else: GNAT, and
//! gprbuild when there's a project file, each with a fix row that has
//! Alire install them (`alr install gnat_native gprbuild`, into
//! `~/.alire/bin`), and Alire itself. Then the Ada Language Server, and
//! with SPARK, GNATprove: the crate's when it's a dependency (asked
//! through `alr exec`), else the one found, installed by Alire
//! (`alr install gnatprove`) -- not `alr with gnatprove`, which changes
//! the crate's manifest.

use fenix_extension_api::{Check, Health, Section};
use fenix_lang_kit::{find_version, Fix, Output, Program};

use crate::project::Project;
use crate::tools::{hints, Platform, Tools};

/// The fix rows' commands that aren't lang-kit's.
pub const INSTALL_GNAT: &str = "ada.install-gnat";
pub const INSTALL_GNATPROVE: &str = "ada.install-gnatprove";
pub const SELECT_TOOLCHAIN: &str = "ada.select-toolchain";

/// How long `alr exec` has to answer: it may sync the crate first.
const ALR_EXEC_MILLIS: u32 = 60_000;

/// `name`, looked for where the pack installs it, Alire does, and
/// GNAT's installers do.
fn program(name: &str, label: &str) -> Program {
    hints(name).into_iter().fold(Program::new(name, label), Program::hint)
}

/// Its fix row installs the release of that name, when there's one for
/// this machine (`ada.install.<name>`).
fn release_fix(program: Program, label: &str, name: &str, platform: &Platform) -> Program {
    if platform.installs().iter().any(|i| i.name == name) {
        program.fix(label, &format!("ada.install.{name}"), Fix::Install(name.into()))
    } else {
        program
    }
}

/// What `alr version` says of the GNAT Alire uses: `tool #1 gnat:
/// gnat_native=16.1.0`, or `not configured`.
pub fn alire_gnat(out: &Output) -> (Health, String) {
    let text = out.text();
    let Some(gnat) = text.lines().find_map(|l| l.trim().strip_prefix("tool #1 gnat:")).map(str::trim) else {
        return (Health::Warn, format!("alr version didn't say: {}", out.why()));
    };
    if gnat.is_empty() || gnat == "not configured" {
        (Health::Bad, "Alire has no GNAT selected: alr builds nothing until one is".into())
    } else {
        let gprbuild = text.lines().find_map(|l| l.trim().strip_prefix("tool #2 gprbuild:")).map(str::trim).filter(|g| *g != "not configured");
        (Health::Ok, gprbuild.map_or_else(|| gnat.to_string(), |g| format!("{gnat}, {g}")))
    }
}

/// What the crate's GNATprove said, through `alr exec`.
fn crates_gnatprove(out: &Output) -> (Health, String) {
    match find_version(&out.text()).filter(|_| out.ok()) {
        Some(v) => (Health::Ok, format!("{v} (the crate's dependency)")),
        None => (Health::Warn, format!("alr exec -- gnatprove failed: {}", out.why())),
    }
}

/// The checks for `project`, with the tools found (the fix rows run
/// Alire by its path).
pub fn programs(project: &Project, tools: &Tools, platform: &Platform) -> Vec<Program> {
    let mut out = Vec::new();
    let alire = program("alr", "Alire");
    if let Some(c) = &project.alire {
        out.push(release_fix(alire, "Install Alire", "alr", platform));
        let select = tools.alr().args(["toolchain", "--select", "gnat_native", "gprbuild"]);
        out.push(program("alr", "GNAT (Alire's)").version_args(["-n", "version"]).judge(alire_gnat).fix("Select Alire's GNAT and gprbuild", SELECT_TOOLCHAIN, Fix::Task(select)));
        if project.spark && c.gnatprove {
            out.push(program("alr", "GNATprove").version_args(["-n", "exec", "--", "gnatprove", "--version"]).timeout(ALR_EXEC_MILLIS).judge(crates_gnatprove));
        }
    } else if project.is_ada() {
        out.push(program("gnat", "GNAT").fix("Install GNAT and gprbuild with Alire", INSTALL_GNAT, Fix::Own));
        if !project.gprs.is_empty() {
            out.push(program("gprbuild", "gprbuild").fix("Install GNAT and gprbuild with Alire", INSTALL_GNAT, Fix::Own));
        }
        out.push(release_fix(alire.optional(), "Install Alire", "alr", platform));
    }
    if project.is_ada() {
        out.push(release_fix(program("ada_language_server", "Ada Language Server").optional(), "Install the Ada Language Server", "als", platform));
    }
    if project.spark && !project.alire.as_ref().is_some_and(|c| c.gnatprove) {
        out.push(program("gnatprove", "GNATprove").optional().fix("Install GNATprove with Alire", INSTALL_GNATPROVE, Fix::Own));
    }
    out
}

/// The project file's check, when there's something to say: the one
/// chosen isn't there, or there are several and none is chosen.
pub fn project_file(gpr: &Result<Option<String>, String>) -> Option<Check> {
    match gpr {
        Err(why) => Some(Check::new(Section::Project, "Project file", Health::Warn, why.clone())),
        Ok(_) => None,
    }
}

/// What a fix row the pack runs itself does: `alr install` of `crates`,
/// with the Alire found. Without one, there's nothing to install with.
pub fn alr_install(tools: &Tools, crates: &[&str]) -> Result<fenix_extension_api::Command, String> {
    if tools.alr.is_none() {
        return Err("Alire installs it, and Alire isn't installed: its row in the doctor installs it first".into());
    }
    Ok(tools.alr().arg("install").args(crates.iter().copied()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Crate;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.name.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    fn row(label: &str, name: &str, required: bool, fix: Option<&str>) -> (String, String, bool, Option<String>) {
        (label.into(), name.into(), required, fix.map(str::to_string))
    }

    #[test]
    fn a_crate_needs_alire_and_the_gnat_it_selected() {
        let linux = Platform::new("linux", "x86_64");
        let project = Project { alire: Some(Crate::parse(include_str!("samples/alire.toml"))), spark: true, ..Project::default() };
        let tools = Tools { alr: Some("/t/alr".into()), ..Tools::default() };
        let p = programs(&project, &tools, &linux);
        assert_eq!(
            rows(&p),
            [
                row("Alire", "alr", true, Some("ada.install.alr")),
                row("GNAT (Alire's)", "alr", true, Some(SELECT_TOOLCHAIN)),
                row("Ada Language Server", "ada_language_server", false, Some("ada.install.als")),
                row("GNATprove", "gnatprove", false, Some(INSTALL_GNATPROVE)),
            ]
        );
        assert_eq!(p[0].hints[0], "/tools/alr/bin");
        let Some((_, _, Fix::Task(select))) = &p[1].fix else { panic!("{:?}", p[1].fix) };
        assert_eq!(crate::tests::line(select), "/t/alr -n toolchain --select gnat_native gprbuild");

        // What alr 2.1.1 says, before a GNAT is selected and after.
        let before = p[1].evaluate(Some("/t/alr"), Some(&out(0, include_str!("samples/alr-version.txt"))));
        assert_eq!((before.health, before.detail.as_str()), (Health::Bad, "Alire has no GNAT selected: alr builds nothing until one is"));
        assert!(before.fix.is_some());
        let selected = include_str!("samples/alr-version.txt").replace("gnat:              not configured", "gnat:              gnat_native=16.1.0").replace("gprbuild:          not configured", "gprbuild:          gprbuild=26.0.1");
        let after = p[1].evaluate(Some("/t/alr"), Some(&out(0, &selected)));
        assert_eq!((after.health, after.detail.as_str()), (Health::Ok, "gnat_native=16.1.0, gprbuild=26.0.1"));
        assert_eq!(alire_gnat(&out(1, "")).0, Health::Warn);
        let c = p[0].evaluate(Some("/t/alr"), Some(&out(0, "alr 2.1.1\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "2.1.1 (/t/alr)"));

        // GNATprove the crate's dependency: asked through alr exec.
        let c = Crate::parse("name = \"orbit\"\n[[depends-on]]\ngnatprove = \"^16\"\n");
        let p = programs(&Project { alire: Some(c), spark: true, ..Project::default() }, &tools, &linux);
        let prove = p.iter().find(|p| p.label == "GNATprove").unwrap();
        assert_eq!((prove.name.as_str(), prove.version_args.join(" "), prove.timeout), ("alr", "-n exec -- gnatprove --version".to_string(), Some(60_000)));
        let c = prove.evaluate(Some("/t/alr"), Some(&out(0, "FSF 16.1.0\nWhy3 for gnatprove version 1.8.2+git\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "16.1.0 (the crate's dependency)"));
        assert_eq!(prove.evaluate(Some("/t/alr"), Some(&out(1, ""))).health, Health::Warn);
    }

    #[test]
    fn a_gnat_project_needs_gnat_and_gprbuild() {
        let project = Project { gprs: vec!["orbit.gpr".into()], sources: vec!["src/orbit.adb".into()], ..Project::default() };
        let p = programs(&project, &Tools::default(), &Platform::new("windows", "aarch64"));
        assert_eq!(
            rows(&p),
            [row("GNAT", "gnat", true, Some(INSTALL_GNAT)), row("gprbuild", "gprbuild", true, Some(INSTALL_GNAT)), row("Alire", "alr", false, None), row("Ada Language Server", "ada_language_server", false, None)],
            "no releases for Windows on Arm"
        );
        let c = p[0].evaluate(Some("/o/gnat"), Some(&out(0, "GNAT 16.1.0\nCopyright (C) 1996-2025, Free Software Foundation, Inc.\n")));
        assert_eq!(c.detail, "16.1.0 (/o/gnat)");
        let c = p[1].evaluate(Some("/o/gprbuild"), Some(&out(0, "GPRBUILD 26.0.0 (2026-04-15) (x86_64-pc-linux-gnu)\nCopyright (C) 2004-2026, AdaCore\n")));
        assert_eq!(c.detail, "26.0.0 (/o/gprbuild)");
        assert_eq!(p[0].evaluate(None, None).health, Health::Bad);
        assert!(programs(&Project::default(), &Tools::default(), &Platform::new("linux", "x86_64")).is_empty(), "not an Ada project");
    }

    #[test]
    fn alire_installs_what_the_fix_rows_ask_for() {
        assert!(alr_install(&Tools::default(), &["gnatprove"]).is_err());
        let tools = Tools { alr: Some("/t/alr".into()), ..Tools::default() };
        assert_eq!(crate::tests::line(&alr_install(&tools, &["gnat_native", "gprbuild"]).unwrap()), "/t/alr -n install gnat_native gprbuild");
        assert!(project_file(&Ok(Some("orbit.gpr".into()))).is_none());
        let c = project_file(&Err("a.gpr, b.gpr: choose one with ada.projectFile".into())).unwrap();
        assert_eq!((c.section, c.health), (Section::Project, Health::Warn));
    }
}
