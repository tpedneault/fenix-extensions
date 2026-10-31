//! `zig ast-check` on a saved Zig file: what Zig finds without building
//! -- a syntax error, an unused local, a `var` never changed, a name
//! shadowed -- in a few milliseconds, shown with ZLS's problems. It prints
//! them on stderr as compilers do (`file:line:col: error: message`, a
//! `note:` after), each followed by the line and a caret, and exits 1.

use fenix_extension_api::{Command, Problem, Severity};
use fenix_lang_kit::{compiler_lines, Linter, Output};

/// `zig ast-check` for a saved file.
pub fn ast_check(zig: &Command) -> Linter {
    let zig = zig.clone();
    Linter::new("zig ast-check", &["zig", "zon"], move |file| zig.clone().args(["ast-check", file]), parse)
}

/// Its problems in `file`; a failure that names no place (the file isn't
/// there) is an error of the linter's.
pub fn parse(out: &Output, file: &str) -> Result<Vec<Problem>, String> {
    let text = format!("{}\n{}", out.stderr, out.stdout);
    let problems: Vec<Problem> = compiler_lines(&text, file, Severity::Error).into_iter().map(|p| p.source("zig")).collect();
    if problems.is_empty() && !out.ok() && !text.contains(": error: ") {
        return Err(out.why());
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    fn out(code: i32, stderr: &str) -> Output {
        Output { code: Some(code), stdout: String::new(), stderr: stderr.into() }
    }

    #[test]
    fn ast_checks_errors_and_notes_are_problems() {
        let p = parse(&out(1, include_str!("samples/ast-check.txt")), "src/main.zig").unwrap();
        let rows: Vec<(Severity, (u32, u32), &str)> = p.iter().map(|p| (p.severity, p.start, p.message.as_str())).collect();
        assert_eq!(
            rows,
            [
                (Severity::Error, (6, 4), "local variable is never mutated"),
                (Severity::Hint, (6, 4), "consider using 'const'"),
                (Severity::Error, (11, 19), "expected ';' after statement"),
            ],
            "the source lines and carets, and another file's, left out"
        );
        assert!(p.iter().all(|p| p.source.as_deref() == Some("zig")));
        assert_eq!(parse(&out(0, ""), "src/main.zig").unwrap(), Vec::new());
        assert!(parse(&out(1, "error: unable to load 'src/gone.zig': FileNotFound"), "src/gone.zig").is_err());
    }

    #[test]
    fn ast_check_runs_on_the_zig_file_saved() {
        let l = ast_check(&Command::new("/t/zig"));
        assert_eq!(line(&(l.command)("src/main.zig")), "/t/zig ast-check src/main.zig");
        assert!(l.takes("src/main.zig") && l.takes("build.zig.zon") && !l.takes("main.c"));
    }
}
