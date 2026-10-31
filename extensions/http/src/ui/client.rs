//! The client's page: the collections (the project's `.http` files, by
//! folder, each request with its method) and the history in a sidebar;
//! the request chosen -- its method and URL, then tabs for its params,
//! headers, body, auth and tests, each a part of its `.http` text that a
//! change edits; and its response.

use std::cell::Cell;
use std::collections::HashSet;

use fenix_ui_kit::{Canvas, Fill, Key, Mouse, Outcome, Rect, Role, Size, Split, Style, TabsState, TextField, TreeNode, TreeState};

use super::method_role;
use super::response::{ResponseView, Shown};
use crate::model::Exchange;
use crate::parse::Request;

/// A `.http` file of the project's, and its requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Below the project.
    pub rel: String,
    /// Each request: its method, label and line.
    pub requests: Vec<(String, String, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Side,
    Request,
    Response,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReqTab {
    Params,
    Headers,
    Body,
    Auth,
    Tests,
}

const REQ_TABS: [ReqTab; 5] = [ReqTab::Params, ReqTab::Headers, ReqTab::Body, ReqTab::Auth, ReqTab::Tests];

/// What a key on the page asks of the extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Redraw,
    /// Show request `index` of file `rel`.
    Select(String, usize),
    Send,
    PickEnv,
    Reload,
    /// Open the selected request's file at its line, or this line.
    OpenFile(Option<u32>),
    ShowHistory(usize),
    SaveHistory(usize),
    Method,
    SetUrl(String),
    AddRow(ReqTab),
    EditRow(ReqTab, usize),
    DeleteRow(ReqTab, usize),
    BodyType,
    Auth,
    CopyAs,
    NewRequest,
    /// Run every request below this path: a file, or a folder ("" the
    /// project).
    RunAll(String),
    OpenBody,
    SaveBody,
}

/// What the page draws from: the extension's, borrowed.
pub struct Data<'a> {
    pub files: &'a [FileEntry],
    /// The request selected, as its file has it now.
    pub request: Option<&'a Request>,
    pub env: Option<&'a str>,
    pub history: &'a [Exchange],
    pub shown: &'a Shown,
    /// Environments there are to choose from.
    pub envs: usize,
}

#[derive(Debug)]
pub struct ClientPage {
    pub area: Area,
    pub side: TabsState,
    pub tree: TreeState,
    pub history: TreeState,
    pub filter: TextField,
    pub filtering: bool,
    /// The request shown: its file and index there.
    pub selected: Option<(String, usize)>,
    /// A history entry shown in its place.
    pub viewing: Option<usize>,
    /// A history entry was sent again: its answer is shown when it comes.
    pub resent: bool,
    pub req_tabs: TabsState,
    pub row: usize,
    pub url: TextField,
    pub editing_url: bool,
    pub response: ResponseView,
    pub hide_side: bool,
    /// Branches opened once already, so a new one opens and a closed one
    /// stays closed.
    opened: HashSet<String>,
    /// Where each area was drawn: the sidebar, the request, the response.
    rects: Cell<[Rect; 3]>,
    /// Rows in the request's tab, at the last render.
    rows_len: Cell<usize>,
}

impl Default for ClientPage {
    fn default() -> ClientPage {
        ClientPage {
            area: Area::Side,
            side: TabsState::default(),
            tree: with_empty("No .http files in the project: n makes one"),
            history: with_empty("Nothing sent yet"),
            filter: {
                let mut f = TextField::new().with_placeholder("/ searches the history");
                f.focused = false;
                f
            },
            filtering: false,
            selected: None,
            viewing: None,
            resent: false,
            req_tabs: TabsState::new(),
            row: 0,
            url: {
                let mut f = TextField::new();
                f.focused = false;
                f
            },
            editing_url: false,
            response: ResponseView::new(),
            hide_side: false,
            opened: HashSet::new(),
            rects: Cell::new([Rect::default(); 3]),
            rows_len: Cell::new(0),
        }
    }
}

