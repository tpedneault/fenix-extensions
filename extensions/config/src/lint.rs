//! hadolint on a saved Dockerfile: its rules' findings (and ShellCheck's,
//! for the shell in its `RUN`s), shown with the server's problems.
//! `--format json` prints a list of findings; `--no-fail` keeps its exit
//! code 0 when it found some, so an exit code that isn't is a failure.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::lint::{from_one, severity};
use fenix_lang_kit::serde_json::{self, Value};
use fenix_lang_kit::{Linter, Output};

/// The names Fenix reads as Dockerfiles, with or without a `.suffix`.
pub const DOCKERFILES: &[&str] = &["Dockerfile", "Containerfile"];

/// hadolint for a saved Dockerfile.
pub fn hadolint(hadolint: &Command) -> Linter {
    let hadolint = hadolint.clone();
    Linter::new("hadolint", &["dockerfile"], move |file| hadolint.clone().args(["--format", "json", "--no-fail", file]), parse_hadolint).named(DOCKERFILES)
}

/// What `hadolint --format json` said: each finding, its rule as its
/// code. `style` is a hint.
pub fn parse_hadolint(out: &Output, _file: &str) -> Result<Vec<Problem>, String> {
    let text = out.stdout.trim();
    if text.is_empty() {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    }
    let found: Vec<Value> = serde_json::from_str(text).map_err(|e| format!("hadolint: {e}: {}", out.why()))?;
    Ok(found
        .iter()
        .filter_map(|f| {
            let line = f.get("line")?.as_u64()? as u32;
            let column = f.get("column").and_then(Value::as_u64).unwrap_or(1) as u32;
            let message = f.get("message")?.as_str()?;
            let level = f.get("level").and_then(Value::as_str).unwrap_or("warning");
            let sev = if level == "style" { Severity::Hint } else { severity(level).unwrap_or(Severity::Warning) };
            let (l, c) = from_one(line, column);
            // The instruction's line, to the next one's start.
            let mut p = Problem::new(sev, (l, c), (l + 1, 0), message).source("hadolint");
            p.code = f.get("code").and_then(Value::as_str).map(str::to_string);
            Some(p)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    #[test]
    fn hadolints_findings_are_problems() {
        let said = r#"[{"code":"DL3008","column":1,"file":"deploy/Dockerfile","level":"warning","line":3,"message":"Pin versions in apt get install."},{"code":"SC2086","column":1,"file":"deploy/Dockerfile","level":"info","line":5,"message":"Double quote to prevent globbing and word splitting."},{"code":"DL3059","column":1,"file":"deploy/Dockerfile","level":"style","line":6,"message":"Multiple consecutive `RUN` instructions."},{"code":"DL1000","column":1,"file":"deploy/Dockerfile","level":"error","line":9,"message":"unexpected 'X'"}]"#;
        let p = parse_hadolint(&out(0, said, ""), "deploy/Dockerfile").unwrap();
        let rows: Vec<(Severity, u32, String)> = p.iter().map(|p| (p.severity, p.start.0, p.code.clone().unwrap())).collect();
        assert_eq!(rows, [(Severity::Warning, 2, "DL3008".into()), (Severity::Info, 4, "SC2086".into()), (Severity::Hint, 5, "DL3059".into()), (Severity::Error, 8, "DL1000".into())]);
        assert_eq!(p[0].message, "Pin versions in apt get install.");
        assert_eq!(parse_hadolint(&out(0, "[]", ""), "Dockerfile").unwrap(), Vec::new());
        assert_eq!(parse_hadolint(&out(0, "", ""), "Dockerfile").unwrap(), Vec::new());
        assert!(parse_hadolint(&out(1, "", "openFile: does not exist"), "Dockerfile").is_err());
    }

    #[test]
    fn hadolint_runs_on_the_dockerfile_saved() {
        let l = hadolint(&Command::new("/t/hadolint"));
        assert_eq!(line(&(l.command)("deploy/Dockerfile.prod")), "/t/hadolint --format json --no-fail deploy/Dockerfile.prod");
        assert!(l.takes("deploy/Dockerfile.prod") && l.takes("Containerfile") && l.takes("api.dockerfile") && !l.takes("compose.yaml"));
    }
}
