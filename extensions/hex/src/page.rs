//! A file's page: its bytes as a hex dump -- each row's offset, its bytes
//! in hex and as text -- the cursor's bytes read as numbers beside or
//! below it, and the keys and the mouse for all of it.

use std::cell::Cell;

use fenix_ui_kit::{Canvas, Fill, Key, KeyPress, Mouse, Role, Style};
use serde::{Deserialize, Serialize};

use crate::data::{Data, Input};
use crate::inspect::{self, Search};

/// Which side of the dump has the cursor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Column {
    #[default]
    Hex,
    Text,
}

/// What a prompt asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    GoTo,
    Search,
}

/// What a key on the page asks of Fenix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Redraw,
    /// Copied, and what's said about it.
    Copy(String, String),
    /// Said, and drawn again.
    Say(String),
    /// A prompt: what for, its label, what's in it.
    Ask(Ask, String, String),
    /// The file read again.
    Reload,
}

/// What a page keeps across a restart.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    /// The file's full path.
    pub full: String,
    /// Where it's read: `/project/...`, or the full path.
    pub path: String,
    pub cursor: u64,
    /// The first row's offset.
    pub top: u64,
    pub column: Column,
    #[serde(default)]
    pub no_inspector: bool,
}

/// The most a copy takes.
const MAX_COPY: u64 = 1024 * 1024;
/// The lines at the top (title, column header) and at the foot (where
/// the cursor is, the keys).
const TOP: usize = 2;
const FOOT: usize = 2;
/// The inspector: a value's width, and the whole of it beside the dump.
const VALUE_W: usize = 22;
const KIND_W: usize = 6;
const SIDE_W: usize = 3 + KIND_W + 2 * VALUE_W;
/// Its lines: a header, its rows, UTF-8.
const INSPECTOR_LINES: usize = 13;

/// How the dump is laid out, as last drawn: what keys and clicks go by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    per_row: usize,
    digits: usize,
    rows: usize,
    side: bool,
    bottom: bool,
}

impl Layout {
    fn hex_x(&self) -> usize {
        self.digits + 2
    }

    fn text_x(&self) -> usize {
        self.hex_x() + hex_width(self.per_row) + 2
    }

    fn width(&self) -> usize {
        self.text_x() + self.per_row
    }
}

/// Where byte `i` of a row is in its hex: a space more each 8.
fn hex_at(i: usize) -> usize {
    i * 3 + i / 8
}

fn hex_width(per_row: usize) -> usize {
    hex_at(per_row - 1) + 2
}

/// Hex digits for an offset into a file of `len` bytes: 8, or as many
/// more as it takes.
fn digits(len: u64) -> usize {
    let need = (64 - len.saturating_sub(1).leading_zeros() as usize).div_ceil(4);
    need.max(8).next_multiple_of(2)
}

/// `n` with a comma each three digits.
pub fn count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A size as it's said: bytes, KiB, MiB, GiB.
pub fn size(n: u64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{:.1} KiB", n as f64 / 1024.0),
        n if n < 1024 * 1024 * 1024 => format!("{:.1} MiB", n as f64 / (1024.0 * 1024.0)),
        n => format!("{:.1} GiB", n as f64 / (1024.0 * 1024.0 * 1024.0)),
    }
}

/// A byte's colour: nul muted, text plain, control characters and the
/// rest picked out.
fn role(b: u8) -> Role {
    match b {
        0 => Role::Muted,
        0x20..=0x7e => Role::Text,
        0x01..=0x1f | 0x7f => Role::Good,
        _ => Role::Accent,
    }
}

/// A byte in the text column: itself when it's printable ASCII.
fn shown(b: u8) -> char {
    if (0x20..=0x7e).contains(&b) {
        b as char
    } else {
        '.'
    }
}

/// How a copy is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum As {
    Hex,
    Text,
    Code,
}