fn with_empty(empty: &str) -> TreeState {
    let mut t = TreeState::new();
    t.empty = empty.into();
    t
}

/// The collections as a tree: folders, their files, each file's requests.
pub fn tree(files: &[FileEntry]) -> Vec<TreeNode> {
    fn insert(nodes: &mut Vec<TreeNode>, parts: &[&str], prefix: &str, file: &FileEntry) {
        let (first, rest) = (parts[0], &parts[1..]);
        if rest.is_empty() {
            let children = file
                .requests
                .iter()
                .enumerate()
                .map(|(i, (method, label, _))| TreeNode::leaf(format!("r:{}#{i}", file.rel), format!("{method:<6} {label}")).style(method_role(method)))
                .collect();
            nodes.push(TreeNode::branch(format!("f:{}", file.rel), first.to_string(), children).detail(file.requests.len().to_string()));
            return;
        }
        let id = format!("d:{prefix}{first}");
        let at = match nodes.iter().position(|n| n.id == id) {
            Some(i) => i,
            None => {
                nodes.push(TreeNode::branch(id.clone(), format!("{first}/"), Vec::new()).style(Role::Title));
                nodes.len() - 1
            }
        };
        insert(&mut nodes[at].children, rest, &format!("{prefix}{first}/"), file);
    }
    let mut roots = Vec::new();
    for f in files {
        let parts: Vec<&str> = f.rel.split('/').collect();
        insert(&mut roots, &parts, "", f);
    }
    roots
}

fn history_nodes(history: &[Exchange], query: &str) -> Vec<TreeNode> {
    crate::store::search(history, query)
        .into_iter()
        .map(|i| {
            let e = &history[i];
            let (status, role) = match &e.answer {
                Ok(r) => (r.status.to_string(), super::response::status_role(r.status)),
                Err(_) => ("ERR".into(), Role::Bad),
            };
            let when = fenix_extension_api::chrono::DateTime::from_timestamp(e.at, 0).map(|t| t.format("%m-%d %H:%M").to_string()).unwrap_or_default();
            // The path, the host being the same most of the time.
            let shown = match e.sent.url.split_once("://").and_then(|(_, r)| r.find('/').map(|i| &r[i..])) {
                Some(p) => p.to_string(),
                None => e.sent.url.clone(),
            };
            TreeNode::leaf(format!("h:{i}"), format!("{status} {:<6} {shown}", e.sent.method)).style(role).detail(when)
        })
        .collect()
}

/// A request's rows on tab `tab`: what's shown, a cell each.
pub fn rows(r: &Request, tab: ReqTab) -> Vec<(String, String)> {
    match tab {
        ReqTab::Params => crate::url::split(&r.url).1,
        ReqTab::Headers => r.headers.iter().map(|h| (h.name.clone(), h.value.clone())).collect(),
        ReqTab::Body => match &r.body {
            Some(b) => b.text.lines().map(|l| (l.to_string(), String::new())).collect(),
            None => Vec::new(),
        },
        ReqTab::Auth => {
            let mut out = Vec::new();
            if let Some(a) = r.header("authorization") {
                let (scheme, value) = a.split_once(' ').unwrap_or((a, ""));
                let kind = match scheme.to_ascii_lowercase().as_str() {
                    "bearer" if value.contains("$auth.token") => "OAuth 2.0",
                    "bearer" => "Bearer",
                    "basic" => "Basic",
                    "digest" => "Digest (not done: sent as it is)",
                    _ => "Authorization",
                };
                out.push((kind.to_string(), super::mask(value)));
            }
            for h in r.headers.iter().filter(|h| h.name.to_ascii_lowercase().contains("api-key") || h.name.eq_ignore_ascii_case("x-api-token")) {
                out.push((format!("API key ({})", h.name), super::mask(&h.value)));
            }
            out
        }
        ReqTab::Tests => r.tests().map(|d| (d.value.clone(), String::new())).collect(),
    }
}

