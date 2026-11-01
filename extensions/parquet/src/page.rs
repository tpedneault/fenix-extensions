//! A Parquet file's page: its rows as a table, the focused value whole
//! below it, or its schema -- a row a column, with its type, nulls,
//! range, size and codec -- and the keys for both.

use std::borrow::Cow;

use fenix_ui_kit::{Canvas, Column, Key, Mouse, Outcome, Rect, Role, Style, TableSource, TableState};
use serde::{Deserialize, Serialize};

use crate::file::{bytes, count, Input, ParquetFile};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum View {
    #[default]
    Rows,
    Schema,
}

/// What a key on the page asks of Fenix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Redraw,
    /// Copied, and what's said about it.
    Copy(String, String),
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
    pub view: View,
    pub row: usize,
    pub col: usize,
}

pub struct ParquetPage {
    pub full: String,
    pub path: String,
    pub file: Result<ParquetFile, String>,
    pub view: View,
    rows: TableState,
    schema: TableState,
}

/// `text` as a CSV field: quoted when it has to be.
fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// A cell on one line: its line breaks shown, not taken.
fn one_line(text: &str) -> Cow<'_, str> {
    if text.contains(['\n', '\r']) {
        Cow::Owned(text.replace("\r\n", "⏎").replace(['\n', '\r'], "⏎"))
    } else {
        Cow::Borrowed(text)
    }
}

impl ParquetPage {
    pub fn new(full: &str, path: &str, input: Result<Input, String>) -> ParquetPage {
        ParquetPage {
            full: full.to_string(),
            path: path.to_string(),
            file: input.and_then(ParquetFile::open),
            view: View::Rows,
            rows: TableState::new(),
            schema: TableState::new(),
        }
    }

    /// The file's name: the page's title.
    pub fn name(&self) -> String {
        self.full.rsplit(['/', '\\']).next().unwrap_or(&self.full).to_string()
    }

    pub fn saved(&self) -> Saved {
        Saved { full: self.full.clone(), path: self.path.clone(), view: self.view, row: self.rows.row, col: self.rows.col }
    }

    pub fn restore(&mut self, s: &Saved) {
        self.view = s.view;
        (self.rows.row, self.rows.col) = (s.row, s.col);
    }

    /// The file read again (`R`): where it was, as far as the file still
    /// goes there.
    pub fn reload(&mut self, input: Result<Input, String>) {
        let s = self.saved();
        self.file = input.and_then(ParquetFile::open);
        self.rows.refit();
        self.schema.refit();
        self.restore(&s);
    }

    fn table(&mut self) -> &mut TableState {
        match self.view {
            View::Rows => &mut self.rows,
            View::Schema => &mut self.schema,
        }
    }

    /// The table's size, as the focused view has it: rows by columns.
    fn size(&self) -> (usize, usize) {
        match (&self.file, self.view) {
            (Ok(f), View::Rows) => (f.len(), f.columns.len()),
            (Ok(f), View::Schema) => (f.columns.len(), SCHEMA_COLUMNS.len()),
            (Err(_), _) => (0, 0),
        }
    }

    pub fn render(&self, width: usize, height: usize) -> Canvas {
        let mut c = Canvas::new();
        let (w, h) = (width.max(20), height.max(8));
        let x = c.put(0, 0, &self.name(), Role::Title);
        let f = match &self.file {
            Ok(f) => f,
            Err(e) => {
                c.put_fit(0, 2, w, &format!("Not read as Parquet: {e}"), Role::Bad);
                c.put_fit(0, 4, w, "R reads it again", Role::Muted);
                return c;
            }
        };
        let i = &f.info;
        let groups = if i.row_groups == 1 { "1 row group".to_string() } else { format!("{} row groups", count(i.row_groups as u64)) };
        let about = format!("  {} rows · {} columns · {groups} · {} · {}", count(i.rows), f.columns.len(), i.codecs.join(", ").to_lowercase(), bytes(i.size));
        c.put_fit(x, 0, w.saturating_sub(x), &about, Role::Muted);

        let mut x = 0;
        for (view, label) in [(View::Rows, "Rows"), (View::Schema, "Schema")] {
            x = c.put(x, 1, label, if view == self.view { Role::Accent } else { Role::Muted }) + 2;
        }
        c.put_fit(x, 1, w.saturating_sub(x), "Tab switches", Role::Muted);

        let body = Rect::new(0, 3, w, h.saturating_sub(5));
        let (info_y, value_y) = (h - 2, h - 1);
        match self.view {
            View::Rows => {
                let columns: Vec<Column> = f.columns.iter().map(|c| if c.numeric { Column::fit(c.name.clone(), 40).right() } else { Column::fit(c.name.clone(), 40) }).collect();
                self.rows.render(&columns, &Rows(f), body, &mut c);
                if let Some(e) = f.error() {
                    c.put_fit(0, info_y, w, &format!("Not read: {e}"), Role::Bad);
                } else if let Some(col) = f.columns.get(self.rows.col) {
                    let row = self.rows.row.min(f.len().saturating_sub(1));
                    let at = format!("row {} of {} · ", count(row as u64 + 1), count(f.len() as u64));
                    let x = c.put(0, info_y, &at, Role::Muted);
                    let x = c.put(x, info_y, &col.name, Role::Accent);
                    c.put_fit(x + 1, info_y, w.saturating_sub(x + 1), &col.kind, Role::Muted);
                    if !f.is_empty() {
                        match f.with_row(row, |r| r.get(self.rows.col).cloned().flatten()) {
                            Some(v) => c.put_fit(0, value_y, w, &one_line(&v), Role::Text),
                            None => c.put_fit(0, value_y, w, "null", Role::Muted),
                        };
                    }
                }
            }
            View::Schema => {
                let columns: Vec<Column> = SCHEMA_COLUMNS.iter().map(|&(title, max, right)| if right { Column::fit(title, max).right() } else { Column::fit(title, max) }).collect();
                self.schema.render(&columns, &Schema(f), body, &mut c);
                let mut about = vec![format!("Parquet {}", i.version)];
                if let Some(by) = &i.created_by {
                    about.push(format!("written by {by}"));
                }
                if !i.keys.is_empty() {
                    about.push(format!("metadata: {}", i.keys.join(", ")));
                }
                c.put_fit(0, info_y, w, &about.join(" · "), Role::Muted);
                c.put_fit(0, value_y, w, "Enter shows the column's rows", Role::Muted);
            }
        }
        c
    }

