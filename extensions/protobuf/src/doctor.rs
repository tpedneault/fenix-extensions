//! The project doctor's checks: buf, which the pack's language server,
//! formatter, lint, breaking and tasks are, held to what its version can
//! do (a language server from 1.43, `buf lsp serve` from 1.59, a formatter
//! reading stdin from 1.74); protoc, for a project without buf; grpcurl,
//! for the gRPC client; protols, when it's the server chosen. Each that's
//! missing has a fix row installing the release for this machine. The
//! pack's own check, `buf breaking` against the default branch, is
//! [`breaking`].

use fenix_extension_api::{Check, Health, Section};
use fenix_lang_kit::{find_version, Fix, Output, Program, Version};

use crate::project::Project;
use crate::tools::{hints, Platform};

/// What buf's version gives the pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufCan {
    /// A language server at all (1.43), and as `buf lsp serve` (1.59).
    pub lsp: bool,
    pub lsp_serve: bool,
    /// `buf format --stdin-filepath` (1.74).
    pub stdin_format: bool,
}

impl BufCan {
    /// For a buf of `version`; none known, the newest's.
    pub fn of(version: Option<&Version>) -> BufCan {
        let at = |v: &str| version.is_none_or(|have| *have >= Version::parse(v).unwrap());
        BufCan { lsp: at("1.43.0"), lsp_serve: at("1.59.0"), stdin_format: at("1.74.0") }
    }
}

/// `name`, looked for where the pack installs it and the managers do.
fn program(name: &str, label: &str) -> Program {
    hints(name).into_iter().fold(Program::new(name, label), Program::hint)
}

/// Its fix row installs the release of that name, when there's one for
/// this machine (`protobuf.install.<name>`).
fn release_fix(program: Program, label: &str, name: &str, platform: &Platform) -> Program {
    if platform.installs().iter().any(|i| i.name == name) {
        program.fix(label, &format!("protobuf.install.{name}"), Fix::Install(name.into()))
    } else {
        program
    }
}

/// What `buf --version` said, and what that buf can do.
pub fn buf(out: &Output) -> (Health, String) {
    let Some(version) = find_version(&out.text()) else {
        return (Health::Warn, format!("its version couldn't be read: {}", out.why()));
    };
    let can = BufCan::of(Some(&version));
    if !can.lsp {
        return (Health::Warn, format!("{version}: older than 1.43, it has no language server; the pack's buf is {}", crate::tools::BUF));
    }
    let mut detail = version.to_string();
    if !can.lsp_serve {
        detail.push_str(", its language server buf beta lsp");
    }
    if !can.stdin_format {
        detail.push_str(", formatting through its language server (buf format reads stdin from 1.74)");
    }
    (Health::Ok, detail)
}

/// The checks for `project`; `protols` when it's the server asked for.
pub fn programs(project: &Project, protols: bool, platform: &Platform) -> Vec<Program> {
    let mut out = Vec::new();
    let mut buf_check = program("buf", "buf").judge(buf);
    if project.buf_dir.is_none() {
        // Without a buf.yaml, protoc builds as well.
        buf_check = buf_check.optional();
    }
    out.push(release_fix(buf_check, "Install buf", "buf", platform));
    let protoc = program("protoc", "protoc").optional();
    out.push(release_fix(protoc, "Install protoc", "protoc", platform));
    let grpcurl = program("grpcurl", "grpcurl (the gRPC client)").version_args(["-version"]).optional();
    out.push(release_fix(grpcurl, "Install grpcurl", "grpcurl", platform));
    if protols {
        // protols comes from cargo (`cargo install protols`) or Homebrew.
        out.push(program("protols", "protols").optional());
    }
    out
}

/// What `buf breaking` found last, for the doctor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Breaking {
    /// Not run: no buf, no workspace, no repository.
    NotRun,
    Running(String),
    /// Its violations against the branch, as a line.
    Found { branch: String, count: usize, detail: String },
    /// It couldn't compare: the branch isn't there, a dep can't be fetched.
    Failed(String),
}