impl ClientPage {
    pub fn new() -> ClientPage {
        ClientPage::default()
    }

    pub fn req_tab(&self) -> ReqTab {
        REQ_TABS[self.req_tabs.active.min(REQ_TABS.len() - 1)]
    }

    /// The tree's branches opened the first time they're seen.
    pub fn open_new(&mut self, nodes: &[TreeNode]) {
        fn walk(page: &mut ClientPage, nodes: &[TreeNode]) {
            for n in nodes {
                if n.expandable() && page.opened.insert(n.id.clone()) {
                    page.tree.expand(&n.id);
                }
                walk(page, &n.children);
            }
        }
        walk(self, nodes);
    }

    /// Request `index` of `rel` selected in the tree, and shown.
    pub fn select(&mut self, files: &[FileEntry], rel: &str, index: usize) {
        let nodes = tree(files);
        self.tree.select_id(&nodes, &format!("r:{rel}#{index}"));
        self.selected = Some((rel.to_string(), index));
        self.viewing = None;
        self.row = 0;
    }

    pub fn render(&self, d: &Data, width: usize, height: usize) -> Canvas {
        let mut c = Canvas::new();
        c.set_mouse(true);
        c.set_more_keys(true);
        let mut x = c.put(0, 0, "HTTP", Role::Title);
        x = c.put(x, 0, "   env ", Role::Muted);
        x = match d.env {
            Some(e) => c.put(x, 0, e, Style::new(Role::Accent).bg(Fill::Panel)),
            None if d.envs == 0 => c.put(x, 0, "none (http-client.env.json)", Role::Muted),
            None => c.put(x, 0, "none", Role::Muted),
        };
        if d.envs > 0 {
            c.put(x, 0, "  e switch", Role::Muted);
        }
        if let Some((rel, _)) = &self.selected {
            let w = fenix_ui_kit::text::width(rel);
            if w + x + 4 < width {
                c.put(width - w, 0, rel, Role::Muted);
            }
        }
        c.rule(1, 0..width);
        let body = Rect::new(0, 2, width, height.saturating_sub(3));
        let (side, main) = if self.hide_side { (Rect::default(), body) } else { Split::sidebar((width / 3).clamp(24, 40)).render(body, &mut c) };
        if !self.hide_side {
            self.render_side(d, side, &mut c);
        }
        let (req, resp) = {
            let (a, b) = Split::vertical(Size::Percent(42)).min(4).render(main, &mut c);
            (a.inset_x(1), b.inset_x(1))
        };
        self.render_request(d, req, &mut c);
        self.response.render(d.shown, resp, self.area == Area::Response, &mut c);
        self.rects.set([side, req, resp]);
        let keys = match self.area {
            _ if self.editing_url => "Enter set the URL  Esc leave it",
            _ if self.filtering => "type to search  Enter Esc done",
            Area::Side if self.side.active == 1 => "Enter show  s save to a collection  / search  ] collections  Tab next",
            Area::Side => "Enter show  r send  x run all  n new  o open its file  ] history  Tab next  e env",
            Area::Request => "r send  m method  u URL  a add  Enter edit  d delete  b body type  [ ] tabs  y copy as  o open",
            Area::Response => "[ ] tabs  j k scroll  p raw  O open the body  S save it  r send again",
        };
        c.put_fit(0, height.saturating_sub(1), width, keys, Role::Muted);
        c
    }

    fn render_side(&self, d: &Data, rect: Rect, c: &mut Canvas) {
        let (bar, rest) = rect.split_top(2);
        self.side.render(&[super::tab("Collections", d.files.len().to_string()), super::tab("History", d.history.len().to_string())], bar, c);
        if self.side.active == 1 {
            let (field, list) = rest.split_top(1);
            self.filter.render(field, c);
            let nodes = history_nodes(d.history, self.filter.text());
            let mut h = self.history.clone();
            h.focused = self.area == Area::Side && !self.filtering;
            h.render(&nodes, list.split_top(1).1, c);
        } else {
            let nodes = tree(d.files);
            let mut t = self.tree.clone();
            t.focused = self.area == Area::Side;
            t.render(&nodes, rest, c);
        }
    }

