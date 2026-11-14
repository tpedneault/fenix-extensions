//! The keys a hint names, as the key scheme in use has them.

use crate::api;

/// The keys that run `command` now, else `vim` with the leader of the
/// scheme in use (`C-k` for `SPC` with standard keys).
pub fn keys_for(command: &str, vim: &str) -> String {
    api::key_for(command).unwrap_or_else(|| match (api::key_scheme(), vim.strip_prefix("SPC ")) {
        (api::KeyScheme::Standard, Some(rest)) => format!("C-k {rest}"),
        _ => vim.to_string(),
    })
}

/// Fenix's settings page.
pub fn settings() -> String {
    keys_for("settings.open", "SPC ,")
}
