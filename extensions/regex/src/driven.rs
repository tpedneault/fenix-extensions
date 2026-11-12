//! The playground driven as Fenix drives it, on the test kit's fake host:
//! opened on the regex under the cursor, its page as it reads, its
//! matches marked in the file, a copy picked for Python, and the marks
//! following you to another file and going when it closes.

use fenix_test_kit::{assert_snapshot, Fake, Harness};

use crate::api::{Decoration, Focus, Key, Shade};
use crate::RegexExtension;

const FILE: &str = "/work/shop/src/orders.rs";
const SOURCE: &str = "let id = Regex::new(r\"(\\d{4})-(\\d+)\").unwrap();\nlet text = \"orders 2024-17 and 2025-3\";\n";

/// You're in `orders.rs` of the project `shop`, on its regex.
fn in_orders() -> Fake {
    let fake = Fake::new();
    fake.project("shop", "/work/shop").in_file(FILE, "rs", SOURCE).cursor(0, 24);
    fake
}

#[test]
fn it_opens_beside_the_file_on_the_regex_under_the_cursor() {
    let mut h: Harness<RegexExtension> = Harness::with_fake(in_orders());
    h.command("regex.open").unwrap();
    let page = h.page("regex");
    let calls = h.fake.take();
    assert_eq!(calls[..2], ["page 1 regex *regex* (beside 0.5)", "page-focus 1"]);
    assert!(calls.contains(&format!("decorations {FILE} 6")), "{calls:?}");

    h.size = (72, 28);
    assert_snapshot!("playground", h.screen(page));
    assert_snapshot!("playground-marked", h.marked(page));
}

#[test]
fn its_matches_are_marked_in_the_file_and_follow_you() {
    let fake = in_orders();
    let mut h: Harness<RegexExtension> = Harness::with_fake(fake.clone());
    h.command("regex.open").unwrap();

    // 2024-17 and 2025-3 on line 1: each group in the palette's colour,
    // faint on the theme's background, and the rest of the match on the
    // selection's.
    let marks = fake.decorations(FILE);
    let tints: Vec<((u32, u32), (u32, u32))> = marks.iter().map(|d| (d.start, d.end)).collect();
    assert_eq!(tints, [((1, 19), (1, 23)), ((1, 23), (1, 24)), ((1, 24), (1, 26)), ((1, 31), (1, 35)), ((1, 35), (1, 36)), ((1, 36), (1, 37))]);
    assert_eq!(marks[0], Decoration::tint((1, 19), (1, 23), Shade::Rgb((94, 38, 38))), "the fake's first colour, 35% over its background");
    assert_eq!(marks[1], Decoration::tint((1, 23), (1, 24), Shade::Selection));

    // Another file: these cleared, that one marked.
    let other = "/work/shop/src/notes.txt";
    fake.buffer(other, "due 2026-01\n");
    h.focus(Focus::File(Some(other.into())));
    assert!(fake.decorations(FILE).is_empty());
    assert_eq!(fake.decorations(other).len(), 3);

    // The playground closed: its marks go with it.
    h.close(h.page("regex"));
    assert!(fake.decorations(other).is_empty());
    assert!(fake.calls().contains(&format!("decorations {other} 0")), "set empty, not left");
}

#[test]
fn y_copies_the_pattern_for_the_language_picked() {
    let fake = in_orders();
    fake.on_pick("Copy", "the pattern for Python");
    let mut h: Harness<RegexExtension> = Harness::with_fake(fake.clone());
    h.command("regex.open").unwrap();
    let page = h.page("regex");
    h.key(page, Key::Char('y'));

    let picker = fake.pickers().pop().expect("a picker");
    assert_eq!(picker.title, "Copy");
    assert_eq!(picker.items.len(), 6, "six languages; no text, so no matches to copy: {:?}", picker.items);
    assert_eq!(fake.copied().as_deref(), Some(r#"r"(\d{4})-(\d+)""#));
    assert_eq!(fake.notifications(), ["copied the pattern for Python"]);

    // Nothing picked: nothing copied.
    fake.on_pick_nothing("Copy");
    h.key(page, Key::Char('y'));
    assert_eq!(fake.state().copied.len(), 1);
}

#[test]
fn typing_a_pattern_matches_again_and_marks_the_file() {
    let fake = in_orders();
    let mut h: Harness<RegexExtension> = Harness::with_fake(fake.clone());
    h.command("regex.open").unwrap();
    let page = h.page("regex");
    fake.take();

    // c on the pattern: cleared, and typed again; j to the text, e, typed.
    h.keys(page, r"c\d+<Esc>");
    assert_eq!(fake.decorations(FILE).len(), 5, r"2024, 17, 2025 and 3, and the 4 of \d{{4}} in the code, a tint each");
    h.keys(page, "jea 12 b 345<Esc>");
    let screen = h.screen(page);
    assert!(screen.contains("pattern   \\d+\n            2 matches"), "{screen}");
    assert!(h.marked(page).contains("a «bg=selection:12» b «bg=selection:345»"), "{}", h.marked(page));
    assert_eq!(fake.take().iter().filter(|c| c.starts_with("refresh 1")).count(), 18, "a refresh for each of the 18 keys");

    // k back to the pattern, d: an empty pattern marks nothing.
    h.keys(page, "kd");
    assert!(fake.decorations(FILE).is_empty());
    assert_eq!(fake.problems(FILE), [], "and sets no problems: it has none");
}