    fn render_request(&self, d: &Data, rect: Rect, c: &mut Canvas) {
        if rect.is_empty() {
            return;
        }
        // A history entry: what was sent, as it was.
        if let Some(e) = self.viewing.and_then(|i| d.history.get(i)) {
            let mut x = c.put(rect.x, rect.y, &e.sent.method, Style::new(method_role(&e.sent.method)).bg(Fill::Panel));
            x = c.put(x + 1, rect.y, &fenix_ui_kit::text::fit(&e.sent.url, rect.right().saturating_sub(x + 1)), Role::Text);
            let _ = x;
            c.put(rect.x, rect.y + 1, "From the history, as it was sent · s saves it to a collection", Role::Muted);
            for (i, (k, v)) in e.sent.headers.iter().enumerate().take(rect.h.saturating_sub(3)) {
                let x = c.put(rect.x, rect.y + 3 + i, &format!("{k}: "), Role::Syntax("property".into()));
                c.put_fit(x, rect.y + 3 + i, rect.right().saturating_sub(x), v, Role::Text);
            }
            self.rows_len.set(0);
            return;
        }
        let Some(r) = d.request else {
            c.put_fit(rect.x, rect.y, rect.w, "Choose a request in the sidebar, or n for a new one", Role::Muted);
            self.rows_len.set(0);
            return;
        };
        let x = c.put(rect.x, rect.y, &r.method, Style::new(method_role(&r.method)).bg(Fill::Panel)) + 1;
        if self.editing_url {
            self.url.render(Rect::new(x, rect.y, rect.right().saturating_sub(x), 1), c);
        } else {
            c.put_fit(x, rect.y, rect.right().saturating_sub(x), &r.url, Role::Text);
        }
        let n = |t: ReqTab| rows(r, t).len();
        let tabs = [
            super::tab("Params", n(ReqTab::Params).to_string()),
            super::tab("Headers", n(ReqTab::Headers).to_string()),
            super::tab("Body", r.header("content-type").map(short_type).unwrap_or_default()),
            super::tab("Auth", rows(r, ReqTab::Auth).first().map(|a| a.0.split(' ').next().unwrap_or("").to_string()).unwrap_or_default()),
            super::tab("Tests", n(ReqTab::Tests).to_string()),
        ];
        let (bar, list) = Rect::new(rect.x, rect.y + 1, rect.w, rect.h.saturating_sub(1)).split_top(2);
        self.req_tabs.render(&tabs, bar, c);
        if self.area == Area::Request {
            c.put(bar.right().saturating_sub(1), bar.y, "●", Role::Accent);
        }
        let tab = self.req_tab();
        let rs = rows(r, tab);
        self.rows_len.set(rs.len());
        if rs.is_empty() {
            let empty = match tab {
                ReqTab::Params => "No query parameters: a adds one",
                ReqTab::Headers => "No headers: a adds one",
                ReqTab::Body => "No body: b chooses one",
                ReqTab::Auth => "No auth: Enter chooses Bearer, Basic, an API key or OAuth 2.0",
                ReqTab::Tests => "No tests: a adds one (status == 200, $.id exists)",
            };
            c.put_fit(list.x, list.y, list.w, empty, Role::Muted);
            return;
        }
        let kw = rs.iter().map(|(k, _)| fenix_ui_kit::text::width(k)).max().unwrap_or(0).min(list.w / 2);
        let top = self.row.saturating_sub(list.h.saturating_sub(1));
        for (i, (k, v)) in rs.iter().enumerate().skip(top).take(list.h) {
            let y = list.y + i - top;
            let key_role = match tab {
                ReqTab::Body => Role::Text,
                ReqTab::Tests => Role::Text,
                _ => Role::Syntax("property".into()),
            };
            let mut x = c.put(list.x, y, &fenix_ui_kit::text::fit(k, if v.is_empty() { list.w } else { kw }), key_role);
            if !v.is_empty() {
                x = list.x + kw + 2;
                c.put_fit(x, y, list.right().saturating_sub(x), v, Role::Text);
            }
            if tab == ReqTab::Tests {
                if let Some(t) = d.shown.tests.iter().find(|t| t.test == *k) {
                    let mark = if t.passed { "✓" } else { "✗" };
                    c.put(list.right().saturating_sub(1), y, mark, if t.passed { Role::Good } else { Role::Bad });
                }
            }
            let _ = x;
            if self.area == Area::Request && i == self.row && tab != ReqTab::Body {
                c.focus(y, list.x..list.right());
            }
        }
    }

