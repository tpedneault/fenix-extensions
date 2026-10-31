//! A file's table page: its keys and how it's laid out. What it asks of
//! Fenix -- the buffer, the clipboard, the message line -- goes through
//! [`World`], so it's tested here without one.

use std::borrow::Cow;
use std::cell::RefCell;
use std::ops::Range;

use fenix_ui_kit::text::{fit, width, wrap};
use fenix_ui_kit::{Canvas, Column, Key, Mouse, Outcome, Rect, Role, Selecting, Style, TableSource, TableState, TextField, Width};
use serde::{Deserialize, Serialize};

use crate::copy::{as_csv, as_markdown};
use crate::dialect::{number, Dialect};
use crate::edit::{coalesce, text_edits, Change, History, TextEdit, MANY};
use crate::table::{Splice, Table};
use crate::view::{self, parse_filter, show_number, Filter, Sort, Stats};

/// Bigger than this, the buffer's text isn't read back to check it's
/// the page's before each change: two copies of it don't fit.
pub const CHECK_UP_TO: usize = 16 * 1024 * 1024;
/// Bigger than this, a change to every row (a column, a sort saved) is
/// refused: the text, the new text and what undoes it don't fit in an
/// extension's 128 MiB.
pub const WHOLE_UP_TO: usize = 20 * 1024 * 1024;
/// The widest a column is drawn; the focused cell is shown whole below.
const CELL_MAX: usize = 40;
/// Lines of the focused cell shown below the table.
const DETAIL: usize = 3;
/// The table's first line: under the title and the filter.
const TOP: usize = 2;
/// Rows shown when the pane's height isn't known.
pub const ROWS: usize = 30;

/// What the page asks of Fenix.
pub trait World {
    /// The buffer on `path`'s text, if one is open.
    fn buffer_text(&mut self, path: &str) -> Option<String>;
    fn edit(&mut self, path: &str, edits: Vec<TextEdit>) -> Result<(), String>;
    fn save(&mut self, path: &str) -> Result<(), String>;
    fn copy(&mut self, text: &str);
    fn say(&mut self, text: &str);
    /// The file's own text, its buffer, to edit (`path` as the page reads
    /// it).
    fn text(&mut self, path: &str);
    /// The dialect chosen for this file kept (none: guessed again).
    fn remember(&mut self, full: &str, dialect: Option<Dialect>);
    /// The buffer's last change undone, or done again (`redo`): Fenix's
    /// own history, the text's edits and the page's in one.
    fn buffer_undo(&mut self, path: &str, redo: bool) -> Result<(), String>;
}

/// A cell's background, besides its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    /// The focused cell.
    Focus,
    Selection,
}


#[derive(Debug)]
enum Mode {
    Table,
    /// `/`: typing a filter.
    Filter(TextField),
    /// Typing a cell's value: its data row, or the header's, and column.
    Edit {
        field: TextField,
        row: Option<usize>,
        col: usize,
    },
    /// `d`, waiting for `d` (the row) or `c` (the column).
    Delete,
}

/// What a page keeps in the session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Saved {
    pub path: String,
    #[serde(default)]
    pub full: String,
    #[serde(default)]
    pub row: usize,
    #[serde(default)]
    pub col: usize,
    #[serde(default)]
    pub filter: String,
    #[serde(default)]
    pub sort: Option<(usize, bool)>,
    #[serde(default)]
    pub stats: bool,
}

pub struct TablePage {
    /// The file as Fenix gave it, and as it reads it (the same, or under
    /// `/project` when restored).
    pub full: String,
    pub path: String,
    pub table: Table,
    /// Why it can't be changed, when it can't.
    read_only: Option<String>,
    /// The dialect guessed, and whether the one used was chosen instead.
    guessed: Dialect,
    pub grid: TableState,
    mode: Mode,
    /// The rows shown, as data rows, when filtered or sorted.
    order: Option<Vec<u32>>,
    filter_text: String,
    filter: Option<Filter>,
    sort: Option<Sort>,
    pub stats: bool,
    history: History,
    /// Changed since it was last saved (here).
    pub modified: bool,
    /// Data rows the table shows at once, when `csv.rows` says; else as
    /// many as the pane has room for.
    pub rows_shown: Option<usize>,
    /// Counts changes, for what's worked out from the rows.
    version: u64,
    cache: RefCell<Cache>,
}

#[derive(Default)]
struct Cache {
    /// The titles and which columns are numbers, at a version.
    columns: Option<(u64, Vec<String>, Vec<bool>)>,
    stats: Option<((u64, usize), Stats)>,
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Whether `path` is a TSV, where a tab wins a tie.
pub fn is_tsv(path: &str) -> bool {
    file_name(path).to_lowercase().ends_with(".tsv") || file_name(path).to_lowercase().ends_with(".tab")
}

/// `n` with a thousands separator.
fn count(n: usize) -> String {
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

impl TablePage {
    /// `text`, the file's (`bytes` that may not be UTF-8), read with
    /// `chosen` if one was kept for it, else as guessed.
    pub fn new(full: &str, path: &str, bytes: Vec<u8>, chosen: Option<Dialect>) -> TablePage {
        let (text, read_only) = match String::from_utf8(bytes) {
            Ok(t) => (t, None),
            Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), Some("it isn't UTF-8 text, so it's shown and not changed".to_string())),
        };
        let guessed = crate::dialect::detect(&text, is_tsv(full));
        let table = Table::new(text, chosen.unwrap_or(guessed));
        TablePage {
            full: full.to_string(),
            path: path.to_string(),
            table,
            read_only,
            guessed,
            grid: TableState::new(),
            mode: Mode::Table,
            order: None,
            filter_text: String::new(),
            filter: None,
            sort: None,
            stats: false,
            history: History::default(),
            modified: false,
            rows_shown: None,
            version: 0,
            cache: RefCell::new(Cache::default()),
        }
    }

    pub fn name(&self) -> &str {
        file_name(&self.full)
    }

    pub fn typing(&self) -> bool {
        matches!(self.mode, Mode::Filter(_) | Mode::Edit { .. })
    }

    pub fn saved(&self) -> Saved {
        Saved {
            path: self.path.clone(),
            full: self.full.clone(),
            row: self.grid.row,
            col: self.grid.col,
            filter: self.filter_text.clone(),
            sort: self.sort.map(|s| (s.col, s.desc)),
            stats: self.stats,
        }
    }

    /// Where it was, put back.
    pub fn restore(&mut self, s: &Saved) {
        self.stats = s.stats;
        self.sort = s.sort.filter(|(c, _)| *c < self.table.cols()).map(|(col, desc)| Sort { col, desc });
        self.set_filter(&s.filter);
        self.grid.row = s.row.min(self.len().saturating_sub(1));
        self.grid.col = s.col.min(self.table.cols().saturating_sub(1));
    }

