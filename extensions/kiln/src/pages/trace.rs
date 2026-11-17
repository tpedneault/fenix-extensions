//! Trace: per open item of the traced type, a column per coverage role: what covers it, or `gap`; `m` only gaps, `T` the next type (`kiln trace`).
//!
//! Not built yet: a stand-in so the tab is registered and the frame
//! complete. E1.4 replaces this file (see `README.md` here and
//! [`super::tree`], the worked example).
//!
//! What it deliberately doesn't do: anything but say so.

// E1.4 replaces this.

use super::view::{Ctx, Row, View};

#[derive(Debug, Default)]
pub struct Trace;

impl View for Trace {
    fn rows(&self, _ctx: &Ctx) -> Vec<Row> {
        vec![Row::note("Trace: not built yet")]
    }
}