    pub fn key(&mut self, key: Key) -> Action {
        let Ok(f) = &self.file else {
            return if key == Key::Char('R') { Action::Reload } else { Action::Nothing };
        };
        match key {
            Key::Tab | Key::BackTab => {
                self.view = if self.view == View::Rows { View::Schema } else { View::Rows };
                Action::Redraw
            }
            Key::Char('R') => Action::Reload,
            Key::Enter if self.view == View::Schema => {
                if self.schema.row < f.columns.len() {
                    self.rows.col = self.schema.row;
                    self.view = View::Rows;
                }
                Action::Redraw
            }
            Key::Char('y') => match self.view {
                View::Rows if !f.is_empty() => {
                    let col = self.rows.col.min(f.columns.len().saturating_sub(1));
                    let Some(name) = f.columns.get(col).map(|c| c.name.clone()) else { return Action::Nothing };
                    match f.with_row(self.rows.row, |r| r.get(col).cloned().flatten()) {
                        Some(v) => Action::Copy(v, format!("Copied {name}")),
                        None => Action::Copy(String::new(), format!("{name} is null: copied nothing")),
                    }
                }
                View::Schema => match f.columns.get(self.schema.row) {
                    Some(c) => Action::Copy(c.name.clone(), format!("Copied the name {}", c.name)),
                    None => Action::Nothing,
                },
                _ => Action::Nothing,
            },
            Key::Char('Y') => match self.view {
                View::Rows if !f.is_empty() => {
                    let header: Vec<String> = f.columns.iter().map(|c| csv_field(&c.name)).collect();
                    let row: Vec<String> = f.row(self.rows.row).iter().map(|v| v.as_deref().map(csv_field).unwrap_or_default()).collect();
                    Action::Copy(format!("{}\n{}\n", header.join(","), row.join(",")), format!("Copied row {} as CSV", count(self.rows.row as u64 + 1)))
                }
                View::Schema => {
                    let mut out = String::from("column,type,nullable\n");
                    for c in &f.columns {
                        out += &format!("{},{},{}\n", csv_field(&c.name), csv_field(&c.kind), c.nullable);
                    }
                    Action::Copy(out, "Copied the schema as CSV".into())
                }
                _ => Action::Nothing,
            },
            key => {
                let (rows, cols) = self.size();
                match self.table().handle_key(key, rows, cols) {
                    Outcome::Ignored => Action::Nothing,
                    _ => Action::Redraw,
                }
            }
        }
    }

    pub fn mouse(&mut self, m: &Mouse) -> Action {
        let (rows, cols) = self.size();
        match self.table().mouse(m, rows, cols) {
            Outcome::Ignored => Action::Nothing,
            _ => Action::Redraw,
        }
    }
}

/// The schema's columns: title, widest, and whether it's a number.
const SCHEMA_COLUMNS: [(&str, usize, bool); 9] =
    [("Column", 40, false), ("Type", 40, false), ("Stored as", 30, false), ("Null", 4, false), ("Nulls", 14, true), ("Min", 30, false), ("Max", 30, false), ("Size", 10, true), ("Codec", 20, false)];

struct Rows<'a>(&'a ParquetFile);

