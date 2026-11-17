//! Check: `kiln check`'s findings grouped by file: `12:3  K102 error  message`, `Enter` to the line.
//!
//! Not built yet: a stand-in so the tab is registered and the frame
//! complete. E1.5 replaces this file (see `README.md` here and
//! [`super::tree`], the worked example).
//!
//! What it deliberately doesn't do: anything but say so.

// E1.5 replaces this.

use super::view::{Ctx, Row, View};

#[derive(Debug, Default)]
pub struct Check;

impl View for Check {
    fn rows(&self, _ctx: &Ctx) -> Vec<Row> {
        vec![Row::note("Check: not built yet")]
    }
}
