//! Text measured in display cells: a wide character takes two, a
//! combining mark none.

use unicode_width::UnicodeWidthChar;

/// The cells `c` takes: a tab one (it's drawn as a space), other
/// controls none.
pub fn char_width(c: char) -> usize {
    if c == '\t' {
        return 1;
    }
    c.width().unwrap_or(0)
}

/// The cells `s` takes.
pub fn width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// `s` in at most `max` cells, ending in `…` when it had to be cut.
pub fn fit(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = char_width(c);
        if w + cw > max - 1 {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// `s` in at most `max` cells keeping its end, `…` marking what went off
/// the front.
pub fn fit_tail(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut kept = Vec::new();
    let mut w = 0;
    for c in s.chars().rev() {
        let cw = char_width(c);
        if w + cw > max - 1 {
            break;
        }
        kept.push(c);
        w += cw;
    }
    std::iter::once('…').chain(kept.into_iter().rev()).collect()
}

/// The cells `from .. from + w` of `s`: half a wide character at either
/// edge is a space.
pub fn slice(s: &str, from: usize, w: usize) -> String {
    let mut out = String::new();
    let mut col = 0;
    let end = from + w;
    for c in s.chars() {
        let cw = char_width(c);
        if col >= end {
            break;
        }
        if col >= from && col + cw <= end {
            out.push(c);
        } else if col + cw > from {
            // Cut by an edge: what of it shows is blank.
            let shown = (col + cw).min(end) - col.max(from);
            out.extend(std::iter::repeat_n(' ', shown));
        }
        col += cw;
    }
    out
}

/// `text` wrapped at word boundaries to lines of at most `w` cells; a
/// word longer than that (a path) is broken where it has to be.
pub fn wrap(text: &str, w: usize) -> Vec<String> {
    let w = w.max(1);
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let mut word = word.to_string();
            loop {
                let room = if line.is_empty() { w } else { w.saturating_sub(width(&line) + 1) };
                if width(&word) <= room {
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(&word);
                    break;
                }
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    continue;
                }
                // Longer than a line: as much as fits, and the rest on.
                let head = slice_chars(&word, w);
                word = word[head.len()..].to_string();
                lines.push(head);
            }
        }
        lines.push(line);
    }
    lines
}

/// The longest start of `s` that fits in `w` cells (at least a char).
fn slice_chars(s: &str, w: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = char_width(c);
        if used + cw > w && !out.is_empty() {
            break;
        }
        out.push(c);
        used += cw;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_characters_take_two_cells() {
        assert_eq!(width("abc"), 3);
        assert_eq!(width("日本"), 4);
        assert_eq!(width("e\u{301}"), 1, "a combining accent takes none");
        assert_eq!(fit("日本語です", 5), "日本…");
        assert_eq!(fit("日本語", 6), "日本語");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("abc", 0), "");
        assert_eq!(fit_tail("abcdef", 4), "…def");
        assert_eq!(fit_tail("日本語", 4), "…語");
    }

    #[test]
    fn a_slice_blanks_half_a_wide_character() {
        assert_eq!(slice("abcdef", 2, 3), "cde");
        assert_eq!(slice("日本語", 1, 4), " 本 ");
        assert_eq!(slice("ab", 5, 3), "");
    }

    #[test]
    fn text_wraps_at_words() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("a\n\nb", 4), ["a", "", "b"]);
        assert_eq!(wrap("日本語です", 4), ["日本", "語で", "す"]);
    }
}
