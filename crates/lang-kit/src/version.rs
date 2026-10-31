//! Versions as the tools print them, compared: `1.2.3`, `v20.11.0`,
//! `go1.22.3`, `3.12.0rc1`, `1.0-SNAPSHOT`, `0.0.0-20240101-abcdef`, and
//! the ranges a project asks for (`>=3.9`, `^1.2`, `20.x`, `>=18 <21 ||
//! 22`).

use std::cmp::Ordering;
use std::fmt;

/// A version: its numbers, and what comes after them. A pre-release
/// (`-rc.1`, `rc1`, `-SNAPSHOT`, a Go pseudo-version's date) comes before
/// the release; a post-release (`.post1`) after it. A build (`+abc`) is
/// left out of comparing, and so is how it was written: `1.2` is `1.2.0`.
#[derive(Debug, Clone)]
pub struct Version {
    pub numbers: Vec<u64>,
    pub pre: Option<String>,
    pub post: Option<u64>,
    /// As it was written.
    pub text: String,
}

/// How far apart two versions are: what changed first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bump {
    None,
    Patch,
    Minor,
    Major,
}

impl Version {
    /// `text` as a version: a `v` or a name before its numbers is left
    /// out (`v1.2`, `go1.22`); `None` when it has no number.
    pub fn parse(text: &str) -> Option<Version> {
        let t = text.trim();
        let start = t.find(|c: char| c.is_ascii_digit())?;
        // Only a word or `v` before it: `go1.22`, `v2`, not `a 1`.
        if t[..start].contains(char::is_whitespace) {
            return None;
        }
        let rest = &t[start..];
        let rest = rest.split('+').next().unwrap_or(rest);
        let mut numbers = Vec::new();
        let mut at = 0;
        let bytes = rest.as_bytes();
        loop {
            let digits = rest[at..].bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                break;
            }
            numbers.push(rest[at..at + digits].parse().unwrap_or(u64::MAX));
            at += digits;
            if at < bytes.len() && bytes[at] == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit) {
                at += 1;
            } else {
                break;
            }
        }
        let tail = rest[at..].trim_start_matches(['.', '-', '_']);
        let (pre, post) = if tail.is_empty() {
            (None, None)
        } else if let Some(n) = tail.strip_prefix("post").or_else(|| tail.strip_prefix("p")).filter(|n| n.bytes().all(|b| b.is_ascii_digit())) {
            (None, Some(n.parse().unwrap_or(0)))
        } else {
            (Some(tail.split_whitespace().next().unwrap_or(tail).to_string()), None)
        };
        Some(Version { numbers, pre, post, text: t.to_string() })
    }

    /// Its numbers, as many as given (`1.2` is `1.2.0`).
    pub fn number(&self, i: usize) -> u64 {
        self.numbers.get(i).copied().unwrap_or(0)
    }

    pub fn major(&self) -> u64 {
        self.number(0)
    }

    pub fn is_prerelease(&self) -> bool {
        self.pre.is_some()
    }

    /// What changed from `self` to `newer`.
    pub fn bump_to(&self, newer: &Version) -> Bump {
        if newer <= self {
            return Bump::None;
        }
        if newer.number(0) != self.number(0) {
            // Below 1.0, a minor is a major (0.2 to 0.3).
            Bump::Major
        } else if newer.number(1) != self.number(1) {
            if self.number(0) == 0 {
                Bump::Major
            } else {
                Bump::Minor
            }
        } else {
            Bump::Patch
        }
    }

    /// Whether it's `min` or later.
    pub fn at_least(&self, min: &Version) -> bool {
        self >= min
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// A pre-release's parts compared: numbers as numbers, below words.
fn compare_pre(a: &str, b: &str) -> Ordering {
    let split = |s: &str| -> Vec<String> {
        let mut parts = Vec::new();
        let mut cur = String::new();
        for c in s.chars() {
            if matches!(c, '.' | '-' | '_') {
                if !cur.is_empty() {
                    parts.push(std::mem::take(&mut cur));
                }
            } else if !cur.is_empty() && cur.chars().last().is_some_and(|l| l.is_ascii_digit()) != c.is_ascii_digit() {
                parts.push(std::mem::take(&mut cur));
                cur.push(c);
            } else {
                cur.push(c);
            }
        }
        if !cur.is_empty() {
            parts.push(cur);
        }
        parts
    };
    let (a, b) = (split(a), split(b));
    for (x, y) in a.iter().zip(&b) {
        let o = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase()),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> Ordering {
        let n = self.numbers.len().max(other.numbers.len());
        for i in 0..n {
            match self.number(i).cmp(&other.number(i)) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        match (&self.pre, &other.pre) {
            (None, Some(_)) => return Ordering::Greater,
            (Some(_), None) => return Ordering::Less,
            (Some(a), Some(b)) => match compare_pre(a, b) {
                Ordering::Equal => {}
                o => return o,
            },
            (None, None) => {}
        }
        self.post.unwrap_or(0).cmp(&other.post.unwrap_or(0)).then(self.post.is_some().cmp(&other.post.is_some()))
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Version) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The first version in what a program printed: `Python 3.12.1`,
/// `rustc 1.80.0 (051478957 2024-07-21)`, `go version go1.22.3
/// linux/amd64`, `openjdk 21.0.2 2024-01-16`. One with a dot is
/// preferred to a bare number (`clang-format version 18`).
pub fn find_version(text: &str) -> Option<Version> {
    let mut bare = None;
    for word in text.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ',' | ';' | '"' | '\'' | '/' | ':' | '=')) {
        let Some(start) = word.find(|c: char| c.is_ascii_digit()) else { continue };
        // A letter run before it is a name (`go1.22`, `v20`); a date or a
        // hash isn't a version.
        let lead = &word[..start];
        if !lead.chars().all(|c| c.is_ascii_alphabetic() || c == '-') {
            continue;
        }
        let rest = &word[start..];
        let dotted = rest.split(|c: char| !c.is_ascii_digit() && c != '.').next().unwrap_or("");
        if looks_like_date(rest) {
            continue;
        }
        if dotted.contains('.') && dotted.split('.').filter(|p| !p.is_empty()).count() >= 2 {
            // A distribution's revision is kept as it's written, but isn't
            // a pre-release: Ubuntu's 13.3.0-6ubuntu2 is 13.3.0.
            let mut v = Version::parse(rest)?;
            if without_revision(rest) != rest {
                v.pre = None;
            }
            return Some(v);
        }
        if bare.is_none() && lead.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) && rest.len() < 6 {
            bare = Version::parse(rest);
        }
    }
    bare
}

