//! A text field, one line or several: a caret, a selection, a word at a
//! time, pastes, undo, and scrolled to keep the caret in view.

use std::cell::Cell;
use std::ops::Range;

use crate::motion::top_for;
use crate::scroll::scrollbar;
use crate::text::char_width;
use crate::{Canvas, Fill, Key, KeyPress, Mouse, Outcome, Rect, Role, Style};

/// The caret, as Fenix's own fields draw it.
const CARET: char = '▏';

/// How many edits undo goes back.
const UNDO: usize = 200;

/// What an edit was, to undo typed runs as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    Typed,
    Other,
}

/// A field being typed in. The text and caret are kept together, so they
/// change through its methods; positions are in chars.
#[derive(Debug, Clone)]
pub struct TextField {
    text: String,
    caret: usize,
    /// Where the selection started: it runs from here to the caret.
    anchor: Option<usize>,
    /// Several lines: `Enter` breaks the line, `Up` and `Down` move
    /// between them.
    pub multiline: bool,
    /// Whether it has the keyboard: drawn as a panel with its caret, and
    /// the canvas marked as typing.
    pub focused: bool,
    /// Shown, muted, while it's empty.
    pub placeholder: String,
    /// The column `Up` and `Down` keep to.
    goal: Option<usize>,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    last: Option<Edit>,
    /// One line: the first cell shown. Several: the first row.
    scroll: Cell<usize>,
    view: Cell<usize>,
    area: Cell<Rect>,
}

