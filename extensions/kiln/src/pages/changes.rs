//! Changes: the items added, changed and removed since the latest tag (`kiln diff`), `b` another revision.
//!
//! Not built yet: a stand-in so the tab is registered and the frame
//! complete. E1.6 replaces this file (see `README.md` here and
//! [`super::tree`], the worked example).
//!
//! What it deliberately doesn't do: anything but say so.

// E1.6 replaces this.

use super::view::{Ctx, Row, View};

#[derive(Debug, Default)]
pub struct Changes;

impl View for Changes {
    fn rows(&self, _ctx: &Ctx) -> Vec<Row> {
        vec![Row::note("Changes: not built yet")]
    }
}