    pub fn key(&mut self, key: Key, d: &Data) -> Action {
        if self.editing_url {
            return match self.url.handle_key(key) {
                Outcome::Submit => {
                    self.editing_url = false;
                    Action::SetUrl(self.url.text().to_string())
                }
                Outcome::Cancel => {
                    self.editing_url = false;
                    Action::Redraw
                }
                _ => Action::Redraw,
            };
        }
        if self.filtering {
            return match self.filter.handle_key(key) {
                Outcome::Submit | Outcome::Cancel => {
                    self.filtering = false;
                    self.filter.focused = false;
                    Action::Redraw
                }
                Outcome::Edited => {
                    self.history.selected = 0;
                    Action::Redraw
                }
                _ => Action::Redraw,
            };
        }
        match key {
            Key::Tab => {
                self.area = match self.area {
                    Area::Side => Area::Request,
                    Area::Request => Area::Response,
                    Area::Response if self.hide_side => Area::Request,
                    Area::Response => Area::Side,
                };
                return Action::Redraw;
            }
            Key::BackTab => {
                self.area = match self.area {
                    Area::Side => Area::Response,
                    Area::Request if self.hide_side => Area::Response,
                    Area::Request => Area::Side,
                    Area::Response => Area::Request,
                };
                return Action::Redraw;
            }
            Key::Char('e') => return Action::PickEnv,
            Key::Char('R') => return Action::Reload,
            Key::Char('r') => return Action::Send,
            Key::Char('y') => return Action::CopyAs,
            Key::Char('n') => return Action::NewRequest,
            Key::Char('o') if self.area != Area::Response => return Action::OpenFile(None),
            Key::Char('\\') => {
                self.hide_side = !self.hide_side;
                if self.hide_side && self.area == Area::Side {
                    self.area = Area::Request;
                }
                return Action::Redraw;
            }
            _ => {}
        }
        match self.area {
            Area::Side => self.side_key(key, d),
            Area::Request => self.request_key(key, d),
            Area::Response => match key {
                Key::Char('O') => Action::OpenBody,
                Key::Char('S') => Action::SaveBody,
                k => {
                    self.response.key(k);
                    Action::Redraw
                }
            },
        }
    }

