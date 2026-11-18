//! Comments: `kiln confluence comments` -- the unresolved comments under
//! each Confluence target, by the item they are about; `Enter` opens one
//! in the browser.
//!
//! Not built yet: a stand-in so the tab is registered and the frame
//! complete. E3.3 replaces this file (see `README.md` here and
//! [`super::tree`], the worked example).
//!
//! What it deliberately doesn't do: anything but say so.

// E3.3 replaces this.

use super::view::{Ctx, Row, View};

#[derive(Debug, Default)]
pub struct Comments;

impl View for Comments {
    fn rows(&self, _ctx: &Ctx) -> Vec<Row> {
        vec![Row::note("Comments: not built yet")]
    }
}