pub struct HexPage {
    pub full: String,
    pub path: String,
    pub data: Result<Data, String>,
    pub cursor: u64,
    pub column: Column,
    /// Where a selection started (`v`, a drag, Shift).
    pub anchor: Option<u64>,
    /// The last match: its offset and length.
    pub found: Option<(u64, u64)>,
    pub search: Option<Search>,
    pub inspector: bool,
    /// `hex.bytesPerRow`: none fits the pane.
    pub per_row: Option<usize>,
    /// Where the mouse went down.
    pressed: Option<u64>,
    top: Cell<u64>,
    layout: Cell<Layout>,
}

impl HexPage {
    pub fn new(full: &str, path: &str, input: Result<Input, String>) -> HexPage {
        HexPage {
            full: full.to_string(),
            path: path.to_string(),
            data: input.map(Data::new),
            cursor: 0,
            column: Column::Hex,
            anchor: None,
            found: None,
            search: None,
            inspector: true,
            per_row: Some(16),
            pressed: None,
            top: Cell::new(0),
            layout: Cell::new(Layout { per_row: 16, digits: 8, rows: 20, side: false, bottom: false }),
        }
    }

    /// The file's name.
    pub fn name(&self) -> String {
        self.full.rsplit(['/', '\\']).next().unwrap_or(&self.full).to_string()
    }

    pub fn saved(&self) -> Saved {
        Saved { full: self.full.clone(), path: self.path.clone(), cursor: self.cursor, top: self.top.get(), column: self.column, no_inspector: !self.inspector }
    }

    pub fn restore(&mut self, s: &Saved) {
        let last = self.last();
        self.cursor = s.cursor.min(last);
        self.top.set(s.top.min(last));
        self.column = s.column;
        self.inspector = !s.no_inspector;
    }

    /// The file read again (`R`): where it was, as far as it still goes.
    pub fn reload(&mut self, input: Result<Input, String>) {
        let s = self.saved();
        self.data = input.map(Data::new);
        self.anchor = None;
        self.found = None;
        self.restore(&s);
    }

    fn len(&self) -> u64 {
        self.data.as_ref().map_or(0, Data::len)
    }

    /// The last byte's offset.
    fn last(&self) -> u64 {
        self.len().saturating_sub(1)
    }

    /// The bytes selected, first and last; else the cursor's.
    fn chosen(&self) -> (u64, u64) {
        match self.anchor {
            Some(a) => (a.min(self.cursor), a.max(self.cursor)),
            None => (self.cursor, self.cursor),
        }
    }

    fn layout(&self, w: usize, h: usize) -> Layout {
        let digits = digits(self.len());
        let fits = |n: usize| Layout { per_row: n, digits, rows: 0, side: false, bottom: false }.width();
        let per_row = match self.per_row {
            Some(n) if n > 0 => n.min(64),
            _ => {
                let room = if self.inspector { w.saturating_sub(SIDE_W) } else { w };
                [32, 16, 8].into_iter().find(|&n| fits(n) <= room).or_else(|| [32, 16, 8].into_iter().find(|&n| fits(n) <= w)).unwrap_or(8)
            }
        };
        let side = self.inspector && fits(per_row) + SIDE_W <= w;
        let body = h.saturating_sub(TOP + FOOT);
        let bottom = self.inspector && !side && body >= INSPECTOR_LINES + 1 + 6;
        let rows = if bottom { body - INSPECTOR_LINES - 1 } else { body }.max(1);
        Layout { per_row, digits, rows, side, bottom }
    }

