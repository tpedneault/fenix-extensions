//! What a command works on -- the selection, else the whole buffer -- and
//! its result put back in the buffer's way: its line endings, the
//! selection's indentation, a final newline if it had one.

use crate::node::{end_of, Pos};

#[derive(Debug, PartialEq)]
pub struct Target {
    /// What's transformed: the selection, without the whitespace around it.
    pub text: String,
    pub start: Pos,
    pub end: Pos,
    /// The whitespace around it, kept.
    before: String,
    after: String,
    /// The indentation of the line it starts on, for the lines after its
    /// first.
    indent: String,
    crlf: bool,
}

/// The text from `start` up to `end` (characters, as Fenix counts them).
pub fn slice(text: &str, start: Pos, end: Pos) -> String {
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        let i = i as u32;
        if i < start.line || i > end.line {
            continue;
        }
        let from = if i == start.line { start.col as usize } else { 0 };
        let chars: Vec<char> = line.chars().collect();
        let to = if i == end.line { (end.col as usize).min(chars.len()) } else { chars.len() };
        if from < to {
            out.extend(&chars[from..to]);
        }
        if i < end.line {
            out.push('\n');
        }
    }
    out
}

/// The selection in `buffer`, or all of it.
pub fn target(buffer: &str, selection: Option<(Pos, Pos)>) -> Target {
    let crlf = buffer.contains("\r\n");
    let (start, end) = selection.filter(|(a, b)| a != b).unwrap_or((Pos::default(), end_of(buffer)));
    let whole = slice(buffer, start, end).replace("\r\n", "\n");
    let trimmed_start = whole.trim_start();
    let before = whole[..whole.len() - trimmed_start.len()].to_string();
    let body = trimmed_start.trim_end();
    let after = trimmed_start[body.len()..].to_string();
    let line = buffer.split('\n').nth(start.line as usize).unwrap_or("");
    let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    // Its lines after the first, as far left as its first is.
    let mut lines = body.split('\n');
    let mut text = lines.next().unwrap_or("").to_string();
    for l in lines {
        text.push('\n');
        let strip = l.chars().take(indent.chars().count()).take_while(|c| *c == ' ' || *c == '\t').count();
        text.extend(l.chars().skip(strip));
    }
    Target { text, start, end, before, after, indent, crlf }
}

impl Target {
    /// `out` in its place: the whitespace that was around it kept (one
    /// newline at the end of a whole buffer that ended with any), lines
    /// after the first indented as the line it starts on, CRLF if the
    /// buffer had it.
    pub fn finish(&self, out: &str) -> String {
        let out = out.replace("\r\n", "\n");
        let mut lines = out.split('\n');
        let mut text = lines.next().unwrap_or("").to_string();
        for line in lines {
            text.push('\n');
            if !line.is_empty() {
                text.push_str(&self.indent);
            }
            text.push_str(line);
        }
        let after = if self.after.contains('\n') && self.after.trim().is_empty() && self.after.ends_with('\n') && self.start == Pos::default() { "\n".to_string() } else { self.after.clone() };
        let full = format!("{}{text}{after}", self.before);
        if self.crlf {
            full.replace('\n', "\r\n")
        } else {
            full
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_by_characters() {
        let text = "aé\nbcd\nef";
        assert_eq!(slice(text, Pos::new(0, 1), Pos::new(1, 2)), "é\nbc");
        assert_eq!(slice(text, Pos::new(1, 0), Pos::new(2, 0)), "bcd\n");
        assert_eq!(slice(text, Pos::default(), end_of(text)), text);
    }

    #[test]
    fn the_whole_buffer_keeps_one_final_newline() {
        let t = target("{\"a\":1}\n\n", None);
        assert_eq!((t.text.as_str(), t.start, t.end), ("{\"a\":1}", Pos::new(0, 0), Pos::new(2, 0)));
        assert_eq!(t.finish("{\n  \"a\": 1\n}"), "{\n  \"a\": 1\n}\n");
        assert_eq!(target("[1]", None).finish("[\n  1\n]"), "[\n  1\n]");
    }

    #[test]
    fn crlf_stays_crlf() {
        let t = target("{\r\n\"a\": 1}\r\n", None);
        assert_eq!(t.text, "{\n\"a\": 1}");
        assert_eq!(t.finish("{\n  \"a\": 1\n}"), "{\r\n  \"a\": 1\r\n}\r\n");
    }

    #[test]
    fn a_selection_is_indented_as_its_line() {
        let buffer = "{\n    \"cfg\": {\"a\":1,\"b\":[2]},\n    \"x\": 1\n}";
        let t = target(buffer, Some((Pos::new(1, 11), Pos::new(1, 26))));
        assert_eq!(t.text, "{\"a\":1,\"b\":[2]}");
        assert_eq!(t.finish("{\n  \"a\": 1\n}"), "{\n      \"a\": 1\n    }");
        let nested = target("x:\n  a:\n    b: 1\n", Some((Pos::new(1, 0), Pos::new(3, 0))));
        assert_eq!(nested.text, "a:\n  b: 1");
        // A linewise selection: its own indentation and newline kept.
        let yaml = "top:\n  b: 1\n  a: 2\nend: 3\n";
        let t = target(yaml, Some((Pos::new(1, 0), Pos::new(3, 0))));
        assert_eq!(t.text, "b: 1\na: 2");
        assert_eq!(t.finish("a: 2\nb: 1"), "  a: 2\n  b: 1\n");
    }

    #[test]
    fn an_empty_selection_is_the_buffer() {
        assert_eq!(target("a: 1", Some((Pos::new(0, 2), Pos::new(0, 2)))).text, "a: 1");
    }
}
