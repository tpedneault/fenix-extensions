//! The tracker seam (`plans/tracker-seam.md`): what the agenda asks of a
//! tracker extension -- the issues assigned to you, these issues, what
//! can be done next with one and doing it, and a few writes -- and what
//! the tracker answers, as JSON over Fenix's services between
//! extensions.
//!
//! A tracker serves `<its extension id>.tracker.<call>` ([`service`], the
//! one place the names are made). This crate is both sides' shapes
//! ([`seam`]), the cleaning every piece of untrusted text goes through on
//! both sides ([`clean`]), the errors ([`SeamError`]), the gate a tracker
//! puts on writes ([`gate`]), the memory of writes already done
//! ([`Dedupe`]), and [`FakeTracker`], an in-memory tracker that answers
//! every call, for tests. No I/O, no Fenix API: a tracker's extension and
//! the agenda each wire it to their own.

pub mod clean;
mod dedupe;
mod error;
pub mod fake;
pub mod gate;
pub mod seam;

pub use dedupe::Dedupe;
pub use error::{code, SeamError};
pub use fake::FakeTracker;
pub use seam::*;

/// The newest major version of the contract this crate speaks; a request
/// carries the one its caller chose.
pub const SEAM: u32 = 1;

/// Every major version this crate speaks, oldest first: what a tracker
/// built on it puts in `about.seams`.
pub const SEAMS: [u32; 1] = [1];

/// The minor version: fields and capabilities added within [`SEAM`].
pub const MINOR: u32 = 1;

/// The highest major version both sides speak, if any.
pub fn common_seam(mine: &[u32], theirs: &[u32]) -> Option<u32> {
    mine.iter().copied().filter(|s| theirs.contains(s)).max()
}

/// The trackers in this repository, by extension id: the agenda asks each
/// of them, and a tracker added here is added to this list.
pub const KNOWN: [&str; 4] = ["fenix.jira", "fenix.github-issues", "fenix.gitlab-issues", "fenix.linear"];

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

    /// The calls that change something in the tracker: gated (`crate::gate`)
    /// and carrying an `op`.
    pub const WRITES: [&str; 6] = [TRANSITION, UPDATE, COMMENT, WORKLOG, CREATE, REMEMBER];

    pub fn writes(call: &str) -> bool {
        WRITES.contains(&call)
    }
}

/// The capabilities `about.can` may list, each one some calls or fields
/// a tracker may leave out.
pub mod can {
    /// Issues carry `comments`.
    pub const COMMENTS: &str = "comments";
    /// `update` with `title`.
    pub const EDIT_TITLE: &str = "edit.title";
    /// `update` with `body`.
    pub const EDIT_BODY: &str = "edit.body";
    /// `update` with `priority`, and `priorities`.
    pub const EDIT_PRIORITY: &str = "edit.priority";
    /// `update` with `due`.
    pub const EDIT_DUE: &str = "edit.due";
    /// `update` with `assignee` (or `null`), and `people`.
    pub const EDIT_ASSIGNEE: &str = "edit.assignee";
    /// `update` with `labels`.
    pub const EDIT_LABELS: &str = "edit.labels";
    /// `comment`.
    pub const COMMENT: &str = "comment";
    /// `worklog`.
    pub const WORKLOG: &str = "worklog";
    /// `create`.
    pub const CREATE: &str = "create";
    /// `remember`.
    pub const REMEMBER: &str = "remember";

    pub const ALL: [&str; 11] = [COMMENTS, EDIT_TITLE, EDIT_BODY, EDIT_PRIORITY, EDIT_DUE, EDIT_ASSIGNEE, EDIT_LABELS, COMMENT, WORKLOG, CREATE, REMEMBER];

    /// The capability an `update` of `change` needs.
    pub fn of(change: &crate::Change) -> &'static str {
        match change {
            crate::Change::Title(_) => EDIT_TITLE,
            crate::Change::Body(_) => EDIT_BODY,
            crate::Change::Priority(_) => EDIT_PRIORITY,
            crate::Change::Due(_) => EDIT_DUE,
            crate::Change::Assignee(_) => EDIT_ASSIGNEE,
            crate::Change::Labels(_) => EDIT_LABELS,
        }
    }
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

/// The service tracker `provider` (its full extension id, `fenix.jira`)
/// answers `call` on: `fenix.jira.tracker.about`. The one place the seam's
/// names are made: if Fenix comes to route by caller or reserve names,
/// this is what changes.
pub fn service(provider: &str, call: &str) -> String {
    format!("{provider}.tracker.{call}")
}

/// The call a service name is, when it's tracker `provider`'s.
pub fn call_of<'a>(provider: &str, service: &'a str) -> Option<&'a str> {
    service.strip_prefix(provider)?.strip_prefix(".tracker.")
}

/// Whether `id` is an extension id as Fenix takes one: `publisher.name`,
/// lower-case letters, digits, `-` and `_`, two parts or more.
pub fn extension_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trackers_services_start_with_its_whole_id() {
        assert_eq!(service("fenix.jira", call::ABOUT), "fenix.jira.tracker.about");
        assert_eq!(service("acme.youtrack", call::ISSUES), "acme.youtrack.tracker.issues");
        assert_eq!(call_of("fenix.jira", "fenix.jira.tracker.issues"), Some("issues"));
        assert_eq!(call_of("fenix.jira", "jira.request"), None);
        assert_eq!(call_of("fenix.linear", "fenix.jira.tracker.issues"), None);
        assert!(KNOWN.iter().all(|k| extension_id(k)));
        assert!(!extension_id("jira") && !extension_id("Fenix.jira") && !extension_id("a..b"));
    }

    #[test]
    fn the_highest_seam_both_speak_is_chosen() {
        assert_eq!(common_seam(&[1], &[1, 2]), Some(1));
        assert_eq!(common_seam(&[1, 2], &[1, 2, 3]), Some(2));
        assert_eq!(common_seam(&[1], &[2]), None);
    }

    #[test]
    fn every_write_needs_its_own_capability() {
        assert_eq!(can::of(&Change::Assignee(None)), "edit.assignee");
        assert_eq!(can::of(&Change::Labels(vec![])), "edit.labels");
        assert!(call::writes("comment") && !call::writes("issues"));
    }
}