    pub fn render(&self, width: usize, height: usize) -> Canvas {
        let mut c = Canvas::new();
        let (w, h) = (width.max(30), height.max(10));
        let x = c.put(0, 0, &self.name(), Role::Title);
        let d = match &self.data {
            Ok(d) => d,
            Err(e) => {
                c.put_fit(0, 2, w, &format!("Not read: {e}"), Role::Bad);
                c.put_fit(0, 4, w, "R reads it again", Role::Muted);
                return c;
            }
        };
        let l = self.layout(w, h);
        self.layout.set(l);
        let len = d.len();
        let per = l.per_row as u64;
        let about = format!("  {} bytes{} · {} a row", count(len), if len >= 1024 { format!(" ({})", size(len)) } else { String::new() }, l.per_row);
        c.put_fit(x, 0, w.saturating_sub(x), &about, Role::Muted);
        if len == 0 {
            c.put_fit(0, 2, w, "Empty: there are no bytes to show", Role::Muted);
            return c;
        }

        // The rows shown: the cursor's among them.
        let total = len.div_ceil(per);
        let cursor_row = self.cursor / per;
        let mut top = self.top.get() / per;
        if cursor_row < top {
            top = cursor_row;
        } else if cursor_row >= top + l.rows as u64 {
            top = cursor_row + 1 - l.rows as u64;
        }
        top = top.min(total.saturating_sub(l.rows as u64)).min(cursor_row);
        self.top.set(top * per);

        // The column header: each byte's place in its row.
        let in_row = (self.cursor % per) as usize;
        c.put(0, 1, &format!("{:<w$}", "offset", w = l.digits), Role::Muted);
        for i in 0..l.per_row {
            let role = if i == in_row { Role::Accent } else { Role::Muted };
            c.put(l.hex_x() + hex_at(i), 1, &format!("{:02x}", i % 256), role.clone());
            c.put(l.text_x() + i, 1, &format!("{:x}", i % 16), role);
        }

        let (sel_from, sel_to) = self.chosen();
        let selected = |at: u64| self.anchor.is_some() && (sel_from..=sel_to).contains(&at);
        let matched = |at: u64| self.found.is_some_and(|(f, n)| (f..f + n).contains(&at));
        let first = top * per;
        let bytes = d.read(first, l.rows * l.per_row);
        for (r, row) in bytes.chunks(l.per_row).enumerate() {
            let y = TOP + r;
            let offset = first + r as u64 * per;
            let at_role = if offset / per == cursor_row { Role::Accent } else { Role::Muted };
            c.put(0, y, &format!("{offset:0w$x}", w = l.digits), at_role);
            for (i, &b) in row.iter().enumerate() {
                let at = offset + i as u64;
                let mut style = Style::new(role(b));
                if selected(at) {
                    style = style.bg(Fill::Selection);
                } else if matched(at) {
                    style = style.bg(Fill::Warn);
                }
                let (mut hex, mut text) = (style.clone(), style);
                if at == self.cursor {
                    match self.column {
                        Column::Hex => {
                            hex = hex.bg(Fill::Focus);
                            text = text.underline();
                        }
                        Column::Text => {
                            text = text.bg(Fill::Focus);
                            hex = hex.underline();
                        }
                    }
                }
                c.put(l.hex_x() + hex_at(i), y, &format!("{b:02x}"), hex);
                c.put(l.text_x() + i, y, &shown(b).to_string(), text);
            }
        }

        if l.side {
            self.inspect(&mut c, l.width() + 3, 1, d);
        } else if l.bottom {
            self.inspect(&mut c, 0, TOP + l.rows + 1, d);
        }

        // Where the cursor is, what's selected or found; the keys.
        let (y, keys) = (h - 2, h - 1);
        if let Some(e) = d.error() {
            c.put_fit(0, y, w, &format!("Not read: {e}"), Role::Bad);
        } else {
            let pct = if len > 1 { self.cursor * 100 / (len - 1) } else { 100 };
            let mut at = format!("0x{:x} · {} of {} · {pct}%", self.cursor, count(self.cursor), count(len));
            if self.anchor.is_some() {
                let n = sel_to - sel_from + 1;
                at += &format!(" · {} selected, 0x{sel_from:x}-0x{sel_to:x}", if n == 1 { "1 byte".to_string() } else { format!("{} bytes", count(n)) });
            } else if let (Some((f, _)), Some(s)) = (self.found, &self.search) {
                at += &format!(" · {} at 0x{f:x}", s.said);
            }
            c.put_fit(0, y, w, &at, Role::Text);
        }
        let hint = "hjkl move · o go to · / search, n N · v select, x clear · y hex, Y text, c code · Tab hex/text · i inspector";
        c.put_fit(0, keys, w, hint, Role::Muted);
        c
    }

