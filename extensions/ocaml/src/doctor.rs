//! The project doctor's OCaml checks, in an OCaml project: opam; OCaml;
//! dune, held to `dune-project`'s `(lang dune ...)`; ocaml-lsp-server;
//! ocamlformat, held to the version `.ocamlformat` pins, where it formats;
//! and earlybird, the debugger, where there's an executable to debug.
//! Each is asked where it was found, or, when it wasn't but opam is,
//! through `opam exec --` in the switch opam picks for the project. A fix
//! row runs `opam install`; without opam, it opens opam's page. OCaml
//! itself is the switch's compiler: `opam switch create` makes one, which
//! the pack leaves to you.

use fenix_extension_api::Health;
use fenix_lang_kit::{find_version, Fix, Output, Program, Requirement};

use crate::project::Project;
use crate::tools::{hints, Tool, Tools, DUNE, EARLYBIRD, OCAML, OCAMLFORMAT, OCAMLLSP};

/// The fix rows' commands, in the manifest.
pub const GET_OPAM: &str = "ocaml.get-opam";
pub const INSTALL_DUNE: &str = "ocaml.install-dune";
pub const INSTALL_LSP: &str = "ocaml.install-lsp";
pub const INSTALL_OCAMLFORMAT: &str = "ocaml.install-ocamlformat";
pub const INSTALL_EARLYBIRD: &str = "ocaml.install-earlybird";

/// How long `opam exec` has to answer: opam reads its switch first.
pub const OPAM_MILLIS: u32 = 15_000;

/// What `opam exec -- <tool> --version` said: the version, held to
/// `wants`; a failure is the tool not being in the switch.
pub fn judge_in_switch(out: &Output, wants: Option<&Requirement>, required: bool) -> (Health, String) {
    let missing = if required { Health::Bad } else { Health::Warn };
    if !out.ok() {
        return (missing, format!("it isn't in opam's switch for the project: {}", out.why()));
    }
    match (find_version(&out.text()), wants) {
        (Some(v), Some(r)) if !r.matches(&v) => (missing, format!("{v}, but the project needs {r} (opam's switch)")),
        (Some(v), _) => (Health::Ok, format!("{v} (opam's switch)")),
        (None, _) => (Health::Warn, "its version couldn't be read (opam's switch)".into()),
    }
}

/// `tool`'s check: where it was found, else through opam.
fn check(tool: &Tool, tools: &Tools, args: &[&str], wants: Option<&str>, required: bool) -> Program {
    let wants = wants.and_then(Requirement::parse);
    let p = if tools.found(tool.program).is_none() && tools.opam.is_some() {
        let held = wants.clone();
        let args: Vec<String> = ["exec", "--", tool.program].iter().chain(args).map(|a| a.to_string()).collect();
        Program::new("opam", tool.label).version_args(args).timeout(OPAM_MILLIS).judge(move |out| judge_in_switch(out, held.as_ref(), required))
    } else {
        let p = hints().into_iter().fold(Program::new(tool.program, tool.label), Program::hint).version_args(args.iter().copied());
        match wants {
            Some(r) => p.wants(&r.to_string()),
            None => p,
        }
    };
    if required {
        p
    } else {
        p.optional()
    }
}

/// Its fix row: `opam install <package>` when there's opam, else opam's
/// page.
fn opam_fix(program: Program, tools: &Tools, package: &str, label: &str, command: &str) -> Program {
    match tools.opam_install(&[package]) {
        Some(cmd) => program.fix(&format!("Install {label} (opam install {package})"), command, Fix::Task(cmd)),
        None => program.fix(&format!("Get opam, which installs {label} (opam.ocaml.org)"), GET_OPAM, Fix::Own),
    }
}

