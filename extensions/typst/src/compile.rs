//! Compiling: the command lines for `typst compile` and `typst watch`, and
//! what `--diagnostic-format short` prints read into problems.
//!
//! Short format is one line a diagnostic, `path:line:column: severity:
//! message`. The line counts from 1 and **the column from 0**, in
//! characters (checked on Typst 0.15.1: `*a` unclosed on line 1 is
//! `1:0`), so the line is taken one down and the column as it comes.
//! A diagnostic with no place (an input file that isn't there) is a line
//! of just `error: message`; it's shown on the document, at its top.
//! Hints are not in this format.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::lint::relative;
use fenix_lang_kit::Files;

/// `typst compile` for `main`, writing `pdf`, run in the project's folder:
/// the project is the root, so `/lib.typ` in an `#import` is its own.
pub fn compile(typst: Command, main: &str, pdf: &str) -> Command {
    typst.args(["compile", "--diagnostic-format", "short", "--root", ".", main, pdf])
}

/// `typst watch`, which compiles again whenever a file it read changes.
pub fn watch(typst: Command, main: &str, pdf: &str) -> Command {
    typst.args(["watch", "--diagnostic-format", "short", "--root", ".", main, pdf])
}

/// One diagnostic line: its path, line and column, severity and message.
fn place(line: &str) -> Option<(&str, u32, u32, Severity, &str)> {
    // A drive's colon (`C:\book\main.typ`) belongs to the path.
    let skip = if line.len() > 2 && line.as_bytes()[1] == b':' && line.as_bytes()[0].is_ascii_alphabetic() { 2 } else { 0 };
    let mut parts = line[skip..].splitn(4, ':');
    let path = parts.next()?;
    let (l, c) = (parts.next()?.parse::<u32>().ok()?, parts.next()?.parse::<u32>().ok()?);
    let rest = parts.next()?.trim_start();
    let (word, message) = rest.split_once(':')?;
    let severity = match word {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        _ => return None,
    };
    let path = &line[..skip + path.len()];
    Some((path, l, c, severity, message.trim()))
}

/// The problems `output` says, by file: in the project (`chapters/a.typ`),
/// or a full path for one outside it, which is how Typst prints a file it
/// found away from the project's root. `main` is where one with no place
/// goes. Errors first in each file.
pub fn parse(output: &str, root: &str, main: &str) -> Files {
    let mut files: Files = Vec::new();
    let mut add = |file: String, p: Problem| match files.iter_mut().find(|(f, _)| *f == file) {
        Some((_, ps)) => ps.push(p),
        None => files.push((file, vec![p])),
    };
    for line in output.lines() {
        let line = line.trim_end();
        if let Some((path, l, c, severity, message)) = place(line) {
            let path = path.replace('\\', "/");
            let file = relative(&path, root).unwrap_or_else(|| path.trim_start_matches("./").to_string());
            let at = (l.saturating_sub(1), c);
            add(file, Problem::new(severity, at, (at.0, at.1 + 1), message.to_string()));
        } else if let Some((severity, message)) = line.strip_prefix("error: ").map(|m| (Severity::Error, m)).or_else(|| line.strip_prefix("warning: ").map(|m| (Severity::Warning, m))) {
            add(main.to_string(), Problem::new(severity, (0, 0), (0, 1), message.trim().to_string()));
        }
    }
    for (_, ps) in &mut files {
        ps.sort_by_key(|p| (p.severity != Severity::Error, p.start));
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/samples").join(name)).unwrap()
    }

    type Seen = Vec<(String, Vec<(Severity, (u32, u32), String)>)>;

    fn summary(files: &Files) -> Seen {
        files.iter().map(|(f, ps)| (f.clone(), ps.iter().map(|p| (p.severity, p.start, p.message.clone())).collect())).collect()
    }

    #[test]
    fn the_command_lines_read_the_project_as_the_root() {
        let c = compile(Command::new("/t/typst"), "src/main.typ", "src/main.pdf");
        assert_eq!(fenix_test_kit::line(&c), "/t/typst compile --diagnostic-format short --root . src/main.typ src/main.pdf");
        assert_eq!(fenix_test_kit::line(&watch(Command::new("typst"), "main.typ", "main.pdf")), "typst watch --diagnostic-format short --root . main.typ main.pdf");
    }

    #[test]
    fn an_error_in_an_imported_file_is_that_files() {
        // Recorded: Typst 0.15.1, short format.
        let files = parse(&sample("compile-error.txt"), "/project", "main.typ");
        assert_eq!(summary(&files), [("main.typ".to_string(), vec![(Severity::Error, (4, 9), "unknown variable: undefined-name".to_string())])]);
        let found = parse(&sample("compile-import-error.txt"), "/project", "main.typ");
        assert_eq!(summary(&found), [("chapters/intro.typ".to_string(), vec![(Severity::Error, (0, 21), "unknown variable: foo".to_string())])]);
    }

    #[test]
    fn warnings_and_errors_in_one_run_errors_first() {
        let files = parse(&sample("compile-warning.txt"), "/project", "w.typ");
        assert_eq!(
            summary(&files),
            [("w.typ".to_string(), vec![(Severity::Error, (5, 6), "unexpected argument".to_string()), (Severity::Warning, (0, 16), "unknown font family: no such font".to_string())])]
        );
    }

    #[test]
    fn several_errors_and_columns_that_count_from_zero_in_characters() {
        let files = parse(&sample("compile-two-errors.txt"), "/project", "h3.typ");
        assert_eq!(files[0].1.iter().map(|p| p.start).collect::<Vec<_>>(), [(0, 2), (1, 9)]);
        // `éé *a`: the star is the fourth character, column 3.
        assert_eq!(parse(&sample("compile-columns.txt"), "/project", "c1.typ")[0].1[0].start, (0, 3));
        assert_eq!(parse("c0.typ:1:0: error: unclosed delimiter\n", "/project", "c0.typ")[0].1[0].start, (0, 0));
    }

    #[test]
    fn a_diagnostic_with_no_place_goes_on_the_document() {
        let files = parse(&sample("compile-no-input.txt"), "/project", "nosuch.typ");
        assert_eq!(summary(&files), [("nosuch.typ".to_string(), vec![(Severity::Error, (0, 0), "input file not found (searched at nosuch.typ)".to_string())])]);
    }

    #[test]
    fn a_full_path_in_the_project_is_made_relative_and_one_outside_stays() {
        let files = parse("/home/ada/book/chapters/a.typ:3:1: error: bad\n/home/ada/.cache/typst/packages/preview/x/lib.typ:2:5: warning: old\nC:\\book\\main.typ:1:2: error: win\n", "/home/ada/book", "main.typ");
        let names: Vec<&str> = files.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(names, ["chapters/a.typ", "/home/ada/.cache/typst/packages/preview/x/lib.typ", "C:/book/main.typ"]);
        // The file-not-found line Typst prints carries its path after the message: still one line, no place of its own.
        let nf = parse(&sample("compile-file-not-found.txt"), "/home/ada/book", "main.typ");
        assert_eq!(nf[0].0, "h1.typ");
        assert_eq!(nf[0].1[0].message, "file not found (searched at /home/ada/book/nope.typ)");
    }

    #[test]
    fn a_clean_compile_and_noise_say_nothing() {
        assert!(parse(&sample("compile-ok.txt"), "/project", "main.typ").is_empty());
        assert!(parse("compiling...\nwriting main.pdf\n", "/project", "main.typ").is_empty());
    }
}
