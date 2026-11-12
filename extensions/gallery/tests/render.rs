//! Every section drawn at sizes from a wide pane down to none, kept to its
//! rectangle; and each as text at 100 by 30, against its snapshot.

mod support;

use fenix_gallery::sections::Id;
use fenix_ui_kit::{Canvas, Rect};
use support::{outside, scenarios, screen, snapshot};

const SIZES: [(usize, usize); 9] = [(120, 40), (100, 30), (80, 24), (40, 12), (20, 6), (7, 3), (1, 1), (0, 0), (1, 40)];

#[test]
fn every_section_stays_in_its_rectangle_at_every_size() {
    let mut bad = Vec::new();
    for (name, g) in scenarios() {
        for (w, h) in SIZES {
            // Away from the corner, so a write left or above shows too.
            let rect = Rect::new(3, 2, w, h);
            let mut c = Canvas::new();
            g.render_section(g.section, rect, &mut c);
            for b in outside(&c, rect) {
                bad.push(format!("{name} at {w}x{h}: {b}"));
            }
        }
    }
    assert!(bad.is_empty(), "drawn outside:\n{}", bad.join("\n"));
}

#[test]
fn the_page_stays_in_its_pane_at_every_size() {
    let mut bad = Vec::new();
    for (name, g) in scenarios() {
        for (w, h) in SIZES {
            let c = g.render(w, h);
            for b in outside(&c, Rect::new(0, 0, w, h)) {
                bad.push(format!("{name} at {w}x{h}: {b}"));
            }
        }
    }
    assert!(bad.is_empty(), "drawn outside:\n{}", bad.join("\n"));
}

/// Some also at 80 by 24, where the key strip and the titles are tight.
const AT_80: [&str; 5] = ["table", "tree-open", "field-editing", "menu-open", "styles"];

#[test]
fn every_section_looks_as_it_did() {
    for (name, g) in scenarios() {
        snapshot(&name, &screen(&g.render(100, 30), 30));
        if AT_80.contains(&name.as_str()) {
            snapshot(&format!("80x24-{name}"), &screen(&g.render(80, 24), 24));
        }
    }
}

#[test]
fn the_key_strip_fits_at_80_columns_in_two_lines() {
    for (name, g) in scenarios() {
        let keys = g.keys();
        assert!(fenix_gallery::draw::strip_height(&keys, 80) <= 2, "{name}'s strip: {keys:?}");
    }
}

#[test]
fn every_section_has_a_snapshot_and_no_snapshot_is_left_over() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let names: Vec<String> = scenarios().into_iter().map(|(n, _)| n).collect();
    for id in Id::ALL {
        assert!(names.iter().any(|n| n == id.file().trim_end_matches(".rs")), "{id:?}");
    }
    if std::env::var(fenix_test_kit::snapshot::UPDATE).is_ok_and(|v| !v.is_empty() && v != "0") {
        return;
    }
    for entry in std::fs::read_dir(dir).unwrap() {
        let file = entry.unwrap().file_name().into_string().unwrap();
        let stem = file.trim_end_matches(".txt");
        let stem = stem.strip_prefix("80x24-").filter(|s| AT_80.contains(s)).unwrap_or(stem);
        assert!(names.iter().any(|n| n == stem), "tests/snapshots/{file} is no scenario's");
    }
}
