//! The few calls the pack makes to Fenix's API itself rather than through
//! lang-kit's `Host`: its own settings, and a terminal. In a test they go to
//! the test kit's fake, as the harness installs it.

use fenix_extension_api::Command;

#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

/// One of the pack's own settings, as text.
pub fn setting(key: &str) -> Option<String> {
    api::setting(key)
}

/// A setting changed and saved (`nix.server_runner`).
pub fn set_setting(key: &str, value: &str) -> Result<(), String> {
    api::set_setting(key, Some(value))
}

/// `cmd` in a terminal beside the editor, called `title`.
pub fn terminal(cmd: &Command, title: &str) -> Result<u64, String> {
    api::host::run_in_terminal(cmd, title, false)
}
