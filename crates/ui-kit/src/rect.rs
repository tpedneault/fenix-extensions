//! Rectangles of cells, and cutting them up.

/// Cells `x .. x + w` of lines `y .. y + h`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Rect {
    pub fn new(x: usize, y: usize, w: usize, h: usize) -> Rect {
        Rect { x, y, w, h }
    }

    /// The column after it.
    pub fn right(&self) -> usize {
        self.x + self.w
    }

    /// The line after it.
    pub fn bottom(&self) -> usize {
        self.y + self.h
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    pub fn contains(&self, x: usize, y: usize) -> bool {
        (self.x..self.right()).contains(&x) && (self.y..self.bottom()).contains(&y)
    }

    /// `n` cells in from each side.
    pub fn inset(&self, n: usize) -> Rect {
        let (dx, dy) = (n.min(self.w / 2), n.min(self.h / 2));
        Rect::new(self.x + dx, self.y + dy, self.w - 2 * dx, self.h - 2 * dy)
    }

    /// `n` cells in from the left and right.
    pub fn inset_x(&self, n: usize) -> Rect {
        let dx = n.min(self.w / 2);
        Rect::new(self.x + dx, self.y, self.w - 2 * dx, self.h)
    }

    /// Its line `i`, one high (empty past the bottom).
    pub fn row(&self, i: usize) -> Rect {
        Rect::new(self.x, self.y + i, self.w, usize::from(i < self.h))
    }

    /// The top `n` lines, and the rest.
    pub fn split_top(&self, n: usize) -> (Rect, Rect) {
        let n = n.min(self.h);
        (Rect::new(self.x, self.y, self.w, n), Rect::new(self.x, self.y + n, self.w, self.h - n))
    }

    /// The bottom `n` lines, and the rest above them.
    pub fn split_bottom(&self, n: usize) -> (Rect, Rect) {
        let (rest, bottom) = self.split_top(self.h.saturating_sub(n));
        (bottom, rest)
    }

    /// The left `n` columns, and the rest.
    pub fn split_left(&self, n: usize) -> (Rect, Rect) {
        let n = n.min(self.w);
        (Rect::new(self.x, self.y, n, self.h), Rect::new(self.x + n, self.y, self.w - n, self.h))
    }

    /// The right `n` columns, and the rest left of them.
    pub fn split_right(&self, n: usize) -> (Rect, Rect) {
        let (rest, right) = self.split_left(self.w.saturating_sub(n));
        (right, rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rect_is_cut_up() {
        let r = Rect::new(2, 1, 10, 5);
        assert_eq!(r.split_top(2), (Rect::new(2, 1, 10, 2), Rect::new(2, 3, 10, 3)));
        assert_eq!(r.split_bottom(1), (Rect::new(2, 5, 10, 1), Rect::new(2, 1, 10, 4)));
        assert_eq!(r.split_left(3), (Rect::new(2, 1, 3, 5), Rect::new(5, 1, 7, 5)));
        assert_eq!(r.split_right(4), (Rect::new(8, 1, 4, 5), Rect::new(2, 1, 6, 5)));
        assert_eq!(r.split_top(9).1.h, 0, "no more than it has");
        assert_eq!(r.inset(1), Rect::new(3, 2, 8, 3));
        assert_eq!(r.row(4), Rect::new(2, 5, 10, 1));
        assert!(r.row(5).is_empty());
        assert!(r.contains(11, 5) && !r.contains(12, 5));
    }
}
