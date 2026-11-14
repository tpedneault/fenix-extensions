use chrono::{DateTime};
use crate::clock::Local;
use serde::{Deserialize, Serialize};

/// Identity of one task, stable for as long as it exists (including once
/// archived -- only `AgendaStore::delete` ever frees an id, and even then
/// `next_id` is never reused, so a dangling `depends_on`/detail-pane
/// reference always means "really gone," never "now means something else").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TaskId(pub u32);

/// Where a task sits in the Kanban board's four columns. `Blocked` is a
/// manual, external state ("waiting on a review", "waiting on IT") --
/// distinct from a task merely having an unresolved `depends_on` entry,
/// which `AgendaStore::is_ready` reports separately and never changes
/// `status` itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Todo,
    InProgress,
    Blocked,
    Done,
}

impl Status {
    pub const ALL: [Status; 4] = [Status::Todo, Status::InProgress, Status::Blocked, Status::Done];

    pub fn label(self) -> &'static str {
        match self {
            Status::Todo => "Todo",
            Status::InProgress => "In Progress",
            Status::Blocked => "Blocked",
            Status::Done => "Done",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    Low,
    Medium,
    High,
    Urgent,
}

impl Priority {
    pub const ALL: [Priority; 4] = [Priority::Low, Priority::Medium, Priority::High, Priority::Urgent];

    pub fn label(self) -> &'static str {
        match self {
            Priority::Low => "Low",
            Priority::Medium => "Medium",
            Priority::High => "High",
            Priority::Urgent => "Urgent",
        }
    }
}

/// One entry in a task's notes log. Ordinarily append-only -- logging what
/// changed as a *new* entry, rather than rewriting an old one, is what
/// keeps earlier context from being lost the way a single freeform "notes"
/// field would lose it on every edit -- but `AgendaStore::edit_note`/
/// `remove_note` exist to fix a mistaken or accidental entry after the
/// fact, the same "a bad edit shouldn't have to stay forever" posture
/// `remove_subtask` already has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteEntry {
    pub at: DateTime<Local>,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeSource {
    /// Recorded by `AgendaStore::clock_in`/`clock_out`.
    Timer,
    /// Recorded by `AgendaStore::log_manual_time`, for time worked before
    /// the timer was started or that never went through it at all.
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeEntry {
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub source: TimeSource,
    /// Already sent to its tracker as (part of) a worklog -- or dismissed from
    /// the worklog review without sending. Only meaningful on a linked
    /// task.
    #[serde(default)]
    pub sent: bool,
}

impl TimeEntry {
    pub fn duration(&self) -> chrono::Duration {
        self.end - self.start
    }
}

/// One step in a task's own checklist -- a lighter-weight breakdown than a
/// full linked task: no status, priority, or time tracking of its own, just
/// text and whether it's done. A step worth tracking on its own terms
/// belongs in `depends_on` instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subtask {
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "TaskFile", into = "TaskFile")]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub description: String,
    pub status: Status,
    pub priority: Priority,
    /// Matches a name in `Config::agenda_categories` by value, not by
    /// index -- so removing a category from `config.ini` never orphans or
    /// fails to load a task that still names it; it just becomes an
    /// unrecognized label wherever it's shown.
    pub category: Option<String>,
    pub notes: Vec<NoteEntry>,
    pub time_entries: Vec<TimeEntry>,
    pub subtasks: Vec<Subtask>,
    /// Tasks that must be `Done` before this one is considered ready to
    /// work (see `AgendaStore::is_ready`). The reverse view ("this blocks
    /// ...") is never stored -- `AgendaStore::blocks` derives it by
    /// scanning every task's own `depends_on`, so the two can never drift
    /// out of sync with each other.
    pub depends_on: Vec<TaskId>,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
    pub done_at: Option<DateTime<Local>>,
    /// Hides a `Done` task from the board/list while keeping it (and its
    /// time entries) in the time report and in `agenda.json` -- a soft
    /// "cross it off the paper list" rather than a delete.
    pub archived: bool,
    /// Manual position within its status column; lower sorts first. Only
    /// meaningful relative to other tasks sharing the same `status`.
    pub order: i64,
    /// The issue this task tracks, if any -- see `crate::model::link`.
    /// On disk (`TaskFile`), a Jira link under `"jira"` as agenda 1.2
    /// wrote it, any other tracker's under `"link"`, which 1.2 ignores.
    pub link: Option<crate::model::link::IssueLink>,
    /// When it's due, if ever. On a linked task, the issue's own due date.
    #[serde(default)]
    pub due: Option<chrono::NaiveDate>,
    /// The code it's about, when it was made from a selection.
    #[serde(default)]
    pub code: Option<CodeRef>,
}

