//! The project doctor's Go checks: the Go that builds the project, held
//! to `go.mod`'s `go` line (and to its `toolchain` line, which is only a
//! suggestion), go.mod and go.sum tidy, gopls, Delve, govulncheck, and
//! golangci-lint where the project has its config. Each that isn't right
//! has a fix row: `go install` for the tools, Homebrew or winget for Go,
//! `go mod tidy` for the modules.

use fenix_extension_api::{Command, Health, Section};
use fenix_lang_kit::{Fix, Output, Program};

use crate::project::Project;
use crate::tools::{hints, Tools, DELVE, GOLANGCI_LINT, GOPLS, GOVULNCHECK};

/// The fix rows' commands, in the manifest.
pub const INSTALL_GO: &str = "go.install-go";
pub const TIDY: &str = "go.tidy";
pub const INSTALL_GOPLS: &str = "go.install-gopls";
pub const INSTALL_DELVE: &str = "go.install-delve";
pub const INSTALL_GOVULNCHECK: &str = "go.install-govulncheck";
pub const INSTALL_GOLANGCI_LINT: &str = "go.install-golangci-lint";

/// Go 1.21 is the first to switch to the toolchain go.mod asks for.
pub const LEAST_GO: &str = "1.21";

/// `go mod tidy -diff` (Go 1.23): nothing printed when go.mod and go.sum
/// are tidy; else a diff of what tidying would change, and exit 1.
pub fn tidy(out: &Output) -> (Health, String) {
    let text = out.text();
    if text.contains("flag provided but not defined: -diff") {
        return (Health::Warn, "go mod tidy -diff needs Go 1.23 or later".into());
    }
    let files: Vec<&str> = text.lines().filter_map(|l| l.strip_prefix("--- current/")).map(str::trim).collect();
    if !files.is_empty() {
        let verb = if files.len() == 1 { "isn't" } else { "aren't" };
        return (Health::Warn, format!("{} {verb} tidy", files.join(" and ")));
    }
    if out.ok() {
        return (Health::Ok, "go.mod and go.sum are tidy".into());
    }
    (Health::Warn, format!("go mod tidy -diff failed: {}", out.why()))
}

/// `govulncheck -version`: its own version is the scanner's, not the
/// Go one it names first.
pub fn scanner(out: &Output) -> (Health, String) {
    let text = out.text();
    match text.lines().find_map(|l| l.trim().strip_prefix("Scanner: govulncheck@")) {
        Some(v) => (Health::Ok, v.trim().trim_start_matches('v').to_string()),
        None if out.ok() => (Health::Ok, "installed".into()),
        None => (Health::Warn, format!("asking its version failed: {}", out.why())),
    }
}

/// `name`, looked for where `go install` puts tools too.
fn program(name: &str, label: &str) -> Program {
    hints().into_iter().fold(Program::new(name, label), Program::hint)
}

fn fix(program: Program, label: &str, command: &str, cmd: Option<Command>) -> Program {
    match cmd {
        Some(cmd) => program.fix(label, command, Fix::Task(cmd)),
        None => program,
    }
}

