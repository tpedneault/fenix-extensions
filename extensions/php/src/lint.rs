//! PHPStan on a saved file (`--error-format=json`), with the project's
//! config, its problems shown with the server's. PHPStan exits 1 when it
//! found some; what isn't about a file (a config it can't read) is said
//! as the run's failure.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::lint::from_one;
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{Linter, Output};

/// `phpstan analyse` on the saved file.
pub fn phpstan(phpstan: &Command) -> Linter {
    let phpstan = phpstan.clone();
    Linter::new("phpstan", &["php"], move |file| phpstan.clone().args(["analyse", "--error-format=json", "--no-progress", "--no-interaction", "--memory-limit=1G"]).arg(file), parse)
}

fn same_file(path: &str, file: &str) -> bool {
    let path = path.replace('\\', "/");
    let file = file.replace('\\', "/");
    path == file || path.ends_with(&format!("/{file}"))
}

/// PHPStan's JSON: each message in `file` an error at its line, its
/// identifier (`argument.type`) the code, its tip after the message.
pub fn parse(out: &Output, file: &str) -> Result<Vec<Problem>, String> {
    let Some(report) = out.json_stream().into_iter().next() else {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    };
    let mut problems = Vec::new();
    for (path, found) in report["files"].as_object().into_iter().flatten() {
        if !same_file(path, file) {
            continue;
        }
        for m in found["messages"].as_array().into_iter().flatten() {
            let Some(message) = m["message"].as_str() else { continue };
            let (l, c) = from_one(m["line"].as_u64().unwrap_or(1) as u32, 1);
            let message = match m["tip"].as_str() {
                Some(tip) => format!("{message}\n{tip}"),
                None => message.to_string(),
            };
            let mut p = Problem::new(Severity::Error, (l, c), (l, c + 1), message);
            p.code = m["identifier"].as_str().map(str::to_string);
            problems.push(p);
        }
    }
    let general: Vec<&str> = report["errors"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if problems.is_empty() && !general.is_empty() {
        return Err(general.join("; "));
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    #[test]
    fn phpstan_runs_on_the_saved_file() {
        let l = phpstan(&Command::new("/p/vendor/bin/phpstan"));
        assert_eq!(line(&(l.command)("src/Orbit.php")), "/p/vendor/bin/phpstan analyse --error-format=json --no-progress --no-interaction --memory-limit=1G src/Orbit.php");
        assert!(l.takes("src/Orbit.php") && !l.takes("composer.json"));
    }

    #[test]
    fn its_messages_in_the_file_are_read_from_its_json() {
        let out = Output { code: Some(1), stdout: include_str!("samples/phpstan.json").into(), stderr: "Note: Using configuration file /home/u/orbit/phpstan.neon.\n".into() };
        let problems = parse(&out, "src/Orbit.php").unwrap();
        let rows: Vec<(u32, Option<&str>, &str)> = problems.iter().map(|p| (p.start.0, p.code.as_deref(), p.message.as_str())).collect();
        assert_eq!(
            rows,
            [
                (11, Some("argument.type"), "Parameter #1 $seconds of method Orbit\\Desk\\Orbit::wait() expects int, string given."),
                (17, Some("return.missing"), "Method Orbit\\Desk\\Orbit::period() should return float but return statement is missing.\nAdd a return statement, or declare the method's return type as void."),
            ]
        );
        assert!(problems.iter().all(|p| p.severity == Severity::Error));
        assert_eq!(parse(&out, "src/Other.php").unwrap(), [], "another file's left out");
        let clean = Output { code: Some(0), stdout: r#"{"totals":{"errors":0,"file_errors":0},"files":[],"errors":[]}"#.into(), stderr: String::new() };
        assert_eq!(parse(&clean, "src/Orbit.php").unwrap(), []);
        let config = Output { code: Some(1), stdout: r#"{"totals":{"errors":1,"file_errors":0},"files":[],"errors":["Invalid configuration: parameters.level is missing"]}"#.into(), stderr: String::new() };
        assert_eq!(parse(&config, "src/Orbit.php").unwrap_err(), "Invalid configuration: parameters.level is missing");
        let failed = Output { code: Some(255), stdout: String::new(), stderr: "PHP Fatal error:  Allowed memory size exhausted\n".into() };
        assert!(parse(&failed, "src/Orbit.php").unwrap_err().contains("Allowed memory size"));
    }
}