impl TableSource for Rows<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn row(&self, index: usize) -> Vec<Cow<'_, str>> {
        self.0.with_row(index, |r| r.iter().map(|v| Cow::Owned(v.as_deref().map_or("null".into(), |v| one_line(v).into_owned()))).collect())
    }

    fn style(&self, row: usize, col: usize) -> Style {
        Style::new(if self.0.is_null(row, col) { Role::Muted } else { Role::Text })
    }
}

struct Schema<'a>(&'a ParquetFile);

impl TableSource for Schema<'_> {
    fn len(&self) -> usize {
        self.0.columns.len()
    }

    fn row(&self, index: usize) -> Vec<Cow<'_, str>> {
        let c = &self.0.columns[index];
        let or_blank = |v: &Option<String>| Cow::Owned(v.as_deref().map(one_line).unwrap_or_default().into_owned());
        vec![
            Cow::Borrowed(c.name.as_str()),
            Cow::Borrowed(c.kind.as_str()),
            Cow::Borrowed(c.stored.as_str()),
            Cow::Borrowed(if c.nullable { "yes" } else { "no" }),
            Cow::Owned(c.nulls.map(count).unwrap_or_default()),
            or_blank(&c.min),
            or_blank(&c.max),
            Cow::Owned(bytes(c.compressed)),
            Cow::Owned(c.codecs.join(", ").to_lowercase()),
        ]
    }

    fn style(&self, _row: usize, col: usize) -> Style {
        Style::new(match col {
            0 => Role::Syntax("variable".into()),
            1 => Role::Syntax("type".into()),
            2 | 3 | 8 => Role::Muted,
            _ => Role::Text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(name: &str) -> ParquetPage {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let len = std::fs::metadata(&path).unwrap().len();
        ParquetPage::new(&path, &path, Ok(Input::Path(path.clone(), len)))
    }

    #[test]
    fn the_rows_are_shown_with_the_focused_value_whole_below() {
        let p = page("types-zstd.parquet");
        let text = p.render(140, 20).text();
        assert!(text.contains("types-zstd.parquet  1,000 rows · 9 columns · 4 row groups · zstd"), "{text}");
        assert!(text.contains("craft-1"), "{text}");
        assert!(text.contains("row 1 of 1,000 · id Int64"), "{text}");
    }

    #[test]
    fn keys_move_through_the_rows_and_copy_them() {
        let mut p = page("types-snappy.parquet");
        assert_eq!(p.key(Key::Char('G')), Action::Redraw);
        assert!(p.render(140, 20).text().contains("row 1,000 of 1,000"));
        assert_eq!(p.key(Key::Char('l')), Action::Redraw);
        assert_eq!(p.key(Key::Char('y')), Action::Copy("craft-999".into(), "Copied name".into()));
        p.key(Key::Char('g'));
        assert_eq!(p.key(Key::Char('y')), Action::Copy(String::new(), "name is null: copied nothing".into()));
        let Action::Copy(csv, _) = p.key(Key::Char('Y')) else { panic!() };
        assert_eq!(csv.lines().next(), Some("id,name,score,ok,day,at,price,tags,pos"));
        assert!(csv.lines().nth(1).unwrap().starts_with("1,,0.0,true,2026-01-01,2026-10-31T12:00:00Z,0.00,[],"), "{csv}");
        assert!(csv.contains("\"{x: 0, y: 0}\""), "a field with a comma is quoted: {csv}");
    }

    #[test]
    fn the_schema_lists_each_column_and_enter_goes_to_its_rows() {
        let mut p = page("types-zstd.parquet");
        p.key(Key::Tab);
        let text = p.render(200, 20).text();
        assert!(text.contains("Column"), "{text}");
        assert!(text.contains("Decimal128(10, 2)"), "{text}");
        assert!(text.contains("ARROW:schema"), "{text}");
        p.key(Key::Char('j'));
        p.key(Key::Char('j'));
        p.key(Key::Enter);
        assert_eq!(p.view, View::Rows);
        assert!(p.render(140, 20).text().contains("· score Float64"));
    }

    #[test]
    fn a_page_comes_back_where_it_was() {
        let mut p = page("types-gzip.parquet");
        p.key(Key::Char('G'));
        p.key(Key::Char('$'));
        p.key(Key::Tab);
        let mut again = page("types-gzip.parquet");
        again.restore(&p.saved());
        assert_eq!(again.saved(), p.saved());
        assert_eq!(again.view, View::Schema);
    }

    #[test]
    fn a_file_that_isnt_parquet_says_why() {
        let mut p = ParquetPage::new("/x/a.parquet", "/x/a.parquet", Ok(Input::Bytes(bytes::Bytes::from_static(b"nope"))));
        assert!(p.render(80, 10).text().contains("Not read as Parquet"));
        assert_eq!(p.key(Key::Char('j')), Action::Nothing);
        assert_eq!(p.key(Key::Char('R')), Action::Reload);
        let p = ParquetPage::new("/x/b.parquet", "/x/b.parquet", Err("too big".into()));
        assert!(p.render(80, 10).text().contains("Not read as Parquet: too big"));
    }
}
