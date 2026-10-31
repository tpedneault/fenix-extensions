//! A tab bar: "1 Packets 12   2 By APID 3", the active tab tinted.

use std::cell::{Cell, RefCell};
use std::ops::Range;

use crate::text::{fit, width};
use crate::{Canvas, Fill, Key, Mouse, Outcome, Rect, Role, Style};

/// One tab: its title, and a count or note after it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tab {
    pub title: String,
    pub badge: Option<String>,
}

impl Tab {
    pub fn new(title: impl Into<String>) -> Tab {
        Tab { title: title.into(), badge: None }
    }

    pub fn badge(mut self, badge: impl Into<String>) -> Tab {
        self.badge = Some(badge.into());
        self
    }
}

/// Which tab is active. `Tab` and `Shift+Tab` go round them; with
/// `numbers`, each is numbered and `1` to `9` go to one.
#[derive(Debug, Clone, Default)]
pub struct TabsState {
    pub active: usize,
    pub numbers: bool,
    /// The first tab drawn, when they don't all fit.
    first: Cell<usize>,
    area: Cell<Rect>,
    /// Where each tab drawn was: its index and its cells.
    drawn: RefCell<Vec<(usize, Range<usize>)>>,
}

const GAP: usize = 3;

impl TabsState {
    pub fn new() -> TabsState {
        TabsState { numbers: true, ..TabsState::default() }
    }

    /// `key` on a bar of `n` tabs.
    pub fn handle_key(&mut self, key: Key, n: usize) -> Outcome {
        if n == 0 {
            return Outcome::Ignored;
        }
        let at = self.active.min(n - 1);
        let to = match key {
            Key::Tab => (at + 1) % n,
            Key::BackTab => (at + n - 1) % n,
            Key::Char(c @ '1'..='9') if self.numbers && (c as usize - '1' as usize) < n => c as usize - '1' as usize,
            _ => return Outcome::Ignored,
        };
        if to == self.active {
            return Outcome::Ignored;
        }
        self.active = to;
        Outcome::Moved
    }

    /// What tab `i` takes on the bar.
    fn label(&self, i: usize, tab: &Tab) -> Vec<(String, Style)> {
        let mut parts = Vec::new();
        if self.numbers && i < 9 {
            parts.push((format!("{} ", i + 1), Style::new(Role::Accent)));
        }
        let title = if i == self.active { Style::new(Role::Title).bg(Fill::Panel).underline() } else { Style::new(Role::Muted) };
        parts.push((tab.title.clone(), title));
        if let Some(badge) = &tab.badge {
            parts.push((format!(" {badge}"), Style::new(Role::Muted)));
        }
        parts
    }

    /// The bar on `rect`'s first line, a hairline under it when it's two
    /// high. When the tabs don't fit, those before the active one go, a
    /// `‹` marking it.
    pub fn render(&self, tabs: &[Tab], rect: Rect, canvas: &mut Canvas) {
        self.area.set(rect);
        if rect.is_empty() {
            return;
        }
        let labels: Vec<_> = tabs.iter().enumerate().map(|(i, t)| self.label(i, t)).collect();
        let widths: Vec<usize> = labels.iter().map(|l| l.iter().map(|(s, _)| width(s)).sum()).collect();
        let active = self.active.min(tabs.len().saturating_sub(1));
        // From the first drawn before, but no later than the active tab
        // and early enough that it fits.
        let mut first = self.first.get().min(active);
        let span = |from: usize, to: usize| widths[from..=to].iter().sum::<usize>() + GAP * (to - from) + if from > 0 { 2 } else { 0 };
        while first < active && span(first, active) > rect.w {
            first += 1;
        }
        while first > 0 && span(first - 1, active) <= rect.w && span(first - 1, tabs.len() - 1) <= rect.w {
            first -= 1;
        }
        self.first.set(first);
        let mut drawn = self.drawn.borrow_mut();
        drawn.clear();
        let mut x = rect.x;
        if first > 0 {
            x = canvas.put(x, rect.y, "‹", Role::Muted) + 1;
        }
        for (i, label) in labels.iter().enumerate().skip(first) {
            if x >= rect.right() {
                break;
            }
            if i > first {
                x += GAP;
            }
            let from = x;
            for (text, style) in label {
                let room = rect.right().saturating_sub(x);
                if room == 0 {
                    break;
                }
                x = canvas.put(x, rect.y, &fit(text, room), style);
            }
            drawn.push((i, from..x));
        }
        if rect.h >= 2 {
            canvas.rule(rect.y + 1, rect.x..rect.right());
        }
    }

