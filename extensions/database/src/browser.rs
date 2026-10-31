//! The browser's page: the saved connections in a sidebar, each one's
//! schema under it once connected, and beside them what the last run
//! gave -- a tab a statement, rows in a table, a message or an error.

use fenix_ui_kit::{Canvas, Key, Mouse, Outcome, Popup, Rect, Role, Selecting, Split, Style, Tab, TabsState, TreeNode, TreeState};

use crate::result::Outcome as Came;
use crate::schema::id;
use crate::view::{draw_detail, draw_strip, strip_height, ResultView};

/// What a key on the page asks of the extension.
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    Connect(String),
    Disconnect(String),
    Refresh(String),
    OpenTable {
        conn: String,
        schema: String,
        table: String,
    },
    /// A new `.sql` buffer on a connection.
    NewQuery(String),
    /// The `.sql` buffer you were in runs on a connection from now on.
    Use(String),
    Copy(String),
    /// Text in a buffer of its own: a title, its language.
    Scratch {
        title: String,
        text: String,
        language: String,
    },
    /// Text written to a file you name, `.csv` or `.json`.
    ExportFile {
        text: String,
        extension: String,
    },
    History,
    Say(String),
}

/// A tab of the last run: one statement's outcome.
#[derive(Debug, Clone)]
pub enum ResultTab {
    Rows { view: Box<ResultView>, statement: String },
    Message { text: String, error: bool, statement: String },
}

/// The last run, said above its tabs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ran {
    pub conn: String,
    pub millis: u64,
    pub statements: usize,
}

#[derive(Debug)]
pub struct Browser {
    pub tree: TreeState,
    /// The sidebar has the keyboard; else the results.
    pub side: bool,
    pub results: Vec<ResultTab>,
    pub tabs: TabsState,
    pub ran: Option<Ran>,
    /// A line said at the foot: what's happened.
    pub note: Option<(String, Role)>,
    /// Keys and their help shown in a popup (`?`).
    pub help: bool,
}

impl Default for Browser {
    fn default() -> Browser {
        let mut tree = TreeState::new();
        tree.empty = "No connections: add them in database.connections, or open a .sqlite file".into();
        Browser { tree, side: true, results: Vec::new(), tabs: TabsState::new(), ran: None, note: None, help: false }
    }
}

impl ResultTab {
    fn title(&self, i: usize) -> Tab {
        match self {
            ResultTab::Rows { view, .. } => Tab::new(if i == 0 { "Rows".to_string() } else { format!("Rows {}", i + 1) }).badge(crate::result::thousands(view.len() as u64)),
            ResultTab::Message { error: true, .. } => Tab::new("Error"),
            ResultTab::Message { .. } => Tab::new("Done"),
        }
    }
}

impl Browser {
    /// What a run gave: a tab each statement's.
    pub fn show(&mut self, ran: Ran, statements: &[String], outcomes: Vec<Came>) {
        self.results = outcomes
            .into_iter()
            .enumerate()
            .map(|(i, o)| {
                let statement = statements.get(i).cloned().unwrap_or_default();
                match o {
                    Came::Rows(r) => {
                        let mut view = ResultView::new(r);
                        view.table.focused = false;
                        ResultTab::Rows { view: Box::new(view), statement }
                    }
                    Came::Done { .. } => ResultTab::Message { text: o.summary(), error: false, statement },
                    Came::Error(e) => ResultTab::Message { text: e, error: true, statement },
                }
            })
            .collect();
        // The first with rows, else the first error.
        self.tabs.active =
            self.results.iter().position(|t| matches!(t, ResultTab::Rows { .. })).or_else(|| self.results.iter().position(|t| matches!(t, ResultTab::Message { error: true, .. }))).unwrap_or(0);
        self.ran = Some(ran);
        if self.results.iter().any(|t| matches!(t, ResultTab::Rows { .. })) {
            self.focus_results(true);
        }
    }

    fn focus_results(&mut self, on: bool) {
        self.side = !on;
        self.tree.focused = !on;
        for t in &mut self.results {
            if let ResultTab::Rows { view, .. } = t {
                view.table.focused = on;
            }
        }
    }

    fn active_view(&mut self) -> Option<&mut ResultView> {
        match self.results.get_mut(self.tabs.active) {
            Some(ResultTab::Rows { view, .. }) => Some(view),
            _ => None,
        }
    }

    fn keys(&self) -> Vec<(&'static str, &'static str)> {
        if self.side {
            vec![
                ("Enter", "connect, open"),
                ("c d", "connect, disconnect"),
                ("r", "read the schema again"),
                ("n", "new query"),
                ("u", "use for the .sql buffer"),
                ("H", "history"),
                ("Tab", "results"),
                ("?", "help"),
            ]
        } else {
            vec![
                ("hjkl", "move"),
                ("s", "sort"),
                ("v V", "select"),
                ("y Y", "copy cell, rows"),
                ("x X", "CSV, JSON to a buffer"),
                ("w", "export to a file"),
                ("1-9 [ ]", "results"),
                ("Tab", "connections"),
            ]
        }
    }

