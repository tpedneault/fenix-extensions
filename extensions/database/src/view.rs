//! Rows on a page: ui-kit's table, sorted by a column, NULLs drawn as
//! such, cells and rows copied; and what the pages share -- the key strip,
//! the focused cell shown whole.

use std::borrow::Cow;

use fenix_ui_kit::table::{Column as UiColumn, TableSource, TableState, Width};
use fenix_ui_kit::text::{fit, width, wrap};
use fenix_ui_kit::{Canvas, Key, Outcome, Rect, Role, Selecting, Style};

use crate::result::{Kind, Order, Rows};

/// The widest a column's drawn.
const CELL_MAX: usize = 40;

/// Rows, as a table shows them.
#[derive(Debug, Clone)]
pub struct ResultView {
    pub rows: Rows,
    /// The rows' order: indices into `rows.rows`.
    order: Vec<usize>,
    pub sort: Option<(usize, Order)>,
    pub table: TableState,
}

impl ResultView {
    pub fn new(rows: Rows) -> ResultView {
        let order = (0..rows.rows.len()).collect();
        let mut table = TableState::new();
        table.empty = "No rows".into();
        ResultView { rows, order, sort: None, table }
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn cols(&self) -> usize {
        self.rows.columns.len()
    }

    /// The row index (into `rows.rows`) shown at `i`.
    pub fn row_at(&self, i: usize) -> Option<usize> {
        self.order.get(i).copied()
    }

    /// The focused cell: its row (into `rows.rows`) and column.
    pub fn focused(&self) -> Option<(usize, usize)> {
        if self.is_empty() || self.cols() == 0 {
            return None;
        }
        Some((self.order[self.table.row.min(self.len() - 1)], self.table.col.min(self.cols() - 1)))
    }

    /// Sorts by the focused column: up, then down, then as they came.
    pub fn cycle_sort(&mut self) {
        let col = self.table.col.min(self.cols().saturating_sub(1));
        let focused = self.focused().map(|(r, _)| r);
        self.sort = match self.sort {
            Some((c, Order::Up)) if c == col => Some((col, Order::Down)),
            Some((c, Order::Down)) if c == col => None,
            _ => Some((col, Order::Up)),
        };
        self.order = match self.sort {
            Some((c, o)) => self.rows.sorted(c, o),
            None => (0..self.rows.rows.len()).collect(),
        };
        // The same row stays focused.
        if let Some(r) = focused {
            self.table.row = self.order.iter().position(|&x| x == r).unwrap_or(0);
        }
    }

    /// The table's columns: titles with the sort's arrow, numbers on the
    /// right.
    pub fn columns(&self) -> Vec<UiColumn> {
        self.rows
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mark = match self.sort {
                    Some((s, Order::Up)) if s == i => " ↑",
                    Some((s, Order::Down)) if s == i => " ↓",
                    _ => "",
                };
                let col = UiColumn::new(format!("{}{mark}", c.name), Width::Fit { min: 1, max: CELL_MAX });
                if c.kind == Kind::Number {
                    col.right()
                } else {
                    col
                }
            })
            .collect()
    }

    pub fn key(&mut self, key: Key) -> Outcome {
        let (n, c) = (self.len(), self.cols());
        self.table.handle_key(key, n, c)
    }

    /// The rows selected, in the order shown -- or the focused one.
    pub fn chosen_rows(&self) -> Vec<usize> {
        match self.table.selection(self.len(), self.cols()) {
            Some((rows, _)) => rows.filter_map(|i| self.row_at(i)).collect(),
            None => self.focused().map(|(r, _)| vec![r]).unwrap_or_default(),
        }
    }

    /// `y`: the focused cell's value, or the selection's cells, a tab
    /// between them and a line a row.
    pub fn copy_cells(&self) -> Option<String> {
        match self.table.selection(self.len(), self.cols()) {
            Some((rows, cols)) => {
                let idx: Vec<usize> = rows.filter_map(|i| self.row_at(i)).collect();
                Some(self.rows.tsv(&idx, cols))
            }
            None => {
                let (r, c) = self.focused()?;
                Some(self.rows.rows[r].get(c).cloned().flatten().unwrap_or_default())
            }
        }
    }

    /// `Y`: the chosen rows as CSV, with the header.
    pub fn copy_rows(&self) -> Option<String> {
        let rows = self.chosen_rows();
        (!rows.is_empty()).then(|| self.rows.csv(Some(&rows)))
    }

    /// Every row, in the order shown: for an export.
    pub fn all_rows(&self) -> Vec<usize> {
        self.order.clone()
    }

    pub fn select(&mut self, how: Selecting) {
        self.table.select(how);
    }

    pub fn render(&self, rect: Rect, canvas: &mut Canvas) {
        let columns = self.columns();
        if columns.is_empty() {
            canvas.put(rect.x, rect.y, "No columns", Role::Muted);
            return;
        }
        self.table.render(&columns, self, rect, canvas);
    }
}

impl TableSource for ResultView {
    fn len(&self) -> usize {
        self.order.len()
    }

