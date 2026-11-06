//! hlint on a saved Haskell file: its hints shown with the server's
//! problems, each with hlint's own rewrite as its fix (`SPC c a`). It
//! reads `--json`: a hint's span is the code it found (`from`), from 1
//! and its end one past, and `to` is what it would have instead, so the
//! fix puts `to` where the span was. A rewrite over several lines isn't
//! offered: hlint prints it again as GHC would, not as it was laid out.
//! The project's `.hlint.yaml` is hlint's to read, in `/project`.

use fenix_extension_api::{Command, Problem, Severity, TextEdit};
use fenix_lang_kit::lint::from_one;
use fenix_lang_kit::{Linter, Output};
use serde::Deserialize;

/// Haskell's files, as the language file has them.
pub const EXTENSIONS: &[&str] = &["hs", "hs-boot"];

/// hlint for a saved file.
pub fn linter(hlint: &Command) -> Linter {
    let hlint = hlint.clone();
    Linter::with_fixes("hlint", EXTENSIONS, move |file| hlint.clone().args(["--json", "--no-exit-code", file]), parse)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Idea {
    severity: String,
    hint: String,
    file: String,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    note: Vec<String>,
}

/// An error, a warning, or a suggestion (a note); `Ignore` isn't shown.
fn severity(word: &str) -> Option<Severity> {
    match word {
        "Error" => Some(Severity::Error),
        "Warning" => Some(Severity::Warning),
        "Suggestion" => Some(Severity::Info),
        _ => None,
    }
}

/// What `hlint --json` said of `file`: its hints, each with its rewrite
/// as a fix. Nothing printed and a failure is hlint's own error.
pub fn parse(out: &Output, file: &str) -> Result<Vec<Problem>, String> {
    let Some(at) = out.stdout.find('[') else {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    };
    let ideas: Vec<Idea> = fenix_lang_kit::serde_json::from_str(&out.stdout[at..]).map_err(|e| format!("hlint: {e}: {}", out.why()))?;
    let file = file.replace('\\', "/");
    let file = file.trim_start_matches("./");
    Ok(ideas
        .into_iter()
        .filter(|i| {
            let f = i.file.replace('\\', "/");
            let f = f.trim_start_matches("./");
            f == file || f.ends_with(&format!("/{file}")) || file.ends_with(&format!("/{f}"))
        })
        .filter_map(|i| {
            let severity = severity(&i.severity)?;
            let start = from_one(i.start_line, i.start_column);
            let end = from_one(i.end_line, i.end_column);
            let to = i.to.filter(|to| !to.contains('\n'));
            let mut message = match &to {
                Some(to) if !to.is_empty() => format!("{}: {to}", i.hint),
                Some(_) => format!("{}: remove it", i.hint),
                None => i.hint.clone(),
            };
            if !i.note.is_empty() {
                message = format!("{message} ({})", i.note.join("; "));
            }
            let mut p = Problem::new(severity, start, end, message).source("hlint");
            p.code = Some(i.hint.clone());
            Some(match to {
                Some(to) if severity != Severity::Error => p.fix(&i.hint, vec![TextEdit::new(start, end, to)]),
                _ => p,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    const IDEAS: &str = include_str!("samples/hlint.json");

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    /// `source` with each problem's first fix made, last first.
    fn fixed(source: &str, problems: &[Problem]) -> String {
        let mut lines: Vec<String> = source.lines().map(str::to_string).collect();
        let mut edits: Vec<&TextEdit> = problems.iter().filter_map(|p| p.fixes.first()).flat_map(|f| f.edits.iter()).collect();
        edits.sort_by_key(|e| std::cmp::Reverse(e.start));
        for e in edits {
            assert_eq!(e.start.0, e.end.0, "on one line");
            let l = &mut lines[e.start.0 as usize];
            let (a, b) = (e.start.1 as usize, e.end.1 as usize);
            l.replace_range(a..b, &e.text);
        }
        lines.join("\n") + "\n"
    }

    #[test]
    fn hlints_hints_are_problems_with_its_rewrites() {
        let p = parse(&out(0, IDEAS, ""), "src/Hints.hs").unwrap();
        let rows: Vec<_> = p.iter().map(|p| (p.severity, p.start, p.end, p.code.as_deref().unwrap(), p.fixes.len())).collect();
        assert_eq!(
            rows,
            [
                (Severity::Info, (3, 13), (3, 33), "Redundant bracket", 1),
                (Severity::Warning, (6, 10), (6, 27), "Use print", 1),
                (Severity::Info, (9, 16), (9, 27), "Avoid lambda using `infix`", 1),
                (Severity::Warning, (9, 0), (9, 31), "Eta reduce", 1),
            ],
            "the hint it ignores and the other file's left out"
        );
        assert_eq!(p[1].message, "Use print: print s");
        assert_eq!(p[3].message, "Eta reduce: twice = map (\\x -> x * 2) (increases laziness)");
        assert!(p.iter().all(|p| p.source.as_deref() == Some("hlint")));
        assert_eq!(p[1].fixes[0].title, "Use print");

        // Made, each rewrite is the code hlint means. A lambda's brackets
        // stay: hlint's span is inside them, and its `to` has its own.
        let source = include_str!("samples/Hints.hs");
        let once = fixed(source, &p[..3]);
        assert!(once.contains("describe n = \"crew: \" ++ show n\n") && once.contains("shout s = print s\n") && once.contains("twice xs = map ((* 2)) xs\n"), "{once}");
        assert!(fixed(source, &p[3..]).ends_with("twice = map (\\x -> x * 2)\n"));
    }

    #[test]
    fn a_parse_error_is_an_error_without_a_fix() {
        let p = parse(&out(0, IDEAS, ""), "src/Broken.hs").unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].severity, p[0].start, p[0].message.as_str()), (Severity::Error, (2, 8), "Parse error: on input `='"));
        assert!(p[0].fixes.is_empty());
    }

    #[test]
    fn nothing_said_is_nothing_found_unless_it_failed() {
        assert!(parse(&out(0, "[]\n", ""), "src/A.hs").unwrap().is_empty());
        assert!(parse(&out(0, "No hints\n", ""), "src/A.hs").unwrap().is_empty());
        assert_eq!(parse(&out(1, "", "hlint: Failed to read .hlint.yaml"), "src/A.hs").unwrap_err(), "hlint: Failed to read .hlint.yaml");
        let removal = r#"[{"severity":"Warning","hint":"Unused LANGUAGE pragma","file":"A.hs","startLine":1,"startColumn":1,"endLine":1,"endColumn":31,"to":"","note":[]}]"#;
        let p = parse(&out(0, removal, ""), "A.hs").unwrap();
        assert_eq!((p[0].message.as_str(), p[0].fixes[0].edits[0].text.as_str()), ("Unused LANGUAGE pragma: remove it", ""));
        let multi = r#"[{"severity":"Suggestion","hint":"Use list comprehension","file":"A.hs","startLine":3,"startColumn":5,"endLine":5,"endColumn":9,"to":"[x\n  | x <- xs]","note":[]}]"#;
        let p = parse(&out(0, multi, ""), "A.hs").unwrap();
        assert!(p[0].fixes.is_empty() && p[0].message == "Use list comprehension", "laid out again by GHC: not offered");
    }

    #[test]
    fn hlint_runs_on_the_haskell_file_saved() {
        let l = linter(&Command::new("/g/hlint"));
        assert_eq!(line(&(l.command)("src/Orbit.hs")), "/g/hlint --json --no-exit-code src/Orbit.hs");
        assert!(l.takes("src/Orbit.hs") && l.takes("src/Orbit.hs-boot") && !l.takes("orbit.cabal"));
    }
}
