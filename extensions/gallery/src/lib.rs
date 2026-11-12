//! The widget gallery, as a WebAssembly extension: one page with a
//! section for each of `ui-kit`'s widgets, working as they do in a real
//! extension, and `c` for the code that draws each.
//!
//! The page is [`gallery::Gallery`], plain state drawn into a
//! `ui_kit::Canvas`; the sections are under [`sections`], each a file
//! whose own text is what `c` shows. This file is the glue: Fenix's keys,
//! mouse, pastes and session in; the canvas, the clipboard and the code's
//! buffer out; and the keys that open it, as the scheme in use has them.

pub mod data;
pub mod draw;
pub mod gallery;
pub mod sections;

use std::collections::HashMap;

// The API: in its tests, the test kit's, with a fake host.
#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

use api::{export, page as pages, Extension, Key, KeyPress, KeyScheme, MouseEvent, Page};

use gallery::{Effect, Gallery, Saved};

/// Its one kind of page.
const KIND: &str = "gallery";

#[derive(Default)]
struct Extn {
    pages: HashMap<u64, Gallery>,
}

/// What a key or a click asked of Fenix.
fn perform(effect: Option<Effect>) {
    match effect {
        Some(Effect::Copy(text)) => api::copy(&text),
        // Rust, highlighted: Fenix goes by the file name's extension.
        Some(Effect::Code { title, text }) => api::open_text_beside(&title, &text, Some("rs"), 0.5),
        None => {}
    }
}

impl Extension for Extn {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "gallery.open" => {
                // One gallery: brought here if it's open already.
                if let Some(&page) = self.pages.keys().next() {
                    pages::focus(page);
                } else {
                    let page = pages::open(KIND, "Widget gallery");
                    self.pages.insert(page, Gallery::new());
                }
                Ok(())
            }
            _ => Err(format!("no command {id}")),
        }
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        let Some(g) = self.pages.get_mut(&page) else {
            return Page::new().line([pages::muted("Nothing here: the gallery was closed")]);
        };
        g.styles.palette = api::theme_palette();
        // Yours over Fenix's, `SPC h g` or `C-k h g`; none if you unbound it.
        g.hint = api::key_for("gallery.open");
        g.render(width as usize, height as usize).to_page()
    }

    fn page_key(&mut self, page: u64, key: Key) {
        if let Some(g) = self.pages.get_mut(&page) {
            perform(g.key(fenix_ui_kit::KeyPress::from(fenix_ui_kit::Key::from(key))));
        }
    }

    /// PageUp, PageDown, and Shift or Ctrl with a key: a field's
    /// selection and undo.
    fn page_key_press(&mut self, page: u64, key: KeyPress) {
        let (Some(g), Some(press)) = (self.pages.get_mut(&page), fenix_ui_kit::KeyPress::from_api(&key)) else { return };
        perform(g.key(press));
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        if let Some(g) = self.pages.get_mut(&page) {
            perform(g.mouse(&fenix_ui_kit::Mouse::from(&event)));
        }
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        if let Some(g) = self.pages.get_mut(&page) {
            g.paste(text);
        }
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        serde_json::to_string(&self.pages.get(&page)?.saved()).ok()
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if kind != KIND {
            return false;
        }
        let mut g = Gallery::new();
        g.restore(&serde_json::from_str::<Saved>(state).unwrap_or_default());
        self.pages.insert(page, g);
        true
    }

    fn page_closed(&mut self, page: u64) {
        self.pages.remove(&page);
    }

    /// Vim keys or standard keys now: the foot's keys are said again.
    fn key_scheme_changed(&mut self, _scheme: KeyScheme) {
        for &page in self.pages.keys() {
            pages::refresh(page);
        }
    }
}

export!(Extn);

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_test_kit::{render, Fake, Harness};

    /// The foot's second line, where the hint is when nothing's said.
    fn foot_hint(h: &mut Harness<Extn>, page: u64) -> String {
        let text = render::text(&h.render(page, 100, 30));
        let lines: Vec<&str> = text.lines().collect();
        let rule = lines.iter().rposition(|l| l.starts_with('─')).expect("the foot's hairline");
        lines[rule + 1].to_string()
    }

    #[test]
    fn the_foot_names_the_key_that_opens_it_in_the_scheme_in_use() {
        let fake = Fake::new();
        fake.key("gallery.open", "SPC h g");
        let mut h: Harness<Extn> = Harness::with_fake(fake.clone());
        h.command("gallery.open").unwrap();
        let page = h.page(KIND);
        assert_eq!(foot_hint(&mut h, page), "SPC h g opens the gallery from anywhere");

        fake.take();
        h.key_scheme(KeyScheme::Standard);
        assert_eq!(fake.take(), [format!("refresh {page}")], "told the scheme changed, it says the keys again");
        assert_eq!(foot_hint(&mut h, page), "C-k h g opens the gallery from anywhere");

        // Rebound, and unbound.
        fake.key_in(KeyScheme::Standard, "gallery.open", "F1");
        assert_eq!(foot_hint(&mut h, page), "F1 opens the gallery from anywhere");
        fake.no_key("gallery.open");
        assert_eq!(foot_hint(&mut h, page), "", "no key opens it: nothing said");
    }

    #[test]
    fn a_message_goes_before_the_hint_and_a_second_open_brings_the_first() {
        let fake = Fake::new();
        fake.key("gallery.open", "SPC h g");
        let mut h: Harness<Extn> = Harness::with_fake(fake.clone());
        h.command("gallery.open").unwrap();
        let page = h.page(KIND);
        // The table's `y` copies the focused cell, and says so.
        h.key(page, Key::Char('y'));
        assert!(fake.copied().is_some());
        assert!(!foot_hint(&mut h, page).contains("opens the gallery"), "what was said shows instead");
        h.command("gallery.open").unwrap();
        assert_eq!(fake.open_pages().len(), 1);
        assert_eq!(fake.take().last().map(String::as_str), Some(format!("page-focus {page}").as_str()));
    }
}
