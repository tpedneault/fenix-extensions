//! Each widget's keys and mouse, as the page passes them on.

mod support;

use fenix_gallery::gallery::{Effect, Gallery, Saved};
use fenix_gallery::sections::Id;
use fenix_ui_kit::{Button, Key, KeyPress, Mods, Mouse, MouseAction};
use support::{press, showing, type_keys};

/// The text of line `y` of the page at 100 by 30.
fn line(g: &Gallery, y: usize) -> String {
    g.render(100, 30).line(y)
}

#[test]
fn brackets_go_round_the_sections_and_the_sidebar_picks_one() {
    let mut g = Gallery::new();
    assert_eq!(g.section, Id::Table);
    type_keys(&mut g, "]]");
    assert_eq!(g.section, Id::Tabs);
    type_keys(&mut g, "[[[");
    assert_eq!(g.section, Id::Mouse, "round to the last");
    assert!(line(&g, 2).contains("Mouse  Mouse::from"), "{}", line(&g, 2));
    // The sidebar: a click on its fourth row.
    g.render(100, 30);
    g.mouse(&Mouse::click(3, 5));
    assert_eq!(g.section, Id::Field);
}

#[test]
fn c_opens_the_sections_own_code() {
    let mut g = showing(Id::Tree);
    let Some(Effect::Code { title, text }) = g.key(KeyPress::from(Key::Char('c'))) else { panic!("no code") };
    assert_eq!(title, "Gallery: Tree");
    assert!(text.starts_with("// extensions/gallery/src/sections/tree.rs"));
    assert!(text.contains("pub fn render(&self, rect: Rect, c: &mut Canvas)"));
    assert!(!text.contains("gallery:begin") && !text.contains("impl Default for State"), "only what's between the markers");
}

#[test]
fn sorting_the_table_changes_its_order() {
    let mut g = showing(Id::Table);
    let first = |g: &Gallery| g.table.shown(0).to_vec();
    assert_eq!(first(&g)[0], "Tromsø", "the file's order");
    // `l l` to High °C, `s` sorts up, again down, again back.
    type_keys(&mut g, "lls");
    let coldest: f64 = first(&g)[2].parse().unwrap();
    assert!((1..400).all(|i| g.table.shown(i)[2].parse::<f64>().unwrap() >= coldest));
    assert!(line(&g, 6).contains("High °C ↑"), "{}", line(&g, 6));
    type_keys(&mut g, "s");
    let hottest: f64 = first(&g)[2].parse().unwrap();
    assert!(hottest > coldest && (1..400).all(|i| g.table.shown(i)[2].parse::<f64>().unwrap() <= hottest));
    type_keys(&mut g, "s");
    assert_eq!(first(&g)[0], "Tromsø", "and back");
    // Text sorts as text.
    type_keys(&mut g, "hhs");
    assert_eq!(first(&g)[0], "Chennai");
}

#[test]
fn the_tables_focus_moves_selects_and_copies() {
    let mut g = showing(Id::Table);
    type_keys(&mut g, "jjl");
    assert_eq!((g.table.grid.row, g.table.grid.col), (2, 1));
    type_keys(&mut g, "vjl");
    let Some(Effect::Copy(text)) = g.key(KeyPress::from(Key::Char('y'))) else { panic!("no copy") };
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].split('\t').count(), 2, "{text}");
    assert_eq!(g.message.as_deref(), Some("Copied 2 rows by 2 columns, tab-separated"));
    press(&mut g, Key::Enter);
    assert!(g.message.as_deref().is_some_and(|m| m.starts_with("札幌 Sapporo on 2026-03-01: -5.4 to 2.2 °C")), "{:?}", g.message);
    // `G` to the last of 400, `J` a screen up and down.
    type_keys(&mut g, "G");
    assert_eq!(g.table.grid.row, 399);
}