/// The checks for `project`, with what's on this machine.
pub fn programs(project: &Project, tools: &Tools) -> Vec<Program> {
    let mut out = Vec::new();

    // The Go that builds it: in the project, `go version` is the
    // toolchain Go switches to for it.
    let go = program("go", "Go").version_args(["version"]);
    let go = match project.go() {
        Some(v) => go.at_least(v),
        None => go.at_least(LEAST_GO),
    };
    out.push(fix(go, "Install Go", INSTALL_GO, tools.install_go()));
    if let Some(toolchain) = project.toolchain() {
        out.push(program("go", "Toolchain").version_args(["version"]).at_least(toolchain).optional());
    }

    if project.module.is_some() {
        let modules = program("go", "Modules").section(Section::Project).version_args(["mod", "tidy", "-diff"]).judge(tidy);
        out.push(fix(modules, "Tidy go.mod and go.sum", TIDY, Some(tools.go().args(["mod", "tidy"]))));
    }

    let gopls = program("gopls", "gopls").version_args(["version"]).optional();
    out.push(fix(gopls, "Install gopls", INSTALL_GOPLS, tools.install(GOPLS)));
    let dlv = program("dlv", "Delve").version_args(["version"]).optional();
    out.push(fix(dlv, "Install Delve", INSTALL_DELVE, tools.install(DELVE)));
    let vulncheck = program("govulncheck", "govulncheck").version_args(["-version"]).judge(scanner).optional();
    out.push(fix(vulncheck, "Install govulncheck", INSTALL_GOVULNCHECK, tools.install(GOVULNCHECK)));

    // golangci-lint: checked when the project uses it, or it's there.
    if project.golangci || tools.golangci_lint.is_some() {
        let lint = program("golangci-lint", "golangci-lint").optional();
        out.push(fix(lint, "Install golangci-lint", INSTALL_GOLANGCI_LINT, tools.install(GOLANGCI_LINT)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::GoMod;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.name.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    fn project(go_mod: &str) -> Project {
        Project { module: Some(GoMod::parse(go_mod)), ..Project::default() }
    }

    #[test]
    fn tidy_says_whether_go_mod_needs_it() {
        assert_eq!(tidy(&out(0, "", "")), (Health::Ok, "go.mod and go.sum are tidy".into()));
        let diff = include_str!("samples/go-mod-tidy-diff.txt");
        assert_eq!(tidy(&out(1, diff, "")), (Health::Warn, "go.mod and go.sum aren't tidy".into()));
        assert_eq!(tidy(&out(2, "", "flag provided but not defined: -diff\nusage: go mod tidy [-e] [-v] [-x] [-go=version] [-compat=version]\n")).1, "go mod tidy -diff needs Go 1.23 or later");
        assert_eq!(tidy(&out(1, "", "go: example.com/orbit imports\n\tgithub.com/nowhere/x: cannot find module providing package github.com/nowhere/x")).1, "go mod tidy -diff failed: github.com/nowhere/x: cannot find module providing package github.com/nowhere/x");
    }

    #[test]
    fn govulncheck_says_the_scanners_version() {
        let said = out(0, "Go: go1.24.7\nScanner: govulncheck@v1.9.0\nDB: https://vuln.go.dev\nDB updated: 2026-10-28 18:04:51 +0000 UTC\n", "");
        assert_eq!(scanner(&said), (Health::Ok, "1.9.0".into()));
        assert_eq!(scanner(&out(2, "", "flag provided but not defined: -version")).0, Health::Warn);
    }

    #[test]
    fn go_is_held_to_go_mod_and_its_toolchain() {
        let tools = Tools { go: Some("/usr/local/go/bin/go".into()), gopls: Some("/h/go/bin/gopls".into()), ..Tools::default() };
        let p = programs(&project(include_str!("samples/go.mod")), &tools);
        assert_eq!(
            rows(&p),
            [
                ("Go".into(), "go".into(), true, None),
                ("Toolchain".into(), "go".into(), false, None),
                ("Modules".into(), "go".into(), true, Some(TIDY.into())),
                ("gopls".into(), "gopls".into(), false, Some(INSTALL_GOPLS.into())),
                ("Delve".into(), "dlv".into(), false, Some(INSTALL_DELVE.into())),
                ("govulncheck".into(), "govulncheck".into(), false, Some(INSTALL_GOVULNCHECK.into())),
            ],
            "Linux: no fix row installs Go; no golangci-lint config, none found"
        );
        let said = |v: &str| out(0, &format!("go version go{v} linux/amd64\n"), "");
        let c = p[0].evaluate(Some("/usr/local/go/bin/go"), Some(&said("1.21.13")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "1.21.13, but the project needs >=1.22 (/usr/local/go/bin/go)"));
        assert_eq!(p[0].evaluate(Some("/usr/local/go/bin/go"), Some(&said("1.22.0"))).health, Health::Ok);
        let c = p[1].evaluate(Some("/usr/local/go/bin/go"), Some(&said("1.23.4")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Warn, "1.23.4, but the project needs >=1.24.7 (/usr/local/go/bin/go)"), "a suggestion: a warning");
        assert_eq!(p[1].evaluate(Some("/usr/local/go/bin/go"), Some(&said("1.24.7"))).health, Health::Ok);
        let dlv = p[4].evaluate(Some("/h/go/bin/dlv"), Some(&out(0, "Delve Debugger\nVersion: 1.27.2\nBuild: $Id: 360e7b2181d3da54115aaee4f569954bca95cfe3 $\n", "")));
        assert_eq!(dlv.detail, "1.27.2 (/h/go/bin/dlv)");
        let gopls = p[3].evaluate(Some("/h/go/bin/gopls"), Some(&out(0, "golang.org/x/tools/gopls v0.24.0\n", "")));
        assert_eq!(gopls.detail, "0.24.0 (/h/go/bin/gopls)");
        match &p[2].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(crate::tests::line(cmd), "/usr/local/go/bin/go mod tidy"),
            other => panic!("{other:?}"),
        }
        assert!(p[3].hints.contains(&"$HOME/go/bin".to_string()));
    }

    #[test]
    fn without_go_mod_go_is_held_to_what_switches_toolchains() {
        let tools = Tools { os: "macos".into(), brew: Some("/opt/homebrew/bin/brew".into()), golangci_lint: Some("/h/golangci-lint".into()), ..Tools::default() };
        let p = programs(&Project::default(), &tools);
        assert_eq!(
            rows(&p),
            [
                ("Go".into(), "go".into(), true, Some(INSTALL_GO.into())),
                ("gopls".into(), "gopls".into(), false, None),
                ("Delve".into(), "dlv".into(), false, None),
                ("govulncheck".into(), "govulncheck".into(), false, None),
                ("golangci-lint".into(), "golangci-lint".into(), false, None),
            ],
            "no Go: nothing installs the tools"
        );
        let c = p[0].evaluate(Some("/usr/bin/go"), Some(&out(0, "go version go1.20.14 darwin/arm64", "")));
        assert_eq!(c.health, Health::Bad);
        let lint = p[4].evaluate(Some("/h/golangci-lint"), Some(&out(0, "golangci-lint has version 2.5.0 built with go1.25.1 from ff63786c on 2025-09-21T19:04:05Z\n", "")));
        assert_eq!(lint.detail, "2.5.0 (/h/golangci-lint)");
    }
}
