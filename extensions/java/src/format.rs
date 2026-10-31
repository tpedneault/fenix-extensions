//! google-java-format for Java and ktlint for Kotlin, as the formatters
//! Fenix runs on the buffer (`--stdin`); ktlint on save as Kotlin's
//! linter, too. Without google-java-format, JDT LS formats Java.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{Linter, Output};

/// google-java-format on the buffer: the tools folder's (native, or the
/// jar run by `java`), else the one on `PATH`.
pub fn google_java_format(installed: Option<&str>, on_path: Option<&str>, java: Option<&str>) -> Option<Command> {
    match (installed, on_path, java) {
        (Some(p), _, Some(java)) if p.ends_with(".jar") => Some(Command::new(java).args(["-jar", &format!("${{tools}}/google-java-format/{}", p.rsplit('/').next().unwrap_or(p)), "-"])),
        (Some(p), _, _) if !p.ends_with(".jar") => Some(Command::new(p).arg("-")),
        (_, Some(p), _) => Some(Command::new(p).arg("-")),
        _ => None,
    }
}

/// ktlint: the tools folder's (a jar, run by `java`), else the one on
/// `PATH`.
pub fn ktlint(installed: bool, on_path: Option<&str>, java: Option<&str>) -> Option<Command> {
    match (installed, java, on_path) {
        (true, Some(java), _) => Some(Command::new(java).args(["-jar", "${tools}/ktlint/ktlint"])),
        (_, _, Some(p)) => Some(Command::new(p)),
        _ => None,
    }
}

/// ktlint on the buffer. It ends badly while there's a problem it can't
/// fix, having written the text with what it could: Fenix takes it all
/// the same (`formatter-set-ex`, keep-output, 0.8.7), and the linter says
/// what's left.
pub fn ktlint_format(ktlint: &Command) -> Command {
    ktlint.clone().args(["--format", "--stdin", "--stdin-path=${file}", "--log-level=none"])
}

/// ktlint's problems in a saved file (`--reporter=json`): each a warning,
/// its rule its code.
pub fn parse_ktlint(out: &Output, _file: &str) -> Result<Vec<Problem>, String> {
    let text = out.stdout.trim();
    if text.is_empty() {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    }
    let files: Value = fenix_lang_kit::serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut problems = Vec::new();
    for f in files.as_array().into_iter().flatten() {
        for e in f.get("errors").and_then(Value::as_array).into_iter().flatten() {
            let line = e.get("line").and_then(Value::as_u64).unwrap_or(1) as u32;
            let col = e.get("column").and_then(Value::as_u64).unwrap_or(1) as u32;
            let start = fenix_lang_kit::lint::from_one(line, col);
            problems.push(Problem {
                start,
                end: (start.0, start.1 + 1),
                severity: Severity::Warning,
                message: e.get("message").and_then(Value::as_str).unwrap_or("").to_string(),
                source: Some("ktlint".into()),
                code: e.get("rule").and_then(Value::as_str).map(str::to_string),
                fixes: Vec::new(),
            });
        }
    }
    Ok(problems)
}

/// ktlint as the linter for `.kt` and `.kts`.
pub fn ktlint_linter(ktlint: Command) -> Linter {
    Linter::new("ktlint", &["kt", "kts"], move |file| ktlint.clone().args(["--reporter=json", "--relative", "--log-level=none", file]), parse_ktlint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    #[test]
    fn the_formatters_and_where_they_are() {
        assert_eq!(line(&google_java_format(Some("/h/tools/fenix.java/google-java-format/google-java-format"), None, None).unwrap()), "/h/tools/fenix.java/google-java-format/google-java-format -");
        assert_eq!(line(&google_java_format(Some("/h/t/google-java-format/google-java-format.jar"), None, Some("/tools/jre/bin/java")).unwrap()), "/tools/jre/bin/java -jar ${tools}/google-java-format/google-java-format.jar -");
        assert_eq!(line(&google_java_format(None, Some("/usr/bin/google-java-format"), None).unwrap()), "/usr/bin/google-java-format -");
        assert_eq!(google_java_format(None, None, Some("java")), None, "JDT LS formats");
        let k = ktlint(true, None, Some("/tools/jre/bin/java")).unwrap();
        assert_eq!(line(&ktlint_format(&k)), "/tools/jre/bin/java -jar ${tools}/ktlint/ktlint --format --stdin --stdin-path=${file} --log-level=none");
        assert_eq!(line(&ktlint(false, Some("/opt/homebrew/bin/ktlint"), None).unwrap()), "/opt/homebrew/bin/ktlint");
        assert_eq!(ktlint(false, None, Some("java")), None);
    }

    #[test]
    fn ktlints_problems() {
        let out = Output { code: Some(1), stdout: include_str!("samples/ktlint.json").into(), stderr: String::new() };
        let p = parse_ktlint(&out, "A.kt").unwrap();
        assert_eq!(p.len(), 7);
        assert_eq!((p[1].start, p[1].code.as_deref(), p[1].severity), ((0, 6), Some("standard:class-naming"), Severity::Warning));
        assert!(p[1].message.starts_with("Class or object name should start with an uppercase letter"));
        assert!(parse_ktlint(&Output { code: Some(0), ..Output::default() }, "A.kt").unwrap().is_empty());
        let l = ktlint_linter(Command::new("ktlint"));
        assert!(l.takes("core/src/main/kotlin/A.kt") && l.takes("build.gradle.kts") && !l.takes("A.java"));
        assert_eq!(line(&(l.command)("core/A.kt")), "ktlint --reporter=json --relative --log-level=none core/A.kt");
    }
}