    fn side_key(&mut self, key: Key, d: &Data) -> Action {
        if matches!(key, Key::Char('[') | Key::Char(']')) {
            self.side.active = 1 - self.side.active.min(1);
            return Action::Redraw;
        }
        if self.side.active == 1 {
            let nodes = history_nodes(d.history, self.filter.text());
            let at = self.history.selected_node(&nodes).and_then(|n| n.id.strip_prefix("h:")).and_then(|i| i.parse::<usize>().ok());
            return match key {
                Key::Char('/') => {
                    self.filtering = true;
                    self.filter.focused = true;
                    Action::Redraw
                }
                Key::Enter => match at {
                    Some(i) => {
                        self.viewing = Some(i);
                        Action::ShowHistory(i)
                    }
                    None => Action::Nothing,
                },
                Key::Char('s') => at.map(Action::SaveHistory).unwrap_or(Action::Nothing),
                k => {
                    self.history.handle_key(k, &nodes);
                    Action::Redraw
                }
            };
        }
        let nodes = tree(d.files);
        let id = self.tree.selected_node(&nodes).map(|n| n.id.clone()).unwrap_or_default();
        match key {
            Key::Char('x') => {
                let path = id.strip_prefix("f:").or_else(|| id.strip_prefix("d:")).map(String::from).or_else(|| id.strip_prefix("r:").and_then(|r| r.split_once('#')).map(|(f, _)| f.to_string())).unwrap_or_default();
                Action::RunAll(path)
            }
            Key::Enter => {
                if let Some((rel, i)) = id.strip_prefix("r:").and_then(|r| r.rsplit_once('#')) {
                    let i: usize = i.parse().unwrap_or(0);
                    self.selected = Some((rel.to_string(), i));
                    self.viewing = None;
                    self.row = 0;
                    self.area = Area::Request;
                    return Action::Select(rel.to_string(), i);
                }
                if self.tree.is_expanded(&id) {
                    self.tree.collapse(&id);
                } else {
                    self.tree.expand(&id);
                }
                Action::Redraw
            }
            k => {
                // Moving onto a request shows it.
                if self.tree.handle_key(k, &nodes) == Outcome::Moved {
                    if let Some(a) = self.show_selected(&nodes) {
                        return a;
                    }
                }
                Action::Redraw
            }
        }
    }

    /// The request selected in the tree, shown.
    fn show_selected(&mut self, nodes: &[TreeNode]) -> Option<Action> {
        let n = self.tree.selected_node(nodes)?;
        let (rel, i) = n.id.strip_prefix("r:").and_then(|r| r.rsplit_once('#'))?;
        let i: usize = i.parse().unwrap_or(0);
        self.selected = Some((rel.to_string(), i));
        self.viewing = None;
        self.row = 0;
        Some(Action::Select(rel.to_string(), i))
    }

    fn request_key(&mut self, key: Key, d: &Data) -> Action {
        let Some(r) = d.request else { return Action::Nothing };
        let tab = self.req_tab();
        let n = rows(r, tab).len();
        match key {
            Key::Char(']') => {
                self.req_tabs.active = (self.req_tabs.active + 1) % REQ_TABS.len();
                self.row = 0;
                Action::Redraw
            }
            Key::Char('[') => {
                self.req_tabs.active = (self.req_tabs.active + REQ_TABS.len() - 1) % REQ_TABS.len();
                self.row = 0;
                Action::Redraw
            }
            Key::Char(c @ '1'..='5') => {
                self.req_tabs.active = c as usize - '1' as usize;
                self.row = 0;
                Action::Redraw
            }
            Key::Char('j') | Key::Down => {
                self.row = (self.row + 1).min(n.saturating_sub(1));
                Action::Redraw
            }
            Key::Char('k') | Key::Up => {
                self.row = self.row.saturating_sub(1);
                Action::Redraw
            }
            Key::Char('m') => Action::Method,
            Key::Char('u') | Key::Char('i') => {
                self.url = TextField::new().with_text(&r.url);
                self.url.focused = true;
                self.editing_url = true;
                Action::Redraw
            }
            Key::Char('b') => Action::BodyType,
            Key::Char('a') => match tab {
                ReqTab::Body => Action::OpenFile(r.body.as_ref().map(|b| b.start).or(Some(r.headers_end()))),
                ReqTab::Auth => Action::Auth,
                t => Action::AddRow(t),
            },
            Key::Enter | Key::Char('c') => match tab {
                ReqTab::Body => Action::OpenFile(Some(r.body.as_ref().map(|b| b.start + self.row as u32).unwrap_or(r.headers_end()))),
                ReqTab::Auth => Action::Auth,
                t if n > 0 => Action::EditRow(t, self.row.min(n - 1)),
                t => Action::AddRow(t),
            },
            Key::Char('d') | Key::Delete if n > 0 => match tab {
                ReqTab::Body => Action::DeleteRow(ReqTab::Body, 0),
                t => Action::DeleteRow(t, self.row.min(n - 1)),
            },
            _ => Action::Nothing,
        }
    }

