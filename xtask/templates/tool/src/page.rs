//! The page: a box to type in, and what the text comes to, drawn with
//! `ui-kit` so the field's caret, selection, undo and mouse are its work.

use fenix_ui_kit::{Canvas, Key, Outcome, Rect, Role, TextField};

use crate::stats::Stats;

/// What a key on the page asks of the extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Draw the page again.
    Redraw,
    /// Close it (Esc, with nothing selected).
    Close,
}

pub struct Counter {
    pub field: TextField,
    /// Words a minute, for the reading time.
    pub speed: u32,
}

impl Counter {
    pub fn new(speed: u32) -> Counter {
        Counter { field: TextField::multiline().with_placeholder("Type or paste some text"), speed }
    }

    /// The page, `width` cells by `height` lines.
    pub fn render(&self, width: usize, height: usize) -> Canvas {
        let mut c = Canvas::new();
        let w = width.max(20);
        let x = c.put(0, 0, "{{name}}", Role::Title);
        c.put_fit(x, 0, w.saturating_sub(x), "  counts what you type", Role::Muted);
        c.rule(1, 0..w);
        c.put_fit(0, 2, w, &Stats::of(self.field.text()).summary(self.speed), Role::Accent);
        // The field takes what's left, short of the line of keys below it.
        let keys_y = height.max(6) - 1;
        self.field.render(Rect::new(0, 4, w, keys_y - 4), &mut c);
        let x = c.put(0, keys_y, "Esc", Role::Accent);
        c.put_fit(x + 1, keys_y, w.saturating_sub(x + 1), "close", Role::Muted);
        c
    }

    /// A key on the page: the field has them all, and what it leaves is
    /// Esc, which closes the page.
    pub fn key(&mut self, key: Key) -> Action {
        match self.field.handle_key(key) {
            Outcome::Cancel => Action::Close,
            Outcome::Ignored => Action::Nothing,
            _ => Action::Redraw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_in(page: &mut Counter, text: &str) {
        for c in text.chars() {
            assert_eq!(page.key(if c == '\n' { Key::Enter } else { Key::Char(c) }), Action::Redraw);
        }
    }

    #[test]
    fn typing_changes_the_counts() {
        let mut page = Counter::new(200);
        assert!(page.render(80, 20).line(2).contains("0 words"), "{}", page.render(80, 20).text());
        type_in(&mut page, "two words\nand three");
        let shown = page.render(80, 20).line(2);
        assert!(shown.contains("4 words") && shown.contains("2 lines"), "{shown}");
        assert_eq!(page.key(Key::Backspace), Action::Redraw);
        assert!(page.render(80, 20).line(2).contains("18 characters"), "{}", page.render(80, 20).line(2));
    }

    #[test]
    fn escape_closes_it_and_keys_the_field_leaves_are_the_pages() {
        let mut page = Counter::new(200);
        assert_eq!(page.key(Key::Escape), Action::Close);
        assert_eq!(page.key(Key::Backspace), Action::Nothing, "nothing to delete");
    }

    #[test]
    fn it_lays_out_at_any_size() {
        let page = Counter::new(200);
        for (w, h) in [(0, 0), (10, 3), (80, 24), (300, 80)] {
            assert!(page.render(w, h).height() >= 5, "{w}x{h}");
        }
    }
}