/// A distribution's revision of a package taken off: Ubuntu's
/// `13.3.0-6ubuntu2~24.04.1` is 13.3.0, Debian's `2.39.2-1~deb12u1` and
/// `1:2.43.0-1` the same; `-rc1`, `-SNAPSHOT` and `-beta.2` are kept.
fn without_revision(word: &str) -> &str {
    let Some(dash) = word.find('-') else { return word };
    let tail = &word[dash + 1..];
    let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
    let after = &tail[digits..];
    let packaged = digits > 0 && (after.is_empty() || after.starts_with("ubuntu") || after.starts_with("build") || after.starts_with('~') || after.starts_with("+deb") || after.starts_with(".el") || after.starts_with(".fc"));
    if packaged {
        &word[..dash]
    } else {
        word
    }
}

fn looks_like_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 10 && b[..4].iter().all(u8::is_ascii_digit) && b[4] == b'-' && b[7] == b'-'
}

/// One comparison of a range.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Ge,
    Gt,
    Le,
    Lt,
    Eq,
}

/// What a project asks of a version: comparisons all of which hold
/// (`>=18 <21`), alternatives any of which does (`||`). `^1.2` is
/// `>=1.2 <2`, `~1.2.3` `>=1.2.3 <1.3`, a bare `20` or `20.x` any 20,
/// `*` or nothing anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    any: Vec<Vec<(Op, Version)>>,
    text: String,
}