#[test]
fn a_click_on_the_focused_columns_header_sorts_it() {
    let mut g = showing(Id::Table);
    g.render(100, 30);
    // The header is line 6; "Station" starts at cell 15.
    g.mouse(&Mouse::click(16, 6));
    assert_eq!(g.table.sort, Some((0, false)));
    g.render(100, 30);
    // Sorted, the focused row is kept in view: a click on the second
    // row shown focuses it.
    let top = g.table.grid.visible_rows().start;
    g.mouse(&Mouse::click(17, 9));
    assert_eq!(g.table.grid.row, top + 1, "a row clicked");
}

#[test]
fn opening_a_tree_node_shows_its_children() {
    let mut g = showing(Id::Tree);
    let shown = |g: &Gallery| g.render(100, 30).text();
    assert!(!shown(&g).contains("harmonics.rs"));
    // The root, src/, then tides/: `j j l` opens it.
    type_keys(&mut g, "jjl");
    assert!(shown(&g).contains("harmonics.rs"));
    type_keys(&mut g, "h");
    assert!(!shown(&g).contains("harmonics.rs"), "closed again");
    // tests/fixtures/ is read only when it's opened.
    assert!(!g.tree.loaded);
    type_keys(&mut g, "+");
    assert!(g.tree.loaded && shown(&g).contains("brest-2025.csv"));
    type_keys(&mut g, "-");
    assert_eq!(g.tree.tree.rows(&g.tree.roots()).len(), 1, "all closed");
}

#[test]
fn a_lazy_folder_is_read_as_it_opens() {
    let mut g = showing(Id::Tree);
    // Close src/, open tests/, go to its fixtures/ and open that.
    type_keys(&mut g, "jhjljl");
    assert!(g.tree.loaded);
    assert_eq!(g.message.as_deref(), Some("Read harbour-tides/tests/fixtures/: 4 files"));
}

#[test]
fn tabs_switch_by_key_and_click() {
    let mut g = showing(Id::Tabs);
    press(&mut g, Key::Tab);
    assert_eq!(g.tabs.tabs.active, 1);
    assert!(g.render(100, 30).text().contains("content-type: application/json"));
    press(&mut g, Key::BackTab);
    press(&mut g, Key::BackTab);
    assert_eq!(g.tabs.tabs.active, 4, "round to the last");
    type_keys(&mut g, "3");
    assert_eq!(g.tabs.tabs.active, 2);
    type_keys(&mut g, "n");
    assert!(!g.tabs.tabs.numbers);
    assert!(line(&g, 4).starts_with("› Tabs       │ Summary   Headers 8"), "{}", line(&g, 4));
    // "Timing" on the bar, clicked.
    let at = line(&g, 4).find("Timing").unwrap();
    let cell = line(&g, 4)[..at].chars().count();
    g.mouse(&Mouse::click(cell + 1, 4));
    assert_eq!(g.tabs.tabs.active, 3);
}

#[test]
fn typing_in_the_field_edits_it() {
    let mut g = showing(Id::Field);
    type_keys(&mut g, "e");
    assert!(g.typing());
    // While typing, `[` and `c` are the field's.
    for _ in 0..6 {
        press(&mut g, Key::Backspace);
    }
    type_keys(&mut g, "[c] tide");
    assert_eq!(g.field.name.text(), "Brest harbour[c] tide");
    assert_eq!(g.section, Id::Field);
    // Shift+Left four times selects "tide"; typing replaces it.
    for _ in 0..4 {
        g.key(KeyPress::new(Key::Left, Mods::SHIFT));
    }
    assert_eq!(g.field.name.selected_text(), Some("tide"));
    type_keys(&mut g, "gauge");
    assert_eq!(g.field.name.text(), "Brest harbour[c] gauge");
    g.key(KeyPress::new(Key::Char('z'), Mods::CTRL));
    assert_eq!(g.field.name.text(), "Brest harbour[c] tide", "undone");
    press(&mut g, Key::End);
    g.paste(" (east)");
    assert!(g.field.name.text().ends_with("tide (east)"));
    press(&mut g, Key::Enter);
    assert!(!g.typing());
    assert_eq!(g.message.as_deref(), Some("Kept the name: Brest harbour[c] tide (east)"));
    // Esc puts it back as it was.
    type_keys(&mut g, "exyz");
    press(&mut g, Key::Escape);
    assert_eq!(g.field.name.text(), "Brest harbour[c] tide (east)");
}