    /// Rows shown.
    pub fn len(&self) -> usize {
        self.order.as_ref().map_or(self.table.rows(), Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The data row view row `i` is.
    pub fn data_row(&self, i: usize) -> usize {
        self.order.as_ref().map_or(i, |o| o[i] as usize)
    }

    fn focused_row(&self) -> Option<usize> {
        (!self.is_empty()).then(|| self.data_row(self.grid.row.min(self.len() - 1)))
    }

    fn col(&self) -> usize {
        self.grid.col.min(self.table.cols().saturating_sub(1))
    }

    fn columns_info(&self) -> (Vec<String>, Vec<bool>) {
        let mut cache = self.cache.borrow_mut();
        if let Some((v, t, n)) = &cache.columns {
            if *v == self.version {
                return (t.clone(), n.clone());
            }
        }
        let titles = self.table.titles();
        // Numbers lined up on the right: a column's first hundred rows.
        let sample: Vec<Vec<Cow<'_, str>>> = (0..self.table.rows().min(100)).map(|r| self.table.row(r)).collect();
        let numeric = (0..titles.len())
            .map(|c| {
                let vals: Vec<&str> = sample.iter().filter_map(|r| r.get(c)).map(|s| s.as_ref()).filter(|s| !s.trim().is_empty()).collect();
                !vals.is_empty() && vals.iter().all(|v| number(v).is_some())
            })
            .collect::<Vec<_>>();
        cache.columns = Some((self.version, titles.clone(), numeric.clone()));
        (titles, numeric)
    }

    /// The rows worked out again: filtered and sorted afresh after a
    /// change, the focus on the row it was on if it's still shown.
    fn refresh(&mut self) {
        let keep = self.focused_row();
        self.version += 1;
        self.grid.refit();
        self.order = view::rows(&self.table, self.filter.as_ref(), self.sort);
        if let (Some(r), Some(order)) = (keep, &self.order) {
            if let Some(i) = order.iter().position(|&x| x as usize == r) {
                self.grid.row = i;
            }
        }
        self.grid.row = self.grid.row.min(self.len().saturating_sub(1));
        self.grid.col = self.col();
    }

    fn set_filter(&mut self, text: &str) {
        self.filter_text = text.trim().to_string();
        self.filter = parse_filter(&self.filter_text, &self.table.titles());
        self.refresh();
    }

    // Changes, through the buffer.

    /// Whether the buffer still holds what the page does; if it doesn't
    /// (changed there), the page reads it again and says so.
    fn in_step(&mut self, w: &mut dyn World) -> bool {
        if self.table.text().len() > CHECK_UP_TO {
            return true;
        }
        match w.buffer_text(&self.path) {
            Some(text) if text != self.table.text() => {
                self.reread(text);
                w.say(&format!("{} changed in its buffer: read again, nothing changed here", self.name()));
                false
            }
            _ => true,
        }
    }

    /// Its buffer changed (`buffer-changed`): `text`, what it holds now,
    /// read again if it isn't what the table has -- an edit in the text,
    /// not one of its own. Whether it was.
    pub fn changed(&mut self, text: Option<String>) -> bool {
        match text {
            Some(text) if text != self.table.text() => {
                self.reread(text);
                true
            }
            _ => false,
        }
    }

    /// The text replaced (read again), the dialect kept.
    pub fn reread(&mut self, text: String) {
        let d = self.table.dialect;
        self.table = Table::new(text, d);
        self.history.clear();
        self.refresh();
    }

    /// `splices` made to the buffer and the page, as one change.
    fn change(&mut self, splices: Vec<Splice>, what: &str, w: &mut dyn World) -> bool {
        if let Some(why) = &self.read_only {
            w.say(&format!("{}: {why}", self.name()));
            return false;
        }
        if splices.is_empty() || !self.in_step(w) {
            return false;
        }
        let splices = if splices.len() > MANY { vec![coalesce(self.table.text(), &splices)] } else { splices };
        let whole = splices.iter().map(|s| s.text.len()).sum::<usize>();
        if whole > WHOLE_UP_TO {
            w.say(&format!("{what}: too big a change for the extension's memory -- change it as text (t)"));
            return false;
        }
        if let Err(e) = w.edit(&self.path, text_edits(self.table.text(), &splices)) {
            w.say(&format!("{what}: {e}"));
            return false;
        }
        let undo = self.table.apply(&splices);
        if !self.history.push(Change { what: what.to_string(), undo, redo: splices }) {
            w.say(&format!("{what}: done, but too big to undo here"));
        }
        self.modified = true;
        self.refresh();
        true
    }

    fn undo(&mut self, redo: bool, w: &mut dyn World) {
        if !self.in_step(w) {
            return;
        }
        let change = if redo { self.history.redo() } else { self.history.undo() };
        let Some(change) = change.cloned() else {
            return w.say(if redo { "nothing to redo" } else { "nothing to undo" });
        };
        let splices = if redo { &change.redo } else { &change.undo };
        if let Err(e) = w.edit(&self.path, text_edits(self.table.text(), splices)) {
            return w.say(&e);
        }
        self.table.apply(splices);
        self.modified = true;
        self.refresh();
        w.say(&format!("{} {}", if redo { "redid:" } else { "undid:" }, change.what));
    }

    /// Ctrl+Z, Ctrl+Shift+Z: the buffer's own undo, whatever made the
    /// change -- the page or the text -- and the table read again.
    fn buffer_undo(&mut self, redo: bool, w: &mut dyn World) {
        if let Err(e) = w.buffer_undo(&self.path, redo) {
            return w.say(&e);
        }
        let text = w.buffer_text(&self.path);
        if self.changed(text) {
            self.modified = true;
        }
    }

    fn save(&mut self, w: &mut dyn World) {
        if let Some(sort) = self.sort {
            if self.read_only.is_none() {
                let all = view::rows(&self.table, None, Some(sort)).unwrap_or_default();
                let splices = self.table.reorder(&all);
                if !self.change(splices, "sort saved", w) && self.table.rows() > 1 {
                    return;
                }
                self.sort = None;
                self.refresh();
            }
        }
        match w.save(&self.path) {
            Ok(()) => {
                self.modified = false;
                w.say(&format!("saved {}", self.name()));
            }
            Err(e) => w.say(&e),
        }
    }

    fn set_dialect(&mut self, d: Dialect, w: &mut dyn World) {
        self.table.set_dialect(d);
        self.sort = self.sort.filter(|s| s.col < self.table.cols());
        w.remember(&self.full, (d != self.guessed).then_some(d));
        let text = self.filter_text.clone();
        self.set_filter(&text);
        self.grid.clear_selection();
    }

    // Keys.

    pub fn key(&mut self, key: Key, w: &mut dyn World) {
        self.key_in_mode(key, w);
        // The table's cell is drawn as focused but while a filter's typed.
        self.grid.focused = !matches!(self.mode, Mode::Filter(_));
        self.grid.empty = if self.filter.is_some() { "No rows match the filter".into() } else { "No rows: o adds one".into() };
    }

    fn key_in_mode(&mut self, key: Key, w: &mut dyn World) {
        match std::mem::replace(&mut self.mode, Mode::Table) {
            Mode::Table => self.table_key(key, w),
            Mode::Delete => match key {
                Key::Char('d') => self.delete_row(w),
                Key::Char('c') => self.delete_column(w),
                _ => {}
            },
            Mode::Filter(mut field) => match field.handle_key(key) {
                Outcome::Submit => {
                    let text = field.text().to_string();
                    self.set_filter(&text);
                    self.grid.row = 0;
                    if self.filter.is_some() && self.is_empty() {
                        w.say("no rows match");
                    }
                }
                Outcome::Cancel => {}
                _ => self.mode = Mode::Filter(field),
            },
            Mode::Edit { mut field, row, col } => {
                let done = match key {
                    Key::Tab => Outcome::Submit,
                    k => field.handle_key(k),
                };
                match done {
                    Outcome::Submit => {
                        let value = field.text().to_string();
                        let splices = match row {
                            Some(r) => self.table.set_cell(r, col, &value),
                            None => self.table.set_title(col, &value),
                        };
                        match splices {
                            Ok(s) => {
                                self.change(s, "cell", w);
                            }
                            Err(e) => {
                                w.say(&e);
                                self.mode = Mode::Edit { field, row, col };
                            }
                        }
                    }
                    Outcome::Cancel => {}
                    _ => self.mode = Mode::Edit { field, row, col },
                }
            }
        }
    }

    /// A key with Ctrl held (`c`, as typed with Shift or not): Ctrl+Z
    /// undoes, Ctrl+Shift+Z and Ctrl+Y redo -- in a field its typing,
    /// else the file's buffer.
    pub fn ctrl_key(&mut self, c: char, shift: bool, w: &mut dyn World) {
        let redo = match c.to_ascii_lowercase() {
            'z' => shift,
            'y' => true,
            _ => return,
        };
        match &mut self.mode {
            Mode::Filter(f) | Mode::Edit { field: f, .. } => {
                if redo {
                    f.redo();
                } else {
                    f.undo();
                }
            }
            Mode::Delete => self.mode = Mode::Table,
            Mode::Table => self.buffer_undo(redo, w),
        }
    }

    /// The data row and column at (`line`, `cell`) of the last layout,
    /// if it's a row's.
    fn cell_at(&self, line: usize, cell: usize) -> Option<(usize, usize)> {
        let shown = self.grid.columns_shown();
        // Between two columns, the one on the left.
        let col = shown.iter().rev().find(|(_, xs)| xs.start <= cell).filter(|(_, xs)| cell < xs.end + self.grid.gap).map(|(c, _)| *c)?;
        let rows = self.grid.visible_rows();
        let row = rows.start + line.checked_sub(TOP + 2)?;
        (row < rows.end && row < self.len()).then_some((row, col))
    }

    /// A click focuses the cell (a column's title, its column), a double
    /// click edits it (the title, the column's name), a drag selects cells
    /// to where it goes, the wheel moves the focused row.
    pub fn mouse(&mut self, m: &Mouse, w: &mut dyn World) {
        if matches!(self.mode, Mode::Delete) {
            self.mode = Mode::Table;
        }
        if !matches!(self.mode, Mode::Table) {
            return;
        }
        if m.is_drag() {
            let Some((row, col)) = self.cell_at(m.y, m.x) else { return };
            if self.grid.selecting().is_none() {
                self.grid.select(Selecting::Cells);
            }
            (self.grid.row, self.grid.col) = (row, col);
        } else {
            if m.left_clicks() > 0 {
                self.grid.clear_selection();
            }
            match self.grid.mouse(m, self.len(), self.table.cols()) {
                Outcome::Submit => self.edit(false, w),
                _ if m.left_clicks() >= 2 && m.y == TOP => self.edit(true, w),
                _ => {}
            }
        }
        self.grid.focused = true;
    }

    pub fn paste(&mut self, text: &str) {
        match &mut self.mode {
            Mode::Filter(f) | Mode::Edit { field: f, .. } => {
                f.paste(text);
            }
            _ => {}
        }
    }

    fn table_key(&mut self, key: Key, w: &mut dyn World) {
        let (rows, cols) = (self.len(), self.table.cols());
        match key {
            Key::Enter | Key::Char('e') | Key::Char('i') => self.edit(false, w),
            Key::Char('E') => self.edit(true, w),
            Key::Char('/') => {
                let mut f = TextField::new().with_text(&self.filter_text).with_placeholder("text, or a column's condition: price > 3, name ~ ada, & for both");
                f.select_all();
                self.mode = Mode::Filter(f);
            }
            Key::Char('s') => {
                let col = self.col();
                self.sort = match self.sort {
                    Some(Sort { col: c, desc: false }) if c == col => Some(Sort { col, desc: true }),
                    Some(Sort { col: c, desc: true }) if c == col => None,
                    _ if cols > 0 => Some(Sort { col, desc: false }),
                    _ => None,
                };
                self.refresh();
            }
            Key::Char('o') | Key::Char('O') => {
                let at = match self.focused_row() {
                    Some(r) if key == Key::Char('o') => r + 1,
                    Some(r) => r,
                    None => self.table.rows(),
                };
                let splices = self.table.insert_row(at);
                if self.change(splices, "row added", w) {
                    match self.order.as_ref().map(|o| o.iter().position(|&x| x as usize == at)) {
                        None => self.grid.row = at,
                        Some(Some(i)) => self.grid.row = i,
                        Some(None) => w.say("row added, out of sight: the filter or sort doesn't show it here"),
                    }
                }
            }
            Key::Char('a') | Key::Char('A') => {
                let at = if cols == 0 {
                    0
                } else if key == Key::Char('a') {
                    self.col() + 1
                } else {
                    self.col()
                };
                if self.table.text().len() > WHOLE_UP_TO {
                    return w.say("a column goes through every row: too big a file for the extension's memory -- change it as text (t)");
                }
                match self.table.insert_column(at, "") {
                    Ok(s) => {
                        if self.change(s, "column added", w) {
                            self.grid.col = at;
                        }
                    }
                    Err(e) => w.say(&e),
                }
            }
            Key::Char('d') => self.mode = Mode::Delete,
            Key::Char('u') => self.undo(false, w),
            Key::Char('U') => self.undo(true, w),
            Key::Char('w') => self.save(w),
            Key::Char('v') => self.grid.select(Selecting::Cells),
            Key::Char('V') => self.grid.select(Selecting::Rows),
            Key::Char('y') => self.copy(false, false, w),
            Key::Char('Y') => self.copy(true, false, w),
            Key::Char('m') => self.copy(true, true, w),
            Key::Char('c') => self.stats = !self.stats,
            Key::Char('H') => {
                let d = Dialect { header: !self.table.dialect.header, ..self.table.dialect };
                self.set_dialect(d, w);
            }
            Key::Char(';') => {
                let d = self.table.dialect.next_delimiter();
                self.set_dialect(d, w);
                w.say(&format!("fields split by {}", d.delimiter_name()));
            }
            Key::Char('"') => {
                let d = self.table.dialect.next_quote();
                self.set_dialect(d, w);
                w.say(&format!("quotes: {}", d.quote_name()));
            }
            Key::Char('t') => w.text(&self.path),
            key => {
                self.grid.handle_key(key, rows, cols);
            }
        }
    }

    fn edit(&mut self, header: bool, w: &mut dyn World) {
        if let Some(why) = &self.read_only {
            return w.say(&format!("{}: {why}", self.name()));
        }
        let col = self.col();
        let (row, value) = if header {
            if !self.table.dialect.header {
                return w.say("this file has no header (H takes its first row as one)");
            }
            (None, self.table.titles().get(col).cloned().unwrap_or_default())
        } else {
            match self.focused_row() {
                Some(r) => (Some(r), self.table.cell(r, col).into_owned()),
                None => return w.say("no row to change: o adds one"),
            }
        };
        let mut field = if value.contains(['\n', '\r']) { TextField::multiline() } else { TextField::new() };
        field = field.with_text(&value);
        self.mode = Mode::Edit { field, row, col: if self.table.cols() == 0 { 0 } else { col } };
    }

    fn delete_row(&mut self, w: &mut dyn World) {
        let Some(r) = self.focused_row() else { return };
        let splices = self.table.delete_row(r);
        self.change(splices, "row deleted", w);
    }

    fn delete_column(&mut self, w: &mut dyn World) {
        if self.table.cols() == 0 {
            return;
        }
        if self.table.text().len() > WHOLE_UP_TO {
            return w.say("a column goes through every row: too big a file for the extension's memory -- change it as text (t)");
        }
        let splices = self.table.delete_column(self.col());
        if self.change(splices, "column deleted", w) {
            self.grid.col = self.col();
        }
    }

    /// `y`: the selection, or the focused cell's value; `Y`: the
    /// selection or the row, as CSV; `m`: as a Markdown table.
    fn copy(&mut self, rows: bool, markdown: bool, w: &mut dyn World) {
        let (n, cols) = (self.len(), self.table.cols());
        if n == 0 || cols == 0 {
            return;
        }
        let titles = self.table.titles();
        let (rs, cs) = match self.grid.selection(n, cols) {
            Some(s) => s,
            None if rows => (self.grid.row.min(n - 1)..self.grid.row.min(n - 1) + 1, 0..cols),
            None => {
                let v = self.table.cell(self.data_row(self.grid.row.min(n - 1)), self.col()).into_owned();
                w.copy(&v);
                return w.say("copied the cell");
            }
        };
        let data: Vec<Vec<String>> = rs.clone().map(|i| cs.clone().map(|c| self.table.cell(self.data_row(i), c).into_owned()).collect()).collect();
        let text = if markdown { as_markdown(&titles[cs.clone()], &data) } else { as_csv(&self.table.dialect, &data) };
        w.copy(&text);
        let what = if rs.len() == 1 && cs.len() == cols { "the row".to_string() } else { format!("{} × {}", rs.len(), cs.len()) };
        w.say(&format!("copied {what}{}", if markdown { " as a Markdown table" } else { "" }));
        self.grid.clear_selection();
    }

    fn column_stats(&self) -> Stats {
        let col = self.col();
        let mut cache = self.cache.borrow_mut();
        if let Some(((v, c), s)) = &cache.stats {
            if (*v, *c) == (self.version, col) {
                return s.clone();
            }
        }
        let s = match &self.order {
            Some(o) => view::stats(&self.table, o.iter().map(|&r| r as usize), col),
            None => view::stats(&self.table, 0..self.table.rows(), col),
        };
        cache.stats = Some(((self.version, col), s.clone()));
        s
    }

    // Layout.

    /// Laid out `width_cells` wide, for a pane of no known height.
    pub fn render(&self, width_cells: usize) -> Canvas {
        self.render_in(width_cells, 0)
    }

    /// Its keys, for the strip at its foot.
    fn keys(&self) -> &'static [(&'static str, &'static str)] {
        match &self.mode {
            Mode::Delete => &[("d", "delete the row"), ("c", "delete the column")],
            Mode::Filter(_) => &[("Enter", "filter"), ("Esc", "leave it"), ("Enter on nothing", "every row")],
            Mode::Edit { .. } => &[],
            Mode::Table if self.grid.selecting().is_some() => &[("y", "copy as CSV"), ("m", "as Markdown"), ("v V", "stop selecting")],
            Mode::Table => &[
                ("hjkl", "move"),
                ("e", "edit"),
                ("/", "filter"),
                ("s", "sort"),
                ("o a", "add row, column"),
                ("dd dc", "delete"),
                ("u U", "undo, redo"),
                ("w", "save"),
                ("v V", "select"),
                ("y Y m", "copy"),
                ("c", "stats"),
                ("t", "text"),
            ],
        }
    }

    /// Laid out `width_cells` wide for a pane `height` lines tall (0: not
    /// known): the table's rows fill what the rest leaves, so its header
    /// stays in sight; `csv.rows` sets how many instead.
    pub fn render_in(&self, width_cells: usize, height: usize) -> Canvas {
        let w = width_cells.max(20);
        let mut c = Canvas::new();
        let (titles, numeric) = self.columns_info();
        let d = &self.table.dialect;

        // The title line.
        let mut x = c.put(0, 0, self.name(), Role::Title);
        let mut about = vec![format!("{} rows", count(self.table.rows())), format!("{} columns", count(self.table.cols()))];
        about.push(format!("{} {}{}", d.delimiter_name(), d.quote_name(), if d.header { " · header" } else { " · no header" }));
        if let Some(s) = self.sort {
            about.push(format!("sorted by {} {}", titles.get(s.col).map(String::as_str).unwrap_or("?"), if s.desc { "↓" } else { "↑" }));
        }
        if self.read_only.is_some() {
            about.push("read only".into());
        }
        x = c.put_fit(x + 2, 0, w.saturating_sub(x + 12), &about.join(" · "), Role::Muted);
        if self.modified {
            c.put(x + 2, 0, "● modified", Role::Warn);
        }

        // The filter line.
        match &self.mode {
            Mode::Filter(f) => {
                c.put(0, 1, "/", Role::Accent);
                f.render(Rect::new(2, 1, w - 2, 1), &mut c);
            }
            _ if self.filter.is_some() => {
                let x = c.put(0, 1, "/ ", Role::Muted);
                let x = c.put(x, 1, &self.filter_text, Role::Accent);
                c.put(x, 1, &format!("  {} of {} rows", count(self.len()), count(self.table.rows())), Role::Muted);
            }
            _ => {}
        }

        // The table.
        let columns: Vec<Column> = titles
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mark = match self.sort {
                    Some(s) if s.col == i => {
                        if s.desc {
                            " ↓"
                        } else {
                            " ↑"
                        }
                    }
                    _ => "",
                };
                let col = Column::new(format!("{t}{mark}"), Width::Fit { min: 1, max: CELL_MAX });
                if numeric.get(i).copied().unwrap_or(false) {
                    col.right()
                } else {
                    col
                }
            })
            .collect();
        let keys = key_strip(self.keys(), w);
        let foot = 1 + (1 + DETAIL) + usize::from(self.stats && !columns.is_empty()) + 1 + keys.last().map_or(0, |k| k.1 + 1);
        let rows = match (self.rows_shown, height) {
            (Some(n), _) => n,
            (None, 0) => ROWS,
            (None, h) => h.saturating_sub(TOP + 2 + foot),
        };
        let grid_h = rows.max(1) + 2;
        let top = TOP;
        if columns.is_empty() {
            c.put(0, top, "Empty: o adds a row", Role::Muted);
        } else {
            self.grid.render(&columns, &Source { page: self }, Rect::new(0, top, w, grid_h), &mut c);
        }
        let mut y = top + grid_h;
        c.rule(y, 0..w);
        y += 1;

