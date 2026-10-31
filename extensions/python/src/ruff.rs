//! ruff: its server beside basedpyright (`language-server-add`, 0.8.5),
//! for its problems as you type and its fixes; or, as `python.ruff` says,
//! `ruff check --output-format json` on save, its fixes offered too
//! (`diagnostics-set-ex`, 0.8.6). And the formatter Fenix runs. The
//! project's own ruff (`uv run ruff`) when it asks for it, else the one
//! installed.

use fenix_extension_api::{Command, Problem, Severity, TextEdit};
use fenix_lang_kit::lint::from_one;
use fenix_lang_kit::{Linter, Output};
use serde::Deserialize;

/// How ruff lints (`python.ruff`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// `ruff server` beside the language server.
    #[default]
    Server,
    /// `ruff check` on each file saved.
    Save,
    Off,
}

impl Mode {
    pub fn parse(text: &str) -> Mode {
        match text.trim() {
            "save" => Mode::Save,
            "off" => Mode::Off,
            _ => Mode::Server,
        }
    }
}

/// ruff's server, as it's added beside the language server.
pub const SERVER: &str = "ruff";

/// The first ruff whose server is stable.
pub const SERVER_SINCE: &str = "0.5.3";

/// `ruff server`: its problems, fixes and code actions over LSP.
pub fn server(ruff: &Command) -> Command {
    ruff.clone().arg("server")
}

use crate::project::Project;
use crate::tools::Tools;

/// ruff, without its subcommand: `None` when there's none to run.
pub fn ruff(project: &Project, tools: &Tools) -> Option<Command> {
    if project.uv && project.has("ruff") && tools.uv.is_some() {
        return Some(tools.uv().args(["run", "ruff"]));
    }
    tools.ruff.as_deref().map(Command::new)
}

/// What Fenix formats Python with: the buffer on stdin, `${file}` its
/// path, so the project's settings for that file are used.
pub fn formatter(ruff: &Command) -> Command {
    ruff.clone().args(["format", "--stdin-filename", "${file}", "-"])
}

/// `ruff check` on a saved file: its problems with the fixes ruff has for
/// them, never applied (whatever the project's `fix = true` says), and an
/// excluded file left alone.
pub fn linter(ruff: &Command) -> Linter {
    let ruff = ruff.clone();
    Linter::with_fixes("ruff", &["py", "pyi"], move |file| ruff.clone().args(["check", "--output-format", "json", "--no-fix", "--force-exclude", file]), parse_with_fixes)
}

#[derive(Deserialize)]
struct Place {
    row: u32,
    column: u32,
}

#[derive(Deserialize)]
struct Edit {
    content: String,
    location: Place,
    end_location: Place,
}

#[derive(Deserialize)]
struct Fix {
    #[serde(default)]
    applicability: String,
    #[serde(default)]
    message: Option<String>,
    edits: Vec<Edit>,
}

#[derive(Deserialize)]
struct Diagnostic {
    #[serde(default)]
    code: Option<String>,
    message: String,
    location: Place,
    end_location: Place,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    fix: Option<Fix>,
}

/// Rules that are mistakes Python itself would stop on, not style.
const ERRORS: &[&str] = &["F821", "F822", "F823", "E999", "invalid-syntax"];

/// `ruff check --output-format json`'s problems in `file`. It exits 1
/// when it found some. Newer ruff calls every one an error, so a rule's
/// is a warning unless it's a syntax error or an undefined name; a
/// severity it says that's milder is kept.
pub fn parse(out: &Output, file: &str) -> Result<Vec<Problem>, String> {
    Ok(parse_with_fixes(out, file)?.into_iter().collect())
}

