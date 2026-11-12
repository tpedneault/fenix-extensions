//! The keys a hint names, as the key scheme in use writes them: `SPC k k`
//! with Vim keys, `C-k k k` with standard keys (the leader is `Ctrl+K`, or
//! what `keys.leader_chord` says), a chord when Fenix or you bound one.
//!
//! The resolving is pure (`resolve`, `leader_from`) so what a hint
//! says is tested for both schemes; the host is asked only in `keys`
//! and `leader`, and natively (`cargo test`) those answer
//! Vim keys, as `key_for` can't be called there.

/// The keys that run `command` now, or `literal` -- written with Vim keys,
/// `SPC` for the leader -- when Fenix names none.
pub fn keys(command: &str, literal: &str) -> String {
    resolve(lookup(command), &leader(), literal)
}

/// `found` (what `key_for` said), else `literal` with its `SPC` as the
/// leader of the scheme in use.
pub fn resolve(found: Option<String>, leader: &str, literal: &str) -> String {
    found.unwrap_or_else(|| match literal.strip_prefix("SPC") {
        Some(rest) if leader != "SPC" && (rest.is_empty() || rest.starts_with(' ')) => format!("{leader}{rest}"),
        _ => literal.to_string(),
    })
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

/// The keys the pure parts of the extension name in what they say, as the
/// scheme in use has them. `Default` is Vim keys, as natively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hints {
    /// Go to a definition (`gd`, `F12`).
    pub definition: String,
    /// `mib.page`: the MIB page.
    pub page: String,
    /// `mib.edit_call`: the telecommand call on the line, in its form.
    pub edit_call: String,
    /// `mib.settings`: this project's MIBs.
    pub settings: String,
    /// `mib.generate`.
    pub generate: String,
    /// `settings.open`: Fenix's settings, where the project's profile is.
    pub fenix_settings: String,
    /// `ccsds.recording`: the CCSDS extension's, a recording opened.
    pub recording: String,
}

impl Default for Hints {
    fn default() -> Self {
        Hints {
            definition: "gd".into(),
            page: "SPC k k".into(),
            edit_call: "SPC k e".into(),
            settings: "SPC k ,".into(),
            generate: "SPC k g".into(),
            fenix_settings: "SPC ,".into(),
            recording: "SPC k f".into(),
        }
    }
}

impl Hints {
    /// What the keys are now: asks the host, a call for each.
    pub fn current() -> Hints {
        let d = Hints::default();
        Hints {
            definition: keys("code.goto_definition", &d.definition),
            page: keys("mib.page", &d.page),
            edit_call: keys("mib.edit_call", &d.edit_call),
            settings: keys("mib.settings", &d.settings),
            generate: keys("mib.generate", &d.generate),
            fenix_settings: keys("settings.open", &d.fenix_settings),
            recording: keys("ccsds.recording", &d.recording),
        }
    }

    /// The same, as standard keys write them with Fenix's leader (tests).
    #[cfg(test)]
    pub fn standard() -> Hints {
        Hints { definition: "F12".into(), page: "C-k k k".into(), edit_call: "C-k k e".into(), settings: "C-k k ,".into(), generate: "C-k k g".into(), fenix_settings: "C-k ,".into(), recording: "C-k k f".into() }
    }
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
    fn the_leader_is_read_off_the_keys_of_a_leader_command() {
        assert_eq!(leader_from(false, |_| Some("C-k h ?".into())), "SPC");
        assert_eq!(leader_from(true, |c| (c == "help.keys_sheet").then(|| "C-k h ?".to_string())), "C-k");
        assert_eq!(leader_from(true, |c| (c == "settings.open").then(|| "C-; ,".to_string())), "C-;");
        // A chord over the leader's keys, or nothing: Fenix's own leader.
        assert_eq!(leader_from(true, |_| Some("F1".into())), "C-k");
        assert_eq!(leader_from(true, |_| None), "C-k");
    }
}
