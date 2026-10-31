//! Colour drawn in a page's cells: blocks, a see-through colour over a
//! checkerboard, gradient bars and their markers, and big letters drawn
//! in cells of a colour (a page's text has no colour of its own to give).

use fenix_ui_kit::{Canvas, Fill, Rect, Role};

use crate::colour::Colour;

pub fn fill(c: &Colour) -> Fill {
    let (r, g, b) = c.gamut_map().rgb();
    Fill::Rgb(r, g, b)
}

/// The checkerboard's light and dark squares, under a see-through colour.
const LIGHT: Colour = Colour { r: 0.9, g: 0.9, b: 0.9, a: 1.0 };
const DARK: Colour = Colour { r: 0.6, g: 0.6, b: 0.6, a: 1.0 };

/// `rect` in colour `c`; see-through, over a checkerboard of squares two
/// cells wide (a cell is about twice as tall as it's wide).
pub fn block(canvas: &mut Canvas, rect: Rect, c: &Colour) {
    let mapped = c.gamut_map();
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            let fill = if mapped.bytes()[3] == 255 {
                fill(&mapped)
            } else {
                let under = if ((x - rect.x) / 2 + (y - rect.y)).is_multiple_of(2) { LIGHT } else { DARK };
                fill(&mapped.over(&under))
            };
            canvas.fill(y, x..x + 1, fill);
        }
    }
}

/// A bar across `rect`'s first line, each cell the colour `at` gives at
/// its place from 0 to 1, a `▲` under the one at `value`.
pub fn bar(canvas: &mut Canvas, rect: Rect, value: f64, at: impl Fn(f64) -> Colour) {
    if rect.w == 0 {
        return;
    }
    let last = (rect.w - 1).max(1) as f64;
    for i in 0..rect.w {
        let c = at(i as f64 / last);
        // See-through stops are shown over the checkerboard.
        block(canvas, Rect::new(rect.x + i, rect.y, 1, 1), &c);
    }
    if rect.h > 1 {
        let x = rect.x + (value.clamp(0.0, 1.0) * last).round() as usize;
        canvas.put(x, rect.y + 1, "▲", Role::Accent);
    }
}

/// Where along a bar drawn on `rect` cell `x` is, from 0 to 1.
pub fn bar_value(rect: Rect, x: usize) -> f64 {
    let last = (rect.w.max(2) - 1) as f64;
    (x.saturating_sub(rect.x) as f64 / last).clamp(0.0, 1.0)
}

/// Letters three squares wide and five high, a square two cells wide.
fn glyph(c: char) -> [&'static str; 5] {
    match c {
        'A' => [".#.", "#.#", "###", "#.#", "#.#"],
        'a' => ["...", ".##", "#.#", "#.#", ".##"],
        'g' => [".##", "#.#", ".##", "..#", "##."],
        '1' => [".#.", "##.", ".#.", ".#.", "###"],
        _ => ["...", "...", "...", "...", "..."],
    }
}

/// How wide `text` is in big letters.
pub fn big_width(text: &str) -> usize {
    text.chars().count() * 8
}

/// `text` in big letters, in `fg`, over `bg`, at (`x`, `y`): five lines
/// high, each letter six cells wide and two apart.
pub fn big(canvas: &mut Canvas, x: usize, y: usize, text: &str, fg: &Colour, bg: &Colour) {
    let ink = fill(&fg.over(bg));
    for (k, c) in text.chars().enumerate() {
        for (row, bits) in glyph(c).iter().enumerate() {
            for (col, bit) in bits.chars().enumerate() {
                if bit == '#' {
                    let cx = x + k * 8 + col * 2;
                    canvas.fill(y + row, cx..cx + 2, ink);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_and_its_marker() {
        let mut c = Canvas::new();
        bar(&mut c, Rect::new(2, 0, 5, 2), 0.5, |t| Colour::new(t, 0.0, 0.0, 1.0));
        let bg = c.backgrounds();
        assert_eq!(bg.first(), Some(&(0, 2..3, Fill::Rgb(0, 0, 0))));
        assert_eq!(bg.last(), Some(&(0, 6..7, Fill::Rgb(255, 0, 0))));
        assert_eq!(c.line(1), "    ▲");
        assert_eq!(bar_value(Rect::new(2, 0, 5, 2), 4), 0.5);
        assert_eq!(bar_value(Rect::new(2, 0, 5, 2), 0), 0.0);
        assert_eq!(bar_value(Rect::new(2, 0, 5, 2), 9), 1.0);
    }

    #[test]
    fn see_through_over_a_checkerboard() {
        let mut c = Canvas::new();
        block(&mut c, Rect::new(0, 0, 4, 2), &Colour::BLACK.with_alpha(0.5));
        let light = fill(&Colour::BLACK.with_alpha(0.5).over(&LIGHT));
        let dark = fill(&Colour::BLACK.with_alpha(0.5).over(&DARK));
        assert_eq!(c.backgrounds(), [(0, 0..2, light), (0, 2..4, dark), (1, 0..2, dark), (1, 2..4, light)]);
        let mut c = Canvas::new();
        block(&mut c, Rect::new(0, 0, 4, 1), &Colour::WHITE);
        assert_eq!(c.backgrounds(), [(0, 0..4, Fill::Rgb(255, 255, 255))]);
    }

    #[test]
    fn big_letters() {
        let mut c = Canvas::new();
        c.fill_rect(Rect::new(0, 0, 16, 5), Fill::Rgb(255, 255, 255));
        big(&mut c, 0, 0, "Aa", &Colour::BLACK, &Colour::WHITE);
        let ink = Fill::Rgb(0, 0, 0);
        let row0: Vec<_> = c.backgrounds().into_iter().filter(|(y, _, f)| *y == 0 && *f == ink).map(|(_, r, _)| r).collect();
        assert_eq!((row0.len(), row0.first()), (1, Some(&(2..4))), "the A's point");
        assert_eq!(big_width("Aa"), 16);
    }
}
