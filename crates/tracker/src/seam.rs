//! The seam's shapes: every request and answer, as JSON. Each request
//! carries `seam`; every optional field is optional on the wire, and
//! fields a side doesn't know are ignored, so versions within a major
//! read each other (`plans/tracker-seam.md` §3.5).

use serde::{Deserialize, Serialize};

fn ten() -> u32 {
    10
}

fn seam() -> u32 {
    crate::SEAM
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn seams() -> Vec<u32> {
    vec![crate::SEAM]
}

/// `about`'s answer: who the tracker is, which versions of the seam it
/// speaks, and whether it's ready. Answered whatever `seam` it was asked
/// with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct About {
    /// The version this answer is in: the highest of `seams`.
    pub seam: u32,
    /// Every major version it speaks; the caller picks the highest it
    /// shares.
    #[serde(default = "seams")]
    pub seams: Vec<u32>,
    #[serde(default)]
    pub minor: u32,
    /// Its extension's full id, `fenix.jira`: the caller checks it's the
    /// one it asked.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub ready: bool,
    /// What to do about it, when it isn't ready.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_ready: Option<String>,
    /// Its capabilities (`crate::can`).
    #[serde(default)]
    pub can: Vec<String>,
    /// How often linked tasks should be brought up to date; 0 never.
    #[serde(default = "ten")]
    pub sync_minutes: u32,
    /// The key of Fenix's project metadata naming its project here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_meta: Option<String>,
    /// An issue's address with `{ref}` in it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browse: Option<String>,
    /// An example `ref`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_hint: Option<String>,
    /// Whether it has a Blocked of its own; false, and the agenda keeps
    /// Blocked to itself (a card moved there sends nothing).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub blocked: bool,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

impl About {
    pub fn can(&self, capability: &str) -> bool {
        self.can.iter().any(|c| c == capability)
    }

    /// The address of issue `key`, from `browse`.
    pub fn browse_url(&self, key: &str) -> Option<String> {
        self.browse.as_deref().filter(|b| b.contains("{ref}")).map(|b| b.replace("{ref}", key))
    }
}

/// Where an issue stands.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// The tracker's own id for it.
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// `todo`, `doing` or `done` (`crate::column`, less `blocked`).
    pub stage: String,
    /// The agenda column it belongs in: `stage`, or `blocked`.
    #[serde(default)]
    pub column: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Priority {
    pub name: String,
    /// `urgent`, `high`, `medium` or `low` (`crate::level`); `null` for
    /// a priority that means none (Linear's "No priority").
    #[serde(default)]
    pub level: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    #[serde(default)]
    pub id: String,
    pub author: String,
    pub body: String,
    #[serde(default)]
    pub created: String,
}

/// One issue, as every call that returns issues returns it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    /// The tracker's immutable id for it, never shown: what a link is
    /// kept by. A tracker with none gives its `ref`.
    #[serde(default)]
    pub id: String,
    /// Its key as people write it (`PROJ-12`, `owner/repo#3`), shown;
    /// it may change when the issue moves.
    #[serde(rename = "ref")]
    pub key: String,
    /// In `issues`' answer: the item of `refs` it answers, as it was
    /// asked -- a ref, an id or a URL -- so a moved issue is found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub title: String,
    /// Plain text as a person would read it raw.
    #[serde(default)]
    pub body: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub body_cut: bool,
    pub state: State,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Person>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mine: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    #[serde(default)]
    pub updated: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<Comment>,
}

/// A request that carries nothing but the version: `about`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    #[serde(default = "seam")]
    pub seam: u32,
}

impl Default for Hello {
    fn default() -> Self {
        Hello { seam: crate::SEAM }
    }
}

/// `assigned`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assigned {
    #[serde(default = "seam")]
    pub seam: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// A previous answer's `next`, byte for byte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl Assigned {
    pub const DEFAULT_LIMIT: u32 = 50;
    pub const MAX_LIMIT: u32 = 200;

    pub fn new(limit: u32, cursor: Option<String>) -> Self {
        Assigned { seam: crate::SEAM, limit: Some(limit), cursor }
    }

    /// `limit`, held to 1 to 200.
    pub fn limit(&self) -> u32 {
        self.limit.unwrap_or(Self::DEFAULT_LIMIT).clamp(1, Self::MAX_LIMIT)
    }
}

/// `assigned`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub issues: Vec<Issue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

/// `issues`: each of `refs` an issue's `id`, its `ref` or its URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issues {
    #[serde(default = "seam")]
    pub seam: u32,
    pub refs: Vec<String>,
}

impl Issues {
    /// The most refs one call takes.
    pub const MAX_REFS: usize = 50;

    pub fn new(refs: Vec<String>) -> Self {
        Issues { seam: crate::SEAM, refs }
    }
}

/// `issues`' answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    pub issues: Vec<Issue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

/// `transitions`: what can be done next with an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transitions {
    #[serde(default = "seam")]
    pub seam: u32,
    #[serde(rename = "ref")]
    pub key: String,
    /// The column a card was moved to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toward: Option<String>,
    /// The state the agenda last saw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<State>,
}

