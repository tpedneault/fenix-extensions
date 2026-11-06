//! `buf lint` on save, over the whole workspace: a rule's violation in any
//! file is a warning there, a file that doesn't compile an error, shown
//! with the language server's problems; a file it no longer speaks of is
//! made clean. It prints one JSON object a line (`--error-format json`):
//! the file (in the workspace's folder), where (lines and columns from 1,
//! the end's column past the last character), the rule and the message.
//! `buf breaking` prints the same, its rules the changes it found.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::{Files, Linter, Output};
use serde::Deserialize;

/// A line of buf's JSON.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Violation {
    pub path: String,
    #[serde(default)]
    pub start_line: u32,
    #[serde(default)]
    pub start_column: u32,
    #[serde(default)]
    pub end_line: u32,
    #[serde(default)]
    pub end_column: u32,
    /// The rule: `FIELD_LOWER_SNAKE_CASE`, `FIELD_NO_DELETE`, or
    /// `COMPILE` for a file that doesn't.
    #[serde(rename = "type", default)]
    pub rule: String,
    pub message: String,
}

impl Violation {
    /// As a problem, its places from 0.
    pub fn problem(&self, source: &str) -> Problem {
        let start = (self.start_line.saturating_sub(1), self.start_column.saturating_sub(1));
        let end = if self.end_line == 0 { (start.0, start.1 + 1) } else { (self.end_line.saturating_sub(1), self.end_column.saturating_sub(1)) };
        let severity = if self.rule == "COMPILE" { Severity::Error } else { Severity::Warning };
        let p = Problem::new(severity, start, end, self.message.clone()).source(source);
        if self.rule.is_empty() || self.rule == "COMPILE" {
            p
        } else {
            p.code(&self.rule)
        }
    }
}

/// What buf printed: its violations, those that read. A failure that
/// printed none (no buf.yaml, a module it can't fetch) is an error of
/// buf's, in its words.
pub fn violations(out: &Output) -> Result<Vec<Violation>, String> {
    let found: Vec<Violation> = out.stdout.lines().chain(out.stderr.lines()).filter_map(|l| serde_json::from_str(l.trim()).ok()).collect();
    if found.is_empty() && !out.ok() {
        return Err(out.why());
    }
    Ok(found)
}

/// `buf lint`'s problems, by file below the project (`dir`, the
/// workspace's folder there, before each).
pub fn parse(out: &Output, dir: &str) -> Result<Files, String> {
    let mut files: Files = Vec::new();
    for v in violations(out)? {
        let path = if dir.is_empty() { v.path.clone() } else { format!("{dir}/{}", v.path.trim_start_matches("./")) };
        let problem = v.problem("buf lint");
        match files.iter_mut().find(|(f, _)| *f == path) {
            Some((_, ps)) => ps.push(problem),
            None => files.push((path, vec![problem])),
        }
    }
    Ok(files)
}

/// `buf lint` on the workspace, run in its folder (`cwd`), `dir` that
/// folder below the project.
pub fn buf_lint(buf: &Command, cwd: &str, dir: &str) -> Linter {
    let (buf, cwd, dir) = (buf.clone(), cwd.to_string(), dir.to_string());
    Linter::across("buf lint", &["proto"], move |_| buf.clone().args(["lint", "--error-format", "json"]).cwd(&cwd), move |out, _| parse(out, &dir))
}

/// What the doctor says of `buf breaking`'s violations against `branch`.
pub fn breaking_detail(found: &[Violation], branch: &str) -> String {
    match found {
        [] => format!("none against {branch}"),
        [one] => format!("1 against {branch}: {} ({}:{})", one.message, one.path, one.start_line),
        many => {
            let mut rules: Vec<&str> = many.iter().map(|v| v.rule.as_str()).collect();
            rules.sort_unstable();
            rules.dedup();
            format!("{} against {branch}: {}", many.len(), rules.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    #[test]
    fn buf_lints_json_is_each_files_problems() {
        let files = parse(&out(100, include_str!("samples/buf-lint.json")), "").unwrap();
        assert_eq!(files.iter().map(|(f, ps)| (f.as_str(), ps.len())).collect::<Vec<_>>(), [("proto/orbit/v1/ground.proto", 5), ("proto/orbit/v1/orbit.proto", 1)]);
        let p = &files[0].1[0];
        assert_eq!((p.start, p.end, p.severity), ((4, 8), (4, 22), Severity::Warning), "\"ground_station\", from 0, its end past it");
        assert_eq!((p.code.as_deref(), p.source.as_deref()), (Some("MESSAGE_PASCAL_CASE"), Some("buf lint")));
        assert_eq!(p.message, "Message name \"ground_station\" should be PascalCase, such as \"GroundStation\".");
        assert_eq!(files[1].1[0].code.as_deref(), Some("RPC_RESPONSE_STANDARD_NAME"));

        // A file that doesn't compile: an error, no rule.
        let broken = parse(&out(100, include_str!("samples/buf-lint-compile.json")), "api").unwrap();
        assert_eq!(broken[0].0, "api/proto/orbit/v1/broken.proto", "below the workspace's folder");
        assert_eq!((broken[0].1[0].severity, broken[0].1[0].code.clone()), (Severity::Error, None));

        assert_eq!(parse(&out(0, ""), "").unwrap(), Vec::new(), "clean");
        let failed = Output { code: Some(1), stdout: String::new(), stderr: "Failure: could not find a buf.yaml".into() };
        assert_eq!(parse(&failed, ""), Err("Failure: could not find a buf.yaml".into()));
    }

    #[test]
    fn buf_lint_runs_in_the_workspace_on_any_proto_saved() {
        let l = buf_lint(&Command::new("/t/buf"), "/project/api", "api");
        let cmd = (l.command)("api/orbit/v1/orbit.proto");
        assert_eq!(crate::tests::line(&cmd), "/t/buf lint --error-format json");
        assert_eq!(cmd.cwd.as_deref(), Some("/project/api"));
        assert!(l.across && l.takes("api/orbit/v1/orbit.proto") && !l.takes("buf.yaml"));
    }

    #[test]
    fn breaking_changes_are_said_in_a_line() {
        let found = violations(&out(100, include_str!("samples/buf-breaking.json"))).unwrap();
        assert_eq!(found.iter().map(|v| v.rule.as_str()).collect::<Vec<_>>(), ["FIELD_NO_DELETE", "FIELD_SAME_TYPE"]);
        assert_eq!(breaking_detail(&found, "main"), "2 against main: FIELD_NO_DELETE, FIELD_SAME_TYPE");
        assert_eq!(breaking_detail(&found[1..], "main"), "1 against main: Field \"3\" with name \"period_minutes\" on message \"Satellite\" changed type from \"double\" to \"float\". (proto/orbit/v1/orbit.proto:26)");
        assert_eq!(breaking_detail(&[], "trunk"), "none against trunk");
        assert_eq!(found[0].problem("buf breaking").end, (34, 1), "a message's whole span");
    }
}