/// [`parse`], each problem with ruff's fix for it: its message, and
/// "(unsafe)" after one ruff applies only when asked to.
pub fn parse_with_fixes(out: &Output, file: &str) -> Result<Vec<Problem>, String> {
    if out.stdout.trim().is_empty() {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    }
    let found: Vec<Diagnostic> = out.json()?;
    let file = file.replace('\\', "/");
    Ok(found
        .into_iter()
        .filter(|d| d.filename.is_empty() || d.filename.replace('\\', "/").ends_with(&file))
        .map(|d| {
            let code = d.code.as_deref();
            let severity = match d.severity.as_deref() {
                Some("warning") => Severity::Warning,
                Some("info") => Severity::Info,
                Some("hint") => Severity::Hint,
                _ if code.is_none_or(|c| ERRORS.contains(&c)) => Severity::Error,
                _ => Severity::Warning,
            };
            let mut p = Problem::new(severity, from_one(d.location.row, d.location.column), from_one(d.end_location.row, d.end_location.column), d.message);
            p.code = d.code.filter(|c| c != "invalid-syntax");
            let mut p = p;
            if let Some(fix) = d.fix.filter(|f| !f.edits.is_empty()) {
                let title = fix.message.unwrap_or_else(|| format!("Fix {}", p.code.as_deref().unwrap_or("it")));
                let title = if fix.applicability == "safe" { title } else { format!("{title} (unsafe)") };
                let edits = fix.edits.into_iter().map(|e| TextEdit::new(from_one(e.location.row, e.location.column), from_one(e.end_location.row, e.end_location.column), e.content)).collect();
                p = p.fix(&title, edits);
            }
            p
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Dep;
    use crate::tests::line;

    const CHECK: &str = include_str!("samples/ruff-check.json");

    #[test]
    fn ruffs_problems_are_read_from_its_json() {
        let out = Output { code: Some(1), stdout: CHECK.into(), stderr: String::new() };
        let problems = parse(&out, "src/orbit/app.py").unwrap();
        let rows: Vec<_> = problems.iter().map(|p| (p.start, p.end, p.severity, p.code.as_deref(), p.message.as_str())).collect();
        assert_eq!(
            rows,
            [
                ((0, 7), (0, 9), Severity::Warning, Some("F401"), "`os` imported but unused"),
                ((1, 4), (1, 18), Severity::Error, Some("F821"), "Undefined name `undefined_name`"),
                ((3, 6), (3, 7), Severity::Error, None, "Expected a parameter or the end of the parameter list"),
                ((4, 88), (4, 120), Severity::Info, Some("E501"), "Line too long (120 > 88)"),
            ],
            "another file's left out"
        );
        assert_eq!(parse(&Output { code: Some(0), ..Output::default() }, "a.py").unwrap(), []);
        assert_eq!(parse(&Output { code: Some(2), stderr: "error: unknown rule".into(), ..Output::default() }, "a.py").unwrap_err(), "error: unknown rule");
    }

    #[test]
    fn ruffs_fixes_come_with_its_problems() {
        let out = Output { code: Some(1), stdout: CHECK.into(), stderr: String::new() };
        let problems = parse_with_fixes(&out, "src/orbit/app.py").unwrap();
        let unused = &problems[0];
        assert_eq!(unused.fixes.len(), 1);
        assert_eq!(unused.fixes[0].title, "Remove unused import: `os`");
        let e = &unused.fixes[0].edits[0];
        assert_eq!((e.start, e.end, e.text.as_str()), ((0, 0), (1, 0), ""), "the import's whole line");
        assert!(problems[1].fixes.is_empty(), "an undefined name has none");
        let unsafe_one = r#"[{"code":"UP007","message":"Use `X | Y`","location":{"row":3,"column":9},"end_location":{"row":3,"column":22},"filename":"a.py","fix":{"applicability":"unsafe","message":null,"edits":[{"content":"int | str","location":{"row":3,"column":9},"end_location":{"row":3,"column":22}}]}}]"#;
        let p = parse_with_fixes(&Output { code: Some(1), stdout: unsafe_one.into(), stderr: String::new() }, "a.py").unwrap();
        assert_eq!(p[0].fixes[0].title, "Fix UP007 (unsafe)");
        assert_eq!(Mode::parse("save"), Mode::Save);
        assert_eq!(Mode::parse(""), Mode::Server);
        assert_eq!(line(&server(&Command::new("/r/ruff"))), "/r/ruff server");
    }

    #[test]
    fn the_projects_ruff_is_used_else_the_installed_one() {
        let tools = Tools { uv: Some("uv".into()), ruff: Some("/r/ruff".into()), ..Tools::default() };
        let mut project = Project { uv: true, ..Project::default() };
        assert_eq!(ruff(&project, &tools).map(|c| line(&c)).as_deref(), Some("/r/ruff"));
        project.deps.push(Dep { name: "ruff".into(), group: Some("dev".into()) });
        let r = ruff(&project, &tools).unwrap();
        assert_eq!(line(&formatter(&r)), "uv run ruff format --stdin-filename ${file} -");
        assert_eq!(line(&(linter(&r).command)("src/a.py")), "uv run ruff check --output-format json --no-fix --force-exclude src/a.py");
        assert!(linter(&r).takes("stubs.pyi"));
        assert_eq!(ruff(&Project::default(), &Tools::default()), None);
    }
}