    /// A key, on a sidebar of `roots`.
    pub fn key(&mut self, key: Key, roots: &[TreeNode]) -> Option<Intent> {
        self.note = None;
        if self.help {
            self.help = false;
            return None;
        }
        match key {
            Key::Tab | Key::BackTab => {
                let to_results = self.side && !self.results.is_empty();
                self.focus_results(to_results);
                return None;
            }
            Key::Char('?') => {
                self.help = true;
                return None;
            }
            Key::Char('H') => return Some(Intent::History),
            _ => {}
        }
        if self.side {
            return self.side_key(key, roots);
        }
        let n = self.results.len();
        match key {
            Key::Char('[') if n > 0 => {
                self.tabs.active = (self.tabs.active + n - 1) % n;
                return None;
            }
            Key::Char(']') if n > 0 => {
                self.tabs.active = (self.tabs.active + 1) % n;
                return None;
            }
            Key::Char('1'..='9') => {
                self.tabs.handle_key(key, n);
                return None;
            }
            _ => {}
        }
        let conn = self.ran.as_ref().map(|r| r.conn.clone()).unwrap_or_default();
        let view = self.active_view()?;
        match key {
            Key::Char('s') => view.cycle_sort(),
            Key::Char('v') => view.select(Selecting::Cells),
            Key::Char('V') => view.select(Selecting::Rows),
            Key::Escape => view.table.clear_selection(),
            Key::Char('y') => return view.copy_cells().map(Intent::Copy),
            Key::Char('Y') => return view.copy_rows().map(Intent::Copy),
            Key::Char('x') => return Some(Intent::Scratch { title: format!("{conn}.csv"), text: view.rows.csv(Some(&view.all_rows())), language: "csv".into() }),
            Key::Char('X') => return Some(Intent::Scratch { title: format!("{conn}.json"), text: view.rows.json(Some(&view.all_rows())), language: "json".into() }),
            Key::Char('w') => return Some(Intent::ExportFile { text: view.rows.csv(Some(&view.all_rows())), extension: "csv".into() }),
            Key::Char('W') => return Some(Intent::ExportFile { text: view.rows.json(Some(&view.all_rows())), extension: "json".into() }),
            k => {
                view.key(k);
            }
        }
        None
    }

    fn side_key(&mut self, key: Key, roots: &[TreeNode]) -> Option<Intent> {
        let node = self.tree.selected_node(roots).cloned();
        let conn = node.as_ref().and_then(|n| id::conn_of(&n.id)).map(str::to_string);
        match key {
            Key::Char('c') => return conn.map(Intent::Connect),
            Key::Char('d') => return conn.map(Intent::Disconnect),
            Key::Char('r') => return conn.map(Intent::Refresh),
            Key::Char('n') => return conn.map(Intent::NewQuery),
            Key::Char('u') => return conn.map(Intent::Use),
            Key::Char('y') => return node.map(|n| Intent::Copy(n.label)),
            _ => {}
        }
        match self.tree.handle_key(key, roots) {
            Outcome::Submit => {
                let node = node?;
                if let Some((conn, schema, table)) = id::parse_table(&node.id) {
                    return Some(Intent::OpenTable { conn, schema, table });
                }
                if node.lazy {
                    self.tree.expand(&node.id);
                    return conn.map(Intent::Connect);
                }
                if self.tree.is_expanded(&node.id) {
                    self.tree.collapse(&node.id);
                } else if node.expandable() {
                    self.tree.expand(&node.id);
                }
            }
            // A connection opened before it's connected: connect.
            Outcome::Expand if node.as_ref().is_some_and(|n| n.lazy) => return conn.map(Intent::Connect),
            _ => {}
        }
        None
    }

    /// The mouse: a click in the sidebar or the results gives it the
    /// keyboard; a double click opens what's there.
    pub fn mouse(&mut self, m: &Mouse, roots: &[TreeNode], split_x: usize) -> Option<Intent> {
        let in_side = m.x < split_x;
        if m.left_clicks() > 0 && in_side != self.side && (in_side || !self.results.is_empty()) {
            self.focus_results(!in_side);
        }
        if in_side {
            match self.tree.mouse(m, roots) {
                Outcome::Submit => return self.side_key(Key::Enter, roots),
                Outcome::Expand => {
                    let node = self.tree.selected_node(roots)?;
                    return node.lazy.then(|| id::conn_of(&node.id).map(|c| Intent::Connect(c.to_string()))).flatten();
                }
                _ => {}
            }
            return None;
        }
        if self.tabs.mouse(m).handled() {
            return None;
        }
        let view = self.active_view()?;
        let (n, c) = (view.len(), view.cols());
        view.table.mouse(m, n, c);
        None
    }

