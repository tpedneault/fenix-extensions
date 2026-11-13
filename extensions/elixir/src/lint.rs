//! What's run when an Elixir file is saved, and what it printed read as
//! problems: the compiler's warnings and errors (`mix compile
//! --all-warnings`, which prints those of files it needn't compile again
//! too, so a warning stays until it's fixed) and, in a project that has
//! it, Credo's (`mix credo <file> --format json`).
//!
//! The compiler prints a diagnostic in two shapes. Elixir 1.16 and after:
//!
//! ```text
//!     warning: variable "unused" is unused (...)
//!     │
//!  11 │     unused = 1
//!     │     ~~~~~~
//!     │
//!     └─ lib/orbit.ex:11:5: Orbit.period/1
//! ```
//!
//! and before it, the message and then `  lib/orbit.ex:11: Orbit.period/1`
//! (no column). An error that stops the compile may name its place after
//! `** (CompileError)` instead. All are read here.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::serde_json::Value;
use fenix_lang_kit::{Files, Linter, Output};

/// The extensions of the files the compiler reads, and what it says
/// a place in.
const SOURCES: &[&str] = &["ex", "exs", "heex", "eex", "leex"];

/// `mix compile --all-warnings` for a saved file: the whole project is
/// compiled (Mix compiles what changed), and every file it speaks of has
/// its problems set.
pub fn compile(mix: &Command) -> Linter {
    let mix = mix.clone();
    Linter::across("mix compile", &["ex", "heex", "eex", "leex"], move |_file| mix.clone().args(["compile", "--all-warnings"]), parse_compile).named(&["mix.exs"])
}

/// Credo on a saved file.
pub fn credo(mix: &Command) -> Linter {
    let mix = mix.clone();
    Linter::new("credo", &["ex", "exs"], move |file| mix.clone().args(["credo", file, "--format", "json"]), parse_credo)
}

/// The text without terminal colours.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `lib/orbit.ex:11:5` as a path, a line and a column; the column is
/// optional. Drive letters (`C:\...`) keep their colon.
fn place(token: &str) -> Option<(String, u32, Option<u32>)> {
    let token = token.trim_end_matches(':');
    let mut numbers = Vec::new();
    let mut rest = token;
    while numbers.len() < 2 {
        let Some((head, tail)) = rest.rsplit_once(':') else { break };
        match tail.parse::<u32>() {
            Ok(n) => {
                numbers.push(n);
                rest = head;
            }
            Err(_) => break,
        }
    }
    let ext = rest.rsplit_once('.')?.1;
    if numbers.is_empty() || !SOURCES.contains(&ext) {
        return None;
    }
    let (line, column) = match numbers[..] {
        [line] => (line, None),
        [column, line] => (line, Some(column)),
        _ => return None,
    };
    Some((rest.replace('\\', "/").trim_start_matches("./").to_string(), line, column))
}

/// A diagnostic whose place hasn't come yet.
struct Pending {
    severity: Severity,
    message: String,
    /// How many characters the `~~~~` or `^^^^` under its code covers.
    width: Option<u32>,
}

fn problem(pending: &Pending, line: u32, column: Option<u32>) -> Problem {
    let l = line.saturating_sub(1);
    let (start, end) = match column {
        Some(c) => ((l, c.saturating_sub(1)), (l, c.saturating_sub(1) + pending.width.unwrap_or(1).max(1))),
        None => ((l, 0), (l + 1, 0)),
    };
    Problem::new(pending.severity, start, end, pending.message.clone()).source("mix compile")
}

/// The problems in what `mix compile` printed, by file in the order it
/// spoke of them. Dependencies' (`deps/...`) are left out.
pub fn parse_compile(out: &Output, _file: &str) -> Result<Files, String> {
    let text = plain(&format!("{}\n{}", out.stderr, out.stdout));
    let mut files: Files = Vec::new();
    let mut add = |path: String, p: Problem| {
        if path.starts_with("deps/") || path.contains("/deps/") {
            return;
        }
        match files.iter_mut().find(|(f, _)| *f == path) {
            Some((_, list)) => list.push(p),
            None => files.push((path, vec![p])),
        }
    };
    let mut pending: Option<Pending> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("** (") {
            // `** (CompileError) lib/x.ex:3: message`; a syntax error's
            // header names its place after "found on", and its `error:`
            // line, below, is the one that's read.
            if let Some((_, after)) = rest.split_once(") ") {
                if let Some((token, message)) = after.split_once(' ') {
                    if let Some((path, l, c)) = place(token) {
                        let p = Pending { severity: Severity::Error, message: message.trim().to_string(), width: None };
                        add(path, problem(&p, l, c));
                    }
                }
            }
            continue;
        }
        let header = [("warning: ", Severity::Warning), ("error: ", Severity::Error)].into_iter().find_map(|(word, severity)| line.strip_prefix(word).map(|m| (severity, m)));
        if let Some((severity, message)) = header {
            pending = Some(Pending { severity, message: message.trim().to_string(), width: None });
            continue;
        }
        let Some(p) = pending.as_mut() else { continue };
        if let Some(after) = line.strip_prefix('│') {
            let marks = after.trim();
            if p.width.is_none() && !marks.is_empty() && marks.chars().all(|c| c == '~' || c == '^') {
                p.width = Some(marks.chars().count() as u32);
            }
            continue;
        }
        let token = line.strip_prefix("└─").unwrap_or(line).split_whitespace().next().unwrap_or("");
        if let Some((path, l, c)) = place(token) {
            let p = pending.take().expect("pending");
            add(path, problem(&p, l, c));
        }
    }
    if files.is_empty() && !out.ok() {
        return Err(out.why());
    }
    Ok(files)
}

