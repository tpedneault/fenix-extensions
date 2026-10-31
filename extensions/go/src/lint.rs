//! The linter run on save: golangci-lint when it's installed, else go
//! vet. Both work on packages, not files, so each runs on the saved
//! file's package and its problems in that file are kept.
//!
//! golangci-lint 2 writes JSON with `--output.json.path stdout`; 1.x with
//! `--out-format json`. go vet's `-json` goes to stderr, a package's
//! findings by analyzer; a package that doesn't compile is lines, as the
//! compiler prints them.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::lint::{from_one, severity};
use fenix_lang_kit::serde_json::{self, Value};
use fenix_lang_kit::{compiler_lines, Linter, Output};
use serde::Deserialize;

/// The package of `file` (in the project, `internal/store/store.go`), as
/// go's tools take it: `./internal/store`, or `.` at the top.
pub fn package_of(file: &str) -> String {
    let file = file.replace('\\', "/");
    match file.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => format!("./{dir}"),
        _ => ".".into(),
    }
}

fn same_file(path: &str, file: &str) -> bool {
    let path = path.replace('\\', "/");
    let path = path.trim_start_matches("./");
    let file = file.replace('\\', "/");
    path == file || path.ends_with(&format!("/{file}"))
}

/// Lines as Go's compiler prints them, `vet: ` before some.
fn compiled(text: &str, file: &str) -> Vec<Problem> {
    let lines: String = text.lines().map(|l| l.strip_prefix("vet: ").unwrap_or(l)).collect::<Vec<_>>().join("\n");
    compiler_lines(&lines, file, Severity::Error)
}

/// `go vet -json` on a saved file's package.
pub fn vet(go: &Command) -> Linter {
    let go = go.clone();
    Linter::new("go vet", &["go"], move |file| go.clone().args(["vet".to_string(), "-json".to_string(), package_of(file)]), parse_vet)
}

/// What `go vet -json` said, about `file`: each analyzer's findings as
/// warnings, with the analyzer as its code; a package that doesn't
/// compile, the compiler's errors.
pub fn parse_vet(out: &Output, file: &str) -> Result<Vec<Problem>, String> {
    // `# package` lines, then a JSON object for each package.
    let json: String = out.stderr.lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>().join("\n");
    let objects: Vec<Value> = serde_json::Deserializer::from_str(&json).into_iter::<Value>().map_while(Result::ok).collect();
    if objects.is_empty() {
        let errors = compiled(&out.stderr, file);
        return if !errors.is_empty() || out.ok() { Ok(errors) } else { Err(out.why()) };
    }
    let mut problems = Vec::new();
    for analyzers in objects.iter().filter_map(Value::as_object).flat_map(|o| o.values()).filter_map(Value::as_object) {
        for (analyzer, found) in analyzers {
            for d in found.as_array().into_iter().flatten() {
                let (Some(posn), Some(message)) = (d.get("posn").and_then(Value::as_str), d.get("message").and_then(Value::as_str)) else { continue };
                // path:line:col, a drive's colon in the path.
                let mut parts = posn.rsplitn(3, ':');
                let (col, line, path) = (parts.next(), parts.next(), parts.next());
                let (Some(path), Some(Ok(line)), Some(Ok(col))) = (path, line.map(str::parse::<u32>), col.map(str::parse::<u32>)) else { continue };
                if !same_file(path, file) {
                    continue;
                }
                let (l, c) = from_one(line, col);
                let mut p = Problem::new(Severity::Warning, (l, c), (l, c + 1), message);
                p.code = Some(analyzer.clone());
                problems.push(p);
            }
        }
    }
    Ok(problems)
}

