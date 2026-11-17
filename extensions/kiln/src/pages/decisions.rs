//! Decisions: the decision log in ID order: status, then what each supersedes or is superseded by.
//!
//! Not built yet: a stand-in so the tab is registered and the frame
//! complete. E1.3 replaces this file (see `README.md` here and
//! [`super::tree`], the worked example).
//!
//! What it deliberately doesn't do: anything but say so.

// E1.3 replaces this.

use super::view::{Ctx, Row, View};

#[derive(Debug, Default)]
pub struct Decisions;

impl View for Decisions {
    fn rows(&self, _ctx: &Ctx) -> Vec<Row> {
        vec![Row::note("Decisions: not built yet")]
    }
}
