//! What `regex-syntax` can't read, found by hand: lookaround, atomic
//! groups and backreferences (fancy-regex's), and how each named group
//! is written -- for the explanation, the flavour notes and the escaped
//! copies. A pattern is walked once, its escapes and classes skipped.

use std::ops::Range;

/// One construct, where it is in the pattern (bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub at: Range<usize>,
    pub what: Special,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Special {
    /// `(?=` or `(?!`: its opening.
    Lookahead { negative: bool },
    /// `(?<=` or `(?<!`: its opening.
    Lookbehind { negative: bool },
    /// `(?>`: its opening.
    Atomic,
    /// `\1`, `\k<name>`, `(?P=name)`: the whole of it.
    Backref(String),
    /// A named group's opening: `(?P<` (Python's way) or `(?<`.
    Named { python: bool },
}

/// Every lookaround, atomic group, backreference and named group in
/// `pattern`. With `extended`, `# ...` to the end of a line is a comment.
pub fn scan(pattern: &str, extended: bool) -> Vec<Found> {
    let b = pattern.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    // How deep in `[...]` it is (Rust nests classes), and whether a `]`
    // here would be the class's first character, so a literal.
    let mut class = 0usize;
    let mut first = false;
    while i < b.len() {
        let c = b[i];
        if c == b'\\' {
            let start = i;
            i += 1;
            let Some(&e) = b.get(i) else { break };
            if class == 0 && e.is_ascii_digit() && e != b'0' {
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                out.push(Found { at: start..i, what: Special::Backref(pattern[start + 1..i].to_string()) });
                continue;
            }
            if class == 0 && e == b'k' {
                if let Some((name, end)) = bracketed(pattern, i + 1) {
                    out.push(Found { at: start..end, what: Special::Backref(name) });
                    i = end;
                    continue;
                }
            }
            // Past the escaped character, whole (it may be several bytes).
            i += pattern[i..].chars().next().map_or(1, char::len_utf8);
            first = false;
            continue;
        }
        if class > 0 {
            if c == b'[' && b.get(i + 1) == Some(&b':') {
                // `[:alpha:]` inside a class.
                if let Some(end) = pattern[i + 2..].find(":]") {
                    i += 2 + end + 2;
                    first = false;
                    continue;
                }
            }
            match c {
                b']' if !first => class -= 1,
                b'[' => {
                    class += 1;
                    first = true;
                    i += 1;
                    if b.get(i) == Some(&b'^') {
                        i += 1;
                    }
                    continue;
                }
                _ => {}
            }
            first = false;
            i += 1;
            continue;
        }
        match c {
            b'[' => {
                class = 1;
                first = true;
                i += 1;
                if b.get(i) == Some(&b'^') {
                    i += 1;
                }
                continue;
            }
            b'#' if extended => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'(' if b.get(i + 1) == Some(&b'?') => {
                let rest = &pattern[i + 2..];
                let what = if rest.starts_with('=') {
                    Some((Special::Lookahead { negative: false }, 3))
                } else if rest.starts_with('!') {
                    Some((Special::Lookahead { negative: true }, 3))
                } else if rest.starts_with("<=") {
                    Some((Special::Lookbehind { negative: false }, 4))
                } else if rest.starts_with("<!") {
                    Some((Special::Lookbehind { negative: true }, 4))
                } else if rest.starts_with('>') {
                    Some((Special::Atomic, 3))
                } else if rest.starts_with("P<") {
                    Some((Special::Named { python: true }, 4))
                } else if rest.starts_with('<') {
                    Some((Special::Named { python: false }, 3))
                } else if let Some(name) = rest.strip_prefix("P=") {
                    match name.find(')') {
                        Some(end) => {
                            let to = i + 4 + end + 1;
                            out.push(Found { at: i..to, what: Special::Backref(name[..end].to_string()) });
                            i = to;
                            continue;
                        }
                        None => None,
                    }
                } else {
                    None
                };
                if let Some((what, len)) = what {
                    out.push(Found { at: i..i + len, what });
                    i += len;
                    continue;
                }
            }
            _ => {}
        }
        i += pattern[i..].chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// `<name>`, `{name}` or `'name'` at byte `at`: the name, and the byte
/// after its closing.
fn bracketed(pattern: &str, at: usize) -> Option<(String, usize)> {
    let open = *pattern.as_bytes().get(at)?;
    let close = match open {
        b'<' => '>',
        b'{' => '}',
        b'\'' => '\'',
        _ => return None,
    };
    let rest = &pattern[at + 1..];
    let end = rest.find(close)?;
    let name = &rest[..end];
    (!name.is_empty()).then(|| (name.to_string(), at + 1 + end + 1))
}

/// `pattern` with what `regex-syntax` can't read put as something it
/// can, every byte where it was: a lookaround or atomic group opens a
/// plain group (`(?:`, `(?u:`), and a backreference is one character
/// (`\a`, `[a]`, `\x07`, `\x{007}`). The explanation reads this, and
/// tells those pieces by where they start.
pub fn readable(pattern: &str, found: &[Found]) -> String {
    let mut out = pattern.to_string();
    for f in found {
        let len = f.at.len();
        let with = match &f.what {
            Special::Lookahead { .. } | Special::Atomic => "(?:".to_string(),
            Special::Lookbehind { .. } => "(?u:".to_string(),
            Special::Backref(_) => match len {
                2 => r"\a".to_string(),
                3 => "[a]".to_string(),
                4 => r"\x07".to_string(),
                n => format!(r"\x{{{}7}}", "0".repeat(n - 5)),
            },
            Special::Named { .. } => continue,
        };
        out.replace_range(f.at.clone(), &with);
    }
    out
}

/// `pattern` with its named groups written `(?<name>` (`python` false)
/// or `(?P<name>` -- as the language it's copied for wants them.
pub fn named_groups_as(pattern: &str, python: bool, extended: bool) -> String {
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut last = 0;
    for f in scan(pattern, extended) {
        if let Special::Named { python: p } = f.what {
            if p != python {
                out.push_str(&pattern[last..f.at.start]);
                out.push_str(if python { "(?P<" } else { "(?<" });
                last = f.at.end;
            }
        }
    }
    out.push_str(&pattern[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn whats(p: &str) -> Vec<Special> {
        scan(p, false).into_iter().map(|f| f.what).collect()
    }

    #[test]
    fn lookaround_backrefs_and_named_groups_are_found() {
        assert_eq!(
            whats(r"(?<=\$)(?P<n>\d+)(?!x)\1\k<n>(?P=n)(?>a)(?<m>b)"),
            [
                Special::Lookbehind { negative: false },
                Special::Named { python: true },
                Special::Lookahead { negative: true },
                Special::Backref("1".into()),
                Special::Backref("n".into()),
                Special::Backref("n".into()),
                Special::Atomic,
                Special::Named { python: false },
            ]
        );
    }

    #[test]
    fn escapes_classes_and_comments_hide_them() {
        assert!(whats(r"\(?=x[(?=][\]\1(?!]").is_empty(), "escaped, or in a class");
        assert!(whats(r"[[:alpha:](?=]").is_empty(), "a POSIX class inside a class");
        assert!(whats(r"[]\1]").is_empty(), "a `]` first in a class is a literal");
        assert_eq!(scan("a # (?=x)\n(?!y)", true).len(), 1, "a comment, with x");
        assert_eq!(scan("a # (?=x)", false).len(), 1, "not a comment without x");
    }

    #[test]
    fn readable_keeps_every_byte_where_it_was() {
        let p = r"(?<=a)b(?=c)(x)\1(?P<n>y)\k<n>";
        let r = readable(p, &scan(p, false));
        assert_eq!(r.len(), p.len());
        assert_eq!(r, r"(?u:a)b(?:c)(x)\a(?P<n>y)\x{7}");
        assert!(regex_syntax::ast::parse::Parser::new().parse(&r).is_ok(), "{r}");
    }

    #[test]
    fn named_groups_are_rewritten_for_the_language() {
        assert_eq!(named_groups_as(r"(?P<y>\d{4})-(?<m>\d\d)", false, false), r"(?<y>\d{4})-(?<m>\d\d)");
        assert_eq!(named_groups_as(r"(?P<y>\d{4})-(?<m>\d\d)", true, false), r"(?P<y>\d{4})-(?P<m>\d\d)");
        assert_eq!(named_groups_as(r"(?<=a)", true, false), r"(?<=a)", "a lookbehind isn't a name");
    }
}
