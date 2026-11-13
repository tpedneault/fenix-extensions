//! The keys what it says names, as the key scheme in use writes them:
//! `SPC h l` with Vim keys, `C-k h l` with standard keys, yours when
//! you've bound your own. Fenix says `SPC` as the leader chord in the
//! message line by itself, but not on a page, nor a key you rebound.

// The API: in its tests, the test kit's, with a fake host that answers
// `key_for` as the test says.
#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

/// The keys that run `command` now (`key_for`); with none, `vim`, the
/// keys Fenix's keymap gives it, as Vim keys write them.
pub fn of(command: &str, vim: &str) -> String {
    api::key_for(command).unwrap_or_else(|| vim.to_string())
}

/// What the extensions log, where a kernel's last words are:
/// `extensions.output`.
pub fn log() -> String {
    of("extensions.output", "SPC h l")
}

/// Fenix's settings, where `jupyter.python` is: `settings.open`.
pub fn settings() -> String {
    of("settings.open", "SPC ,")
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::KeyScheme;
    use fenix_test_kit::Fake;

    #[test]
    fn keys_are_the_scheme_in_uses_and_vims_with_none() {
        let fake = Fake::new();
        fake.key("extensions.output", "SPC h l").key("settings.open", "SPC ,");
        assert_eq!((log(), settings()), ("SPC h l".to_string(), "SPC ,".to_string()));
        fake.key_scheme(KeyScheme::Standard);
        assert_eq!((log(), settings()), ("C-k h l".to_string(), "C-k ,".to_string()));
        fake.key_in(KeyScheme::Standard, "settings.open", "C-,");
        assert_eq!(settings(), "C-,", "yours");
        fake.no_key("extensions.output").no_key("settings.open");
        assert_eq!((log(), settings()), ("SPC h l".to_string(), "SPC ,".to_string()), "no key: Fenix's keymap's");
    }
}
