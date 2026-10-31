//! A table's page: its rows a page at a time, by its key's order, those a
//! filter lets through; a cell changed with an `UPDATE` shown to you
//! first, keyed by the row's primary key.

use fenix_ui_kit::{Canvas, Key, KeyPress, Mouse, Outcome, Popup, Rect, Role, Selecting, Style, TextField};

use crate::edit::{Target, ROWID};
use crate::result::{thousands, Cell, Kind, Outcome as Came, Rows};
use crate::view::{draw_detail, draw_strip, strip_height, ResultView};

/// What a key on it asks of the extension.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    /// Its page of rows, and the count, asked for again.
    Fetch,
    /// The UPDATE you said yes to.
    Update(String),
    Copy(String),
}

#[derive(Debug, Clone)]
pub enum Mode {
    Browse,
    Filter(TextField),
    /// A cell being changed: its row (in the page's rows) and column.
    Edit {
        row: usize,
        col: usize,
        field: TextField,
    },
    /// The UPDATE, asked about.
    Confirm(String),
}

#[derive(Debug, Clone)]
pub struct TableView {
    pub conn: String,
    pub target: Target,
    pub filter: String,
    pub page: usize,
    pub size: usize,
    pub total: Option<u64>,
    pub view: Option<ResultView>,
    pub error: Option<String>,
    pub mode: Mode,
    pub loading: bool,
    pub note: Option<(String, Role)>,
    /// The connection may not be written to: no editing.
    pub read_only: bool,
}

/// Its state kept with the session.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Saved {
    pub conn: String,
    pub schema: Option<String>,
    pub table: String,
    pub filter: String,
    pub page: usize,
}

impl TableView {
    pub fn new(conn: &str, target: Target, size: usize, read_only: bool) -> TableView {
        TableView { conn: conn.into(), target, filter: String::new(), page: 0, size: size.max(1), total: None, view: None, error: None, mode: Mode::Browse, loading: true, note: None, read_only }
    }

    pub fn title(&self) -> String {
        match &self.target.schema {
            Some(s) if !s.is_empty() && s != "main" => format!("{s}.{}", self.target.table),
            _ => self.target.table.clone(),
        }
    }

    pub fn saved(&self) -> Saved {
        Saved { conn: self.conn.clone(), schema: self.target.schema.clone(), table: self.target.table.clone(), filter: self.filter.clone(), page: self.page }
    }

    /// The page's query, and the count's.
    pub fn queries(&self) -> (String, String) {
        (self.target.select(&self.filter, self.page, self.size), self.target.count(&self.filter))
    }

    fn pages(&self) -> Option<usize> {
        self.total.map(|t| (t as usize).div_ceil(self.size).max(1))
    }

    /// Its page of rows came.
    pub fn rows_came(&mut self, outcomes: Vec<Came>) {
        self.loading = false;
        match outcomes.into_iter().next() {
            Some(Came::Rows(rows)) => {
                let (row, col) = self.view.as_ref().map(|v| (v.table.row, v.table.col)).unwrap_or((0, 0));
                let mut v = ResultView::new(rows);
                v.table.row = row.min(v.len().saturating_sub(1));
                v.table.col = col;
                self.view = Some(v);
                self.error = None;
            }
            Some(Came::Error(e)) => self.error = Some(e),
            Some(Came::Done { .. }) | None => self.error = Some("the query gave no rows back".into()),
        }
    }

    pub fn count_came(&mut self, outcomes: Vec<Came>) {
        if let Some(Came::Rows(r)) = outcomes.first() {
            self.total = r.rows.first().and_then(|row| row.first()).cloned().flatten().and_then(|n| n.trim().parse().ok());
        }
    }

    /// How the UPDATE went: one row, or none, or an error.
    pub fn update_came(&mut self, outcomes: &[Came]) -> bool {
        match outcomes.first() {
            Some(Came::Done { changed: Some(1), .. }) => {
                self.note = Some(("Changed".into(), Role::Good));
                true
            }
            Some(Came::Done { changed: Some(0), .. }) => {
                self.note = Some(("No row changed: it's gone, or its key changed since".into(), Role::Warn));
                true
            }
            Some(Came::Done { changed, .. }) => {
                self.note = Some((format!("{} rows changed", changed.map_or("Some".into(), thousands)), Role::Warn));
                true
            }
            Some(Came::Error(e)) => {
                self.note = Some((e.clone(), Role::Bad));
                false
            }
            _ => false,
        }
    }

