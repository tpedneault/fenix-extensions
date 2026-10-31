//! clippy on save: `cargo clippy --message-format json` on the project,
//! its lints shown with rust-analyzer's problems, and its suggestions as
//! their fixes (`SPC c a`). rust-analyzer already shows rustc's own errors
//! and warnings (its `cargo check` on save), so only clippy's lints
//! (`clippy::...`) are taken, and none is shown twice.
//!
//! clippy lints a crate, not a file: lang-kit's `Linter::across` sets the
//! problems of every file it found some in, and takes them off the files
//! it found none in since.

use std::collections::BTreeMap;

use fenix_extension_api::{Command, Problem, Severity, TextEdit};
use fenix_lang_kit::lint::from_one;
use fenix_lang_kit::{serde_json, Files, Linter, Output};
use serde::Deserialize;

use crate::PROJECT;

/// The linter's name, and the setting that turns it off.
pub const NAME: &str = "clippy";

#[derive(Deserialize)]
struct Line {
    reason: String,
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    message: String,
    #[serde(default)]
    code: Option<Code>,
    level: String,
    #[serde(default)]
    spans: Vec<Span>,
    #[serde(default)]
    children: Vec<Message>,
}

#[derive(Deserialize)]
struct Code {
    code: String,
}

#[derive(Deserialize)]
struct Span {
    file_name: String,
    line_start: u32,
    column_start: u32,
    line_end: u32,
    column_end: u32,
    is_primary: bool,
    #[serde(default)]
    suggested_replacement: Option<String>,
    #[serde(default)]
    suggestion_applicability: Option<String>,
}

/// A help's suggestions in `file` as a fix: those clippy is sure of, and
/// those it may have wrong ("check it"); none for one with placeholders.
fn fix(help: &Message, file: &str) -> Option<(String, Vec<TextEdit>)> {
    let spans: Vec<&Span> = help.spans.iter().filter(|s| s.suggested_replacement.is_some()).collect();
    if spans.is_empty() || spans.iter().any(|s| s.file_name != file) {
        return None;
    }
    let sure = match spans.iter().filter_map(|s| s.suggestion_applicability.as_deref()).next() {
        Some("MachineApplicable") => true,
        Some("MaybeIncorrect") => false,
        _ => return None,
    };
    let edits = spans.iter().map(|s| TextEdit::new(from_one(s.line_start, s.column_start), from_one(s.line_end, s.column_end), s.suggested_replacement.clone().unwrap_or_default())).collect();
    let title = if sure { format!("clippy: {}", help.message) } else { format!("clippy: {} (check it)", help.message) };
    Some((title, edits))
}

/// clippy's lints in what `cargo clippy --message-format json` printed,
/// by file (as cargo says it: from the workspace's root), each once, with
/// the fixes its suggestions make.
pub fn parse(stdout: &str) -> BTreeMap<String, Vec<Problem>> {
    let mut out: BTreeMap<String, Vec<Problem>> = BTreeMap::new();
    for line in stdout.lines().filter(|l| l.starts_with('{')) {
        let Ok(Line { reason, message: Some(m) }) = serde_json::from_str::<Line>(line) else { continue };
        let Some(code) = m.code.as_ref().map(|c| c.code.clone()).filter(|c| c.starts_with("clippy::")) else { continue };
        if reason != "compiler-message" {
            continue;
        }
        let Some(span) = m.spans.iter().find(|s| s.is_primary) else { continue };
        let severity = match m.level.as_str() {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            _ => Severity::Info,
        };
        // What it suggests, after what it says.
        let help: Vec<&Message> = m.children.iter().filter(|c| c.level == "help" && !c.message.starts_with("for further information")).collect();
        let said: Vec<&str> = help.iter().map(|c| c.message.as_str()).collect();
        let text = if said.is_empty() { m.message.clone() } else { format!("{} (help: {})", m.message, said.join("; ")) };
        let mut p = Problem::new(severity, from_one(span.line_start, span.column_start), from_one(span.line_end, span.column_end), text);
        p.code = Some(code);
        let mut p = p;
        for h in help {
            if let Some((title, edits)) = fix(h, &span.file_name) {
                p = p.fix(&title, edits);
            }
        }
        let list = out.entry(span.file_name.replace('\\', "/")).or_default();
        // A crate's lib and its tests are linted apart: the same lint twice.
        if !list.iter().any(|q| q.start == p.start && q.end == p.end && q.code == p.code) {
            list.push(p);
        }
    }
    out
}

