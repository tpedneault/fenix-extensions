//! Item: for the item handed to it with `v` on any tab: what `kiln show` gives, a row per related item or code location; `v` to follow, back and forward.
//!
//! Not built yet: a stand-in so the tab is registered, the frame complete
//! and the hand-off (`v` on any tab, [`View::hand`]) in place. E1.5
//! replaces this file (see `README.md` here and [`super::tree`], the
//! worked example).
//!
//! What it deliberately doesn't do: ask kiln anything yet.

// E1.5 replaces this.

use super::view::{Ctx, Row, View};

#[derive(Debug, Default)]
pub struct ItemView {
    /// The item handed to it last.
    id: Option<String>,
}

impl View for ItemView {
    fn rows(&self, _ctx: &Ctx) -> Vec<Row> {
        let of = match &self.id {
            Some(id) => format!(" of {id}"),
            None => String::new(),
        };
        vec![Row::note(format!("Item{of}: not built yet"))]
    }

    fn hand(&mut self, id: &str) {
        self.id = Some(id.to_string());
    }
}
