//! Snapshots: text kept in a file beside the tests, and compared.
//!
//! [`assert_snapshot!`](crate::assert_snapshot) keeps `name` in the
//! crate's `tests/snapshots/<name>.txt`. A test fails when the text isn't
//! what the file has, or there's no file yet; run it with
//! `UPDATE_SNAPSHOTS=1` to write the files as the text is now, then read
//! the diff before committing them.

use std::path::Path;

/// The variable that writes snapshots instead of comparing them.
pub const UPDATE: &str = "UPDATE_SNAPSHOTS";

fn updating() -> bool {
    std::env::var(UPDATE).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// `actual` compared with the snapshot `name` in `dir`, or written there
/// with `UPDATE_SNAPSHOTS=1`. Panics, saying where they first differ.
pub fn check(dir: &Path, name: &str, actual: &str) {
    let path = dir.join(format!("{name}.txt"));
    if updating() {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        std::fs::write(&path, actual).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        return;
    }
    let Ok(expected) = std::fs::read_to_string(&path) else {
        panic!("no snapshot {} yet: run the test with {UPDATE}=1 to write it, then read it\n\n{actual}", path.display());
    };
    // A checkout on Windows may have given it CRLF.
    let expected = expected.replace("\r\n", "\n");
    if expected != actual {
        panic!("{} isn't what it was ({UPDATE}=1 writes it again):\n{}", path.display(), diff(&expected, actual));
    }
}

/// The lines of `old` and `new`, side by side where they differ.
pub fn diff(old: &str, new: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let mut out = String::new();
    for i in 0..a.len().max(b.len()) {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) if x == y => out.push_str(&format!("  {:>3} {x}\n", i + 1)),
            (x, y) => {
                if let Some(x) = x {
                    out.push_str(&format!("- {:>3} {x}\n", i + 1));
                }
                if let Some(y) = y {
                    out.push_str(&format!("+ {:>3} {y}\n", i + 1));
                }
            }
        }
    }
    out
}

/// Text compared with its snapshot, `tests/snapshots/<name>.txt` in the
/// crate the test is in; `UPDATE_SNAPSHOTS=1` writes it.
///
/// ```rust,ignore
/// fenix_test_kit::assert_snapshot!("playground", harness.screen(page));
/// ```
#[macro_export]
macro_rules! assert_snapshot {
    ($name:expr, $actual:expr) => {
        $crate::snapshot::check(&::std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("snapshots"), $name, &$actual)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_is_compared_and_a_difference_said() {
        if updating() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("fenix-test-kit-snap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("page.txt"), "one\r\ntwo\r\n").unwrap();
        check(&dir, "page", "one\ntwo\n");
        let said = std::panic::catch_unwind(|| check(&dir, "page", "one\nthree\n")).unwrap_err();
        let said = said.downcast_ref::<String>().unwrap();
        assert!(said.contains("-   2 two\n+   2 three"), "{said}");
        assert!(std::panic::catch_unwind(|| check(&dir, "missing", "x")).is_err(), "none yet is a failure");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
