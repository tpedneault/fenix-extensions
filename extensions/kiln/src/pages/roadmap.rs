//! Roadmap: milestones in `due` order, each with its status, due date and own progress, and the items `part-of` it under it.
//!
//! Not built yet: a stand-in so the tab is registered and the frame
//! complete. E1.3 replaces this file (see `README.md` here and
//! [`super::tree`], the worked example).
//!
//! What it deliberately doesn't do: anything but say so.

// E1.3 replaces this.

use super::view::{Ctx, Row, View};

#[derive(Debug, Default)]
pub struct Roadmap;

impl View for Roadmap {
    fn rows(&self, _ctx: &Ctx) -> Vec<Row> {
        vec![Row::note("Roadmap: not built yet")]
    }
}
