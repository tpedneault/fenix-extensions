//! Splices as the buffer takes them -- a line and a column in
//! characters, its line breaks counted as Fenix's buffers count them --
//! and the undo and redo of the changes made through it.

use crate::table::Splice;

/// More splices than this go to the buffer as one, from the first to the
/// last: a column added to a million rows is one edit, not a million.
pub const MANY: usize = 64;

/// A change to the buffer: (line, column) to (line, column), both from
/// 0, in characters, as the text was before it, replaced with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub start: (u32, u32),
    pub end: (u32, u32),
    pub text: String,
}

/// Where bytes `offsets` (in order) are in `text`: (line, column) in
/// characters. A line ends at `\n`, `\r\n`, a lone `\r`, and the other
/// breaks a rope counts (vertical tab, form feed, NEL, U+2028, U+2029),
/// as Fenix's buffers do.
pub fn positions(text: &str, offsets: &[usize]) -> Vec<(u32, u32)> {
    let mut out = Vec::with_capacity(offsets.len());
    let (mut line, mut col) = (0u32, 0u32);
    let mut k = 0;
    let mut chars = text.char_indices().peekable();
    while k < offsets.len() {
        let Some((b, c)) = chars.next() else {
            while k < offsets.len() {
                out.push((line, col));
                k += 1;
            }
            break;
        };
        while k < offsets.len() && offsets[k] <= b {
            out.push((line, col));
            k += 1;
        }
        match c {
            '\n' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                line += 1;
                col = 0;
            }
            '\r' if chars.peek().is_some_and(|&(_, n)| n == '\n') => col += 1,
            '\r' => {
                line += 1;
                col = 0;
            }
            _ => col += 1,
        }
    }
    out
}

/// `splices` (in order, apart) as one, from the first's start to the
/// last's end, the text between them kept.
pub fn coalesce(text: &str, splices: &[Splice]) -> Splice {
    let (start, end) = (splices[0].start, splices[splices.len() - 1].end);
    let mut out = String::new();
    let mut at = start;
    for s in splices {
        out.push_str(&text[at..s.start]);
        out.push_str(&s.text);
        at = s.end;
    }
    Splice::new(start..end, out)
}

/// `splices` of `text` as the buffer's edits; many as one.
pub fn text_edits(text: &str, splices: &[Splice]) -> Vec<TextEdit> {
    if splices.is_empty() {
        return Vec::new();
    }
    let one;
    let splices = if splices.len() > MANY {
        one = [coalesce(text, splices)];
        &one[..]
    } else {
        splices
    };
    let offsets: Vec<usize> = splices.iter().flat_map(|s| [s.start, s.end]).collect();
    let at = positions(text, &offsets);
    splices.iter().enumerate().map(|(i, s)| TextEdit { start: at[2 * i], end: at[2 * i + 1], text: s.text.clone() }).collect()
}

/// A change made, and what undoes it.
#[derive(Debug, Clone)]
pub struct Change {
    pub what: String,
    pub undo: Vec<Splice>,
    pub redo: Vec<Splice>,
}

impl Change {
    fn bytes(&self) -> usize {
        self.undo.iter().chain(&self.redo).map(|s| s.text.len()).sum()
    }
}

/// How much text undo keeps, at most: a change bigger than that (a
/// column taken out of a big file) can't be undone here.
pub const KEPT: usize = 32 * 1024 * 1024;

/// The changes made through the buffer, to undo and redo them.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Change>,
    redo: Vec<Change>,
}

impl History {
    /// `change` made: what it was undone by is kept, the oldest let go
    /// past [`KEPT`]. False when it's too big to keep at all.
    pub fn push(&mut self, change: Change) -> bool {
        self.redo.clear();
        if change.bytes() > KEPT {
            return false;
        }
        self.undo.push(change);
        while self.undo.iter().map(Change::bytes).sum::<usize>() > KEPT {
            self.undo.remove(0);
        }
        true
    }

    pub fn undo(&mut self) -> Option<&Change> {
        let c = self.undo.pop()?;
        self.redo.push(c);
        self.redo.last()
    }

    pub fn redo(&mut self) -> Option<&Change> {
        let c = self.redo.pop()?;
        self.undo.push(c);
        self.undo.last()
    }

    /// The text changed under it (reread): nothing to undo.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_are_lines_and_characters() {
        let text = "ab\r\nçd\re\u{2028}f\n日本,x";
        let at = |s: &str| text.find(s).unwrap();
        assert_eq!(positions(text, &[0, at("b"), at("\r\n"), at("ç"), at("d"), at("e"), at("f"), at(",x"), text.len()]), [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (2, 0), (3, 0), (4, 2), (4, 4)]);
    }

    #[test]
    fn splices_become_the_buffers_edits() {
        let text = "id,name\n1,ada\n2,grace\n";
        let edits = text_edits(text, &[Splice::new(10..13, "\"Ada, C\""), Splice::insert(22, "3,x\n")]);
        assert_eq!(edits, [TextEdit { start: (1, 2), end: (1, 5), text: "\"Ada, C\"".into() }, TextEdit { start: (3, 0), end: (3, 0), text: "3,x\n".into() }]);
    }

    #[test]
    fn many_splices_go_as_one() {
        let text: String = (0..100).map(|i| format!("{i},x\n")).collect();
        let splices: Vec<Splice> = (0..100).map(|i| Splice::insert(text.match_indices('\n').nth(i).unwrap().0, ",y")).collect();
        let edits = text_edits(&text, &splices);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].start, (0, 3));
        assert_eq!(edits[0].end, (99, 4));
        let mut t = text.clone();
        let s = coalesce(&text, &splices);
        t.replace_range(s.start..s.end, &s.text);
        assert_eq!(t, (0..100).map(|i| format!("{i},x,y\n")).collect::<String>());
    }

    #[test]
    fn undo_and_redo_go_back_and_forth() {
        let mut h = History::default();
        let c = |w: &str| Change { what: w.into(), undo: vec![Splice::insert(0, "u")], redo: vec![Splice::insert(0, "r")] };
        h.push(c("one"));
        h.push(c("two"));
        assert_eq!(h.undo().unwrap().what, "two");
        assert_eq!(h.redo().unwrap().what, "two");
        assert_eq!(h.undo().unwrap().what, "two");
        h.push(c("three"));
        assert!(h.redo().is_none(), "a new change drops what was undone");
        assert_eq!(h.undo().unwrap().what, "three");
        assert_eq!(h.undo().unwrap().what, "one");
        assert!(h.undo().is_none());
        let big = Change { what: "big".into(), undo: vec![Splice::insert(0, "x".repeat(KEPT + 1))], redo: Vec::new() };
        assert!(!h.push(big));
    }
}
