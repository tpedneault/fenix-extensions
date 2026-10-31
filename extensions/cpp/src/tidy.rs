//! clang-tidy, run on a saved source file with the build's
//! `compile_commands.json` (`-p`), its problems shown with clangd's, each
//! with the fix clang-tidy has for it (`--export-fixes`), which `SPC c a`
//! offers (0.8.6); and clang-format as the formatter Fenix runs, the
//! file's `.clang-format` found from its path.

use std::cell::RefCell;
use std::rc::Rc;

use fenix_extension_api::{Command, Problem, QuickFix, Severity, TextEdit};
use fenix_lang_kit::{compiler_lines, Linter, Output};

/// The problems with fixes clang-tidy last gave, by file (in the
/// project), for the pack to set: lang-kit's linter sets problems alone.
pub type Fixes = Rc<RefCell<Vec<(String, Vec<Problem>)>>>;

/// The files clang-tidy is run on: sources, which the compilation
/// database has. A header has no command of its own there.
pub const SOURCES: &[&str] = &["c", "cc", "cpp", "cxx", "c++", "cppm", "ixx"];

/// What Fenix formats C and C++ with: the buffer on stdin, styled as the
/// `.clang-format` above `${file}` says (LLVM's when there's none).
pub fn formatter(clang_format: &str) -> Command {
    Command::new(clang_format).arg("--assume-filename=${file}")
}

/// Where clang-tidy writes its fixes for `file`: beside the compilation
/// database, in the build's folder.
pub fn fixes_file(database: &str, file: &str) -> String {
    format!("{database}/fenix-tidy-{}.yaml", file.replace(['/', '\\', ':'], "_"))
}

/// clang-tidy on a saved file, with the compilation database in
/// `database` (relative to the project, which is `root`); `--quiet`
/// leaves out its counts of what it suppressed. Its fixes, read once it
/// has said its problems, go to `fixes`.
pub fn linter(clang_tidy: &str, database: &str, root: &str, fixes: Fixes) -> Linter {
    let (tidy, db, root) = (clang_tidy.to_string(), database.to_string(), root.to_string());
    let db2 = db.clone();
    Linter::new(
        "clang-tidy",
        SOURCES,
        move |file| Command::new(&tidy).args(["-p", &db, "--quiet", &format!("--export-fixes={}", fixes_file(&db, file)), file]),
        move |out, file| {
            let problems = parse(out, file)?;
            let path = std::path::Path::new(&root).join(fixes_file(&db2, file));
            if let Ok(yaml) = std::fs::read_to_string(&path) {
                let _ = std::fs::remove_file(&path);
                let text = std::fs::read_to_string(std::path::Path::new(&root).join(file)).unwrap_or_default();
                let found = with_fixes(&problems, &read_fixes(&yaml, file), &text);
                if found.iter().any(|p| !p.fixes.is_empty()) {
                    fixes.borrow_mut().push((file.to_string(), found));
                }
            }
            Ok(problems)
        },
    )
}

/// A fix clang-tidy has for one of its problems: the check, where it
/// is (a byte offset in the file), and its edits (offset, length, new
/// text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TidyFix {
    pub check: String,
    pub offset: usize,
    pub edits: Vec<(usize, usize, String)>,
}

/// A YAML scalar as LLVM writes one: plain, `'...'` (`''` a quote) or
/// `"..."` (with `\n` and the like).
fn scalar(value: &str) -> String {
    let v = value.trim();
    if let Some(inner) = v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return inner.replace("''", "'");
    }
    let Some(inner) = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else { return v.to_string() };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                out.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// The fixes in what `--export-fixes` wrote, for `file` (in the project):
