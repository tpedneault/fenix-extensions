//! Moving through rows, as tables, lists and scrolled regions do.

use crate::Key;

/// Where a row-moving key takes `at` of `len` rows, `page` to a screen:
/// `j` `k` and the arrows a row, `J` `K` (`PageDown`, `PageUp`) a page,
/// `g` `G` (`Home`, `End`) the first and last. None when it isn't one.
pub(crate) fn step(key: Key, at: usize, len: usize, page: usize) -> Option<usize> {
    let last = len.saturating_sub(1);
    let page = page.max(1);
    Some(match key {
        Key::Down | Key::Char('j') => (at + 1).min(last),
        Key::Up | Key::Char('k') => at.saturating_sub(1),
        Key::PageDown | Key::Char('J') => (at + page).min(last),
        Key::PageUp | Key::Char('K') => at.saturating_sub(page),
        Key::Home | Key::Char('g') => 0,
        Key::End | Key::Char('G') => last,
        _ => return None,
    })
}

/// The first row shown, `view` rows at a time, for row `at` to be in
/// view, moving as little as it can from `top`.
pub(crate) fn top_for(top: usize, at: usize, view: usize, len: usize) -> usize {
    if view == 0 {
        return 0;
    }
    let mut top = top.min(len.saturating_sub(view));
    if at < top {
        top = at;
    } else if at >= top + view {
        top = at + 1 - view;
    }
    top
}

/// A screen, from how many rows were last shown (a guess before then).
pub(crate) fn page(view: usize) -> usize {
    if view == 0 {
        20
    } else {
        view.saturating_sub(1).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_moved_through_and_kept_in_view() {
        assert_eq!(step(Key::Char('j'), 4, 5, 10), Some(4), "the last stays");
        assert_eq!(step(Key::Char('J'), 0, 100, 10), Some(10));
        assert_eq!(step(Key::Char('G'), 0, 100, 10), Some(99));
        assert_eq!(step(Key::Char('x'), 0, 100, 10), None);
        assert_eq!(top_for(0, 12, 10, 100), 3);
        assert_eq!(top_for(5, 2, 10, 100), 2);
        assert_eq!(top_for(95, 99, 10, 100), 90, "no blank rows past the end");
        assert_eq!(top_for(4, 6, 10, 8), 0, "all of it fits");
    }
}
