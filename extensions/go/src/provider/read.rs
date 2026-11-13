//! What the readers share: a step's context, the lines `go` prints for a
//! test (`file_test.go:37: got -2, want -3`), a panic's message and its
//! first frame in the project, and a failure's text.

use std::rc::Rc;

use fenix_lang_kit::tests::{Covered, Found};

use super::index::Shared;
use super::layout::{key, package_group};

/// What a reader knows of its step.
pub struct Ctx {
    pub index: Shared,
    /// The folder the step ran in, below the project (`""` for the top):
    /// where a compiler's relative paths start.
    pub cwd: String,
    /// What the step claims to run: empty is all of the ask.
    pub covers: Vec<Covered>,
}

impl Ctx {
    /// Whether the step runs `key` of `file` in `package`: it names it, or
    /// its package or file, or it claims nothing in particular.
    pub fn runs(&self, package: &str, name: &str, file: &str) -> bool {
        self.covers.is_empty()
            || self.covers.iter().any(|c| match c {
                Covered::Test(k) => k.0 == key(package, name),
                Covered::Group(labels) => labels.first().is_some_and(|p| p == package) && labels.get(1).is_none_or(|f| f == file),
            })
    }
}

pub type Context = Rc<Ctx>;

/// `file.go:37: text` or `file.go:37:5: text` (a compiler's, a vet's) as
/// its file, line, column and text. The file is whatever precedes `.go:`,
/// so a path with spaces or a drive's colon is whole.
pub fn file_line(line: &str) -> Option<(&str, u32, Option<u32>, &str)> {
    let line = line.trim_start();
    let mut from = 0;
    while let Some(at) = line.get(from..)?.find(".go:") {
        let end = from + at + 3;
        let rest = &line[end + 1..];
        let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        if digits > 0 {
            if let Ok(n) = rest[..digits].parse::<u32>() {
                let after = &rest[digits..];
                if let Some(text) = after.strip_prefix(": ").or_else(|| (after == ":").then_some("")) {
                    return Some((&line[..end], n, None, text.trim()));
                }
                if let Some(col) = after.strip_prefix(':') {
                    let cd = col.find(|c: char| !c.is_ascii_digit()).unwrap_or(col.len());
                    if cd > 0 {
                        if let (Ok(c), Some(text)) = (col[..cd].parse::<u32>(), col[cd..].strip_prefix(": ").or_else(|| (&col[cd..] == ":").then_some(""))) {
                            return Some((&line[..end], n, Some(c), text.trim()));
                        }
                    }
                }
            }
        }
        from = end;
    }
    None
}

/// Whether a line is one `go test -v` prints around a test, not from it.
pub fn is_marker(line: &str) -> bool {
    let t = line.trim_start();
    ["=== RUN", "=== PAUSE", "=== CONT", "=== NAME", "=== FAIL", "=== SKIP", "--- PASS:", "--- FAIL:", "--- SKIP:", "--- BENCH:"].iter().any(|m| t.starts_with(m))
}

/// A panic's first line (`panic: assignment to entry in nil map [recovered]`)
/// as its message.
pub fn panic_message(lines: &[String]) -> Option<String> {
    let l = lines.iter().find(|l| l.starts_with("panic: ") || l.starts_with("fatal error: "))?;
    let l = l.trim_end().trim_end_matches(" [recovered]");
    Some(l.to_string())
}

/// The first frame of a goroutine's trace that's in the project's own
/// packages: the function's line (`example.com/orbit/geo.TestPanics(0x..)`)
/// then its file's (`\t/home/u/orbit/geo/geo_test.go:13 +0x28`). Returns
/// the package, the file as printed and the line.
pub fn first_frame<'a>(lines: &'a [String], is_project: &dyn Fn(&str) -> Option<String>) -> Option<(String, &'a str, u32)> {
    for pair in lines.windows(2) {
        let (function, file) = (&pair[0], &pair[1]);
        if !file.starts_with('\t') || function.starts_with(['\t', ' ']) {
            continue;
        }
        let Some(package) = is_project(function) else { continue };
        let f = file.trim_start();
        let Some(at) = f.find(".go:") else { continue };
        let end = at + 3;
        let rest = &f[end + 1..];
        let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        if let Ok(n) = rest[..digits].parse::<u32>() {
            return Some((package, &f[..end], n));
        }
    }
    None
}

