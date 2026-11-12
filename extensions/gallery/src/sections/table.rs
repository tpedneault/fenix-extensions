//! The table: twenty days of weather at twenty stations.

// gallery:begin
use std::borrow::Cow;
use std::cell::Cell;

use fenix_ui_kit::{Canvas, Column, Key, KeyPress, Mouse, Rect, Role, Selecting, Style, TableSource, TableState, Width};

use crate::data::{observations, Observation};
use crate::draw;
use crate::sections::Reply;

pub struct State {
    pub grid: TableState,
    /// The rows' cells, as text, a column each.
    rows: Vec<Vec<String>>,
    /// Which columns hold numbers: sorted as numbers, lined up right.
    numeric: Vec<bool>,
    /// The rows in the order shown: indexes into `rows`.
    order: Vec<usize>,
    /// The column sorted on, and whether it's descending.
    pub sort: Option<(usize, bool)>,
    /// Where the table was drawn, for a click on its header.
    area: Cell<Rect>,
}

const TITLES: [&str; 7] = ["Station", "Date", "High °C", "Low °C", "Rain mm", "Wind km/h", "Conditions"];

fn cells(o: &Observation) -> Vec<String> {
    vec![o.station.to_string(), o.date.clone(), format!("{:.1}", o.high), format!("{:.1}", o.low), format!("{:.1}", o.rain), o.wind.to_string(), o.conditions.to_string()]
}

/// The rows as the widget asks for them: only those it shows.
struct Sorted<'a> {
    state: &'a State,
}

impl TableSource for Sorted<'_> {
    fn len(&self) -> usize {
        self.state.order.len()
    }

    fn row(&self, index: usize) -> Vec<Cow<'_, str>> {
        self.state.rows[self.state.order[index]].iter().map(|s| Cow::Borrowed(s.as_str())).collect()
    }

    /// Frost in the accent, heat as a warning.
    fn style(&self, row: usize, col: usize) -> Style {
        let value = |c: usize| self.state.rows[self.state.order[row]][c].parse::<f64>().unwrap_or(0.0);
        match col {
            2 if value(2) >= 30.0 => Style::new(Role::Warn),
            3 if value(3) < 0.0 => Style::new(Role::Accent),
            6 => Style::new(Role::Muted),
            _ => Style::default(),
        }
    }
}

impl State {
    pub fn new() -> State {
        let rows: Vec<Vec<String>> = observations().iter().map(cells).collect();
        let numeric = (0..TITLES.len()).map(|c| rows.iter().all(|r| r[c].parse::<f64>().is_ok())).collect();
        let order = (0..rows.len()).collect();
        State { grid: TableState::new(), rows, numeric, order, sort: None, area: Cell::new(Rect::default()) }
    }

    fn columns(&self) -> Vec<Column> {
        TITLES
            .iter()
            .enumerate()
            .map(|(i, title)| {
                let mark = match self.sort {
                    Some((c, desc)) if c == i => if desc { " ↓" } else { " ↑" },
                    _ => "",
                };
                let width = match i {
                    1 => Width::Fixed(10),
                    6 => Width::Fit { min: 10, max: 64 },
                    _ => Width::Fit { min: 4, max: 18 },
                };
                let col = Column::new(format!("{title}{mark}"), width);
                if self.numeric[i] { col.right() } else { col }
            })
            .collect()
    }

    /// `s` on a column: up, then down, then the file's order.
    pub fn sort_by(&mut self, col: usize) {
        self.sort = match self.sort {
            Some((c, false)) if c == col => Some((col, true)),
            Some((c, true)) if c == col => None,
            _ => Some((col, false)),
        };
        // The focused row stays focused, wherever it moves to.
        let focused = self.order.get(self.grid.row).copied();
        let rows = &self.rows;
        self.order = (0..rows.len()).collect();
        if let Some((c, desc)) = self.sort {
            let number = |i: usize| rows[i][c].parse::<f64>().unwrap_or(f64::MIN);
            if self.numeric[c] {
                self.order.sort_by(|&a, &b| number(a).total_cmp(&number(b)));
            } else {
                self.order.sort_by(|&a, &b| rows[a][c].cmp(&rows[b][c]));
            }
            if desc {
                self.order.reverse();
            }
        }
        if let Some(r) = focused {
            self.grid.row = self.order.iter().position(|&i| i == r).unwrap_or(0);
        }
        self.grid.refit();
    }

