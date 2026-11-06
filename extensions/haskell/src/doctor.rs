//! The project doctor's Haskell checks, in a Haskell project: GHC; Cabal
//! or Stack, whichever builds it; the Haskell Language Server; and hlint
//! and the formatter, where they're on. GHC, Cabal, Stack and HLS come
//! from ghcup: a fix row runs `ghcup install`, or, without ghcup, opens
//! its page, which says how to get it. hlint, ormolu and fourmolu are
//! Hackage's, installed by Cabal (or Stack). Stack brings the GHC each
//! project asks for itself, so GHC on `PATH` isn't needed for it.

use fenix_extension_api::Command;
use fenix_lang_kit::{Fix, Program};

use crate::project::Project;
use crate::tools::{hints, BuildTool, Formatter, Tools};

/// The fix rows' commands, in the manifest.
pub const GET_GHCUP: &str = "haskell.get-ghcup";
pub const INSTALL_GHC: &str = "haskell.install-ghc";
pub const INSTALL_CABAL: &str = "haskell.install-cabal";
pub const INSTALL_STACK: &str = "haskell.install-stack";
pub const INSTALL_HLS: &str = "haskell.install-hls";
pub const INSTALL_HLINT: &str = "haskell.install-hlint";
pub const INSTALL_FORMATTER: &str = "haskell.install-formatter";

/// How long HLS's wrapper has to say its version: it looks for the
/// server of the GHC it finds first.
const HLS_MILLIS: u32 = 10_000;

/// `name`, looked for where ghcup, Cabal and Stack put it.
fn program(name: &str, label: &str) -> Program {
    hints().into_iter().fold(Program::new(name, label), Program::hint).version_args(["--numeric-version"])
}

/// Its fix row: ghcup's install of `tool` when there's ghcup, else
/// ghcup's page.
fn ghcup_fix(program: Program, tools: &Tools, tool: &str, label: &str, command: &str) -> Program {
    match tools.ghcup_install(tool) {
        Some(cmd) => program.fix(&format!("Install {label} (ghcup)"), command, Fix::Task(cmd)),
        None => program.fix(&format!("Get ghcup, which installs {label} (haskell.org)"), GET_GHCUP, Fix::Own),
    }
}

fn task_fix(program: Program, label: &str, command: &str, cmd: Option<Command>) -> Program {
    match cmd {
        Some(cmd) => program.fix(label, command, Fix::Task(cmd)),
        None => program,
    }
}

