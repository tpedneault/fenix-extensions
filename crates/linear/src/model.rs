//! What Linear answers, as far as this crate reads it, and what becomes
//! of it: an issue as the seam has it, plus the extras Linear's own page
//! shows (its team, cycle, project, sub-issues and suggested branch).

use serde::Deserialize;

use fenix_tracker::{clean, column, level, seam, Issue};

// -- What Linear answers ---------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Conn<T> {
    pub nodes: Vec<T>,
    #[serde(rename = "pageInfo")]
    pub page_info: Option<PageInfo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
}

impl PageInfo {
    /// The cursor to continue from, when there's more.
    pub fn next(&self) -> Option<String> {
        self.end_cursor.clone().filter(|_| self.has_next_page)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawUser {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub is_me: bool,
    pub active: Option<bool>,
}

impl RawUser {
    /// The name people know them by.
    pub fn shown(&self) -> &str {
        if self.name.trim().is_empty() {
            &self.display_name
        } else {
            &self.name
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RawState {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub position: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RawTeam {
    pub id: String,
    pub key: String,
    pub name: String,
    pub states: Option<Conn<RawState>>,
    pub members: Option<Conn<RawUser>>,
    pub labels: Option<Conn<RawLabel>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RawLabel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RawCycle {
    pub id: String,
    pub number: f64,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RawNamed {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RawParent {
    pub identifier: String,
    pub title: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RawChild {
    pub identifier: String,
    pub title: String,
    pub state: Option<RawState>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawComment {
    pub id: String,
    pub body: String,
    pub created_at: String,
    pub user: Option<RawUser>,
    pub bot_actor: Option<RawNamed>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RawIssue {
    pub id: String,
    pub identifier: String,
    pub url: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<f64>,
    pub branch_name: Option<String>,
    pub due_date: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: String,
    pub state: Option<RawState>,
    pub team: Option<RawTeam>,
    pub assignee: Option<RawUser>,
    pub creator: Option<RawUser>,
    pub labels: Option<Conn<RawLabel>>,
    pub cycle: Option<RawCycle>,
    pub project: Option<RawNamed>,
    pub parent: Option<RawParent>,
    pub children: Option<Conn<RawChild>>,
    pub comments: Option<Conn<RawComment>>,
}

// -- Priorities ------------------------------------------------------------

/// Linear's priorities 0 to 4 and the seam's four levels: (number, the
/// name `priorities` offers and `update` takes back, level). 0, no
/// priority, has no level (`null`: the agenda reads it as Medium), and is
/// offered last, as a way to clear it.
pub const PRIORITIES: [(u8, &str, Option<&str>); 5] = [(1, "Urgent", Some(level::URGENT)), (2, "High", Some(level::HIGH)), (3, "Medium", Some(level::MEDIUM)), (4, "Low", Some(level::LOW)), (0, "No priority", None)];

/// The seam's priority for Linear's number: none for 0 or a number Linear
/// doesn't have.
pub fn priority_of(number: f64) -> Option<seam::Priority> {
    let n = number.round();
    PRIORITIES.iter().find(|(p, _, _)| *p != 0 && f64::from(*p) == n).map(|(_, name, level)| seam::Priority { name: (*name).into(), level: level.map(Into::into) })
}

/// Linear's number for a priority's name: `Urgent`, `High`, `Medium` (or
/// the `Normal` it used to be called), `Low`, `No priority`, in any case.
pub fn priority_number(name: &str) -> Option<u8> {
    let n = name.trim().to_ascii_lowercase();
    match n.as_str() {
        "normal" => Some(3),
        "none" => Some(0),
        _ => PRIORITIES.iter().find(|(_, p, _)| p.to_ascii_lowercase() == n).map(|(num, _, _)| *num),
    }
}

/// Linear's number for a level: the priority the agenda's Urgent, High,
/// Medium and Low are.
pub fn priority_for_level(l: &str) -> Option<u8> {
    PRIORITIES.iter().find(|(num, _, lv)| *num != 0 && *lv == Some(l)).map(|(num, _, _)| *num)
}

// -- States ----------------------------------------------------------------

/// A workflow state's types, in the order Linear's own board has them.
pub const STATE_TYPES: [&str; 6] = ["triage", "backlog", "unstarted", "started", "completed", "canceled"];

/// How far along a state of this type is: the seam's `stage`.
pub fn stage_of(state_type: &str) -> &'static str {
    match state_type {
        "started" => column::DOING,
        "completed" | "canceled" | "cancelled" => column::DONE,
        _ => column::TODO,
    }
}

/// A state as the seam has it. Linear has no state that means Blocked, so
/// the column is the stage.
pub fn state(raw: &RawState) -> seam::State {
    let stage = stage_of(&raw.kind);
    seam::State { id: raw.id.clone(), name: raw.name.clone(), stage: stage.into(), column: stage.into() }
}

/// Where a state of `kind` goes in the order of the moves offered (`position`
/// within it).
pub fn state_order(kind: &str) -> usize {
    STATE_TYPES.iter().position(|t| *t == kind).unwrap_or(STATE_TYPES.len())
}

// -- An issue --------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Team {
    pub id: String,
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Child {
    pub key: String,
    pub title: String,
    pub state: String,
    pub stage: String,
}

/// One issue: the seam's, and what Linear's own page shows besides.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detail {
    /// Every field cleaned and capped, as the seam sends it.
    pub issue: Issue,
    /// Linear's id for it, what a write names it by.
    pub id: String,
    pub team: Team,
    /// Linear's priority, 0 to 4.
    pub priority: u8,
    pub state_type: String,
    /// The branch name Linear suggests for it (`branchName`), as it came.
    pub branch: String,
    /// `Cycle 12` or `Cycle 12: Hardening`.
    pub cycle: Option<String>,
    pub project_name: Option<String>,
    /// The parent: identifier and title.
    pub parent: Option<(String, String)>,
    pub children: Vec<Child>,
    pub creator: Option<String>,
    pub created: String,
    /// There are more comments than came.
    pub comments_more: bool,
}

/// An issue from what Linear answered. `None` when its identifier can't be
/// kept as a `ref` (empty, or with white space in it).
pub fn detail(raw: RawIssue) -> Option<Detail> {
    let state = raw.state.clone().unwrap_or_default();
    let team = raw.team.clone().unwrap_or_default();
    let number = raw.priority.unwrap_or(0.0);
    let mut comments: Vec<(String, seam::Comment)> = raw
        .comments
        .iter()
        .flat_map(|c| c.nodes.iter())
        .map(|c| {
            let author = c.user.as_ref().map(|u| u.shown().to_string()).filter(|a| !a.trim().is_empty()).or_else(|| c.bot_actor.as_ref().map(|b| b.name.clone())).filter(|a| !a.trim().is_empty()).unwrap_or_else(|| "Linear".into());
            (c.created_at.clone(), seam::Comment { id: c.id.clone(), author, body: c.body.clone(), created: c.created_at.clone() })
        })
        .collect();
    // Newest first as they come; the seam wants the most recent, oldest first.
    comments.sort_by(|a, b| a.0.cmp(&b.0));
    let comments_more = raw.comments.as_ref().and_then(|c| c.page_info.as_ref()).is_some_and(|p| p.has_next_page);
    let assignee = raw.assignee.as_ref().filter(|u| !u.id.is_empty());
    let issue = Issue {
        id: raw.id.clone(),
        asked: None,
        key: raw.identifier.trim().to_ascii_uppercase(),
        url: raw.url.clone(),
        project: Some(team.key.clone()).filter(|k| !k.is_empty()),
        title: raw.title.clone(),
        body: raw.description.clone().unwrap_or_default(),
        body_cut: false,
        state: self::state(&state),
        kind: None,
        priority: priority_of(number),
        assignee: assignee.map(|u| seam::Person { id: u.id.clone(), name: u.shown().to_string() }),
        mine: Some(assignee.is_some_and(|u| u.is_me)),
        labels: raw.labels.iter().flat_map(|l| l.nodes.iter()).map(|l| l.name.clone()).collect(),
        due: raw.due_date.clone().filter(|d| !d.is_empty()),
        updated: raw.updated_at.clone(),
        comments: comments.into_iter().map(|(_, c)| c).collect(),
    }
    .cleaned()?;
    let cycle = raw.cycle.as_ref().filter(|c| !c.id.is_empty()).map(|c| match c.name.as_deref().map(clean::name).filter(|n| !n.is_empty()) {
        Some(name) => format!("Cycle {}: {name}", c.number.round()),
        None => format!("Cycle {}", c.number.round()),
    });
    Some(Detail {
        issue,
        id: clean::name(&raw.id),
        team: Team { id: team.id.clone(), key: clean::name(&team.key), name: clean::name(&team.name) },
        priority: PRIORITIES.iter().find(|(p, _, _)| f64::from(*p) == number.round()).map(|(p, _, _)| *p).unwrap_or(0),
        state_type: state.kind.clone(),
        branch: raw.branch_name.clone().unwrap_or_default(),
        cycle,
        project_name: raw.project.as_ref().map(|p| clean::name(&p.name)).filter(|n| !n.is_empty()),
        parent: raw.parent.as_ref().filter(|p| !p.identifier.is_empty()).map(|p| (clean::name(&p.identifier), clean::line(&p.title, 500))),
        children: raw
            .children
            .iter()
            .flat_map(|c| c.nodes.iter())
            .map(|c| {
                let state = c.state.clone().unwrap_or_default();
                Child { key: clean::name(&c.identifier), title: clean::line(&c.title, 500), state: clean::name(&state.name), stage: stage_of(&state.kind).into() }
            })
            .collect(),
        creator: raw.creator.as_ref().map(|u| clean::name(u.shown())).filter(|n| !n.is_empty()),
        created: raw.created_at.as_deref().map(clean::stamp).unwrap_or_default(),
        comments_more,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priorities_map_both_ways() {
        // 1 to 4 are the seam's four levels, 0 is none.
        for (n, name, level) in [(1.0, "Urgent", "urgent"), (2.0, "High", "high"), (3.0, "Medium", "medium"), (4.0, "Low", "low")] {
            let p = priority_of(n).unwrap();
            assert_eq!((p.name.as_str(), p.level.as_deref()), (name, Some(level)));
            assert_eq!(priority_number(name), Some(n as u8));
            assert_eq!(priority_number(&name.to_uppercase()), Some(n as u8), "any case");
            assert_eq!(priority_for_level(level), Some(n as u8));
        }
        assert_eq!(priority_of(0.0), None, "no priority is none");
        assert_eq!(priority_of(7.0), None);
        assert_eq!(priority_number("No priority"), Some(0));
        assert_eq!(priority_number("Normal"), Some(3), "what Medium used to be called");
        assert_eq!(priority_number("Blocker"), None);
        assert_eq!(priority_for_level("none"), None);
        // `priorities` offers all five, and every name it offers `update` takes.
        for (n, name, _) in PRIORITIES {
            assert_eq!(priority_number(name), Some(n));
        }
    }

    #[test]
    fn a_states_type_gives_its_stage_and_the_column_is_the_stage() {
        for (kind, stage) in [("triage", "todo"), ("backlog", "todo"), ("unstarted", "todo"), ("started", "doing"), ("completed", "done"), ("canceled", "done"), ("cancelled", "done"), ("something new", "todo")] {
            let s = state(&RawState { id: "s".into(), name: "N".into(), kind: kind.into(), position: 0.0 });
            assert_eq!((s.stage.as_str(), s.column.as_str()), (stage, stage), "{kind}");
        }
        assert!(state_order("triage") < state_order("backlog") && state_order("started") < state_order("completed") && state_order("completed") < state_order("canceled") && state_order("odd") > state_order("canceled"));
    }

    #[test]
    fn comments_come_oldest_first_and_a_bot_has_a_name() {
        let raw: RawIssue = serde_json::from_str(
            r#"{"id": "u", "identifier": "eng-1", "title": "T", "updatedAt": "2026-11-14T09:30:00.000Z", "state": {"id": "s", "name": "Todo", "type": "unstarted"},
                "comments": {"nodes": [
                  {"id": "c3", "body": "newest", "createdAt": "2026-11-14T09:00:00.000Z", "user": {"name": "Jo", "displayName": "jo"}},
                  {"id": "c2", "body": "from a bot", "createdAt": "2026-11-13T09:00:00.000Z", "user": null, "botActor": {"name": "GitHub"}},
                  {"id": "c1", "body": "oldest", "createdAt": "2026-11-12T09:00:00.000Z", "user": null}
                ], "pageInfo": {"hasNextPage": true}}}"#,
        )
        .unwrap();
        let d = detail(raw).unwrap();
        assert_eq!(d.issue.key, "ENG-1", "capitals");
        assert_eq!(d.issue.id, "u", "Linear's own id, kept for the agenda");
        assert_eq!(d.issue.comments.iter().map(|c| (c.id.as_str(), c.author.as_str())).collect::<Vec<_>>(), [("c1", "Linear"), ("c2", "GitHub"), ("c3", "Jo")]);
        assert!(d.comments_more);
        assert_eq!(d.issue.mine, Some(false), "unassigned isn't mine");
        assert!(d.issue.priority.is_none() && d.issue.assignee.is_none());
    }

    #[test]
    fn an_issue_without_a_usable_identifier_is_dropped() {
        assert!(detail(RawIssue { identifier: "".into(), title: "x".into(), ..Default::default() }).is_none());
        assert!(detail(RawIssue { identifier: "ENG 1".into(), title: "x".into(), ..Default::default() }).is_none());
    }
}
