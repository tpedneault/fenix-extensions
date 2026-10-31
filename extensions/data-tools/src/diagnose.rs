//! A file's problems as places in it: where it doesn't parse, and what
//! its schema says against it.

use crate::node::{Lines, Pos};
use crate::path::{by_pointer, render, Located, Style};
use crate::schema::Problem;

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub start: Pos,
    pub end: Pos,
    pub message: String,
}

/// Where it stops parsing: the character there.
pub fn syntax(at: Pos, message: &str) -> Mark {
    Mark { start: at, end: Pos::new(at.line, at.col + 1), message: message.to_string() }
}

/// Each problem at the key (or value) it's about; one about something
/// that isn't there (`required`) at what holds it.
pub fn problems(problems: &[Problem], located: &[Located], lines: &Lines) -> Vec<Mark> {
    problems
        .iter()
        .map(|p| {
            let Some(l) = by_pointer(located, &p.pointer) else {
                return Mark { start: Pos::default(), end: Pos::new(0, lines.line(0).chars().count() as u32), message: p.message.clone() };
            };
            let exact = render(&l.path, Style::Pointer) == p.pointer;
            let (start, end) = match l.key {
                Some((s, e)) => (s, e),
                // A value's first line.
                None => (l.start, if l.end.line == l.start.line { l.end } else { Pos::new(l.start.line, lines.line(l.start.line).chars().count() as u32) }),
            };
            let message = if exact || p.pointer.is_empty() { p.message.clone() } else { format!("{}: {}", p.pointer, p.message) };
            Mark { start, end, message }
        })
        .collect()
}

/// A `file://` URI's path: `/home/me/a.json`, `C:/work/a.json`.
pub fn uri_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let path = String::from_utf8(out).ok()?;
    // `/C:/work` is `C:/work`.
    let b = path.as_bytes();
    Some(if b.len() > 2 && b[0] == b'/' && b[2] == b':' { path[1..].to_string() } else { path })
}

/// Whether two full paths are one file (`\` or `/`, a drive's case).
pub fn same_path(a: &str, b: &str) -> bool {
    let n = |p: &str| p.replace('\\', "/");
    let (a, b) = (n(a), n(b));
    if a.as_bytes().get(1) == Some(&b':') {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::locate;

    #[test]
    fn problems_go_to_their_keys() {
        let text = "{\n  \"name\": 1,\n  \"servers\": [\n    {\"port\": 70000}\n  ]\n}";
        let located = locate(&crate::json::parse(text).unwrap().node);
        let lines = Lines::new(text);
        let found = problems(
            &[
                Problem { pointer: "/name".into(), message: "1 is not of type \"string\"".into() },
                Problem { pointer: "/servers/0/port".into(), message: "too big".into() },
                Problem { pointer: "".into(), message: "\"id\" is a required property".into() },
                Problem { pointer: "/servers/0/missing".into(), message: "odd".into() },
                Problem { pointer: "/servers/0".into(), message: "an item".into() },
            ],
            &located,
            &lines,
        );
        assert_eq!((found[0].start, found[0].end), (Pos::new(1, 2), Pos::new(1, 8)));
        assert_eq!((found[1].start, found[1].end), (Pos::new(3, 5), Pos::new(3, 11)));
        assert_eq!((found[2].start, found[2].end, found[2].message.as_str()), (Pos::new(0, 0), Pos::new(0, 1), "\"id\" is a required property"));
        assert_eq!(found[3].message, "/servers/0/missing: odd");
        assert_eq!((found[4].start, found[4].end), (Pos::new(3, 4), Pos::new(3, 19)));
    }

    #[test]
    fn uris_and_paths() {
        assert_eq!(uri_path("file:///home/me/a%20b.json").as_deref(), Some("/home/me/a b.json"));
        assert_eq!(uri_path("file:///C:/work/%C3%A9.json").as_deref(), Some("C:/work/é.json"));
        assert_eq!(uri_path("untitled:1"), None);
        assert!(same_path("C:\\Work\\a.json", "c:/work/a.json"));
        assert!(!same_path("/a/B.json", "/a/b.json"));
        assert_eq!(syntax(Pos::new(2, 3), "x").end, Pos::new(2, 4));
    }
}
