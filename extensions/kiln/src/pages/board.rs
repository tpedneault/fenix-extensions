//! Board: the work items grouped under a heading per status, in the type's order, with ready, waiting on and owner; closed statuses hidden until `a`.
//!
//! Not built yet: a stand-in so the tab is registered and the frame
//! complete. E1.2 replaces this file (see `README.md` here and
//! [`super::tree`], the worked example).
//!
//! What it deliberately doesn't do: anything but say so.

// E1.2 replaces this.

use super::view::{Ctx, Row, View};

#[derive(Debug, Default)]
pub struct Board;

impl View for Board {
    fn rows(&self, _ctx: &Ctx) -> Vec<Row> {
        vec![Row::note("Board: not built yet")]
    }
}
