//! Finding a literal again before it's written over, and the edits a
//! conversion makes: the literal at the cursor, or every one selected.

use crate::literal::{at_column, find, Literal, Scan, Target};
use crate::picker::Place;

/// A literal replaced: its line and characters, from 0, and its new text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Where `place`'s literal is in `text` now: where it was, if it's still
/// there; else the same text on its line nearest where it was. An
/// insertion point (no text) is wherever its line still reaches.
pub fn locate(text: &str, place: &Place) -> Option<(usize, usize)> {
    let line: Vec<char> = text.lines().nth(place.line).unwrap_or("").chars().collect();
    let want: Vec<char> = place.text.chars().collect();
    if want.is_empty() {
        let lines = text.lines().count() + usize::from(text.is_empty() || text.ends_with('\n'));
        let at = place.start.min(line.len());
        return (place.line < lines).then_some((at, at));
    }
    if line.get(place.start..place.end) == Some(&want[..]) {
        return Some((place.start, place.end));
    }
    (0..line.len().saturating_sub(want.len() - 1)).filter(|&i| line[i..i + want.len()] == want[..]).min_by_key(|&i| i.abs_diff(place.start)).map(|i| (i, i + want.len()))
}

/// The literals on `line` of `text`, and the line's characters.
pub fn on_line(text: &str, line: usize, scan: Scan) -> (Vec<char>, Vec<Literal>) {
    let s = text.lines().nth(line).unwrap_or("");
    (s.chars().collect(), find(s, scan))
}

/// The literal at (`line`, `col`), or the nearest on its line: where it
/// is, and how it's written.
pub fn at_cursor(text: &str, path: &str, line: usize, col: usize) -> Option<(Place, Literal)> {
    let (chars, found) = on_line(text, line, Scan::for_path(path));
    let lit = at_column(&found, col)?.clone();
    let place = Place { path: path.to_string(), line, start: lit.start, end: lit.end, text: chars[lit.start..lit.end].iter().collect() };
    Some((place, lit))
}

/// Every literal from (line, column) up to another, whole ones only.
pub fn in_range(text: &str, path: &str, from: (usize, usize), to: (usize, usize)) -> Vec<(Place, Literal)> {
    let scan = Scan::for_path(path);
    let mut out = Vec::new();
    for (i, s) in text.lines().enumerate().skip(from.0).take(to.0.saturating_sub(from.0) + 1) {
        let chars: Vec<char> = s.chars().collect();
        for lit in find(s, scan) {
            let starts_in = i > from.0 || lit.start >= from.1;
            let ends_in = i < to.0 || lit.end <= to.1;
            if starts_in && ends_in {
                out.push((Place { path: path.to_string(), line: i, start: lit.start, end: lit.end, text: chars[lit.start..lit.end].iter().collect() }, lit));
            }
        }
    }
    out
}

/// Each literal written as `target`, CSS's legacy syntax when `legacy`
/// (or the literal had it); those already written so left out.
pub fn convert(found: &[(Place, Literal)], target: Target, legacy: bool) -> Vec<Change> {
    found
        .iter()
        .filter_map(|(place, lit)| {
            let text = target.format(&lit.format, legacy).write(&lit.colour);
            (text != place.text).then_some(Change { line: place.line, start: place.start, end: place.end, text })
        })
        .collect()
}

/// The conversions offered for a literal: each target and what it'd
/// write, the literal's own kind marked.
pub fn offers(lit: &Literal, legacy: bool) -> Vec<(Target, String, bool)> {
    Target::ALL.iter().map(|&t| (t, t.format(&lit.format, legacy).write(&lit.colour), lit.format.kind() == t.name())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSS: &str = "a {\n  color: #fff;\n  background: rgb(0 0 0) linear-gradient(red, #00f);\n}\n";

    #[test]
    fn found_again_before_its_written_over() {
        let (place, _) = at_cursor(CSS, "x.css", 1, 10).unwrap();
        assert_eq!((place.line, place.start, place.end, place.text.as_str()), (1, 9, 13, "#fff"));
        assert_eq!(locate(CSS, &place), Some((9, 13)));
        // Text typed before it on its line: found where it went.
        let moved = CSS.replace("color: #fff", "color:   #fff");
        assert_eq!(locate(&moved, &place), Some((11, 15)));
        let gone = CSS.replace("#fff", "#eee");
        assert_eq!(locate(&gone, &place), None, "changed meanwhile: not written over");
        let at = Place { path: "x.css".into(), line: 3, start: 5, end: 5, text: String::new() };
        assert_eq!(locate(CSS, &at), Some((1, 1)), "an insertion point, kept to its line");
        assert_eq!(locate(CSS, &Place { line: 9, ..at }), None);
    }

    #[test]
    fn the_nearest_on_the_line() {
        let (place, lit) = at_cursor(CSS, "x.css", 2, 0).unwrap();
        assert_eq!((place.text.as_str(), lit.format.kind()), ("rgb(0 0 0)", "rgb"));
        let (place, _) = at_cursor(CSS, "x.css", 2, 60).unwrap();
        assert_eq!(place.text, "#00f");
        assert!(at_cursor(CSS, "x.css", 0, 0).is_none());
    }

    #[test]
    fn a_selection_converted_at_once() {
        let found = in_range(CSS, "x.css", (1, 0), (2, 100));
        assert_eq!(found.iter().map(|(p, _)| p.text.as_str()).collect::<Vec<_>>(), ["#fff", "rgb(0 0 0)", "red", "#00f"]);
        let changes = convert(&found, Target::Hex, false);
        assert_eq!(changes.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["#ffffff", "#000000", "#ff0000", "#0000ff"]);
        let changes = convert(&found, Target::Rgb, false);
        assert_eq!(changes.len(), 3, "the rgb() already is");
        assert_eq!(changes[0], Change { line: 1, start: 9, end: 13, text: "rgb(255 255 255)".into() });
        assert_eq!(convert(&found, Target::Hsl, true)[1].text, "hsl(0, 0%, 0%)");
        // Half a literal isn't taken.
        let found = in_range(CSS, "x.css", (1, 10), (2, 20));
        assert!(found.is_empty(), "#fff starts before, rgb() ends after: {found:?}");
    }

    #[test]
    fn what_a_literal_could_become() {
        let (_, lit) = at_cursor("x = #3b82f6", "x.rs", 0, 5).unwrap();
        let o = offers(&lit, false);
        assert_eq!(o.iter().map(|(_, s, mine)| (s.as_str(), *mine)).collect::<Vec<_>>(), [("#3b82f6", true), ("rgb(59 130 246)", false), ("hsl(217.2 91.2% 59.8%)", false), ("oklch(62.31% 0.188 259.81)", false)]);
    }
}