/// The package a frame's function line is in: `example.com/orbit/geo`
/// of `example.com/orbit/geo.TestPanics(0xc0)` or
/// `example.com/orbit/geo.(*T).Run(...)`.
pub fn frame_package(function: &str) -> Option<&str> {
    let slash = function.rfind('/').map_or(0, |i| i + 1);
    let dot = function[slash..].find('.')? + slash;
    (dot > 0).then(|| &function[..dot])
}

/// A test's `Found` when no scan saw it: under its package, no file.
pub fn unscanned(package: &str, name: &str) -> Found {
    Found::test(key(package, name), name).under([package_group(package)])
}

/// A subtest's `Found`, at its top-level test's line: a case of the
/// nearest ancestor that ran, else of its top-level test (then `a/b` is
/// one name: Go can't tell a subtest named `a/b` from `b` under an `a`
/// that didn't run).
pub fn case(package: &str, name: &str, parent: &str, at: Option<(&str, u32)>) -> Found {
    let label = name.strip_prefix(parent).map_or(name, |l| l.trim_start_matches('/'));
    let found = Found::case(key(package, name), label, key(package, parent));
    match at {
        Some((file, line)) => found.at(file, line),
        None => found,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_line_says_its_file_line_and_text() {
        assert_eq!(file_line("    orbit_test.go:37: got -2, want -3"), Some(("orbit_test.go", 37, None, "got -2, want -3")));
        assert_eq!(file_line("    /home/my user/orbit/orbit_test.go:37: x: y"), Some(("/home/my user/orbit/orbit_test.go", 37, None, "x: y")));
        assert_eq!(file_line("geo/geo_test.go:6:13: cannot use \"3\" (untyped string constant)"), Some(("geo/geo_test.go", 6, Some(13), "cannot use \"3\" (untyped string constant)")));
        assert_eq!(file_line("    C:/Users/u/orbit/x_test.go:9: no"), Some(("C:/Users/u/orbit/x_test.go", 9, None, "no")));
        assert_eq!(file_line("        line two of a message"), None);
        assert_eq!(file_line("--- FAIL: TestX (0.00s)"), None);
        assert_eq!(file_line("x.go:abc: no"), None);
        assert_eq!(file_line("x.go:12 no colon"), None);
        assert_eq!(file_line("x.go:12:"), Some(("x.go", 12, None, "")), "an empty message");
        assert_eq!(file_line(""), None);
        assert_eq!(file_line("é.go:é"), None, "no panic on odd bytes");
    }

    #[test]
    fn markers_are_what_go_prints_around_a_test() {
        assert!(is_marker("=== RUN   TestX") && is_marker("    --- FAIL: TestX/y (0.00s)") && is_marker("=== CONT  TestX"));
        assert!(!is_marker("    x_test.go:1: --- FAIL: not at the start"));
    }

    #[test]
    fn a_frame_is_in_the_project_by_its_functions_package() {
        assert_eq!(frame_package("example.com/orbit/geo.TestPanics(0xc000082a80?)"), Some("example.com/orbit/geo"));
        assert_eq!(frame_package("example.com/orbit.(*T).Run(0x1)"), Some("example.com/orbit"));
        assert_eq!(frame_package("testing.tRunner(0xc000082a80, 0x5843a0)"), Some("testing"));
        assert_eq!(frame_package("main.main()"), Some("main"));
        assert_eq!(frame_package("nodot"), None);
        let lines: Vec<String> = [
            "panic: boom",
            "goroutine 19 [running]:",
            "testing.tRunner.func1()",
            "\t/usr/local/go1.24.7/src/testing/testing.go:1737 +0x35e",
            "example.com/orbit/geo.TestPanics(0xc000082a80?)",
            "\t/home/u/orbit/geo/geo_test.go:13 +0x28",
        ]
        .map(String::from)
        .to_vec();
        let project = |f: &str| frame_package(f).filter(|p| p.starts_with("example.com/orbit")).map(String::from);
        assert_eq!(first_frame(&lines, &project), Some(("example.com/orbit/geo".into(), "/home/u/orbit/geo/geo_test.go", 13)));
        assert_eq!(panic_message(&lines).as_deref(), Some("panic: boom"));
        assert_eq!(panic_message(&["panic: x [recovered]".to_string()]).as_deref(), Some("panic: x"));
        assert_eq!(panic_message(&["fine".to_string()]), None);
    }
}