/// A place in a file: what a task made from a selection points back to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRef {
    pub path: std::path::PathBuf,
    /// 1-based.
    pub line: usize,
}

impl Task {
    /// Its issue's `ref`, when it's linked.
    pub fn issue_key(&self) -> Option<&str> {
        self.link.as_ref().map(|link| link.key.as_str())
    }

    /// Whether the task belongs to a project called `name` whose
    /// trackers' projects are `issues` (tracker, project). Categories
    /// double as projects: a task filed under a category named like the
    /// project is in it, and so is a linked task whose issue is in one of
    /// the project's trackers' projects.
    pub fn in_project(&self, name: &str, issues: &[(String, String)]) -> bool {
        let by_category = self.category.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(name));
        let by_issue = self.link.as_ref().is_some_and(|link| {
            issues.iter().any(|(tracker, project)| link.tracker == *tracker && link.project().is_some_and(|p| p.eq_ignore_ascii_case(project)))
        });
        by_category || by_issue
    }

    pub fn total_time(&self) -> chrono::Duration {
        self.time_entries.iter().fold(chrono::Duration::zero(), |acc, e| acc + e.duration())
    }
}

/// A task as `agenda.json` keeps it. Agenda 1.2 reads `"jira"` as a Jira
/// link and ignores `"link"`: so a Jira link stays under `"jira"`, where
/// going back to 1.2 keeps it, and every other tracker's goes under
/// `"link"`, where 1.2 can't mistake a Linear `ENG-12` for Jira's.
#[derive(Serialize, Deserialize)]
struct TaskFile {
    id: TaskId,
    title: String,
    description: String,
    status: Status,
    priority: Priority,
    category: Option<String>,
    notes: Vec<NoteEntry>,
    time_entries: Vec<TimeEntry>,
    subtasks: Vec<Subtask>,
    depends_on: Vec<TaskId>,
    created_at: DateTime<Local>,
    updated_at: DateTime<Local>,
    done_at: Option<DateTime<Local>>,
    archived: bool,
    order: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    jira: Option<crate::model::link::IssueLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    link: Option<crate::model::link::IssueLink>,
    #[serde(default)]
    due: Option<chrono::NaiveDate>,
    #[serde(default)]
    code: Option<CodeRef>,
}

impl From<TaskFile> for Task {
    fn from(f: TaskFile) -> Task {
        Task {
            id: f.id,
            title: f.title,
            description: f.description,
            status: f.status,
            priority: f.priority,
            category: f.category,
            notes: f.notes,
            time_entries: f.time_entries,
            subtasks: f.subtasks,
            depends_on: f.depends_on,
            created_at: f.created_at,
            updated_at: f.updated_at,
            done_at: f.done_at,
            archived: f.archived,
            order: f.order,
            link: f.link.or(f.jira),
            due: f.due,
            code: f.code,
        }
    }
}

impl From<Task> for TaskFile {
    fn from(t: Task) -> TaskFile {
        let jira = t.link.as_ref().is_some_and(|l| l.tracker == crate::legacy::ID);
        let (jira, link) = if jira { (t.link, None) } else { (None, t.link) };
        TaskFile {
            id: t.id,
            title: t.title,
            description: t.description,
            status: t.status,
            priority: t.priority,
            category: t.category,
            notes: t.notes,
            time_entries: t.time_entries,
            subtasks: t.subtasks,
            depends_on: t.depends_on,
            created_at: t.created_at,
            updated_at: t.updated_at,
            done_at: t.done_at,
            archived: t.archived,
            order: t.order,
            jira,
            link,
            due: t.due,
            code: t.code,
        }
    }
}
