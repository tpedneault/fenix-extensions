//! The keys what it says names, as the key scheme in use writes them:
//! `SPC h l` with Vim keys, `C-k h l` with standard keys, yours when
//! you've bound your own. Fenix says `SPC` as the leader chord in the
//! message line by itself, but not a key you rebound.

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

/// What the extensions log, where an import's notes are:
/// `extensions.output`.
pub fn log() -> String {
    of("extensions.output", "SPC h l")
}

/// What an import says after it of the `n` things it didn't bring, and
/// where they're listed.
pub fn not_brought(n: usize) -> String {
    format!(" ({n} thing{} not brought: {})", if n == 1 { "" } else { "s" }, log())
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::KeyScheme;
    use fenix_test_kit::Fake;

    #[test]
    fn an_import_names_the_logs_keys_in_use() {
        let fake = Fake::new();
        fake.key("extensions.output", "SPC h l");
        assert_eq!(not_brought(2), " (2 things not brought: SPC h l)");
        fake.key_scheme(KeyScheme::Standard);
        assert_eq!(not_brought(2), " (2 things not brought: C-k h l)");
        fake.key_in(KeyScheme::Standard, "extensions.output", "F8");
        assert_eq!(not_brought(1), " (1 thing not brought: F8)", "yours");
        fake.no_key("extensions.output");
        assert_eq!(not_brought(3), " (3 things not brought: SPC h l)", "no key: Fenix's keymap's");
    }
}
