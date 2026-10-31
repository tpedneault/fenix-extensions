//! A region taller than its rectangle, scrolled with the keys, and the
//! scrollbar beside it.

use std::cell::Cell;

use crate::motion::{page, step};
use crate::{Canvas, Key, Mouse, Outcome, Rect, Role};

/// A scrollbar down column `x`, lines `y .. y + h`, for `total` rows
/// with the first shown `offset`: a `┃` thumb on a `│` track. Nothing
/// when they all fit.
pub fn scrollbar(canvas: &mut Canvas, x: usize, y: usize, h: usize, total: usize, offset: usize) {
    if h == 0 || total <= h {
        return;
    }
    let thumb = ((h * h + total / 2) / total).clamp(1, h);
    let at = offset.min(total - h) * (h - thumb) / (total - h);
    for i in 0..h {
        if (at..at + thumb).contains(&i) {
            canvas.put(x, y + i, "┃", Role::Text);
        } else {
            canvas.put(x, y + i, "│", Role::Muted);
        }
    }
}

/// Where a scrolled region is: its first line shown. The region draws
/// its lines itself, lazily, through [`ScrollState::render`].
#[derive(Debug, Clone, Default)]
pub struct ScrollState {
    offset: usize,
    /// Lines shown at the last render.
    view: Cell<usize>,
    area: Cell<Rect>,
}

impl ScrollState {
    pub fn new() -> ScrollState {
        ScrollState::default()
    }

    /// The first line shown.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Scrolled to show line `line` first (as far as `len` lets it).
    pub fn scroll_to(&mut self, line: usize, len: usize) {
        self.offset = line.min(self.max_offset(len));
    }

    /// Scrolled as little as it takes to show line `line`.
    pub fn show(&mut self, line: usize, len: usize) {
        let view = self.view.get().max(1);
        self.offset = crate::motion::top_for(self.offset, line, view, len);
    }

    fn max_offset(&self, len: usize) -> usize {
        len.saturating_sub(self.view.get().max(1))
    }

    /// `j` `k` and the arrows a line, `J` `K` (`PageDown`, `PageUp`) a
    /// page, `g` `G` (`Home`, `End`) the top and bottom, of `len` lines.
    pub fn handle_key(&mut self, key: Key, len: usize) -> Outcome {
        // The offset moves over 0 ..= max, the last screen's first line.
        let Some(to) = step(key, self.offset, self.max_offset(len) + 1, page(self.view.get())) else {
            return Outcome::Ignored;
        };
        if to == self.offset {
            return Outcome::Ignored;
        }
        self.offset = to;
        Outcome::Moved
    }

    /// Draws lines `offset ..` of `len` into `rect`, each by `line`
    /// (given the line's index and its row), a scrollbar on the right
    /// when they don't all fit. Only the lines shown are drawn.
    pub fn render(&self, len: usize, rect: Rect, canvas: &mut Canvas, mut line: impl FnMut(usize, Rect, &mut Canvas)) {
        self.view.set(rect.h);
        self.area.set(rect);
        if rect.is_empty() {
            return;
        }
        let bar = len > rect.h;
        let body = if bar { rect.split_right(1).1 } else { rect };
        let offset = self.offset.min(len.saturating_sub(rect.h));
        for (i, index) in (offset..len.min(offset + rect.h)).enumerate() {
            line(index, body.row(i), canvas);
        }
        if bar {
            scrollbar(canvas, rect.right() - 1, rect.y, rect.h, len, offset);
        }
    }

    /// The mouse on a region of `len` lines, as last drawn: the wheel
    /// scrolls three lines a notch; a click or a drag on the scrollbar
    /// scrolls to there.
    pub fn mouse(&mut self, m: &Mouse, len: usize) -> Outcome {
        let rect = self.area.get();
        if !rect.contains(m.x, m.y) {
            return Outcome::Ignored;
        }
        let max = self.max_offset(len);
        let to = if let Some(lines) = m.wheel() {
            let by = lines.unsigned_abs() as usize * 3;
            if lines > 0 {
                self.offset.saturating_sub(by)
            } else {
                (self.offset + by).min(max)
            }
        } else if (m.left_clicks() > 0 || m.is_drag()) && len > rect.h && m.x == rect.right() - 1 {
            ((m.y - rect.y) * max + (rect.h - 1) / 2) / (rect.h - 1).max(1)
        } else {
            return Outcome::Ignored;
        };
        if to == self.offset {
            return Outcome::Ignored;
        }
        self.offset = to;
        Outcome::Moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(s: &ScrollState, len: usize) -> Canvas {
        let mut c = Canvas::new();
        s.render(len, Rect::new(0, 0, 8, 4), &mut c, |i, row, c| {
            c.put(row.x, row.y, &format!("line {i}"), Role::Text);
        });
        c
    }

    #[test]
    fn a_region_scrolls_with_a_scrollbar() {
        let mut s = ScrollState::new();
        let mut asked = Vec::new();
        let mut c = Canvas::new();
        s.render(100, Rect::new(0, 0, 8, 4), &mut c, |i, _, _| asked.push(i));
        assert_eq!(asked, [0, 1, 2, 3], "only the lines shown");
        assert_eq!(s.handle_key(Key::Char('k'), 100), Outcome::Ignored, "at the top");
        assert_eq!(s.handle_key(Key::Char('J'), 100), Outcome::Moved);
        assert_eq!(s.offset(), 3);
        let c = draw(&s, 100);
        assert_eq!(c.text(), "line 3 ┃\nline 4 │\nline 5 │\nline 6 │");
        s.handle_key(Key::Char('G'), 100);
        assert_eq!(s.offset(), 96);
        let c = draw(&s, 100);
        assert_eq!(c.line(3), "line 99┃");
        assert_eq!(s.handle_key(Key::Char('j'), 100), Outcome::Ignored, "at the bottom");
        s.show(10, 100);
        assert_eq!(s.offset(), 10);
        s.show(12, 100);
        assert_eq!(s.offset(), 10, "already shown");
    }

    #[test]
    fn no_scrollbar_when_it_fits() {
        let s = ScrollState::new();
        assert_eq!(draw(&s, 2).text(), "line 0\nline 1");
    }

    #[test]
    fn the_thumb_is_where_the_view_is() {
        let mut c = Canvas::new();
        scrollbar(&mut c, 0, 0, 10, 20, 10);
        assert_eq!(c.text(), "│\n│\n│\n│\n│\n┃\n┃\n┃\n┃\n┃");
    }

    #[test]
    fn the_wheel_and_the_scrollbar_scroll_it() {
        let mut s = ScrollState::new();
        draw(&s, 100);
        let wheel = |lines| Mouse { action: crate::MouseAction::Wheel(lines), ..Mouse::click(2, 1) };
        assert_eq!(s.mouse(&wheel(1), 100), Outcome::Ignored, "at the top");
        assert_eq!(s.mouse(&wheel(-2), 100), Outcome::Moved);
        assert_eq!(s.offset(), 6);
        assert_eq!(s.mouse(&Mouse::click(7, 3), 100), Outcome::Moved, "the scrollbar's foot");
        assert_eq!(s.offset(), 96);
        assert_eq!(s.mouse(&Mouse::click(7, 0), 100), Outcome::Moved);
        assert_eq!(s.offset(), 0);
        assert_eq!(s.mouse(&Mouse::click(2, 2), 100), Outcome::Ignored, "the text");
    }
}