    /// The sidebar's width for a page `w` wide.
    pub fn side_width(w: usize) -> usize {
        (w / 3).clamp(24, 40).min(w.saturating_sub(20))
    }

    /// The page, `w` by `h` cells.
    pub fn render(&self, roots: &[TreeNode], w: usize, h: usize) -> Canvas {
        let w = w.max(40);
        let h = if h == 0 { 30 } else { h.max(8) };
        let mut c = Canvas::new();
        c.set_more_keys(true);
        c.set_mouse(true);
        let keys = self.keys();
        let strip = strip_height(&keys, w);
        let body_h = h.saturating_sub(strip + 2);
        let (side, main) = Split::sidebar(Self::side_width(w)).render(Rect::new(0, 0, w, body_h), &mut c);
        c.put(side.x, 0, "Connections", Role::Title);
        let (_, tree_rect) = side.split_top(2);
        self.tree.render(roots, tree_rect, &mut c);

        let main = main.inset_x(1);
        let mut y = main.y;
        match &self.ran {
            None => {
                c.put(main.x, y, "Results", Role::Title);
                c.put(main.x, y + 2, "Run a statement from a .sql buffer (SPC m r), or open a table from the sidebar.", Role::Muted);
            }
            Some(ran) => {
                let x = c.put(main.x, y, &ran.conn, Role::Title);
                let about = format!("{} statement{} · {} ms", ran.statements, if ran.statements == 1 { "" } else { "s" }, ran.millis);
                c.put_fit(x + 2, y, main.right().saturating_sub(x + 2), &about, Role::Muted);
                y += 1;
                if !self.results.is_empty() {
                    let tabs: Vec<Tab> = self.results.iter().enumerate().map(|(i, t)| t.title(i)).collect();
                    self.tabs.render(&tabs, Rect::new(main.x, y, main.w, 2), &mut c);
                    y += 2;
                }
                let active = self.tabs.active.min(self.results.len().saturating_sub(1));
                let room = main.bottom().saturating_sub(y);
                match self.results.get(active) {
                    Some(ResultTab::Rows { view, statement }) => {
                        let detail = 3;
                        let table_h = room.saturating_sub(detail + 1);
                        view.render(Rect::new(main.x, y, main.w, table_h), &mut c);
                        let mut fy = y + table_h;
                        c.rule(fy, main.x..main.right());
                        fy += 1;
                        if let Some((r, col)) = view.focused() {
                            let name = &view.rows.columns[col];
                            let mut x = c.put(main.x, fy, &name.name, Role::Accent);
                            x = c.put(x + 1, fy, &name.type_name, Role::Muted);
                            let shown = view.table.row.min(view.len().saturating_sub(1)) + 1;
                            let mut about = format!("  row {} of {}", crate::result::thousands(shown as u64), crate::result::thousands(view.len() as u64));
                            if view.rows.truncated {
                                about.push_str(" (more not kept)");
                            }
                            c.put(x, fy, &about, Role::Muted);
                            let value = view.rows.rows[r].get(col).cloned().flatten();
                            let mut dc = Canvas::new();
                            draw_detail(&mut dc, value.as_deref(), 0, main.w, detail - 1);
                            for line in 0..dc.height() {
                                c.put(main.x, fy + 1 + line, &dc.line(line), if value.is_none() { Role::Muted } else { Role::Text });
                            }
                        } else {
                            c.put_fit(main.x, fy, main.w, &crate::history::one_line(statement, main.w), Role::Muted);
                        }
                    }
                    Some(ResultTab::Message { text, error, statement }) => {
                        let mut ly = y;
                        for line in fenix_ui_kit::text::wrap(text, main.w) {
                            if ly >= main.bottom() {
                                break;
                            }
                            c.put(main.x, ly, &line, if *error { Role::Bad } else { Role::Good });
                            ly += 1;
                        }
                        ly += 1;
                        for line in fenix_ui_kit::text::wrap(statement, main.w).into_iter().take(main.bottom().saturating_sub(ly)) {
                            c.put(main.x, ly, &line, Style::new(Role::Syntax("comment".into())));
                            ly += 1;
                        }
                    }
                    None => {
                        c.put(main.x, y, "Nothing came back", Role::Muted);
                    }
                }
            }
        }
        let foot = body_h;
        c.rule(foot, 0..w);
        if let Some((note, role)) = &self.note {
            c.put_fit(0, foot + 1, w, note, role.clone());
        }
        draw_strip(&mut c, &keys, foot + 2, w);
        if self.help {
            let rows: Vec<Vec<(String, Style)>> = [
                ("The sidebar", ""),
                ("Enter", "connect; open a branch; a table's rows"),
                ("c d r", "connect, disconnect, read the schema again"),
                ("n u", "a new query buffer; the .sql buffer you were in runs here"),
                ("The results", ""),
                ("s", "sort by the column: up, down, as they came"),
                ("y Y", "copy the cell (or the selection), the rows as CSV"),
                ("x X", "every row as CSV, as JSON, in a buffer"),
                ("w W", "every row to a file, CSV or JSON"),
                ("In a .sql buffer", ""),
                ("SPC m r", "run the statement under the cursor, or the selection"),
                ("SPC m R", "run them all"),
                ("SPC m c", "pick its connection"),
            ]
            .iter()
            .map(
                |(k, what)| {
                    if what.is_empty() {
                        vec![(k.to_string(), Style::new(Role::Title))]
                    } else {
                        vec![(format!("{k:<10}"), Style::new(Role::Accent)), (what.to_string(), Style::new(Role::Text))]
                    }
                },
            )
            .collect();
            c.popup(Popup { line: 1, col: 2, rows });
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::{Column, Kind, Rows};

    fn roots() -> Vec<TreeNode> {
        vec![
            TreeNode::branch(id::connection("dev"), "dev", vec![TreeNode::branch("x", "Tables (1)", vec![TreeNode::leaf(id::table("dev", "public", "users"), "users")])]),
            TreeNode::lazy(id::connection("prod"), "prod").detail("PostgreSQL"),
        ]
    }

    fn rows() -> Came {
        Came::Rows(Rows {
            columns: vec![Column::new("id", "int4", Kind::Number), Column::new("name", "text", Kind::Text)],
            rows: vec![vec![Some("1".into()), Some("ada".into())], vec![Some("2".into()), None]],
            truncated: false,
        })
    }

    #[test]
    fn the_sidebar_connects_and_opens_tables() {
        let mut b = Browser::default();
        let r = roots();
        assert_eq!(b.key(Key::Char('j'), &r), None);
        assert_eq!(b.key(Key::Enter, &r), Some(Intent::Connect("prod".into())), "a connection not yet connected");
        b.tree.selected = 0;
        b.key(Key::Char('l'), &r);
        b.key(Key::Char('j'), &r);
        b.key(Key::Char('l'), &r);
        b.key(Key::Char('j'), &r);
        assert_eq!(b.key(Key::Enter, &r), Some(Intent::OpenTable { conn: "dev".into(), schema: "public".into(), table: "users".into() }));
        assert_eq!(b.key(Key::Char('n'), &r), Some(Intent::NewQuery("dev".into())));
        assert_eq!(b.key(Key::Char('H'), &r), Some(Intent::History));
    }

    #[test]
    fn a_run_shows_a_tab_a_statement() {
        let mut b = Browser::default();
        let r = roots();
        b.show(
            Ran { conn: "dev".into(), millis: 12, statements: 3 },
            &["select".into(), "update".into(), "bad".into()],
            vec![rows(), Came::Done { tag: "UPDATE 2".into(), changed: Some(2) }, Came::Error("syntax error".into())],
        );
        assert!(!b.side, "rows came: the results have the keyboard");
        let c = b.render(&r, 90, 20);
        let text = c.text();
        assert!(text.contains("dev  3 statements · 12 ms"), "{text}");
        assert!(text.contains("1 Rows 2   2 Done   3 Error"), "{text}");
        assert!(text.contains("ada"));
        assert!(text.contains("id int4  row 1 of 2"), "{text}");
        assert_eq!(b.key(Key::Char('y'), &r), Some(Intent::Copy("1".into())));
        b.key(Key::Char('j'), &r);
        assert_eq!(b.key(Key::Char('Y'), &r), Some(Intent::Copy("id,name\r\n2,\r\n".into())));
        assert!(matches!(b.key(Key::Char('X'), &r), Some(Intent::Scratch { language, .. }) if language == "json"));
        assert!(matches!(b.key(Key::Char('w'), &r), Some(Intent::ExportFile { extension, .. }) if extension == "csv"));
        b.key(Key::Char('3'), &r);
        assert!(b.render(&r, 90, 20).text().contains("syntax error"));
        if std::env::var("SHOW").is_ok() {
            b.key(Key::Char('1'), &r);
            println!("{}", b.render(&r, 90, 20).text());
            b.key(Key::Tab, &r);
            b.tree.selected = 0;
            b.key(Key::Char('l'), &r);
            println!("{}", b.render(&r, 90, 20).text());
            b.key(Key::Tab, &r);
        }
        b.key(Key::Tab, &r);
        assert!(b.side);
    }
}