        // The focused cell, whole; or the field it's typed in.
        let col = self.col();
        let title = titles.get(col).cloned().unwrap_or_default();
        match &self.mode {
            Mode::Edit { field, row, .. } => {
                let what = match row {
                    Some(r) => format!("{title} · row {}", r + 1),
                    None => format!("column {}'s name", col + 1),
                };
                let x = c.put(0, y, &what, Role::Accent);
                c.put(x + 2, y, if field.multiline { "Tab keeps it, Esc drops it" } else { "Enter keeps it, Esc drops it" }, Role::Muted);
                field.render(Rect::new(0, y + 1, w, if field.multiline { DETAIL } else { 1 }), &mut c);
                y += 1 + DETAIL;
            }
            _ => match self.focused_row() {
                Some(r) if !columns.is_empty() => {
                    let shown = self.grid.row.min(self.len() - 1) + 1;
                    let mut x = c.put(0, y, &title, Role::Accent);
                    x = c.put(x, y, &format!("  row {} of {}", count(shown), count(self.len())), Role::Muted);
                    if self.order.is_some() {
                        x = c.put(x, y, &format!(" (row {} in the file)", count(r + 1)), Role::Muted);
                    }
                    let _ = x;
                    let value = self.table.cell(r, col);
                    let mut lines = wrap(&value, w);
                    if lines.len() > DETAIL {
                        lines.truncate(DETAIL);
                        let last = lines.pop().unwrap_or_default();
                        lines.push(fit(&format!("{last} …"), w));
                    }
                    for (i, l) in lines.iter().enumerate() {
                        c.put(0, y + 1 + i, l, Role::Text);
                    }
                    y += 1 + DETAIL;
                }
                _ => y += 1 + DETAIL,
            },
        }

