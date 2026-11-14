//! A TeX log read into problems: its errors; the warnings that matter --
//! undefined references and citations, and every other `LaTeX Warning` and
//! package warning; overfull boxes as warnings and underfull ones as info.
//!
//! The format is TeX's own and has no grammar, so this is the heuristics
//! every tool that reads it uses, written down:
//!
//! - TeX breaks the log's lines at 79 characters; a line of exactly 79 is
//!   joined to the next.
//! - Which file a message is about is found from the parentheses the log
//!   opens a file with, `(./chapters/a.tex`, and closes with `)`. A `(` that
//!   isn't followed by something that looks like a file is kept in the stack
//!   as a plain one, so the `)`s balance. Message lines aren't scanned.
//! - An error is `! message.` and its place is the `l.12 \foo` line that
//!   follows; with `-file-line-error`, `./main.tex:12: message.`. An error in
//!   a file that isn't the project's (a package, the class) is put on the
//!   project file that read it, at its top, saying which file it was.
//! - Tectonic's console says `error: main.tex:5: message` and `warning: ...`:
//!   the same, read the same way.

use fenix_extension_api::{Problem, Severity};
use fenix_lang_kit::Files;

/// What a log is read against.
pub struct Context<'a> {
    /// The project's folder as the log may write it in full (`/home/ada/book`).
    pub root: &'a str,
    /// The folder the names in the log are relative to, in the project.
    pub base: &'a str,
    /// The document: where a message about nothing in particular goes.
    pub main: &'a str,
    /// Whether a file (in the project) exists.
    pub exists: &'a dyn Fn(&str) -> bool,
}

/// One message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub severity: Severity,
    /// The file, in the project (`chapters/a.tex`).
    pub file: String,
    /// The line, from 1; none when the message has none.
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    pub message: String,
}

/// Extensions a name in a log's parentheses may have and be a file.
const EXTENSIONS: &[&str] = &[
    "tex", "sty", "cls", "def", "cfg", "fd", "clo", "ldf", "aux", "toc", "lof", "lot", "out", "bbl", "bib", "lbx", "bbx", "cbx", "dbx", "ltx", "cnf", "code", "dtx", "txt", "map", "pgf", "tikz", "bst", "xmpdata",
];

enum Frame {
    /// A file of the project, by its path in it.
    Project(String),
    /// Any other file: a package, a class, a font map.
    External(String),
    /// A parenthesis that opened no file.
    Plain,
}

