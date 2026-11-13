//! The keys what it says names, as the key scheme in use writes them:
//! `SPC p e m` with Vim keys, `C-k p e m` with standard keys, yours when
//! you've bound your own. Fenix says `SPC` as the leader chord in the
//! message line by itself, but not in a problem's text, nor a key you
//! rebound.

// The API: in its tests, the test kit's, with a fake host that answers
// `key_for` as the test says.
#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

/// The keys that run `command` now (`key_for`); with none, `vim`, the
/// keys its keymap gives it, as Vim keys write them.
pub fn of(command: &str, vim: &str) -> String {
    api::key_for(command).unwrap_or_else(|| vim.to_string())
}

/// What adds the keys the example has: `dotenv.add_missing`.
pub fn add_missing() -> String {
    of("dotenv.add_missing", "SPC p e m")
}

/// What stops Git tracking env files: `dotenv.untrack`.
pub fn untrack() -> String {
    of("dotenv.untrack", "SPC p e u")
}

/// What adds what Git would commit to `.gitignore`: `dotenv.gitignore`.
pub fn gitignore() -> String {
    of("dotenv.gitignore", "SPC p e i")
}

/// Fenix's task picker: `task.run`.
pub fn tasks() -> String {
    of("task.run", "SPC p t")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_test_kit::Fake;
    use api::KeyScheme;

    #[test]
    fn keys_are_the_scheme_in_uses_and_vims_with_none() {
        let fake = Fake::new();
        fake.key("dotenv.add_missing", "SPC p e m").key("task.run", "SPC p t");
        assert_eq!((add_missing(), tasks()), ("SPC p e m".to_string(), "SPC p t".to_string()));
        fake.key_scheme(KeyScheme::Standard);
        assert_eq!((add_missing(), tasks()), ("C-k p e m".to_string(), "C-k p t".to_string()));
        // Yours, a chord of your own.
        fake.key_in(KeyScheme::Standard, "dotenv.add_missing", "F7");
        assert_eq!(add_missing(), "F7");
        // No key runs it: its keymap's, as Vim keys write them.
        fake.no_key("dotenv.add_missing").no_key("task.run");
        assert_eq!((add_missing(), tasks(), untrack(), gitignore()), ("SPC p e m".into(), "SPC p t".into(), "SPC p e u".into(), "SPC p e i".into()));
    }
}
