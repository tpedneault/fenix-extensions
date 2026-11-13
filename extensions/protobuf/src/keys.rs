//! The keys a hint names, as the key scheme in use writes them.

#[cfg(target_arch = "wasm32")]
use fenix_extension_api as api;

/// The keys that run `command`, for a hint ("`SPC p h` checks it"): its
/// own, as Fenix's `key_for` writes them with the key scheme in use. When
/// `command` is `menu_key` in the pack's `SPC m` menu, which `key_for`
/// knows nothing of, its own keys if you gave it some, else `menu_key`
/// after the keys that open that menu (`SPC m`, or `C-k m` with standard
/// keys). Natively, where there's no Fenix to ask, `vim`.
pub fn keys_for(command: &str, menu_key: Option<&str>, vim: &str) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let menu = menu_key.and_then(|_| api::key_for("leader.local"));
        keys_from(api::key_for(command), menu, menu_key, vim)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = command;
        keys_from(None, None, menu_key, vim)
    }
}

/// `own`, the command's own keys, when it has some; else `menu_key` in
/// the menu opened by `menu`; else `vim`.
pub fn keys_from(own: Option<String>, menu: Option<String>, menu_key: Option<&str>, vim: &str) -> String {
    own.or_else(|| Some(format!("{} {}", menu?, menu_key?))).unwrap_or_else(|| vim.to_string())
}

/// The project doctor, Fenix's (`project.doctor`), which the pack's fix
/// rows are in.
pub fn doctor() -> String {
    keys_for("project.doctor", None, "SPC p h")
}

/// The client's page, `g` in the pack's menu.
pub fn client() -> String {
    keys_for("protobuf.client", Some("g"), "SPC m g")
}

/// The client's message when grpcurl isn't there.
pub fn no_grpcurl(doctor: &str) -> String {
    format!("grpcurl isn't installed: I installs it, or the doctor's fix row ({doctor})")
}

/// What `invoke` says with no method chosen.
pub fn no_method(client: &str) -> String {
    format!("choose a method first: {client} opens the client")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_menu_command_is_its_own_keys_else_its_key_in_the_menu_the_scheme_opens() {
        assert_eq!(keys_from(None, Some("SPC m".into()), Some("g"), "SPC m g"), "SPC m g");
        assert_eq!(keys_from(None, Some("C-k m".into()), Some("g"), "SPC m g"), "C-k m g");
        assert_eq!(keys_from(Some("F8".into()), Some("C-k m".into()), Some("g"), "SPC m g"), "F8", "yours first");
        assert_eq!(keys_from(None, None, Some("g"), "SPC m g"), "SPC m g", "nothing known: as Vim keys have it");
    }

    #[test]
    fn a_leader_command_is_the_keys_fenix_gives_else_vims() {
        assert_eq!(keys_from(Some("SPC p h".into()), None, None, "SPC p h"), "SPC p h");
        assert_eq!(keys_from(Some("C-k p h".into()), None, None, "SPC p h"), "C-k p h");
        assert_eq!(keys_from(None, Some("C-k m".into()), None, "SPC p h"), "SPC p h", "no menu key: the menu isn't it");
    }

    #[test]
    fn what_it_says_names_the_keys_of_the_scheme() {
        assert_eq!(no_grpcurl("SPC p h"), "grpcurl isn't installed: I installs it, or the doctor's fix row (SPC p h)");
        assert_eq!(no_grpcurl("C-k p h"), "grpcurl isn't installed: I installs it, or the doctor's fix row (C-k p h)");
        assert_eq!(no_method("SPC m g"), "choose a method first: SPC m g opens the client");
        assert_eq!(no_method("C-k m g"), "choose a method first: C-k m g opens the client");
        // Natively, where there's no Fenix to ask: Vim's.
        assert_eq!((doctor().as_str(), client().as_str()), ("SPC p h", "SPC m g"));
    }
}
