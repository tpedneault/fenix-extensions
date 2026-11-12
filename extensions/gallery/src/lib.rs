//! The widget gallery, as a WebAssembly extension: one page with a
//! section for each of `ui-kit`'s widgets, working as they do in a real
//! extension, and `c` for the code that draws each.
//!
//! The page is [`gallery::Gallery`], plain state drawn into a
//! `ui_kit::Canvas`; the sections are under [`sections`], each a file
//! whose own text is what `c` shows. This file is the glue: Fenix's keys,
//! mouse, pastes and session in; the canvas, the clipboard and the code's
//! buffer out.

pub mod data;
pub mod draw;
pub mod gallery;
pub mod sections;

use std::collections::HashMap;

use fenix_extension_api::{export, page as pages, Extension, Key, KeyPress, MouseEvent, Page};

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
        Some(Effect::Copy(text)) => fenix_extension_api::copy(&text),
        // Rust, highlighted: Fenix goes by the file name's extension.
        Some(Effect::Code { title, text }) => fenix_extension_api::open_text_beside(&title, &text, Some("rs"), 0.5),
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
        g.styles.palette = fenix_extension_api::theme_palette();
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
}

export!(Extn);