#[test]
fn the_filter_narrows_the_list_as_you_type() {
    let mut g = showing(Id::Field);
    type_keys(&mut g, "/ium");
    assert!(g.typing());
    let names: Vec<String> = g.field.items().iter().map(|n| n.label.clone()).collect();
    assert!(names.len() > 3 && names.iter().all(|n| n.to_lowercase().contains("ium")), "{names:?}");
    press(&mut g, Key::Enter);
    assert!(!g.typing() && g.field.filter.text() == "ium", "kept");
    assert!(line(&g, 7).contains("/ ium"), "{}", line(&g, 7));
    // A symbol finds its element.
    type_keys(&mut g, "/");
    press(&mut g, Key::Escape);
    assert_eq!(g.field.items().len(), 36, "Esc clears it");
    type_keys(&mut g, "/fe");
    assert_eq!(g.field.items().iter().map(|n| n.label.as_str()).collect::<Vec<_>>(), ["Iron"]);
    press(&mut g, Key::Escape);
    type_keys(&mut g, "/zzz");
    press(&mut g, Key::Enter);
    assert!(g.render(100, 30).text().contains("No element matches"), "the empty state");
}

#[test]
fn the_list_ticks_scrolls_and_empties() {
    let mut g = showing(Id::Lists);
    type_keys(&mut g, "jjj");
    press(&mut g, Key::Enter);
    assert!(g.lists.done[3]);
    assert_eq!(g.message.as_deref(), Some("Fix the clippy warning in tides/harmonics.rs: done"));
    type_keys(&mut g, "G");
    assert_eq!(g.lists.list.selected, 11);
    type_keys(&mut g, "x");
    assert!(g.render(100, 30).text().contains("No tasks: x brings them back"));
    type_keys(&mut g, "x");
    assert!(!g.render(100, 30).text().contains("No tasks"));
}

#[test]
fn the_menu_opens_moves_and_picks() {
    let mut g = showing(Id::Menu);
    type_keys(&mut g, "jm");
    let c = g.render(100, 30);
    let popup = c.popup_ref().expect("a popup");
    assert_eq!(popup.line, c.focused().unwrap().0, "beside the focused row");
    type_keys(&mut g, "jj");
    assert_eq!(g.menu.menu, Some(2));
    let Some(Effect::Copy(path)) = g.key(KeyPress::from(Key::Enter)) else { panic!("no copy") };
    assert_eq!(path, "README.md");
    assert_eq!(g.menu.menu, None, "closed when picked");
    // A key picks its row straight away; `m` closes it unpicked.
    type_keys(&mut g, "md");
    assert!(g.message.as_deref().is_some_and(|m| m.starts_with("Delete: README.md")));
    type_keys(&mut g, "mm");
    assert!(g.render(100, 30).popup_ref().is_none());
}

#[test]
fn splits_resize() {
    let mut g = showing(Id::Splits);
    type_keys(&mut g, ">>");
    assert_eq!(g.splits.side, 22);
    assert!(g.render(100, 30).text().contains("22×22 at 15,4"));
    type_keys(&mut g, "--d");
    assert_eq!((g.splits.top, g.splits.dividers), (40, false));
    type_keys(&mut g, "r");
    assert_eq!((g.splits.side, g.splits.top, g.splits.dividers), (18, 60, true));
}

