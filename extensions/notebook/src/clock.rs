//! The time where you are. There's no time zone inside the sandbox
//! (chrono's `Local` is UTC there), so Fenix's comes through
//! `fenix_extension_api::clock`.

use std::time::SystemTime;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};

/// Now, where you are.
pub fn now() -> DateTime<FixedOffset> {
    fenix_extension_api::clock::now()
}

/// Today, where you are.
pub fn today() -> NaiveDate {
    now().date_naive()
}

/// A file's time, where you are.
pub fn local(when: SystemTime) -> DateTime<FixedOffset> {
    fenix_extension_api::clock::local(DateTime::<Utc>::from(when))
}

/// The day a file's time falls on, where you are.
pub fn day_of(when: SystemTime) -> NaiveDate {
    local(when).date_naive()
}
