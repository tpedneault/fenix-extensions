//! A table: columns fixed, fitted to what they hold, or sharing what's
//! left; a header that stays as the rows scroll under it; a focused cell
//! moved with `hjkl`; rows asked of a [`TableSource`] only as they're
//! shown, so a million-row file draws as fast as a ten-row one.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::ops::Range;

use crate::motion::{page, step, top_for};
use crate::scroll::scrollbar;
use crate::text::{fit, width};
use crate::mouse::wheel_step;
use crate::{Canvas, Fill, Key, Mouse, Outcome, Rect, Role, Style};

/// How wide a column is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    /// This many cells.
    Fixed(usize),
    /// As wide as its title and the widest cell shown so far, within
    /// `min ..= max`. It grows as wider cells scroll into view and doesn't
    /// shrink back ([`TableState::refit`] starts it over).
    Fit { min: usize, max: usize },
    /// A share of what the others leave, by weight; at least its title.
    Flex(usize),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub title: String,
    pub width: Width,
    pub align: Align,
}

impl Column {
    pub fn new(title: impl Into<String>, width: Width) -> Column {
        Column { title: title.into(), width, align: Align::Left }
    }

    /// Fitted to what it holds, up to `max`.
    pub fn fit(title: impl Into<String>, max: usize) -> Column {
        Column::new(title, Width::Fit { min: 1, max })
    }

    /// Lined up on the right: numbers.
    pub fn right(mut self) -> Column {
        self.align = Align::Right;
        self
    }
}

/// Where a table's rows come from. Only the rows shown are asked for.
pub trait TableSource {
    /// How many rows there are.
    fn len(&self) -> usize;

    /// Row `index`'s cells, a column each (missing ones are blank).
    fn row(&self, index: usize) -> Vec<Cow<'_, str>>;

    /// How cell (`row`, `col`) is drawn.
    fn style(&self, _row: usize, _col: usize) -> Style {
        Style::default()
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<S: AsRef<str>> TableSource for [Vec<S>] {
    fn len(&self) -> usize {
        <[Vec<S>]>::len(self)
    }

    fn row(&self, index: usize) -> Vec<Cow<'_, str>> {
        self[index].iter().map(|c| Cow::Borrowed(c.as_ref())).collect()
    }
}

impl<S: AsRef<str>> TableSource for Vec<Vec<S>> {
    fn len(&self) -> usize {
        self.as_slice().len()
    }

    fn row(&self, index: usize) -> Vec<Cow<'_, str>> {
        self.as_slice().row(index)
    }
}

/// What a table's selection takes in: the cells from where it started to
/// the focused one, or whole rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selecting {
    Cells,
    Rows,
}

/// A table's focused cell, its selection, and where it's scrolled to.
#[derive(Debug, Clone)]
pub struct TableState {
    /// The focused row and column.
    pub row: usize,
    pub col: usize,
    /// Whether the table has the keyboard: its row is the canvas's focus
    /// and its cell is drawn as focused.
    pub focused: bool,
    /// Cells between columns.
    pub gap: usize,
    /// What's drawn when there are no rows.
    pub empty: String,
    /// Where the selection started, and what it takes in.
    anchor: Option<(usize, usize, Selecting)>,
    top: Cell<usize>,
    left: Cell<usize>,
    view: Cell<usize>,
    fitted: RefCell<Vec<usize>>,
    area: Cell<Rect>,
    /// The columns last drawn: each one's index, its first cell and its
    /// width.
    drawn: RefCell<Vec<(usize, usize, usize)>>,
}

impl Default for TableState {
    fn default() -> TableState {
        TableState {
            row: 0,
            col: 0,
            focused: true,
            gap: 2,
            empty: "No rows".into(),
            anchor: None,
            top: Cell::new(0),
            left: Cell::new(0),
            view: Cell::new(0),
            fitted: RefCell::new(Vec::new()),
            area: Cell::new(Rect::default()),
            drawn: RefCell::new(Vec::new()),
        }
    }
}