/// each diagnostic's own replacements in that file. Its notes' and its
/// ranges aren't fixes.
pub fn read_fixes(yaml: &str, file: &str) -> Vec<TidyFix> {
    #[derive(PartialEq)]
    enum In {
        Other,
        Message,
        Replacements,
    }
    let same = |path: &str| {
        let (a, b) = (path.replace('\\', "/"), file.replace('\\', "/"));
        let (a, b) = (a.trim_start_matches("./"), b.trim_start_matches("./"));
        a == b || a.ends_with(&format!("/{b}"))
    };
    let mut out = Vec::new();
    // The diagnostic read so far: its fix, and whether it's this file's.
    let mut diag: Option<(TidyFix, bool)> = None;
    let mut edit: Option<(String, usize, usize, String)> = None;
    let mut at = In::Other;
    let finish_edit = |diag: &mut Option<(TidyFix, bool)>, edit: &mut Option<(String, usize, usize, String)>| {
        if let (Some((fix, _)), Some((path, offset, length, text))) = (diag.as_mut(), edit.take()) {
            if same(&path) {
                fix.edits.push((offset, length, text));
            }
        }
    };
    for line in yaml.lines() {
        let indent = line.len() - line.trim_start().len();
        let mut body = line.trim();
        let item = body.starts_with("- ");
        if item {
            body = body[2..].trim_start();
        }
        let (key, value) = body.split_once(':').map(|(k, v)| (k.trim(), v.trim())).unwrap_or((body, ""));
        if item && indent == 2 && key == "DiagnosticName" {
            finish_edit(&mut diag, &mut edit);
            out.extend(diag.take().filter(|(f, mine)| *mine && !f.edits.is_empty()).map(|(f, _)| f));
            diag = Some((TidyFix { check: scalar(value), offset: 0, edits: Vec::new() }, false));
            at = In::Other;
            continue;
        }
        match indent {
            4 => {
                finish_edit(&mut diag, &mut edit);
                at = if key == "DiagnosticMessage" { In::Message } else { In::Other };
            }
            6 if at != In::Other => {
                finish_edit(&mut diag, &mut edit);
                at = In::Message;
                if let Some((fix, mine)) = diag.as_mut() {
                    match key {
                        "FilePath" => *mine = same(&scalar(value)),
                        "FileOffset" => fix.offset = value.parse().unwrap_or(0),
                        "Replacements" => at = In::Replacements,
                        _ => {}
                    }
                }
            }
            8 | 10 if at == In::Replacements => {
                if item && indent == 8 {
                    finish_edit(&mut diag, &mut edit);
                    edit = Some((String::new(), 0, 0, String::new()));
                }
                if let Some(e) = edit.as_mut() {
                    match key {
                        "FilePath" => e.0 = scalar(value),
                        "Offset" => e.1 = value.parse().unwrap_or(0),
                        "Length" => e.2 = value.parse().unwrap_or(0),
                        "ReplacementText" => e.3 = scalar(value),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    finish_edit(&mut diag, &mut edit);
    out.extend(diag.take().filter(|(f, mine)| *mine && !f.edits.is_empty()).map(|(f, _)| f));
    out
}

/// Byte `offset` in `text` as (line, column), from 0, in characters.
fn place(text: &str, offset: usize) -> (u32, u32) {
    let before = &text[..text.floor_char_boundary(offset.min(text.len()))];
    let line = before.matches('\n').count() as u32;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() as u32;
    (line, column)
}

/// `problems`, each with the fix clang-tidy has for it: the one for its
/// check on its line, the file's text being `text`.
pub fn with_fixes(problems: &[Problem], fixes: &[TidyFix], text: &str) -> Vec<Problem> {
    let mut used = vec![false; fixes.len()];
    problems
        .iter()
        .map(|p| {
            let mut ex = p.clone();
            let found = fixes.iter().enumerate().find(|(i, f)| !used[*i] && p.code.as_deref() == Some(f.check.as_str()) && place(text, f.offset).0 == p.start.0);
            if let Some((i, f)) = found {
                used[i] = true;
                let edits = f.edits.iter().map(|(offset, length, new)| TextEdit { start: place(text, *offset), end: place(text, offset + length), text: new.clone() }).collect();
                ex.fixes.push(QuickFix { title: format!("Fix it as {} says", f.check), edits });
            }
            ex
        })
        .collect()
}

/// clang-tidy's lines (`file:line:col: warning: message [check]`), as
/// compilers print them, for `file`: its notes as hints, and a check
/// made an error by `WarningsAsErrors` under its own name. It exits 1
/// when there's an error, and says why on stderr when it couldn't run.
pub fn parse(out: &Output, file: &str) -> Result<Vec<Problem>, String> {
    let mut problems = compiler_lines(&out.stdout, file, Severity::Warning);
    for p in &mut problems {
        // `fatal error: ...`: lang-kit reads `error` as a code.
        if p.code.as_deref() == Some("error") {
            p.code = None;
            if let Some((message, code)) = p.message.strip_suffix(']').and_then(|m| m.rsplit_once(" [")) {
                if !code.contains(' ') {
                    p.code = Some(code.to_string());
                    p.message = message.to_string();
                }
            }
        }
        if let Some(code) = p.code.as_mut() {
            if let Some(c) = code.strip_suffix(",-warnings-as-errors") {
                *code = c.to_string();
            }
        }
    }
    if problems.is_empty() && !out.ok() {
        return Err(out.why());
    }
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    const ORBIT: &str = include_str!("samples/tidy-orbit.txt");
    const BROKEN: &str = include_str!("samples/tidy-broken.txt");

    type Row<'a> = ((u32, u32), Severity, Option<&'a str>, &'a str);

    fn rows(problems: &[Problem]) -> Vec<Row<'_>> {
        problems.iter().map(|p| (p.start, p.severity, p.code.as_deref(), p.message.as_str())).collect()
    }

    #[test]
    fn clang_tidys_problems_are_read_from_its_lines() {
        let out = Output { code: Some(0), stdout: ORBIT.into(), stderr: "17527 warnings generated.\n".into() };
        let problems = parse(&out, "src/orbit.cpp").unwrap();
        let rows = rows(&problems);
        assert_eq!(rows.len(), 7, "the code it quotes isn't a problem: {rows:?}");
        assert_eq!(rows[0], ((6, 4), Severity::Warning, Some("cppcoreguidelines-owning-memory"), "initializing non-owner 'int *' with a newly created 'gsl::owner<>'"));
        assert_eq!(rows[1].2, Some("clang-analyzer-security.insecureAPI.strcpy"));
        assert_eq!((rows[2].1, rows[2].2), (Severity::Hint, None), "a note");
        assert_eq!(rows[3].2, Some("clang-diagnostic-fortify-source"));
        assert_eq!(rows[4], ((9, 4), Severity::Warning, Some("clang-analyzer-cplusplus.NewDeleteLeaks"), "Potential leak of memory pointed to by 'p'"));
        assert!(parse(&out, "src/main.cpp").unwrap().is_empty(), "another file's");
    }

    #[test]
    fn an_error_and_a_check_made_one_keep_their_names() {
        let out = Output { code: Some(1), stdout: BROKEN.into(), stderr: "764 warnings and 1 error generated.\nError while processing /home/user/work/cpp-pack/sample/src/broken.cpp.\n".into() };
        let problems = parse(&out, "src/broken.cpp").unwrap();
        assert_eq!(
            rows(&problems),
            [
                ((0, 9), Severity::Error, Some("clang-diagnostic-error"), "'missing.hpp' file not found"),
                ((3, 4), Severity::Error, Some("bugprone-not-null-terminated-result"), "the result from calling 'memcpy' is not null-terminated"),
            ]
        );
        let fatal = Output { code: Some(1), stdout: "src/a.c:1:10: fatal error: 'x.h' file not found [clang-diagnostic-error]\n".into(), stderr: String::new() };
        assert_eq!(rows(&parse(&fatal, "src/a.c").unwrap()), [((0, 9), Severity::Error, Some("clang-diagnostic-error"), "'x.h' file not found")]);
        let failed = Output { code: Some(1), stdout: String::new(), stderr: "Error: no checks enabled.\n".into() };
        assert_eq!(parse(&failed, "a.c").unwrap_err(), "Error: no checks enabled.");
    }

    #[test]
    fn the_linter_and_formatter_are_clangs() {
        let l = linter("/usr/bin/clang-tidy", "build/debug", "/p", Fixes::default());
        assert_eq!(line(&(l.command)("src/orbit.cpp")), "/usr/bin/clang-tidy -p build/debug --quiet --export-fixes=build/debug/fenix-tidy-src_orbit.cpp.yaml src/orbit.cpp");
        assert!(l.takes("a.cc") && l.takes("m.cppm") && !l.takes("a.hpp"));
        assert_eq!(line(&formatter("clang-format")), "clang-format --assume-filename=${file}");
    }

    #[test]
    fn its_fixes_are_read_and_given_to_their_problems() {
        let fixes = read_fixes(include_str!("samples/tidy-fixes.yaml"), "src/orbit.cpp");
        assert_eq!(fixes.iter().map(|f| f.check.as_str()).collect::<Vec<_>>(), ["modernize-use-override", "modernize-use-nullptr", "readability-braces-around-statements"]);
        assert_eq!(fixes[0].edits, [(139, 8, String::new()), (165, 0, " override".into())]);
        assert_eq!(fixes[2].edits[1], (232, 0, "\n}".into()), "a double-quoted scalar");
        assert!(read_fixes(include_str!("samples/tidy-fixes.yaml"), "src/main.cpp").is_empty(), "another file's");

        let text = include_str!("samples/tidy-fixes.cpp");
        let out = Output { code: Some(0), stdout: include_str!("samples/tidy-fixes.txt").into(), stderr: "3 warnings generated.\n".into() };
        let problems = parse(&out, "src/orbit.cpp").unwrap();
        let ex = with_fixes(&problems, &fixes, text);
        assert_eq!(ex.len(), 3);
        let nullptr = &ex[1];
        assert_eq!((nullptr.start, nullptr.fixes.len()), ((12, 13), 1));
        assert_eq!(nullptr.fixes[0].title, "Fix it as modernize-use-nullptr says");
        assert_eq!(nullptr.fixes[0].edits, [TextEdit { start: (12, 13), end: (12, 17), text: "nullptr".into() }]);
        assert_eq!(ex[0].fixes[0].edits[0], TextEdit { start: (8, 4), end: (8, 12), text: String::new() }, "virtual taken off");
    }

    #[test]
    fn notes_and_ranges_arent_fixes() {
        let fixes = read_fixes(include_str!("samples/tidy-notes.yaml"), "src/leak.cpp");
        assert_eq!(fixes.len(), 1, "the leak has none: {fixes:?}");
        assert_eq!((fixes[0].check.as_str(), fixes[0].offset, fixes[0].edits.len()), ("readability-braces-around-statements", 72, 2));
        assert_eq!(scalar("'Assuming ''radius'' is > 2'"), "Assuming 'radius' is > 2");
        assert_eq!(scalar("\"a\\tb\\x41\""), "a\tbA");
        assert_eq!(place("ab\nçd", 5), (1, 1), "in characters");
    }

    #[test]
    fn the_linter_reads_its_fixes_and_leaves_none_behind() {
        let dir = crate::project::tests::temp("tidy-fixes");
        let root = dir.to_string_lossy().into_owned();
        std::fs::create_dir_all(dir.join("build/debug")).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/orbit.cpp"), include_str!("samples/tidy-fixes.cpp")).unwrap();
        let yaml = dir.join(fixes_file("build/debug", "src/orbit.cpp"));
        std::fs::write(&yaml, include_str!("samples/tidy-fixes.yaml")).unwrap();
        let fixes = Fixes::default();
        let l = linter("clang-tidy", "build/debug", &root, fixes.clone());
        let out = Output { code: Some(0), stdout: include_str!("samples/tidy-fixes.txt").into(), stderr: String::new() };
        assert_eq!((l.parse)(&out, "src/orbit.cpp").unwrap().len(), 3);
        assert!(!yaml.exists());
        let got = fixes.borrow_mut().pop().unwrap();
        assert_eq!((got.0.as_str(), got.1.iter().filter(|p| !p.fixes.is_empty()).count()), ("src/orbit.cpp", 3));
        // No file: no fixes, the problems still.
        assert_eq!((l.parse)(&out, "src/orbit.cpp").unwrap().len(), 3);
        assert!(fixes.borrow().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