    pub fn paste(&mut self, text: &str) -> bool {
        if self.editing_url {
            self.url.paste(text);
            true
        } else if self.filtering {
            self.filter.paste(text);
            true
        } else {
            false
        }
    }

    /// The mouse: a click picks the area and what's under it.
    pub fn mouse(&mut self, m: &Mouse, d: &Data) -> Action {
        let [side, req, resp] = self.rects.get();
        if side.contains(m.x, m.y) {
            self.area = Area::Side;
            if self.side.mouse(m) != Outcome::Ignored {
                return Action::Redraw;
            }
            if self.side.active == 1 {
                let nodes = history_nodes(d.history, self.filter.text());
                let o = self.history.mouse(m, &nodes);
                if o == Outcome::Submit {
                    return self.side_key(Key::Enter, d);
                }
            } else {
                let nodes = tree(d.files);
                match self.tree.mouse(m, &nodes) {
                    Outcome::Submit => return self.side_key(Key::Enter, d),
                    Outcome::Moved => {
                        if let Some(a) = self.show_selected(&nodes) {
                            return a;
                        }
                    }
                    _ => {}
                }
            }
            return Action::Redraw;
        }
        if req.contains(m.x, m.y) {
            self.area = Area::Request;
            if self.req_tabs.mouse(m) != Outcome::Ignored {
                self.row = 0;
            } else if m.left_clicks() > 0 && m.y >= req.y + 3 {
                self.row = (m.y - req.y - 3).min(self.rows_len.get().saturating_sub(1));
                if m.left_clicks() == 2 {
                    return self.request_key(Key::Enter, d);
                }
            }
            return Action::Redraw;
        }
        if resp.contains(m.x, m.y) {
            self.area = Area::Response;
            self.response.mouse(m);
        }
        Action::Redraw
    }
}