/// One thing that can be done next.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    pub id: String,
    pub name: String,
    pub to: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// It changes nothing in the tracker.
    #[serde(default, skip_serializing_if = "is_false")]
    pub local: bool,
    /// A choice the tracker keeps once this is picked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remember: Option<String>,
    /// Said when it's picked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Said once it's done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub said: Option<String>,
}

impl Move {
    /// A picker's row for it.
    pub fn row(&self) -> String {
        match &self.label {
            Some(label) => label.clone(),
            None if self.to.name.is_empty() || self.to.name == self.name => self.name.clone(),
            None => format!("{}  ->  {}", self.name, self.to.name),
        }
    }
}

/// The tracker wants you to choose.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `transitions`' answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moves {
    pub transitions: Vec<Move>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<Ask>,
    /// The label of the agenda's own Blocked row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
    /// Why none reaches `toward`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// `transition`: doing one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    #[serde(default = "seam")]
    pub seam: u32,
    #[serde(rename = "ref")]
    pub key: String,
    pub id: String,
    /// This write's own id, the same each time it's sent: a tracker may
    /// answer a second one as it answered the first, without doing it
    /// again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
}

/// What a write says when it's done.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Said {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub said: Option<String>,
}

/// A request about one issue: `priorities`, `people`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct One {
    #[serde(default = "seam")]
    pub seam: u32,
    #[serde(rename = "ref")]
    pub key: String,
}

/// One field changed (`update`'s `set`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Title(String),
    Body(String),
    /// The tracker's priority name.
    Priority(String),
    /// `YYYY-MM-DD`, or none to clear it.
    Due(Option<String>),
    /// Someone from `people`, or none: unassigned.
    Assignee(Option<Person>),
    /// Every label it should have.
    Labels(Vec<String>),
}

/// `update`: one field of an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Update {
    #[serde(default = "seam")]
    pub seam: u32,
    #[serde(rename = "ref")]
    pub key: String,
    pub set: serde_json::Map<String, serde_json::Value>,
    /// This write's own id (see `Transition::op`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
}

impl Update {
    pub fn new(key: &str, change: &Change, op: Option<String>) -> Self {
        let (name, value) = match change {
            Change::Title(s) => ("title", serde_json::Value::from(s.as_str())),
            Change::Body(s) => ("body", serde_json::Value::from(s.as_str())),
            Change::Priority(s) => ("priority", serde_json::Value::from(s.as_str())),
            Change::Due(d) => ("due", d.as_deref().map(serde_json::Value::from).unwrap_or(serde_json::Value::Null)),
            Change::Assignee(Some(p)) => ("assignee", serde_json::json!({ "id": p.id, "name": p.name })),
            Change::Assignee(None) => ("assignee", serde_json::Value::Null),
            Change::Labels(l) => ("labels", serde_json::json!(l)),
        };
        let mut set = serde_json::Map::new();
        set.insert(name.into(), value);
        Update { seam: crate::SEAM, key: key.to_string(), set, op }
    }

    /// The one field it sets. A field the seam doesn't have is
    /// `unsupported` (a newer caller's); a malformed one `bad_request`.
    pub fn change(&self) -> Result<Change, crate::SeamError> {
        use crate::code;
        let bad = |why: String| crate::SeamError::new(code::BAD_REQUEST, why);
        let mut fields = self.set.iter();
        let (Some((name, value)), None) = (fields.next(), fields.next()) else {
            return Err(bad("update sets one field at a time".into()));
        };
        let text = |v: &serde_json::Value| v.as_str().map(str::to_string).ok_or_else(|| bad(format!("{name}: a string")));
        match name.as_str() {
            "title" => text(value).map(Change::Title),
            "body" => text(value).map(Change::Body),
            "priority" => text(value).map(Change::Priority),
            "due" if value.is_null() => Ok(Change::Due(None)),
            "due" => text(value).map(|d| Change::Due(Some(d))),
            "assignee" if value.is_null() => Ok(Change::Assignee(None)),
            "assignee" => serde_json::from_value::<Person>(value.clone()).map(|p| Change::Assignee(Some(p))).map_err(|e| bad(format!("assignee: {e}"))),
            "labels" => serde_json::from_value::<Vec<String>>(value.clone()).map(Change::Labels).map_err(|e| bad(format!("labels: {e}"))),
            other => Err(crate::SeamError::new(code::UNSUPPORTED, format!("update can't set {other}"))),
        }
    }
}

/// `priorities`' answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Priorities {
    pub priorities: Vec<Priority>,
}

/// `people`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct People {
    pub people: Vec<Person>,
    /// What to do when there's no one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub none: Option<String>,
}

/// `comment`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewComment {
    #[serde(default = "seam")]
    pub seam: u32,
    #[serde(rename = "ref")]
    pub key: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
}

/// `worklog`: time worked, filed on the day it started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worklog {
    #[serde(default = "seam")]
    pub seam: u32,
    #[serde(rename = "ref")]
    pub key: String,
    pub seconds: i64,
    /// RFC 3339.
    pub started: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
}

/// `create`: the tracker's own new-issue form, filled in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Create {
    #[serde(default = "seam")]
    pub seam: u32,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
}