/// The check for what `buf breaking` found.
pub fn breaking(b: &Breaking) -> Option<Check> {
    let (health, detail) = match b {
        Breaking::NotRun => return None,
        Breaking::Running(branch) => (Health::Info, format!("comparing with {branch}…")),
        Breaking::Found { count: 0, detail, .. } => (Health::Ok, detail.clone()),
        Breaking::Found { detail, .. } => (Health::Warn, detail.clone()),
        Breaking::Failed(why) => (Health::Info, format!("couldn't compare: {why}")),
    };
    let check = Check::new(Section::Project, "Breaking changes", health, detail);
    Some(match b {
        Breaking::Found { count, .. } if *count > 0 => check.fix("show them as a task", "protobuf.breaking"),
        Breaking::Failed(_) => check.fix("compare again", "protobuf.breaking"),
        _ => check,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    #[test]
    fn buf_is_needed_in_a_workspace_and_held_to_what_it_can_do() {
        let project = Project { buf_dir: Some(String::new()), ..Project::default() };
        let p = programs(&project, false, &Platform::new("linux", "x86_64"));
        assert_eq!(
            rows(&p),
            [
                ("buf".into(), true, Some("protobuf.install.buf".into())),
                ("protoc".into(), false, Some("protobuf.install.protoc".into())),
                ("grpcurl (the gRPC client)".into(), false, Some("protobuf.install.grpcurl".into())),
            ]
        );
        assert_eq!(p[0].hints[0], "/tools/buf");
        assert_eq!(p[2].version_args, ["-version"], "grpcurl's own flag");
        let c = p[0].evaluate(Some("/t/buf"), Some(&out(0, "1.75.0\n")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "1.75.0"));
        assert_eq!(buf(&out(0, "1.50.0")), (Health::Ok, "1.50.0, its language server buf beta lsp, formatting through its language server (buf format reads stdin from 1.74)".into()));
        assert_eq!(buf(&out(0, "1.40.1")).0, Health::Warn);
        assert_eq!(buf(&out(1, "")).0, Health::Warn);
        assert_eq!(p[0].evaluate(None, None).health, Health::Bad, "a workspace without buf");
        assert_eq!(p[2].evaluate(None, None).health, Health::Warn, "the client wants grpcurl, the rest doesn't");

        let alone = programs(&Project::default(), true, &Platform::new("windows", "aarch64"));
        assert_eq!(alone.iter().map(|p| (p.name.as_str(), p.required, p.fix.is_some())).collect::<Vec<_>>(), [("buf", false, true), ("protoc", false, false), ("grpcurl", false, true), ("protols", false, false)]);
    }

    #[test]
    fn what_each_buf_can_do() {
        let v = |s: &str| Version::parse(s).unwrap();
        assert_eq!(BufCan::of(None), BufCan { lsp: true, lsp_serve: true, stdin_format: true });
        assert_eq!(BufCan::of(Some(&v("1.58.0"))), BufCan { lsp: true, lsp_serve: false, stdin_format: false });
        assert!(BufCan::of(Some(&v("1.74.0"))).stdin_format);
        assert!(!BufCan::of(Some(&v("1.42.9"))).lsp);
    }

    #[test]
    fn breaking_changes_are_a_check_of_the_projects() {
        assert_eq!(breaking(&Breaking::NotRun), None);
        let found = breaking(&Breaking::Found { branch: "main".into(), count: 2, detail: "2 against main: FIELD_NO_DELETE".into() }).unwrap();
        assert_eq!((found.section, found.health, found.fix.clone()), (Section::Project, Health::Warn, Some(("show them as a task".into(), "protobuf.breaking".into()))));
        assert_eq!(breaking(&Breaking::Found { branch: "main".into(), count: 0, detail: "none against main".into() }).unwrap().health, Health::Ok);
        assert_eq!(breaking(&Breaking::Running("main".into())).unwrap().detail, "comparing with main…");
        assert_eq!(breaking(&Breaking::Failed("no branch main".into())).unwrap().detail, "couldn't compare: no branch main");
    }
}
