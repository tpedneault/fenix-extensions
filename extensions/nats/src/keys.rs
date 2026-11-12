//! The keys a hint names, as the key scheme in use has them.

/// The keys that run `command` now, as Fenix's key menu writes them
/// (`SPC o m c`, or `C-k o m c` with standard keys, or yours). When no
/// key does, `vim` -- its Vim keys -- as the scheme in use writes them.
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

/// The keys its hints name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    /// Fenix's settings (`settings.open`), where `nats.servers` is.
    pub settings: String,
    pub open: String,
    pub connect: String,
    pub credentials: String,
    pub send: String,
    pub request: String,
}

impl Keys {
    /// As the key scheme in use has them now.
    pub fn now() -> Keys {
        Keys {
            settings: keys_for("settings.open", "SPC ,"),
            open: keys_for("nats.open", "SPC o m m"),
            connect: keys_for("nats.connect", "SPC o m c"),
            credentials: keys_for("nats.credentials", "SPC o m a"),
            send: keys_for("nats.send", "SPC o m s"),
            request: keys_for("nats.request", "SPC o m r"),
        }
    }
}

impl Default for Keys {
    fn default() -> Keys {
        Keys::now()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// What `key_for` says with standard keys and nothing of yours.
    pub fn standard() -> Keys {
        Keys { settings: "C-,".into(), open: "C-k o m m".into(), connect: "C-k o m c".into(), credentials: "C-k o m a".into(), send: "C-k o m s".into(), request: "C-k o m r".into() }
    }

    #[test]
    fn the_leader_is_ctrl_k_with_standard_keys() {
        assert_eq!(in_scheme("SPC o m c", false), "SPC o m c");
        assert_eq!(in_scheme("SPC o m c", true), "C-k o m c");
        assert_eq!(in_scheme("C-,", true), "C-,");
        assert_eq!(Keys::now().connect, "SPC o m c", "natively, Vim keys");
    }
}