impl Default for TextField {
    fn default() -> TextField {
        TextField {
            text: String::new(),
            caret: 0,
            anchor: None,
            multiline: false,
            focused: true,
            placeholder: String::new(),
            goal: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last: None,
            scroll: Cell::new(0),
            view: Cell::new(0),
            area: Cell::new(Rect::default()),
        }
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl TextField {
    /// A one-line field.
    pub fn new() -> TextField {
        TextField::default()
    }

    /// A field of several lines.
    pub fn multiline() -> TextField {
        TextField { multiline: true, ..TextField::default() }
    }

    /// With `text` in it, the caret at its end.
    pub fn with_text(mut self, text: &str) -> TextField {
        self.set_text(text);
        self
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> TextField {
        self.placeholder = placeholder.into();
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Its text replaced (an edit undo can take back), the caret at the
    /// end.
    pub fn set_text(&mut self, text: &str) {
        let text = self.clean(text);
        if text != self.text {
            self.remember(Edit::Other);
        }
        self.caret = text.chars().count();
        self.text = text;
        self.anchor = None;
    }

    /// The caret, in chars.
    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn set_caret(&mut self, at: usize) {
        self.caret = at.min(self.len());
        self.anchor = None;
        self.goal = None;
    }

    /// The chars selected, when some are.
    pub fn selection(&self) -> Option<Range<usize>> {
        let a = self.anchor?;
        (a != self.caret).then(|| a.min(self.caret)..a.max(self.caret))
    }

    pub fn selected_text(&self) -> Option<&str> {
        let r = self.selection()?;
        Some(&self.text[self.byte(r.start)..self.byte(r.end)])
    }

    /// Chars `range` selected, the caret at its end.
    pub fn select(&mut self, range: Range<usize>) {
        let n = self.len();
        self.anchor = Some(range.start.min(n));
        self.caret = range.end.min(n);
    }

    pub fn select_all(&mut self) {
        self.select(0..self.len());
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte(&self, at: usize) -> usize {
        self.text.char_indices().nth(at).map_or(self.text.len(), |(b, _)| b)
    }

    fn chars(&self) -> Vec<char> {
        self.text.chars().collect()
    }

    /// What a field of its kind keeps of `text`: one line has no line
    /// breaks; neither has other controls (tabs aside).
    fn clean(&self, text: &str) -> String {
        let text = text.replace("\r\n", "\n");
        text.chars()
            .filter_map(|c| match c {
                '\n' | '\r' if !self.multiline => Some(' '),
                '\r' => Some('\n'),
                '\n' | '\t' => Some(c),
                c if c.is_control() => None,
                c => Some(c),
            })
            .collect()
    }

    fn remember(&mut self, edit: Edit) {
        // A run of typing undoes at once, up to a space.
        if !(edit == Edit::Typed && self.last == Some(Edit::Typed)) {
            self.undo.push((self.text.clone(), self.caret));
            if self.undo.len() > UNDO {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last = Some(edit);
    }

    /// Chars `range` replaced with `with`, the caret after it.
    fn replace(&mut self, range: Range<usize>, with: &str, edit: Edit) {
        self.remember(edit);
        let (from, to) = (self.byte(range.start), self.byte(range.end));
        self.text.replace_range(from..to, with);
        self.caret = range.start + with.chars().count();
        self.anchor = None;
        self.goal = None;
    }

    /// `with` typed in place of the selection, or at the caret.
    fn type_in(&mut self, with: &str, edit: Edit) -> Outcome {
        let range = self.selection().unwrap_or(self.caret..self.caret);
        if with.is_empty() && range.is_empty() {
            return Outcome::Ignored;
        }
        self.replace(range, with, edit);
        Outcome::Edited
    }

    /// The text pasted in, at the caret or over the selection: one line
    /// keeps it to one, and controls are left out.
    pub fn paste(&mut self, text: &str) -> Outcome {
        let text = self.clean(text);
        self.type_in(&text, Edit::Other)
    }

    /// Takes back the last edit.
    pub fn undo(&mut self) -> bool {
        let Some((text, caret)) = self.undo.pop() else { return false };
        self.redo.push((std::mem::replace(&mut self.text, text), self.caret));
        self.caret = caret;
        self.anchor = None;
        self.last = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some((text, caret)) = self.redo.pop() else { return false };
        self.undo.push((std::mem::replace(&mut self.text, text), self.caret));
        self.caret = caret;
        self.anchor = None;
        self.last = None;
        true
    }

    /// Where each line starts.
    fn line_starts(chars: &[char]) -> Vec<usize> {
        std::iter::once(0).chain(chars.iter().enumerate().filter(|(_, c)| **c == '\n').map(|(i, _)| i + 1)).collect()
    }

    /// Where a moving key takes the caret, and the column kept for `Up`
    /// and `Down`. None when it isn't one.
    fn target(&self, key: Key) -> Option<(usize, Option<usize>)> {
        let chars = self.chars();
        let n = chars.len();
        let at = self.caret.min(n);
        let starts = Self::line_starts(&chars);
        let line = starts.partition_point(|&s| s <= at) - 1;
        let line_end = |l: usize| starts.get(l + 1).map_or(n, |s| s - 1);
        let col_of = |from: usize, to: usize| chars[from..to].iter().map(|&c| char_width(c)).sum::<usize>();
        // The char of line `l` at column `col`, or its end.
        let at_col = |l: usize, col: usize| {
            let (mut i, mut w) = (starts[l], 0);
            while i < line_end(l) && w + char_width(chars[i]) <= col {
                w += char_width(chars[i]);
                i += 1;
            }
            i
        };
        let lines_by = |down: bool, by: usize| {
            let goal = self.goal.unwrap_or_else(|| col_of(starts[line], at));
            let to = if down { line + by } else { line.wrapping_sub(by) };
            let to = if to < starts.len() {
                at_col(to, goal)
            } else if down {
                n
            } else {
                0
            };
            (to, Some(goal))
        };
        Some(match key {
            Key::Left => (at.saturating_sub(1), None),
            Key::Right => ((at + 1).min(n), None),
            Key::WordLeft => {
                let mut i = at;
                while i > 0 && !is_word(chars[i - 1]) {
                    i -= 1;
                }
                while i > 0 && is_word(chars[i - 1]) {
                    i -= 1;
                }
                (i, None)
            }
            Key::WordRight => {
                let mut i = at;
                while i < n && !is_word(chars[i]) {
                    i += 1;
                }
                while i < n && is_word(chars[i]) {
                    i += 1;
                }
                (i, None)
            }
            Key::Home => (starts[line], None),
            Key::End => (line_end(line), None),
            Key::Up if self.multiline => lines_by(false, 1),
            Key::Down if self.multiline => lines_by(true, 1),
            Key::PageUp if self.multiline => lines_by(false, crate::motion::page(self.view.get())),
            Key::PageDown if self.multiline => lines_by(true, crate::motion::page(self.view.get())),
            _ => return None,
        })
    }

    /// The caret to `to`, the selection dropped or, `extend`ing, grown.
    fn go(&mut self, to: usize, goal: Option<usize>, extend: bool) -> Outcome {
        let had = self.selection();
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        let moved = to != self.caret || had != self.selection();
        self.caret = to;
        self.goal = goal;
        self.last = None;
        if moved {
            Outcome::Moved
        } else {
            Outcome::Ignored
        }
    }

    /// A moving key growing the selection: what Shift and it do, for when
    /// Fenix sends Shift.
    pub fn extend(&mut self, key: Key) -> Outcome {
        match self.target(key) {
            Some((to, goal)) => self.go(to, goal, true),
            None => Outcome::Ignored,
        }
    }

    /// A key while it has the keyboard: the arrows, `Home`, `End` and a
    /// word at a time move the caret; typing, `Backspace`, `Delete` and
    /// `Ctrl+Backspace` edit; `Enter` breaks the line, or in one line
    /// submits; `Esc` drops the selection, or cancels.
    pub fn handle_key(&mut self, key: Key) -> Outcome {
        let sel = self.selection();
        match key {
            Key::Left | Key::Right if sel.is_some() => {
                let r = sel.unwrap_or_default();
                let to = if key == Key::Left { r.start } else { r.end };
                self.go(to, None, false)
            }
            Key::Backspace | Key::Delete | Key::DeleteWordBack if sel.is_some() => self.type_in("", Edit::Other),
            Key::Backspace if self.caret > 0 => self.type_in_range(self.caret - 1..self.caret),
            Key::Delete if self.caret < self.len() => self.type_in_range(self.caret..self.caret + 1),
            Key::DeleteWordBack => {
                let (from, _) = self.target(Key::WordLeft).unwrap_or((self.caret, None));
                self.type_in_range(from..self.caret)
            }
            Key::Backspace | Key::Delete => Outcome::Ignored,
            Key::Enter if self.multiline => self.type_in("\n", Edit::Other),
            Key::Enter => Outcome::Submit,
            Key::Escape if sel.is_some() => {
                self.anchor = None;
                Outcome::Moved
            }
            Key::Escape => Outcome::Cancel,
            Key::Char(c) if !c.is_control() => {
                let edit = if c.is_whitespace() { Edit::Other } else { Edit::Typed };
                self.type_in(c.encode_utf8(&mut [0; 4]), edit)
            }
            key => match self.target(key) {
                Some((to, goal)) => self.go(to, goal, false),
                None => Outcome::Ignored,
            },
        }
    }

    fn type_in_range(&mut self, range: Range<usize>) -> Outcome {
        if range.is_empty() {
            return Outcome::Ignored;
        }
        self.replace(range, "", Edit::Other);
        Outcome::Edited
    }

    /// What's drawn: each char with its style, the caret where it is
    /// when focused; and which of them the caret is.
    fn items(&self) -> (Vec<(char, Style)>, Option<usize>) {
        let sel = self.selection();
        let mut out = Vec::new();
        let mut caret = None;
        for (i, c) in self.text.chars().enumerate() {
            if self.focused && i == self.caret {
                caret = Some(out.len());
                out.push((CARET, Style::new(Role::Accent)));
            }
            let style = if sel.as_ref().is_some_and(|r| r.contains(&i)) { Style::new(Role::Accent).bg(Fill::Selection) } else { Style::default() };
            out.push((if c == '\t' { ' ' } else { c }, style));
        }
        if self.focused && self.caret >= self.len() {
            caret = Some(out.len());
            out.push((CARET, Style::new(Role::Accent)));
        }
        (out, caret)
    }

    /// Draws it into `rect`: one line on its first row, cut with `…` and
    /// scrolled to the caret; several wrapped to its width and scrolled a
    /// row at a time, a scrollbar when they don't fit.
    pub fn render(&self, rect: Rect, canvas: &mut Canvas) {
        self.area.set(rect);
        self.view.set(rect.h);
        if rect.is_empty() {
            return;
        }
        let h = if self.multiline { rect.h } else { 1 };
        if self.focused {
            for y in rect.y..rect.y + h {
                canvas.panel(y, rect.x..rect.right());
            }
            canvas.set_typing(true);
        }
        if self.text.is_empty() && !self.placeholder.is_empty() {
            let x = if self.focused { canvas.put(rect.x, rect.y, &CARET.to_string(), Role::Accent) } else { rect.x };
            canvas.put_fit(x, rect.y, rect.right() - x, &self.placeholder, Role::Muted);
            return;
        }
        if self.multiline {
            self.render_lines(rect, canvas);
        } else {
            self.render_line(rect, canvas);
        }
    }

    fn render_line(&self, rect: Rect, canvas: &mut Canvas) {
        let (items, caret) = self.items();
        let mut cols = Vec::with_capacity(items.len());
        let mut total = 0;
        for (c, _) in &items {
            cols.push(total);
            total += char_width(*c);
        }
        let caret_col = caret.map(|i| cols[i]);
        let w = rect.w;
        let mut off = self.scroll.get();
        if total <= w {
            off = 0;
        } else {
            let margin = usize::from(w >= 3);
            if let Some(cc) = caret_col {
                if cc < off + margin {
                    off = cc.saturating_sub(margin);
                }
                if cc + 1 + margin > off + w {
                    off = cc + 1 + margin - w;
                }
            }
            off = off.min(total - w);
        }
        self.scroll.set(off);
        for ((c, style), &col) in items.iter().zip(&cols) {
            let cw = char_width(*c);
            if col >= off && col + cw <= off + w {
                canvas.put(rect.x + col - off, rect.y, c.encode_utf8(&mut [0; 4]), style);
            }
        }
        if off > 0 {
            canvas.put(rect.x, rect.y, "…", Role::Muted);
        }
        if off + w < total {
            canvas.put(rect.right() - 1, rect.y, "…", Role::Muted);
        }
    }

    /// Its items cut into rows of `w` cells, at line breaks and where a
    /// row fills: each row's first item (where one starting empty would
    /// begin) and its items; and the row the caret's on.
    fn rows(items: &[(char, Style)], caret: Option<usize>, w: usize) -> (Vec<(usize, Vec<usize>)>, usize) {
        let mut rows = vec![(0, Vec::new())];
        let mut used = 0;
        let mut caret_row = 0;
        for (i, (c, _)) in items.iter().enumerate() {
            if *c == '\n' {
                rows.push((i + 1, Vec::new()));
                used = 0;
                continue;
            }
            let cw = char_width(*c);
            if used + cw > w && used > 0 {
                rows.push((i, Vec::new()));
                used = 0;
            }
            if caret == Some(i) {
                caret_row = rows.len() - 1;
            }
            rows.last_mut().unwrap().1.push(i);
            used += cw;
        }
        (rows, caret_row)
    }

    /// The rows as last drawn: where they were cut, and whether a
    /// scrollbar took a column.
    fn drawn_rows(&self, items: &[(char, Style)], caret: Option<usize>, rect: Rect) -> (Vec<(usize, Vec<usize>)>, usize) {
        let (rows, caret_row) = Self::rows(items, caret, rect.w);
        if rows.len() > rect.h && rect.w > 1 {
            Self::rows(items, caret, rect.w - 1)
        } else {
            (rows, caret_row)
        }
    }

    fn render_lines(&self, rect: Rect, canvas: &mut Canvas) {
        let (items, caret) = self.items();
        let (rows, caret_row) = self.drawn_rows(&items, caret, rect);
        let bar = rows.len() > rect.h && rect.w > 1;
        let top = if self.focused { top_for(self.scroll.get(), caret_row, rect.h, rows.len()) } else { self.scroll.get().min(rows.len().saturating_sub(rect.h)) };
        self.scroll.set(top);
        for (i, (_, row)) in rows.iter().skip(top).take(rect.h).enumerate() {
            let mut x = rect.x;
            for &at in row {
                let (c, style) = &items[at];
                x = canvas.put(x, rect.y + i, c.encode_utf8(&mut [0; 4]), style);
            }
        }
        if bar {
            scrollbar(canvas, rect.right() - 1, rect.y, rect.h, rows.len(), top);
        }
    }

    /// The char at cell (`x`, `y`) as it was last drawn -- the caret goes
    /// before it -- or the end of the row there; none outside the field.
    pub fn char_at(&self, x: usize, y: usize) -> Option<usize> {
        let rect = self.area.get();
        if !rect.contains(x, y) {
            return None;
        }
        let (items, caret) = self.items();
        // Items to chars: the caret's glyph is no char.
        let to_char = |i: usize| match caret {
            Some(c) if i > c => i - 1,
            Some(c) if i == c => self.caret,
            _ => i,
        };
        // The item whose cells take in column `col` of `row`, or after
        // the row's last.
        let pick = |row: &[usize], col: usize| -> Option<usize> {
            let mut w = 0;
            for &i in row {
                let cw = char_width(items[i].0);
                if col < w + cw {
                    return Some(i);
                }
                w += cw;
            }
            None
        };
        if !self.multiline {
            if y != rect.y {
                return None;
            }
            let all: Vec<usize> = (0..items.len()).collect();
            let col = x - rect.x + self.scroll.get();
            return Some(pick(&all, col).map_or(self.len(), to_char));
        }
        let (rows, _) = self.drawn_rows(&items, caret, rect);
        let r = self.scroll.get() + (y - rect.y);
        let Some((start, row)) = rows.get(r) else { return Some(self.len()) };
        Some(match pick(row, x - rect.x) {
            Some(i) => to_char(i),
            // Past the row's end: after its last char, before its line
            // break.
            None => to_char(row.last().map_or(*start, |last| last + 1)),
        })
    }

    /// A left click at (`x`, `y`): the caret there.
    pub fn click(&mut self, x: usize, y: usize) -> Outcome {
        self.mouse(&Mouse::click(x, y))
    }

    /// The mouse on it: a click puts the caret there (with Shift, the
    /// selection grows to it), a double click selects the word, a triple
    /// all of it; a drag selects to where it is.
    pub fn mouse(&mut self, m: &Mouse) -> Outcome {
        let Some(at) = self.char_at(m.x, m.y) else { return Outcome::Ignored };
        match m.left_clicks() {
            0 if m.is_drag() => self.go(at, None, true),
            0 => Outcome::Ignored,
            1 => self.go(at, None, m.mods.shift),
            2 => {
                let chars = self.chars();
                let (mut from, mut to) = (at, at);
                while from > 0 && is_word(chars[from - 1]) {
                    from -= 1;
                }
                while to < chars.len() && is_word(chars[to]) {
                    to += 1;
                }
                self.select(from..to);
                Outcome::Moved
            }
            _ => {
                self.select_all();
                Outcome::Moved
            }
        }
    }

    /// A key with what's held with it (a page that takes more keys):
    /// Shift and a moving key select; Ctrl+A all of it; Ctrl+Z undoes,
    /// Ctrl+Shift+Z and Ctrl+Y do again; Ctrl+Left, Right and Backspace a
    /// word at a time. Without Ctrl, Alt or the Windows key, `handle_key`.
    pub fn handle_press(&mut self, press: KeyPress) -> Outcome {
        let word = |k: Key| match k {
            Key::Left => Key::WordLeft,
            Key::Right => Key::WordRight,
            Key::Backspace => Key::DeleteWordBack,
            k => k,
        };
        let m = press.mods;
        if press.is_ctrl('a') {
            self.select_all();
            return Outcome::Moved;
        }
        if press.is_ctrl('z') || press.is_ctrl('y') {
            let redo = press.is_ctrl('y') || m.shift;
            let done = if redo { self.redo() } else { self.undo() };
            return if done { Outcome::Edited } else { Outcome::Ignored };
        }
        if m.alt || m.meta {
            return Outcome::Ignored;
        }
        let key = if m.ctrl { word(press.key) } else { press.key };
        if m.ctrl && key == press.key {
            return Outcome::Ignored;
        }
        if m.shift && !matches!(key, Key::Char(_)) && self.target(key).is_some() {
            return self.extend(key);
        }
        self.handle_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mods;

    fn keys(f: &mut TextField, keys: &[Key]) {
        for k in keys {
            f.handle_key(*k);
        }
    }

    fn typed(f: &mut TextField, s: &str) {
        for c in s.chars() {
            f.handle_key(Key::Char(c));
        }
    }

    fn drawn(f: &TextField, w: usize, h: usize) -> Canvas {
        let mut c = Canvas::new();
        f.render(Rect::new(0, 0, w, h), &mut c);
        c
    }

    #[test]
    fn typing_and_moving_a_word_at_a_time() {
        let mut f = TextField::new();
        typed(&mut f, "fix the bug");
        assert_eq!((f.text(), f.caret()), ("fix the bug", 11));
        assert_eq!(f.handle_key(Key::DeleteWordBack), Outcome::Edited);
        assert_eq!((f.text(), f.caret()), ("fix the ", 8));
        f.handle_key(Key::WordLeft);
        assert_eq!(f.caret(), 4);
        f.handle_key(Key::WordLeft);
        assert_eq!(f.caret(), 0);
        assert_eq!(f.handle_key(Key::WordLeft), Outcome::Ignored, "at the start");
        assert_eq!(f.handle_key(Key::Left), Outcome::Ignored);
        f.handle_key(Key::WordRight);
        assert_eq!(f.caret(), 3);
        keys(&mut f, &[Key::Delete, Key::Char('-')]);
        assert_eq!(f.text(), "fix-the ");
        keys(&mut f, &[Key::End, Key::Backspace, Key::Home]);
        assert_eq!((f.text(), f.caret()), ("fix-the", 0));
        assert_eq!(f.handle_key(Key::Backspace), Outcome::Ignored);
        assert_eq!(f.handle_key(Key::Enter), Outcome::Submit);
        assert_eq!(f.handle_key(Key::Escape), Outcome::Cancel);
        assert_eq!(f.handle_key(Key::Up), Outcome::Ignored, "one line: the page's");
        assert_eq!(f.handle_key(Key::Tab), Outcome::Ignored);
    }

    #[test]
    fn a_selection_is_typed_over() {
        let mut f = TextField::new().with_text("hello world");
        f.handle_key(Key::Home);
        f.extend(Key::WordRight);
        assert_eq!(f.selection(), Some(0..5));
        assert_eq!(f.selected_text(), Some("hello"));
        f.extend(Key::Right);
        assert_eq!(f.selected_text(), Some("hello "));
        typed(&mut f, "J");
        assert_eq!(f.text(), "Jworld");
        f.select_all();
        assert_eq!(f.handle_key(Key::Escape), Outcome::Moved, "Esc drops the selection first");
        assert_eq!(f.selection(), None);
        f.select(1..3);
        assert_eq!(f.handle_key(Key::Left), Outcome::Moved);
        assert_eq!((f.caret(), f.selection()), (1, None), "Left goes to its start");
        f.select(1..3);
        f.handle_key(Key::Backspace);
        assert_eq!(f.text(), "Jrld");
    }

    #[test]
    fn a_paste_keeps_to_one_line() {
        let mut f = TextField::new().with_text("ad");
        f.set_caret(1);
        assert_eq!(f.paste("b\r\nc\u{7}"), Outcome::Edited);
        assert_eq!((f.text(), f.caret()), ("ab cd", 4));
        let mut f = TextField::multiline().with_text("ad");
        f.set_caret(1);
        f.paste("b\r\nc\u{7}\t");
        assert_eq!((f.text(), f.caret()), ("ab\nc\td", 5));
        assert_eq!(f.paste(""), Outcome::Ignored);
    }

    #[test]
    fn undo_takes_back_a_run_of_typing() {
        let mut f = TextField::new();
        typed(&mut f, "one two");
        f.handle_key(Key::Backspace);
        assert!(f.undo());
        assert_eq!(f.text(), "one two");
        assert!(f.undo());
        assert_eq!(f.text(), "one ");
        assert!(f.undo());
        assert_eq!(f.text(), "one");
        assert!(f.undo());
        assert_eq!(f.text(), "");
        assert!(!f.undo());
        assert!(f.redo());
        assert_eq!(f.text(), "one");
    }

    #[test]
    fn lines_keep_their_column() {
        let mut f = TextField::multiline().with_text("first line\nab\nthird");
        f.set_caret(8);
        f.handle_key(Key::Down);
        assert_eq!(f.caret(), 13, "the end of the short line");
        f.handle_key(Key::Down);
        assert_eq!(f.caret(), 19, "back to column 8, as far as it goes");
        f.handle_key(Key::Home);
        assert_eq!(f.caret(), 14);
        f.handle_key(Key::Up);
        f.handle_key(Key::End);
        assert_eq!(f.caret(), 13);
        typed(&mut f, "c");
        f.handle_key(Key::Enter);
        typed(&mut f, "x");
        assert_eq!(f.text(), "first line\nabc\nx\nthird");
        f.set_caret(2);
        assert_eq!(f.handle_key(Key::Up), Outcome::Moved);
        assert_eq!(f.caret(), 0, "the first line's start");
        assert_eq!(f.handle_key(Key::Up), Outcome::Ignored);
    }

    #[test]
    fn one_line_is_drawn_with_its_caret_scrolled_into_view() {
        let mut f = TextField::new().with_text("abcdefghijklmnop");
        let c = drawn(&f, 8, 1);
        assert_eq!(c.line(0), "…klmnop▏");
        assert!(c.typing());
        assert_eq!(c.panels(), [(0, 0..8)]);
        f.handle_key(Key::Home);
        let c = drawn(&f, 8, 1);
        assert_eq!(c.line(0), "▏abcdef…");
        f.focused = false;
        let c = drawn(&f, 8, 1);
        assert_eq!(c.line(0), "abcdefg…");
        assert!(!c.typing() && c.panels().is_empty());
        let f = TextField::new().with_placeholder("Search");
        assert_eq!(drawn(&f, 20, 1).line(0), "▏Search");
        let mut f = TextField::new().with_text("日本語");
        f.select(0..1);
        let c = drawn(&f, 20, 1);
        assert_eq!(c.line(0), "日▏本語");
        assert_eq!(c.style_at(0, 0), Style::new(Role::Accent).bg(Fill::Selection));
    }

    #[test]
    fn several_lines_wrap_and_scroll() {
        let mut f = TextField::multiline().with_text("one\ntwo three four\nfive");
        let c = drawn(&f, 10, 5);
        assert_eq!(c.text().trim_end(), "one\ntwo three\nfour\nfive▏");
        f.set_text("1\n2\n3\n4\n5\n6");
        let c = drawn(&f, 6, 3);
        assert_eq!(c.text().trim_end(), "4    │\n5    ┃\n6▏   ┃");
        f.set_caret(0);
        let c = drawn(&f, 6, 3);
        assert_eq!(c.line(0), "▏1   ┃");
        assert_eq!(c.panels().len(), 3);
    }

    #[test]
    fn a_click_puts_the_caret_there() {
        let mut f = TextField::new().with_text("hello world");
        f.focused = false;
        drawn(&f, 20, 1);
        assert_eq!(f.click(3, 0), Outcome::Moved);
        assert_eq!(f.caret(), 3);
        f.focused = true;
        drawn(&f, 20, 1);
        // "hel▏lo world": the caret's glyph takes a cell.
        assert_eq!(f.char_at(3, 0), Some(3));
        assert_eq!(f.char_at(4, 0), Some(3), "on the l after the caret");
        assert_eq!(f.char_at(5, 0), Some(4));
        assert_eq!(f.char_at(19, 0), Some(11), "past the end");
        assert_eq!(f.char_at(2, 1), None, "under it");
        let shift = Mouse { mods: crate::Mods::SHIFT, ..Mouse::click(9, 0) };
        f.mouse(&shift);
        assert_eq!(f.selected_text(), Some("lo wo"), "to the r, drawn a cell on for the caret");
        f.mouse(&Mouse { action: crate::MouseAction::Press { button: crate::Button::Left, clicks: 2 }, ..Mouse::click(9, 0) });
        assert_eq!(f.selected_text(), Some("world"));
        f.mouse(&Mouse { action: crate::MouseAction::Press { button: crate::Button::Left, clicks: 3 }, ..Mouse::click(1, 0) });
        assert_eq!(f.selected_text(), Some("hello world"));
        f.set_caret(0);
        drawn(&f, 20, 1);
        f.mouse(&Mouse { action: crate::MouseAction::Drag(crate::Button::Left), ..Mouse::click(6, 0) });
        assert_eq!(f.selected_text(), Some("hello"), "dragged from the caret");
    }

    #[test]
    fn a_click_in_several_lines() {
        let mut f = TextField::multiline().with_text("one\n\nabcd日gh");
        f.focused = false;
        drawn(&f, 5, 5);
        // one / (empty) / abcd / 日gh: the wide one doesn't fit after abcd.
        assert_eq!(f.char_at(1, 0), Some(1));
        assert_eq!(f.char_at(4, 0), Some(3), "past the end: before the line break");
        assert_eq!(f.char_at(3, 1), Some(4), "an empty line");
        assert_eq!(f.char_at(4, 2), Some(9), "a row cut where it filled: after its last");
        assert_eq!(f.char_at(1, 3), Some(9), "either half of a wide one");
        assert_eq!(f.char_at(2, 3), Some(10));
        assert_eq!(f.char_at(0, 4), Some(12), "below the text");
    }

    #[test]
    fn keys_with_shift_and_ctrl() {
        let mut f = TextField::new().with_text("one two");
        let press = |k, m| KeyPress::new(k, m);
        assert_eq!(f.handle_press(press(Key::Left, crate::Mods::SHIFT)), Outcome::Moved);
        assert_eq!(f.handle_press(press(Key::Left, Mods { shift: true, ctrl: true, ..Mods::NONE })), Outcome::Moved);
        assert_eq!(f.selected_text(), Some("two"), "a word, Shift and Ctrl");
        assert_eq!(f.handle_press(press(Key::Char('a'), crate::Mods::CTRL)), Outcome::Moved);
        assert_eq!(f.selected_text(), Some("one two"));
        f.handle_press(press(Key::Char('X'), crate::Mods::SHIFT));
        assert_eq!(f.text(), "X", "Shift with a letter types it");
        assert_eq!(f.handle_press(press(Key::Char('z'), crate::Mods::CTRL)), Outcome::Edited);
        assert_eq!(f.text(), "one two");
        assert_eq!(f.handle_press(press(Key::Char('Z'), Mods { shift: true, ctrl: true, ..Mods::NONE })), Outcome::Edited);
        assert_eq!(f.text(), "X");
        assert_eq!(f.handle_press(press(Key::Char('q'), crate::Mods::CTRL)), Outcome::Ignored);
        f.set_text("a b");
        f.handle_press(press(Key::Backspace, crate::Mods::CTRL));
        assert_eq!(f.text(), "a ");
    }
}