fn looks_like_file(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.starts_with("./") || name.starts_with("../") || name.starts_with('/') || name.as_bytes().get(1) == Some(&b':') {
        return true;
    }
    name.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn classify(ctx: &Context, name: &str) -> Frame {
    let name = name.replace('\\', "/");
    let name = name.trim_start_matches("./");
    let absolute = name.starts_with('/') || name.as_bytes().get(1) == Some(&b':');
    if absolute {
        let root = ctx.root.replace('\\', "/");
        let root = root.trim_end_matches('/');
        return match name.strip_prefix(root).and_then(|r| r.strip_prefix('/')) {
            Some(rest) if !root.is_empty() => Frame::Project(rest.to_string()),
            _ => Frame::External(name.to_string()),
        };
    }
    let joined = crate::project::join(ctx.base, name);
    for candidate in joined.iter().map(String::as_str).chain(std::iter::once(name)) {
        if (ctx.exists)(candidate) {
            return Frame::Project(candidate.to_string());
        }
    }
    Frame::External(name.to_string())
}

/// TeX's lines unwrapped: one of exactly 79 characters goes on in the next.
fn unwrap(log: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for raw in log.lines() {
        let raw = raw.trim_end_matches('\r');
        cur.push_str(raw);
        if raw.chars().count() != 79 {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The line number `l.12 text` says, and its text.
fn place(line: &str) -> Option<(u32, &str)> {
    let rest = line.strip_prefix("l.")?;
    let n = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let (digits, text) = rest.split_at(n);
    Some((digits.parse().ok()?, text.trim_start()))
}

/// The last control sequence in `text`: what an undefined one was.
fn culprit(text: &str) -> Option<String> {
    let at = text.rfind('\\')?;
    let name: String = text[at + 1..].chars().take_while(|c| c.is_ascii_alphabetic() || *c == '@').collect();
    (!name.is_empty()).then(|| format!("\\{name}"))
}

/// `file:12: message` -- file-line-error's, and Tectonic's after its prefix.
fn file_line(line: &str) -> Option<(&str, u32, &str)> {
    let skip = if line.len() > 2 && line.as_bytes()[1] == b':' && line.as_bytes()[0].is_ascii_alphabetic() { 2 } else { 0 };
    let colon = line[skip..].find(':')? + skip;
    let path = &line[..colon];
    if path.contains(' ') || !looks_like_file(path) {
        return None;
    }
    let rest = &line[colon + 1..];
    let (n, message) = rest.split_once(':')?;
    Some((path, n.trim().parse().ok()?, message.trim()))
}

fn is_box(message: &str) -> bool {
    message.starts_with("Overfull \\") || message.starts_with("Underfull \\")
}

/// `(12.3pt too wide) in paragraph at lines 20--21` -> its lines.
fn box_lines(message: &str) -> (Option<u32>, Option<u32>) {
    let after = |key: &str| message.find(key).map(|i| &message[i + key.len()..]);
    let number = |s: &str| -> Option<u32> {
        let digits: String = s.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    };
    if let Some(rest) = after("at lines ") {
        let first = number(rest);
        let second = rest.split_once("--").and_then(|(_, b)| number(b));
        return (first, second.or(first));
    }
    if let Some(rest) = after("at line ") {
        let n = number(rest);
        return (n, n);
    }
    (None, None)
}

fn box_message(message: &str) -> String {
    // Cut at the place: the rest is where, and the message already says.
    let cut = [" at lines ", " at line ", " has occurred"].iter().filter_map(|k| message.find(k)).min().unwrap_or(message.len());
    message[..cut].trim().to_string()
}

/// A warning of LaTeX's or a package's: `LaTeX Warning: ...`, `Package x
/// Warning: ...` and `Class x Warning: ...`: its text without the prefix.
fn warning_text(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix("LaTeX Warning:").or_else(|| line.strip_prefix("LaTeX Font Warning:")) {
        return Some(rest.trim());
    }
    let rest = line.strip_prefix("Package ").or_else(|| line.strip_prefix("Class "))?;
    let (name, after) = rest.split_once(" Warning:")?;
    (!name.contains(' ')).then(|| after.trim())
}

/// The warning that starts at line `i`, whole: its continuation lines (a
/// package's `(name) ...`) joined.
fn gather(lines: &[String], i: usize, first: &str) -> (String, usize) {
    let mut text = first.to_string();
    let mut j = i + 1;
    while j < lines.len() && j < i + 8 {
        // A package's continuation is `(name)   more of it`; anything else is
        // the next thing in the log, the `)` that closes the package's file included.
        let l = lines[j].trim();
        let Some((name, rest)) = l.strip_prefix('(').and_then(|r| r.split_once(')')) else { break };
        if name.contains(' ') || looks_like_file(name) {
            break;
        }
        text.push(' ');
        text.push_str(rest.trim());
        j += 1;
    }
    (text.split_whitespace().collect::<Vec<_>>().join(" "), j - 1)
}

/// `message on input line 12.` -> (`message`, 12).
fn input_line(text: &str) -> (String, Option<u32>) {
    if let Some(at) = text.rfind(" on input line ") {
        let rest = &text[at + " on input line ".len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse() {
            return (text[..at].trim_end_matches('.').trim().to_string(), Some(n));
        }
    }
    (text.trim_end_matches('.').trim().to_string(), None)
}

/// Whether a line is one TeX or a package wrote about itself, which the
/// parenthesis scan must leave alone.
fn is_message(line: &str) -> bool {
    ["!", "l.", "<", "\\", "LaTeX", "Package", "Class", "Overfull", "Underfull", "Type ", "Here is", "See the", "?", "Runaway", "Output written", "Transcript", "Missing character", "pdfTeX", "Document Class", "For additional", "Please see", "Babel"]
        .iter()
        .any(|p| line.starts_with(p))
}

fn push(out: &mut Vec<Entry>, e: Entry) {
    if !out.contains(&e) {
        out.push(e);
    }
}

/// The messages in `log`.
pub fn entries(log: &str, ctx: &Context) -> Vec<Entry> {
    let lines = unwrap(log);
    let mut stack: Vec<Frame> = Vec::new();
    let mut out: Vec<Entry> = Vec::new();
    // Where the innermost project file is, and whether a message is in it.
    let located = |stack: &[Frame]| -> (String, Option<String>) {
        let top = stack.iter().rev().find(|f| !matches!(f, Frame::Plain));
        let project = stack.iter().rev().find_map(|f| if let Frame::Project(p) = f { Some(p.clone()) } else { None });
        match top {
            Some(Frame::Project(p)) => (p.clone(), None),
            Some(Frame::External(e)) => (project.unwrap_or_else(|| ctx.main.to_string()), Some(e.rsplit('/').next().unwrap_or(e).to_string())),
            _ => (ctx.main.to_string(), None),
        }
    };
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i].as_str();
        let (prefix, line) = match raw.strip_prefix("warning: ") {
            Some(r) => (Some(Severity::Warning), r),
            None => match raw.strip_prefix("error: ") {
                Some(r) => (Some(Severity::Error), r),
                None => (None, raw),
            },
        };
        if raw.starts_with("note: ") {
            i += 1;
            continue;
        }

        // `! message.`, and the `l.12 \foo` that places it.
        if let Some(message) = line.strip_prefix("! ").filter(|_| prefix.is_none()) {
            let mut n = None;
            let mut text = "";
            let mut j = i + 1;
            while j < lines.len() && j < i + 16 && !lines[j].starts_with("! ") {
                if let Some((l, t)) = place(&lines[j]) {
                    n = Some(l);
                    text = &lines[j][lines[j].len() - t.len()..];
                    break;
                }
                j += 1;
            }
            let (file, outside) = located(&stack);
            let mut message = message.trim_end_matches('.').trim_start_matches("LaTeX Error: ").to_string();
            if message == "Undefined control sequence" {
                if let Some(c) = culprit(text) {
                    message = format!("{message}: {c}");
                }
            }
            let (message, line) = match outside {
                Some(name) => (format!("{name}: {message}"), None),
                None => (message, n),
            };
            push(&mut out, Entry { severity: Severity::Error, file, line, end_line: line, message });
            i += 1;
            continue;
        }

        // `./main.tex:12: message.`
        if let Some((path, n, message)) = file_line(line) {
            let severity = prefix.unwrap_or(if is_box(message) || warning_text(message).is_some() { Severity::Warning } else { Severity::Error });
            // What follows it is the context, or a box's dump: not a message.
            let mut skip_to = i;
            let mut text = "";
            // Tectonic's lines are one each: its context follows as lines of
            // their own, which `is_context` knows.
            for j in i + 1..lines.len().min(if prefix.is_some() { i + 3 } else { i + 12 }) {
                if lines[j].trim().is_empty() {
                    break;
                }
                if prefix.is_none() {
                    skip_to = j;
                }
                if let Some((_, t)) = place(&lines[j]) {
                    text = &lines[j][lines[j].len() - t.len()..];
                    break;
                }
                // Tectonic gives the context as `error: main.tex:5: \foo`.
                let next = lines[j].strip_prefix("error: ").or_else(|| lines[j].strip_prefix("warning: ")).unwrap_or(&lines[j]);
                if let Some((_, _, t)) = file_line(next) {
                    if t.starts_with('\\') {
                        text = t;
                    }
                }
            }
            let file = match classify(ctx, path) {
                Frame::Project(p) => p,
                _ => located(&stack).0,
            };
            if is_box(message) {
                let (a, b) = box_lines(message);
                let sev = if message.starts_with("Underfull") { Severity::Info } else { severity };
                push(&mut out, Entry { severity: sev, file, line: a.or(Some(n)), end_line: b.or(Some(n)), message: box_message(message) });
            } else {
                let mut message = message.trim_end_matches('.').to_string();
                if let Some(rest) = message.strip_prefix("LaTeX Error: ") {
                    message = rest.to_string();
                }
                if message == "Undefined control sequence" {
                    if let Some(c) = culprit(text) {
                        message = format!("{message}: {c}");
                    }
                }
                // Tectonic's `halted on ...` is its ending, not a problem.
                if !message.starts_with("halted on potentially-recoverable error") && !(prefix.is_some() && is_context(&out, &file, n, &message)) {
                    push(&mut out, Entry { severity, file, line: Some(n), end_line: Some(n), message });
                }
            }
            i = skip_to + 1;
            continue;
        }

        // `Overfull \hbox (...) in paragraph at lines 20--21`, and its dump.
        if is_box(line) {
            let (a, b) = box_lines(line);
            let (file, _) = located(&stack);
            let severity = if line.starts_with("Underfull") { Severity::Info } else { Severity::Warning };
            push(&mut out, Entry { severity, file, line: a, end_line: b, message: box_message(line) });
            while i + 1 < lines.len() && !lines[i + 1].trim().is_empty() {
                i += 1;
            }
            i += 1;
            continue;
        }

        // `LaTeX Warning: ...`, `Package x Warning: ...`.
        if let Some(first) = warning_text(line) {
            let (text, end) = gather(&lines, i, first);
            let (message, n) = input_line(&text);
            if message == "There were undefined references" {
                i = end + 1;
                continue;
            }
            let (file, outside) = located(&stack);
            let message = match outside {
                Some(name) if n.is_none() => format!("{name}: {message}"),
                _ => message,
            };
            push(&mut out, Entry { severity: Severity::Warning, file, line: n, end_line: n, message });
            i = end + 1;
            continue;
        }
        if line.starts_with("Missing character:") || line.starts_with("pdfTeX warning") {
            let (file, _) = located(&stack);
            push(&mut out, Entry { severity: Severity::Warning, file, line: None, end_line: None, message: line.trim().to_string() });
            i += 1;
            continue;
        }

        // Anything else says what file it's in.
        if !is_message(line) && prefix.is_none() {
            let chars: Vec<char> = line.chars().collect();
            let mut k = 0;
            while k < chars.len() {
                match chars[k] {
                    '(' => {
                        let name: String = chars[k + 1..].iter().take_while(|c| !c.is_whitespace() && **c != ')' && **c != '(').collect();
                        if looks_like_file(&name) {
                            k += name.chars().count();
                            stack.push(classify(ctx, &name));
                        } else {
                            stack.push(Frame::Plain);
                        }
                    }
                    ')' => {
                        stack.pop();
                    }
                    _ => {}
                }
                k += 1;
            }
        }
        i += 1;
    }
    out
}

/// Whether `message` on `file:line` is a line of Tectonic's context for a
/// message it just gave there (`error: main.tex:5: \foo`), which says
/// nothing new.
fn is_context(out: &[Entry], file: &str, line: u32, message: &str) -> bool {
    let near = out.last().is_some_and(|e| e.file == file && e.line == Some(line));
    near && (message.starts_with('\\') || message.starts_with("l.") || message.starts_with("[]") || message.is_empty())
}

/// The problems in `log`, by file: errors first, each file's in order of line.
pub fn parse(log: &str, ctx: &Context) -> Files {
    let mut files: Files = Vec::new();
    for e in entries(log, ctx) {
        let start = (e.line.unwrap_or(1).saturating_sub(1), 0);
        let end = (e.end_line.unwrap_or(e.line.unwrap_or(1)).saturating_sub(1).max(start.0), 1);
        let problem = Problem::new(e.severity, start, end, e.message);
        match files.iter_mut().find(|(f, _)| *f == e.file) {
            Some((_, ps)) => ps.push(problem),
            None => files.push((e.file, vec![problem])),
        }
    }
    for (_, ps) in &mut files {
        ps.sort_by_key(|p| (severity_rank(p.severity), p.start));
    }
    files
}

fn severity_rank(s: Severity) -> u8 {
    match s {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str) -> String {
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/samples").join(name)).unwrap()
    }

    const FILES: &[&str] = &["main.tex", "chapters/intro.tex", "chapters/body.tex", "thesis/main.tex", "thesis/chapters/one.tex"];

    fn read(log: &str, base: &str, main: &str) -> Vec<Entry> {
        let exists = |p: &str| FILES.contains(&p);
        entries(log, &Context { root: "/home/ada/book", base, main, exists: &exists })
    }

    /// `file:line severity message`, one per entry.
    fn shown(es: &[Entry]) -> Vec<String> {
        es.iter()
            .map(|e| {
                let sev = match e.severity {
                    Severity::Error => "E",
                    Severity::Warning => "W",
                    _ => "I",
                };
                match (e.line, e.end_line) {
                    (Some(a), Some(b)) if a != b => format!("{}:{a}-{b} {sev} {}", e.file, e.message),
                    (Some(a), _) => format!("{}:{a} {sev} {}", e.file, e.message),
                    _ => format!("{} {sev} {}", e.file, e.message),
                }
            })
            .collect()
    }

    // The pdflatex-style logs are written from the shape of real logs
    // (`src/samples/README.md`): the parser is held to what they say.

    #[test]
    fn file_line_errors_are_the_files_they_name_and_the_context_says_what_was_undefined() {
        let got = shown(&read(&sample("pdflatex-errors.log"), "", "main.tex"));
        assert_eq!(
            got,
            [
                "main.tex:9 E Undefined control sequence: \\undefinedmacro",
                "chapters/intro.tex:3 E Missing $ inserted",
                "chapters/intro.tex:7 E Environment itemze undefined",
                "main.tex:14 E File `missingpkg.sty' not found",
                "main.tex:14 E Emergency stop",
            ]
        );
    }

    #[test]
    fn warnings_are_in_the_file_that_was_open_and_boxes_say_their_lines() {
        let got = shown(&read(&sample("pdflatex-warnings.log"), "", "main.tex"));
        assert_eq!(
            got,
            [
                "main.tex:14 W Option `hidelinks' is deprecated, use the option `colorlinks=false' instead",
                "main.tex:21-22 W Overfull \\hbox (13.40111pt too wide) in paragraph",
                "chapters/body.tex:5-6 I Underfull \\hbox (badness 10000) in paragraph",
                "chapters/body.tex:12 W Reference `fig:missing' on page 1 undefined",
                "chapters/body.tex:13 W Citation `knuth84' on page 1 undefined",
                "chapters/body.tex:30-33 W Overfull \\hbox (4.5pt too wide) in alignment",
                "chapters/body.tex:40 W Font shape `OT1/cmr/bx/sc' undefined using `OT1/cmr/bx/n' instead",
                "main.tex W `h' float specifier changed to `ht'",
                "main.tex:77 W Reference `sec:a-rather-long-label-name-for-wrapping' on page 12 undefined",
                "main.tex:51 W Citation `lamport94' on page 3 undefined",
                "main.tex W Label(s) may have changed. Rerun to get cross-references right",
            ]
        );
    }

    #[test]
    fn without_file_line_error_the_place_is_the_l_line_and_a_package_error_is_on_the_file_that_read_it() {
        let got = shown(&read(&sample("plain-errors.log"), "", "thesis/main.tex"));
        assert_eq!(
            got,
            [
                "thesis/main.tex W Missing character: There is no ∀ in font [lmroman10-regular]:mapping=tex-text",
                "thesis/chapters/one.tex:5 E Undefined control sequence: \\maketitel",
                "thesis/chapters/one.tex:9 E Extra }, or forgotten $",
                "thesis/main.tex E broken.sty: Package broken Error: Something inside the package went wrong",
                "thesis/main.tex:14 E Missing \\begin{document}",
            ]
        );
    }

    #[test]
    fn tectonics_console_is_read_the_same_way_and_its_ending_and_context_are_not_problems() {
        let got = shown(&read(&sample("tectonic-console.txt"), "", "main.tex"));
        assert_eq!(got, ["main.tex:7 W Overfull \\hbox (13.0pt too wide) in paragraph", "main.tex:5 E Undefined control sequence: \\foo"]);
    }

    #[test]
    fn names_are_relative_to_where_the_builder_ran() {
        // latexmk -cd thesis/main.tex runs in thesis/: the log says `chapters/one.tex`.
        let log = "(./main.tex\n(./chapters/one.tex\n./chapters/one.tex:4: Undefined control sequence.\nl.4 \\foo\n\n)\n)\n";
        let got = shown(&read(log, "thesis", "thesis/main.tex"));
        assert_eq!(got, ["thesis/chapters/one.tex:4 E Undefined control sequence: \\foo"]);
        // A full path in the project is the project's; elsewhere it isn't.
        let log = "(/home/ada/book/chapters/intro.tex\n! Undefined control sequence.\nl.2 \\x\n)\n(/usr/share/texlive/a.sty\n! Undefined control sequence.\nl.2 \\y\n)\n";
        let got = shown(&read(log, "", "main.tex"));
        assert_eq!(got, ["chapters/intro.tex:2 E Undefined control sequence: \\x", "main.tex E a.sty: Undefined control sequence: \\y"]);
    }

    #[test]
    fn a_clean_log_and_parentheses_in_messages_say_nothing_and_the_files_still_balance() {
        let clean = "(./main.tex (./chapters/intro.tex) [1] (see the transcript file for additional information)\nOutput written on main.pdf (1 page, 8000 bytes).\n";
        assert!(read(clean, "", "main.tex").is_empty());
        // A parenthesis in text that isn't a file doesn't shut the file early.
        let log = "(./main.tex (the answer is (42) here)\n! Undefined control sequence.\nl.3 \\foo\n)\n";
        assert_eq!(shown(&read(log, "", "main.tex")), ["main.tex:3 E Undefined control sequence: \\foo"]);
        assert!(read("", "", "main.tex").is_empty());
    }

    #[test]
    fn problems_are_grouped_by_file_errors_first_with_a_range_for_a_box() {
        let exists = |p: &str| FILES.contains(&p);
        let files = parse(&sample("pdflatex-warnings.log"), &Context { root: "/p", base: "", main: "main.tex", exists: &exists });
        let names: Vec<&str> = files.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(names, ["main.tex", "chapters/body.tex"]);
        let main = &files[0].1;
        let overfull = main.iter().find(|p| p.message.starts_with("Overfull")).unwrap();
        assert_eq!((overfull.start, overfull.end), ((20, 0), (21, 1)), "lines 21 to 22, from 0");
        let body = &files[1].1;
        assert_eq!(body.last().unwrap().severity, Severity::Info, "the underfull box is last: info");
        let files = parse(&sample("pdflatex-errors.log"), &Context { root: "/p", base: "", main: "main.tex", exists: &exists });
        assert!(files[0].1.iter().all(|p| p.severity == Severity::Error));
        assert_eq!(files[0].1[0].start, (8, 0), "main.tex:9");
    }
}
