//! The project doctor's Python checks: the Python the project gets (uv's
//! choice, held to `requires-python`), uv, the venv in step with
//! `uv.lock`, the language server, ruff, and debugpy where uv doesn't
//! bring it. Each that isn't right has a fix row that installs it.

use fenix_extension_api::{Health, Section};
use fenix_lang_kit::{Fix, Output, Program};

use crate::project::Project;
use crate::ruff::{self, Mode};
use crate::tools::{hints, python_hints, Server, Tools};

/// The fix rows' commands, in the manifest.
pub const INSTALL_PYTHON: &str = "python.install-python";
pub const INSTALL_UV: &str = "python.install-uv";
pub const SYNC: &str = "python.sync";
pub const INSTALL_SERVER: &str = "python.install-server";
pub const INSTALL_RUFF: &str = "python.install-ruff";
pub const INSTALL_DEBUGPY: &str = "python.install-debugpy";

/// `uv sync --locked --dry-run`: the venv is in step with `uv.lock`
/// when it would change nothing.
pub fn in_step(out: &Output) -> (Health, String) {
    let text = out.text();
    let changes: Vec<&str> = text.lines().map(str::trim).filter(|l| l.starts_with("+ ") || l.starts_with("- ") || l.starts_with("~ ")).collect();
    if text.contains("needs to be updated") {
        return (Health::Bad, "uv.lock isn't up to date with pyproject.toml".into());
    }
    if text.contains("Would create") && text.contains("environment") {
        return (Health::Warn, "there's no .venv yet".into());
    }
    if !out.ok() {
        return (Health::Warn, format!("uv sync --dry-run failed: {}", out.why()));
    }
    if changes.is_empty() {
        return (Health::Ok, ".venv is in step with uv.lock".into());
    }
    let shown: Vec<&str> = changes.iter().take(3).map(|c| c.trim_start_matches(['+', '-', '~', ' '])).collect();
    let more = if changes.len() > 3 { format!(" and {} more", changes.len() - 3) } else { String::new() };
    (Health::Warn, format!(".venv differs from uv.lock: {}{more}", shown.join(", ")))
}

/// `name`, looked for where uv and pipx put tools too.
fn program(name: &str, label: &str) -> Program {
    hints().into_iter().fold(Program::new(name, label), Program::hint)
}

fn fix(program: Program, label: &str, command: &str, cmd: Option<fenix_extension_api::Command>) -> Program {
    match cmd {
        Some(cmd) => program.fix(label, command, Fix::Task(cmd)),
        None => program,
    }
}

