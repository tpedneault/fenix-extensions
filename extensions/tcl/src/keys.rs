//! The keys a hint names, as the key scheme in use writes them: `SPC m s`
//! with Vim keys, `C-k m s` with standard keys (the leader is `Ctrl+K`, or
//! what `keys.leader_chord` says), a chord when Fenix or you bound one.
//!
//! The resolving is pure (`resolve`, `menu`, `leader_from`) so what a hint
//! says is tested for both schemes; the host is asked only in `keys`,
//! `menu_keys` and `leader`, and natively (`cargo test`) those answer
//! Vim keys, as `key_for` can't be called there.

/// The keys that run `command` now, or `literal` -- written with Vim keys,
/// `SPC` for the leader -- when Fenix names none.
pub fn keys(command: &str, literal: &str) -> String {
    resolve(lookup(command), &leader(), literal)
}

/// The keys of a command in the buffer's `SPC m` menu, as `key` there.
/// `key_for` doesn't know a `[[local.*]]` entry (only the leader's keys
/// and the chords), so unless the command has one of those too the menu is
/// written from the leader: `SPC m T`, `C-k m T`.
pub fn menu_keys(command: &str, key: &str) -> String {
    menu(lookup(command), &leader(), key)
}

/// `found` (what `key_for` said), else `literal` with its `SPC` as the
/// leader of the scheme in use.
pub fn resolve(found: Option<String>, leader: &str, literal: &str) -> String {
    found.unwrap_or_else(|| match literal.strip_prefix("SPC") {
        Some(rest) if leader != "SPC" && (rest.is_empty() || rest.starts_with(' ')) => format!("{leader}{rest}"),
        _ => literal.to_string(),
    })
}

/// `found`, else `key` in the `SPC m` menu under `leader`.
pub fn menu(found: Option<String>, leader: &str, key: &str) -> String {
    found.unwrap_or_else(|| format!("{leader} m {key}"))
}

/// The leader as the scheme writes it: `SPC` with Vim keys; with standard
/// keys the chord Fenix's own key menu writes (`C-k`), read off a
/// leader-only command's keys -- `help.keys_sheet` is `<leader> h ?`.
pub fn leader_from(standard: bool, said: impl Fn(&str) -> Option<String>) -> String {
    if !standard {
        return "SPC".to_string();
    }
    for (command, tail) in [("help.keys_sheet", " h ?"), ("settings.open", " ,"), ("extensions.open", " h x")] {
        if let Some(leader) = said(command).as_deref().and_then(|k| k.strip_suffix(tail)) {
            if !leader.is_empty() && !leader.contains(' ') {
                return leader.to_string();
            }
        }
    }
    "C-k".to_string()
}

#[cfg(target_arch = "wasm32")]
fn lookup(command: &str) -> Option<String> {
    fenix_extension_api::key_for(command)
}

#[cfg(not(target_arch = "wasm32"))]
fn lookup(_command: &str) -> Option<String> {
    None
}

#[cfg(target_arch = "wasm32")]
fn leader() -> String {
    leader_from(matches!(fenix_extension_api::key_scheme(), fenix_extension_api::KeyScheme::Standard), lookup)
}

#[cfg(not(target_arch = "wasm32"))]
fn leader() -> String {
    "SPC".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_fenix_says_wins() {
        assert_eq!(resolve(Some("F12".into()), "C-k", "gd"), "F12");
        assert_eq!(menu(Some("C-k m T".into()), "C-k", "T"), "C-k m T");
    }

    #[test]
    fn a_literal_takes_the_leader_of_the_scheme() {
        assert_eq!(resolve(None, "SPC", "SPC k f"), "SPC k f");
        assert_eq!(resolve(None, "C-k", "SPC k f"), "C-k k f");
        assert_eq!(resolve(None, "C-k", "SPC"), "C-k");
        assert_eq!(resolve(None, "C-k", "SPCx"), "SPCx");
        assert_eq!(resolve(None, "C-k", "gd"), "gd");
    }

    #[test]
    fn the_menu_is_the_leader_then_m() {
        assert_eq!(menu(None, "SPC", "T"), "SPC m T");
        assert_eq!(menu(None, "C-k", "T"), "C-k m T");
    }

    #[test]
    fn the_leader_is_read_off_the_keys_of_a_leader_command() {
        assert_eq!(leader_from(false, |_| Some("C-k h ?".into())), "SPC");
        assert_eq!(leader_from(true, |c| (c == "help.keys_sheet").then(|| "C-k h ?".to_string())), "C-k");
        assert_eq!(leader_from(true, |c| (c == "settings.open").then(|| "C-; ,".to_string())), "C-;");
        // A chord over the leader's keys, or nothing: Fenix's own leader.
        assert_eq!(leader_from(true, |_| Some("F1".into())), "C-k");
        assert_eq!(leader_from(true, |_| None), "C-k");
    }
}