/// `create`'s answer: what was made; or nothing; or, from a tracker that
/// doesn't hold the call while you fill its form in, `opened`: said at
/// once ("Opened a new issue in your browser -- I links it once it's
/// made").
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Created {
    #[serde(rename = "ref", default)]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opened: Option<String>,
}

/// `project`'s answer: the Fenix project you're in, as this tracker's
/// issues name their `project` -- from a setting of its own that a
/// project may set, from the git remote -- or `null` when it can't tell.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    #[serde(default)]
    pub project: Option<String>,
}

/// `remember`: keep a choice a move carried.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remember {
    #[serde(default = "seam")]
    pub seam: u32,
    #[serde(rename = "ref", default)]
    pub key: String,
    pub remember: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_issue_reads_with_only_what_is_required() {
        let issue: Issue = serde_json::from_str(r#"{"ref": "A-1", "title": "T", "state": {"name": "Open", "stage": "todo"}}"#).unwrap();
        assert_eq!(issue.key, "A-1");
        assert_eq!(issue.body, "");
        assert!(issue.comments.is_empty() && issue.labels.is_empty() && issue.mine.is_none());
        // Written back, optional fields that are absent stay absent.
        let json = serde_json::to_value(&issue).unwrap();
        assert!(json.get("url").is_none() && json.get("comments").is_none() && json.get("body_cut").is_none());
        assert_eq!(json["ref"], "A-1");
    }

    #[test]
    fn fields_it_doesnt_know_are_ignored() {
        let about: About = serde_json::from_str(r#"{"seam": 1, "name": "X", "ready": true, "colour": "red", "can": ["edit", "teleport"]}"#).unwrap();
        assert!(about.can("edit") && about.can("teleport") && !about.can("comment"));
        assert_eq!(about.sync_minutes, 10, "the default");
    }

    #[test]
    fn an_update_sets_one_field_and_due_can_be_cleared() {
        let up = Update::new("A-1", &Change::Due(None), Some("op-1".into()));
        assert_eq!(serde_json::to_string(&up).unwrap(), r#"{"seam":1,"ref":"A-1","set":{"due":null},"op":"op-1"}"#);
        assert_eq!(up.change(), Ok(Change::Due(None)));
        let jo = Person { id: "jo smith".into(), name: "Jo".into() };
        assert_eq!(Update::new("A-1", &Change::Assignee(Some(jo.clone())), None).change(), Ok(Change::Assignee(Some(jo))));
        let nobody = Update::new("A-1", &Change::Assignee(None), None);
        assert_eq!(serde_json::to_value(&nobody).unwrap()["set"]["assignee"], serde_json::Value::Null);
        assert_eq!(nobody.change(), Ok(Change::Assignee(None)), "unassigned");
        assert_eq!(Update::new("A-1", &Change::Labels(vec!["x".into()]), None).change(), Ok(Change::Labels(vec!["x".into()])));
        let two: Update = serde_json::from_str(r#"{"ref": "A-1", "set": {"title": "a", "body": "b"}}"#).unwrap();
        assert_eq!(two.change().unwrap_err().code, "bad_request");
        let odd: Update = serde_json::from_str(r#"{"ref": "A-1", "set": {"colour": "red"}}"#).unwrap();
        assert_eq!(odd.change().unwrap_err().code, "unsupported", "a newer caller's field");
    }

    #[test]
    fn a_moves_row_says_where_it_lands_when_its_name_doesnt() {
        let mut m = Move { id: "1".into(), name: "Start".into(), to: State { name: "In Progress".into(), ..Default::default() }, ..Default::default() };
        assert_eq!(m.row(), "Start  ->  In Progress");
        m.to.name = "Start".into();
        assert_eq!(m.row(), "Start");
        m.label = Some("Flag it".into());
        assert_eq!(m.row(), "Flag it");
    }

    #[test]
    fn since_is_gone_from_seam_1_and_an_old_caller_sending_it_is_ignored() {
        let a: Assigned = serde_json::from_str(r#"{"seam": 1, "since": "2026-01-01T00:00:00Z"}"#).unwrap();
        assert!(!serde_json::to_string(&a).unwrap().contains("since"));
    }

    #[test]
    fn a_limit_is_held_to_what_the_seam_allows() {
        assert_eq!(Assigned::new(500, None).limit(), 200);
        assert_eq!(Assigned::new(0, None).limit(), 1);
        assert_eq!(serde_json::from_str::<Assigned>("{}").unwrap().limit(), 50);
    }

    #[test]
    fn browse_fills_in_the_ref() {
        let about: About = serde_json::from_str(r#"{"seam": 1, "name": "J", "ready": true, "browse": "https://j/browse/{ref}"}"#).unwrap();
        assert_eq!(about.seams, [1], "an about without seams speaks its seam");
        assert!(about.blocked, "a Blocked of its own unless it says not");
        let p: Priority = serde_json::from_str(r#"{"name": "No priority", "level": null}"#).unwrap();
        assert_eq!(p.level, None);
        assert_eq!(about.browse_url("A-1").as_deref(), Some("https://j/browse/A-1"));
    }
}