/// The checks for `project`, with what's on this machine.
pub fn programs(project: &Project, tools: &Tools, server: Server, ruff: Mode) -> Vec<Program> {
    let mut out = Vec::new();
    let python_wants = project.requires_python.as_deref().map(|r| r.split(',').filter(|c| !c.trim().starts_with("!=")).collect::<Vec<_>>().join(","));
    let with_uv = tools.uv.is_some() || project.uv;

    // The Python it gets: uv's pick when uv runs it (the venv's, or one
    // that fits requires-python), else the one on PATH.
    let python = if with_uv {
        let p = program("uv", "Python").version_args(["python", "find", "--show-version"]);
        fix(p, "Install a Python that fits", INSTALL_PYTHON, Some(tools.uv().args(["python", "install"])))
    } else {
        let name = if tools.python.as_deref().is_some_and(|p| !p.contains("python3")) { "python" } else { "python3" };
        python_hints().into_iter().fold(Program::new(name, "Python"), Program::hint)
    };
    out.push(match python_wants.as_deref() {
        Some(r) => python.wants(r),
        None => python.at_least("3.9"),
    });

    // uv: needed for a project it manages, else welcome.
    let uv = program("uv", "uv");
    let uv = if project.uv { uv } else { uv.optional() };
    out.push(fix(uv, "Install uv", INSTALL_UV, tools.install_uv()));

    if project.locked {
        let env = program("uv", "Environment").section(Section::Project).version_args(["sync", "--locked", "--dry-run", "--offline"]).judge(in_step);
        out.push(fix(env, "Sync .venv with uv.lock", SYNC, Some(tools.uv().arg("sync"))));
    }

    // The language server: the one found, else the one asked for.
    let s = tools.pick_server(server).map(|(s, _)| s).unwrap_or(server);
    let ls = program(s.program(), "Language server").version_args(Vec::<String>::new()).optional();
    out.push(fix(ls, &format!("Install {}", s.package()), INSTALL_SERVER, tools.install_tool(s.package())));

    // ruff lints and formats: the project's, when it asks for it. Its
    // server is stable from 0.5.3.
    if !(project.uv && project.has("ruff")) {
        let r = program("ruff", "ruff").optional();
        let r = if ruff == Mode::Server { r.at_least(ruff::SERVER_SINCE) } else { r };
        out.push(fix(r, "Install ruff", INSTALL_RUFF, tools.install_tool("ruff")));
    }

    // uv brings debugpy along for the debugger; else it's the Python's.
    if tools.uv.is_none() {
        let name = out[0].name.clone();
        let debugpy = program(&name, "debugpy").version_args(["-c", "import debugpy; print(debugpy.__version__)"]).optional();
        out.push(fix(debugpy, "Install debugpy", INSTALL_DEBUGPY, tools.python.as_ref().map(|_| tools.python().args(["-m", "pip", "install", "debugpy"]))));
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.name.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    #[test]
    fn uv_sync_says_whether_the_venv_is_in_step() {
        let ok = out(0, "", "Would use project environment at: .venv\nResolved 14 packages in 2ms\nChecked 12 packages in 0.37ms\nWould make no changes");
        assert_eq!(in_step(&ok), (Health::Ok, ".venv is in step with uv.lock".into()));
        let differs = out(0, "", "Would uninstall 1 package\nWould install 5 packages\n - idna==3.6\n + idna==2.10\n + iniconfig==2.3.1\n + rich==15.0.0\n + mdurl==0.1.2");
        assert_eq!(in_step(&differs), (Health::Warn, ".venv differs from uv.lock: idna==3.6, idna==2.10, iniconfig==2.3.1 and 2 more".into()));
        let stale = out(1, "", " + rich==15.0.0\nerror: The lockfile at `uv.lock` needs to be updated, but `--locked` was provided.\n\nhint: To update the lockfile, run `uv lock`.");
        assert_eq!(in_step(&stale).0, Health::Bad);
        let none = out(0, "", "Using CPython 3.13.1 interpreter at: /usr/bin/python3.13\nWould create project environment at: .venv\n + certifi==2026.10.23");
        assert_eq!(in_step(&none), (Health::Warn, "there's no .venv yet".into()));
        assert_eq!(in_step(&out(2, "", "error: No `project` table found")).1, "uv sync --dry-run failed: error: No `project` table found");
    }

    #[test]
    fn a_uv_project_checks_uvs_python_and_its_lock() {
        let project = Project { uv: true, locked: true, requires_python: Some(">=3.11,!=3.11.2".into()), deps: vec![crate::project::Dep { name: "ruff".into(), group: Some("dev".into()) }], ..Project::default() };
        let tools = Tools { uv: Some("/u/uv".into()), basedpyright: Some("/u/bp".into()), ..Tools::default() };
        let p = programs(&project, &tools, Server::Basedpyright, Mode::Server);
        assert_eq!(
            rows(&p),
            [
                ("Python".into(), "uv".into(), true, Some(INSTALL_PYTHON.into())),
                ("uv".into(), "uv".into(), true, None),
                ("Environment".into(), "uv".into(), true, Some(SYNC.into())),
                ("Language server".into(), "basedpyright-langserver".into(), false, Some(INSTALL_SERVER.into())),
            ],
            "uv is there, so nothing installs it; ruff is the project's, debugpy uv's"
        );
        // The Python uv finds, held to requires-python (its != left out).
        let python = &p[0];
        assert_eq!(python.version_args, ["python", "find", "--show-version"]);
        let c = python.evaluate(Some("/u/uv"), Some(&out(0, "3.10.14\n", "")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "3.10.14, but the project needs >=3.11 (/u/uv)"));
        assert_eq!(c.fix, Some(("Install a Python that fits".into(), INSTALL_PYTHON.into())));
        let c = python.evaluate(Some("/u/uv"), Some(&out(0, "3.13.1\n", "")));
        assert_eq!((c.health, c.fix), (Health::Ok, None));
        assert!(p[1].hints.contains(&"$HOME/.local/bin".to_string()));
        assert!(p[3].version_args.is_empty(), "a server is only found");
        match &p[2].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(crate::tests::line(cmd), "/u/uv sync"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn without_uv_its_python_and_debugpy_are_checked() {
        let project = Project { pyproject: true, ..Project::default() };
        let tools = Tools { python: Some("C:\\Python312\\python.exe".into()), pyright: Some("/p".into()), ..Tools::default() };
        let p = programs(&project, &tools, Server::Basedpyright, Mode::Server);
        assert_eq!(
            rows(&p),
            [
                ("Python".into(), "python".into(), true, None),
                ("uv".into(), "uv".into(), false, Some(INSTALL_UV.into())),
                ("Language server".into(), "pyright-langserver".into(), false, Some(INSTALL_SERVER.into())),
                ("ruff".into(), "ruff".into(), false, Some(INSTALL_RUFF.into())),
                ("debugpy".into(), "python".into(), false, Some(INSTALL_DEBUGPY.into())),
            ],
            "pyright's found, so it's the one checked"
        );
        let c = p[0].evaluate(Some("/usr/bin/python"), Some(&out(0, "Python 3.8.10", "")));
        assert_eq!(c.health, Health::Bad, "3.9 at least");
        let missing = p[4].evaluate(Some("/usr/bin/python"), Some(&out(1, "", "Traceback (most recent call last):\nModuleNotFoundError: No module named 'debugpy'")));
        assert_eq!((missing.health, missing.fix.map(|f| f.1)), (Health::Warn, Some(INSTALL_DEBUGPY.into())));
        assert_eq!(p[4].evaluate(Some("/usr/bin/python"), Some(&out(0, "1.8.11", ""))).health, Health::Ok);
        // Nothing to install with: no fix rows.
        assert_eq!(p[0].hints[0], crate::tools::INTERPRETER, "the Python you chose first");
        let old = p[3].evaluate(Some("/r/ruff"), Some(&out(0, "ruff 0.4.10", "")));
        assert_eq!(old.health, Health::Warn, "too old for its server");
        let save = programs(&project, &tools, Server::Basedpyright, Mode::Save);
        assert_eq!(save[3].evaluate(Some("/r/ruff"), Some(&out(0, "ruff 0.4.10", ""))).health, Health::Ok, "on save any ruff does");
        let bare = programs(&project, &Tools::default(), Server::Pyright, Mode::Server);
        assert!(bare.iter().all(|p| p.fix.is_none()), "{:?}", rows(&bare));
        assert_eq!(bare[2].name, "pyright-langserver", "the one asked for, when neither is found");
    }
}
