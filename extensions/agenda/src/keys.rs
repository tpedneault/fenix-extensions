//! The keys a hint names, as the key scheme in use has them.

/// The keys that run `command` now, as Fenix's key menu writes them
/// (`SPC A n`, or `C-k A n` with standard keys, or yours). When no key
/// does, `vim` -- its Vim keys -- as the scheme in use writes them.
/// Natively, where there's no host to ask, `vim`.
pub fn keys_for(command: &str, vim: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let standard = matches!(fenix_extension_api::key_scheme(), fenix_extension_api::KeyScheme::Standard);
        fenix_extension_api::key_for(command).unwrap_or_else(|| in_scheme(vim, standard))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = command;
        vim.to_string()
    }
}

/// Vim keys' `SPC …` as the key menu writes it with standard keys:
/// `C-k …`.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn in_scheme(vim: &str, standard: bool) -> String {
    match vim.strip_prefix("SPC ") {
        Some(rest) if standard => format!("C-k {rest}"),
        _ => vim.to_string(),
    }
}

/// The Jira extension's page (`jira.open`).
pub fn jira() -> String {
    keys_for("jira.open", "SPC j j")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leader_is_ctrl_k_with_standard_keys() {
        assert_eq!(in_scheme("SPC A n", false), "SPC A n");
        assert_eq!(in_scheme("SPC A n", true), "C-k A n");
        assert_eq!(in_scheme("SPC j j", true), "C-k j j");
        assert_eq!(in_scheme("C-,", true), "C-,");
    }
}