        // The focused column's stats.
        if self.stats && !columns.is_empty() {
            let s = self.column_stats();
            let mut parts = vec![format!("count {}", count(s.count)), format!("distinct {}", count(s.distinct))];
            if s.blank > 0 {
                parts.push(format!("blank {}", count(s.blank)));
            }
            if let Some(m) = &s.min {
                parts.push(format!("min {}", fit(m, 24)));
            }
            if let Some(m) = &s.max {
                parts.push(format!("max {}", fit(m, 24)));
            }
            if let Some(m) = s.mean {
                parts.push(format!("mean {}", show_number(m)));
            }
            let x = c.put(0, y, &format!("{title}: "), Role::Accent);
            c.put_fit(x, y, w.saturating_sub(x), &parts.join(" · "), if s.numeric { Role::Good } else { Role::Text });
            y += 1;
        }

        // Its keys.
        for (x, dy, k, what) in keys {
            let x = c.put(x, y + 1 + dy, k, Role::Accent);
            c.put(x + 1, y + 1 + dy, what, Role::Muted);
        }
        c
    }

    /// What's drawn on a background in `c`, its last layout: the focused
    /// cell's whole width, and the selection's cells. Columns are in
    /// characters, as the API counts them.
    pub fn tints(&self, c: &Canvas) -> Vec<(usize, Range<usize>, Tint)> {
        let (n, cols) = (self.len(), self.table.cols());
        if !matches!(self.mode, Mode::Table | Mode::Delete) || n == 0 || cols == 0 {
            return Vec::new();
        }
        let shown = self.grid.columns_shown();
        let rows = self.grid.visible_rows();
        let body = TOP + 2;
        let mut out = Vec::new();
        if let Some((rs, cs)) = self.grid.selection(n, cols) {
            for r in rs.start.max(rows.start)..rs.end.min(rows.end).min(n) {
                let y = body + r - rows.start;
                // Selected columns side by side: one run, the gaps in it.
                let xs: Vec<&Range<usize>> = shown.iter().filter(|(col, _)| cs.contains(col)).map(|(_, xs)| xs).collect();
                if let (Some(first), Some(last)) = (xs.first(), xs.last()) {
                    out.push((y, c.char_range(y, &(first.start..last.end)), Tint::Selection));
                }
            }
        }
        let row = self.grid.row.min(n - 1);
        if rows.contains(&row) {
            if let Some((_, xs)) = shown.iter().find(|(col, _)| *col == self.col()) {
                let y = body + row - rows.start;
                out.push((y, c.char_range(y, xs), Tint::Focus));
            }
        }
        out
    }
}

