//! The gRPC client's page: the server and where its services come from
//! in the header; the services and their methods (or the method's
//! history) in a sidebar; the method chosen -- how it streams, its input
//! and output, the metadata sent with it, the body as it'll go; and the
//! answer -- its status and how long it took, then tabs for its messages
//! (each with when it came), headers, trailers and timing.

use std::cell::Cell;
use std::collections::HashSet;

use fenix_ui_kit::{Canvas, Fill, Key, Mouse, Outcome, Rect, Role, ScrollState, Size, Split, Style, Tab, TabsState, TreeNode, TreeState};

use super::{code_role, json_line, ms, streaming_role};
use crate::grpc::call::{Outcome as Ended, Reply};
use crate::grpc::describe::{Method, Service};
use crate::grpc::history::Entry;
use crate::grpc::Target;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Side,
    Request,
    Response,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tabbed {
    Messages,
    Headers,
    Trailers,
    Timing,
}

const TABS: [Tabbed; 4] = [Tabbed::Messages, Tabbed::Headers, Tabbed::Trailers, Tabbed::Timing];

/// What a key on the page asks of the extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Redraw,
    /// The method chosen: a service's, by name.
    Select(String, String),
    Invoke,
    Cancel,
    PickTarget,
    AddTarget,
    ToggleTls,
    ToggleSource,
    Reload,
    /// The body's scratch buffer, beside.
    EditBody,
    /// The body made again from the input message.
    ResetBody,
    AddMetadata,
    EditMetadata(usize),
    DeleteMetadata(usize),
    /// A call from the method's history: its body and metadata to send
    /// again.
    FromHistory(usize),
    /// The messages, copied, or in a buffer of their own.
    CopyResponse,
    OpenResponse,
    InstallGrpcurl,
}

/// What the page draws from: the extension's, borrowed.
pub struct Data<'a> {
    pub target: Option<&'a Target>,
    pub targets: usize,
    /// `reflection` or `the project's protos`.
    pub source: &'a str,
    pub services: &'a [Service],
    /// What it's waiting for (`listing the services`), or why it failed.
    pub loading: Option<&'a str>,
    pub error: Option<&'a str>,
    /// The method chosen, and its service.
    pub method: Option<(&'a str, &'a Method)>,
    pub metadata: &'a [(String, String)],
    pub body: &'a str,
    pub reply: Option<&'a Reply>,
    pub history: &'a [Entry],
    /// grpcurl was found.
    pub grpcurl: bool,
}

#[derive(Debug)]
pub struct ClientPage {
    pub area: Area,
    pub side: TabsState,
    pub tree: TreeState,
    pub history_row: usize,
    pub meta_row: usize,
    pub tabs: TabsState,
    pub scroll: ScrollState,
    /// Services opened once already, so a new one opens and a closed one
    /// stays closed.
    opened: HashSet<String>,
    /// Lines in the response's tab, at the last render.
    len: Cell<usize>,
    rects: Cell<[Rect; 3]>,
}

impl Default for ClientPage {
    fn default() -> ClientPage {
        let mut tree = TreeState::new();
        tree.empty = "No services yet".into();
        ClientPage {
            area: Area::Side,
            side: {
                let mut t = TabsState::new();
                t.numbers = false;
                t
            },
            tree,
            history_row: 0,
            meta_row: 0,
            tabs: TabsState::new(),
            scroll: ScrollState::new(),
            opened: HashSet::new(),
            len: Cell::new(0),
            rects: Cell::new([Rect::default(); 3]),
        }
    }
}

/// The services as a tree: each with its methods, how each streams beside
/// it.
pub fn tree(services: &[Service]) -> Vec<TreeNode> {
    services
        .iter()
        .map(|s| {
            let methods = s.methods.iter().map(|m| TreeNode::leaf(format!("m:{}", s.path(&m.name)), m.name.clone()).detail(m.streaming.word()).style(streaming_role(m.streaming))).collect();
            let short = s.name.rsplit('.').next().unwrap_or(&s.name).to_string();
            TreeNode::branch(format!("s:{}", s.name), short, methods).detail(s.name.rsplit_once('.').map(|(p, _)| p.to_string()).unwrap_or_default()).style(Role::Title)
        })
        .collect()
}