    pub fn typing(&self) -> bool {
        matches!(self.mode, Mode::Filter(_) | Mode::Edit { .. })
    }

    /// The row's key's values, as the UPDATE finds it by.
    fn key_of(&self, rows: &Rows, row: usize) -> Result<Vec<(Cell, Kind)>, String> {
        self.target
            .keyed_by()
            .iter()
            .map(|k| {
                let i = rows.columns.iter().position(|c| &c.name == k).ok_or_else(|| format!("the key's {k} isn't in the view"))?;
                Ok((rows.rows[row][i].clone(), rows.columns[i].kind))
            })
            .collect()
    }

    /// The UPDATE that sets the cell at (`row`, `col`) to `value`.
    pub fn update_for(&self, row: usize, col: usize, value: Cell) -> Result<String, String> {
        let view = self.view.as_ref().ok_or("no rows")?;
        let rows = &view.rows;
        let column = rows.columns.get(col).ok_or("no such column")?;
        if self.target.rowid && column.name == ROWID {
            return Err("the rowid is SQLite's: it isn't changed here".into());
        }
        if column.kind == Kind::Bytes {
            return Err(format!("{} holds bytes: they aren't changed here", column.name));
        }
        let key = self.key_of(rows, row)?;
        self.target.update(&column.name, &value, column.kind, &key)
    }

    fn start_edit(&mut self) {
        if self.read_only {
            self.note = Some((format!("{} is read-only", self.conn), Role::Warn));
            return;
        }
        let Some((row, col)) = self.view.as_ref().and_then(ResultView::focused) else { return };
        if self.target.keyed_by().is_empty() {
            self.note = Some((format!("{} has no primary key: there's no telling its rows apart to change one", self.target.table), Role::Warn));
            return;
        }
        let value = self.view.as_ref().and_then(|v| v.rows.rows[row][col].clone()).unwrap_or_default();
        let mut field = TextField::new().with_text(&value);
        field.focused = true;
        field.select_all();
        self.mode = Mode::Edit { row, col, field };
    }

    fn propose(&mut self, row: usize, col: usize, value: Cell) {
        match self.update_for(row, col, value) {
            Ok(sql) => self.mode = Mode::Confirm(sql),
            Err(e) => {
                self.mode = Mode::Browse;
                self.note = Some((e, Role::Warn));
            }
        }
    }

