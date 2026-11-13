//! What cargo's and nextest's readers share: a panic's place and
//! message, a failure's text, a test no scan found placed under its
//! module or its template, and what a reader is told of its step.

use fenix_lang_kit::tests::{At, Ended, Found, Group, Outcome, Report};

use super::index::Shared;
use super::layout::{doc_place, Layout, TargetKey};

/// What a step's reader reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// `--list`: the tests there are.
    Listing,
    /// A run.
    Run,
    /// `--no-run` before debugging: the binaries' paths.
    Build,
}

/// What a reader knows of its step.
pub struct Ctx {
    pub layout: Layout,
    pub index: Shared,
    pub purpose: Purpose,
    /// The package a step of one package runs (`-p orbit`): what a
    /// binary's header says without its package.
    pub package: Option<String>,
    /// The paths a step named exactly, by target id: nextest reports an
    /// ignored test it wasn't asked for.
    pub exact: Option<Vec<(String, String)>>,
}

/// `thread 'tests::adds' (9760) panicked at src/lib.rs:25:9:` (Rust 1.73
/// and later; the thread's id since 1.90): the thread's name and the place.
pub fn panic_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start().strip_prefix("thread '")?;
    let (name, rest) = rest.split_once('\'')?;
    let rest = rest.trim_start();
    let rest = match rest.strip_prefix('(') {
        Some(r) => r.split_once(')')?.1.trim_start(),
        None => rest,
    };
    let place = rest.strip_prefix("panicked at ")?.trim_end().strip_suffix(':')?;
    Some((name, place))
}

/// `src/lib.rs:25:9` (or `src\lib.rs:25:9`, `C:\x\lib.rs:25:9`) as a
/// place below the project: read from its end, so a drive's colon stays.
pub fn place(text: &str, layout: &Layout) -> Option<At> {
    let mut parts = text.trim().rsplitn(3, ':');
    let (column, line, file) = (parts.next()?, parts.next()?, parts.next()?);
    let (line, column) = (line.parse().ok()?, column.parse().ok());
    (!file.is_empty()).then(|| At::new(layout.place(file), line, column))
}

/// A failed test's result from what libtest captured of it (its
/// `---- name stdout ----` section, or nextest's `stdout`): the panic's
/// place and the line after it as its message, all of it as its text; and
/// what it printed before the panic, for its output.
pub fn failure(key: &str, lines: &[&str], layout: &Layout) -> (Ended, Option<String>) {
    let mut e = Ended::new(key, Outcome::Failed);
    let mut printed = None;
    if let Some(at) = lines.iter().position(|l| panic_line(l).is_some()) {
        let (_, where_) = panic_line(lines[at]).expect("found");
        e.at = place(where_, layout);
        e.message = lines[at + 1..].iter().map(|l| l.trim()).find(|l| !l.is_empty()).map(str::to_string);
        let before: Vec<&str> = lines[..at].iter().copied().skip_while(|l| l.trim().is_empty()).collect();
        let before = before.join("\n");
        let before = before.trim_end();
        if !before.is_empty() {
            printed = Some(before.to_string());
        }
    } else if let Some(l) = lines.iter().map(|l| l.trim()).find(|l| !l.is_empty()) {
        // `note: test did not panic as expected`, a doc test that didn't
        // compile.
        e.message = Some(l.to_string());
    }
    let text = lines.join("\n");
    let text = text.trim_matches('\n');
    if !text.is_empty() {
        e.text = Some(text.to_string());
    }
    (e, printed)
}

/// A test a listing or a run gave that no scan found: under its template
/// (`tests::sums::case_1`, a case), else the deepest module a scan found
/// it in, else its target alone; a doc test under its file, at its line.
/// `None` for one a scan found: lang-kit has its groups and place.
pub fn found(ctx: &Ctx, target: &TargetKey, path: &str) -> Option<Found> {
    let id = target.id();
    let index = ctx.index.borrow();
    if index.scanned_test(&id, path) {
        return None;
    }
    let key = target.key(path);
    if target.is_doc() {
        let (file, line) = doc_place(path)?;
        let file = ctx.layout.place(file);
        let label = path.split_once(" - ").map(|(_, l)| l).unwrap_or(path);
        return Some(Found::test(key, label).under([target.group(), Group::file(file.clone())]).at(file, line));
    }
    if let Some(template) = index.template_of(&id, path) {
        let groups = index.template_groups(&id, &template).unwrap_or_else(|| vec![target.group()]);
        let label = &path[template.len() + 2..];
        return Some(Found::case(key, label, target.key(&template)).under(groups));
    }
    let groups = index.groups_of(&id, path).unwrap_or_else(|| vec![target.group()]);
    let label = path.rsplit("::").next().unwrap_or(path);
    Some(Found::test(key, label).under(groups))
}