    /// The inspector at (`x`, `y`): the cursor's bytes as numbers, both
    /// ways round, and as UTF-8.
    fn inspect(&self, c: &mut Canvas, x: usize, y: usize, d: &Data) {
        let bytes = d.read(self.cursor, 8);
        let (le, be) = (x + KIND_W, x + KIND_W + VALUE_W);
        c.put(le, y, "little-endian", Role::Muted);
        c.put(be, y, "big-endian", Role::Muted);
        let value = |c: &mut Canvas, x: usize, y: usize, v: &Option<String>| match v {
            Some(v) => c.put_fit(x, y, VALUE_W - 1, v, Role::Text),
            None => c.put(x, y, "-", Role::Muted),
        };
        for (i, row) in inspect::rows(&bytes).iter().enumerate() {
            let y = y + 1 + i;
            c.put(x, y, row.kind, Role::Muted);
            if matches!(row.kind, "u8" | "i8" | "bits") {
                match &row.le {
                    Some(v) => c.put_fit(le, y, 2 * VALUE_W - 1, v, Role::Text),
                    None => c.put(le, y, "-", Role::Muted),
                };
            } else {
                value(c, le, y, &row.le);
                value(c, be, y, &row.be);
            }
        }
        let y = y + INSPECTOR_LINES - 1;
        c.put(x, y, "UTF-8", Role::Muted);
        c.put_fit(le, y, 2 * VALUE_W - 1, &inspect::utf8(&bytes), Role::Text);
    }

    /// Move to `to`, as far as the file goes.
    fn go(&mut self, to: u64) -> Action {
        self.cursor = to.min(self.last());
        Action::Redraw
    }

    pub fn key(&mut self, key: Key) -> Action {
        if self.data.is_err() || self.len() == 0 {
            return if key == Key::Char('R') { Action::Reload } else { Action::Nothing };
        }
        let l = self.layout.get();
        let (per, page) = (l.per_row as u64, (l.per_row * l.rows.max(1)) as u64);
        let (cur, last) = (self.cursor, self.last());
        let row = cur - cur % per;
        match key {
            Key::Left | Key::Char('h') => self.go(cur.saturating_sub(1)),
            Key::Right | Key::Char('l') => self.go(cur + 1),
            Key::Up | Key::Char('k') => self.go(cur.checked_sub(per).unwrap_or(cur)),
            // To the last row, a short one, from a byte past its end too.
            Key::Down | Key::Char('j') if row + per <= last => self.go(cur + per),
            Key::Down | Key::Char('j') => Action::Nothing,
            Key::Home | Key::Char('0') => self.go(row),
            Key::End | Key::Char('$') => self.go(row + per - 1),
            Key::Char('w') => self.go((cur / 8 + 1) * 8),
            Key::Char('b') => self.go(cur.saturating_sub(1) / 8 * 8),
            Key::Char('g') => self.go(0),
            Key::Char('G') => self.go(last),
            Key::PageDown | Key::Char('J') => {
                self.top.set(self.top.get().saturating_add(page).min(last - last % per));
                self.go(cur.saturating_add(page))
            }
            Key::PageUp | Key::Char('K') => {
                self.top.set(self.top.get().saturating_sub(page));
                self.go(cur.saturating_sub(page))
            }
            Key::Tab | Key::BackTab => {
                self.column = if self.column == Column::Hex { Column::Text } else { Column::Hex };
                Action::Redraw
            }
            Key::Char('v') => {
                self.anchor = if self.anchor.is_some() { None } else { Some(cur) };
                Action::Redraw
            }
            Key::Char('x') => {
                self.anchor = None;
                self.found = None;
                Action::Redraw
            }
            Key::Char('i') => {
                self.inspector = !self.inspector;
                Action::Redraw
            }
            // `o`, not `:`: Fenix keeps `:` for its command line on every page.
            Key::Char('o') => Action::Ask(Ask::GoTo, "Go to offset (0x1f0, 496, 50%, +16)".into(), String::new()),
            Key::Char('/') => {
                let label = match self.column {
                    Column::Hex => "Search: hex bytes (de ad ?? ef), or \"text\"",
                    Column::Text => "Search the text",
                };
                Action::Ask(Ask::Search, label.into(), String::new())
            }
            Key::Char('n') => self.next(cur + 1, true),
            Key::Char('N') => self.next(cur, false),
            Key::Char('*') => {
                let (from, to) = self.chosen();
                let Ok(d) = &self.data else { return Action::Nothing };
                let bytes = d.read(from, (to - from + 1).min(256) as usize);
                let said = bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
                self.search = Some(Search { pattern: bytes.into_iter().map(Some).collect(), said });
                self.anchor = None;
                self.next(from + 1, true)
            }
            Key::Char('y') => self.copy(As::Hex),
            Key::Char('Y') => self.copy(As::Text),
            Key::Char('c') => self.copy(As::Code),
            Key::Char('R') => Action::Reload,
            _ => Action::Nothing,
        }
    }

