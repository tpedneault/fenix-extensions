//! Impact: for the item handed to it with `i` on any tab: everything that reaches it by links or parents, nearest first (`kiln impact ID`).
//!
//! Not built yet: a stand-in so the tab is registered, the frame complete
//! and the hand-off (`i` on any tab, [`View::hand`]) in place. E1.4
//! replaces this file (see `README.md` here and [`super::tree`], the
//! worked example).
//!
//! What it deliberately doesn't do: ask kiln anything yet.

// E1.4 replaces this.

use super::view::{Ctx, Row, View};

#[derive(Debug, Default)]
pub struct Impact {
    /// The item handed to it last.
    id: Option<String>,
}

impl View for Impact {
    fn rows(&self, _ctx: &Ctx) -> Vec<Row> {
        let of = match &self.id {
            Some(id) => format!(" of {id}"),
            None => String::new(),
        };
        vec![Row::note(format!("Impact{of}: not built yet"))]
    }

    fn hand(&mut self, id: &str) {
        self.id = Some(id.to_string());
    }
}
