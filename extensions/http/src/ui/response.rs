//! The response view: its status, how long it took and how big it is, then
//! tabs for the body (pretty-printed and coloured, a picture shown), the
//! headers, the cookies, the timing and the tests. In the client's page,
//! and on its own beside a `.http` file you run a request from.

use std::cell::Cell;

use fenix_ui_kit::{Canvas, Fill, Key, Mouse, Outcome, Rect, Role, ScrollState, Style, Tab, TabsState};

use crate::assert::Outcome as Test;
use crate::cookies::{self, Cookie};
use crate::model::{duration, size, Exchange};
use crate::pretty::{self, Kind, Line};

/// What the view shows: an exchange, how its tests went, the cookies its
/// environment keeps for it, and its picture when it's one.
#[derive(Debug, Clone, Default)]
pub struct Shown {
    pub exchange: Option<Exchange>,
    pub tests: Vec<Test>,
    pub jar: Vec<Cookie>,
    /// Sent, its answer not here yet (or coming, streamed).
    pub sending: bool,
    /// An image it loaded for a picture's body: id, width, height.
    pub image: Option<(u32, u32, u32)>,
    /// What it's about: a request's label.
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tabbed {
    Body,
    Headers,
    Cookies,
    Timing,
    Tests,
}

const TABS: [Tabbed; 5] = [Tabbed::Body, Tabbed::Headers, Tabbed::Cookies, Tabbed::Timing, Tabbed::Tests];

/// What a body was laid out for: when it came, its size, raw or not.
type BodyKey = (i64, usize, bool);

#[derive(Debug, Default)]
pub struct ResponseView {
    pub tabs: TabsState,
    pub scroll: ScrollState,
    /// The body as it came, not pretty-printed.
    pub raw: bool,
    /// The body's lines, laid out once: for what (when, how).
    cache: std::cell::RefCell<Option<(BodyKey, std::rc::Rc<Vec<Line>>)>>,
    /// Lines in the tab shown, at the last render.
    len: Cell<usize>,
}

/// A status's colour: good, a redirect, the client's fault, the server's.
pub fn status_role(status: u16) -> Role {
    match status {
        200..=299 => Role::Good,
        300..=399 => Role::Accent,
        400..=499 => Role::Warn,
        _ => Role::Bad,
    }
}

impl ResponseView {
    pub fn new() -> ResponseView {
        ResponseView { tabs: TabsState::new(), ..ResponseView::default() }
    }

    pub fn tab(&self) -> Tabbed {
        TABS[self.tabs.active.min(TABS.len() - 1)]
    }

    fn body_lines(&self, ex: &Exchange) -> std::rc::Rc<Vec<Line>> {
        let Ok(r) = &ex.answer else { return Default::default() };
        let key = (ex.at, r.body.len(), self.raw);
        if let Some((k, lines)) = &*self.cache.borrow() {
            if *k == key {
                return lines.clone();
            }
        }
        let kind = pretty::kind(r.content_type(), &r.body);
        let lines = std::rc::Rc::new(pretty::lines(kind, &r.body, !self.raw));
        *self.cache.borrow_mut() = Some((key, lines.clone()));
        lines
    }

    /// The body laid out again: it changed as it streamed in.
    pub fn forget(&self) {
        *self.cache.borrow_mut() = None;
    }

