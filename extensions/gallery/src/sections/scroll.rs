//! A scrolled region: a build's 300 lines of output, drawn only as far as
//! they're shown.

// gallery:begin
use fenix_ui_kit::{Canvas, Key, KeyPress, Mouse, Rect, Role, ScrollState};

use crate::data::build_log;
use crate::draw;
use crate::sections::Reply;

pub struct State {
    pub scroll: ScrollState,
    lines: Vec<String>,
}

/// A line's colour, by what it says.
fn role(line: &str) -> Role {
    let said = line.split_once("] ").map_or(line, |(_, s)| s);
    if said.starts_with("error") {
        Role::Bad
    } else if said.starts_with("warning") {
        Role::Warn
    } else if line.contains("Finished") || line.ends_with(" ok") {
        Role::Good
    } else if line.contains("cargo build") {
        Role::Title
    } else {
        Role::Text
    }
}

impl State {
    pub fn new() -> State {
        State { scroll: ScrollState::new(), lines: build_log() }
    }

    pub fn count(&self) -> usize {
        self.lines.len()
    }

    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        vec![("j k", "a line"), ("J K PgUp PgDn", "a page"), ("g G", "top, bottom"), ("n", "the next problem")]
    }

    pub fn render(&self, rect: Rect, c: &mut Canvas) {
        let (head, body) = rect.split_top(2);
        let first = self.scroll.offset().min(self.count().saturating_sub(body.h));
        let last = (first + body.h).min(self.count());
        draw::text(c, head, 0, 0, &format!("cargo build, 6 runs · lines {} to {last} of {}", first + 1, self.count()), Role::Muted);
        // Asked for a line at a time: only those shown are drawn.
        self.scroll.render(self.count(), body, c, |i, row, c| {
            let x = draw::text(c, row, 0, 0, &format!("{:>4} ", i + 1), Role::Muted);
            draw::text(c, row, x - row.x, 0, &self.lines[i], role(&self.lines[i]));
        });
    }

    pub fn key(&mut self, press: KeyPress) -> Reply {
        if press.key == Key::Char('n') {
            // The next warning or error after the first line shown, wrapping.
            let from = self.scroll.offset() + 1;
            let next = (from..self.count()).chain(0..from).find(|&i| matches!(role(&self.lines[i]), Role::Warn | Role::Bad));
            if let Some(i) = next {
                self.scroll.scroll_to(i, self.count());
                return Reply::Say(format!("Line {}: {}", i + 1, self.lines[i]));
            }
        }
        Reply::from(self.scroll.handle_key(press.key, self.count()))
    }

    /// The wheel scrolls; a click or a drag on the scrollbar goes there.
    pub fn mouse(&mut self, m: &Mouse) -> Reply {
        Reply::from(self.scroll.mouse(m, self.count()))
    }
}
// gallery:end

impl Default for State {
    fn default() -> State {
        State::new()
    }
}