/// `application/json; charset=utf-8` as `json`.
fn short_type(ct: &str) -> String {
    let t = ct.split(';').next().unwrap_or("").trim();
    let t = t.rsplit('/').next().unwrap_or(t);
    let t = t.rsplit('+').next().unwrap_or(t);
    match t {
        "x-www-form-urlencoded" => "form".into(),
        "form-data" => "multipart".into(),
        "octet-stream" => "binary".into(),
        t => t.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::HttpFile;

    const FILE: &str = "### users\n# @name users\n# @test status == 200\nGET https://x.test/users?page=1&q=a\nAccept: application/json\nAuthorization: Bearer abcdefghijklmnop\n\n{\"a\": 1}\n";

    fn files() -> Vec<FileEntry> {
        vec![
            FileEntry { rel: "api/users.http".into(), requests: vec![("GET".into(), "users".into(), 3), ("POST".into(), "create".into(), 9)] },
            FileEntry { rel: "api/admin/keys.http".into(), requests: vec![("DELETE".into(), "drop".into(), 0)] },
            FileEntry { rel: "top.http".into(), requests: vec![] },
        ]
    }

    #[test]
    fn the_collections_are_a_tree() {
        let nodes = tree(&files());
        assert_eq!(nodes.len(), 2);
        assert_eq!((nodes[0].id.as_str(), nodes[0].label.as_str()), ("d:api", "api/"));
        assert_eq!(nodes[0].children[0].id, "f:api/users.http");
        assert_eq!(nodes[0].children[0].children[1].label, "POST   create");
        assert_eq!(nodes[0].children[0].children[1].style, Style::new(Role::Accent));
        assert_eq!(nodes[0].children[1].id, "d:api/admin");
        assert_eq!(nodes[0].children[1].children[0].id, "f:api/admin/keys.http");
        assert_eq!(nodes[1].id, "f:top.http");
    }

    #[test]
    fn a_requests_tabs() {
        let f = HttpFile::parse(FILE);
        let r = &f.requests[0];
        assert_eq!(rows(r, ReqTab::Params), [("page".to_string(), "1".to_string()), ("q".into(), "a".into())]);
        assert_eq!(rows(r, ReqTab::Headers).len(), 2);
        assert_eq!(rows(r, ReqTab::Body), [("{\"a\": 1}".to_string(), String::new())]);
        assert_eq!(rows(r, ReqTab::Auth), [("Bearer".to_string(), "abcd…••••••".to_string())]);
        assert_eq!(rows(r, ReqTab::Tests), [("status == 200".to_string(), String::new())]);
        assert_eq!(short_type("application/vnd.api+json; x"), "json");
        assert_eq!(short_type("application/x-www-form-urlencoded"), "form");
    }

    #[test]
    fn the_page_is_drawn_and_keys_ask_for_edits() {
        let f = HttpFile::parse(FILE);
        let shown = Shown::default();
        let files = files();
        let d = Data { files: &files, request: Some(&f.requests[0]), env: Some("dev"), history: &[], shown: &shown, envs: 2 };
        let mut p = ClientPage::new();
        p.open_new(&tree(&files));
        p.select(&files, "api/users.http", 0);
        let c = p.render(&d, 100, 24);
        assert!(c.line(0).starts_with("HTTP   env dev  e switch"), "{}", c.line(0));
        assert!(c.text().contains("GET    users"), "{}", c.text());
        assert!(c.text().contains("GET https://x.test/users?page=1&q=a"));
        assert!(c.text().contains("1 Params 2   2 Headers 2   3 Body   4 Auth Bearer   5 Tests 1"), "{}", c.text());
        assert!(!c.typing(), "no field has the keyboard");
        p.side.active = 1;
        assert!(!p.render(&d, 100, 24).typing(), "nor the history's search, until /");
        p.area = Area::Side;
        p.key(Key::Char('/'), &d);
        assert!(p.render(&d, 100, 24).typing());
        p.key(Key::Escape, &d);
        p.side.active = 0;
        p.area = Area::Request;
        assert_eq!(p.key(Key::Char('j'), &d), Action::Redraw);
        assert_eq!(p.key(Key::Enter, &d), Action::EditRow(ReqTab::Params, 1));
        assert_eq!(p.key(Key::Char('2'), &d), Action::Redraw);
        assert_eq!(p.key(Key::Char('a'), &d), Action::AddRow(ReqTab::Headers));
        assert_eq!(p.key(Key::Char('d'), &d), Action::DeleteRow(ReqTab::Headers, 0));
        assert_eq!(p.key(Key::Char('m'), &d), Action::Method);
        assert_eq!(p.key(Key::Char('u'), &d), Action::Redraw);
        assert!(p.editing_url);
        for _ in 0..4 {
            p.key(Key::Backspace, &d);
        }
        assert_eq!(p.key(Key::Enter, &d), Action::SetUrl("https://x.test/users?page=1".into()));
        assert_eq!(p.key(Key::Char('3'), &d), Action::Redraw);
        assert_eq!(p.key(Key::Enter, &d), Action::OpenFile(Some(7)), "the body is edited in its file");
        assert_eq!(p.key(Key::Tab, &d), Action::Redraw);
        assert_eq!(p.area, Area::Response);
        assert_eq!(p.key(Key::Char('r'), &d), Action::Send);
        assert_eq!(p.key(Key::Tab, &d), Action::Redraw);
        assert_eq!(p.area, Area::Side);
        assert_eq!(p.key(Key::Char('x'), &d), Action::RunAll("api/users.http".into()));
    }
}