    /// The tab's lines, for the scrolled region.
    fn lines(&self, shown: &Shown, width: usize) -> Vec<Line> {
        let Some(ex) = &shown.exchange else { return Vec::new() };
        let r = match &ex.answer {
            Ok(r) => r,
            Err(e) => return crate::ui::wrap(&format!("No answer: {e}"), width).into_iter().map(|l| vec![(l, Role::Bad)]).collect(),
        };
        match self.tab() {
            Tabbed::Body => {
                if r.body.is_empty() {
                    return vec![vec![("No body".into(), Role::Muted)]];
                }
                self.body_lines(ex).as_ref().clone()
            }
            Tabbed::Headers => {
                let w = r.headers.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0).min(32);
                r.headers.iter().map(|(k, v)| vec![(format!("{k:<w$}  "), Role::Syntax("property".into())), (v.clone(), Role::Text)]).collect()
            }
            Tabbed::Cookies => {
                let (host, path) = (crate::url::host(&r.url).map(|h| h.1).unwrap_or_default(), crate::url::path(&r.url));
                let mut out: Vec<Line> = Vec::new();
                let set: Vec<Cookie> = r.headers_named("set-cookie").filter_map(|h| cookies::parse(h, &host, &path, ex.at)).collect();
                if !set.is_empty() {
                    out.push(vec![("Set by this answer".into(), Role::Title)]);
                    out.extend(set.iter().map(cookie_line));
                    out.push(Vec::new());
                }
                out.push(vec![(format!("Kept for {}", ex.env.as_deref().unwrap_or("no environment")), Role::Title)]);
                if shown.jar.is_empty() {
                    out.push(vec![("None".into(), Role::Muted)]);
                }
                out.extend(shown.jar.iter().map(cookie_line));
                out
            }
            Tabbed::Timing => timing(r, width),
            Tabbed::Tests => {
                if shown.tests.is_empty() {
                    return vec![vec![("No tests: add one, # @test status == 200".into(), Role::Muted)]];
                }
                shown.tests.iter().map(|t| vec![(if t.passed { "✓ ".into() } else { "✗ ".into() }, if t.passed { Role::Good } else { Role::Bad }), (t.test.clone(), Role::Text), (format!("  {}", t.detail), Role::Muted)]).collect()
            }
        }
    }

    /// Draws it into `rect`.
    pub fn render(&self, shown: &Shown, rect: Rect, focused: bool, c: &mut Canvas) {
        if rect.is_empty() {
            return;
        }
        let (status, rest) = rect.split_top(1);
        match &shown.exchange {
            None if shown.sending => {
                c.put(status.x, status.y, "Sending…", Role::Muted);
            }
            None => {
                c.put_fit(status.x, status.y, status.w, "No response yet: r sends the request", Role::Muted);
                return;
            }
            Some(ex) => {
                let mut x = status.x;
                match &ex.answer {
                    Ok(r) => {
                        x = c.put(x, status.y, &r.status_line(), Style::new(status_role(r.status)).bg(Fill::Panel));
                        x = c.put(x, status.y, &format!("  {}  ·  {}", duration(r.timing.total), size(r.body.len())), Role::Muted);
                        if shown.sending {
                            x = c.put(x, status.y, "  ·  streaming…", Role::Accent);
                        }
                        if !shown.tests.is_empty() {
                            let passed = shown.tests.iter().filter(|t| t.passed).count();
                            let role = if passed == shown.tests.len() { Role::Good } else { Role::Bad };
                            x = c.put(x, status.y, &format!("  ·  tests {passed}/{}", shown.tests.len()), role);
                        }
                        if r.url != ex.sent.url && !r.url.is_empty() {
                            let room = status.right().saturating_sub(x + 4);
                            if room > 8 {
                                c.put(x, status.y, &format!("  → {}", fenix_ui_kit::text::fit(&r.url, room)), Role::Muted);
                            }
                        }
                    }
                    Err(_) => {
                        c.put(x, status.y, "Failed", Style::new(Role::Bad).bg(Fill::Panel));
                    }
                }
            }
        }
        let (bar, body) = rest.split_top(2);
        let ex = shown.exchange.as_ref();
        let ok = ex.and_then(|e| e.answer.as_ref().ok());
        let tabs = [
            super::tab("Body", ok.map(|r| pretty_kind(r.content_type(), &r.body)).unwrap_or_default()),
            super::tab("Headers", ok.map(|r| r.headers.len().to_string()).unwrap_or_default()),
            super::tab("Cookies", ok.map(|r| r.headers_named("set-cookie").count()).filter(|n| *n > 0).map(|n| n.to_string()).unwrap_or_default()),
            Tab::new("Timing"),
            super::tab("Tests", if shown.tests.is_empty() { String::new() } else { format!("{}/{}", shown.tests.iter().filter(|t| t.passed).count(), shown.tests.len()) }),
        ];
        self.tabs.render(&tabs, bar, c);
        if focused {
            c.put(bar.right().saturating_sub(1), bar.y, "●", Role::Accent);
        }
        // A picture: drawn in the body's cells.
        if let (Tabbed::Body, Some((id, w, h)), false) = (self.tab(), shown.image, self.raw) {
            let rows = body.h.min(((h as f32 / w.max(1) as f32) * body.w as f32 / 2.0).ceil() as usize).max(1);
            c.picture(Rect::new(body.x, body.y, body.w, rows), id, None, Vec::new());
            if body.h > rows {
                c.put(body.x, body.y + rows, &format!("{w} x {h} pixels · p shows its bytes"), Role::Muted);
            }
            self.len.set(0);
            return;
        }
        let lines = self.lines(shown, body.w);
        self.len.set(lines.len());
        self.scroll.render(lines.len(), body, c, |i, row, c| {
            let mut x = row.x;
            for (text, role) in &lines[i] {
                if x >= row.right() {
                    break;
                }
                x = c.put(x, row.y, &fenix_ui_kit::text::fit(text, row.right() - x), role.clone());
            }
        });
    }

