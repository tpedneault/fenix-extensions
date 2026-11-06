//! A log's page: its entries a row each, coloured by level, a stack
//! trace folded into the entry it follows and opened with Enter (a JSON
//! line pretty-printed), the levels and filters above with their counts,
//! a row of bars for the entries over time -- and the keys and the mouse
//! for all of it.
//!
//! The page only lays out what it shows: the rows on screen are read
//! from their chunks as they're drawn. Indexing and filtering go on in
//! steps (`work`) between frames.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use fenix_ui_kit::{text, Canvas, Fill, Key, KeyPress, Mouse, Role, Style};
use serde::{Deserialize, Serialize};

use crate::detect::{Level, Rules, CAP};
use crate::entry::{scan, Entry, MAX_BYTES};
use crate::filter::{counted, next_error, Field, Filter, Matcher, Pass, Range, TOGGLES};
use crate::hist::{self, Histogram};
use crate::index::{Index, STEP};
use crate::source::Source;
use crate::time;

/// The lines above the rows: the title, the levels, the filters, the
/// bars and their times, a rule.
pub const HEAD: usize = 6;
/// How long a call of work goes on: a few milliseconds, so the frame
/// waiting on it isn't late.
const WORK_TIME: std::time::Duration = std::time::Duration::from_millis(8);
/// The lines below: where the cursor is, the keys.
const FOOT: usize = 2;
/// What of a line is read for its row.
const SHOWN: usize = 4096;
/// The most a copy takes.
const MAX_COPY: usize = 4 * 1024 * 1024;
/// The chunks kept read for the rows.
const KEPT_CHUNKS: usize = 8;
/// The opened entries kept laid out.
const KEPT_OPENED: usize = 16;

/// What a prompt asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Text,
    Search,
    Include,
    Exclude,
    Field,
    Time,
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
    /// The file's text opened at a line (from 1).
    Open(u32),
    /// The file read again from its start.
    Reload,
    /// Following turned on or off.
    Follow(bool),
}

/// What a page keeps across a restart.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub full: String,
    pub path: String,
    #[serde(default)]
    pub entry: u64,
    #[serde(default)]
    pub follow: bool,
    #[serde(default)]
    pub levels: Option<[bool; 6]>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub range: Option<Range>,
}

/// The bars as drawn: their line, first column, how many, and the times
/// they span.
type Bars = (usize, usize, usize, i64, i64);

/// A chunk's entries as read for the rows, with each one's first line.
struct Scanned {
    key: (u64, u64, u64, u64),
    first_entry: u64,
    entries: Vec<(Entry, u64)>,
}

/// A level's colour.
pub fn role(level: Level) -> Role {
    match level {
        Level::Fatal | Level::Error => Role::Bad,
        Level::Warn => Role::Warn,
        Level::Debug | Level::Trace => Role::Muted,
        Level::Info | Level::Other => Role::Text,
    }
}

/// Whether a column's `some` of `of` (its errors, its warnings) are
/// picked out: any at all while they're rare in the log (under 5% of it),
/// else where there are half as many again as in the log as a whole.
fn stands_out(some: u32, of: u32, total: u64, all: u64) -> bool {
    if some == 0 || all == 0 {
        return false;
    }
    if total * 20 < all {
        return true;
    }
    (some as u64) * all * 2 >= (of as u64) * total * 3
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

/// A line's bytes as the row shows them: what isn't UTF-8 as `�`, a tab
/// a space, other controls `·`.
fn clean(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| {
            if c == '\t' {
                ' '
            } else if c.is_control() {
                '·'
            } else {
                c
            }
        })
        .collect()
}