/// Where Fenix takes a file's problems: `/project/...` when it's in the
/// project, else its full path (shown while it's open).
pub fn place(file: &str, workspace_root: &str, project_root: &str) -> String {
    let full = if file.starts_with('/') || file.get(1..3) == Some(":/") { file.to_string() } else { format!("{}/{file}", workspace_root.replace('\\', "/").trim_end_matches('/')) };
    match fenix_lang_kit::lint::relative(&full, project_root) {
        Some(rel) => format!("{PROJECT}/{rel}"),
        None => full,
    }
}

/// `cargo clippy` for the whole project: every target, and every package
/// of a workspace.
pub fn command(cargo: Command, workspace: bool) -> Command {
    let cmd = cargo.args(["clippy", "--message-format", "json", "--all-targets"]);
    let cmd = if workspace { cmd.arg("--workspace") } else { cmd };
    cmd.cwd(PROJECT)
}

/// What a run found, file by file, placed in the project. A run that
/// didn't get as far as linting (a Cargo.toml that doesn't read) is an
/// error, so what was shown stays.
pub fn files(out: &Output, workspace_root: &str, project_root: &str) -> Result<Files, String> {
    let found = parse(&out.stdout);
    if found.is_empty() && !out.ok() && !out.stdout.contains("\"reason\":\"compiler-message\"") {
        return Err(crate::doctor::error_line(out));
    }
    Ok(found.into_iter().map(|(file, problems)| (place(&file, workspace_root, project_root), problems)).collect())
}

