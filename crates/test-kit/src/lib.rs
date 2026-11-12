//! Testing an extension in plain `cargo test`, without Fenix.
//!
//! An extension's code reaches Fenix through the API's host calls, which
//! are WebAssembly imports: built natively for a test, every one of them
//! panics (`unreachable`). So the kit gives the test a host of its own:
//!
//! - [`Fake`]: a fake host that writes down what it was asked -- pages
//!   opened and refreshed, messages and notifications, the modeline, processes,
//!   requests, timers, pickers, prompts, services, settings, problems,
//!   decorations -- and answers with what the test gave it: a process's
//!   output, an HTTP response, a service's reply, a picker's row, as the
//!   events Fenix would send.
//! - [`api`]: the extension API with the fake for its host. An extension
//!   takes it in its tests in place of `fenix_extension_api`, by a
//!   `#[cfg(test)]` on its `use`, so its packed WebAssembly is as it was.
//! - [`Harness`]: the extension driven as Fenix drives it -- commands,
//!   events, keys, the mouse -- the fake's answers given to it after each.
//! - [`render`]: a page, or a `ui-kit` canvas, as plain text; and
//!   [`assert_snapshot!`] to keep it in a file and compare.
//! - With the `lang-kit` feature, [`Fake`] is lang-kit's `Host` too, so a
//!   language pack's kit and its own code talk to the one fake.
//!
//! It's for `[dev-dependencies]`: nothing of it is in an extension's
//! WebAssembly.

pub mod api;
pub mod fake;
pub mod harness;
#[cfg(feature = "lang-kit")]
pub mod lang;
pub mod render;
pub mod snapshot;

pub use fake::{line, notification_line, Choice, Fake, Keys, OpenPage, Picker, Process, Progress, Prompt, Ran, ServiceCall, State, Timer};
pub use harness::{parse_keys, Harness};

/// The README's example, compiled.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct Readme;

/// `api::info!`: logs to the fake, at `info`.
#[doc(hidden)]
#[macro_export]
macro_rules! __test_kit_info {
    ($($arg:tt)*) => { $crate::api::host::log($crate::api::Level::Info, &format!($($arg)*)) };
}

/// `api::warn!`: logs to the fake, at `warn`.
#[doc(hidden)]
#[macro_export]
macro_rules! __test_kit_warn {
    ($($arg:tt)*) => { $crate::api::host::log($crate::api::Level::Warn, &format!($($arg)*)) };
}

/// `api::error!`: logs to the fake, at `error`.
#[doc(hidden)]
#[macro_export]
macro_rules! __test_kit_error {
    ($($arg:tt)*) => { $crate::api::host::log($crate::api::Level::Error, &format!($($arg)*)) };
}
