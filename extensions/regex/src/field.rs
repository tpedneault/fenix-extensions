//! The test text's field: several lines, a caret that moves between them,
//! and the text cut into rows of the page's width.

use fenix_extension_api::grid::edit_line;
use fenix_extension_api::Key;

/// The char each line starts at.
pub fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.chars().enumerate().filter(|(_, c)| *c == '\n').map(|(i, _)| i + 1));
    starts
}

/// The line (from 0) and column char `at` is on.
pub fn line_col(starts: &[usize], at: usize) -> (usize, usize) {
    let line = starts.partition_point(|&s| s <= at).saturating_sub(1);
    (line, at - starts[line])
}

/// `key` on a field of several lines with the caret at char `caret`:
/// Enter breaks the line, Up and Down move a line keeping the column,
/// Home and End go to the line's ends; the rest as a one-line field.
/// Whether it took the key.
pub fn edit_text(text: &mut String, caret: &mut usize, key: Key) -> bool {
    let n = text.chars().count();
    *caret = (*caret).min(n);
    let starts = line_starts(text);
    let (line, col) = line_col(&starts, *caret);
    let line_end = |l: usize| starts.get(l + 1).map_or(n, |s| s - 1);
    match key {
        Key::Enter => {
            insert(text, caret, "\n");
            true
        }
        Key::Up | Key::Down => {
            let to = if key == Key::Up { line.checked_sub(1) } else { (line + 1 < starts.len()).then_some(line + 1) };
            match to {
                Some(l) => *caret = (starts[l] + col).min(line_end(l)),
                None if key == Key::Up => *caret = 0,
                None => *caret = n,
            }
            true
        }
        Key::Home => {
            *caret = starts[line];
            true
        }
        Key::End => {
            *caret = line_end(line);
            true
        }
        key => edit_line(text, caret, key),
    }
}

/// `more` put in at the caret, the caret after it: pasted text keeps its
/// lines and tabs (`\r\n` read as `\n`), and drops other controls.
pub fn insert(text: &mut String, caret: &mut usize, more: &str) {
    let more: String = more.replace("\r\n", "\n").chars().filter(|&c| c == '\n' || c == '\t' || !c.is_control()).collect();
    let at = (*caret).min(text.chars().count());
    let byte: usize = text.chars().take(at).map(char::len_utf8).sum();
    text.insert_str(byte, &more);
    *caret = at + more.chars().count();
}

/// `text` cut into rows of at most `width` chars, each line at least one:
/// where each row starts, and how long it is (its newline not counted).
pub fn rows(text: &str, width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut at = 0;
    for line in text.split('\n') {
        let len = line.chars().count();
        let mut start = 0;
        loop {
            let take = (len - start).min(width);
            out.push((at + start, take));
            start += take;
            if start >= len {
                break;
            }
        }
        at += len + 1;
    }
    out
}

/// The row the caret at char `caret` is drawn on.
pub fn caret_row(rows: &[(usize, usize)], caret: usize) -> usize {
    // The last row starting at or before it; at the end of a full row
    // it's drawn there, past its last character.
    rows.iter().rposition(|&(start, _)| start <= caret).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_caret_moves_between_lines_keeping_its_column() {
        let mut t = "first line\nab\nthird".to_string();
        let mut c = 8;
        edit_text(&mut t, &mut c, Key::Down);
        assert_eq!(c, 13, "the end of the short line");
        edit_text(&mut t, &mut c, Key::Down);
        assert_eq!(c, 16);
        edit_text(&mut t, &mut c, Key::Home);
        assert_eq!(c, 14);
        edit_text(&mut t, &mut c, Key::Up);
        edit_text(&mut t, &mut c, Key::End);
        assert_eq!(c, 13);
        edit_text(&mut t, &mut c, Key::Enter);
        edit_text(&mut t, &mut c, Key::Char('x'));
        assert_eq!(t, "first line\nab\nx\nthird");
        edit_text(&mut t, &mut c, Key::Backspace);
        edit_text(&mut t, &mut c, Key::Backspace);
        assert_eq!(t, "first line\nab\nthird");
    }

    #[test]
    fn a_paste_keeps_its_lines() {
        let (mut t, mut c) = ("ad".to_string(), 1);
        insert(&mut t, &mut c, "b\r\nc\u{7}");
        assert_eq!((t.as_str(), c), ("ab\ncd", 4));
    }

    #[test]
    fn rows_of_the_width() {
        assert_eq!(rows("abcdef\n\nxy", 4), [(0, 4), (4, 2), (7, 0), (8, 2)]);
        let r = rows("abcd", 4);
        assert_eq!(caret_row(&r, 4), 0);
        let r = rows("abcdef", 4);
        assert_eq!(caret_row(&r, 4), 1);
        assert_eq!(line_col(&line_starts("ab\ncd"), 4), (1, 1));
    }
}