    /// The keys `key` doesn't get: Ctrl+D and Ctrl+U a half page, Ctrl+F
    /// and Ctrl+B a page, Shift with a move selects.
    pub fn press(&mut self, press: KeyPress) -> Action {
        let l = self.layout.get();
        let half = (l.per_row * (l.rows / 2).max(1)) as u64;
        if self.data.is_err() || self.len() == 0 {
            return Action::Nothing;
        }
        if press.is_ctrl('d') {
            return self.go(self.cursor.saturating_add(half));
        }
        if press.is_ctrl('u') {
            return self.go(self.cursor.saturating_sub(half));
        }
        if press.is_ctrl('f') {
            return self.key(Key::PageDown);
        }
        if press.is_ctrl('b') {
            return self.key(Key::PageUp);
        }
        let moves = matches!(press.key, Key::Left | Key::Right | Key::Up | Key::Down | Key::Home | Key::End | Key::PageUp | Key::PageDown);
        if press.mods.shift && moves {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
            return self.key(press.key);
        }
        match press.key {
            Key::PageUp | Key::PageDown if !press.mods.chord() => self.key(press.key),
            _ => Action::Nothing,
        }
    }

    /// The match after `from` (`forward`), or the last before it.
    fn next(&mut self, from: u64, forward: bool) -> Action {
        let (Ok(d), Some(s)) = (&self.data, &self.search) else {
            return Action::Say("Nothing searched for yet: / searches".into());
        };
        match d.find(&s.pattern, from, forward) {
            Some(at) => {
                let round = if forward { at < from } else { at >= from };
                self.cursor = at;
                self.found = Some((at, s.pattern.len() as u64));
                if round {
                    Action::Say(format!("{}: went round the {}", s.said, if forward { "end" } else { "start" }))
                } else {
                    Action::Redraw
                }
            }
            None => {
                self.found = None;
                Action::Say(format!("{} isn't in the file", s.said))
            }
        }
    }

    /// What was typed at a prompt.
    pub fn answer(&mut self, ask: Ask, typed: &str) -> Action {
        match ask {
            Ask::GoTo => match inspect::offset(typed, self.cursor, self.len()) {
                Ok(at) => {
                    let to = at.min(self.last());
                    self.go(to);
                    if at > to {
                        Action::Say(format!("The file ends at 0x{to:x}"))
                    } else {
                        Action::Redraw
                    }
                }
                Err(e) => Action::Say(e),
            },
            Ask::Search => match inspect::search(typed, self.column == Column::Text) {
                Ok(s) => {
                    self.search = Some(s);
                    self.anchor = None;
                    self.next(self.cursor, true)
                }
                Err(e) => Action::Say(e),
            },
        }
    }

