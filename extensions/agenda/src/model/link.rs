//! A task's optional link to an issue in a tracker, and the pieces that
//! keep the two in step -- still no calls: `crate::sync` asks the tracker
//! (`plans/tracker-seam.md`), turns its issues into a `RemoteUpdate`, and
//! sends what's in `AgendaStore::outbox`. What lives here is the part
//! worth unit-testing on its own: which side wins when both changed.
//!
//! Sync is field-by-field against `IssueLink::base`, the last state of
//! the issue this task agreed with. A field the tracker changed and you
//! didn't takes the tracker's value; a field you changed (an op still in
//! the outbox) and the tracker didn't gets pushed; a field both changed
//! becomes a `Conflict` you settle by hand, and its op is held until you
//! do.
//!
//! `agenda.json` keeps the shape agenda 1.2 wrote, when every link was
//! Jira's: a link is still under `"jira"`, its fields named as they were,
//! so going back to 1.2 keeps them (`crate::legacy` reads the old one).

use chrono::DateTime;
use crate::clock::Local;
use serde::{Deserialize, Serialize};

use crate::model::task::{Priority, Status, TaskId};

/// One comment as the tracker has it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteComment {
    pub id: String,
    pub author: String,
    pub body: String,
    /// The tracker's own timestamp string, kept verbatim.
    pub created: String,
}

/// The tracker's side of a linked task, as of one fetch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSnapshot {
    /// Its title.
    pub summary: String,
    /// Its body, as text.
    pub description: String,
    pub status_id: String,
    pub status_name: String,
    /// Its stage: `todo`, `doing` or `done` (Jira's `new`, `indeterminate`
    /// and `done` in a file from agenda 1.2).
    pub status_category: String,
    /// The column the tracker puts it in -- its stage, or `blocked`.
    /// None until the first sync after agenda 1.2.
    #[serde(default)]
    pub column: Option<String>,
    pub priority: Option<String>,
    pub assignee: Option<String>,
    pub assignee_id: Option<String>,
    /// Agenda 1.2's, for Jira's Flagged field: kept so 1.2 still reads
    /// the file, never read here.
    #[serde(default)]
    pub flagged: bool,
    pub updated: String,
    pub comments: Vec<RemoteComment>,
    /// `YYYY-MM-DD`, when the issue has a due date.
    #[serde(default)]
    pub due: Option<String>,
    /// Its page.
    #[serde(default)]
    pub url: Option<String>,
    /// Its project, repository or team.
    #[serde(default)]
    pub project: Option<String>,
    /// Its body was longer than the agenda keeps: never written back.
    #[serde(default)]
    pub description_cut: bool,
    /// Its title was longer than the agenda keeps: never written back.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub title_cut: bool,
}

/// A fetched issue plus what it means for the agenda's own fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteUpdate {
    pub snapshot: RemoteSnapshot,
    pub status: Status,
    pub priority: Priority,
    /// Whether the issue is still assigned to you; `None` when unknown.
    pub mine: Option<bool>,
}

/// The fields that sync both ways, and so can conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncField {
    Title,
    Description,
    Status,
    Priority,
    Due,
}

impl SyncField {
    pub const ALL: [SyncField; 5] = [SyncField::Title, SyncField::Description, SyncField::Status, SyncField::Priority, SyncField::Due];

    pub fn label(self) -> &'static str {
        match self {
            SyncField::Title => "title",
            SyncField::Description => "description",
            SyncField::Status => "status",
            SyncField::Priority => "priority",
            SyncField::Due => "due date",
        }
    }
}

impl crate::model::task::Task {
    /// This task's own value for `field`, for display beside the
    /// tracker's.
    pub fn sync_value(&self, field: SyncField) -> String {
        match field {
            SyncField::Title => self.title.clone(),
            SyncField::Description => self.description.clone(),
            SyncField::Status => self.status.label().to_string(),
            SyncField::Priority => self.priority.label().to_string(),
            SyncField::Due => self.due.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_else(|| "none".to_string()),
        }
    }
}

/// Both sides changed `field` since the last sync. `theirs` is what the
/// tracker had when that was noticed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub field: SyncField,
    pub theirs: RemoteUpdate,
}

impl Conflict {
    /// The tracker's value for the conflicting field, for display.
    pub fn their_value(&self) -> String {
        let s = &self.theirs.snapshot;
        match self.field {
            SyncField::Title => s.summary.clone(),
            SyncField::Description => s.description.clone(),
            SyncField::Status => s.status_name.clone(),
            SyncField::Priority => s.priority.clone().unwrap_or_default(),
            SyncField::Due => s.due.clone().unwrap_or_else(|| "none".to_string()),
        }
    }
}

