//! What every section draws with besides the widgets: a line of text kept
//! inside its rectangle, and the key strip.

use fenix_ui_kit::text::width;
use fenix_ui_kit::{Canvas, Rect, Role, Style};

/// `text` on line `row` of `rect`, `dx` cells in, cut with `…` to what's
/// left of the line; nothing past its bottom. The column after it.
pub fn text(c: &mut Canvas, rect: Rect, dx: usize, row: usize, text: &str, style: impl Into<Style>) -> usize {
    if row >= rect.h || dx >= rect.w {
        return rect.x + dx.min(rect.w);
    }
    c.put_fit(rect.x + dx, rect.y + row, rect.w - dx, text, style)
}

/// Where each of `keys` goes in a strip `w` wide, wrapped: its column,
/// its line from the strip's first, the keys and what they do.
pub fn key_strip<'a>(keys: &[(&'a str, &'a str)], w: usize) -> Vec<(usize, usize, &'a str, &'a str)> {
    let (mut x, mut y) = (0, 0);
    let mut out = Vec::new();
    for &(k, what) in keys {
        let len = width(k) + width(what) + 1;
        if x > 0 && x + len > w {
            (x, y) = (0, y + 1);
        }
        out.push((x, y, k, what));
        x += len + 2;
    }
    out
}

/// How many lines the strip takes, `w` wide.
pub fn strip_height(keys: &[(&str, &str)], w: usize) -> usize {
    key_strip(keys, w).last().map_or(0, |k| k.1 + 1)
}

/// The strip in `rect`: keys in the accent, what they do muted; lines
/// that don't fit left out.
pub fn draw_strip(c: &mut Canvas, keys: &[(&str, &str)], rect: Rect) {
    for (x, row, k, what) in key_strip(keys, rect.w) {
        let after = text(c, rect, x, row, k, Role::Accent);
        text(c, rect, after - rect.x + 1, row, what, Role::Muted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_stays_in_its_rectangle() {
        let mut c = Canvas::new();
        let r = Rect::new(2, 1, 6, 2);
        assert_eq!(text(&mut c, r, 1, 0, "harbour", Role::Text), 8);
        text(&mut c, r, 0, 2, "below", Role::Text);
        text(&mut c, r, 9, 1, "right", Role::Text);
        assert_eq!(c.text(), "\n   harb…");
    }

    #[test]
    fn the_strip_wraps() {
        let keys = [("hjkl", "move"), ("s", "sort"), ("c", "its code")];
        assert_eq!(strip_height(&keys, 80), 1);
        assert_eq!(strip_height(&keys, 12), 3);
        let mut c = Canvas::new();
        draw_strip(&mut c, &keys, Rect::new(0, 0, 80, 1));
        assert_eq!(c.text(), "hjkl move  s sort  c its code");
    }
}