    /// The selection, or the cursor's byte, copied.
    fn copy(&self, how: As) -> Action {
        let Ok(d) = &self.data else { return Action::Nothing };
        let (from, to) = self.chosen();
        let n = to - from + 1;
        if n > MAX_COPY {
            return Action::Say(format!("{} bytes is too many to copy: at most {}", count(n), size(MAX_COPY)));
        }
        let bytes = d.read(from, n as usize);
        let many = if n == 1 { "1 byte".to_string() } else { format!("{} bytes", count(n)) };
        match how {
            As::Hex => Action::Copy(bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "), format!("Copied {many} as hex")),
            As::Code => Action::Copy(bytes.iter().map(|b| format!("0x{b:02x}")).collect::<Vec<_>>().join(", "), format!("Copied {many} as code")),
            As::Text => match String::from_utf8(bytes) {
                Ok(text) => Action::Copy(text, format!("Copied {many} as text")),
                Err(e) => Action::Copy(String::from_utf8_lossy(e.as_bytes()).into_owned(), format!("Copied {many} as text, what isn't UTF-8 as \u{fffd}")),
            },
        }
    }

    pub fn mouse(&mut self, m: &Mouse) -> Action {
        if self.data.is_err() || self.len() == 0 {
            return Action::Nothing;
        }
        if let Some(lines) = m.wheel() {
            let by = lines.unsigned_abs() as u64 * 3 * self.layout.get().per_row as u64;
            let top = self.top.get();
            self.top.set(if lines > 0 { top.saturating_sub(by) } else { top.saturating_add(by) });
            return self.go(if lines > 0 { self.cursor.saturating_sub(by) } else { self.cursor.saturating_add(by) });
        }
        let Some((at, column)) = self.hit(m.x, m.y) else { return Action::Nothing };
        if m.left_clicks() > 0 {
            if m.mods.shift {
                self.anchor.get_or_insert(self.cursor);
            } else {
                self.anchor = None;
            }
            self.pressed = Some(at);
        } else if m.is_drag() {
            if let Some(p) = self.pressed {
                self.anchor = (p != at).then_some(p);
            }
        } else {
            return Action::Nothing;
        }
        self.column = column;
        self.go(at)
    }

    /// The byte at cell (`x`, `y`), and its column.
    fn hit(&self, x: usize, y: usize) -> Option<(u64, Column)> {
        let l = self.layout.get();
        let row = y.checked_sub(TOP).filter(|&r| r < l.rows)?;
        let i = if (l.text_x()..l.text_x() + l.per_row).contains(&x) {
            (x - l.text_x(), Column::Text)
        } else {
            let hx = x.checked_sub(l.hex_x())?;
            ((0..l.per_row).find(|&i| (hex_at(i)..hex_at(i) + 2).contains(&hx))?, Column::Hex)
        };
        let at = self.top.get() + (row * l.per_row + i.0) as u64;
        (at < self.len()).then_some((at, i.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(bytes: &[u8]) -> HexPage {
        HexPage::new("/x/a.bin", "/x/a.bin", Ok(Input::Bytes(bytes.to_vec())))
    }

    fn sample() -> Vec<u8> {
        let mut b: Vec<u8> = (0..=255).collect();
        b.extend_from_slice(b"Hello, hex!\x00\x01\xde\xad\xbe\xef");
        b
    }

    #[test]
    fn a_row_is_its_offset_its_bytes_and_their_text() {
        let p = page(&sample());
        let c = p.render(140, 30);
        assert!(c.line(0).starts_with("a.bin  273 bytes · 16 a row"), "{}", c.line(0));
        assert!(c.line(1).starts_with("offset    00 01 02 03 04 05 06 07  08 09"), "{}", c.line(1));
        let row = c.line(TOP + 4);
        assert!(row.starts_with("00000040  40 41 42 43 44 45 46 47  48 49 4a 4b 4c 4d 4e 4f  @ABCDEFGHIJKLMNO"), "{row}");
        let row = c.line(TOP);
        assert!(row.contains("  ................"), "non-printables are dots: {row}");
        let last = c.line(TOP + 17);
        assert!(last.starts_with("00000110  ef  "), "a short last row: {last}");
        assert!(c.text().contains("little-endian"), "the inspector is beside it");
    }

    #[test]
    fn keys_move_by_a_byte_a_row_and_a_page() {
        let mut p = page(&sample());
        p.render(140, 30);
        p.key(Key::Char('l'));
        p.key(Key::Char('j'));
        assert_eq!(p.cursor, 17);
        p.key(Key::Char('$'));
        assert_eq!(p.cursor, 31);
        p.key(Key::Char('0'));
        assert_eq!(p.cursor, 16);
        p.key(Key::Char('G'));
        assert_eq!(p.cursor, 272);
        assert_eq!(p.key(Key::Char('j')), Action::Nothing);
        p.key(Key::Char('k'));
        assert_eq!(p.cursor, 256);
        p.key(Key::Char('$'));
        assert_eq!(p.cursor, 271, "a short row's end is the file's");
        p.key(Key::Char('g'));
        p.key(Key::PageDown);
        assert_eq!(p.cursor, 272, "a page past the end is the end");
        p.press(KeyPress::new(Key::Char('u'), fenix_ui_kit::Mods::CTRL));
        assert_eq!(p.cursor, 272 - 16 * 13, "half of 26 rows");
    }

    #[test]
    fn a_page_down_stops_at_the_end_and_a_big_file_scrolls() {
        let mut p = page(&vec![7u8; 100_000]);
        p.render(100, 20);
        for _ in 0..10_000 {
            p.key(Key::PageDown);
        }
        assert_eq!(p.cursor, 99_999);
        let c = p.render(100, 20);
        assert!(c.text().contains("00018690  07"), "the last row is shown");
        p.answer(Ask::GoTo, "0x100");
        assert_eq!(p.cursor, 0x100);
        assert!(p.render(100, 20).text().contains("00000100"));
        assert_eq!(p.answer(Ask::GoTo, "0xffffff"), Action::Say("The file ends at 0x1869f".into()));
    }

    #[test]
    fn a_search_finds_bytes_or_text_and_goes_round() {
        let mut p = page(&sample());
        p.render(140, 30);
        assert_eq!(p.answer(Ask::Search, "6c 6c"), Action::Redraw);
        assert_eq!(p.cursor, 258);
        assert_eq!(p.answer(Ask::Search, "de ad"), Action::Redraw);
        assert_eq!(p.cursor, 269, "not 0xde: 0xdf follows it");
        assert_eq!(p.key(Key::Char('n')), Action::Say("de ad: went round the end".into()));
        assert_eq!(p.cursor, 269);
        assert_eq!(p.answer(Ask::Search, "0x41 ?? 43"), Action::Say("41 ?? 43: went round the end".into()));
        assert_eq!(p.cursor, 0x41);
        p.answer(Ask::Search, "\"hex\"");
        assert_eq!(p.cursor, 263);
        assert!(p.render(140, 30).text().contains("\"hex\" at 0x107"));
        assert_eq!(p.answer(Ask::Search, "ca fe"), Action::Say("ca fe isn't in the file".into()));
        p.key(Key::Tab);
        p.key(Key::Char('g'));
        p.answer(Ask::Search, "Hello");
        assert_eq!(p.cursor, 256, "the text column searches text");
    }

    #[test]
    fn a_selection_copies_as_hex_text_or_code() {
        let mut p = page(&sample());
        p.render(140, 30);
        p.answer(Ask::GoTo, "256");
        p.key(Key::Char('v'));
        p.key(Key::Char('l'));
        p.key(Key::Char('l'));
        p.key(Key::Char('l'));
        p.key(Key::Char('l'));
        assert_eq!(p.key(Key::Char('y')), Action::Copy("48 65 6c 6c 6f".into(), "Copied 5 bytes as hex".into()));
        assert_eq!(p.key(Key::Char('Y')), Action::Copy("Hello".into(), "Copied 5 bytes as text".into()));
        assert_eq!(p.key(Key::Char('c')), Action::Copy("0x48, 0x65, 0x6c, 0x6c, 0x6f".into(), "Copied 5 bytes as code".into()));
        p.key(Key::Char('x'));
        assert_eq!(p.key(Key::Char('y')), Action::Copy("6f".into(), "Copied 1 byte as hex".into()), "x clears the selection");
        p.key(Key::Char('G'));
        assert_eq!(p.key(Key::Char('y')), Action::Copy("ef".into(), "Copied 1 byte as hex".into()));
        let Action::Copy(text, said) = p.key(Key::Char('Y')) else { panic!() };
        assert_eq!(text, "\u{fffd}");
        assert!(said.contains("isn't UTF-8"));
        p.press(KeyPress::new(Key::Left, fenix_ui_kit::Mods::SHIFT));
        assert_eq!(p.chosen(), (271, 272), "Shift with a move selects");
        p.key(Key::Char('*'));
        assert_eq!(p.found, Some((271, 2)), "* searches the selection: only itself here");
    }

    #[test]
    fn the_inspector_reads_the_cursors_bytes() {
        let mut p = page(&sample());
        p.render(140, 30);
        p.answer(Ask::GoTo, "0x10d");
        let text = p.render(140, 30).text();
        assert!(text.contains("u32   3735928559") || text.contains("3735928559"), "{text}");
        assert!(text.contains("u16"), "{text}");
        p.key(Key::Char('i'));
        assert!(!p.render(140, 30).text().contains("little-endian"));
        p.key(Key::Char('i'));
        assert!(p.render(80, 40).text().contains("little-endian"), "below, when it isn't room beside");
    }

    #[test]
    fn bytes_per_row_is_set_or_fits_the_pane() {
        let mut p = page(&sample());
        p.per_row = Some(8);
        assert!(p.render(140, 30).line(TOP + 1).starts_with("00000008  08 09 0a 0b 0c 0d 0e 0f  ........"));
        p.per_row = None;
        p.inspector = false;
        assert!(p.render(200, 30).line(0).contains("32 a row"));
        assert!(p.render(80, 30).line(0).contains("16 a row"));
    }

    #[test]
    fn a_click_puts_the_cursor_on_a_byte_and_a_drag_selects() {
        let mut p = page(&sample());
        p.render(140, 30);
        let l = p.layout.get();
        p.mouse(&Mouse::click(l.hex_x() + hex_at(9), TOP + 2));
        assert_eq!((p.cursor, p.column), (41, Column::Hex));
        p.mouse(&Mouse { action: fenix_ui_kit::MouseAction::Drag(fenix_ui_kit::Button::Left), ..Mouse::click(l.text_x() + 12, TOP + 2) });
        assert_eq!(p.chosen(), (41, 44));
        assert_eq!(p.column, Column::Text);
    }

    #[test]
    fn a_page_comes_back_where_it_was() {
        let mut p = page(&sample());
        p.render(140, 30);
        p.answer(Ask::GoTo, "0x100");
        p.key(Key::Tab);
        p.key(Key::Char('i'));
        let mut again = page(&sample());
        again.restore(&p.saved());
        assert_eq!(again.saved(), p.saved());
        assert_eq!((again.cursor, again.column, again.inspector), (0x100, Column::Text, false));
    }

    #[test]
    fn an_empty_or_unread_file_says_so() {
        let mut p = page(b"");
        assert!(p.render(80, 20).text().contains("Empty"));
        assert_eq!(p.key(Key::Char('j')), Action::Nothing);
        let mut p = HexPage::new("/x/b.bin", "/x/b.bin", Err("too big".into()));
        assert!(p.render(80, 10).text().contains("Not read: too big"));
        assert_eq!(p.key(Key::Char('R')), Action::Reload);
    }

    #[test]
    fn offsets_take_as_many_digits_as_the_file_needs() {
        assert_eq!(digits(0), 8);
        assert_eq!(digits(1 << 32), 8);
        assert_eq!(digits((1 << 32) + 1), 10);
        assert_eq!(count(1_234_567), "1,234,567");
        assert_eq!(size(3 * 1024 * 1024), "3.0 MiB");
    }
}