/// clippy as lang-kit's linter, for the crate whose files are under
/// `project_root` in the workspace at `workspace_root` (both full paths,
/// as cargo gives them; the second is the first when the project is the
/// workspace).
pub fn linter(cargo: Command, workspace: bool, workspace_root: &str, project_root: &str) -> Linter {
    let cmd = command(cargo, workspace);
    let (w, p) = (workspace_root.to_string(), project_root.to_string());
    Linter::across(NAME, &["rs"], move |_| cmd.clone(), move |out, _| files(out, &w, &p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_extension_api::{Event, ProcessOutput};
    use fenix_lang_kit::host::mock::Mock;
    use fenix_lang_kit::Lint;

    const CLIPPY: &str = include_str!("samples/cargo-clippy.json");

    #[test]
    fn clippys_lints_are_read_by_file_once_each() {
        let found = parse(CLIPPY);
        assert_eq!(found.keys().collect::<Vec<_>>(), ["orbit-core/src/lib.rs", "orbit-core/src/maths.rs"]);
        let maths: Vec<_> = found["orbit-core/src/maths.rs"].iter().map(|p| (p.start, p.end, p.severity, p.code.as_deref().unwrap())).collect();
        assert_eq!(
            maths,
            [
                ((15, 18), (15, 27), Severity::Warning, "clippy::if_same_then_else"),
                ((15, 7), (15, 17), Severity::Warning, "clippy::manual_is_multiple_of"),
                ((18, 19), (18, 27), Severity::Warning, "clippy::ptr_arg"),
                ((5, 0), (5, 9), Severity::Warning, "clippy::items_after_test_module"),
            ],
            "rustc's unused variable is rust-analyzer's to show; the lib's test build said them all again"
        );
        let ret = &found["orbit-core/src/lib.rs"][0];
        assert_eq!(ret.message, "unneeded `return` statement (help: remove `return`)");
        assert_eq!((ret.start, ret.end), ((4, 4), (4, 14)));
        // Its suggestions, as fixes: one clippy is sure of; none for one it
        // isn't (`&[u8]`, Unspecified).
        assert_eq!(ret.fixes.iter().map(|f| f.title.as_str()).collect::<Vec<_>>(), ["clippy: remove `return`"]);
        let multiple = &found["orbit-core/src/maths.rs"][1];
        let e = &multiple.fixes[0].edits[0];
        assert_eq!((e.start, e.end, e.text.as_str()), ((15, 7), (15, 17), "x.is_multiple_of(2)"));
        assert!(found["orbit-core/src/maths.rs"][2].fixes.is_empty(), "ptr_arg's isn't sure");
    }

    #[test]
    fn a_file_is_placed_in_the_project_or_by_its_full_path() {
        assert_eq!(place("orbit-core/src/lib.rs", "/home/u/orbit", "/home/u/orbit"), "/project/orbit-core/src/lib.rs");
        assert_eq!(place("orbit-core/src/lib.rs", "/home/u/orbit", "/home/u/orbit/orbit-core"), "/project/src/lib.rs", "a member opened on its own");
        assert_eq!(place("/home/u/vendor/x/src/lib.rs", "/home/u/orbit", "/home/u/orbit"), "/home/u/vendor/x/src/lib.rs");
        assert_eq!(place("src\\main.rs", "C:\\work\\orbit", "C:\\work\\orbit"), "/project/src/main.rs");
    }

    #[test]
    fn a_save_lints_the_project_and_clears_what_was_fixed() {
        let mut host = Mock::default();
        let mut lint = Lint::new();
        lint.add(linter(Command::new("cargo"), true, "/home/u/orbit", "/home/u/orbit"));
        lint.file_saved(&mut host, "/project/Cargo.toml");
        assert!(host.take().is_empty(), "only a Rust file");
        lint.file_saved(&mut host, "/project/orbit-core/src/lib.rs");
        assert_eq!(host.take(), ["spawn 1 cargo clippy --message-format json --all-targets --workspace", "close 1"]);
        for line in CLIPPY.lines() {
            assert!(lint.handle(&mut host, &Event::ProcessOutput(ProcessOutput { process: 1, stderr: false, line: line.into() })));
        }
        assert!(lint.handle(&mut host, &Event::ProcessExit((1, Some(0)))));
        assert_eq!(host.take(), ["problems /project/orbit-core/src/lib.rs 1 (1 fixes)", "problems /project/orbit-core/src/maths.rs 4 (2 fixes)"]);
        assert_eq!(host.problems["/project/orbit-core/src/lib.rs"][0].code.as_deref(), Some("clippy::needless_return"));
        assert_eq!(host.problems["/project/orbit-core/src/lib.rs"][0].source.as_deref(), Some("clippy"));

        // lib.rs is clean now: its problems taken off.
        lint.file_saved(&mut host, "/project/orbit-core/src/lib.rs");
        host.take();
        let maths = CLIPPY.lines().filter(|l| !l.contains("needless_return")).collect::<Vec<_>>().join("\n");
        lint.handle(&mut host, &Event::ProcessOutput(ProcessOutput { process: 2, stderr: false, line: maths }));
        lint.handle(&mut host, &Event::ProcessExit((2, Some(0))));
        let mut calls = host.take();
        calls.sort();
        assert_eq!(calls, ["problems /project/orbit-core/src/lib.rs 0", "problems /project/orbit-core/src/maths.rs 4 (2 fixes)"]);

        // A Cargo.toml that doesn't read: nothing taken off.
        lint.file_saved(&mut host, "/project/src/main.rs");
        host.take();
        lint.handle(&mut host, &Event::ProcessOutput(ProcessOutput { process: 3, stderr: true, line: "error: failed to parse manifest at `/home/u/orbit/Cargo.toml`".into() }));
        lint.handle(&mut host, &Event::ProcessExit((3, Some(101))));
        assert_eq!(host.take(), ["log clippy on src/main.rs: error: failed to parse manifest at `/home/u/orbit/Cargo.toml` (error: failed to parse manifest at `/home/u/orbit/Cargo.toml`)"]);
        // Off: what it showed taken off.
        lint.remove(&mut host, NAME);
        assert_eq!(host.take(), ["problems /project/orbit-core/src/maths.rs 0"]);
    }
}