    pub fn key(&mut self, key: Key) -> Option<Ask> {
        if !matches!(self.mode, Mode::Confirm(_)) {
            self.note = None;
        }
        match &mut self.mode {
            Mode::Filter(field) => {
                match field.handle_key(key) {
                    Outcome::Submit => {
                        self.filter = field.text().trim().to_string();
                        self.page = 0;
                        self.mode = Mode::Browse;
                        self.loading = true;
                        return Some(Ask::Fetch);
                    }
                    Outcome::Cancel => self.mode = Mode::Browse,
                    _ => {}
                }
                return None;
            }
            Mode::Edit { row, col, field } => {
                let (row, col) = (*row, *col);
                match field.handle_key(key) {
                    Outcome::Submit => {
                        let text = field.text().to_string();
                        self.propose(row, col, Some(text));
                    }
                    Outcome::Cancel => self.mode = Mode::Browse,
                    _ => {}
                }
                return None;
            }
            Mode::Confirm(sql) => {
                let sql = sql.clone();
                self.mode = Mode::Browse;
                return match key {
                    Key::Char('y') | Key::Enter => Some(Ask::Update(sql)),
                    _ => {
                        self.note = Some(("Left as it was".into(), Role::Muted));
                        None
                    }
                };
            }
            Mode::Browse => {}
        }
        match key {
            Key::Char('/') => {
                let mut f = TextField::new().with_text(&self.filter).with_placeholder("a WHERE's condition: age > 30 AND name LIKE 'a%'");
                f.focused = true;
                self.mode = Mode::Filter(f);
                None
            }
            Key::Char('n') | Key::Char(']') => {
                if self.pages().is_none_or(|p| self.page + 1 < p) {
                    self.page += 1;
                    self.loading = true;
                    return Some(Ask::Fetch);
                }
                None
            }
            Key::Char('p') | Key::Char('[') => {
                if self.page > 0 {
                    self.page -= 1;
                    self.loading = true;
                    return Some(Ask::Fetch);
                }
                None
            }
            Key::Char('r') => {
                self.loading = true;
                Some(Ask::Fetch)
            }
            Key::Char('e') | Key::Enter => {
                self.start_edit();
                None
            }
            Key::Char('D') => {
                if self.read_only {
                    self.note = Some((format!("{} is read-only", self.conn), Role::Warn));
                    return None;
                }
                let (row, col) = self.view.as_ref().and_then(ResultView::focused)?;
                self.propose(row, col, None);
                None
            }
            Key::Char('y') => self.view.as_ref().and_then(ResultView::copy_cells).map(Ask::Copy),
            Key::Char('Y') => self.view.as_ref().and_then(ResultView::copy_rows).map(Ask::Copy),
            Key::Char('s') => {
                if let Some(v) = &mut self.view {
                    v.cycle_sort();
                }
                None
            }
            Key::Char('v') => {
                if let Some(v) = &mut self.view {
                    v.select(Selecting::Cells);
                }
                None
            }
            Key::Char('V') => {
                if let Some(v) = &mut self.view {
                    v.select(Selecting::Rows);
                }
                None
            }
            Key::Escape => {
                if let Some(v) = &mut self.view {
                    v.table.clear_selection();
                }
                None
            }
            k => {
                if let Some(v) = &mut self.view {
                    v.key(k);
                }
                None
            }
        }
    }

    /// A key Fenix gives only to a page that takes more: PageUp and
    /// PageDown a page of rows; Ctrl+Z in a field, its undo.
    pub fn key_press(&mut self, press: KeyPress) -> Option<Ask> {
        match (&mut self.mode, press.key) {
            (Mode::Filter(f) | Mode::Edit { field: f, .. }, _) => {
                f.handle_press(press);
                None
            }
            (_, Key::PageDown) => self.key(Key::Char('n')),
            (_, Key::PageUp) => self.key(Key::Char('p')),
            _ => None,
        }
    }

    pub fn paste(&mut self, text: &str) {
        if let Mode::Filter(f) | Mode::Edit { field: f, .. } = &mut self.mode {
            f.paste(text);
        }
    }

    pub fn mouse(&mut self, m: &Mouse) -> Option<Ask> {
        let view = self.view.as_mut()?;
        let (n, c) = (view.len(), view.cols());
        if view.table.mouse(m, n, c) == Outcome::Submit && matches!(self.mode, Mode::Browse) {
            self.start_edit();
        }
        None
    }

    fn keys(&self) -> Vec<(&'static str, &'static str)> {
        match &self.mode {
            Mode::Filter(_) => vec![("Enter", "filter"), ("Esc", "leave it"), ("Enter on nothing", "every row")],
            Mode::Edit { .. } => vec![("Enter", "change it"), ("Esc", "leave it")],
            Mode::Confirm(_) => vec![("y", "run it"), ("n", "don't")],
            Mode::Browse => {
                let mut k = vec![("hjkl", "move"), ("n p", "next, last page"), ("/", "filter"), ("r", "again")];
                if !self.read_only {
                    k.extend([("e", "change the cell"), ("D", "set it NULL")]);
                }
                k.extend([("s", "sort the page"), ("v V", "select"), ("y Y", "copy cell, rows")]);
                k
            }
        }
    }