/// golangci-lint on a saved file's package; `v1` for its 1.x flags.
pub fn golangci(lint: &Command, v1: bool) -> Linter {
    let lint = lint.clone();
    let flags: &[&str] = if v1 { &["run", "--out-format", "json", "--print-issued-lines=false"] } else { &["run", "--output.json.path", "stdout", "--show-stats=false"] };
    let flags: Vec<String> = flags.iter().map(|f| f.to_string()).collect();
    Linter::new("golangci-lint", &["go"], move |file| lint.clone().args(flags.clone()).arg(package_of(file)), parse_golangci)
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Report {
    #[serde(default)]
    issues: Option<Vec<Issue>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Issue {
    from_linter: String,
    text: String,
    #[serde(default)]
    severity: String,
    pos: Pos,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Pos {
    filename: String,
    line: u32,
    #[serde(default)]
    column: u32,
}

/// golangci-lint's JSON: its issues in `file`, warnings unless it says a
/// severity, its linter as the code. It exits 1 when it found some. A
/// package that doesn't compile is one `typecheck` issue at the top of
/// a file, the compiler's lines in its text: those are the errors.
pub fn parse_golangci(out: &Output, file: &str) -> Result<Vec<Problem>, String> {
    let Some(first) = out.json_stream().into_iter().next() else {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    };
    let report: Report = serde_json::from_value(first).map_err(|e| format!("its output isn't what was expected: {e}"))?;
    let mut problems = Vec::new();
    for issue in report.issues.unwrap_or_default() {
        if issue.from_linter == "typecheck" {
            let mut errors = compiled(&issue.text, file);
            if errors.is_empty() && same_file(&issue.pos.filename, file) {
                let (l, c) = from_one(issue.pos.line, issue.pos.column);
                errors.push(Problem::new(Severity::Error, (l, c), (l, c + 1), issue.text.trim_start_matches(':').trim()));
            }
            problems.extend(errors);
            continue;
        }
        if !same_file(&issue.pos.filename, file) {
            continue;
        }
        let (l, c) = from_one(issue.pos.line, issue.pos.column);
        let mut p = Problem::new(severity(&issue.severity).unwrap_or(Severity::Warning), (l, c), (l, c + 1), issue.text);
        p.code = Some(issue.from_linter);
        problems.push(p);
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    type Row<'a> = ((u32, u32), Severity, Option<&'a str>, &'a str);

    fn rows(problems: &[Problem]) -> Vec<Row<'_>> {
        problems.iter().map(|p| (p.start, p.severity, p.code.as_deref(), p.message.as_str())).collect()
    }

    #[test]
    fn go_tools_take_the_files_package() {
        assert_eq!(package_of("internal/store/store.go"), "./internal/store");
        assert_eq!(package_of("main.go"), ".");
        assert_eq!(package_of("cmd\\orbit\\main.go"), "./cmd/orbit");
        let v = vet(&Command::new("/g/go"));
        assert_eq!(line(&(v.command)("internal/store/store.go")), "/g/go vet -json ./internal/store");
        assert!(v.takes("main.go") && !v.takes("go.mod"));
        assert_eq!(line(&(golangci(&Command::new("golangci-lint"), false).command)("main.go")), "golangci-lint run --output.json.path stdout --show-stats=false .");
        assert_eq!(line(&(golangci(&Command::new("golangci-lint"), true).command)("a/b.go")), "golangci-lint run --out-format json --print-issued-lines=false ./a");
    }

    #[test]
    fn vets_findings_are_read_from_its_json() {
        let out = Output { code: Some(0), stdout: String::new(), stderr: include_str!("samples/go-vet.json").into() };
        let problems = parse_vet(&out, "internal/store/store.go").unwrap();
        assert_eq!(
            rows(&problems),
            [
                ((13, 8), Severity::Warning, Some("copylocks"), "Get passes lock by value: example.com/orbit/internal/store.Store contains sync.Mutex"),
                ((25, 11), Severity::Warning, Some("unusedresult"), "result of fmt.Errorf call not used"),
            ]
        );
        assert_eq!(parse_vet(&out, "internal/store/other.go").unwrap(), [], "another file's left out");
        let broken = Output { code: Some(1), stdout: String::new(), stderr: include_str!("samples/go-vet-broken.txt").into() };
        assert_eq!(rows(&parse_vet(&broken, "internal/broken/broken.go").unwrap()), [((3, 8), Severity::Error, None, "undefined: missing")]);
        assert_eq!(parse_vet(&Output { code: Some(0), ..Output::default() }, "a.go").unwrap(), []);
        let failed = Output { code: Some(1), stdout: String::new(), stderr: "go: go.mod file not found in current directory or any parent directory\n".into() };
        assert!(parse_vet(&failed, "a.go").unwrap_err().starts_with("go: go.mod file not found"));
    }

    #[test]
    fn golangci_lints_issues_are_read_from_its_json() {
        let out = Output { code: Some(1), stdout: include_str!("samples/golangci-lint.json").into(), stderr: String::new() };
        let problems = parse_golangci(&out, "internal/store/store.go").unwrap();
        assert_eq!(
            rows(&problems),
            [
                ((24, 8), Severity::Warning, Some("errcheck"), "Error return value of `f.Write` is not checked"),
                ((25, 11), Severity::Warning, Some("errcheck"), "Error return value of `fmt.Errorf` is not checked"),
                ((13, 8), Severity::Warning, Some("govet"), "copylocks: Get passes lock by value: example.com/orbit/internal/store.Store contains sync.Mutex"),
            ]
        );
        let broken = Output { code: Some(1), stdout: include_str!("samples/golangci-lint-broken.json").into(), stderr: String::new() };
        assert_eq!(rows(&parse_golangci(&broken, "internal/broken/broken.go").unwrap()), [((3, 8), Severity::Error, None, "undefined: missing")], "the compiler's place, not the top of the file");
        let clean = Output { code: Some(0), stdout: "{\"Issues\":[],\"Report\":{}}\n".into(), stderr: String::new() };
        assert_eq!(parse_golangci(&clean, "a.go").unwrap(), []);
        let v1 = Output { code: Some(1), stdout: "{\"Issues\":[{\"FromLinter\":\"revive\",\"Text\":\"exported function Run should have comment\",\"Severity\":\"error\",\"Pos\":{\"Filename\":\"a.go\",\"Line\":3,\"Column\":1}}],\"Report\":{}}\n".into(), stderr: String::new() };
        assert_eq!(parse_golangci(&v1, "a.go").unwrap()[0].severity, Severity::Error, "a severity it says is kept");
        let failed = Output { code: Some(3), stdout: String::new(), stderr: "Error: unknown flag: --out-format\n".into() };
        assert_eq!(parse_golangci(&failed, "a.go").unwrap_err(), "Error: unknown flag: --out-format");
    }
}