impl Requirement {
    pub fn parse(text: &str) -> Option<Requirement> {
        let mut any = Vec::new();
        for alt in text.split("||") {
            let mut all = Vec::new();
            // `>= 18` as `>=18`; commas as spaces (`>=3.9,<4`).
            let alt = alt.replace(',', " ");
            let mut words: Vec<String> = Vec::new();
            for w in alt.split_whitespace() {
                match words.last_mut() {
                    Some(last) if last.chars().all(|c| "<>=~^".contains(c)) => last.push_str(w),
                    _ => words.push(w.to_string()),
                }
            }
            for w in words {
                let w = w.trim_start_matches('v');
                if w == "*" || w.eq_ignore_ascii_case("x") || w.is_empty() {
                    continue;
                }
                let (op, rest) = if let Some(r) = w.strip_prefix(">=") {
                    ("ge", r)
                } else if let Some(r) = w.strip_prefix("<=") {
                    ("le", r)
                } else if let Some(r) = w.strip_prefix('>') {
                    ("gt", r)
                } else if let Some(r) = w.strip_prefix('<') {
                    ("lt", r)
                } else if let Some(r) = w.strip_prefix("==").or_else(|| w.strip_prefix('=')) {
                    ("eq", r)
                } else if let Some(r) = w.strip_prefix("~=").or_else(|| w.strip_prefix('~')) {
                    ("tilde", r)
                } else if let Some(r) = w.strip_prefix('^') {
                    ("caret", r)
                } else {
                    ("eq", w)
                };
                let rest = rest.trim_start_matches('v');
                // `20.x`, `1.2.*`: as many numbers as are given.
                let given: Vec<&str> = rest.split('.').take_while(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())).collect();
                let wild = rest.split('.').any(|p| p == "x" || p == "X" || p == "*");
                let v = if wild { Version::parse(&given.join("."))? } else { Version::parse(rest)? };
                let upper = |at: usize| -> Version {
                    let mut n: Vec<u64> = (0..=at).map(|i| v.number(i)).collect();
                    n[at] = n[at].saturating_add(1);
                    let text = n.iter().map(u64::to_string).collect::<Vec<_>>().join(".");
                    Version { numbers: n, pre: Some("0".into()), post: None, text }
                };
                match op {
                    "ge" => all.push((Op::Ge, v)),
                    "gt" => all.push((Op::Gt, v)),
                    "le" => all.push((Op::Le, v)),
                    "lt" => all.push((Op::Lt, v)),
                    // A bare partial version (`20`, `1.2`, `20.x`): any
                    // with those numbers.
                    "eq" if !w.starts_with('=') && v.pre.is_none() && (wild || given.len() < 3) => {
                        let at = given.len().max(1) - 1;
                        all.push((Op::Ge, v.clone()));
                        all.push((Op::Lt, upper(at)));
                    }
                    "eq" => all.push((Op::Eq, v)),
                    "tilde" => {
                        // `~1.2.3` (npm): the patch moves, `~1` the minor.
                        // `~=1.2.3` (Python): the last but one does.
                        let n = given.len().max(1);
                        let at = if w.starts_with("~=") { n.saturating_sub(2) } else { (n - 1).min(1) };
                        all.push((Op::Ge, v.clone()));
                        all.push((Op::Lt, upper(at)));
                    }
                    _ => {
                        // `^`: the first number that isn't 0 stays.
                        let at = (0..given.len().max(1)).find(|&i| v.number(i) != 0).unwrap_or(given.len().max(1) - 1);
                        all.push((Op::Ge, v.clone()));
                        all.push((Op::Lt, upper(at)));
                    }
                }
            }
            any.push(all);
        }
        Some(Requirement { any, text: text.trim().to_string() })
    }

    /// The least version it takes (`>=3.9` → `3.9`), to show.
    pub fn minimum(&self) -> Option<&Version> {
        self.any.iter().flat_map(|all| all.iter()).filter(|(op, _)| matches!(op, Op::Ge | Op::Gt | Op::Eq)).map(|(_, v)| v).min()
    }

    pub fn matches(&self, v: &Version) -> bool {
        self.any.iter().any(|all| {
            all.iter().all(|(op, r)| match op {
                Op::Ge => v >= r,
                Op::Gt => v > r,
                Op::Le => v <= r,
                Op::Lt => v < r,
                Op::Eq => v == r,
            })
        })
    }
}

