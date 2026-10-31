//! A rectangle cut in two: a sidebar and the body, a list over its
//! details.

use crate::{Canvas, Rect, Role};

/// Which way a split cuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Side by side, a `│` between them.
    Horizontal,
    /// One over the other, a hairline between them.
    Vertical,
}

/// How big the first part is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// This many cells.
    Fixed(usize),
    /// This percentage of the whole.
    Percent(u8),
}

/// Cuts a rectangle in two, the first part `first` big, at least `min`
/// cells left for each when there's room, a divider between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split {
    pub direction: Direction,
    pub first: Size,
    pub min: usize,
    pub divider: bool,
}

impl Split {
    /// Side by side, the left `first` big.
    pub fn horizontal(first: Size) -> Split {
        Split { direction: Direction::Horizontal, first, min: 1, divider: true }
    }

    /// One over the other, the top `first` big.
    pub fn vertical(first: Size) -> Split {
        Split { direction: Direction::Vertical, first, min: 1, divider: true }
    }

    /// A sidebar `width` cells wide, the body right of it.
    pub fn sidebar(width: usize) -> Split {
        Split { min: 10, ..Split::horizontal(Size::Fixed(width)) }
    }

    pub fn min(mut self, min: usize) -> Split {
        self.min = min;
        self
    }

    pub fn without_divider(mut self) -> Split {
        self.divider = false;
        self
    }

    /// The two parts of `rect`, and the divider's (empty without one).
    pub fn areas(&self, rect: Rect) -> (Rect, Rect, Rect) {
        let whole = match self.direction {
            Direction::Horizontal => rect.w,
            Direction::Vertical => rect.h,
        };
        let gap = usize::from(self.divider).min(whole);
        let room = whole - gap;
        let want = match self.first {
            Size::Fixed(n) => n,
            Size::Percent(p) => room * usize::from(p.min(100)) / 100,
        };
        // Each keeps `min` when both can; when they can't, the first wins.
        let first = if room >= 2 * self.min { want.clamp(self.min, room - self.min) } else { want.min(room) };
        match self.direction {
            Direction::Horizontal => {
                let (a, rest) = rect.split_left(first);
                let (d, b) = rest.split_left(gap);
                (a, b, d)
            }
            Direction::Vertical => {
                let (a, rest) = rect.split_top(first);
                let (d, b) = rest.split_top(gap);
                (a, b, d)
            }
        }
    }

    /// The two parts of `rect`, its divider drawn.
    pub fn render(&self, rect: Rect, canvas: &mut Canvas) -> (Rect, Rect) {
        let (a, b, d) = self.areas(rect);
        if !d.is_empty() {
            match self.direction {
                Direction::Horizontal => canvas.vline(d.x, d.y..d.bottom(), Role::Muted),
                Direction::Vertical => canvas.rule(d.y, d.x..d.right()),
            }
        }
        (a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sidebar_and_its_body() {
        let r = Rect::new(0, 0, 40, 3);
        let mut c = Canvas::new();
        let (side, body) = Split::sidebar(12).render(r, &mut c);
        assert_eq!((side, body), (Rect::new(0, 0, 12, 3), Rect::new(13, 0, 27, 3)));
        assert_eq!(c.line(2), format!("{}│", " ".repeat(12)));
        // Too narrow for the sidebar it asked for: the body keeps its min.
        let (side, body, _) = Split::sidebar(35).areas(r);
        assert_eq!((side.w, body.w), (29, 10));
    }

    #[test]
    fn one_over_the_other() {
        let mut c = Canvas::new();
        let (top, bottom) = Split::vertical(Size::Percent(50)).render(Rect::new(2, 0, 10, 11), &mut c);
        assert_eq!((top, bottom), (Rect::new(2, 0, 10, 5), Rect::new(2, 6, 10, 5)));
        assert_eq!(c.rules(), [(5, 2..12)]);
        let (a, b, d) = Split::vertical(Size::Fixed(3)).without_divider().areas(Rect::new(0, 0, 5, 5));
        assert_eq!((a.h, b.h, d.h), (3, 2, 0));
    }
}