impl TableState {
    pub fn new() -> TableState {
        TableState::default()
    }

    /// The rows the last render showed.
    pub fn visible_rows(&self) -> Range<usize> {
        self.top.get()..self.top.get() + self.view.get()
    }

    /// The first column the last render showed.
    pub fn first_column(&self) -> usize {
        self.left.get()
    }

    /// The columns the last render showed, and the cells each took: a
    /// cell's whole width, for a page drawing its background.
    pub fn columns_shown(&self) -> Vec<(usize, Range<usize>)> {
        self.drawn.borrow().iter().map(|&(c, x, w)| (c, x..x + w)).collect()
    }

    /// A selection started at the focused cell; one of the same kind
    /// already there ends instead (`v` again), one of the other kind
    /// becomes this kind.
    pub fn select(&mut self, how: Selecting) {
        self.anchor = match self.anchor {
            Some((_, _, was)) if was == how => None,
            Some((r, c, _)) => Some((r, c, how)),
            None => Some((self.row, self.col, how)),
        };
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// What's selected, in a table of `rows` by `cols`: its rows and
    /// columns, from the anchor to the focused cell either way round.
    pub fn selection(&self, rows: usize, cols: usize) -> Option<(Range<usize>, Range<usize>)> {
        let (ar, ac, how) = self.anchor?;
        if rows == 0 || cols == 0 {
            return None;
        }
        let (ar, r) = (ar.min(rows - 1), self.row.min(rows - 1));
        let (ac, c) = (ac.min(cols - 1), self.col.min(cols - 1));
        let rows = ar.min(r)..ar.max(r) + 1;
        Some(match how {
            Selecting::Cells => (rows, ac.min(c)..ac.max(c) + 1),
            Selecting::Rows => (rows, 0..cols),
        })
    }

    /// The kind of selection there is, if there's one.
    pub fn selecting(&self) -> Option<Selecting> {
        self.anchor.map(|a| a.2)
    }

    /// Fitted columns measured again from what's next shown: the rows
    /// changed.
    pub fn refit(&self) {
        self.fitted.borrow_mut().clear();
    }

    /// `j` `k` `h` `l` and the arrows a cell, `J` `K` (`PageDown`,
    /// `PageUp`) a screen, `g` `G` (`Home`, `End`) the first and last
    /// row, `0` `$` the first and last column, `Enter` to open the row;
    /// in a table of `rows` by `cols`.
    pub fn handle_key(&mut self, key: Key, rows: usize, cols: usize) -> Outcome {
        let (row, col) = (self.row.min(rows.saturating_sub(1)), self.col.min(cols.saturating_sub(1)));
        let (mut r, mut c) = (row, col);
        match key {
            Key::Enter if rows > 0 => return Outcome::Submit,
            Key::Left | Key::Char('h') => c = c.saturating_sub(1),
            Key::Right | Key::Char('l') => c = (c + 1).min(cols.saturating_sub(1)),
            Key::Char('0') => c = 0,
            Key::Char('$') => c = cols.saturating_sub(1),
            key => match step(key, r, rows, page(self.view.get())) {
                Some(to) => r = to,
                None => return Outcome::Ignored,
            },
        }
        let moved = (r, c) != (self.row, self.col);
        (self.row, self.col) = (r, c);
        if moved {
            Outcome::Moved
        } else {
            Outcome::Ignored
        }
    }

    /// Draws `source` in `columns` into `rect`: the header, a hairline,
    /// then as many rows as fit, a scrollbar on the right when they don't
    /// all. The focused cell is kept in view.
    pub fn render<S: TableSource + ?Sized>(&self, columns: &[Column], source: &S, rect: Rect, canvas: &mut Canvas) {
        self.area.set(rect);
        let n = source.len();
        let body_h = rect.h.saturating_sub(2);
        self.view.set(body_h);
        if rect.is_empty() || columns.is_empty() {
            return;
        }
        let bar = n > body_h && rect.w > 1;
        let w = rect.w - usize::from(bar);
        let row = self.row.min(n.saturating_sub(1));
        let focus_col = self.col.min(columns.len() - 1);
        let top = top_for(self.top.get(), row, body_h, n);
        self.top.set(top);
        let shown: Vec<(usize, Vec<Cow<'_, str>>)> = (top..n.min(top + body_h)).map(|i| (i, source.row(i))).collect();

        // What each column needs: fitted ones measured on what's shown.
        let natural: Vec<usize> = {
            let mut fitted = self.fitted.borrow_mut();
            fitted.resize(columns.len(), 0);
            columns
                .iter()
                .enumerate()
                .map(|(c, col)| match col.width {
                    Width::Fixed(n) => n,
                    Width::Fit { min, max } => {
                        let widest = shown.iter().filter_map(|(_, cells)| cells.get(c)).map(|s| width(s)).max().unwrap_or(0);
                        fitted[c] = fitted[c].max(widest).max(width(&col.title));
                        fitted[c].clamp(min, max.max(min))
                    }
                    Width::Flex(_) => width(&col.title).max(1),
                })
                .collect()
        };
        let gap = self.gap;
        let span = |from: usize, to: usize| natural[from..=to].iter().sum::<usize>() + gap * (to - from);

        // The first column: no later than the focused one, late enough
        // for it to fit, and no later than it needs to be.
        let last = columns.len() - 1;
        let mut left = self.left.get().min(focus_col);
        while left < focus_col && span(left, focus_col) > w {
            left += 1;
        }
        while left > 0 && span(left - 1, last) <= w {
            left -= 1;
        }
        self.left.set(left);

        // The columns shown, and where; flexible ones share what's left
        // when the last one fits.
        let mut widths: Vec<(usize, usize)> = Vec::new();
        let mut used = 0;
        for (c, &nat) in natural.iter().enumerate().skip(left) {
            let start = if widths.is_empty() { 0 } else { used + gap };
            if start >= w {
                break;
            }
            let cw = nat.min(w - start);
            widths.push((c, cw));
            used = start + cw;
        }
        let all_fit = widths.last().is_some_and(|&(c, cw)| c == last && cw == natural[c]);
        if all_fit && used < w {
            let weights: Vec<usize> = widths.iter().map(|&(c, _)| if let Width::Flex(k) = columns[c].width { k } else { 0 }).collect();
            let total: usize = weights.iter().sum();
            let spare = w - used;
            if total > 0 {
                let mut given = 0;
                let mut seen = 0;
                for (i, k) in weights.iter().enumerate() {
                    if *k == 0 {
                        continue;
                    }
                    seen += k;
                    // Shared out so the shares add up to what's spare.
                    let upto = (spare * seen).checked_div(total).unwrap_or(0);
                    widths[i].1 += upto - given;
                    given = upto;
                }
            }
        }
        let mut xs = Vec::new();
        let mut x = rect.x;
        for &(c, cw) in &widths {
            xs.push((c, x, cw));
            x += cw + gap;
        }
        *self.drawn.borrow_mut() = xs.clone();

        // The header, frozen.
        for &(c, x, cw) in &xs {
            let style = if c == focus_col && self.focused { Role::Accent } else { Role::Muted };
            put_cell(canvas, x, rect.y, cw, &columns[c].title, columns[c].align, Style::new(style));
        }
        if rect.h >= 2 {
            canvas.rule(rect.y + 1, rect.x..rect.x + w);
        }
        if body_h == 0 {
            return;
        }
        let body_y = rect.y + 2;
        let selected = self.selection(n, columns.len());
        if n == 0 {
            canvas.put_fit(rect.x, body_y, w, &self.empty, Role::Muted);
            return;
        }
        for (i, cells) in &shown {
            let y = body_y + (i - top);
            for &(c, x, cw) in &xs {
                let text = cells.get(c).map(|s| s.as_ref()).unwrap_or("");
                let mut style = source.style(*i, c);
                if selected.as_ref().is_some_and(|(rs, cs)| rs.contains(i) && cs.contains(&c)) {
                    style = Style::new(Role::Accent).bg(Fill::Selection);
                }
                if self.focused && *i == row && c == focus_col {
                    style = Style::new(Role::Accent).bg(Fill::Focus);
                }
                put_cell(canvas, x, y, cw, text, columns[c].align, style);
            }
        }
        if self.focused {
            canvas.focus(body_y + (row - top), rect.x..rect.x + w);
        }
        if bar {
            scrollbar(canvas, rect.x + w, body_y, body_h, n, top);
        }
    }

    /// A left click at (`x`, `y`), in a table of `rows` by `cols`.
    pub fn click(&mut self, x: usize, y: usize, rows: usize, cols: usize) -> Outcome {
        self.mouse(&Mouse::click(x, y), rows, cols)
    }

    /// The mouse on a table of `rows` by `cols`, as last drawn: a click
    /// focuses the cell (on the header, its column), a double click
    /// submits it (as `Enter`), the wheel moves three rows a notch.
    pub fn mouse(&mut self, m: &Mouse, rows: usize, cols: usize) -> Outcome {
        let rect = self.area.get();
        if !rect.contains(m.x, m.y) || cols == 0 {
            return Outcome::Ignored;
        }
        let (was_row, was_col) = (self.row, self.col);
        if let Some(lines) = m.wheel() {
            if let Some(to) = wheel_step(self.row.min(rows.saturating_sub(1)), rows, lines) {
                self.row = to;
            }
        } else if m.left_clicks() > 0 {
            let gap = self.gap;
            if let Some(&(c, _, _)) = self.drawn.borrow().iter().find(|(_, x, w)| (*x..x + w + gap).contains(&m.x)) {
                self.col = c.min(cols - 1);
            }
            if m.y >= rect.y + 2 {
                let r = self.top.get() + (m.y - rect.y - 2);
                if r >= rows {
                    return Outcome::Ignored;
                }
                self.row = r;
                if m.left_clicks() >= 2 {
                    return Outcome::Submit;
                }
            }
        }
        if (self.row, self.col) != (was_row, was_col) {
            Outcome::Moved
        } else {
            Outcome::Ignored
        }
    }
}

/// `text` in `w` cells at (`x`, `y`), cut with `…`, lined up.
fn put_cell(canvas: &mut Canvas, x: usize, y: usize, w: usize, text: &str, align: Align, style: Style) {
    // A cell is one line: what's after a newline is left out.
    let line = text.split(['\n', '\r']).next().unwrap_or("");
    let shown = fit(line, w);
    let x = match align {
        Align::Left => x,
        Align::Right => x + w - width(&shown),
    };
    canvas.put(x, y, &shown, style);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<Vec<&'static str>> {
        vec![vec!["ls", "List files", "12"], vec!["grep", "Search text in files", "7"], vec!["日本語", "wide", "100"]]
    }

    fn cols() -> Vec<Column> {
        vec![Column::fit("Name", 10), Column::new("What", Width::Flex(1)), Column::new("N", Width::Fixed(3)).right()]
    }

    #[test]
    fn a_table_draws_its_header_rows_and_focus() {
        let s = TableState::new();
        let mut c = Canvas::new();
        s.render(&cols(), rows().as_slice(), Rect::new(0, 0, 30, 6), &mut c);
        assert_eq!(c.text(), ["Name    What                 N", "", "ls      List files          12", "grep    Search text in f…    7", "日本語  wide               100",].join("\n"));
        assert_eq!(c.rules(), [(1, 0..30)]);
        assert_eq!(c.focused(), Some(&(2, 0..30)));
        assert_eq!(s.columns_shown(), [(0, 0..6), (1, 8..25), (2, 27..30)]);
        assert_eq!(c.style_at(0, 2), Style::new(Role::Accent).bg(Fill::Focus), "the focused cell");
        assert_eq!(c.style_at(0, 0), Style::new(Role::Accent), "its column's title");
        assert_eq!(c.style_at(8, 0), Style::new(Role::Muted));
    }

    #[test]
    fn keys_move_the_focused_cell() {
        let mut s = TableState::new();
        assert_eq!(s.handle_key(Key::Char('k'), 3, 3), Outcome::Ignored, "at the top");
        assert_eq!(s.handle_key(Key::Char('j'), 3, 3), Outcome::Moved);
        assert_eq!(s.handle_key(Key::Char('l'), 3, 3), Outcome::Moved);
        assert_eq!((s.row, s.col), (1, 1));
        s.handle_key(Key::Char('$'), 3, 3);
        assert_eq!(s.col, 2);
        assert_eq!(s.handle_key(Key::Right, 3, 3), Outcome::Ignored, "at the last column");
        s.handle_key(Key::Char('G'), 3, 3);
        assert_eq!(s.row, 2);
        s.handle_key(Key::Home, 3, 3);
        s.handle_key(Key::Char('0'), 3, 3);
        assert_eq!((s.row, s.col), (0, 0));
        assert_eq!(s.handle_key(Key::Enter, 3, 3), Outcome::Submit);
        assert_eq!(s.handle_key(Key::Enter, 0, 3), Outcome::Ignored, "nothing to open");
        assert_eq!(s.handle_key(Key::Char('x'), 3, 3), Outcome::Ignored);
    }

    /// A million rows, made up as they're asked for, counting how many
    /// were.
    struct Million {
        asked: Cell<usize>,
    }

    impl TableSource for Million {
        fn len(&self) -> usize {
            1_000_000
        }

        fn row(&self, index: usize) -> Vec<Cow<'_, str>> {
            self.asked.set(self.asked.get() + 1);
            vec![Cow::Owned(index.to_string()), Cow::Owned(format!("row {index}"))]
        }

        fn style(&self, _row: usize, col: usize) -> Style {
            if col == 0 {
                Style::new(Role::Muted)
            } else {
                Style::default()
            }
        }
    }