/// The first line of `bytes`, without its newline.
fn first_line(bytes: &[u8]) -> &[u8] {
    let line = &bytes[..memchr::memchr(b'\n', bytes).unwrap_or(bytes.len())];
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// An entry's text as it's opened: a JSON line pretty-printed, the rest
/// of its lines as they are.
pub fn opened(bytes: &[u8]) -> Vec<String> {
    let head = first_line(bytes);
    let rest = bytes.get(head.len()..).unwrap_or_default();
    let rest = rest.strip_prefix(b"\r").unwrap_or(rest);
    let rest = rest.strip_prefix(b"\n").unwrap_or(rest);
    let mut out: Vec<String> = match (head.first(), serde_json::from_slice::<serde_json::Value>(head)) {
        (Some(b'{'), Ok(v)) if v.is_object() => serde_json::to_string_pretty(&v).unwrap_or_default().lines().map(clean_str).collect(),
        _ => vec![clean(head)],
    };
    if !rest.is_empty() {
        let rest = rest.strip_suffix(b"\n").unwrap_or(rest);
        out.extend(rest.split(|&b| b == b'\n').map(|l| clean(l.strip_suffix(b"\r").unwrap_or(l))));
    }
    out
}

fn clean_str(s: &str) -> String {
    clean(s.as_bytes())
}

pub struct LogPage {
    pub full: String,
    pub path: String,
    pub src: Result<Source, String>,
    pub index: Index,
    pub pass: Pass,
    /// The row the cursor is on, and the lines of its entry scrolled past
    /// when it's open and taller than the pane.
    pub cursor: u64,
    inner: usize,
    /// An entry to put the cursor on once it's worked out: where it was
    /// before a filter changed, or the session's.
    anchor: Option<u64>,
    pub opened: HashSet<u64>,
    pub follow: bool,
    pub hscroll: usize,
    /// Said under the rows until the next key.
    note: Option<String>,
    top: Cell<u64>,
    body: Cell<usize>,
    /// Where each row drawn is: its line, its row.
    drawn: RefCell<Vec<(usize, u64)>>,
    /// The bars as drawn: their line, first column, how many, and the
    /// times they span.
    bars: Cell<Option<Bars>>,
    chunks: RefCell<Vec<Scanned>>,
    laid: RefCell<HashMap<u64, Rc<Vec<String>>>>,
}

impl LogPage {
    pub fn new(full: &str, path: &str, src: Result<Source, String>, rules: Rc<Rules>) -> LogPage {
        let len = src.as_ref().map_or(0, Source::len);
        let index = Index::new(rules, len);
        let pass = Pass::new(Filter::default(), None, &index);
        LogPage {
            full: full.to_string(),
            path: path.to_string(),
            src,
            index,
            pass,
            cursor: 0,
            inner: 0,
            anchor: None,
            opened: HashSet::new(),
            follow: false,
            hscroll: 0,
            note: None,
            top: Cell::new(0),
            body: Cell::new(20),
            drawn: RefCell::new(Vec::new()),
            bars: Cell::new(None),
            chunks: RefCell::new(Vec::new()),
            laid: RefCell::new(HashMap::new()),
        }
    }

    /// The file's name.
    pub fn name(&self) -> String {
        self.full.rsplit(['/', '\\']).next().unwrap_or(&self.full).to_string()
    }

    pub fn saved(&self) -> Saved {
        let f = &self.pass.filter;
        Saved {
            full: self.full.clone(),
            path: self.path.clone(),
            entry: self.entry_at(self.cursor).unwrap_or(0),
            follow: self.follow,
            levels: Some(f.levels),
            text: f.text.as_ref().map(|m| m.said.clone()),
            search: self.pass.search.as_ref().map(|m| m.said.clone()),
            include: f.include.iter().map(|m| m.said.clone()).collect(),
            exclude: f.exclude.iter().map(|m| m.said.clone()).collect(),
            fields: f.fields.clone(),
            range: f.range.clone(),
        }
    }

    pub fn restore(&mut self, s: &Saved) {
        let mut f = Filter { levels: s.levels.unwrap_or(self.pass.filter.levels), ..Filter::default() };
        f.text = s.text.as_deref().and_then(|t| Matcher::new(t).ok());
        f.include = s.include.iter().filter_map(|t| Matcher::new(t).ok()).collect();
        f.exclude = s.exclude.iter().filter_map(|t| Matcher::new(t).ok()).collect();
        f.fields = s.fields.clone();
        f.range = s.range.clone();
        let search = s.search.as_deref().and_then(|t| Matcher::new(t).ok());
        self.pass = Pass::new(f, search, &self.index);
        self.follow = s.follow;
        self.anchor = (!s.follow).then_some(s.entry);
    }

    /// The levels shown when it opens (`logs.levels`).
    pub fn set_levels(&mut self, levels: [bool; 6]) {
        let mut f = self.pass.filter.clone();
        f.levels = levels;
        self.refilter(f, self.pass.search.clone());
    }

    /// The file read again from its start, with what's set kept.
    pub fn reload(&mut self, src: Result<Source, String>) {
        let len = src.as_ref().map_or(0, Source::len);
        self.anchor = self.entry_at(self.cursor);
        self.src = src;
        self.index.restart(len);
        self.forget();
    }

    /// What was read for the rows let go of, and the filter worked out
    /// again: the index started again.
    fn forget(&mut self) {
        self.pass = Pass::new(self.pass.filter.clone(), self.pass.search.clone(), &self.index);
        self.chunks.borrow_mut().clear();
        self.laid.borrow_mut().clear();
        self.opened.clear();
        self.inner = 0;
    }

    /// Whether there's indexing or filtering still to do.
    pub fn busy(&self) -> bool {
        self.src.is_ok() && (!self.index.done || !self.pass.finished(&self.index))
    }

    /// Steps of indexing and of filtering, for a few milliseconds: what a
    /// call between frames does. Whether there's more.
    pub fn work(&mut self) -> bool {
        let Ok(src) = &self.src else { return false };
        let rows = self.pass.rows(&self.index);
        let at_end = self.cursor + 1 >= rows;
        // Slices of the file until the call has had its few
        // milliseconds: a frame waits for it at most that long.
        let start = std::time::Instant::now();
        let (mut more, mut filtering);
        loop {
            more = self.index.step(src, STEP);
            filtering = self.pass.step(&self.index, src, STEP);
            if !(more || filtering) || start.elapsed() >= WORK_TIME {
                break;
            }
        }
        if self.follow && at_end && self.anchor.is_none() {
            self.cursor = self.pass.rows(&self.index).saturating_sub(1);
            self.inner = 0;
        }
        if let Some(a) = self.anchor {
            // Placed once what's before it is worked out.
            let row = self.pass.row_of(&self.index, a);
            if row < self.pass.rows(&self.index) || !self.busy() {
                self.cursor = row.min(self.pass.rows(&self.index).saturating_sub(1));
                self.anchor = None;
            }
        }
        more || filtering
    }

    /// The file looked at again, as `F` follows it: `bytes` is it read
    /// again whole when it's from outside the project. Whether it changed.
    pub fn poll(&mut self, bytes: Option<Vec<u8>>) -> bool {
        let Ok(src) = &mut self.src else { return false };
        match bytes {
            Some(b) => src.replace(b),
            None => {
                if let Err(e) = src.refresh() {
                    self.note = Some(format!("Not read: {e}"));
                    return false;
                }
            }
        }
        if self.index.changed(src) {
            // Truncated or rotated: read again, from the start.
            let len = src.len();
            self.index.restart(len);
            self.forget();
            self.cursor = 0;
            self.note = Some("The file was truncated or replaced: read again from its start".into());
            return true;
        }
        if src.len() > self.index.len {
            self.index.grow(src.len());
            self.chunks.borrow_mut().retain(|s| s.key.3 != 1);
            return true;
        }
        false
    }

    /// A new filter or search, the cursor kept on its entry (or the next
    /// shown) once it's worked out.
    fn refilter(&mut self, filter: Filter, search: Option<Matcher>) {
        let at = self.entry_at(self.cursor);
        self.pass = Pass::new(filter, search, &self.index);
        self.anchor = at.or(self.anchor);
        self.inner = 0;
        if self.pass.trivial() {
            self.work_anchor();
        }
    }

    fn work_anchor(&mut self) {
        if let Some(a) = self.anchor.take() {
            self.cursor = self.pass.row_of(&self.index, a).min(self.pass.rows(&self.index).saturating_sub(1));
        }
    }

    fn rows(&self) -> u64 {
        self.pass.rows(&self.index)
    }

    /// The entry on row `row`.
    fn entry_at(&self, row: u64) -> Option<u64> {
        self.pass.entry(&self.index, row)
    }

    /// The entries of the chunk entry `e` is in, read if they aren't kept:
    /// the entry, and its first line's number (from 0).
    fn info(&self, e: u64) -> Option<(Entry, u64)> {
        let src = self.src.as_ref().ok()?;
        let i = self.index.chunk_of(e)?;
        let v = self.index.chunk(i)?;
        let c = v.chunk;
        let key = (self.index.generation, c.first_entry, c.end, u64::from(v.eof || i == self.index.closed_chunks().len()));
        let mut kept = self.chunks.borrow_mut();
        if let Some(p) = kept.iter().position(|s| s.key == key) {
            let s = kept.remove(p);
            kept.push(s);
        } else {
            let mut entries = Vec::with_capacity(c.entries as usize);
            let mut line = c.first_line;
            scan(src, c.offset..c.end, v.eof, self.index.grouping(), &self.index.rules, false, |en, _| {
                entries.push((*en, line));
                line += en.lines as u64;
            });
            if kept.len() >= KEPT_CHUNKS {
                kept.remove(0);
            }
            kept.push(Scanned { key, first_entry: c.first_entry, entries });
        }
        let s = kept.last()?;
        s.entries.get((e - s.first_entry) as usize).copied()
    }

    /// An entry's first line, as much as a row shows.
    fn head(&self, en: &Entry) -> Vec<u8> {
        let Ok(src) = &self.src else { return Vec::new() };
        let bytes = src.read(en.offset, (en.len as usize).min(SHOWN));
        if bytes.starts_with(b"__CURSOR=") {
            let whole = src.read(en.offset, (en.len as usize).min(CAP));
            if let Some(s) = crate::detect::journal_summary(&whole) {
                return s.into_bytes();
            }
        }
        first_line(&bytes).to_vec()
    }

    /// An entry's lines as it's shown open.
    fn laid_out(&self, e: u64, en: &Entry) -> Rc<Vec<String>> {
        if let Some(l) = self.laid.borrow().get(&e) {
            return l.clone();
        }
        let bytes = self.src.as_ref().map(|s| s.read(en.offset, (en.len as usize).min(MAX_BYTES as usize + CAP))).unwrap_or_default();
        let lines = Rc::new(opened(&bytes));
        let mut laid = self.laid.borrow_mut();
        if laid.len() >= KEPT_OPENED {
            laid.clear();
        }
        laid.insert(e, lines.clone());
        lines
    }

    /// The lines row `row` takes.
    fn height(&self, row: u64) -> usize {
        let Some(e) = self.entry_at(row) else { return 1 };
        if !self.opened.contains(&e) {
            return 1;
        }
        self.info(e).map_or(1, |(en, _)| self.laid_out(e, &en).len().max(1))
    }

    /// The first row shown, `cursor`'s among those shown.
    fn scroll(&self, cursor: u64, body: usize) -> u64 {
        // Each row takes a line at least.
        let mut top = self.top.get().min(cursor).max((cursor + 1).saturating_sub(body as u64));
        while top < cursor {
            let mut used = self.height(cursor).min(body);
            let mut r = top;
            while r < cursor && used <= body {
                used += self.height(r);
                r += 1;
            }
            if used <= body {
                break;
            }
            top += 1;
        }
        self.top.set(top);
        top
    }

    pub fn render(&self, width: usize, height: usize) -> Canvas {
        let mut c = Canvas::new();
        let (w, h) = (width.max(40), height.max(HEAD + FOOT + 3));
        let x = c.put(0, 0, &self.name(), Role::Title);
        let src = match &self.src {
            Ok(s) => s,
            Err(e) => {
                c.put_fit(0, 2, w, &format!("Not read: {e}"), Role::Bad);
                c.put_fit(0, 4, w, "R reads it again", Role::Muted);
                return c;
            }
        };
        let (ix, pass) = (&self.index, &self.pass);
        let rows = pass.rows(ix);

        // The title: the file, its entries, what's under way.
        let many = |n: u64, one: &str, more: &str| format!("{} {}", count(n), if n == 1 { one } else { more });
        let mut about = format!("  {} · {} · {}", size(src.len()), many(ix.entries(), "entry", "entries"), many(ix.lines(), "line", "lines"));
        if ix.entries() == 0 && ix.done {
            about = format!("  {} · no entries", size(src.len()));
        }
        let x = c.put_fit(x, 0, w.saturating_sub(x), &about, Role::Muted);
        let mut state = Vec::new();
        if !ix.done {
            state.push(format!("indexing {:.0}%", ix.progress() * 100.0));
        } else if !pass.finished(ix) {
            state.push(format!("filtering {:.0}%", pass.progress(ix) * 100.0));
        }
        if self.follow {
            state.push("following".into());
        }
        if !state.is_empty() {
            c.put_fit(x, 0, w.saturating_sub(x), &format!(" · {}", state.join(" · ")), Role::Accent);
        }

        // The levels, each with its key and count: their names and
        // whether they're off when there's room, else the counts alone.
        let counts = counted(&ix.counts());
        let levels = [Level::Error, Level::Warn, Level::Info, Level::Debug, Level::Trace, Level::Other];
        let words = |long: bool, i: usize| {
            let on = pass.filter.levels[i];
            match (long, on) {
                (true, true) => format!(" {} {}", count(counts[i]), levels[i].name()),
                (true, false) => format!(" {} {} (off)", count(counts[i]), levels[i].name()),
                (false, _) => format!(" {}", count(counts[i])),
            }
        };
        let long = (0..TOGGLES.len()).map(|i| 1 + text::width(&words(true, i)) + 3).sum::<usize>() <= w + 3;
        let mut x = 0;
        for (i, key) in TOGGLES.iter().enumerate() {
            let on = pass.filter.levels[i];
            let r = match levels[i] {
                _ if !on => Role::Muted,
                Level::Info | Level::Other => Role::Text,
                l => role(l),
            };
            x = c.put(x, 1, &key.to_string(), Style::new(Role::Accent).underline());
            x = c.put(x, 1, &words(long, i), r);
            x += if long { 3 } else { 2 };
        }

        // The filters, and what they let through.
        let said = pass.filter.said();
        if pass.trivial() {
            c.put_fit(0, 2, w, "/ text · + - patterns · = field · @ time · ? search", Role::Muted);
        } else {
            let mut line = format!("{} of {} shown", count(rows), count(ix.entries()));
            if !said.is_empty() {
                line += &format!(" · {said}");
            }
            let x = c.put_fit(0, 2, w, &line, Role::Text);
            if let Some(s) = &pass.search {
                c.put_fit(x, 2, w.saturating_sub(x), &format!(" · ?{} {} found", s.said, count(pass.found())), Role::Accent);
            }
        }

        // The bars: entries over time, those with errors red.
        self.bars.set(None);
        match ix.time_range() {
            Some((from, to)) if to > from => {
                let cols = w.saturating_sub(2);
                let whole = ix.hist();
                let h: &Histogram = if pass.reads() { &pass.hist } else { &whole };
                let counted = h.columns(from, to, cols);
                let most = counted.iter().map(|c| c[0]).max().unwrap_or(0);
                let (all, warns, errors) = counted.iter().fold((0u64, 0u64, 0u64), |t, n| (t.0 + n[0] as u64, t.1 + n[1] as u64, t.2 + n[2] as u64));
                for (i, n) in counted.iter().enumerate() {
                    let r = if stands_out(n[2], n[0], errors, all) {
                        Role::Bad
                    } else if stands_out(n[1], n[0], warns, all) {
                        Role::Warn
                    } else {
                        Role::Text
                    };
                    c.put(1 + i, 3, &hist::bar(n[0], most).to_string(), r);
                }
                self.bars.set(Some((3, 1, cols, from, to)));
                let (a, z) = (time::format(from), time::format(to));
                let mid = format!("{} a column", time::span(Histogram::column_width(from, to, cols)));
                c.put(1, 4, &a, Role::Muted);
                let zx = (w.saturating_sub(1 + text::width(&z))).max(text::width(&a) + 2);
                c.put(zx, 4, &z, Role::Muted);
                let mx = (w.saturating_sub(text::width(&mid))) / 2;
                if mx > text::width(&a) + 2 && mx + text::width(&mid) + 2 < zx {
                    c.put(mx, 4, &mid, Role::Muted);
                }
            }
            Some((t, _)) => {
                c.put_fit(1, 4, w, &format!("{} -- every entry at the same time", time::format(t)), Role::Muted);
            }
            None => {
                c.put_fit(1, 4, w, "No times in this log", Role::Muted);
            }
        }
        c.rule(HEAD - 1, 0..w);

        // The rows.
        let body = h - HEAD - FOOT;
        self.body.set(body);
        let cursor = self.cursor.min(rows.saturating_sub(1));
        let digits = count(ix.lines().max(1)).len().max(4);
        let gutter = digits + 4;
        let mut drawn = Vec::new();
        if rows == 0 {
            let why = if ix.entries() == 0 {
                if ix.done {
                    "Empty: there's nothing in it yet"
                } else {
                    "Reading..."
                }
            } else if pass.finished(ix) {
                "Nothing shown: x clears the filters, a shows every level"
            } else {
                "Filtering..."
            };
            c.put_fit(0, HEAD, w, why, Role::Muted);
        } else {
            let top = self.scroll(cursor, body);
            let mut y = HEAD;
            let mut row = top;
            while y < HEAD + body && row < rows {
                let Some(e) = self.entry_at(row) else { break };
                let Some((en, line)) = self.info(e) else { break };
                let focused = row == cursor;
                let r = role(en.level);
                let open = self.opened.contains(&e);
                let lines: Vec<String> = if open { self.laid_out(e, &en).to_vec() } else { vec![clean(&self.head(&en))] };
                let skip = if focused && row == top { self.inner.min(lines.len().saturating_sub(1)) } else { 0 };
                for (k, text) in lines.iter().enumerate().skip(skip) {
                    if y >= HEAD + body {
                        break;
                    }
                    drawn.push((y, row));
                    if k == 0 {
                        c.put(0, y, &format!("{:>digits$}", count(line + 1)), Role::Muted);
                        c.put(digits + 1, y, &en.level.badge().to_string(), Style::new(r.clone()));
                        let fold = if en.lines > 1 || open && lines.len() > 1 {
                            if open {
                                "▾"
                            } else {
                                "▸"
                            }
                        } else {
                            " "
                        };
                        c.put(digits + 2, y, fold, Role::Muted);
                    }
                    let room = w.saturating_sub(gutter);
                    self.put_text(&mut c, gutter, y, room, text, if k == 0 || en.level <= Level::Warn { r.clone() } else { Role::Text });
                    if k == 0 && !open && en.lines > 1 {
                        let more = format!(" +{} lines", en.lines - 1);
                        let end = gutter + text::width(&text::slice(text, self.hscroll, room));
                        if end + text::width(&more) <= w {
                            c.put(end, y, &more, Role::Muted);
                        }
                    }
                    // The entry's first line shown is the focused row.
                    if focused && k == skip {
                        c.focus(y, 0..w);
                    }
                    y += 1;
                }
                row += 1;
            }
        }
        *self.drawn.borrow_mut() = drawn;

        // Where the cursor is; the keys.
        let (y, keys) = (h - 2, h - 1);
        if let Some(n) = &self.note {
            c.put_fit(0, y, w, n, Role::Warn);
        } else if let Some(e) = src.error() {
            c.put_fit(0, y, w, &format!("Not read: {e}"), Role::Bad);
        } else if rows > 0 {
            if let Some((en, line)) = self.entry_at(cursor).and_then(|e| self.info(e)) {
                let mut at = format!("row {} of {} · line {}", count(cursor + 1), count(rows), count(line + 1));
                if en.lines > 1 {
                    at += &format!("-{}", count(line + en.lines as u64));
                }
                at += &format!(" · {}", en.level.name());
                if let Some(t) = en.time {
                    at += &format!(" · {}", time::format(t));
                }
                c.put_fit(0, y, w, &at, Role::Text);
            }
        }
        let hint = "jk move · Enter open · e w i d t o levels · / filter, + - = @ · ? search, n N · ] [ errors · F follow · y Y copy · r raw · x clear";
        c.put_fit(0, keys, w, hint, Role::Muted);
        c
    }

    /// A line of an entry at (`x`, `y`) in `w` cells, scrolled across,
    /// what the filter's text and the search find in it picked out.
    fn put_text(&self, c: &mut Canvas, x: usize, y: usize, w: usize, line: &str, r: Role) {
        let mut marks: Vec<(std::ops::Range<usize>, Fill)> = Vec::new();
        if let Some(m) = &self.pass.filter.text {
            marks.extend(m.find(line.as_bytes()).into_iter().map(|r| (r, Fill::Accent)));
        }
        if let Some(m) = &self.pass.search {
            marks.extend(m.find(line.as_bytes()).into_iter().map(|r| (r, Fill::Warn)));
        }
        let mut col = 0;
        let end = self.hscroll + w;
        for (at, ch) in line.char_indices() {
            let cw = text::char_width(ch);
            if col >= end {
                break;
            }
            if col >= self.hscroll && col + cw <= end {
                let mut style = Style::new(r.clone());
                if let Some((_, fill)) = marks.iter().rev().find(|(m, _)| m.contains(&at)) {
                    style = style.bg(*fill);
                }
                c.put(x + col - self.hscroll, y, &ch.to_string(), style);
            }
            col += cw;
        }
        if col > end {
            c.put(x + w - 1, y, "…", Role::Muted);
        }
    }

    /// Move the cursor to `row`, as far as the rows go.
    fn go(&mut self, row: u64) -> Action {
        let rows = self.rows();
        self.cursor = row.min(rows.saturating_sub(1));
        self.inner = 0;
        Action::Redraw
    }

    fn go_entry(&mut self, e: u64) -> Action {
        let row = self.pass.row_of(&self.index, e);
        self.go(row)
    }

    pub fn key(&mut self, key: Key) -> Action {
        self.note = None;
        if self.src.is_err() {
            return if key == Key::Char('R') { Action::Reload } else { Action::Nothing };
        }
        let rows = self.rows();
        let page = self.body.get().max(2) as u64 - 1;
        let cur = self.cursor.min(rows.saturating_sub(1));
        match key {
            Key::Down | Key::Char('j') => {
                // Through an open entry taller than the pane first.
                let body = self.body.get();
                if self.top.get() == cur && self.height(cur) > body + self.inner {
                    self.inner += 1;
                    return Action::Redraw;
                }
                self.go(cur + 1)
            }
            Key::Up | Key::Char('k') => {
                if self.inner > 0 {
                    self.inner -= 1;
                    return Action::Redraw;
                }
                self.go(cur.saturating_sub(1))
            }
            Key::Char('g') | Key::Home => self.go(0),
            Key::Char('G') | Key::End => self.go(rows.saturating_sub(1)),
            Key::PageDown | Key::Char('J') => {
                self.top.set(self.top.get() + page);
                self.go(cur + page)
            }
            Key::PageUp | Key::Char('K') => {
                self.top.set(self.top.get().saturating_sub(page));
                self.go(cur.saturating_sub(page))
            }
            Key::Char('h') | Key::Left => {
                self.hscroll = self.hscroll.saturating_sub(8);
                Action::Redraw
            }
            Key::Char('l') | Key::Right => {
                self.hscroll += 8;
                Action::Redraw
            }
            Key::Char('0') => {
                self.hscroll = 0;
                Action::Redraw
            }
            Key::Enter | Key::Tab => {
                let Some(e) = self.entry_at(cur) else { return Action::Nothing };
                if !self.opened.remove(&e) {
                    self.opened.insert(e);
                }
                self.inner = 0;
                Action::Redraw
            }
            Key::Char('C') => {
                self.opened.clear();
                self.inner = 0;
                Action::Redraw
            }
            Key::Char(k) if TOGGLES.contains(&k) => {
                let i = TOGGLES.iter().position(|&t| t == k).unwrap_or(0);
                let mut f = self.pass.filter.clone();
                f.levels[i] = !f.levels[i];
                self.refilter(f, self.pass.search.clone());
                Action::Redraw
            }
            Key::Char('a') => {
                let mut f = self.pass.filter.clone();
                f.levels = [true; 6];
                self.refilter(f, self.pass.search.clone());
                Action::Redraw
            }
            Key::Char('x') => {
                let levels = self.pass.filter.levels;
                self.refilter(Filter { levels, ..Filter::default() }, None);
                Action::Redraw
            }
            Key::Char('/') => Action::Ask(
                Ask::Text,
                "Filter: text, or re:regex (empty clears it)".into(),
                self.pass.filter.text.as_ref().map(|m| m.said.clone()).unwrap_or_default(),
            ),
            Key::Char('?') => Action::Ask(
                Ask::Search,
                "Search: text, or re:regex (empty clears it)".into(),
                self.pass.search.as_ref().map(|m| m.said.clone()).unwrap_or_default(),
            ),
            Key::Char('+') => Action::Ask(Ask::Include, "Show only entries with (empty clears these)".into(), String::new()),
            Key::Char('-') => Action::Ask(Ask::Exclude, "Hide entries with (empty clears these)".into(), String::new()),
            Key::Char('=') => Action::Ask(Ask::Field, "Field: key=value, key!=value, key~value (empty clears them)".into(), String::new()),
            Key::Char('@') => Action::Ask(
                Ask::Time,
                "Time: 12:00..12:30, 2024-03-01 12:00.., -15m (empty clears it)".into(),
                self.pass.filter.range.as_ref().map(|r| r.said.clone()).unwrap_or_default(),
            ),
            Key::Char('n') | Key::Char('N') => {
                let forward = key == Key::Char('n');
                if self.pass.search.is_some() {
                    return self.next_found(forward);
                }
                self.next_error(forward)
            }
            Key::Char(']') => self.next_error(true),
            Key::Char('[') => self.next_error(false),
            Key::Char('F') => {
                self.follow = !self.follow;
                if self.follow {
                    self.go(rows.saturating_sub(1));
                }
                Action::Follow(self.follow)
            }
            Key::Char('y') => self.copy(false),
            Key::Char('Y') => self.copy(true),
            Key::Char('r') => match self.entry_at(cur).and_then(|e| self.info(e)) {
                Some((_, line)) => Action::Open(line as u32 + 1),
                None => Action::Nothing,
            },
            Key::Char('R') => Action::Reload,
            _ => Action::Nothing,
        }
    }

    /// The keys `key` doesn't get: Ctrl+D and Ctrl+U a half page, Ctrl+F
    /// and Ctrl+B a page, PageUp and PageDown.
    pub fn press(&mut self, press: KeyPress) -> Action {
        let half = (self.body.get() / 2).max(1) as u64;
        if press.is_ctrl('d') {
            return self.go(self.cursor + half);
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
        match press.key {
            Key::PageUp | Key::PageDown if !press.mods.chord() => self.key(press.key),
            _ => Action::Nothing,
        }
    }

    fn next_found(&mut self, forward: bool) -> Action {
        if !self.pass.finished(&self.index) && self.pass.found() == 0 {
            return Action::Say("Still searching".into());
        }
        let cur = self.entry_at(self.cursor).unwrap_or(0);
        match self.pass.next_found(&self.index, cur, forward) {
            Some(e) => {
                let round = if forward { e <= cur } else { e >= cur };
                self.go_entry(e);
                if round {
                    Action::Say(format!("Went round the {}", if forward { "end" } else { "start" }))
                } else {
                    Action::Redraw
                }
            }
            None => Action::Say(format!("{} isn't in what's shown", self.pass.search.as_ref().map_or("", |s| s.said.as_str()))),
        }
    }

    fn next_error(&mut self, forward: bool) -> Action {
        let cur = self.entry_at(self.cursor).unwrap_or(0);
        match next_error(&self.index, &self.pass, cur, forward) {
            Some(e) => self.go_entry(e),
            None => Action::Say(format!("No error {} this one", if forward { "after" } else { "before" })),
        }
    }

    /// The cursor's line, or its whole entry (a JSON line pretty-printed).
    fn copy(&self, whole: bool) -> Action {
        let Some((en, line)) = self.entry_at(self.cursor).and_then(|e| self.info(e)) else { return Action::Nothing };
        let Ok(src) = &self.src else { return Action::Nothing };
        let bytes = src.read(en.offset, (en.len as usize).min(MAX_COPY));
        let cut = if en.len as usize > MAX_COPY { " (its first 4 MiB)" } else { "" };
        if whole {
            let text = if en.lines == 1 && bytes.first() == Some(&b'{') {
                opened(&bytes).join("\n")
            } else {
                String::from_utf8_lossy(bytes.strip_suffix(b"\n").unwrap_or(&bytes)).into_owned()
            };
            Action::Copy(text, format!("Copied the entry at line {}{cut}", count(line + 1)))
        } else {
            Action::Copy(String::from_utf8_lossy(first_line(&bytes)).into_owned(), format!("Copied line {}{cut}", count(line + 1)))
        }
    }

    /// What was typed at a prompt.
    pub fn answer(&mut self, ask: Ask, typed: &str) -> Action {
        let typed = typed.trim();
        let mut f = self.pass.filter.clone();
        let mut search = self.pass.search.clone();
        let matcher = |t: &str| if t.is_empty() { Ok(None) } else { Matcher::new(t).map(Some) };
        let result: Result<(), String> = (|| {
            match ask {
                Ask::Text => f.text = matcher(typed)?,
                Ask::Search => search = matcher(typed)?,
                Ask::Include => match matcher(typed)? {
                    Some(m) => f.include.push(m),
                    None => f.include.clear(),
                },
                Ask::Exclude => match matcher(typed)? {
                    Some(m) => f.exclude.push(m),
                    None => f.exclude.clear(),
                },
                Ask::Field => {
                    if typed.is_empty() {
                        f.fields.clear();
                    } else {
                        f.fields.push(Field::new(typed)?);
                    }
                }
                Ask::Time => {
                    f.range = if typed.is_empty() { None } else { Some(Range::new(typed, self.index.time_range().map(|r| r.1), self.index.rules.year)?) };
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            return Action::Say(e);
        }
        let searched = ask == Ask::Search && search.is_some();
        self.refilter(f, search);
        if searched {
            self.note = Some("n and N go to what it finds".into());
        }
        Action::Redraw
    }

    pub fn mouse(&mut self, m: &Mouse) -> Action {
        if let Some(lines) = m.wheel() {
            let by = lines.unsigned_abs() as u64 * 3;
            return if lines > 0 { self.go(self.cursor.saturating_sub(by)) } else { self.go(self.cursor + by) };
        }
        let clicks = m.left_clicks();
        if clicks == 0 {
            return Action::Nothing;
        }
        if let Some((y, x0, cols, from, to)) = self.bars.get() {
            if m.y == y && m.x >= x0 {
                // A bar: the first entry at its time.
                let t = from + ((m.x - x0) as f64 / cols.max(1) as f64 * (to - from) as f64) as i64;
                return self.go_time(t);
            }
        }
        let hit = self.drawn.borrow().iter().find(|(y, _)| *y == m.y).map(|(_, r)| *r);
        let Some(row) = hit else { return Action::Nothing };
        if row != self.cursor {
            self.go(row);
        }
        if clicks >= 2 {
            return self.key(Key::Enter);
        }
        Action::Redraw
    }

    /// The cursor on the first entry shown at `t` or after.
    pub fn go_time(&mut self, t: i64) -> Action {
        let n = self.index.chunk_count();
        let first = (0..n).filter_map(|i| self.index.chunk(i)).find(|v| v.chunk.times.is_some_and(|(_, z)| z >= t));
        let Some(v) = first else { return self.go(self.rows().saturating_sub(1)) };
        let c = v.chunk;
        let at = (c.first_entry..c.first_entry + c.entries)
            .find(|&e| self.info(e).is_some_and(|(en, _)| en.time.is_some_and(|et| et >= t)))
            .unwrap_or(c.first_entry);
        self.go_entry(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(text: &str) -> LogPage {
        let mut p = LogPage::new("/x/app.log", "/x/app.log", Ok(Source::bytes(text.as_bytes().to_vec())), Rc::new(Rules { custom: Vec::new(), year: 2024 }));
        while p.work() {}
        p
    }

    fn java() -> LogPage {
        page(include_str!("samples/java.log"))
    }

    #[test]
    fn the_page_shows_counts_levels_bars_and_rows() {
        let p = java();
        let c = p.render(120, 20);
        assert!(c.line(0).starts_with("app.log  556 B · 4 entries · 10 lines"), "{}", c.line(0));
        assert_eq!(c.line(1).trim_end(), "e 1 error   w 1 warn   i 1 info   d 1 debug   t 0 trace   o 0 other");
        assert_eq!(p.render(50, 20).line(1).trim_end(), "e 1  w 1  i 1  d 1  t 0  o 0", "narrow: the counts alone");
        assert!(c.line(2).starts_with("/ text"), "{}", c.line(2));
        assert!(c.line(3).contains('█'), "the bars: {}", c.line(3));
        assert!(c.line(4).starts_with(" 2024-03-01 12:00:01.100") && c.line(4).trim_end().ends_with("2024-03-01 12:00:04.500"), "{}", c.line(4));
        let rows: Vec<String> = (HEAD..HEAD + 4).map(|y| c.line(y).trim_end().to_string()).collect();
        assert_eq!(rows[0], "   1 I  2024-03-01 12:00:01,100 [main] INFO  com.acme.App - Starting on port 8080");
        assert!(rows[1].starts_with("   2 E▸ 2024-03-01 12:00:02,250 [http-1] ERROR"), "{}", rows[1]);
        assert!(rows[1].ends_with("+6 lines"), "{}", rows[1]);
        assert!(rows[2].starts_with("   9 W "), "{}", rows[2]);
        assert!(rows[3].starts_with("  10 D "), "{}", rows[3]);
        assert!(c.line(18).starts_with("row 1 of 4 · line 1 · info · 2024-03-01 12:00:01.100"), "{}", c.line(18));
        assert_eq!(c.style_at(10, HEAD + 1).fg, Role::Bad, "an error is red");
        assert_eq!(c.style_at(10, HEAD + 3).fg, Role::Muted, "debug is dim");
    }

    #[test]
    fn enter_opens_an_entry_and_json_is_pretty() {
        let mut p = java();
        p.key(Key::Char('j'));
        p.key(Key::Enter);
        let c = p.render(100, 20);
        assert!(c.line(HEAD + 1).starts_with("   2 E▾"), "{}", c.line(HEAD + 1));
        assert_eq!(c.line(HEAD + 2).trim_end(), "        java.lang.IllegalStateException: order 42 is closed");
        assert_eq!(c.line(HEAD + 3).trim_end(), "         at com.acme.orders.Order.update(Order.java:88)");
        assert_eq!(c.line(HEAD + 7).trim_end(), "         ... 12 more");
        assert!(c.line(HEAD + 8).starts_with("   9 W"));
        assert!(c.line(18).contains("line 2-8"), "{}", c.line(18));

        let mut p = page(include_str!("samples/app.jsonl"));
        p.key(Key::Char('j'));
        p.key(Key::Enter);
        let c = p.render(100, 30);
        let opened: Vec<String> = (HEAD + 1..HEAD + 12).map(|y| c.line(y).trim_end().to_string()).collect();
        assert!(opened[0].contains("   2 E▾ {"), "{opened:#?}");
        assert_eq!(opened[1], "          \"time\": \"2024-03-01T12:00:02Z\",");
        assert!(opened.iter().any(|l| l.trim() == "\"name\": \"bob\""), "nested and in order: {opened:#?}");
        let Action::Copy(text, _) = p.key(Key::Char('Y')) else { panic!() };
        assert!(text.starts_with("{\n  \"time\""), "{text}");
        let Action::Copy(text, said) = p.key(Key::Char('y')) else { panic!() };
        assert!(text.starts_with("{\"time\":\"2024-03-01T12:00:02Z\""));
        assert_eq!(said, "Copied line 2");
    }

    #[test]
    fn level_keys_filter_and_errors_are_jumped_to() {
        let mut p = page(include_str!("samples/rust.log"));
        assert_eq!(p.rows(), 7);
        p.key(Key::Char('i'));
        p.key(Key::Char('d'));
        p.key(Key::Char('t'));
        while p.work() {}
        assert_eq!(p.rows(), 3, "warn, error, the panic");
        let c = p.render(100, 20);
        assert!(c.line(1).contains("i 2 info (off)"), "{}", c.line(1));
        assert!(c.line(2).starts_with("3 of 7 shown"), "{}", c.line(2));
        p.key(Key::Char('a'));
        while p.work() {}
        p.key(Key::Char('g'));
        p.key(Key::Char(']'));
        assert_eq!(p.entry_at(p.cursor), Some(2));
        p.key(Key::Char('n'));
        assert_eq!(p.entry_at(p.cursor), Some(6), "n is the next error without a search: the panic");
        assert_eq!(p.key(Key::Char('n')), Action::Say("No error after this one".into()));
        p.key(Key::Char('['));
        assert_eq!(p.entry_at(p.cursor), Some(2));
    }

    #[test]
    fn filters_and_a_search_are_typed_and_said() {
        let mut p = page(include_str!("samples/logfmt.log"));
        p.answer(Ask::Field, "user=bob");
        while p.work() {}
        assert_eq!(p.rows(), 1);
        p.answer(Ask::Field, "");
        p.answer(Ask::Exclude, "retrying");
        p.answer(Ask::Text, "re:msg=\"?(server|gave)");
        while p.work() {}
        assert_eq!(p.rows(), 2);
        let c = p.render(120, 20);
        assert!(c.line(2).starts_with("2 of 4 shown · /re:msg=\"?(server|gave)/ -retrying"), "{}", c.line(2));
        p.key(Key::Char('x'));
        p.answer(Ask::Search, "bob");
        while p.work() {}
        assert_eq!(p.rows(), 4);
        assert!(p.render(120, 20).line(2).contains("?bob 1 found"));
        p.key(Key::Char('g'));
        p.key(Key::Char('n'));
        assert_eq!(p.cursor, 2);
        let c = p.render(120, 20);
        let at = c.line(HEAD + 2).find("bob").unwrap();
        assert_eq!(c.style_at(at, HEAD + 2).bg, Some(Fill::Warn), "what's found is marked");
        assert_eq!(p.answer(Ask::Time, "whenever"), Action::Say("whenever: not a time -- 12:00, 2024-03-01 12:00:03, -15m".into()));
        p.answer(Ask::Time, "12:00:02..12:00:03");
        while p.work() {}
        assert_eq!(p.rows(), 2);
        assert!(matches!(p.answer(Ask::Text, "re:("), Action::Say(_)));
    }

    #[test]
    fn following_reads_what_was_added_and_starts_again_when_truncated() {
        let mut p = java();
        p.follow = true;
        p.key(Key::Char('G'));
        let more = format!("{}2024-03-01 12:00:05,000 [main] ERROR com.acme.App - late\n", include_str!("samples/java.log"));
        assert!(p.poll(Some(more.into_bytes())));
        while p.work() {}
        assert_eq!(p.rows(), 5);
        assert_eq!(p.cursor, 4, "the cursor stays at the end");
        assert!(!p.poll(Some(p.src.as_ref().unwrap().read(0, 10_000))), "nothing new");
        p.answer(Ask::Text, "late");
        while p.work() {}
        assert!(p.poll(Some(b"2024-03-02 00:00:00 WARN rotated late\n".to_vec())));
        p.render(100, 20);
        while p.work() {}
        assert_eq!(p.rows(), 1);
        let c = p.render(100, 20);
        assert!(c.text().contains("truncated or replaced"));
        assert!(c.line(0).contains("1 entry · 1 line · following"), "{}", c.line(0));
    }

    #[test]
    fn a_page_comes_back_as_it_was() {
        let mut p = java();
        p.answer(Ask::Search, "connection");
        p.key(Key::Char('d'));
        while p.work() {}
        p.key(Key::Char('G'));
        let s = p.saved();
        let mut again =
            LogPage::new("/x/app.log", "/x/app.log", Ok(Source::bytes(include_str!("samples/java.log").as_bytes().to_vec())), Rc::new(Rules::default()));
        again.restore(&s);
        while again.work() {}
        assert_eq!(again.saved(), s);
        assert_eq!(again.cursor, p.cursor);
        assert_eq!(again.key(Key::Char('r')), Action::Open(9));
    }

    #[test]
    fn a_long_entry_scrolls_inside_and_the_mouse_picks_rows() {
        let mut text = String::from("2024-03-01 12:00:00 ERROR deep\n");
        for i in 0..40 {
            text.push_str(&format!("    at frame{i}\n"));
        }
        text.push_str("2024-03-01 12:00:01 INFO after\n");
        let mut p = page(&text);
        p.key(Key::Enter);
        p.render(80, 20);
        // 41 lines in 12: 29 to go through.
        for _ in 0..29 {
            p.key(Key::Char('j'));
        }
        assert_eq!(p.cursor, 0, "still in the open entry");
        assert!(p.render(80, 20).text().contains("frame39"));
        p.key(Key::Char('j'));
        assert_eq!(p.cursor, 1);
        p.key(Key::Char('C'));
        p.key(Key::Char('g'));
        p.render(80, 20);
        p.mouse(&Mouse::click(5, HEAD + 1));
        assert_eq!(p.cursor, 1);
        p.mouse(&Mouse { action: fenix_ui_kit::MouseAction::Press { button: fenix_ui_kit::Button::Left, clicks: 2 }, ..Mouse::click(5, HEAD) });
        assert_eq!(p.cursor, 0);
        assert!(p.opened.contains(&0), "a double click opens it");
    }

    #[test]
    fn a_journal_export_s_row_is_its_message() {
        let p = page(include_str!("samples/journal.export"));
        let c = p.render(100, 20);
        assert!(c.line(HEAD).trim_end().ends_with("I▸ systemd[1]: Started nginx.service. +9 lines"), "{}", c.line(HEAD));
        assert!(c.line(HEAD + 1).contains("E▸ app[42]: disk failure"), "{}", c.line(HEAD + 1));
    }

    #[test]
    fn a_bar_is_red_for_rare_errors_or_a_spike_of_them() {
        assert!(stands_out(1, 1000, 3, 100_000), "rare: any shows");
        assert!(!stands_out(0, 1000, 3, 100_000));
        assert!(!stands_out(100, 1000, 10_000, 100_000), "as many as anywhere");
        assert!(stands_out(150, 1000, 10_000, 100_000), "half as many again");
    }

    #[test]
    fn an_empty_or_unread_log_says_so() {
        let mut p = page("");
        assert!(p.render(80, 20).text().contains("Empty"));
        assert_eq!(p.key(Key::Char('j')), Action::Redraw);
        let mut p = LogPage::new("/x/b.log", "/x/b.log", Err("too big".into()), Rc::new(Rules::default()));
        assert!(p.render(80, 20).text().contains("Not read: too big"));
        assert_eq!(p.key(Key::Char('R')), Action::Reload);
    }
}
