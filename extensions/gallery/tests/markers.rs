//! The code `c` shows is the code that runs: every section's file has its
//! markers, once each, round what draws it.

use std::path::Path;

use fenix_gallery::sections::{markers, Id};

#[test]
fn every_section_has_its_markers_round_its_code() {
    for id in Id::ALL {
        let source = id.source();
        let code = markers(source).unwrap_or_else(|| panic!("src/sections/{}: one `// gallery:begin` line and one `// gallery:end` after it", id.file()));
        for needed in ["pub fn render(&self, rect: Rect, c: &mut Canvas)", "pub fn key(&mut self, press: KeyPress) -> Reply", "pub fn mouse(&mut self"] {
            assert!(code.contains(needed), "src/sections/{}: {needed:?} isn't between its markers", id.file());
        }
        assert!(!id.code().contains("(its markers are missing)"));
    }
}

#[test]
fn what_is_shown_is_the_file_on_disk() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/sections");
    for id in Id::ALL {
        let on_disk = std::fs::read_to_string(dir.join(id.file())).unwrap();
        assert_eq!(on_disk, id.source(), "{}", id.file());
    }
}

#[test]
fn every_file_in_sections_is_a_section() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/sections");
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if name == "mod.rs" {
            continue;
        }
        assert!(Id::ALL.iter().any(|id| id.file() == name), "src/sections/{name} is in no Id: add it, with its markers");
    }
}
