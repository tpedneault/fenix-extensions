//! The test support for the gallery's tests: a canvas as plain text --
//! the test kit's (`fenix_test_kit::render`), with what it doesn't say --
//! and what a canvas drew outside a rectangle.

#![allow(dead_code)]

use fenix_gallery::gallery::Gallery;
use fenix_gallery::sections::Id;
use fenix_ui_kit::text::char_width;
use fenix_ui_kit::{Canvas, Key, KeyPress, Rect};

/// Line `y` of `c` as cells: a wide character's second cell is empty, a
/// combining mark stays on its letter.
fn cells(c: &Canvas, y: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for ch in c.line(y).chars() {
        match char_width(ch) {
            0 => match out.iter_mut().rev().find(|s| !s.is_empty()) {
                Some(last) => last.push(ch),
                None => out.push(ch.to_string()),
            },
            2 => {
                out.push(ch.to_string());
                out.push(String::new());
            }
            _ => out.push(ch.to_string()),
        }
    }
    out
}

/// The canvas as a screen `h` lines tall: its text as the test kit
/// draws it (hairlines as `─` over blank cells, dividers as `│`, the popup
/// Fenix would draw over it under the line it's beside), then the focused
/// row, the popup's place, and whether it's typing.
pub fn screen(c: &Canvas, h: usize) -> String {
    let mut lines: Vec<String> = fenix_test_kit::render::canvas(c).lines().map(str::to_string).collect();
    lines.resize(lines.len().max(h.max(c.height())), String::new());
    let mut out = lines.join("\n");
    out.push_str("\n----\n");
    match c.focused() {
        Some((y, cols)) => out.push_str(&format!("focus: line {y}, cells {}..{}\n", cols.start, cols.end)),
        None => out.push_str("focus: none\n"),
    }
    if let Some(p) = c.popup_ref() {
        out.push_str(&format!("popup: beside line {}, from cell {}\n", p.line, p.col));
    }
    out.push_str(&format!("typing: {}\n", c.typing()));
    out
}

/// What `c` drew outside `rect`, said: text, styles, hairlines, panels,
/// the focused row, dividers, pictures and the popup's line.
pub fn outside(c: &Canvas, rect: Rect) -> Vec<String> {
    let mut bad = Vec::new();
    let within = |y: usize, cols: &std::ops::Range<usize>| cols.is_empty() || (rect.y..rect.bottom()).contains(&y) && cols.start >= rect.x && cols.end <= rect.right();
    for y in 0..c.height() {
        for (x, cell) in cells(c, y).iter().enumerate() {
            if !cell.trim().is_empty() && !rect.contains(x, y) {
                bad.push(format!("{cell:?} at cell {x}, line {y}"));
            }
        }
    }
    for (y, cols, s) in c.spans() {
        if !within(y, &cols) {
            bad.push(format!("a span {s:?} at line {y}, cells {cols:?}"));
        }
    }
    for (y, cols, f) in c.backgrounds() {
        if !within(y, &cols) {
            bad.push(format!("a background {f:?} at line {y}, cells {cols:?}"));
        }
    }
    for (y, cols) in c.rules() {
        if !within(*y, cols) {
            bad.push(format!("a hairline at line {y}, cells {cols:?}"));
        }
    }
    for (y, cols) in c.panels() {
        if !within(y, &cols) {
            bad.push(format!("a panel at line {y}, cells {cols:?}"));
        }
    }
    if let Some((y, cols)) = c.focused() {
        if !within(*y, cols) {
            bad.push(format!("the focus at line {y}, cells {cols:?}"));
        }
    }
    for (x, ys) in c.vrules() {
        if !ys.is_empty() && (!(rect.x..rect.right()).contains(x) || ys.start < rect.y || ys.end > rect.bottom()) {
            bad.push(format!("a divider down cell {x}, lines {ys:?}"));
        }
    }
    if let Some(p) = c.popup_ref() {
        if !rect.contains(p.col, p.line) {
            bad.push(format!("the popup beside line {}, cell {}", p.line, p.col));
        }
    }
    bad
}

/// `text` against the snapshot `name`, under `tests/snapshots/`; with
/// `UPDATE_SNAPSHOTS=1`, written there instead.
pub fn snapshot(name: &str, text: &str) {
    fenix_test_kit::assert_snapshot!(name, text);
}

/// Each key of `keys` pressed on `g`, a char a key.
pub fn type_keys(g: &mut Gallery, keys: &str) {
    for c in keys.chars() {
        g.key(KeyPress::from(Key::Char(c)));
    }
}

pub fn press(g: &mut Gallery, key: Key) {
    g.key(KeyPress::from(key));
}

/// A gallery showing `id`, with the theme's palette as a dark theme's
/// might be.
pub fn showing(id: Id) -> Gallery {
    let mut g = Gallery::new();
    while g.section != id {
        type_keys(&mut g, "]");
    }
    g.styles.palette = vec![(0x5e, 0x9c, 0xf5), (0xe0, 0x6c, 0x75), (0x98, 0xc3, 0x79), (0xe5, 0xc0, 0x7b), (0xc6, 0x78, 0xdd), (0x56, 0xb6, 0xc2), (0xd1, 0x9a, 0x66), (0xab, 0xb2, 0xbf)];
    g
}

/// Each section as it opens, and in the states its keys put it in: a
/// name and the gallery.
pub fn scenarios() -> Vec<(String, Gallery)> {
    let mut out: Vec<(String, Gallery)> = Id::ALL.iter().map(|&id| (id.file().trim_end_matches(".rs").to_string(), showing(id))).collect();
    let mut with = |name: &str, id: Id, keys: &str| {
        let mut g = showing(id);
        type_keys(&mut g, keys);
        out.push((name.to_string(), g));
    };
    with("table-sorted", Id::Table, "jjjlls");
    with("table-selecting", Id::Table, "jvjjl");
    with("table-scrolled", Id::Table, "G$");
    with("tree-open", Id::Tree, "+");
    with("tabs-headers", Id::Tabs, "2");
    with("field-filter", Id::Field, "/iu");
    with("field-editing", Id::Field, "e");
    with("lists-empty", Id::Lists, "x");
    with("menu-open", Id::Menu, "jjm");
    with("splits-narrow", Id::Splits, "<<<<-d");
    with("scroll-problem", Id::Scroll, "n");
    with("styles-narrow", Id::Styles, "------");
    out
}