/// When a call was, as the history says it.
fn when(at: i64) -> String {
    fenix_extension_api::chrono::DateTime::from_timestamp(at, 0).map(|t| t.format("%m-%d %H:%M").to_string()).unwrap_or_default()
}

/// The answer's status, in a few words: `OK · 12 ms · 1 message`.
pub fn status_line(r: &Reply) -> (String, Role) {
    let n = r.messages.len();
    let count = format!("{n} message{}", if n == 1 { "" } else { "s" });
    match &r.outcome {
        Ended::Running => (format!("Calling… {count}"), Role::Accent),
        Ended::Status { code, message, .. } => {
            let took = r.ended_at.map(ms).unwrap_or_default();
            let said = if message.is_empty() { String::new() } else { format!(": {message}") };
            (format!("{code}{said} · {took} · {count}"), code_role(code))
        }
        Ended::Failed(why) => (format!("Failed: {}", why.lines().next().unwrap_or("")), Role::Bad),
    }
}

impl ClientPage {
    pub fn new() -> ClientPage {
        ClientPage::default()
    }

    pub fn tab(&self) -> Tabbed {
        TABS[self.tabs.active.min(TABS.len() - 1)]
    }

    /// New services opened, the first time they're seen.
    pub fn open_new(&mut self, services: &[Service]) {
        for s in services {
            let id = format!("s:{}", s.name);
            if self.opened.insert(id.clone()) {
                self.tree.expand(&id);
            }
        }
    }

    /// The method `path` (`svc/Method`) selected in the tree.
    pub fn select(&mut self, services: &[Service], path: &str) {
        self.tree.select_id(&tree(services), &format!("m:{path}"));
        self.history_row = 0;
        self.meta_row = 0;
    }

