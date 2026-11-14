//! `statix` and `deadnix` on a saved `.nix` file, their JSON read into
//! problems.
//!
//! **statix** (`statix check -o json -- <file>`, built with its `json`
//! feature, as nixpkgs builds it) prints one JSON object for the file: a
//! `report` of lints, each with a `note` (the lint's name), a `code` (7 is
//! W07), a `severity` (`Warn`, `Error`, `Hint`) and `diagnostics`, each an
//! `at` range (`from` and `to`, a `line` and `column` from 1, the end not
//! included), a `message`, and a `suggestion` -- the range to replace and the
//! `fix` text -- when it has one. Its problems come with those fixes, which
//! `SPC c a` applies. It exits 1 when it found something and 0 when it
//! found nothing (and printed nothing); a syntax error is a lint with code 0.
//! A statix without the `json` feature says so on stderr and exits 2.
//!
//! **deadnix** (`deadnix -o json -- <file>`) prints `{"file", "results"}`
//! on one line, each result a `line`, `column` and `endColumn` (from 1, the
//! end not included, on one line) and a `message` ("Unused let binding:
//! x"). It exits 0 either way. It offers no fixes in its JSON (`--edit`
//! rewrites the file itself), so none are offered here. A file it can't parse
//! has results that are only a `message`: those are the language server's to
//! say, so they're left out.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use fenix_extension_api::{Command, Problem, Severity, TextEdit};
use fenix_lang_kit::lint::{from_one, severity};
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{Linter, Output};

fn position(point: &Value) -> Option<(u32, u32)> {
    let line = u32::try_from(point.get("line")?.as_u64()?).ok()?;
    let column = u32::try_from(point.get("column")?.as_u64()?).ok()?;
    Some(from_one(line, column))
}

/// What each linter last found in each file. Fenix keeps one list of
/// problems per extension and file (`diagnostics-set` replaces what the
/// extension gave before), so two linters on one file would each wipe out
/// the other's: each sets its own findings together with the other's last,
/// and the file shows both once both have run.
#[derive(Clone, Default)]
pub struct Shared(Rc<RefCell<Found>>);

/// By linter's name and file.
type Found = BTreeMap<(String, String), Vec<Problem>>;

impl Shared {
    /// `mine`, found by `source` in `file`, kept, and with what the others
    /// last found there.
    pub fn merge(&self, source: &str, file: &str, mine: Vec<Problem>) -> Vec<Problem> {
        let mut all = self.0.borrow_mut();
        all.insert((source.to_string(), file.to_string()), mine.clone());
        let mut out: Vec<Problem> = all.iter().filter(|((s, f), _)| s != source && f == file).flat_map(|(_, ps)| ps.iter().cloned()).collect();
        out.extend(mine);
        out.sort_by_key(|p| p.start);
        out
    }

    /// `source` turned off: what it found is no longer shown beside the others'.
    pub fn forget(&self, source: &str) {
        self.0.borrow_mut().retain(|(s, _), _| s != source);
    }
}

/// `statix check -o json` for a file.
pub fn statix(statix: &Command, shared: &Shared) -> Linter {
    let statix = statix.clone();
    let shared = shared.clone();
    Linter::with_fixes(
        "statix",
        &["nix"],
        move |file| statix.clone().args(["check", "-o", "json", "--", file]),
        move |out, file| Ok(shared.merge("statix", file, parse_statix(out, file)?)),
    )
}

/// What statix said about the saved file.
pub fn parse_statix(out: &Output, _file: &str) -> Result<Vec<Problem>, String> {
    let Some(report) = out.json_stream().into_iter().next() else {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    };
    let lints = report.get("report").and_then(Value::as_array).ok_or_else(|| "its output isn't what statix check -o json prints".to_string())?;
    let mut problems = Vec::new();
    for lint in lints {
        let note = lint.get("note").and_then(Value::as_str).unwrap_or("");
        let word = lint.get("severity").and_then(Value::as_str).unwrap_or("Warn");
        let level = severity(word).unwrap_or(Severity::Warning);
        let code = lint.get("code").and_then(Value::as_u64).filter(|c| *c > 0).map(|c| {
            let letter = match level {
                Severity::Error => 'E',
                Severity::Hint | Severity::Info => 'H',
                Severity::Warning => 'W',
            };
            format!("{letter}{c:02}")
        });
        for d in lint.get("diagnostics").and_then(Value::as_array).into_iter().flatten() {
            let at = d.get("at");
            let Some(start) = at.and_then(|a| a.get("from")).and_then(position) else { continue };
            let end = at.and_then(|a| a.get("to")).and_then(position).filter(|e| *e > start).unwrap_or((start.0, start.1 + 1));
            let message = d.get("message").and_then(Value::as_str).unwrap_or(note);
            let mut problem = Problem::new(level, start, end, message).source("statix");
            if let Some(code) = &code {
                problem = problem.code(code);
            }
            if let Some(s) = d.get("suggestion").filter(|s| s.is_object()) {
                let from = s.get("at").and_then(|a| a.get("from")).and_then(position);
                let to = s.get("at").and_then(|a| a.get("to")).and_then(position);
                if let (Some(from), Some(to), Some(fix)) = (from, to, s.get("fix").and_then(Value::as_str)) {
                    problem = problem.fix(&format!("statix fix: {note}"), vec![TextEdit { start: from, end: to, text: fix.to_string() }]);
                }
            }
            problems.push(problem);
        }
    }
    Ok(problems)
}