    /// Its keys: the tabs (`[` `]`, `1`-`5`), scrolling, `p` the body as it
    /// came.
    pub fn key(&mut self, key: Key) -> Outcome {
        match key {
            Key::Char(']') => {
                self.tabs.active = (self.tabs.active + 1) % TABS.len();
                self.scroll.scroll_to(0, 0);
                Outcome::Moved
            }
            Key::Char('[') => {
                self.tabs.active = (self.tabs.active + TABS.len() - 1) % TABS.len();
                self.scroll.scroll_to(0, 0);
                Outcome::Moved
            }
            Key::Char('p') => {
                self.raw = !self.raw;
                Outcome::Moved
            }
            Key::Char(c @ '1'..='5') => {
                self.tabs.active = c as usize - '1' as usize;
                self.scroll.scroll_to(0, 0);
                Outcome::Moved
            }
            k => self.scroll.handle_key(k, self.len.get()),
        }
    }

    pub fn mouse(&mut self, m: &Mouse) -> Outcome {
        match self.tabs.mouse(m) {
            Outcome::Ignored => self.scroll.mouse(m, self.len.get()),
            o => o,
        }
    }
}

fn pretty_kind(ct: &str, body: &[u8]) -> String {
    match pretty::kind(ct, body) {
        Kind::Json => "json".into(),
        Kind::Xml => "xml".into(),
        Kind::Html => "html".into(),
        Kind::Text => "text".into(),
        Kind::Image => "image".into(),
        Kind::Binary => size(body.len()),
    }
}

fn cookie_line(c: &Cookie) -> Line {
    let mut flags = Vec::new();
    if c.secure {
        flags.push("Secure".to_string());
    }
    if c.http_only {
        flags.push("HttpOnly".into());
    }
    if let Some(e) = c.expires {
        if let Some(t) = fenix_extension_api::chrono::DateTime::from_timestamp(e, 0) {
            flags.push(format!("until {}", t.format("%Y-%m-%d %H:%M")));
        }
    }
    vec![(format!("{}=", c.name), Role::Syntax("property".into())), (c.value.clone(), Role::Text), (format!("  {}{} {}", if c.host_only { "" } else { "." }, c.domain, c.path), Role::Muted), (if flags.is_empty() { String::new() } else { format!("  {}", flags.join(" · ")) }, Role::Muted)]
}

