//! A buffer's colour literals, for its swatches: every line's, or, in a
//! long file, those around where you are; no more than a cap.

use crate::colour::Colour;
use crate::literal::{find, Literal, Scan};

/// A literal and its line, from 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub line: usize,
    pub literal: Literal,
}

/// How much of a buffer is looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most literals found; the rest go without.
    pub most: usize,
    /// Past this many lines, only those this near the cursor's line.
    pub window: usize,
    /// A line longer than this (minified, data) is passed over.
    pub longest_line: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { most: 2000, window: 5000, longest_line: 100_000 }
    }
}

/// The literals in `text`, a buffer's, by line; around line `near` when
/// it's long.
pub fn literals(text: &str, scan: Scan, near: usize, limits: Limits) -> Vec<Found> {
    let total = text.lines().count();
    let range = if total > limits.window * 2 { near.saturating_sub(limits.window)..near + limits.window } else { 0..total };
    let mut out = Vec::new();
    for (line, s) in text.lines().enumerate().skip(range.start).take(range.len()) {
        if s.len() > limits.longest_line * 4 || s.chars().count() > limits.longest_line {
            continue;
        }
        // Quick: no `#`, `(` or letter after a colon, no colour.
        if !s.contains('#') && !s.contains('(') && !(scan.style && (s.contains(':') || s.contains('='))) && !(scan.theme && s.contains('{')) {
            continue;
        }
        for literal in find(s, scan) {
            out.push(Found { line, literal });
            if out.len() >= limits.most {
                return out;
            }
        }
    }
    out
}

/// The swatch's colour for `c`: over the editor's background when it's
/// see-through, as it'd show there.
pub fn swatch(c: &Colour, background: &Colour) -> (u8, u8, u8) {
    c.gamut_map().over(background).rgb()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffers_literals() {
        let text = "a { color: #fff; }\n\nb { color: rgb(0 0 0); background: red }\n// #123\n";
        let found = literals(text, Scan::for_path("x.css"), 0, Limits::default());
        let at: Vec<(usize, usize)> = found.iter().map(|f| (f.line, f.literal.start)).collect();
        assert_eq!(at, [(0, 11), (2, 11), (2, 35), (3, 3)]);
        let found = literals(text, Scan::for_path("x.rs"), 0, Limits::default());
        assert_eq!(found.len(), 2, "no names or #123 in code");
    }

    #[test]
    fn a_long_buffer_only_near_the_cursor_and_capped() {
        let text: String = (0..100).map(|i| format!("x{i} = #ff00{:02x}\n", i)).collect();
        let limits = Limits { most: 1000, window: 10, longest_line: 1000 };
        let found = literals(&text, Scan::default(), 50, limits);
        assert_eq!((found.first().map(|f| f.line), found.last().map(|f| f.line)), (Some(40), Some(59)));
        let found = literals(&text, Scan::default(), 50, Limits { most: 5, ..limits });
        assert_eq!(found.len(), 5);
        let long = format!("{}#fff\nb = #000000\n", "x".repeat(2000));
        let found = literals(&long, Scan::default(), 0, limits);
        assert_eq!(found.iter().map(|f| f.line).collect::<Vec<_>>(), [1], "the long line passed over");
    }

    #[test]
    fn a_see_through_swatch_is_shown_over_the_background() {
        let half_red = Colour::from_u8(255, 0, 0).with_alpha(0.5);
        assert_eq!(swatch(&half_red, &Colour::WHITE), (255, 128, 128));
        assert_eq!(swatch(&half_red, &Colour::BLACK), (128, 0, 0));
        assert_eq!(swatch(&Colour::from_oklch(0.7, 0.4, 145.0, 1.0), &Colour::BLACK), Colour::from_oklch(0.7, 0.4, 145.0, 1.0).gamut_map().rgb());
    }
}
