//! {{name}}: a command that opens a page, and the page, built with
//! `ui-kit`. What it works out is in [`stats`], and how it's laid out in
//! [`page`]; both run on your machine under `cargo test`, with nothing of
//! Fenix's. This file is the part that talks to Fenix.

pub mod page;
pub mod stats;

use std::collections::HashMap;

use fenix_extension_api::{export, page as pages, Event, Extension, Key, Page};

use page::{Action, Counter};

/// The page's kind, and the setting it reads.
const KIND: &str = "main";
const SPEED_SETTING: &str = "{{folder}}.words_per_minute";

/// The reading speed, as the setting says.
fn speed() -> u32 {
    stats::speed(fenix_extension_api::setting(SPEED_SETTING).as_deref())
}

#[derive(Default)]
struct Tool {
    pages: HashMap<u64, Counter>,
}

impl Extension for Tool {
    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "{{folder}}.open" => {
                let page = pages::open(KIND, "{{name}}");
                self.pages.insert(page, Counter::new(speed()));
                Ok(())
            }
            _ => Err(format!("no command {id}")),
        }
    }

    fn handle(&mut self, event: Event) {
        if let Event::SettingChanged(_) = event {
            let speed = speed();
            for (&page, counter) in &mut self.pages {
                counter.speed = speed;
                pages::refresh(page);
            }
        }
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        match self.pages.get(&page) {
            Some(counter) => counter.render(width as usize, height as usize).to_page(),
            None => Page::new().line([pages::muted("Nothing here: the page is closed")]),
        }
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let Some(counter) = self.pages.get_mut(&page) else { return };
        match counter.key(key.into()) {
            Action::Nothing => {}
            Action::Redraw => pages::refresh(page),
            Action::Close => {
                self.pages.remove(&page);
                pages::close(page);
            }
        }
    }

    fn page_closed(&mut self, page: u64) {
        self.pages.remove(&page);
    }
}

export!(Tool);