impl fmt::Display for Requirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// `a` newer than `b`, both as versions; as text when either isn't one.
pub fn newer(a: &str, b: &str) -> bool {
    match (Version::parse(a), Version::parse(b)) {
        (Some(a), Some(b)) => a > b,
        _ => !a.is_empty() && a != b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("{s} is a version"))
    }

    #[test]
    fn versions_are_read_as_tools_write_them() {
        assert_eq!(v("1.2.3").numbers, [1, 2, 3]);
        assert_eq!(v("v20.11.0").numbers, [20, 11, 0]);
        assert_eq!(v("go1.22.3").numbers, [1, 22, 3]);
        assert_eq!(v("3.12.0rc1").pre.as_deref(), Some("rc1"));
        assert_eq!(v("1.0-SNAPSHOT").pre.as_deref(), Some("SNAPSHOT"));
        assert_eq!(v("1.2.3+build.5").pre, None);
        assert_eq!(v("2.0.post3").post, Some(3));
        assert_eq!(v("0.0.0-20240101120000-abcdef123456").numbers, [0, 0, 0]);
        assert_eq!(Version::parse("none"), None);
        assert_eq!(Version::parse("a 1"), None);
    }

    #[test]
    fn versions_compare() {
        assert!(v("1.10.0") > v("1.9.9"));
        assert!(v("1.2") == v("1.2.0"));
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
        assert!(v("1.0.0-rc.2") < v("1.0.0-rc.10"));
        assert!(v("1.0.0-alpha") < v("1.0.0-beta"));
        assert!(v("3.12.0rc1") < v("3.12.0"));
        assert!(v("2.0.post1") > v("2.0"));
        assert!(v("1.0-SNAPSHOT") < v("1.0"));
        assert!(v("0.0.0-20240201-abc") > v("0.0.0-20240101-def"));
        assert!(v("v1.22.3").at_least(&v("1.21")));
        assert!(!v("3.8.10").at_least(&v("3.9")));
        assert!(newer("2.0.0", "1.9.0") && !newer("1.0", "1.0.0") && newer("main", "dev"));
    }

    #[test]
    fn a_bump_is_what_changed_first() {
        assert_eq!(v("1.2.3").bump_to(&v("1.2.4")), Bump::Patch);
        assert_eq!(v("1.2.3").bump_to(&v("1.3.0")), Bump::Minor);
        assert_eq!(v("1.2.3").bump_to(&v("2.0.0")), Bump::Major);
        assert_eq!(v("0.2.3").bump_to(&v("0.3.0")), Bump::Major);
        assert_eq!(v("1.2.3").bump_to(&v("1.2.3")), Bump::None);
        assert_eq!(v("2.0.0").bump_to(&v("1.0.0")), Bump::None);
    }

    #[test]
    fn a_version_is_found_in_what_a_program_says() {
        let f = |s: &str| find_version(s).map(|v| v.to_string());
        assert_eq!(f("Python 3.12.1").as_deref(), Some("3.12.1"));
        assert_eq!(f("rustc 1.80.0 (051478957 2024-07-21)").as_deref(), Some("1.80.0"));
        assert_eq!(f("go version go1.22.3 linux/amd64").as_deref(), Some("1.22.3"));
        assert_eq!(f("openjdk 21.0.2 2024-01-16\nOpenJDK Runtime").as_deref(), Some("21.0.2"));
        assert_eq!(f("v20.11.0").as_deref(), Some("20.11.0"));
        assert_eq!(f("clangd version 18.1.3 (https://github.com/llvm/llvm-project c13b7485b)").as_deref(), Some("18.1.3"));
        assert_eq!(f("cmake version 3.28.3\n\nCMake suite maintained").as_deref(), Some("3.28.3"));
        assert_eq!(f("uv 0.4.18 (7b55e9790 2024-10-01)").as_deref(), Some("0.4.18"));
        assert_eq!(f("gopls v0.15.3").as_deref(), Some("0.15.3"));
        assert_eq!(f("javac 17").as_deref(), Some("17"));
        // A distribution's revision is said as it's written, but isn't a
        // pre-release: Ubuntu's g++ 13.3.0 is 13.3.0.
        let gcc = find_version("g++ (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0").unwrap();
        assert_eq!((gcc.text.as_str(), gcc.pre.as_deref()), ("13.3.0-6ubuntu2~24.04.1", None));
        assert!(Requirement::parse(">=13.3").unwrap().matches(&gcc) && gcc == v("13.3.0"));
        for (said, is) in [("git version 2.39.2-1~deb12u1", "2.39.2"), ("cmake version 3.28.3-1build1", "3.28.3"), ("gcc (GCC) 11.4.1-3.el9", "11.4.1"), ("ii  git 1:2.43.0-1", "2.43.0")] {
            assert_eq!(find_version(said), Version::parse(is), "{said}");
            assert!(find_version(said).is_some_and(|v| v.pre.is_none()), "{said}");
        }
        assert!(find_version("node v22.0.0-rc.1").is_some_and(|v| v.is_prerelease()));
        assert!(find_version("app 1.0-SNAPSHOT").is_some_and(|v| v.is_prerelease()));
        assert!(find_version("v0.0.0-20240101123456-1a2deb3c4d5e").is_some_and(|v| v.is_prerelease()), "a Go pseudo-version");
        assert_eq!(f("built 2024-07-21, nothing else"), None);
        assert_eq!(f("no version here"), None);
    }

    #[test]
    fn requirements_take_what_they_say() {
        let r = |s: &str| Requirement::parse(s).unwrap_or_else(|| panic!("{s} is a requirement"));
        let ok = |req: &str, ver: &str| r(req).matches(&v(ver));
        assert!(ok(">=3.9", "3.12.1") && !ok(">=3.9", "3.8.10"));
        assert!(ok(">= 18 < 21", "20.1.0") && !ok(">=18 <21", "21.0.0"));
        assert!(ok(">=3.9,<4", "3.11") && !ok(">=3.9,<4", "4.0"));
        assert!(ok("^1.2", "1.9.0") && !ok("^1.2", "2.0.0") && !ok("^1.2", "1.1.9"));
        assert!(ok("^0.2.3", "0.2.9") && !ok("^0.2.3", "0.3.0"));
        assert!(ok("~1.2.3", "1.2.9") && !ok("~1.2.3", "1.3.0"));
        assert!(ok("~=3.10", "3.12") && !ok("~=3.10", "4.0"));
        assert!(ok("20", "20.11.0") && !ok("20", "21.0.0") && !ok("20", "19.9"));
        assert!(ok("20.x", "20.1") && ok("1.2.*", "1.2.7") && !ok("1.2.*", "1.3.0"));
        assert!(ok("=1.2.3", "1.2.3") && !ok("=1.2.3", "1.2.4"));
        assert!(ok("16 || >=18", "18.2.0") && ok("16 || >=18", "16.4.0") && !ok("16 || >=18", "17.0.0"));
        assert!(ok("*", "0.1") && ok("", "9"));
        assert!(ok("v20.11.0", "20.11.0"));
        assert_eq!(r(">=3.9 <4").minimum().map(|v| v.to_string()).as_deref(), Some("3.9"));
        assert_eq!(r(">=3.9 <4").to_string(), ">=3.9 <4");
        assert_eq!(Requirement::parse(">=lts"), None);
    }
}
