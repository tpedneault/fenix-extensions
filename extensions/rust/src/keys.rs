//! The keys a hint names, as the key scheme in use writes them.

#[cfg(target_arch = "wasm32")]
use fenix_extension_api as api;

/// The keys that run `command`, `menu_key` in the pack's `SPC m` menu,
/// for a hint: its own keys if you gave it some (Fenix's `key_for` knows
/// the leader's keys and chords, and nothing of a `SPC m` menu), else
/// `menu_key` after the keys that open the menu (`SPC m`, or `C-k m`
/// with standard keys). Natively, where there's no Fenix to ask, `vim`.
pub fn keys_for(command: &str, menu_key: &str, vim: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        keys_from(api::key_for(command), api::key_for("leader.local"), menu_key, vim)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = command;
        keys_from(None, None, menu_key, vim)
    }
}

/// `own`, the command's own keys, when it has some; else `menu_key` in
/// the menu opened by `menu`; else `vim`.
pub fn keys_from(own: Option<String>, menu: Option<String>, menu_key: &str, vim: &str) -> String {
    own.or_else(|| menu.map(|menu| format!("{menu} {menu_key}"))).unwrap_or_else(|| vim.to_string())
}

/// The keys of `rust.doctor`: check the project again.
pub fn doctor() -> String {
    keys_for("rust.doctor", "d", "SPC m d")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_menu_command_is_its_own_keys_else_its_key_in_the_menu_the_scheme_opens() {
        assert_eq!(keys_from(None, Some("SPC m".into()), "d", "SPC m d"), "SPC m d");
        assert_eq!(keys_from(None, Some("C-k m".into()), "d", "SPC m d"), "C-k m d");
        assert_eq!(keys_from(Some("F8".into()), Some("C-k m".into()), "d", "SPC m d"), "F8", "yours first");
        assert_eq!(keys_from(None, None, "d", "SPC m d"), "SPC m d", "nothing known: as Vim keys have it");
        // Natively, where there's no Fenix to ask: Vim's.
        assert_eq!(doctor(), "SPC m d");
    }
}