    fn row(&self, index: usize) -> Vec<Cow<'_, str>> {
        let r = &self.rows.rows[self.order[index]];
        r.iter()
            .map(|c| match c {
                None => Cow::Borrowed("NULL"),
                Some(v) if v.contains(['\n', '\r']) => Cow::Owned(v.replace("\r\n", "↵").replace(['\n', '\r'], "↵")),
                Some(v) => Cow::Borrowed(v.as_str()),
            })
            .collect()
    }

    fn style(&self, row: usize, col: usize) -> Style {
        let r = &self.rows.rows[self.order[row]];
        match (r.get(col), self.rows.columns.get(col).map(|c| c.kind)) {
            (Some(None), _) => Style::new(Role::Muted),
            (_, Some(Kind::Bytes)) => Style::new(Role::Syntax("string".into())),
            (_, Some(Kind::Bool)) => Style::new(Role::Syntax("constant.builtin".into())),
            _ => Style::default(),
        }
    }
}

/// Keys and what they do, laid out in lines `w` wide: (x, line, keys,
/// what).
pub fn key_strip<'a>(keys: &[(&'a str, &'a str)], w: usize) -> Vec<(usize, usize, &'a str, &'a str)> {
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

/// How many lines a strip takes.
pub fn strip_height(keys: &[(&str, &str)], w: usize) -> usize {
    key_strip(keys, w).last().map_or(0, |k| k.1 + 1)
}

/// The strip drawn from line `y`.
pub fn draw_strip(c: &mut Canvas, keys: &[(&str, &str)], y: usize, w: usize) {
    for (x, dy, k, what) in key_strip(keys, w) {
        let after = c.put(x, y + dy, k, Role::Accent);
        c.put(after + 1, y + dy, what, Role::Muted);
    }
}

/// A cell's value, whole, in `lines` lines `w` wide from line `y`: what
/// the table cuts short.
pub fn draw_detail(c: &mut Canvas, value: Option<&str>, y: usize, w: usize, lines: usize) {
    let Some(v) = value else {
        c.put(0, y, "NULL", Role::Muted);
        return;
    };
    let mut ls = wrap(v, w.max(1));
    if ls.len() > lines {
        ls.truncate(lines);
        let last = ls.pop().unwrap_or_default();
        ls.push(fit(&format!("{last} …"), w));
    }
    for (i, l) in ls.iter().enumerate() {
        c.put(0, y + i, l, Role::Text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::Column;

    fn view() -> ResultView {
        ResultView::new(Rows {
            columns: vec![Column::new("id", "int4", Kind::Number), Column::new("name", "text", Kind::Text)],
            rows: vec![vec![Some("2".into()), Some("b".into())], vec![Some("10".into()), None], vec![Some("1".into()), Some("line\nbreak".into())]],
            truncated: false,
        })
    }

    #[test]
    fn rows_are_drawn_sorted_and_copied() {
        let mut v = view();
        let mut c = Canvas::new();
        v.render(Rect::new(0, 0, 30, 6), &mut c);
        assert_eq!(c.text(), ["id  name", "", " 2  b", "10  NULL", " 1  line↵break"].join("\n"));
        assert_eq!(c.style_at(5, 3), Style::new(Role::Muted), "NULL drawn muted");
        v.key(Key::Char('j'));
        v.cycle_sort();
        assert_eq!(v.sort, Some((0, Order::Up)));
        assert_eq!(v.focused(), Some((1, 0)), "the row focused before stays focused");
        assert_eq!(v.table.row, 2);
        let mut c = Canvas::new();
        v.render(Rect::new(0, 0, 30, 6), &mut c);
        assert_eq!(c.line(0), "id ↑  name");
        assert_eq!(c.line(2), "   1  line↵break");
        assert_eq!(v.copy_cells().as_deref(), Some("10"));
        v.table.col = 1;
        assert_eq!(v.copy_cells().as_deref(), Some(""), "NULL copies as nothing");
        v.select(Selecting::Rows);
        v.key(Key::Char('k'));
        assert_eq!(v.copy_rows().as_deref(), Some("id,name\r\n2,b\r\n10,\r\n"));
        v.cycle_sort();
        v.cycle_sort();
        v.table.col = 0;
        v.cycle_sort();
        v.cycle_sort();
        v.cycle_sort();
        assert_eq!((v.sort, v.all_rows()), (None, vec![0, 1, 2]), "up, down, then as they came");
    }

    #[test]
    fn the_key_strip_wraps() {
        let keys = [("j k", "move"), ("Enter", "open"), ("q", "close")];
        assert_eq!(key_strip(&keys, 20), [(0, 0, "j k", "move"), (10, 0, "Enter", "open"), (0, 1, "q", "close")]);
        assert_eq!(strip_height(&keys, 80), 1);
        let mut c = Canvas::new();
        draw_detail(&mut c, Some("a long value that wraps around"), 0, 10, 2);
        assert_eq!(c.text(), "a long\nvalue tha…", "cut on its last line");
    }
}