    #[test]
    fn only_the_rows_shown_are_asked_for() {
        let source = Million { asked: Cell::new(0) };
        let columns = [Column::fit("#", 8).right(), Column::new("Text", Width::Flex(1))];
        let mut s = TableState::new();
        let mut c = Canvas::new();
        s.render(&columns, &source, Rect::new(0, 0, 40, 12), &mut c);
        assert_eq!(source.asked.get(), 10);
        assert_eq!(s.visible_rows(), 0..10);
        s.handle_key(Key::Char('J'), source.len(), 2);
        assert_eq!(s.row, 9, "a screen less a row");
        s.handle_key(Key::Char('G'), source.len(), 2);
        let mut c = Canvas::new();
        s.render(&columns, &source, Rect::new(0, 0, 40, 12), &mut c);
        assert_eq!(source.asked.get(), 20);
        assert_eq!(s.visible_rows(), 999_990..1_000_000);
        assert_eq!(c.line(11), "999999  row 999999                     ┃");
        assert_eq!(c.focused(), Some(&(11, 0..39)));
        assert_eq!(c.style_at(2, 10), Style::new(Role::Muted), "the source's style");
    }

    #[test]
    fn columns_scroll_sideways_to_the_focused_one() {
        let columns: Vec<Column> = (0..6).map(|i| Column::new(format!("col{i}"), Width::Fixed(6))).collect();
        let data = vec![(0..6).map(|i| format!("v{i}")).collect::<Vec<_>>()];
        let mut s = TableState::new();
        s.col = 4;
        let mut c = Canvas::new();
        s.render(&columns, data.as_slice(), Rect::new(0, 0, 20, 3), &mut c);
        assert_eq!(s.first_column(), 3);
        assert_eq!(c.line(0), "col3    col4    col5", "the last cut to what's left");
        s.col = 0;
        let mut c = Canvas::new();
        s.render(&columns, data.as_slice(), Rect::new(0, 0, 20, 3), &mut c);
        assert_eq!(s.first_column(), 0);
        assert_eq!(c.line(2), "v0      v1      v2");
    }

