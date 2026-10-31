//! The "unfocused" warning (`pomodoro.unfocused`): during a work round,
//! going to a page that isn't your work -- another extension's, Fenix's
//! settings -- says so, once a round. A file, or any text (a terminal, a
//! scratch buffer), is your work; so is its own page, and the explorer.

use fenix_extension_api::Focus;

/// The page `focus` is, when it's one away from your work: its kind.
pub fn away(focus: &Focus) -> Option<&str> {
    match focus {
        Focus::Page(page) if page.mine.is_none() => Some(&page.kind),
        _ => None,
    }
}

/// Whether a round has been warned, so it's said once a round.
#[derive(Debug, Default)]
pub struct Watch {
    warned: bool,
}

impl Watch {
    /// Whether to warn: `focus` is away, in a work round that's running,
    /// not warned yet.
    pub fn moved(&mut self, focus: &Focus, working: bool) -> bool {
        if !working || self.warned || away(focus).is_none() {
            return false;
        }
        self.warned = true;
        true
    }

    /// A work round over (ended, skipped, reset): the next may warn.
    pub fn next_round(&mut self) {
        self.warned = false;
    }
}

/// What it says: `left` in round `round`, in `mm:ss`.
pub fn warning(round: u32, left: &str) -> String {
    format!("Round {round} has {left} to go -- this page isn't your work")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_extension_api::FocusedPage;

    fn page(kind: &str, mine: Option<u64>) -> Focus {
        Focus::Page(FocusedPage { kind: kind.into(), mine })
    }

    #[test]
    fn a_page_is_away_but_files_text_and_its_own_page_are_not() {
        assert_eq!(away(&page("fenix.pets/pets", None)), Some("fenix.pets/pets"));
        assert_eq!(away(&page("settings", None)), Some("settings"));
        assert_eq!(away(&page("main", Some(3))), None, "its own page");
        assert_eq!(away(&Focus::File(None)), None);
        assert_eq!(away(&Focus::File(Some("/project/src/lib.rs".into()))), None);
        assert_eq!(away(&Focus::Text(None)), None, "a terminal");
        assert_eq!(away(&Focus::None), None, "the explorer");
    }

    #[test]
    fn once_a_work_round() {
        let mut w = Watch::default();
        let settings = page("settings", None);
        assert!(!w.moved(&settings, false), "not on a break, or paused");
        assert!(w.moved(&settings, true));
        assert!(!w.moved(&Focus::File(None), true));
        assert!(!w.moved(&settings, true), "once");
        w.next_round();
        assert!(w.moved(&settings, true), "the next round may");
    }
}
