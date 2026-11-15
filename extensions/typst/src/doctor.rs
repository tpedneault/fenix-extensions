//! The project doctor's Typst checks: the compiler, which the tasks and the
//! compile and watch commands run; tinymist, the language server; and
//! typstyle, the formatter. Each has a fix row that installs the release the
//! pack knows for this machine, checked against its SHA-256.

use fenix_extension_api::Health;
use fenix_lang_kit::{Fix, Output, Program};

use crate::project::Project;
use crate::tools::{hints, Platform};

/// The oldest Typst whose `compile --diagnostic-format short --root` the
/// pack runs.
pub const OLDEST: &str = "0.11";

/// `name`, looked for where the pack installs it and where users put it.
fn program(name: &str, label: &str) -> Program {
    hints(name).into_iter().fold(Program::new(name, label), Program::hint)
}

/// What `tinymist --version` says: the release prints its build's details
/// and the Typst it bundles (`Typst Version: 0.15.1`), and no version of
/// its own (checked on 0.15.12).
fn judge_tinymist(out: &Output) -> (Health, String) {
    let typst = out.text().lines().find_map(|l| l.trim().strip_prefix("Typst Version:").map(|v| v.trim().to_string()));
    match typst {
        Some(v) if out.ok() => (Health::Ok, format!("bundles Typst {v}")),
        _ if out.ok() => (Health::Warn, "its version couldn't be read".to_string()),
        _ => (Health::Warn, format!("asking its version failed: {}", out.why())),
    }
}

/// The checks. Typst is needed in a project with `.typ` files or a
/// `typst.toml` and only noticed in any other; the server and the formatter
/// are optional: without them Fenix still colours the file.
pub fn programs(project: &Project, platform: &Platform) -> Vec<Program> {
    let typst = program("typst", "Typst").at_least(OLDEST);
    let typst = if project.typst { typst } else { typst.optional() };
    let tinymist = program("tinymist", "tinymist").judge(judge_tinymist).optional();
    let typstyle = program("typstyle", "typstyle").optional();
    let installs = platform.installs();
    let has = |name: &str| installs.iter().any(|i| i.name == name);
    let mut out = Vec::new();
    for (p, label) in [(typst, "Install Typst"), (tinymist, "Install tinymist"), (typstyle, "Install typstyle")] {
        let name = p.name.clone();
        out.push(if has(&name) { p.fix(label, &format!("typst.install.{name}"), Fix::Install(name)) } else { p });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    fn project() -> Project {
        Project { main: Some("main.typ".into()), typst: true }
    }

    #[test]
    fn typst_is_asked_its_version_and_needed_in_a_typst_project() {
        let linux = Platform::new("linux", "x86_64");
        let p = programs(&project(), &linux);
        assert_eq!(p.iter().map(|p| (p.label.as_str(), p.required)).collect::<Vec<_>>(), [("Typst", true), ("tinymist", false), ("typstyle", false)]);
        assert_eq!(p[0].hints[0], "/tools/typst");
        let c = p[0].evaluate(Some("/usr/bin/typst"), Some(&said(include_str!("samples/typst-version.txt"))));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "0.15.1 (/usr/bin/typst)"));
        let old = p[0].evaluate(Some("/usr/bin/typst"), Some(&said("typst 0.9.0 (abc)\n")));
        assert_eq!(old.health, Health::Bad, "{}", old.detail);
        let missing = p[0].evaluate(None, None);
        assert_eq!((missing.health, missing.fix.map(|f| f.1)), (Health::Bad, Some("typst.install.typst".into())));

        let other = programs(&Project::default(), &linux);
        assert_eq!(other[0].evaluate(None, None).health, Health::Warn, "a folder that isn't Typst's isn't failed for it");
    }

    #[test]
    fn tinymists_check_reads_the_typst_it_bundles_for_it_prints_no_version_of_its_own() {
        let p = programs(&project(), &Platform::new("linux", "x86_64"));
        let c = p[1].evaluate(Some("/tools/tinymist/tinymist"), Some(&said(include_str!("samples/tinymist-version.txt"))));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "bundles Typst 0.15.1"));
        let odd = p[1].evaluate(Some("/x/tinymist"), Some(&said("tinymist\n")));
        assert_eq!(odd.health, Health::Warn);
        let failed = p[1].evaluate(Some("/x/tinymist"), Some(&Output { code: Some(1), stdout: String::new(), stderr: "boom".into() }));
        assert!(failed.detail.contains("boom"), "{}", failed.detail);
        let missing = p[1].evaluate(None, None);
        assert_eq!((missing.health, missing.fix.map(|f| f.1)), (Health::Warn, Some("typst.install.tinymist".into())), "Fenix works without it");
    }

    #[test]
    fn typstyle_is_read_from_its_version_block() {
        let p = programs(&project(), &Platform::new("windows", "x86_64"));
        let c = p[2].evaluate(Some("C:/t/typstyle.exe"), Some(&said(include_str!("samples/typstyle-version.txt"))));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "0.15.2 (C:/t/typstyle.exe)"));
        assert_eq!(p[2].fix.as_ref().map(|f| f.1.as_str()), Some("typst.install.typstyle"));
    }

    #[test]
    fn no_release_for_the_machine_means_no_fix_row() {
        let p = programs(&project(), &Platform::new("linux", "riscv64"));
        assert!(p.iter().all(|p| p.fix.is_none()));
    }
}
