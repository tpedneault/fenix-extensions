//! The time where you are. There's no time zone inside the sandbox, so
//! Fenix's comes through `fenix_extension_api::clock`; times are kept
//! with their offset, as `agenda.json` always had them.

use chrono::{DateTime, FixedOffset, NaiveDateTime};

/// What the agenda's times are in: the offset they were taken at.
pub type Local = FixedOffset;

/// Now, where you are.
pub fn now() -> DateTime<Local> {
    fenix_extension_api::clock::now()
}

/// A wall-clock time where you are.
pub fn from_local(naive: NaiveDateTime) -> DateTime<Local> {
    fenix_extension_api::clock::from_local(naive)
}

/// The offset where you are now, for `with_timezone`.
pub fn here() -> FixedOffset {
    *now().offset()
}
