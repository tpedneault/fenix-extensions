//! The gate on writes through the seam (`plans/tracker-seam.md` §3.6).
//!
//! Fenix 1.5 doesn't tell a tracker which extension called it, so any
//! installed extension could ask it to change issues as you. Until Fenix
//! passes the caller's id, a tracker asks you once a session, before the
//! first write the seam brings it, in these words, and keeps your answer
//! for the session; its setting `<name>.seam_writes` (`ask`, `always`,
//! `never`) can say for good. It's a speed bump, not proof of who asks:
//! a write coming just after yours could be someone else's. Every
//! tracker uses this, so the question reads the same everywhere.

use crate::{code, SeamError};

/// The setting's values, the first the default.
pub const SETTING_VALUES: [&str; 3] = ["ask", "always", "never"];

/// The picker's rows.
pub const ALLOW: &str = "Allow for this session";
pub const DENY: &str = "Not now";

/// The question, for tracker `name`.
pub fn question(name: &str) -> String {
    format!("Agenda wants to change issues in {name} for you -- allow for this session?")
}

/// What to do with a write that's just come.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Do it.
    Go,
    /// Ask now ([`question`], [`ALLOW`] or [`DENY`]), and hold it until
    /// you've answered ([`Gate::answered`]).
    Ask,
    /// Already asking: hold it with the others.
    Wait,
    /// Refuse it, with this.
    Refuse(SeamError),
}

/// One tracker's gate, for one session.
#[derive(Debug, Clone, Default)]
pub struct Gate {
    allowed: bool,
    asking: bool,
}

impl Gate {
    /// A write has come, with the setting's value: what to do with it.
    pub fn check(&mut self, setting: Option<&str>, name: &str) -> Verdict {
        match setting.map(str::trim).unwrap_or("ask") {
            "always" => Verdict::Go,
            "never" => Verdict::Refuse(SeamError::new(code::REFUSED, format!("changes from the agenda are turned off in {name}'s settings"))),
            _ if self.allowed => Verdict::Go,
            _ if self.asking => Verdict::Wait,
            _ => {
                self.asking = true;
                Verdict::Ask
            }
        }
    }

    /// You answered: allowed for the session, or not this time (asked
    /// again at the next write).
    pub fn answered(&mut self, allow: bool) {
        self.asking = false;
        self.allowed = allow;
    }

    /// What a write held while asking is refused with, when you said not
    /// now.
    pub fn denied(name: &str) -> SeamError {
        SeamError::new(code::REFUSED, format!("you didn't allow the agenda to change issues in {name} -- r on the task asks again"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_asks_once_a_session_and_the_setting_overrides() {
        let mut g = Gate::default();
        assert_eq!(g.check(None, "Jira"), Verdict::Ask);
        assert_eq!(g.check(Some("ask"), "Jira"), Verdict::Wait, "the same question, not a second");
        g.answered(true);
        assert_eq!(g.check(None, "Jira"), Verdict::Go);
        let mut g = Gate::default();
        g.check(None, "Jira");
        g.answered(false);
        assert_eq!(g.check(None, "Jira"), Verdict::Ask, "asked again next time");
        assert_eq!(Gate::default().check(Some("always"), "Jira"), Verdict::Go);
        assert!(matches!(Gate::default().check(Some("never"), "Jira"), Verdict::Refuse(e) if e.code == "refused"));
        assert_eq!(question("Linear"), "Agenda wants to change issues in Linear for you -- allow for this session?");
    }
}
