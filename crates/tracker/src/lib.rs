//! The tracker seam (`plans/tracker-seam.md`): what the agenda asks of a
//! tracker extension -- the issues assigned to you, these issues, what
//! can be done next with one and doing it, and a few writes -- and what
//! the tracker answers, as JSON over Fenix's services between
//! extensions.
//!
//! A tracker `<id>` serves `<id>.tracker.<call>` ([`service`]). This
//! crate is both sides' shapes ([`seam`]), the cleaning every piece of
//! untrusted text goes through on both sides ([`clean`]), the errors
//! ([`SeamError`]), and [`FakeTracker`], an in-memory tracker that
//! answers every call, for tests. No I/O, no Fenix API: a tracker's
//! extension and the agenda each wire it to their own.

pub mod clean;
mod error;
pub mod fake;
pub mod seam;

pub use error::{code, SeamError};
pub use fake::FakeTracker;
pub use seam::*;

/// The contract's major version: a request carries it, and a tracker
/// that doesn't speak it answers `unsupported`.
pub const SEAM: u32 = 1;

/// The minor version: fields and capabilities added within [`SEAM`].
pub const MINOR: u32 = 0;

/// The calls, as `<id>.tracker.<call>` ends.
pub mod call {
    pub const ABOUT: &str = "about";
    pub const ASSIGNED: &str = "assigned";
    pub const ISSUES: &str = "issues";
    pub const TRANSITIONS: &str = "transitions";
    pub const TRANSITION: &str = "transition";
    pub const UPDATE: &str = "update";
    pub const PRIORITIES: &str = "priorities";
    pub const PEOPLE: &str = "people";
    pub const COMMENT: &str = "comment";
    pub const WORKLOG: &str = "worklog";
    pub const CREATE: &str = "create";
    pub const REMEMBER: &str = "remember";

    /// Every call, the required ones first.
    pub const ALL: [&str; 12] = [ABOUT, ASSIGNED, ISSUES, TRANSITIONS, TRANSITION, UPDATE, PRIORITIES, PEOPLE, COMMENT, WORKLOG, CREATE, REMEMBER];
}

/// The capabilities `about.can` may list, each one some calls or fields
/// a tracker may leave out.
pub mod can {
    /// Issues carry `comments`.
    pub const COMMENTS: &str = "comments";
    /// `update` with `title`, `body`, `priority`, `due`.
    pub const EDIT: &str = "edit";
    /// `priorities`.
    pub const PRIORITIES: &str = "priorities";
    /// `people`, and `update` with `assignee`.
    pub const ASSIGN: &str = "assign";
    /// `comment`.
    pub const COMMENT: &str = "comment";
    /// `worklog`.
    pub const WORKLOG: &str = "worklog";
    /// `create`.
    pub const CREATE: &str = "create";
    /// `remember`.
    pub const REMEMBER: &str = "remember";

    pub const ALL: [&str; 8] = [COMMENTS, EDIT, PRIORITIES, ASSIGN, COMMENT, WORKLOG, CREATE, REMEMBER];
}

/// The columns the agenda has, and an issue's `state.column`.
pub mod column {
    pub const TODO: &str = "todo";
    pub const DOING: &str = "doing";
    pub const BLOCKED: &str = "blocked";
    pub const DONE: &str = "done";

    pub const ALL: [&str; 4] = [TODO, DOING, BLOCKED, DONE];
}

/// The agenda's four priorities, as `priority.level` names them.
pub mod level {
    pub const URGENT: &str = "urgent";
    pub const HIGH: &str = "high";
    pub const MEDIUM: &str = "medium";
    pub const LOW: &str = "low";

    pub const ALL: [&str; 4] = [URGENT, HIGH, MEDIUM, LOW];
}

/// The service tracker `id` answers `call` on: `jira.tracker.about`.
pub fn service(id: &str, call: &str) -> String {
    format!("{id}.tracker.{call}")
}

/// The call a service name is, when it's tracker `id`'s.
pub fn call_of<'a>(id: &str, service: &'a str) -> Option<&'a str> {
    service.strip_prefix(id)?.strip_prefix(".tracker.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trackers_services_start_with_its_own_name() {
        assert_eq!(service("jira", call::ABOUT), "jira.tracker.about");
        assert_eq!(service("acme.youtrack", call::ISSUES), "acme.youtrack.tracker.issues");
        assert_eq!(call_of("jira", "jira.tracker.issues"), Some("issues"));
        assert_eq!(call_of("jira", "jira.request"), None);
        assert_eq!(call_of("github-issues", "jira.tracker.issues"), None);
    }
}
