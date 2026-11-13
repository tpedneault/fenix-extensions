//! The keys what it says names, as the key scheme in use writes them,
//! and yours when you've bound your own. Fenix says `SPC` as the leader
//! chord in the message line and the modeline by itself, but not on a
//! page, nor a key you rebound.
//!
//! Its commands for a `.sql` buffer are in that buffer's `SPC m` menu,
//! which Fenix's `key_for` doesn't know: a command's own keys come
//! first (`C-Enter` runs a statement with standard keys), else its key in
//! the menu after the menu's own keys (`key_for("leader.local")`: `SPC m`,
//! or `C-k m` with standard keys, or a leader you chose), else Vim's.

// The API: in its tests, the test kit's, with a fake host that answers
// `key_for` as the test says.
#[cfg(not(test))]
use fenix_extension_api as api;
#[cfg(test)]
use fenix_test_kit::api;

/// The keys that open a `.sql` buffer's `SPC m` menu.
pub fn menu() -> String {
    api::key_for("leader.local").unwrap_or_else(|| "SPC m".to_string())
}

/// The keys that run `command`, which is `key` in a `.sql` buffer's
/// `SPC m` menu: see [`menu_keys_from`].
pub fn in_menu(command: &str, key: &str) -> String {
    menu_keys_from(api::key_for(command), api::key_for("leader.local"), key)
}

/// `command`'s own keys, when it has some, and none for a command only in
/// a menu: a chord (`C-Enter`), or yours.
pub fn own(command: &str) -> Option<String> {
    api::key_for(command)
}

/// A command's own keys when it has some (`own`); else `key` in the
/// `SPC m` menu, opened by `menu`; else as Vim keys have it.
pub fn menu_keys_from(own: Option<String>, menu: Option<String>, key: &str) -> String {
    own.or_else(|| menu.map(|menu| format!("{menu} {key}"))).unwrap_or_else(|| format!("SPC m {key}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::KeyScheme;
    use fenix_test_kit::Fake;

    #[test]
    fn a_menu_command_is_its_own_keys_else_the_menus_else_vims() {
        let fake = Fake::new();
        fake.key("leader.local", "SPC m").menu_key("database.connection", "c").menu_key("database.run", "r");
        assert_eq!((menu(), in_menu("database.connection", "c"), in_menu("database.run", "r")), ("SPC m".into(), "SPC m c".into(), "SPC m r".into()));
        // Standard keys: the menu under their leader, and run's chord.
        fake.key_scheme(KeyScheme::Standard).key_in(KeyScheme::Standard, "database.run", "C-Enter");
        assert_eq!((menu(), in_menu("database.connection", "c"), in_menu("database.run", "r")), ("C-k m".into(), "C-k m c".into(), "C-Enter".into()));
        assert_eq!(own("database.run").as_deref(), Some("C-Enter"));
        // A leader of your own.
        fake.leader_chord("C-;");
        assert_eq!(in_menu("database.connection", "c"), "C-; m c");
        // No key for the menu at all: Vim's.
        fake.no_key("leader.local").no_key("database.run");
        assert_eq!((menu(), in_menu("database.run", "r"), own("database.run")), ("SPC m".into(), "SPC m r".into(), None));
    }
}