/// A link without a tracker is from agenda 1.2: Jira's.
fn legacy_tracker() -> String {
    crate::legacy::ID.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueLink {
    /// The tracker it's in: the name its services start with (`jira`).
    #[serde(default = "legacy_tracker")]
    pub tracker: String,
    /// The issue's `ref` in its tracker, as last seen: refreshed when the
    /// issue moves.
    pub key: String,
    /// The tracker's immutable id for the issue: what the link is kept
    /// by. None until the first sync of a link from before ids.
    #[serde(default)]
    pub id: Option<String>,
    /// The last state of the issue this task agreed with -- what
    /// "changed" is measured against on both sides.
    pub base: RemoteSnapshot,
    pub last_synced: Option<DateTime<Local>>,
    /// Why the last push failed; cleared by the next success or a retry.
    #[serde(default)]
    pub error: Option<String>,
    /// That failure is worth retrying by itself: offline, rate-limited.
    #[serde(default)]
    pub transient: bool,
    /// No answer came for the change sent: it may have been made. Never
    /// sent again by itself; `r` asks.
    #[serde(default)]
    pub unknown: bool,
    #[serde(default)]
    pub conflicts: Vec<Conflict>,
    /// The tracker reassigned the issue to someone else. The task stays;
    /// this just says so.
    #[serde(default)]
    pub not_mine: bool,
    /// A status change is waiting on its moves -- shown as pending,
    /// never saved.
    #[serde(skip)]
    pub resolving: bool,
    /// Changes the tracker said it will never take (`unsupported`): off
    /// the outbox, kept here so the task says so until you choose -- `r`
    /// sends them again, takes the tracker's value back, or keeps yours
    /// here only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<Dropped>,
    /// Fields whose tracker value the next sync takes back, whatever the
    /// base says (you chose to after a change was dropped).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retake: Vec<SyncField>,
}

/// A change the tracker won't take, and what it said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dropped {
    pub kind: OpKind,
    pub why: String,
}

impl IssueLink {
    /// Its project, as the tracker named it.
    pub fn project(&self) -> Option<&str> {
        self.base.project.as_deref()
    }

    /// Whether this is tracker `tracker`'s issue `key` (a ref or an id).
    pub fn is(&self, tracker: &str, key: &str) -> bool {
        self.tracker == tracker && (self.key == key || self.id.as_deref() == Some(key))
    }

    /// Whether this is `tracker`'s issue with id `id`, or -- for a link
    /// that doesn't know its id yet -- ref `key`.
    pub fn is_issue(&self, tracker: &str, id: &str, key: &str) -> bool {
        self.tracker == tracker
            && match &self.id {
                Some(mine) => mine == id,
                None => self.key == key,
            }
    }

    /// What to ask the tracker for it by, and to write to it by: its id
    /// once known, which outlives a move (`plans/tracker-seam.md` §4.2).
    pub fn asked(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.key)
    }

    /// The column the tracker puts it in, as far as the agenda knows.
    pub fn column(&self) -> Option<&str> {
        self.base.column.as_deref()
    }
}

/// One change waiting to be sent to the tracker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingOp {
    pub id: u64,
    pub task: TaskId,
    pub kind: OpKind,
    /// Its `op` for the tracker: the same each time it's sent, so a
    /// tracker can do it once (`plans/tracker-seam.md` §3.7).
    #[serde(default)]
    pub uid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpKind {
    SetSummary(String),
    SetDescription(String),
    /// The tracker's priority *name*.
    SetPriority(String),
    AddComment(String),
    SetAssignee { id: String, name: String },
    /// A move, already chosen, with where it lands.
    Transition {
        id: String,
        to_id: String,
        to_name: String,
        /// Its stage.
        to_category: String,
        #[serde(default)]
        to_column: Option<String>,
        /// What to say once it's done.
        #[serde(default)]
        said: Option<String>,
    },
    /// Agenda 1.2's, Jira's Flagged field: read from an old outbox and
    /// sent as Jira's own move (`crate::legacy::flag_move`).
    SetFlag(bool),
    /// `YYYY-MM-DD`, or `None` to clear it.
    SetDue(Option<String>),
}

impl OpKind {
    /// Which conflict-checked field this op writes, if any.
    pub fn field(&self) -> Option<SyncField> {
        match self {
            OpKind::SetSummary(_) => Some(SyncField::Title),
            OpKind::SetDescription(_) => Some(SyncField::Description),
            OpKind::SetPriority(_) => Some(SyncField::Priority),
            OpKind::Transition { .. } | OpKind::SetFlag(_) => Some(SyncField::Status),
            OpKind::SetDue(_) => Some(SyncField::Due),
            OpKind::AddComment(_) | OpKind::SetAssignee { .. } => None,
        }
    }