/// Each part of the timing as a bar along the whole.
fn timing(r: &crate::model::Received, width: usize) -> Vec<Line> {
    let t = r.timing;
    let total = t.total.max(1);
    let parts: Vec<(&str, Option<u64>)> = vec![("DNS", t.dns), ("Connect", t.connect), ("TLS", t.tls), ("First byte", Some(t.first_byte))];
    let used: u64 = parts.iter().filter_map(|p| p.1).sum();
    let download = t.total.saturating_sub(used);
    let bar_w = width.saturating_sub(24).max(10);
    let mut out = Vec::new();
    let mut at = 0u64;
    let rows = parts.into_iter().chain(std::iter::once(("Download", Some(download))));
    for (name, v) in rows {
        match v {
            Some(v) => {
                let from = (at as f64 / total as f64 * bar_w as f64) as usize;
                let len = ((v as f64 / total as f64 * bar_w as f64).round() as usize).max(usize::from(v > 0));
                at += v;
                out.push(vec![(format!("{name:<11}"), Role::Text), (format!("{:>9}  ", duration(v)), Role::Muted), (" ".repeat(from.min(bar_w)), Role::Text), ("█".repeat(len.min(bar_w - from.min(bar_w))), Role::Accent)]);
            }
            None => out.push(vec![(format!("{name:<11}"), Role::Text), (format!("{:>9}", "—"), Role::Muted)]),
        }
    }
    out.push(Vec::new());
    out.push(vec![(format!("{:<11}", "Total"), Role::Title), (format!("{:>9}", duration(t.total)), Role::Title)]);
    if t.connect.is_none() && r.url.starts_with("http:") {
        out.push(vec![("On http the connection is in First byte".into(), Role::Muted)]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Received, Sent, Timing};

    pub fn shown() -> Shown {
        Shown {
            exchange: Some(Exchange {
                sent: Sent { method: "GET".into(), url: "https://x.test/a".into(), ..Sent::default() },
                answer: Ok(Received {
                    status: 200,
                    headers: vec![("Content-Type".into(), "application/json".into()), ("Set-Cookie".into(), "sid=1; HttpOnly".into())],
                    body: br#"{"a":1}"#.to_vec(),
                    url: "https://x.test/b".into(),
                    timing: Timing { dns: Some(1000), connect: Some(2000), tls: Some(3000), first_byte: 10_000, total: 20_000 },
                }),
                at: 1,
                file: None,
                name: None,
                env: Some("dev".into()),
            }),
            tests: vec![Test { test: "status == 200".into(), passed: true, detail: "200".into() }],
            ..Shown::default()
        }
    }

    fn draw(v: &ResponseView, s: &Shown) -> Canvas {
        let mut c = Canvas::new();
        v.render(s, Rect::new(0, 0, 70, 12), false, &mut c);
        c
    }

    #[test]
    fn the_status_line_and_the_body() {
        let v = ResponseView::new();
        let c = draw(&v, &shown());
        assert_eq!(c.line(0), "200 OK  20 ms  ·  7 B  ·  tests 1/1  → https://x.test/b");
        assert!(c.line(1).starts_with("1 Body json   2 Headers 2   3 Cookies 1   4 Timing   5 Tests 1/1"), "{}", c.line(1));
        assert_eq!((c.line(3), c.line(4), c.line(5)), ("{".into(), "  \"a\": 1".into(), "}".into()));
    }

    #[test]
    fn each_tab() {
        let mut v = ResponseView::new();
        let s = shown();
        v.key(Key::Char('2'));
        assert_eq!(draw(&v, &s).line(3), "Content-Type  application/json");
        v.key(Key::Char(']'));
        let c = draw(&v, &s);
        assert_eq!(c.line(3), "Set by this answer");
        assert!(c.line(4).starts_with("sid=1  x.test /  HttpOnly"), "{}", c.line(4));
        v.key(Key::Char(']'));
        let c = draw(&v, &s);
        assert!(c.line(3).starts_with("DNS             1 ms  █"), "{}", c.line(3));
        assert!(c.text().contains("Total          20 ms"));
        v.key(Key::Char(']'));
        assert_eq!(draw(&v, &s).line(3), "✓ status == 200  200");
        v.key(Key::Char(']'));
        assert_eq!(v.tab(), Tabbed::Body, "round again");
        let failed = Shown { exchange: s.exchange.clone().map(|e| Exchange { answer: Err("connection refused".into()), ..e }), ..Shown::default() };
        let c = draw(&v, &failed);
        assert_eq!((c.line(0).as_str(), c.line(3).as_str()), ("Failed", "No answer: connection refused"));
        assert!(draw(&v, &Shown::default()).line(0).starts_with("No response yet"));
    }
}
