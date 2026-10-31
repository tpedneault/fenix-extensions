//! SwiftLint on a saved Swift file: its rules' findings, shown with the
//! server's problems. It runs in the project, so the `.swiftlint.yml`
//! there (and those nested beside the file) is read; `--force-exclude`
//! keeps a file the config excludes unlinted, as a whole run would.
//!
//! `--reporter json` prints a list of violations; it exits 2 when one is
//! an error (`serious`), so its output, not its exit code, says whether
//! it worked.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::lint::{from_one, severity};
use fenix_lang_kit::serde_json::{self, Value};
use fenix_lang_kit::{Linter, Output};

/// SwiftLint for a saved file.
pub fn swiftlint(swiftlint: &Command) -> Linter {
    let swiftlint = swiftlint.clone();
    Linter::new("swiftlint", &["swift"], move |file| swiftlint.clone().args(["lint", "--reporter", "json", "--quiet", "--force-exclude", file]), parse_swiftlint)
}

/// What `swiftlint lint --reporter json` said: each violation, its rule
/// as its code, at its column (the line's start when it gives none).
pub fn parse_swiftlint(out: &Output, _file: &str) -> Result<Vec<Problem>, String> {
    let text = out.stdout.trim();
    if text.is_empty() {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    }
    let found: Vec<Value> = serde_json::from_str(text).map_err(|e| format!("swiftlint: {e}: {}", out.why()))?;
    Ok(found
        .iter()
        .filter_map(|v| {
            let line = v.get("line")?.as_u64()? as u32;
            let column = v.get("character").and_then(Value::as_u64).unwrap_or(1) as u32;
            let reason = v.get("reason")?.as_str()?;
            let sev = v.get("severity").and_then(Value::as_str).and_then(severity).unwrap_or(Severity::Warning);
            let (l, c) = from_one(line, column);
            let mut p = Problem::new(sev, (l, c), (l, c + 1), reason).source("swiftlint");
            p.code = v.get("rule_id").and_then(Value::as_str).map(str::to_string);
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
    fn swiftlints_violations_are_problems() {
        let said = include_str!("samples/swiftlint.json");
        let p = parse_swiftlint(&out(2, said, ""), "Sources/Orbit/Orbit.swift").unwrap();
        let rows: Vec<(Severity, (u32, u32), String)> = p.iter().map(|p| (p.severity, p.start, p.code.clone().unwrap())).collect();
        assert_eq!(rows, [(Severity::Warning, (11, 0), "trailing_whitespace".into()), (Severity::Error, (21, 12), "force_cast".into()), (Severity::Warning, (3, 0), "line_length".into())], "a violation without a column: the line's start");
        assert_eq!(p[1].message, "Force casts should be avoided");
        assert_eq!(parse_swiftlint(&out(0, "[]", ""), "a.swift").unwrap(), Vec::new());
        assert_eq!(parse_swiftlint(&out(0, "", ""), "a.swift").unwrap(), Vec::new());
        let failed = parse_swiftlint(&out(1, "", "Could not read configuration: file .swiftlint.yml: YAML parsing error"), "a.swift").unwrap_err();
        assert!(failed.contains("YAML parsing error"), "{failed}");
    }

    #[test]
    fn swiftlint_runs_on_the_file_saved() {
        let l = swiftlint(&Command::new("/t/swiftlint"));
        assert_eq!(line(&(l.command)("Sources/Orbit/Orbit.swift")), "/t/swiftlint lint --reporter json --quiet --force-exclude Sources/Orbit/Orbit.swift");
        assert!(l.takes("Sources/Orbit/Orbit.swift") && l.takes("Package.swift") && !l.takes(".swiftlint.yml"));
    }
}