    #[test]
    fn fitted_columns_grow_as_wider_cells_come_into_view() {
        let data: Vec<Vec<String>> = (0..20).map(|i| vec![if i == 15 { "a much longer one".into() } else { "x".into() }, "y".into()]).collect();
        let columns = [Column::fit("A", 30), Column::new("B", Width::Fixed(1))];
        let mut s = TableState::new();
        let first_row = |s: &TableState| {
            let mut c = Canvas::new();
            s.render(&columns, data.as_slice(), Rect::new(0, 0, 40, 7), &mut c);
            c.line(2).trim_end_matches(['│', '┃']).trim_end().to_string()
        };
        assert_eq!(first_row(&s), "x  y");
        s.row = 19;
        first_row(&s);
        s.row = 0;
        assert_eq!(first_row(&s), format!("x{}y", " ".repeat(18)), "it doesn't shrink back");
        s.refit();
        assert_eq!(first_row(&s), "x  y");
    }

    #[test]
    fn a_selection_runs_from_where_it_started_to_the_focused_cell() {
        let mut s = TableState::new();
        assert_eq!(s.selection(3, 3), None);
        s.row = 1;
        s.col = 2;
        s.select(Selecting::Cells);
        s.handle_key(Key::Char('k'), 3, 3);
        s.handle_key(Key::Char('h'), 3, 3);
        assert_eq!(s.selection(3, 3), Some((0..2, 1..3)));
        let mut c = Canvas::new();
        s.render(&cols(), rows().as_slice(), Rect::new(0, 0, 30, 6), &mut c);
        let sel = Style::new(Role::Accent).bg(Fill::Selection);
        assert_eq!(c.style_at(28, 2), sel, "selected");
        assert_eq!(c.style_at(8, 2), Style::new(Role::Accent).bg(Fill::Focus), "the focused cell drawn as focused");
        assert_eq!(c.style_at(0, 3), Style::default(), "not selected");
        s.select(Selecting::Rows);
        assert_eq!(s.selecting(), Some(Selecting::Rows));
        assert_eq!(s.selection(3, 3), Some((0..2, 0..3)), "the same rows, whole");
        assert_eq!(s.selection(1, 3), Some((0..1, 0..3)), "rows gone since are left out");
        s.select(Selecting::Rows);
        assert_eq!(s.selection(3, 3), None, "again ends it");
        s.select(Selecting::Cells);
        s.clear_selection();
        assert_eq!(s.selecting(), None);
    }

