//! A change to the notebook as one edit of its buffer: the text between
//! what the old and new have in common at their start and their end, so
//! a cell typed in changes a line or two of the file, not all of it --
//! Fenix's undo has it as it is, and the buffer's marks stay put.

/// From (line, column) to (line, column), both from 0 and in characters,
/// replaced with `text`: the API's `TextEdit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub start: (u32, u32),
    pub end: (u32, u32),
    pub text: String,
}

/// Where character `at` of `text` is: its line and column.
fn place(text: &str, at: usize) -> (u32, u32) {
    let (mut line, mut col) = (0u32, 0u32);
    for c in text.chars().take(at) {
        if c == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// The edit that makes `old` into `new`, or none when they're the same.
pub fn diff(old: &str, new: &str) -> Option<TextEdit> {
    if old == new {
        return None;
    }
    let (a, b): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let room = a.len().min(b.len()) - prefix;
    let suffix = a.iter().rev().zip(b.iter().rev()).take(room).take_while(|(x, y)| x == y).count();
    Some(TextEdit { start: place(old, prefix), end: place(old, a.len() - suffix), text: b[prefix..b.len() - suffix].iter().collect() })
}

/// `edit` made to `text`: what the buffer will hold.
#[cfg(test)]
pub fn apply(text: &str, edit: &TextEdit) -> String {
    let offset = |(line, col): (u32, u32)| {
        let mut chars = 0usize;
        for (i, l) in text.split_inclusive('\n').enumerate() {
            if i as u32 == line {
                return chars + col as usize;
            }
            chars += l.chars().count();
        }
        chars + col as usize
    };
    let chars: Vec<char> = text.chars().collect();
    let (s, e) = (offset(edit.start), offset(edit.end));
    chars[..s].iter().chain(edit.text.chars().collect::<Vec<_>>().iter()).chain(chars[e..].iter()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_changed_is_replaced() {
        let old = "{\n \"source\": [\n  \"a = 1\"\n ]\n}\n";
        let new = "{\n \"source\": [\n  \"a = 12\"\n ]\n}\n";
        let e = diff(old, new).unwrap();
        assert_eq!(e, TextEdit { start: (2, 8), end: (2, 8), text: "2".into() });
        assert_eq!(apply(old, &e), new);
    }

    #[test]
    fn lines_added_and_taken_away() {
        let old = "one\ntwo\nthree\n";
        for new in ["one\nthree\n", "one\ntwo\nmore\nthree\n", "", "zero\none\ntwo\nthree\n", "one\ntwo\nthree\nfour\n"] {
            let e = diff(old, new).unwrap();
            assert_eq!(apply(old, &e), new, "{e:?}");
        }
        assert_eq!(diff(old, old), None);
    }

    #[test]
    fn characters_not_bytes() {
        let old = "r² = π\n";
        let new = "r² = 2π\n";
        let e = diff(old, new).unwrap();
        assert_eq!(e.start, (0, 5));
        assert_eq!(apply(old, &e), new);
    }

    #[test]
    fn a_repeated_run_isnt_counted_twice() {
        // "aa" to "aaa": the prefix and the suffix mustn't overlap.
        let e = diff("aa", "aaa").unwrap();
        assert_eq!(apply("aa", &e), "aaa");
        let e = diff("aaa", "aa").unwrap();
        assert_eq!(apply("aaa", &e), "aa");
    }
}