    pub fn render(&self, w: usize, h: usize) -> Canvas {
        let w = w.max(30);
        let h = if h == 0 { 30 } else { h.max(10) };
        let mut c = Canvas::new();
        c.set_more_keys(true);
        c.set_mouse(true);
        let x = c.put(0, 0, &self.title(), Role::Title);
        let mut about = vec![self.conn.clone()];
        if let Some(t) = self.total {
            about.push(format!("{} row{}", thousands(t), if t == 1 { "" } else { "s" }));
        }
        about.push(match self.pages() {
            Some(p) => format!("page {} of {}", self.page + 1, thousands(p as u64)),
            None => format!("page {}", self.page + 1),
        });
        if self.target.keyed_by().is_empty() {
            about.push("no primary key".into());
        }
        if self.read_only {
            about.push("read-only".into());
        }
        if self.loading {
            about.push("reading…".into());
        }
        c.put_fit(x + 2, 0, w.saturating_sub(x + 2), &about.join(" · "), Role::Muted);
        match &self.mode {
            Mode::Filter(f) => {
                c.put(0, 1, "/", Role::Accent);
                f.render(Rect::new(2, 1, w - 2, 1), &mut c);
            }
            _ if !self.filter.is_empty() => {
                let x = c.put(0, 1, "where ", Role::Muted);
                c.put_fit(x, 1, w - x, &self.filter, Role::Accent);
            }
            _ => {}
        }
        let keys = self.keys();
        let strip = strip_height(&keys, w);
        let detail = 3;
        let table_h = h.saturating_sub(2 + 1 + detail + 1 + strip);
        let top = 2;
        match (&self.view, &self.error) {
            (_, Some(e)) => {
                for (i, line) in fenix_ui_kit::text::wrap(e, w).into_iter().take(table_h).enumerate() {
                    c.put(0, top + i, &line, Role::Bad);
                }
            }
            (Some(v), None) => v.render(Rect::new(0, top, w, table_h), &mut c),
            (None, None) => {
                c.put(0, top, "Reading its rows…", Role::Muted);
            }
        }
        let mut y = top + table_h;
        c.rule(y, 0..w);
        y += 1;
        match &self.mode {
            Mode::Edit { col, field, .. } => {
                let name = self.view.as_ref().and_then(|v| v.rows.columns.get(*col)).map(|c| c.name.clone()).unwrap_or_default();
                let x = c.put(0, y, &name, Role::Accent);
                c.put(x + 2, y, "Enter changes it, Esc leaves it", Role::Muted);
                field.render(Rect::new(0, y + 1, w, 1), &mut c);
            }
            _ => {
                if let Some((r, col)) = self.view.as_ref().and_then(ResultView::focused) {
                    let v = self.view.as_ref().expect("focused");
                    let column = &v.rows.columns[col];
                    let mut x = c.put(0, y, &column.name, Role::Accent);
                    x = c.put(x + 1, y, &column.type_name, Role::Muted);
                    let at = self.page * self.size + v.table.row.min(v.len().saturating_sub(1)) + 1;
                    c.put(x + 2, y, &format!("row {}", thousands(at as u64)), Role::Muted);
                    draw_detail(&mut c, v.rows.rows[r].get(col).and_then(|x| x.as_deref()), y + 1, w, detail - 1);
                }
            }
        }
        y += detail;
        if let Some((note, role)) = &self.note {
            c.put_fit(0, y, w, note, role.clone());
        }
        draw_strip(&mut c, &keys, y + 1, w);
        if let Mode::Confirm(sql) = &self.mode {
            let mut rows = vec![vec![("Run this?".to_string(), Style::new(Role::Title))]];
            for line in fenix_ui_kit::text::wrap(sql, w.saturating_sub(6).max(20)) {
                rows.push(vec![(line, Style::new(Role::Syntax("keyword".into())))]);
            }
            rows.push(vec![
                ("y".into(), Style::new(Role::Accent)),
                (" runs it   ".into(), Style::new(Role::Muted)),
                ("n".into(), Style::new(Role::Accent)),
                (" leaves it".into(), Style::new(Role::Muted)),
            ]);
            c.popup(Popup { line: top + table_h, col: 2, rows });
        }
        c.set_typing(self.typing());
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::Column;
    use crate::sql::Dialect;

    fn users() -> TableView {
        let t = Target { dialect: Dialect::Postgres, schema: Some("app".into()), table: "users".into(), key: vec!["id".into()], rowid: false };
        let mut v = TableView::new("dev", t, 2, false);
        v.rows_came(vec![Came::Rows(Rows {
            columns: vec![Column::new("id", "int4", Kind::Number), Column::new("name", "text", Kind::Text), Column::new("data", "bytea", Kind::Bytes)],
            rows: vec![vec![Some("1".into()), Some("ada".into()), None], vec![Some("2".into()), Some("grace".into()), None]],
            truncated: false,
        })]);
        v.count_came(vec![Came::Rows(Rows { columns: vec![Column::new("count", "int8", Kind::Number)], rows: vec![vec![Some("5".into())]], truncated: false })]);
        v
    }

    fn type_text(v: &mut TableView, text: &str) {
        for ch in text.chars() {
            v.key(Key::Char(ch));
        }
    }

    #[test]
    fn a_cell_is_changed_after_you_say_yes() {
        let mut v = users();
        v.key(Key::Char('j'));
        v.key(Key::Char('l'));
        assert_eq!(v.key(Key::Char('e')), None);
        assert!(v.typing());
        type_text(&mut v, "Grace O'Neil");
        v.key(Key::Enter);
        let Mode::Confirm(sql) = &v.mode else { panic!("{:?}", v.mode) };
        assert_eq!(sql, "UPDATE \"app\".\"users\" SET \"name\" = 'Grace O''Neil' WHERE \"id\" = 2");
        assert!(v.render(80, 24).popup_ref().is_some(), "asked in a popup");
        assert_eq!(v.key(Key::Char('y')), Some(Ask::Update("UPDATE \"app\".\"users\" SET \"name\" = 'Grace O''Neil' WHERE \"id\" = 2".into())));
        assert!(v.update_came(&[Came::Done { tag: "UPDATE 1".into(), changed: Some(1) }]));
        // NULL, and a no.
        v.key(Key::Char('D'));
        assert!(matches!(&v.mode, Mode::Confirm(s) if s.ends_with("SET \"name\" = NULL WHERE \"id\" = 2")));
        assert_eq!(v.key(Key::Char('n')), None);
        assert!(matches!(v.mode, Mode::Browse));
        // Bytes aren't changed here.
        v.key(Key::Char('l'));
        v.key(Key::Char('e'));
        v.key(Key::Enter);
        assert!(v.note.as_ref().is_some_and(|n| n.0.contains("bytes")));
    }

    #[test]
    fn pages_filters_and_read_only() {
        let mut v = users();
        assert_eq!(v.queries().0, "SELECT * FROM \"app\".\"users\" ORDER BY \"id\" LIMIT 2 OFFSET 0");
        assert_eq!(v.key(Key::Char('n')), Some(Ask::Fetch));
        assert_eq!(v.queries().0, "SELECT * FROM \"app\".\"users\" ORDER BY \"id\" LIMIT 2 OFFSET 2");
        v.key(Key::Char('n'));
        assert_eq!(v.key(Key::Char('n')), None, "5 rows: 3 pages");
        v.key(Key::Char('/'));
        type_text(&mut v, "name like 'a%'");
        assert_eq!(v.key(Key::Enter), Some(Ask::Fetch));
        assert_eq!((v.page, v.queries().1.as_str()), (0, "SELECT COUNT(*) FROM \"app\".\"users\" WHERE name like 'a%'"));
        let text = v.render(80, 20).text();
        assert!(text.contains("users  dev · 5 rows · page 1 of 3 · reading…"), "{text}");
        assert!(text.contains("where name like 'a%'"));
        v.read_only = true;
        v.key(Key::Char('e'));
        assert!(matches!(v.mode, Mode::Browse));
        assert!(v.note.as_ref().is_some_and(|n| n.0.contains("read-only")));
        let mut none = users();
        none.target.key.clear();
        none.key(Key::Char('e'));
        assert!(none.note.as_ref().is_some_and(|n| n.0.contains("no primary key")));
        assert!(v.update_came(&[Came::Done { tag: String::new(), changed: Some(0) }]) && v.note.as_ref().is_some_and(|n| n.1 == Role::Warn));
        assert!(!v.update_came(&[Came::Error("denied".into())]));
    }
}
