//! Splits: a sidebar, and over its body an editor above two panes side by
//! side, each saying how big it came out.

// gallery:begin
use fenix_ui_kit::{Canvas, Key, KeyPress, Mouse, Rect, Role, Size, Split};

use crate::draw;
use crate::sections::Reply;

pub struct State {
    /// The sidebar's width, in cells.
    pub side: usize,
    /// The top part's share of the body, in percent.
    pub top: u8,
    pub dividers: bool,
}

impl State {
    pub fn new() -> State {
        State { side: 18, top: 60, dividers: true }
    }

    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        vec![("< >", "the sidebar narrower, wider"), ("+ -", "the top taller, shorter"), ("d", "dividers"), ("r", "as they were")]
    }

    fn split(&self, split: Split) -> Split {
        if self.dividers {
            split
        } else {
            split.without_divider()
        }
    }

    /// A pane: its name, and the rectangle it got.
    fn pane(c: &mut Canvas, rect: Rect, name: &str, role: Role) {
        draw::text(c, rect, 1, 0, name, role);
        draw::text(c, rect, 1, 1, &format!("{}×{} at {},{}", rect.w, rect.h, rect.x, rect.y), Role::Muted);
    }

    pub fn render(&self, rect: Rect, c: &mut Canvas) {
        let (side, body) = self.split(Split::sidebar(self.side)).render(rect, c);
        Self::pane(c, side, "Sidebar", Role::Title);
        draw::text(c, side, 1, 3, &format!("sidebar({})", self.side), Role::Syntax("function".into()));
        let (editor, below) = self.split(Split::vertical(Size::Percent(self.top))).render(body, c);
        Self::pane(c, editor, "Editor", Role::Title);
        draw::text(c, editor, 1, 3, &format!("vertical(Percent({}))", self.top), Role::Syntax("function".into()));
        // Side by side, each kept 8 cells wide when there's room.
        let (output, problems) = self.split(Split::horizontal(Size::Percent(50)).min(8)).render(below, c);
        Self::pane(c, output, "Output", Role::Accent);
        draw::text(c, output, 1, 3, "horizontal(Percent(50)).min(8)", Role::Syntax("function".into()));
        Self::pane(c, problems, "Problems", Role::Warn);
    }

    pub fn key(&mut self, press: KeyPress) -> Reply {
        match press.key {
            Key::Char('<') => self.side = self.side.saturating_sub(2).max(4),
            Key::Char('>') => self.side = (self.side + 2).min(60),
            Key::Char('+') => self.top = (self.top + 10).min(90),
            Key::Char('-') => self.top = self.top.saturating_sub(10).max(10),
            Key::Char('d') => self.dividers = !self.dividers,
            Key::Char('r') => *self = State::new(),
            _ => return Reply::Ignored,
        }
        Reply::Done
    }

    /// Splits don't take the mouse: what's in them does.
    pub fn mouse(&mut self, _m: &Mouse) -> Reply {
        Reply::Ignored
    }
}
// gallery:end

impl Default for State {
    fn default() -> State {
        State::new()
    }
}