    /// A short past-tense description for status messages.
    pub fn describe(&self) -> String {
        match self {
            OpKind::SetSummary(_) => "title updated".to_string(),
            OpKind::SetDescription(_) => "description updated".to_string(),
            OpKind::SetPriority(name) => format!("priority set to {name}"),
            OpKind::AddComment(_) => "comment posted".to_string(),
            OpKind::SetAssignee { name, .. } => format!("assigned to {name}"),
            OpKind::Transition { said: Some(said), .. } => said.clone(),
            OpKind::Transition { to_name, .. } => format!("moved to {to_name}"),
            OpKind::SetFlag(true) => "flagged".to_string(),
            OpKind::SetFlag(false) => "unflagged".to_string(),
            OpKind::SetDue(Some(date)) => format!("due {date}"),
            OpKind::SetDue(None) => "due date cleared".to_string(),
        }
    }

    /// Folds a successful op into the base snapshot, so the next fetch
    /// doesn't mistake our own change for the tracker's.
    pub(crate) fn apply_to(&self, base: &mut RemoteSnapshot) {
        match self {
            OpKind::SetSummary(s) => base.summary = s.clone(),
            OpKind::SetDescription(s) => base.description = s.clone(),
            OpKind::SetPriority(p) => base.priority = Some(p.clone()),
            OpKind::AddComment(_) => {}
            OpKind::SetAssignee { id, name } => {
                base.assignee_id = Some(id.clone());
                base.assignee = Some(name.clone());
            }
            OpKind::Transition { to_id, to_name, to_category, to_column, .. } => {
                base.status_id = to_id.clone();
                base.status_name = to_name.clone();
                base.status_category = to_category.clone();
                base.column = to_column.clone();
            }
            OpKind::SetFlag(on) => {
                base.flagged = *on;
                base.column = None;
            }
            OpKind::SetDue(due) => base.due = due.clone(),
        }
    }
}

/// The agenda column a tracker's column is.
pub fn status_for_column(column: &str) -> Status {
    match column {
        fenix_tracker::column::DONE => Status::Done,
        fenix_tracker::column::DOING => Status::InProgress,
        fenix_tracker::column::BLOCKED => Status::Blocked,
        _ => Status::Todo,
    }
}

/// The seam's name for an agenda column.
pub fn column_for_status(status: Status) -> &'static str {
    match status {
        Status::Todo => fenix_tracker::column::TODO,
        Status::InProgress => fenix_tracker::column::DOING,
        Status::Blocked => fenix_tracker::column::BLOCKED,
        Status::Done => fenix_tracker::column::DONE,
    }
}

/// The agenda's priority a tracker's level is; none is Medium.
pub fn priority_for_level(level: Option<&str>) -> Priority {
    match level {
        Some(fenix_tracker::level::URGENT) => Priority::Urgent,
        Some(fenix_tracker::level::HIGH) => Priority::High,
        Some(fenix_tracker::level::LOW) => Priority::Low,
        _ => Priority::Medium,
    }
}

/// Rounds a worklog to the nearest `round` minutes, never down to zero for
/// time that was actually worked.
pub fn round_minutes(minutes: i64, round: u32) -> i64 {
    let round = i64::from(round.max(1));
    if minutes <= 0 {
        return 0;
    }
    (((minutes + round / 2) / round) * round).max(round)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_land_on_the_agendas_priorities() {
        assert_eq!(priority_for_level(Some("urgent")), Priority::Urgent);
        assert_eq!(priority_for_level(Some("high")), Priority::High);
        assert_eq!(priority_for_level(Some("low")), Priority::Low);
        assert_eq!(priority_for_level(Some("medium")), Priority::Medium);
        assert_eq!(priority_for_level(None), Priority::Medium);
    }

    #[test]
    fn rounding_goes_to_the_nearest_unit_but_never_to_zero() {
        assert_eq!(round_minutes(7, 15), 15);
        assert_eq!(round_minutes(22, 15), 15);
        assert_eq!(round_minutes(23, 15), 30);
        assert_eq!(round_minutes(80, 15), 75);
        assert_eq!(round_minutes(83, 15), 90);
        assert_eq!(round_minutes(83, 1), 83);
        assert_eq!(round_minutes(0, 15), 0);
    }

    #[test]
    fn columns_map_to_statuses_and_back() {
        for status in Status::ALL {
            assert_eq!(status_for_column(column_for_status(status)), status);
        }
    }

    #[test]
    fn a_said_move_says_so() {
        let mut op = OpKind::Transition { id: "1".into(), to_id: "2".into(), to_name: "Done".into(), to_category: "done".into(), to_column: Some("done".into()), said: None };
        assert_eq!(op.describe(), "moved to Done");
        if let OpKind::Transition { said, .. } = &mut op {
            *said = Some("flagged".into());
        }
        assert_eq!(op.describe(), "flagged");
    }
}