    #[test]
    fn no_rows() {
        let s = TableState::new();
        let mut c = Canvas::new();
        let data: Vec<Vec<String>> = Vec::new();
        s.render(&cols(), data.as_slice(), Rect::new(0, 0, 30, 4), &mut c);
        assert_eq!(c.line(2), "No rows");
        assert_eq!(c.focused(), None);
    }

    #[test]
    fn clicks_focus_a_cell() {
        let mut s = TableState::new();
        let mut c = Canvas::new();
        s.render(&cols(), rows().as_slice(), Rect::new(0, 0, 30, 6), &mut c);
        // "grep    Search text in f…    7" on line 3.
        assert_eq!(s.click(10, 3, 3, 3), Outcome::Moved);
        assert_eq!((s.row, s.col), (1, 1));
        assert_eq!(s.click(29, 0, 3, 3), Outcome::Moved, "the header: its column");
        assert_eq!((s.row, s.col), (1, 2));
        assert_eq!(s.click(2, 5, 3, 3), Outcome::Ignored, "below the rows");
        let double = Mouse { action: crate::MouseAction::Press { button: crate::Button::Left, clicks: 2 }, ..Mouse::click(1, 4) };
        assert_eq!(s.mouse(&double, 3, 3), Outcome::Submit);
        assert_eq!((s.row, s.col), (2, 0));
        let wheel = Mouse { action: crate::MouseAction::Wheel(1), ..Mouse::click(1, 4) };
        assert_eq!(s.mouse(&wheel, 3, 3), Outcome::Moved);
        assert_eq!(s.row, 0);
    }
}