/// The checks for `project`, built by `tool`, with what's on this
/// machine; `format` is the formatter in use, `lint` whether hlint is.
pub fn programs(project: &Project, tools: &Tools, tool: BuildTool, format: Formatter, lint: bool) -> Vec<Program> {
    let mut out = Vec::new();
    if !project.is_haskell() {
        return out;
    }
    let stack = tool == BuildTool::Stack;

    let ghc = program("ghc", "GHC");
    let ghc = if stack { ghc.optional() } else { ghc };
    out.push(ghcup_fix(ghc, tools, "ghc", "GHC", INSTALL_GHC));
    if stack {
        out.push(ghcup_fix(program("stack", "Stack"), tools, "stack", "Stack", INSTALL_STACK));
    } else if !project.packages.is_empty() || project.cabal_project {
        out.push(ghcup_fix(program("cabal", "Cabal"), tools, "cabal", "Cabal", INSTALL_CABAL));
    }

    // The wrapper, else a server of its own.
    let hls = match (&tools.hls_wrapper, &tools.hls) {
        (None, Some(_)) => program("haskell-language-server", "Haskell Language Server"),
        _ => program("haskell-language-server-wrapper", "Haskell Language Server"),
    };
    out.push(ghcup_fix(hls.optional().timeout(HLS_MILLIS), tools, "hls", "HLS", INSTALL_HLS));

    if lint {
        let p = program("hlint", "hlint").optional();
        out.push(task_fix(p, "Install hlint (cabal install)", INSTALL_HLINT, tools.hackage_install("hlint")));
    }
    if format != Formatter::Off {
        let name = format.program();
        let p = program(name, name).version_args(["--version"]).optional();
        out.push(task_fix(p, &format!("Install {name} (cabal install)"), INSTALL_FORMATTER, tools.hackage_install(name)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{parse_cabal, Lone};
    use crate::tests::line;
    use fenix_extension_api::Health;
    use fenix_lang_kit::Output;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.name.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    fn cabal() -> Project {
        Project { packages: vec![parse_cabal(include_str!("samples/orbit.cabal"))], ..Project::default() }
    }

    #[test]
    fn with_ghcup_each_is_installed_by_it() {
        let tools = Tools { ghcup: Some("/g/ghcup".into()), cabal: Some("/g/cabal".into()), ..Tools::default() };
        let p = programs(&cabal(), &tools, BuildTool::Cabal, Formatter::Ormolu, true);
        assert_eq!(
            rows(&p),
            [
                ("GHC".into(), "ghc".into(), true, Some(INSTALL_GHC.into())),
                ("Cabal".into(), "cabal".into(), true, Some(INSTALL_CABAL.into())),
                ("Haskell Language Server".into(), "haskell-language-server-wrapper".into(), false, Some(INSTALL_HLS.into())),
                ("hlint".into(), "hlint".into(), false, Some(INSTALL_HLINT.into())),
                ("ormolu".into(), "ormolu".into(), false, Some(INSTALL_FORMATTER.into())),
            ]
        );
        match &p[2].fix {
            Some((label, _, Fix::Task(cmd))) => assert_eq!((label.as_str(), line(cmd).as_str()), ("Install HLS (ghcup)", "/g/ghcup install hls recommended")),
            other => panic!("{other:?}"),
        }
        assert_eq!(p[0].version_args, ["--numeric-version"]);
        assert_eq!(p[4].version_args, ["--version"], "ormolu has no --numeric-version");
        assert!(p[0].hints.contains(&"$HOME/.ghcup/bin".to_string()));
        let c = p[0].evaluate(Some("/g/ghc"), Some(&out(0, "9.10.2\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "9.10.2 (/g/ghc)"));
        assert_eq!(p[0].evaluate(None, None).health, Health::Bad, "nothing builds without GHC");
        assert_eq!(p[2].evaluate(None, None).health, Health::Warn, "Fenix works without the server");
        let ormolu = p[4].evaluate(Some("/g/ormolu"), Some(&out(0, "ormolu 0.7.7.0 UNKNOWN UNKNOWN\nusing ghc-lib-parser 9.10.1.20240511\n")));
        assert_eq!(ormolu.detail, "0.7.7.0 (/g/ormolu)");
    }

    #[test]
    fn without_ghcup_its_page_says_how() {
        let p = programs(&cabal(), &Tools::default(), BuildTool::Cabal, Formatter::Off, false);
        assert_eq!(rows(&p).iter().map(|r| r.3.clone()).collect::<Vec<_>>(), [Some(GET_GHCUP.into()), Some(GET_GHCUP.into()), Some(GET_GHCUP.into())], "no Cabal to install hlint with, and neither is on");
        assert_eq!(p[0].fix.as_ref().map(|f| f.0.as_str()), Some("Get ghcup, which installs GHC (haskell.org)"));
        assert!(matches!(p[0].fix, Some((_, _, Fix::Own))));
    }

    #[test]
    fn stack_brings_its_own_ghc() {
        let project = Project { stack: true, ..cabal() };
        let tools = Tools { stack: Some("/g/stack".into()), hls: Some("/nix/hls".into()), ..Tools::default() };
        let p = programs(&project, &tools, BuildTool::Stack, Formatter::Fourmolu, true);
        assert_eq!(
            rows(&p),
            [
                ("GHC".into(), "ghc".into(), false, Some(GET_GHCUP.into())),
                ("Stack".into(), "stack".into(), true, Some(GET_GHCUP.into())),
                ("Haskell Language Server".into(), "haskell-language-server".into(), false, Some(GET_GHCUP.into())),
                ("hlint".into(), "hlint".into(), false, Some(INSTALL_HLINT.into())),
                ("fourmolu".into(), "fourmolu".into(), false, Some(INSTALL_FORMATTER.into())),
            ],
            "the server found is the one checked; Stack installs hlint"
        );
        match &p[3].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(line(cmd), "/g/stack install hlint"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn scripts_need_ghc_and_no_build_tool() {
        let project = Project { lone: vec![Lone { file: "Hello.hs".into(), main: true }], ..Project::default() };
        let labels: Vec<String> = programs(&project, &Tools::default(), BuildTool::Cabal, Formatter::Off, false).into_iter().map(|p| p.label).collect();
        assert_eq!(labels, ["GHC", "Haskell Language Server"]);
        assert!(programs(&Project::default(), &Tools::default(), BuildTool::Cabal, Formatter::Ormolu, true).is_empty(), "not a Haskell project");
    }
}
