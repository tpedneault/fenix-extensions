//! Tabs: a response's parts, as an HTTP client might show them.

// gallery:begin
use fenix_ui_kit::{Canvas, Key, KeyPress, Mouse, Rect, Role, Tab, TabsState};

use crate::draw;
use crate::sections::Reply;

pub struct State {
    pub tabs: TabsState,
}

fn tabs() -> Vec<Tab> {
    vec![Tab::new("Summary"), Tab::new("Headers").badge("8"), Tab::new("Body").badge("2.4 KB"), Tab::new("Timing"), Tab::new("Cookies").badge("0")]
}

/// What each tab shows: a line each, its style.
fn page(tab: usize) -> Vec<(&'static str, Role)> {
    match tab {
        0 => vec![
            ("GET https://tides.example.test/v1/stations/brest/predictions?days=2", Role::Title),
            ("200 OK", Role::Good),
            ("2.4 KB in 182 ms, from the cache: no", Role::Muted),
        ],
        1 => vec![
            ("content-type: application/json; charset=utf-8", Role::Text),
            ("content-length: 2461", Role::Text),
            ("cache-control: max-age=600", Role::Text),
            ("etag: \"5f1c-18e2a\"", Role::Text),
            ("date: Thu, 19 Mar 2026 09:14:07 GMT", Role::Text),
            ("server: tides-api/0.5", Role::Text),
            ("vary: accept-encoding", Role::Text),
            ("x-request-id: 7d0c55e2", Role::Muted),
        ],
        2 => vec![
            ("{", Role::Text),
            ("  \"station\": \"brest\",", Role::Syntax("string".into())),
            ("  \"datum\": \"LAT\",", Role::Syntax("string".into())),
            ("  \"high\": [\"2026-03-19T04:51Z\", \"2026-03-19T17:12Z\"],", Role::Syntax("string".into())),
            ("  \"heights_m\": [6.82, 6.95]", Role::Syntax("number".into())),
            ("}", Role::Text),
        ],
        3 => vec![("dns      4 ms", Role::Text), ("connect 21 ms", Role::Text), ("tls     38 ms", Role::Text), ("first   164 ms", Role::Text), ("total   182 ms", Role::Title)],
        _ => vec![("No cookies were set", Role::Muted)],
    }
}

impl State {
    pub fn new() -> State {
        State { tabs: TabsState::new() }
    }

    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = vec![("Tab S-Tab", "next, previous")];
        if self.tabs.numbers {
            keys.push(("1-5", "a tab"));
        }
        keys.push(("n", "numbers on, off"));
        keys
    }

    pub fn render(&self, rect: Rect, c: &mut Canvas) {
        let tabs = tabs();
        let (bar, body) = rect.split_top(2);
        self.tabs.render(&tabs, bar, c);
        let lines = page(self.tabs.active);
        let shown = lines.len();
        for (i, (line, role)) in lines.into_iter().enumerate() {
            draw::text(c, body, 0, i + 1, line, role);
        }
        // The same bar in a narrow place, when there's room under the
        // page: those before the active one go.
        if body.h < shown + 5 {
            return;
        }
        let (narrow, _) = body.split_bottom(3);
        draw::text(c, narrow, 0, 0, "The same tabs in 28 cells:", Role::Muted);
        // A copy, so clicks still go by where the top bar was drawn.
        let small = self.tabs.clone();
        small.render(&tabs, Rect::new(narrow.x, narrow.y + 1, narrow.w.min(28), narrow.h.saturating_sub(1).min(1)), c);
    }

    pub fn key(&mut self, press: KeyPress) -> Reply {
        if press.key == Key::Char('n') {
            self.tabs.numbers = !self.tabs.numbers;
            return Reply::Done;
        }
        Reply::from(self.tabs.handle_key(press.key, tabs().len()))
    }

    pub fn mouse(&mut self, m: &Mouse) -> Reply {
        Reply::from(self.tabs.mouse(m))
    }
}
// gallery:end

impl Default for State {
    fn default() -> State {
        State::new()
    }
}