/// The problems Credo's JSON gives for `file`.
pub fn parse_credo(out: &Output, file: &str) -> Result<Vec<Problem>, String> {
    // Whatever Mix printed first (a compile) isn't JSON: it starts at the
    // first line that opens the object.
    let mut offset = 0;
    let mut start = None;
    for line in out.stdout.split_inclusive('\n') {
        if line.trim_start().starts_with('{') {
            start = Some(offset);
            break;
        }
        offset += line.len();
    }
    let parsed: Option<Value> = start.and_then(|s| fenix_lang_kit::serde_json::from_str(&out.stdout[s..]).ok());
    let Some(issues) = parsed.as_ref().and_then(|v| v["issues"].as_array()) else {
        return Err(if out.ok() { "Credo's output couldn't be read".to_string() } else { out.why() });
    };
    let want = file.replace('\\', "/");
    let want = want.trim_start_matches("./");
    let mut problems = Vec::new();
    for issue in issues {
        let name = issue["filename"].as_str().unwrap_or("").replace('\\', "/");
        if name.trim_start_matches("./") != want {
            continue;
        }
        let line = issue["line_no"].as_u64().unwrap_or(1).max(1) as u32 - 1;
        let message = issue["message"].as_str().unwrap_or("").to_string();
        let category = issue["category"].as_str().unwrap_or("");
        let severity = if category == "warning" {
            Severity::Warning
        } else if issue["priority"].as_i64().unwrap_or(0) < 0 {
            Severity::Hint
        } else {
            Severity::Info
        };
        let (start, end) = match issue["column"].as_u64() {
            Some(c) => {
                let c = c.max(1) as u32 - 1;
                let end = issue["column_end"].as_u64().map_or(c + 1, |e| (e as u32 - 1).max(c + 1));
                ((line, c), (line, end))
            }
            None => ((line, 0), (line + 1, 0)),
        };
        let mut p = Problem::new(severity, start, end, message).source("credo");
        if let Some(check) = issue["check"].as_str() {
            p = p.code(check);
        }
        problems.push(p);
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_test_kit::line;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    type Row<'a> = (Severity, (u32, u32), (u32, u32), &'a str);

    fn rows(p: &[Problem]) -> Vec<Row<'_>> {
        p.iter().map(|p| (p.severity, p.start, p.end, p.message.split(" (").next().unwrap())).collect()
    }

    #[test]
    fn the_compilers_warnings_are_problems_with_their_underlines() {
        let files = parse_compile(&out(0, "", include_str!("samples/mix-compile-warnings.txt")), "lib/orbit.ex").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "lib/orbit.ex");
        assert_eq!(
            rows(&files[0].1),
            [
                (Severity::Warning, (10, 4), (10, 10), "variable \"unused\" is unused"),
                (Severity::Warning, (19, 7), (19, 8), "function nothing/0 is unused"),
                (Severity::Warning, (22, 18), (22, 19), "Orbit.Missing.run/0 is undefined"),
            ],
            "the ~~~~ under the code is how wide it is; from 1 to Fenix's 0"
        );
        assert!(files[0].1[0].message.ends_with("(if the variable is not meant to be used, prefix it with an underscore)"));
        assert!(files[0].1.iter().all(|p| p.source.as_deref() == Some("mix compile")));
    }

    #[test]
    fn elixir_before_1_16_prints_them_without_a_column() {
        let files = parse_compile(&out(0, "Compiling 2 files (.ex)\n", include_str!("samples/mix-compile-warnings-1-15.txt")), "lib/orbit.ex").unwrap();
        assert_eq!(rows(&files[0].1), [(Severity::Warning, (10, 0), (11, 0), "variable \"unused\" is unused"), (Severity::Warning, (19, 0), (20, 0), "function nothing/0 is unused"), (Severity::Warning, (22, 0), (23, 0), "Orbit.Missing.run/0 is undefined")], "the whole line");
    }

    #[test]
    fn a_compile_that_fails_gives_its_errors() {
        let e = parse_compile(&out(1, "", include_str!("samples/mix-compile-error.txt")), "lib/broken.ex").unwrap();
        assert_eq!(rows(&e[0].1), [(Severity::Error, (2, 4), (2, 19), "undefined variable \"undefined_thing\"")]);
        assert_eq!(e[0].0, "lib/broken.ex");

        // A syntax error: its `error:` line, and the place under it.
        let s = parse_compile(&out(1, "", include_str!("samples/mix-compile-syntax-error.txt")), "lib/broken.ex").unwrap();
        assert_eq!((s[0].0.as_str(), s[0].1.len(), s[0].1[0].severity, s[0].1[0].start), ("lib/broken.ex", 1, Severity::Error, (7, 2)));
        assert!(s[0].1[0].message.starts_with("syntax error before: end"));

        // Before 1.16 an error names its place after the exception.
        let old = parse_compile(&out(1, "", include_str!("samples/mix-compile-error-1-15.txt")), "lib/broken.ex").unwrap();
        assert_eq!(old[0].1[0].start, (2, 0));
        assert!(old[0].1[0].message.starts_with("undefined function undefined_thing/0"));
    }

    #[test]
    fn a_compile_that_says_nothing_is_clean_and_a_failure_that_names_no_file_is_an_error() {
        assert_eq!(parse_compile(&out(0, "Compiling 1 file (.ex)\nGenerated orbit app\n", ""), "lib/a.ex").unwrap(), Vec::new());
        let unchecked = "Unchecked dependencies for environment dev:\n* credo (Hex package)\n  the dependency is not available, run \"mix deps.get\"\n** (Mix) Can't continue due to errors on dependencies\n";
        assert_eq!(parse_compile(&out(1, "", unchecked), "lib/a.ex"), Err("** (Mix) Can't continue due to errors on dependencies".into()));
        // A dependency's warning isn't the project's.
        let dep = "    warning: unused alias X\n    │\n    └─ deps/jason/lib/jason.ex:3:3: Jason\n";
        assert_eq!(parse_compile(&out(0, "", dep), "lib/a.ex").unwrap(), Vec::new());
        // Terminal colours are taken off.
        let coloured = "\u{1b}[33mwarning: \u{1b}[0mx is unused\n  lib/a.ex:2: A.f/0\n";
        assert_eq!(parse_compile(&out(0, "", coloured), "lib/a.ex").unwrap()[0].1.len(), 1);
    }

    #[test]
    fn places_have_a_line_and_maybe_a_column() {
        assert_eq!(place("lib/orbit.ex:11:5:"), Some(("lib/orbit.ex".into(), 11, Some(5))));
        assert_eq!(place("lib/orbit.ex:11"), Some(("lib/orbit.ex".into(), 11, None)));
        assert_eq!(place("C:\\p\\lib\\orbit.ex:3:1"), Some(("C:/p/lib/orbit.ex".into(), 3, Some(1))));
        assert_eq!(place("lib/orbit/page.html.heex:4:9"), Some(("lib/orbit/page.html.heex".into(), 4, Some(9))));
        assert_eq!((place("orbit.ex"), place("lib/orbit.c:3"), place("12:3")), (None, None, None));
    }

    #[test]
    fn credos_json_is_problems_in_the_file() {
        let json = include_str!("samples/credo.json");
        let p = parse_credo(&out(10, json, ""), "lib/orbit/crew.ex").unwrap();
        assert_eq!(
            rows(&p),
            [
                (Severity::Info, (0, 10), (0, 15), "Modules should have a @moduledoc tag."),
                (Severity::Warning, (13, 4), (13, 14), "There should be no calls to `IO.inspect/1`."),
                (Severity::Hint, (19, 0), (20, 0), "Found a TODO tag in a comment: # TODO: sort by rank"),
            ],
            "another file's left out; no column is the whole line; a negative priority a hint"
        );
        assert_eq!((p[0].code.as_deref(), p[0].source.as_deref()), (Some("Credo.Check.Readability.ModuleDoc"), Some("credo")));
        assert_eq!(parse_credo(&out(10, json, ""), "lib/orbit/dock.ex").unwrap().len(), 1);

        // What Mix printed first isn't JSON; Credo's own failure says why.
        let with_compile = format!("Compiling 1 file (.ex)\n{json}");
        assert_eq!(parse_credo(&out(10, &with_compile, ""), "lib/orbit/crew.ex").unwrap().len(), 3);
        assert_eq!(parse_credo(&out(0, "{\"issues\": []}\n", ""), "lib/a.ex").unwrap(), Vec::new());
        assert_eq!(parse_credo(&out(1, "", "** (Mix) The task \"credo\" could not be found\n"), "lib/a.ex"), Err("** (Mix) The task \"credo\" could not be found".into()));
        assert!(parse_credo(&out(0, "no json here", ""), "lib/a.ex").is_err());
    }

    #[test]
    fn the_linters_run_mix_for_the_file_saved() {
        let c = compile(&Command::new("/t/mix"));
        assert_eq!(line(&(c.command)("lib/orbit.ex")), "/t/mix compile --all-warnings");
        assert!(c.across && c.takes("lib/orbit.ex") && c.takes("lib/orbit_web/page.html.heex") && c.takes("mix.exs") && !c.takes("test/orbit_test.exs") && !c.takes("main.c"));
        let k = credo(&Command::new("/t/mix"));
        assert_eq!(line(&(k.command)("lib/orbit.ex")), "/t/mix credo lib/orbit.ex --format json");
        assert!(k.takes("test/orbit_test.exs") && k.takes("mix.exs") && !k.takes("page.heex"));
    }
}