/// Where each of `keys` goes in a strip `w` wide, wrapped: its column,
/// its line from the strip's first, and the key and what it does.
fn key_strip(keys: &[(&'static str, &'static str)], w: usize) -> Vec<(usize, usize, &'static str, &'static str)> {
    let (mut x, mut y) = (0, 0);
    let mut out = Vec::new();
    for &(k, what) in keys {
        let len = width(k) + width(what) + 1;
        if x > 0 && x + len > w {
            (x, y) = (0, y + 1);
        }
        out.push((x, y, k, what));
        x += len + 2;
    }
    out
}

/// The rows shown, as the table widget asks for them.
struct Source<'a> {
    page: &'a TablePage,
}

impl TableSource for Source<'_> {
    fn len(&self) -> usize {
        self.page.len()
    }

    fn row(&self, index: usize) -> Vec<Cow<'_, str>> {
        self.page.table.row(self.page.data_row(index)).into_iter().map(|v| if v.contains(['\n', '\r']) { Cow::Owned(v.replace("\r\n", "↵").replace(['\n', '\r'], "↵")) } else { v }).collect()
    }

    fn style(&self, _row: usize, _col: usize) -> Style {
        Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_ui_kit::{Button, Mods, MouseAction};

    /// A buffer of its own: edits made to its text as Fenix makes them.
    #[derive(Default)]
    struct Fake {
        buffer: Option<String>,
        said: Vec<String>,
        copied: Vec<String>,
        saved: usize,
        kept: Option<Option<Dialect>>,
        texts: Vec<String>,
        /// The buffer before each edit, and after each undo: Fenix's
        /// history.
        undos: Vec<String>,
        redos: Vec<String>,
    }

    impl Fake {
        fn on(text: &str) -> Fake {
            Fake { buffer: Some(text.into()), ..Fake::default() }
        }
    }

    /// `edits` made to `text` by (line, column), as `apply_text_edits`
    /// does: the last first.
    fn apply_edits(text: &str, mut edits: Vec<TextEdit>) -> String {
        let lines: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        let at = |(l, c): (u32, u32)| {
            let start = lines.get(l as usize).copied().unwrap_or(text.len());
            text[start..].char_indices().nth(c as usize).map_or(text.len(), |(b, _)| start + b)
        };
        edits.sort_by_key(|e| std::cmp::Reverse(e.start));
        let mut out = text.to_string();
        for e in edits {
            out.replace_range(at(e.start)..at(e.end), &e.text);
        }
        out
    }

    impl World for Fake {
        fn buffer_text(&mut self, _path: &str) -> Option<String> {
            self.buffer.clone()
        }
        fn edit(&mut self, _path: &str, edits: Vec<TextEdit>) -> Result<(), String> {
            let b = self.buffer.clone().ok_or("no buffer")?;
            self.buffer = Some(apply_edits(&b, edits));
            self.undos.push(b);
            self.redos.clear();
            Ok(())
        }
        fn save(&mut self, _path: &str) -> Result<(), String> {
            self.saved += 1;
            Ok(())
        }
        fn copy(&mut self, text: &str) {
            self.copied.push(text.into());
        }
        fn say(&mut self, text: &str) {
            self.said.push(text.into());
        }
        fn text(&mut self, path: &str) {
            self.texts.push(path.into());
        }
        fn remember(&mut self, _full: &str, dialect: Option<Dialect>) {
            self.kept = Some(dialect);
        }
        fn buffer_undo(&mut self, _path: &str, redo: bool) -> Result<(), String> {
            let (from, to) = if redo { (&mut self.redos, &mut self.undos) } else { (&mut self.undos, &mut self.redos) };
            let text = from.pop().ok_or("nothing to undo")?;
            to.push(self.buffer.replace(text).unwrap_or_default());
            Ok(())
        }
    }

    const PEOPLE: &str = "name,age,city\nada,36,London\ngrace,85,Arlington\nalan,41,london\n";

    fn page(text: &str) -> TablePage {
        let mut p = TablePage::new("/home/me/people.csv", "/home/me/people.csv", text.as_bytes().to_vec(), None);
        p.rows_shown = Some(5);
        p
    }

    fn keys(p: &mut TablePage, w: &mut Fake, keys: &str) {
        for k in keys.chars() {
            p.key(if k == '\n' { Key::Enter } else { Key::Char(k) }, w);
        }
    }

    #[test]
    fn a_table_aligned_its_focused_cell_whole_below() {
        let mut p = page("name,age,note\nada,36,a long note that goes on and on and on past the forty cells a column has\ngrace,85,short\n");
        let mut w = Fake::default();
        keys(&mut p, &mut w, "ll");
        let c = p.render(60);
        let text = c.text();
        assert!(text.starts_with("people.csv  2 rows · 3 columns · , \" · header"), "{text}");
        assert_eq!(c.line(2), "name   age  note");
        assert_eq!(c.line(4), "ada     36  a long note that goes on and on and on …");
        assert_eq!(c.line(5), "grace   85  short");
        assert!(c.line(10).starts_with("note  row 1 of 2"), "{}", c.line(10));
        assert_eq!(c.line(11), "a long note that goes on and on and on past the forty cells");
        assert_eq!(c.line(12), "a column has");
        assert_eq!(c.focused(), Some(&(4, 0..60)));
    }

    #[test]
    fn a_cell_edited_goes_through_the_buffer_and_undoes() {
        let mut p = page(PEOPLE);
        let mut w = Fake::on(PEOPLE);
        keys(&mut p, &mut w, "je");
        assert!(p.typing());
        p.paste(", Countess");
        p.key(Key::Enter, &mut w);
        assert!(!p.typing());
        let want = "name,age,city\nada,36,London\n\"grace, Countess\",85,Arlington\nalan,41,london\n";
        assert_eq!(w.buffer.as_deref(), Some(want));
        assert_eq!(p.table.text(), want);
        assert!(p.modified);
        keys(&mut p, &mut w, "u");
        assert_eq!(w.buffer.as_deref(), Some(PEOPLE));
        assert_eq!(p.table.text(), PEOPLE);
        keys(&mut p, &mut w, "U");
        assert_eq!(w.buffer.as_deref(), Some(want));
        keys(&mut p, &mut w, "w");
        assert_eq!(w.saved, 1);
        assert!(!p.modified);
    }

    #[test]
    fn rows_and_columns_added_and_deleted() {
        let mut p = page(PEOPLE);
        let mut w = Fake::on(PEOPLE);
        keys(&mut p, &mut w, "o");
        assert_eq!(p.grid.row, 1, "on the new row");
        assert_eq!(p.table.text(), "name,age,city\nada,36,London\n,,\ngrace,85,Arlington\nalan,41,london\n");
        assert_eq!(w.buffer.as_deref(), Some(p.table.text()));
        keys(&mut p, &mut w, "dd");
        assert!(w.said.is_empty(), "{:?}", w.said);
        assert_eq!(p.table.text(), PEOPLE);
        keys(&mut p, &mut w, "la");
        assert_eq!(p.grid.col, 2);
        assert_eq!(p.table.text(), "name,age,,city\nada,36,,London\ngrace,85,,Arlington\nalan,41,,london\n");
        keys(&mut p, &mut w, "E");
        p.paste("born");
        p.key(Key::Enter, &mut w);
        assert!(p.table.text().starts_with("name,age,born,city\n"));
        keys(&mut p, &mut w, "hdc");
        assert_eq!(p.table.text(), "name,born,city\nada,,London\ngrace,,Arlington\nalan,,london\n");
        assert_eq!(w.buffer.as_deref(), Some(p.table.text()));
        keys(&mut p, &mut w, "uuuuu");
        assert_eq!(p.table.text(), PEOPLE);
        assert_eq!(w.buffer.as_deref(), Some(PEOPLE));
    }

    #[test]
    fn t_opens_the_file_s_own_text() {
        let mut p = page(PEOPLE);
        let mut w = Fake::on(PEOPLE);
        keys(&mut p, &mut w, "t");
        assert_eq!(w.texts, ["/home/me/people.csv"]);
    }

    #[test]
    fn its_buffer_changed_is_read_again_unless_it_s_the_same() {
        let mut p = page(PEOPLE);
        let mut w = Fake::on(PEOPLE);
        keys(&mut p, &mut w, "jdd");
        assert!(!p.changed(w.buffer.clone()), "its own edit, told back: nothing to read");
        assert_eq!(p.table.rows(), 2);
        assert!(p.changed(Some("name,age,city\nada,37,London\n".into())), "typed in the text");
        assert_eq!(p.table.rows(), 1);
        assert!(!p.changed(None), "closed");
    }

    #[test]
    fn a_buffer_changed_elsewhere_is_read_again_not_overwritten() {
        let mut p = page(PEOPLE);
        let mut w = Fake::on("name,age,city\nada,37,London\n");
        keys(&mut p, &mut w, "dd");
        assert_eq!(p.table.text(), "name,age,city\nada,37,London\n");
        assert_eq!(w.buffer.as_deref(), Some("name,age,city\nada,37,London\n"), "nothing deleted");
        assert!(w.said[0].contains("changed in its buffer"));
    }

    #[test]
    fn sorted_and_filtered_in_the_view_the_sort_written_on_save() {
        let mut p = page(PEOPLE);
        let mut w = Fake::on(PEOPLE);
        keys(&mut p, &mut w, "ls");
        assert_eq!((0..p.len()).map(|i| p.data_row(i)).collect::<Vec<_>>(), [0, 2, 1]);
        assert!(p.render(60).line(2).contains("age ↑"));
        assert_eq!(p.table.text(), PEOPLE, "the file as it was");
        keys(&mut p, &mut w, "s");
        assert_eq!((0..p.len()).map(|i| p.data_row(i)).collect::<Vec<_>>(), [1, 2, 0], "down");
        keys(&mut p, &mut w, "/");
        assert!(p.typing());
        p.paste("city ~ lon");
        p.key(Key::Enter, &mut w);
        assert_eq!((0..p.len()).map(|i| p.data_row(i)).collect::<Vec<_>>(), [2, 0]);
        assert!(p.render(60).line(1).contains("city ~ lon  2 of 3 rows"));
        keys(&mut p, &mut w, "w");
        assert_eq!(w.buffer.as_deref(), Some("name,age,city\ngrace,85,Arlington\nalan,41,london\nada,36,London\n"), "every row in the sort's order");
        assert_eq!(w.saved, 1);
        let c = p.render(60);
        assert!(!c.line(0).contains("sorted"), "the file is in order now");
        keys(&mut p, &mut w, "/");
        p.key(Key::Backspace, &mut w);
        p.key(Key::Enter, &mut w);
        assert_eq!(p.len(), 3, "the filter gone");
    }

    #[test]
    fn copies_a_cell_a_row_and_a_selection() {
        let mut p = page(PEOPLE);
        let mut w = Fake::default();
        keys(&mut p, &mut w, "y");
        assert_eq!(w.copied.last().unwrap(), "ada");
        keys(&mut p, &mut w, "Y");
        assert_eq!(w.copied.last().unwrap(), "ada,36,London\n");
        keys(&mut p, &mut w, "vjly");
        assert_eq!(w.copied.last().unwrap(), "ada,36\ngrace,85\n");
        keys(&mut p, &mut w, "Vjm");
        assert_eq!(w.copied.last().unwrap(), "| name  | age | city      |\n| ----- | --: | --------- |\n| grace |  85 | Arlington |\n| alan  |  41 | london    |\n");
    }

    #[test]
    fn the_dialect_overridden_and_remembered() {
        let mut p = page("a;b\n1;2\n");
        let mut w = Fake::default();
        assert_eq!(p.table.cols(), 2);
        keys(&mut p, &mut w, ";");
        assert_eq!(p.table.dialect.delimiter, b'\t');
        assert_eq!(p.table.cols(), 1);
        assert!(matches!(w.kept, Some(Some(_))));
        keys(&mut p, &mut w, ";;;");
        assert_eq!(p.table.dialect.delimiter, b';');
        assert_eq!(w.kept, Some(None), "back to the guess: nothing kept");
        keys(&mut p, &mut w, "H");
        assert_eq!(p.table.rows(), 2);
        assert_eq!(p.table.titles(), ["1", "2"]);
    }

    #[test]
    fn stats_on_the_focused_column() {
        let mut p = page(PEOPLE);
        let mut w = Fake::default();
        keys(&mut p, &mut w, "lc");
        let c = p.render(80);
        let line = (0..c.height()).map(|y| c.line(y)).find(|l| l.starts_with("age: ")).unwrap();
        assert_eq!(line, "age: count 3 · distinct 3 · min 36 · max 85 · mean 54");
    }

    #[test]
    fn a_file_that_isnt_utf8_is_shown_not_changed() {
        let mut p = TablePage::new("/x.csv", "/x.csv", b"a,b\n\xff,2\n".to_vec(), None);
        let mut w = Fake::on("");
        keys(&mut p, &mut w, "dd");
        assert_eq!(p.table.rows(), 1);
        assert!(w.said[0].contains("isn't UTF-8"));
    }

    #[test]
    fn an_empty_file_gets_its_first_row() {
        let mut p = page("");
        let mut w = Fake::on("");
        assert!(p.render(40).text().contains("Empty: o adds a row"));
        keys(&mut p, &mut w, "oe");
        p.paste("first");
        p.key(Key::Enter, &mut w);
        assert_eq!(w.buffer.as_deref(), Some("first\n"));
        assert_eq!((p.table.rows(), p.table.cols()), (1, 1));
    }

    /// `n` rows of two columns.
    fn many(n: usize) -> String {
        let mut s = "id,name\n".to_string();
        for i in 0..n {
            s.push_str(&format!("{i},row {i}\n"));
        }
        s
    }

    #[test]
    fn its_rows_fit_the_pane_so_the_header_stays() {
        let mut p = TablePage::new("/m.csv", "/m.csv", many(200).into_bytes(), None);
        let mut w = Fake::default();
        for h in [14, 20, 40] {
            let c = p.render_in(60, h);
            assert!(c.height() <= h, "{h}: {} lines\n{}", c.height(), c.text());
            assert!(c.line(2).starts_with("id  "), "the header in sight: {}", c.text());
            assert!(c.height() + 1 >= h, "the pane filled: {h}, {}", c.height());
        }
        // Down past the screen: the header still there, the focus in view.
        keys(&mut p, &mut w, "jjjjjjjjjjjjjjjjjjjj");
        let c = p.render_in(60, 16);
        assert!(c.line(2).starts_with("id  "), "{}", c.text());
        let (y, _) = c.focused().unwrap().clone();
        assert!(y < 16 && c.line(y).starts_with("20"), "{}", c.text());
        // csv.rows says otherwise.
        p.rows_shown = Some(3);
        let c = p.render_in(60, 40);
        assert_eq!(p.grid.visible_rows().len(), 3);
        assert!(c.height() < 30);
    }

    #[test]
    fn page_down_goes_a_screen() {
        let mut p = TablePage::new("/m.csv", "/m.csv", many(200).into_bytes(), None);
        let mut w = Fake::default();
        p.render_in(60, 20);
        let screen = p.grid.visible_rows().len();
        p.key(Key::PageDown, &mut w);
        assert_eq!(p.grid.row, screen - 1);
        p.key(Key::PageUp, &mut w);
        assert_eq!(p.grid.row, 0);
    }

    #[test]
    fn the_focused_cell_and_the_selection_have_backgrounds() {
        let mut p = page(PEOPLE);
        let mut w = Fake::default();
        keys(&mut p, &mut w, "jl");
        let c = p.render(60);
        // "name   age  city": age is cells 7..10, on grace's line.
        assert_eq!(p.tints(&c), [(5, 7..10, Tint::Focus)]);
        keys(&mut p, &mut w, "vjl");
        let c = p.render(60);
        let tints = p.tints(&c);
        assert!(tints.contains(&(5, 7..21, Tint::Selection)) && tints.contains(&(6, 7..21, Tint::Selection)), "{tints:?}");
        assert!(tints.contains(&(6, 12..21, Tint::Focus)));
        keys(&mut p, &mut w, "/");
        assert!(p.tints(&p.render(60)).is_empty(), "not while a filter's typed");
    }

    #[test]
    fn a_click_focuses_a_cell_a_double_click_edits_a_drag_selects() {
        let mut p = page(PEOPLE);
        let mut w = Fake::on(PEOPLE);
        p.render(60);
        let at = |x, y, clicks| Mouse { x, y, action: MouseAction::Press { button: Button::Left, clicks }, mods: Mods::NONE };
        let drag = |x, y| Mouse { x, y, action: MouseAction::Drag(Button::Left), mods: Mods::NONE };
        let wheel = |lines| Mouse { x: 1, y: 5, action: MouseAction::Wheel(lines), mods: Mods::NONE };
        p.mouse(&at(13, 5, 1), &mut w);
        assert_eq!((p.grid.row, p.grid.col), (1, 2), "grace's city");
        p.mouse(&at(1, 2, 1), &mut w);
        assert_eq!((p.grid.row, p.grid.col), (1, 0), "a title: its column");
        p.mouse(&at(1, 30, 1), &mut w);
        assert_eq!((p.grid.row, p.grid.col), (1, 0), "off the table: nothing");
        p.mouse(&drag(8, 6), &mut w);
        assert_eq!(p.grid.selection(3, 3), Some((1..3, 0..2)));
        p.mouse(&at(0, 4, 2), &mut w);
        assert!(p.typing() && p.grid.selection(3, 3).is_none(), "editing ada's name");
        p.paste("!");
        p.key(Key::Enter, &mut w);
        assert!(p.table.text().starts_with("name,age,city\nada!,"));
        p.mouse(&wheel(-1), &mut w);
        assert_eq!(p.grid.row, 2);
        p.mouse(&wheel(1), &mut w);
        assert_eq!(p.grid.row, 0);
        p.mouse(&at(1, 2, 2), &mut w);
        assert!(p.typing(), "a title double-clicked: the column's name");
    }

    #[test]
    fn ctrl_z_is_the_buffer_s_undo() {
        let mut p = page(PEOPLE);
        let mut w = Fake::on(PEOPLE);
        keys(&mut p, &mut w, "dd");
        assert_eq!(p.table.rows(), 2);
        p.ctrl_key('z', false, &mut w);
        assert_eq!((p.table.text(), w.buffer.as_deref()), (PEOPLE, Some(PEOPLE)));
        p.ctrl_key('Z', true, &mut w);
        assert_eq!(p.table.rows(), 2);
        p.ctrl_key('y', false, &mut w);
        assert_eq!(w.said.last().map(String::as_str), Some("nothing to undo"));
        // In a field, its typing.
        keys(&mut p, &mut w, "e");
        p.paste("x");
        p.ctrl_key('z', false, &mut w);
        p.key(Key::Enter, &mut w);
        assert_eq!(p.table.cell(0, 0), "grace", "the cell as it was");
    }

    #[test]
    fn its_place_kept_in_the_session() {
        let mut p = page(PEOPLE);
        let mut w = Fake::default();
        keys(&mut p, &mut w, "jls");
        let s = serde_json::to_string(&p.saved()).unwrap();
        let mut q = page(PEOPLE);
        q.restore(&serde_json::from_str(&s).unwrap());
        assert_eq!((q.grid.row, q.grid.col, q.sort), (2, 1, Some(Sort { col: 1, desc: false })), "grace, on the row the sort put her");
    }
}