/// `found`'s report, when there's one to make.
pub fn found_report(ctx: &Ctx, target: &TargetKey, path: &str, out: &mut Vec<Report>) {
    if let Some(f) = found(ctx, target, path) {
        out.push(Report::Found(f));
    }
}

/// A run's result for `key`: what a test printed first, then its result.
pub fn ended(key: &str, printed: Option<String>, e: Ended, out: &mut Vec<Report>) {
    if let Some(text) = printed {
        out.push(Report::Output { key: Some(key.into()), text });
    }
    out.push(Report::Ended(e));
}

/// `(signal: 6, SIGABRT: process abort signal)` as "its binary crashed:
/// SIGABRT"; `(exit status: 1)` as "its binary exited with 1"; Windows'
/// `(exit code: 0xc0000005, STATUS_ACCESS_VIOLATION)` by its name. `None`
/// for 101, libtest's own failure.
pub fn crash_message(line: &str) -> Option<String> {
    let why = line.rsplit_once('(')?.1.trim_end().strip_suffix(')')?;
    if let Some(rest) = why.strip_prefix("signal: ") {
        let name = rest.split(',').nth(1).and_then(|n| n.trim().split(':').next()).map(str::trim).filter(|n| !n.is_empty());
        return Some(format!("its binary crashed: {}", name.unwrap_or(rest)));
    }
    let code = why.strip_prefix("exit status: ").or_else(|| why.strip_prefix("exit code: "))?;
    if code.trim() == "101" {
        return None;
    }
    match code.split_once(", ") {
        Some((_, name)) => Some(format!("its binary crashed: {}", name.trim())),
        None => Some(format!("its binary exited with {} before its tests ended", code.trim())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panics_and_places_as_rust_prints_them() {
        assert_eq!(panic_line("thread 'tests::fails_on_purpose' (9760) panicked at src/lib.rs:25:9:"), Some(("tests::fails_on_purpose", "src/lib.rs:25:9")));
        assert_eq!(panic_line("thread 'main' panicked at tests/custom.rs:4:5:"), Some(("main", "tests/custom.rs:4:5")), "before Rust 1.90: no id");
        assert_eq!(panic_line("    thread 'x' (1) panicked at src\\lib.rs:2:3:"), Some(("x", "src\\lib.rs:2:3")));
        assert_eq!(panic_line("thread 'x' has overflowed its stack"), None);
        let l = Layout::default();
        assert_eq!(place("src\\lib.rs:25:9", &l), Some(At::new("src/lib.rs", 25, Some(9))), "Windows' paths by hand");
        assert_eq!(place("C:\\work\\orbit\\src\\lib.rs:25:9", &l), Some(At::new("C:/work/orbit/src/lib.rs", 25, Some(9))));
        assert_eq!(place("nowhere", &l), None);
    }

    #[test]
    fn a_binarys_end_says_whether_it_crashed() {
        assert_eq!(crash_message("process didn't exit successfully: `/p/crash-9b9` (signal: 6, SIGABRT: process abort signal)").as_deref(), Some("its binary crashed: SIGABRT"));
        assert_eq!(crash_message("process didn't exit successfully: `C:\\p\\crash.exe` (exit code: 0xc0000005, STATUS_ACCESS_VIOLATION)").as_deref(), Some("its binary crashed: STATUS_ACCESS_VIOLATION"));
        assert_eq!(crash_message("process didn't exit successfully: `/p/x` (exit status: 3)").as_deref(), Some("its binary exited with 3 before its tests ended"));
        assert_eq!(crash_message("process didn't exit successfully: `/p/x` (exit status: 101)"), None);
    }
}
