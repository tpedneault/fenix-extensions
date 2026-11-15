//! chktex, LaTeX's linter, run on a saved file: its output read into
//! problems.
//!
//! It's asked for a format of its own, one line a message,
//! `file:line:column:length:kind:number:message`, and `-q` so it says nothing
//! else. `kind` is `Warning`, `Error` or `Message`.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::lint::from_one;
use fenix_lang_kit::{Linter, Output};

/// The format chktex is given (`-f`).
pub const FORMAT: &str = "%f:%l:%c:%d:%k:%n:%m\\n";

/// chktex on `file` (in the project): quiet (`-q`), with the pack's format,
/// and no `-I` (the `-I0` is "don't reformat inclusion notices"). Its own
/// `.chktexrc`, the project's or the user's, is read as it is in a terminal.
pub fn command(chktex: &str, file: &str) -> Command {
    Command::new(chktex).args(["-q", "-I0", "-f", FORMAT]).arg(file)
}

/// The linter, for `.tex`, `.sty` and `.cls` files.
pub fn linter(chktex: &str) -> Linter {
    let chktex = chktex.to_string();
    Linter::new("chktex", &["tex", "sty", "cls"], move |file| command(&chktex, file), |out, file| Ok(parse(out, file)))
}

/// What chktex printed, for `file`.
pub fn parse(out: &Output, file: &str) -> Vec<Problem> {
    let file = file.replace('\\', "/");
    let file = file.trim_start_matches("./");
    out.stdout
        .lines()
        .filter_map(|line| {
            // A drive's colon belongs to the path.
            let skip = if line.len() > 2 && line.as_bytes()[1] == b':' && line.as_bytes()[0].is_ascii_alphabetic() { 2 } else { 0 };
            let mut parts = line[skip..].splitn(7, ':');
            let path = parts.next()?;
            let (l, c, len) = (parts.next()?.parse::<u32>().ok()?, parts.next()?.parse::<u32>().ok()?, parts.next()?.parse::<u32>().ok()?);
            let kind = parts.next()?;
            let number = parts.next()?.trim();
            let message = parts.next()?.trim();
            let path = line[..skip + path.len()].replace('\\', "/");
            let path = path.trim_start_matches("./");
            if !(path == file || path.ends_with(&format!("/{file}")) || file.ends_with(&format!("/{path}"))) {
                return None;
            }
            let severity = match kind {
                "Error" => Severity::Error,
                "Warning" => Severity::Warning,
                _ => Severity::Info,
            };
            // chktex's columns count from 1 and its lines too.
            let (line0, col0) = from_one(l, c);
            let mut p = Problem::new(severity, (line0, col0), (line0, col0 + len.max(1)), message.to_string());
            p.code = Some(number.to_string());
            Some(p)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Written from chktex's documented format string (`-f`), not recorded:
    // there's no chktex here.
    const OUT: &str = "main.tex:5:12:1:Warning:1:Command terminated with space.\n\
main.tex:9:3:6:Warning:8:Wrong length of dash may have been used.\n\
main.tex:14:1:4:Error:11:You should use \\ldots to achieve an ellipsis.\n\
main.tex:20:7:1:Message:36:You should put a space in front of parenthesis.\n\
other.tex:1:1:1:Warning:1:Command terminated with space.\n\
\n\
a line that isn't one\n";

    fn out(stdout: &str) -> Output {
        Output { code: Some(2), stdout: stdout.into(), stderr: String::new() }
    }

    #[test]
    fn its_lines_are_problems_in_the_file_with_their_numbers() {
        let ps = parse(&out(OUT), "main.tex");
        assert_eq!(ps.len(), 4, "the other file's line and the noise are left out");
        assert_eq!((ps[0].severity, ps[0].start, ps[0].end, ps[0].message.as_str(), ps[0].code.as_deref()), (Severity::Warning, (4, 11), (4, 12), "Command terminated with space.", Some("1")));
        assert_eq!((ps[1].start, ps[1].end), ((8, 2), (8, 8)), "its length is the range");
        assert_eq!(ps[2].severity, Severity::Error);
        assert_eq!(ps[3].severity, Severity::Info, "a Message is info");
        assert_eq!(parse(&out(OUT), "./other.tex").len(), 1);
        assert_eq!(parse(&out(OUT), "chapters/main.tex").len(), 4, "a path ending in the file's");
    }

    #[test]
    fn the_command_is_quiet_and_in_its_own_format() {
        assert_eq!(fenix_test_kit::line(&command("/t/chktex", "a/b.tex")), "/t/chktex -q -I0 -f %f:%l:%c:%d:%k:%n:%m\\n a/b.tex");
        let l = linter("/t/chktex");
        assert!(l.takes("a.tex") && l.takes("x.sty") && l.takes("y.CLS") && !l.takes("refs.bib") && !l.takes("a.typ"));
    }

    #[test]
    fn a_windows_path_and_a_message_with_colons_are_read_whole() {
        let ps = parse(&out("C:\\book\\main.tex:3:1:2:Warning:2:Non-breaking space (~) should have been used: here.\n"), "main.tex");
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].message, "Non-breaking space (~) should have been used: here.");
    }
}