    /// The row shown at `index`, as the table has it.
    pub fn shown(&self, index: usize) -> &[String] {
        &self.rows[self.order[index]]
    }

    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        if self.grid.selecting().is_some() {
            return vec![("hjkl", "grow it"), ("y", "copy it"), ("v V", "stop selecting")];
        }
        vec![("hjkl", "move"), ("J K", "a screen"), ("0 $", "first, last column"), ("s", "sort"), ("v V", "select"), ("y", "copy"), ("Enter", "the row")]
    }

    pub fn render(&self, rect: Rect, c: &mut Canvas) {
        let (_, grid) = rect.split_top(2);
        self.area.set(grid);
        self.grid.render(&self.columns(), &Sorted { state: self }, grid, c);

        // Over it, where you are: drawn after it, which knows the columns
        // it showed.
        let n = self.order.len();
        let mut about = vec![format!("{n} observations")];
        if let Some((col, desc)) = self.sort {
            about.push(format!("sorted by {} {}", TITLES[col], if desc { "↓" } else { "↑" }));
        }
        about.push(format!("row {} of {n}", self.grid.row.min(n.saturating_sub(1)) + 1));
        let shown = self.grid.columns_shown();
        if let (Some(first), Some(last)) = (shown.first(), shown.last()) {
            if shown.len() < TITLES.len() {
                about.push(format!("columns {} to {} of {}", first.0 + 1, last.0 + 1, TITLES.len()));
            }
        }
        if let Some((rows, cols)) = self.grid.selection(n, TITLES.len()) {
            about.push(format!("{} by {} selected", rows.len(), cols.len()));
        }
        draw::text(c, rect, 0, 0, &about.join(" · "), Role::Muted);
    }

    /// What's selected, or the focused cell, as tab-separated lines.
    fn copied(&self) -> (String, String) {
        let (rows, cols) = self.grid.selection(self.order.len(), TITLES.len()).unwrap_or((self.grid.row..self.grid.row + 1, self.grid.col..self.grid.col + 1));
        let text: Vec<String> = rows.clone().map(|r| self.shown(r)[cols.clone()].join("\t")).collect();
        (text.join("\n"), format!("Copied {} rows by {} columns, tab-separated", rows.len(), cols.len()))
    }

    pub fn key(&mut self, press: KeyPress) -> Reply {
        let (n, cols) = (self.order.len(), TITLES.len());
        match press.key {
            Key::Char('s') => self.sort_by(self.grid.col),
            Key::Char('v') => self.grid.select(Selecting::Cells),
            Key::Char('V') => self.grid.select(Selecting::Rows),
            Key::Char('y') => {
                let (text, say) = self.copied();
                self.grid.clear_selection();
                return Reply::Copy { text, say };
            }
            key => {
                return match self.grid.handle_key(key, n, cols) {
                    fenix_ui_kit::Outcome::Submit => {
                        let r = self.shown(self.grid.row);
                        Reply::Say(format!("{} on {}: {} to {} °C, {} mm, {} km/h -- {}", r[0], r[1], r[3], r[2], r[4], r[5], r[6]))
                    }
                    outcome => Reply::from(outcome),
                }
            }
        }
        Reply::Done
    }

    /// A click focuses a cell; on the header of the focused column, it
    /// sorts; a double click is Enter; the wheel moves three rows.
    pub fn mouse(&mut self, m: &Mouse) -> Reply {
        let area = self.area.get();
        let was = self.grid.col;
        let outcome = self.grid.mouse(m, self.order.len(), TITLES.len());
        if m.left_clicks() == 1 && m.y == area.y && area.contains(m.x, m.y) && self.grid.col == was {
            self.sort_by(was);
            return Reply::Done;
        }
        if outcome == fenix_ui_kit::Outcome::Submit {
            return self.key(KeyPress::from(Key::Enter));
        }
        Reply::from(outcome)
    }
}
// gallery:end

impl Default for State {
    fn default() -> State {
        State::new()
    }
}