/// `deadnix -o json` for a file; `-L` leaves a function's unused arguments
/// alone when `arguments` is false.
pub fn deadnix(deadnix: &Command, arguments: bool, shared: &Shared) -> Linter {
    let deadnix = deadnix.clone();
    let shared = shared.clone();
    Linter::with_fixes(
        "deadnix",
        &["nix"],
        move |file| {
            let cmd = deadnix.clone().args(["-o", "json"]);
            let cmd = if arguments { cmd } else { cmd.arg("-L") };
            cmd.args(["--", file])
        },
        move |out, file| Ok(shared.merge("deadnix", file, parse_deadnix(out, file)?)),
    )
}

/// What deadnix said about the saved file: each unused binding, argument or
/// pattern name a warning, with its place.
pub fn parse_deadnix(out: &Output, _file: &str) -> Result<Vec<Problem>, String> {
    let Some(report) = out.json_stream().into_iter().next() else {
        return if out.ok() { Ok(Vec::new()) } else { Err(out.why()) };
    };
    let results = report.get("results").and_then(Value::as_array).ok_or_else(|| "its output isn't what deadnix -o json prints".to_string())?;
    let mut problems = Vec::new();
    for r in results {
        let (Some(line), Some(column)) = (r.get("line").and_then(Value::as_u64), r.get("column").and_then(Value::as_u64)) else { continue };
        let end = r.get("endColumn").and_then(Value::as_u64).unwrap_or(column + 1);
        let (line, column, end) = (u32::try_from(line).unwrap_or(1), u32::try_from(column).unwrap_or(1), u32::try_from(end).unwrap_or(1));
        let start = from_one(line, column);
        let end = (start.0, end.saturating_sub(1).max(start.1 + 1));
        let message = r.get("message").and_then(Value::as_str).unwrap_or("Unused declaration");
        problems.push(Problem::new(Severity::Warning, start, end, message).source("deadnix"));
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, stdout: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: String::new() }
    }

    fn line(c: &Command) -> String {
        fenix_test_kit::line(c)
    }

    #[test]
    fn statix_runs_on_the_saved_file() {
        let l = statix(&Command::new("/n/statix"), &Shared::default());
        assert_eq!(line(&(l.command)("hosts/a.nix")), "/n/statix check -o json -- hosts/a.nix");
        assert!(l.takes("flake.nix") && l.takes("hosts/a.nix") && !l.takes("flake.lock") && !l.takes("a.nixt"));
        assert!(!l.across);
    }

    #[test]
    fn statix_problems_come_with_their_fixes() {
        // Recorded from statix 0.5.8 (built with its json feature) on src/samples/statix-input.nix.
        let problems = parse_statix(&out(1, include_str!("samples/statix-lint.json")), "lint.nix").unwrap();
        assert_eq!(problems.len(), 3);
        let p = &problems[0];
        assert_eq!((p.start, p.end, p.severity), ((9, 6), (9, 15), Severity::Warning), "from 0; the end after the last character: `x == true` is columns 7 to 15 of line 10");
        assert_eq!((p.message.as_str(), p.source.as_deref(), p.code.as_deref()), ("Comparing `x` with boolean literal `true`", Some("statix"), Some("W01")));
        assert_eq!(p.fixes.len(), 1);
        assert_eq!(p.fixes[0].title, "statix fix: Unnecessary comparison with boolean");
        assert_eq!((p.fixes[0].edits[0].start, p.fixes[0].edits[0].end, p.fixes[0].edits[0].text.as_str()), ((9, 6), (9, 15), "x"));
        assert_eq!(problems[1].fixes[0].edits[0].text, "inherit y;");
        assert_eq!((problems[2].start, problems[2].code.as_deref()), ((17, 4), Some("W03")));
    }

    #[test]
    fn a_fix_applies_to_the_text_it_was_found_in() {
        let input = include_str!("samples/statix-input.nix");
        let problems = parse_statix(&out(1, include_str!("samples/statix-lint.json")), "lint.nix").unwrap();
        let mut text = fenix_test_kit::fake::apply_edits(input, &problems[0].fixes[0].edits);
        assert!(text.contains("  a = x;\n") && !text.contains("x == true"), "{text}");
        text = fenix_test_kit::fake::apply_edits(input, &problems[1].fixes[0].edits);
        assert!(text.contains("b = { inherit x; inherit y; };"), "{text}");
    }

    #[test]
    fn a_syntax_error_is_an_error_where_statix_says() {
        let problems = parse_statix(&out(1, include_str!("samples/statix-syntax.json")), "syn.nix").unwrap();
        assert_eq!(problems.len(), 4);
        assert_eq!((problems[0].severity, problems[0].start, problems[0].end, problems[0].code.as_deref()), (Severity::Error, (0, 6), (0, 7), None));
        assert!(problems[0].message.starts_with("Unexpected TOKEN_SEMICOLON") && problems[0].fixes.is_empty());
        assert_eq!(problems[2].start, (0, 0), "at the end of the file: its range is empty, so it's one character");
        assert_eq!(problems[2].end, (0, 1));
    }

    #[test]
    fn nothing_printed_is_nothing_found_and_a_failure_is_the_linters() {
        assert_eq!(parse_statix(&out(0, ""), "a.nix").unwrap(), Vec::new());
        let no_json = Output { code: Some(2), stdout: String::new(), stderr: "error: Invalid value for '--format <FORMAT>': statix was not compiled with the `json` feature flag\n\nFor more information try --help\n".into() };
        assert_eq!(parse_statix(&no_json, "a.nix").unwrap_err(), "For more information try --help");
        assert!(parse_statix(&out(0, "{\"file\": \"a.nix\"}"), "a.nix").is_err());
        let gone = Output { code: Some(1), stdout: String::new(), stderr: "config error: path error: file not found: a.nix\n".into() };
        assert!(parse_statix(&gone, "a.nix").unwrap_err().contains("file not found"));
    }

    #[test]
    fn two_linters_on_one_file_show_both_and_each_forgets_when_turned_off() {
        let shared = Shared::default();
        let at = |line: u32, source: &str| Problem::new(Severity::Warning, (line, 0), (line, 1), "x").source(source);
        assert_eq!(shared.merge("statix", "a.nix", vec![at(4, "statix")]).len(), 1, "alone, its own");
        let both = shared.merge("deadnix", "a.nix", vec![at(9, "deadnix"), at(1, "deadnix")]);
        assert_eq!(both.iter().map(|p| p.start.0).collect::<Vec<_>>(), [1, 4, 9], "both, in order of their place");
        assert_eq!(shared.merge("deadnix", "b.nix", vec![at(2, "deadnix")]).len(), 1, "another file's are another's");
        // statix runs again and finds nothing: deadnix's stay.
        assert_eq!(shared.merge("statix", "a.nix", vec![]).len(), 2);
        shared.forget("statix");
        assert_eq!(shared.merge("deadnix", "a.nix", vec![at(1, "deadnix")]).len(), 1);
    }

    #[test]
    fn deadnix_runs_on_the_saved_file_and_can_leave_arguments_alone() {
        let l = deadnix(&Command::new("/n/deadnix"), true, &Shared::default());
        assert_eq!(line(&(l.command)("a.nix")), "/n/deadnix -o json -- a.nix");
        let l = deadnix(&Command::new("deadnix"), false, &Shared::default());
        assert_eq!(line(&(l.command)("a.nix")), "deadnix -o json -L -- a.nix");
        assert!(l.takes("shell.nix"));
    }

    #[test]
    fn deadnixs_results_are_warnings_where_it_says() {
        // Recorded from deadnix 1.3.1 on src/samples/deadnix-input.nix.
        let problems = parse_deadnix(&out(0, include_str!("samples/deadnix.json")), "ok.nix").unwrap();
        assert_eq!(problems.len(), 13);
        let p = &problems[0];
        assert_eq!((p.start, p.end, p.severity, p.source.as_deref()), ((0, 13), (0, 19), Severity::Warning, Some("deadnix")));
        assert_eq!(p.message, "Unused lambda pattern: unused");
        assert_eq!((problems[1].start, problems[1].end, problems[1].message.as_str()), ((2, 2), (2, 3), "Unused let binding: x"));
        assert!(problems.iter().all(|p| p.fixes.is_empty()), "deadnix's JSON has no fixes");
        assert_eq!(parse_deadnix(&out(0, "{\"file\":\"a.nix\",\"results\":[]}"), "a.nix").unwrap(), Vec::new());
        assert_eq!(parse_deadnix(&out(0, ""), "a.nix").unwrap(), Vec::new());
    }

    #[test]
    fn a_file_deadnix_cant_parse_is_left_to_the_server() {
        let problems = parse_deadnix(&out(0, include_str!("samples/deadnix-syntax.json")), "syn.nix").unwrap();
        assert!(problems.is_empty());
        assert!(parse_deadnix(&out(2, ""), "a.nix").is_err());
        assert!(parse_deadnix(&out(0, "[]"), "a.nix").is_err());
    }
}