    /// A click at (`x`, `y`): the tab there, as last drawn, made active.
    pub fn click(&mut self, x: usize, y: usize) -> Outcome {
        if y != self.area.get().y || !self.area.get().contains(x, y) {
            return Outcome::Ignored;
        }
        let hit = self.drawn.borrow().iter().find(|(_, cols)| cols.contains(&x)).map(|(i, _)| *i);
        match hit {
            Some(i) if i != self.active => {
                self.active = i;
                Outcome::Moved
            }
            _ => Outcome::Ignored,
        }
    }

    /// The mouse on the bar: a click on a tab makes it active.
    pub fn mouse(&mut self, m: &Mouse) -> Outcome {
        if m.left_clicks() == 0 {
            return Outcome::Ignored;
        }
        self.click(m.x, m.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs() -> Vec<Tab> {
        vec![Tab::new("Packets").badge("12"), Tab::new("By APID"), Tab::new("Problems").badge("0")]
    }

    #[test]
    fn the_bar_shows_which_is_active() {
        let mut s = TabsState::new();
        let mut c = Canvas::new();
        s.render(&tabs(), Rect::new(0, 0, 60, 2), &mut c);
        assert_eq!(c.line(0), "1 Packets 12   2 By APID   3 Problems 0");
        assert_eq!(c.style_at(2, 0), Style::new(Role::Title).bg(Fill::Panel).underline());
        assert_eq!(c.style_at(17, 0), Style::new(Role::Muted));
        assert_eq!(c.panels(), [(0, 2..9)], "the active title tinted");
        assert_eq!(c.rules(), [(1, 0..60)]);
        assert_eq!(s.handle_key(Key::Tab, 3), Outcome::Moved);
        assert_eq!(s.active, 1);
        assert_eq!(s.handle_key(Key::BackTab, 3), Outcome::Moved);
        assert_eq!(s.handle_key(Key::BackTab, 3), Outcome::Moved);
        assert_eq!(s.active, 2, "round to the last");
        assert_eq!(s.handle_key(Key::Char('1'), 3), Outcome::Moved);
        assert_eq!(s.handle_key(Key::Char('1'), 3), Outcome::Ignored, "already there");
        assert_eq!(s.handle_key(Key::Char('4'), 3), Outcome::Ignored, "no fourth");
        s.numbers = false;
        assert_eq!(s.handle_key(Key::Char('2'), 3), Outcome::Ignored);
    }

    #[test]
    fn tabs_that_dont_fit_scroll_to_the_active_one() {
        let mut s = TabsState::new();
        s.active = 2;
        let mut c = Canvas::new();
        s.render(&tabs(), Rect::new(0, 0, 20, 1), &mut c);
        assert_eq!(c.line(0), "‹ 3 Problems 0");
        s.active = 0;
        let mut c = Canvas::new();
        s.render(&tabs(), Rect::new(0, 0, 20, 1), &mut c);
        assert_eq!(c.line(0), "1 Packets 12   2 By…");
    }

    #[test]
    fn a_click_picks_a_tab() {
        let mut s = TabsState::new();
        let mut c = Canvas::new();
        s.render(&tabs(), Rect::new(2, 1, 60, 2), &mut c);
        // "  1 Packets 12   2 By APID   3 Problems 0" on line 1.
        assert_eq!(s.click(20, 1), Outcome::Moved);
        assert_eq!(s.active, 1);
        assert_eq!(s.click(20, 1), Outcome::Ignored, "already active");
        assert_eq!(s.click(16, 1), Outcome::Ignored, "between tabs");
        assert_eq!(s.click(20, 2), Outcome::Ignored, "the hairline");
        assert_eq!(s.mouse(&Mouse::click(30, 1)), Outcome::Moved);
        assert_eq!(s.active, 2);
    }
}