#[test]
fn the_scrolled_region_moves_by_key_wheel_and_scrollbar() {
    let mut g = showing(Id::Scroll);
    g.render(100, 30);
    type_keys(&mut g, "jjj");
    assert_eq!(g.scroll.scroll.offset(), 3);
    assert!(line(&g, 6).trim_start_matches(|c| c != '│').contains("   4 [09:14:03]"), "{}", line(&g, 6));
    type_keys(&mut g, "G");
    assert_eq!(g.scroll.scroll.offset(), 300 - 21);
    type_keys(&mut g, "g");
    // The wheel, down a notch: three lines.
    let wheel = Mouse { action: MouseAction::Wheel(-1), ..Mouse::click(40, 10) };
    g.mouse(&wheel);
    assert_eq!(g.scroll.scroll.offset(), 3);
    // The scrollbar's foot, clicked: the end.
    g.render(100, 30);
    g.mouse(&Mouse::click(98, 26));
    assert_eq!(g.scroll.scroll.offset(), 279);
    type_keys(&mut g, "gn");
    assert!(g.message.as_deref().is_some_and(|m| m.starts_with("Line 80:")), "{:?}", g.message);
}

#[test]
fn the_text_helpers_cut_to_the_width_chosen() {
    let mut g = showing(Id::Styles);
    let fit_line = |g: &Gallery| g.render(100, 30).lines_containing("fit(s, w)");
    assert!(fit_line(&g).ends_with("Brest → 東京 → …"));
    type_keys(&mut g, "----");
    assert_eq!(g.styles.width, 8);
    assert!(fit_line(&g).ends_with("Brest →…"), "{}", fit_line(&g));
}

#[test]
fn the_mouse_pad_draws_rubs_out_and_says_what_happened() {
    let mut g = showing(Id::Mouse);
    g.render(100, 30);
    // Inside the pad's frame: line 6 is its first row.
    g.mouse(&Mouse::click(20, 7));
    g.mouse(&Mouse { action: MouseAction::Drag(Button::Left), ..Mouse::click(21, 7) });
    assert_eq!(g.mouse.marks.len(), 2);
    assert!(g.render(100, 30).line(7).contains("●●"));
    g.mouse(&Mouse { action: MouseAction::Wheel(-1), ..Mouse::click(20, 8) });
    assert_eq!(g.mouse.brush, 1);
    g.mouse(&Mouse { action: MouseAction::Press { button: Button::Right, clicks: 1 }, mods: Mods::SHIFT, ..Mouse::click(20, 7) });
    assert_eq!(g.mouse.marks.len(), 1, "rubbed out");
    assert_eq!(g.mouse.events[0], "right press at cell 20, line 7 with Shift");
    assert_eq!(g.mouse.events[1], "wheel down 1 at cell 20, line 8");
    type_keys(&mut g, "x");
    assert!(g.mouse.marks.is_empty() && g.mouse.events.is_empty());
}

#[test]
fn where_you_were_is_kept_with_the_session() {
    let mut g = showing(Id::Table);
    type_keys(&mut g, "jjlls");
    type_keys(&mut g, "]");
    type_keys(&mut g, "+");
    type_keys(&mut g, "]3]]x]]>]");
    type_keys(&mut g, "J]");
    let saved = g.saved();
    let json = serde_json::to_string(&saved).unwrap();
    let mut back = Gallery::new();
    back.styles.palette = g.styles.palette.clone();
    back.restore(&serde_json::from_str::<Saved>(&json).unwrap());
    assert_eq!(back.section, Id::Styles);
    assert_eq!(back.saved(), saved);
    assert_eq!(back.render(100, 30).text(), g.render(100, 30).text());
    // Something older or broken opens it as new.
    let mut new = Gallery::new();
    new.restore(&serde_json::from_str::<Saved>("{\"section\": \"tree\", \"unknown\": 1}").unwrap_or_default());
    assert_eq!(new.section, Id::Tree);
}

/// The page's lines that contain `what`, the first of them.
trait Lines {
    fn lines_containing(&self, what: &str) -> String;
}

impl Lines for fenix_ui_kit::Canvas {
    fn lines_containing(&self, what: &str) -> String {
        self.text().lines().find(|l| l.contains(what)).unwrap_or_default().to_string()
    }
}