    /// The response's lines on its tab.
    fn lines(&self, d: &Data) -> Vec<Vec<(String, Role)>> {
        let Some(r) = d.reply else { return Vec::new() };
        let pairs = |kv: &[(String, String)], none: &str| -> Vec<Vec<(String, Role)>> {
            if kv.is_empty() {
                return vec![vec![(none.to_string(), Role::Muted)]];
            }
            let w = kv.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0).min(32);
            kv.iter().map(|(k, v)| vec![(format!("{k:<w$}  "), Role::Syntax("property".into())), (v.clone(), Role::Text)]).collect()
        };
        match self.tab() {
            Tabbed::Messages => {
                let mut out = Vec::new();
                if let Ended::Status { code, message, details } = &r.outcome {
                    if code != "OK" {
                        out.push(vec![(format!("{code}: {message}"), code_role(code))]);
                        out.extend(details.iter().map(|d| vec![(format!("  {d}"), Role::Muted)]));
                        out.push(Vec::new());
                    }
                }
                if let Ended::Failed(why) = &r.outcome {
                    out.extend(why.lines().map(|l| vec![(l.to_string(), Role::Bad)]));
                }
                if r.messages.is_empty() && !matches!(r.outcome, Ended::Failed(_)) {
                    out.push(vec![(if r.running() { "Waiting for the first message…" } else { "No messages" }.to_string(), Role::Muted)]);
                }
                let several = r.messages.len() > 1;
                for (i, m) in r.messages.iter().enumerate() {
                    if several {
                        out.push(vec![(format!("#{}", i + 1), Role::Title), (format!("  +{}", ms(m.at)), Role::Muted)]);
                    }
                    out.extend(m.json.lines().map(json_line));
                }
                out
            }
            Tabbed::Headers => pairs(&r.headers, "No headers"),
            Tabbed::Trailers => pairs(&r.trailers, if r.running() { "Not yet: they come at the end" } else { "No trailers" }),
            Tabbed::Timing => {
                let mut out = Vec::new();
                let row = |what: &str, at: Option<u64>| vec![(format!("{what:<14}"), Role::Muted), (at.map(|a| format!("+{}", ms(a))).unwrap_or_else(|| "-".into()), Role::Text)];
                out.push(row("Headers", r.headers_at));
                for (i, m) in r.messages.iter().enumerate().take(200) {
                    out.push(row(&format!("Message {}", i + 1), Some(m.at)));
                }
                out.push(row("Ended", r.ended_at));
                if let Some(s) = &r.summary {
                    out.push(Vec::new());
                    out.push(vec![(s.clone(), Role::Muted)]);
                }
                if let Some(desc) = &r.descriptor {
                    out.push(vec![(desc.clone(), Role::Syntax("keyword".into()))]);
                }
                out
            }
        }
    }

    pub fn render(&mut self, d: &Data, width: usize, height: usize) -> Canvas {
        let mut c = Canvas::new();
        c.set_mouse(true);
        let mut x = c.put(0, 0, "gRPC", Role::Title);
        x = c.put(x, 0, "   ", Role::Muted);
        x = match d.target {
            Some(t) => {
                let x = c.put(x, 0, &t.address, Style::new(Role::Accent).bg(Fill::Panel));
                c.put(x, 0, if t.tls { "  TLS" } else { "  plaintext" }, if t.tls { Role::Good } else { Role::Muted })
            }
            None => c.put(x, 0, "no server: a adds one", Role::Warn),
        };
        x = c.put(x, 0, &format!("   via {}", d.source), Role::Muted);
        if !d.grpcurl {
            let say = "  grpcurl isn't installed: I installs it";
            if x + fenix_ui_kit::text::width(say) < width {
                c.put(x, 0, say, Role::Bad);
            }
        }
        c.rule(1, 0..width);
        let body = Rect::new(0, 2, width, height.saturating_sub(3));
        let (side, main) = Split::sidebar((width / 3).clamp(24, 40)).render(body, &mut c);
        self.render_side(d, side, &mut c);
        let (req, resp) = {
            let (a, b) = Split::vertical(Size::Percent(40)).min(5).render(main, &mut c);
            (a.inset_x(1), b.inset_x(1))
        };
        self.render_request(d, req, &mut c);
        self.render_response(d, resp, &mut c);
        self.rects.set([side, req, resp]);
        let keys = match self.area {
            Area::Side if self.side.active == 1 => "Enter send this again  ] methods  Tab next  r invoke  T servers",
            Area::Side => "Enter choose  r invoke  e body  R reload  s protos/reflection  t TLS  T servers  ] history  Tab next",
            Area::Request => "e edit the body  B reset it  a add metadata  Enter edit  d delete  r invoke  x cancel  Tab next",
            Area::Response => "1-4 tabs  j k scroll  y copy  O open  r invoke again  x cancel  Tab next",
        };
        c.put_fit(0, height.saturating_sub(1), width, keys, Role::Muted);
        c
    }

    fn render_side(&mut self, d: &Data, rect: Rect, c: &mut Canvas) {
        let (bar, rest) = rect.split_top(2);
        let methods: usize = d.services.iter().map(|s| s.methods.len()).sum();
        self.side.render(&[Tab::new("Methods").badge(methods.to_string()), Tab::new("History").badge(d.history.len().to_string())], bar, c);
        if self.side.active == 1 {
            if d.history.is_empty() {
                c.put_fit(rest.x, rest.y, rest.w, if d.method.is_some() { "Not called yet" } else { "Choose a method" }, Role::Muted);
                return;
            }
            for (i, e) in d.history.iter().enumerate().take(rest.h) {
                let y = rest.y + i;
                let x = c.put(rest.x, y, &format!("{:<5}", fenix_ui_kit::text::fit(&e.code, 5)), code_role(&e.code));
                let x = c.put(x, y, &format!("{:>8} ", ms(e.ms)), Role::Muted);
                c.put_fit(x, y, rest.right().saturating_sub(x), &format!("{}  {}", when(e.at), e.target), Role::Text);
                if self.area == Area::Side && i == self.history_row {
                    c.focus(y, rest.x..rest.right());
                }
            }
            return;
        }
        if let (Some(msg), true) = (d.loading.or(d.error), d.services.is_empty()) {
            let role = if d.error.is_some() { Role::Bad } else { Role::Muted };
            for (i, l) in fenix_ui_kit::text::wrap(msg, rest.w.max(10)).iter().enumerate().take(rest.h) {
                c.put(rest.x, rest.y + i, l, role.clone());
            }
            return;
        }
        let nodes = tree(d.services);
        // The tree itself, not a copy: where it was drawn is where the
        // mouse finds it.
        self.tree.focused = self.area == Area::Side;
        self.tree.render(&nodes, rest, c);
    }

    fn render_request(&self, d: &Data, rect: Rect, c: &mut Canvas) {
        if rect.is_empty() {
            return;
        }
        let Some((service, m)) = d.method else {
            c.put_fit(rect.x, rect.y, rect.w, "Choose a method in the sidebar", Role::Muted);
            return;
        };
        let x = c.put(rect.x, rect.y, &m.name, Role::Title);
        let x = c.put(x + 2, rect.y, m.streaming.word(), Style::new(streaming_role(m.streaming)).bg(Fill::Panel));
        c.put_fit(x + 2, rect.y, rect.right().saturating_sub(x + 2), service, Role::Muted);
        c.put_fit(rect.x, rect.y + 1, rect.w, &format!("{} → {}", m.input, m.output), Role::Syntax("type".into()));
        let mut y = rect.y + 3;
        let bottom = rect.bottom();
        if y >= bottom {
            return;
        }
        c.put(rect.x, y, &format!("Metadata {}", d.metadata.len()), Role::Title);
        if self.area == Area::Request {
            c.put(rect.right().saturating_sub(1), y, "●", Role::Accent);
        }
        y += 1;
        if d.metadata.is_empty() && y < bottom {
            c.put_fit(rect.x, y, rect.w, "None: a adds one (authorization: Bearer …)", Role::Muted);
            y += 1;
        }
        for (i, (k, v)) in d.metadata.iter().enumerate() {
            if y >= bottom {
                break;
            }
            let x = c.put(rect.x, y, &format!("{k}: "), Role::Syntax("property".into()));
            c.put_fit(x, y, rect.right().saturating_sub(x), v, Role::Text);
            if self.area == Area::Request && i == self.meta_row {
                c.focus(y, rect.x..rect.right());
            }
            y += 1;
        }
        y += 1;
        if y >= bottom {
            return;
        }
        c.put(rect.x, y, "Body", Role::Title);
        c.put(rect.x + 6, y, "e edits it beside", Role::Muted);
        y += 1;
        for line in d.body.lines() {
            if y >= bottom {
                break;
            }
            let mut x = rect.x;
            for (text, role) in json_line(line) {
                x = c.put(x, y, &fenix_ui_kit::text::fit(&text, rect.right().saturating_sub(x)), role);
            }
            y += 1;
        }
    }

    fn render_response(&self, d: &Data, rect: Rect, c: &mut Canvas) {
        if rect.is_empty() {
            return;
        }
        let (status, rest) = rect.split_top(1);
        match d.reply {
            None => {
                c.put_fit(status.x, status.y, status.w, "No answer yet: r invokes the method", Role::Muted);
                self.len.set(0);
                return;
            }
            Some(r) => {
                let (text, role) = status_line(r);
                c.put_fit(status.x, status.y, status.w, &text, Style::new(role).bg(Fill::Panel));
            }
        }
        let r = d.reply.expect("shown");
        let (bar, list) = rest.split_top(2);
        let badge = |n: usize| if n == 0 { String::new() } else { n.to_string() };
        let tabs = [Tab::new("Messages").badge(badge(r.messages.len())), Tab::new("Headers").badge(badge(r.headers.len())), Tab::new("Trailers").badge(badge(r.trailers.len())), Tab::new("Timing")];
        self.tabs.render(&tabs, bar, c);
        if self.area == Area::Response {
            c.put(bar.right().saturating_sub(1), bar.y, "●", Role::Accent);
        }
        let lines = self.lines(d);
        self.len.set(lines.len());
        self.scroll.render(lines.len(), list, c, |i, row, c| {
            let mut x = row.x;
            for (text, role) in &lines[i] {
                if x >= row.right() {
                    break;
                }
                x = c.put(x, row.y, &fenix_ui_kit::text::fit(text, row.right() - x), role.clone());
            }
        });
    }

    /// The method the tree's row is, when it's one.
    fn tree_method(&self, d: &Data) -> Option<(String, String)> {
        let nodes = tree(d.services);
        let id = self.tree.selected_node(&nodes)?.id.clone();
        let path = id.strip_prefix("m:")?;
        let (svc, m) = path.rsplit_once('/')?;
        Some((svc.to_string(), m.to_string()))
    }

    pub fn key(&mut self, key: Key, d: &Data) -> Action {
        // Keys for every area.
        match key {
            Key::Tab => {
                self.area = match self.area {
                    Area::Side => Area::Request,
                    Area::Request => Area::Response,
                    Area::Response => Area::Side,
                };
                return Action::Redraw;
            }
            Key::BackTab => {
                self.area = match self.area {
                    Area::Side => Area::Response,
                    Area::Request => Area::Side,
                    Area::Response => Area::Request,
                };
                return Action::Redraw;
            }
            Key::Char('r') => return Action::Invoke,
            Key::Char('x') => return Action::Cancel,
            Key::Char('t') => return Action::ToggleTls,
            Key::Char('T') => return Action::PickTarget,
            Key::Char('s') => return Action::ToggleSource,
            Key::Char('R') => return Action::Reload,
            Key::Char('e') => return Action::EditBody,
            Key::Char('B') => return Action::ResetBody,
            Key::Char('I') if !d.grpcurl => return Action::InstallGrpcurl,
            Key::Char('a') if d.target.is_none() || (self.area == Area::Side && d.targets == 0) => return Action::AddTarget,
            Key::Char('y') => return Action::CopyResponse,
            Key::Char('O') => return Action::OpenResponse,
            _ => {}
        }
        match self.area {
            Area::Side => {
                if matches!(key, Key::Char(']') | Key::Char('[')) {
                    self.side.active = 1 - self.side.active.min(1);
                    return Action::Redraw;
                }
                if self.side.active == 1 {
                    let n = d.history.len();
                    return match key {
                        Key::Char('j') | Key::Down if self.history_row + 1 < n => {
                            self.history_row += 1;
                            Action::Redraw
                        }
                        Key::Char('k') | Key::Up if self.history_row > 0 => {
                            self.history_row -= 1;
                            Action::Redraw
                        }
                        Key::Enter if self.history_row < n => Action::FromHistory(self.history_row),
                        _ => Action::Nothing,
                    };
                }
                let nodes = tree(d.services);
                match self.tree.handle_key(key, &nodes) {
                    Outcome::Submit => match self.tree_method(d) {
                        Some((s, m)) => Action::Select(s, m),
                        None => {
                            if let Some(n) = self.tree.selected_node(&nodes) {
                                let id = n.id.clone();
                                if self.tree.is_expanded(&id) {
                                    self.tree.collapse(&id);
                                } else {
                                    self.tree.expand(&id);
                                }
                            }
                            Action::Redraw
                        }
                    },
                    o if o.handled() => Action::Redraw,
                    _ => Action::Nothing,
                }
            }
            Area::Request => {
                let n = d.metadata.len();
                match key {
                    Key::Char('j') | Key::Down if self.meta_row + 1 < n => {
                        self.meta_row += 1;
                        Action::Redraw
                    }
                    Key::Char('k') | Key::Up if self.meta_row > 0 => {
                        self.meta_row -= 1;
                        Action::Redraw
                    }
                    Key::Char('a') => Action::AddMetadata,
                    Key::Enter if self.meta_row < n => Action::EditMetadata(self.meta_row),
                    Key::Enter => Action::EditBody,
                    Key::Char('d') if self.meta_row < n => {
                        let i = self.meta_row;
                        self.meta_row = self.meta_row.min(n.saturating_sub(2));
                        Action::DeleteMetadata(i)
                    }
                    _ => Action::Nothing,
                }
            }
            Area::Response => {
                if self.tabs.handle_key(key, TABS.len()).handled() {
                    self.scroll.scroll_to(0, 0);
                    return Action::Redraw;
                }
                match key {
                    Key::Char(']') => {
                        self.tabs.active = (self.tabs.active + 1) % TABS.len();
                        Action::Redraw
                    }
                    Key::Char('[') => {
                        self.tabs.active = (self.tabs.active + TABS.len() - 1) % TABS.len();
                        Action::Redraw
                    }
                    k if self.scroll.handle_key(k, self.len.get()).handled() => Action::Redraw,
                    _ => Action::Nothing,
                }
            }
        }
    }

    /// A click or the wheel: the area under it takes it.
    pub fn mouse(&mut self, m: &Mouse, d: &Data) -> Action {
        let [side, req, resp] = self.rects.get();
        if side.contains(m.x, m.y) {
            self.area = Area::Side;
            if self.side.mouse(m).handled() {
                return Action::Redraw;
            }
            if self.side.active == 0 {
                let nodes = tree(d.services);
                return match self.tree.mouse(m, &nodes) {
                    Outcome::Submit => self.tree_method(d).map(|(s, me)| Action::Select(s, me)).unwrap_or(Action::Redraw),
                    _ => Action::Redraw,
                };
            }
            return Action::Redraw;
        }
        if req.contains(m.x, m.y) {
            self.area = Area::Request;
            return if m.left_clicks() == 2 { Action::EditBody } else { Action::Redraw };
        }
        if resp.contains(m.x, m.y) {
            self.area = Area::Response;
            if self.tabs.mouse(m).handled() {
                return Action::Redraw;
            }
            self.scroll.mouse(m, self.len.get());
            return Action::Redraw;
        }
        Action::Nothing
    }

    /// What's kept with the session: the tab and area.
    pub fn state(&self) -> (usize, usize) {
        (self.side.active, self.tabs.active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::call::{Reader, Received};
    use crate::grpc::describe::parse_describe;

    fn services() -> Vec<Service> {
        parse_describe(include_str!("../samples/grpcurl-describe-service.txt")).services
    }

    fn reply() -> Reply {
        let mut reply = Reply::default();
        let mut r = Reader::new();
        for (i, l) in include_str!("../samples/grpcurl-unary.txt").lines().enumerate() {
            r.line(&mut reply, l, false, 2 + i as u64);
        }
        r.end(&mut reply, Some(0), 48);
        reply
    }

    #[test]
    fn the_page_shows_the_server_the_methods_the_request_and_the_answer() {
        let svcs = services();
        let target = Target::new("localhost:50051", false);
        let method = svcs[0].methods[0].clone();
        let meta = vec![("x-team".to_string(), "ops".to_string())];
        let r = reply();
        let d = Data { target: Some(&target), targets: 1, source: "reflection", services: &svcs, loading: None, error: None, method: Some(("orbit.v1.OrbitService", &method)), metadata: &meta, body: "{\n  \"id\": \"25544\"\n}\n", reply: Some(&r), history: &[], grpcurl: true };
        let mut p = ClientPage::new();
        p.open_new(&svcs);
        p.select(&svcs, "orbit.v1.OrbitService/GetSatellite");
        let c = p.render(&d, 110, 30);
        let text = c.text();
        assert_eq!(c.line(0).trim_end(), "gRPC   localhost:50051  plaintext   via reflection");
        assert!(text.contains("▾ OrbitService") && text.contains("GetSatellite") && text.contains("server stream"), "{text}");
        assert!(text.contains("GetSatellite  unary  orbit.v1.OrbitService"), "{text}");
        assert!(text.contains("orbit.v1.GetSatelliteRequest → orbit.v1.GetSatelliteResponse"));
        assert!(text.contains("Metadata 1") && text.contains("x-team: ops"));
        assert!(text.contains("\"id\": \"25544\""), "the body as it'll go");
        assert!(text.contains("OK · 48 ms · 1 message"), "{text}");
        assert!(text.contains("1 Messages 1   2 Headers 3   3 Trailers 1   4 Timing"), "{text}");
        assert!(text.contains("\"name\": \"ISS ops\""));
        assert!(c.line(29).starts_with("Enter choose  r invoke"));
        let focused = c.focused().map(|(y, _)| c.line(*y)).unwrap_or_default();
        assert!(focused.contains("GetSatellite") && !focused.contains("Request"), "the method's row: {focused}");

        // Trailers and timing.
        p.area = Area::Response;
        assert_eq!(p.key(Key::Char('3'), &d), Action::Redraw);
        assert!(p.render(&d, 110, 30).text().contains("x-cost  3"));
        p.key(Key::Char('4'), &d);
        let timing = p.render(&d, 110, 30).text();
        assert!(timing.contains("Headers       +") && timing.contains("Ended         +48 ms") && timing.contains("Sent 1 request and received 1 response"), "{timing}");
    }

    #[test]
    fn a_stream_shows_each_message_and_an_error_its_status() {
        let svcs = services();
        let method = svcs[0].methods[1].clone();
        let r = Reply { messages: vec![Received { at: 10, json: "{\n  \"rise\": \"x\"\n}".into() }, Received { at: 210, json: "{\n  \"rise\": \"y\"\n}".into() }], ..Reply::default() };
        let d = Data { target: None, targets: 0, source: "the project's protos", services: &svcs, loading: None, error: None, method: Some(("orbit.v1.OrbitService", &method)), metadata: &[], body: "{}", reply: Some(&r), history: &[], grpcurl: false };
        let mut p = ClientPage::new();
        let text = p.render(&d, 100, 30).text();
        assert!(text.contains("Calling… 2 messages") && text.contains("#2  +210 ms"), "{text}");
        assert!(text.contains("no server: a adds one") && text.contains("grpcurl isn't installed: I installs it"), "{text}");
        assert!(text.contains("None: a adds one"));

        let mut r = r.clone();
        r.outcome = Ended::Status { code: "NotFound".into(), message: "no satellite".into(), details: Vec::new() };
        r.ended_at = Some(300);
        r.messages.clear();
        let d = Data { reply: Some(&r), ..d };
        let text = p.render(&d, 100, 30).text();
        assert!(text.contains("NotFound: no satellite · 300 ms · 0 messages") && text.contains("No messages"), "{text}");
        let failed = Reply { outcome: Ended::Failed("Failed to dial target host".into()), ..Reply::default() };
        assert_eq!(status_line(&failed), ("Failed: Failed to dial target host".into(), Role::Bad));
    }

    #[test]
    fn keys_choose_a_method_edit_metadata_and_call() {
        let svcs = services();
        let target = Target::new("localhost:50051", true);
        let meta = vec![("x-team".to_string(), "ops".to_string()), ("authorization".to_string(), "Bearer x".to_string())];
        let history = vec![Entry { at: 0, target: "localhost:50051".into(), body: "{}".into(), metadata: Vec::new(), code: "OK".into(), message: String::new(), ms: 12, messages: 1, first: String::new() }];
        let method = svcs[0].methods[0].clone();
        let d = Data { target: Some(&target), targets: 1, source: "reflection", services: &svcs, loading: None, error: None, method: Some(("orbit.v1.OrbitService", &method)), metadata: &meta, body: "{}", reply: None, history: &history, grpcurl: true };
        let mut p = ClientPage::new();
        p.open_new(&svcs);
        p.render(&d, 100, 30);
        assert_eq!(p.key(Key::Char('j'), &d), Action::Redraw);
        assert_eq!(p.key(Key::Enter, &d), Action::Select("orbit.v1.OrbitService".into(), "GetSatellite".into()));
        p.key(Key::Char('j'), &d);
        assert_eq!(p.key(Key::Enter, &d), Action::Select("orbit.v1.OrbitService".into(), "WatchPasses".into()));
        p.key(Key::Char('g'), &d);
        assert_eq!(p.key(Key::Enter, &d), Action::Redraw, "a service opens and closes");
        assert_eq!((p.key(Key::Char('r'), &d), p.key(Key::Char('t'), &d), p.key(Key::Char('T'), &d), p.key(Key::Char('s'), &d)), (Action::Invoke, Action::ToggleTls, Action::PickTarget, Action::ToggleSource));
        assert_eq!(p.key(Key::Char('I'), &d), Action::Nothing, "grpcurl is there");

        // The history: a call sent again.
        p.key(Key::Char(']'), &d);
        let text = p.render(&d, 100, 30).text();
        assert!(text.contains("OK      12 ms 01-01 00:00  local…"), "{text}");
        assert_eq!(p.key(Key::Enter, &d), Action::FromHistory(0));

        // Metadata.
        p.key(Key::Tab, &d);
        assert_eq!(p.area, Area::Request);
        assert_eq!(p.key(Key::Char('a'), &d), Action::AddMetadata);
        p.key(Key::Char('j'), &d);
        assert_eq!(p.key(Key::Enter, &d), Action::EditMetadata(1));
        assert_eq!(p.key(Key::Char('d'), &d), Action::DeleteMetadata(1));
        assert_eq!(p.meta_row, 0);
        assert_eq!(p.key(Key::Char('e'), &d), Action::EditBody);
        p.key(Key::Tab, &d);
        assert_eq!((p.area, p.key(Key::Char('x'), &d), p.key(Key::Char('y'), &d)), (Area::Response, Action::Cancel, Action::CopyResponse));
        p.key(Key::Tab, &d);
        assert_eq!(p.area, Area::Side);

        let none = Data { target: None, targets: 0, ..d };
        assert_eq!(p.key(Key::Char('a'), &none), Action::AddTarget, "no server: a adds one");
    }

    #[test]
    fn the_mouse_picks_an_area_and_a_method() {
        let svcs = services();
        let method = svcs[0].methods[0].clone();
        let d = Data { target: None, targets: 0, source: "reflection", services: &svcs, loading: Some("listing the services…"), error: None, method: Some(("orbit.v1.OrbitService", &method)), metadata: &[], body: "{}", reply: None, history: &[], grpcurl: true };
        let mut p = ClientPage::new();
        p.open_new(&svcs);
        let c = p.render(&d, 100, 30);
        let y = (0..30).find(|y| c.line(*y).contains("WatchPasses")).unwrap();
        let mut click = Mouse::click(4, y);
        click.action = fenix_ui_kit::MouseAction::Press { button: fenix_ui_kit::Button::Left, clicks: 2 };
        assert_eq!(p.mouse(&click, &d), Action::Select("orbit.v1.OrbitService".into(), "WatchPasses".into()));
        assert_eq!(p.mouse(&Mouse::click(60, 5), &d), Action::Redraw);
        assert_eq!(p.area, Area::Request);

        // Still listing, nothing yet: what it's doing.
        let waiting = Data { services: &[], ..d };
        assert!(ClientPage::new().render(&waiting, 100, 30).text().contains("listing the services…"));
        let failed = Data { services: &[], loading: None, error: Some("server does not support the reflection API: s uses the project's protos"), ..waiting };
        assert!(ClientPage::new().render(&failed, 100, 30).text().contains("server does not support the"));
    }
}