/// The checks for `project`, with what's on this machine; `format` when
/// ocamlformat formats it, `debug` when earlybird is wanted.
pub fn programs(project: &Project, tools: &Tools, format: bool, debug: bool) -> Vec<Program> {
    let mut out = Vec::new();
    if !project.is_ocaml() {
        return out;
    }
    let opam = Program::new("opam", "opam").optional();
    out.push(if tools.opam.is_none() { opam.fix("Get opam (opam.ocaml.org)", GET_OPAM, Fix::Own) } else { opam });

    let ocaml = check(&OCAML, tools, &["-vnum"], None, true);
    out.push(if tools.opam.is_none() { ocaml.fix("Get opam, which installs OCaml (opam.ocaml.org)", GET_OPAM, Fix::Own) } else { ocaml });

    if project.is_dune() {
        let wants = project.dune_lang().map(|v| format!(">={v}"));
        out.push(opam_fix(check(&DUNE, tools, &["--version"], wants.as_deref(), true), tools, DUNE.package, "dune", INSTALL_DUNE));
    }
    out.push(opam_fix(check(&OCAMLLSP, tools, &["--version"], None, false), tools, OCAMLLSP.package, "ocaml-lsp-server", INSTALL_LSP));
    if format && project.ocamlformat {
        let pinned = project.ocamlformat_version.as_deref();
        let wants = pinned.map(|v| format!("={v}"));
        let package = pinned.map_or_else(|| OCAMLFORMAT.package.to_string(), |v| format!("{}.{v}", OCAMLFORMAT.package));
        out.push(opam_fix(check(&OCAMLFORMAT, tools, &["--version"], wants.as_deref(), false), tools, &package, "ocamlformat", INSTALL_OCAMLFORMAT));
    }
    if debug && !project.executables.is_empty() {
        out.push(opam_fix(check(&EARLYBIRD, tools, &["--version"], None, false), tools, EARLYBIRD.package, "earlybird", INSTALL_EARLYBIRD));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::dune_project;
    use crate::project::Lone;
    use crate::tests::line;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.name.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    #[test]
    fn in_a_switch_each_is_asked_through_opam() {
        let dir = dune_project("doctor");
        let project = Project::read(&dir.to_string_lossy());
        let tools = Tools { opam: Some("/o/opam".into()), ..Tools::default() };
        let p = programs(&project, &tools, true, true);
        assert_eq!(
            rows(&p),
            [
                ("opam".into(), "opam".into(), false, None),
                ("OCaml".into(), "opam".into(), true, None),
                ("dune".into(), "opam".into(), true, Some(INSTALL_DUNE.into())),
                ("ocaml-lsp-server".into(), "opam".into(), false, Some(INSTALL_LSP.into())),
                ("ocamlformat".into(), "opam".into(), false, Some(INSTALL_OCAMLFORMAT.into())),
                ("earlybird".into(), "opam".into(), false, Some(INSTALL_EARLYBIRD.into())),
            ]
        );
        assert_eq!(p[3].version_args, ["exec", "--", "ocamllsp", "--version"]);
        assert_eq!(p[1].version_args, ["exec", "--", "ocaml", "-vnum"]);
        // dune held to (lang dune 3.16), ocamlformat to its pin.
        let c = p[2].evaluate(Some("/o/opam"), Some(&out(0, "3.15.3\n", "")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "3.15.3, but the project needs >=3.16 (opam's switch)"));
        assert_eq!(p[2].evaluate(Some("/o/opam"), Some(&out(0, "3.20.2\n", ""))).health, Health::Ok);
        let c = p[4].evaluate(Some("/o/opam"), Some(&out(0, "0.26.2\n", "")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Warn, "0.26.2, but the project needs =0.27.0 (opam's switch)"));
        match &p[4].fix {
            Some((label, _, Fix::Task(cmd))) => assert_eq!((label.as_str(), line(cmd).as_str()), ("Install ocamlformat (opam install ocamlformat.0.27.0)", "/o/opam install ocamlformat.0.27.0 --yes")),
            other => panic!("{other:?}"),
        }
        // Not in the switch: opam exec fails.
        let c = p[3].evaluate(Some("/o/opam"), Some(&out(1, "", "[ERROR] Command not found 'ocamllsp'")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Warn, "it isn't in opam's switch for the project: [ERROR] Command not found 'ocamllsp'"));
        assert_eq!(c.fix.map(|f| f.1), Some(INSTALL_LSP.into()));
        assert_eq!(p[1].evaluate(Some("/o/opam"), Some(&out(2, "", "[ERROR] No switch is currently set."))).health, Health::Bad);
        assert_eq!(p[3].timeout, Some(OPAM_MILLIS));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn found_directly_each_is_asked_as_it_is() {
        let dir = dune_project("direct");
        let project = Project::read(&dir.to_string_lossy());
        let tools = Tools { ocaml: Some("/n/ocaml".into()), dune: Some("/n/dune".into()), ocamllsp: Some("/n/ocamllsp".into()), ..Tools::default() };
        let p = programs(&project, &tools, false, false);
        assert_eq!(
            rows(&p),
            [
                ("opam".into(), "opam".into(), false, Some(GET_OPAM.into())),
                ("OCaml".into(), "ocaml".into(), true, Some(GET_OPAM.into())),
                ("dune".into(), "dune".into(), true, Some(GET_OPAM.into())),
                ("ocaml-lsp-server".into(), "ocamllsp".into(), false, Some(GET_OPAM.into())),
            ],
            "Nix's, without opam: neither ocamlformat (off) nor earlybird (off)"
        );
        let c = p[2].evaluate(Some("/n/dune"), Some(&out(0, "3.15.3\n", "")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "3.15.3, but the project needs >=3.16 (/n/dune)"));
        assert_eq!(p[1].evaluate(Some("/n/ocaml"), Some(&out(0, "5.3.0\n", ""))).detail, "5.3.0 (/n/ocaml)");
        assert!(p[1].hints.contains(&"$HOME/.opam/default/bin".to_string()));
        assert!(matches!(p[0].fix, Some((_, _, Fix::Own))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scripts_need_ocaml_and_no_dune() {
        let project = Project { lone: vec![Lone { file: "hello.ml".into() }], ..Project::default() };
        let labels: Vec<String> = programs(&project, &Tools::default(), true, true).into_iter().map(|p| p.label).collect();
        assert_eq!(labels, ["opam", "OCaml", "ocaml-lsp-server"], "no .ocamlformat, no executable");
        assert!(programs(&Project::default(), &Tools::default(), true, true).is_empty(), "not an OCaml project");
    }
}
