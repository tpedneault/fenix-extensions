//! Keeping linked tasks and their Jira issues in step: fetching issues
//! and turning them into `RemoteUpdate`s, sending the outbox one op at a
//! time, turning a board move into a real workflow transition, learning
//! what "Blocked" means in each project, linking and importing issues,
//! and sending worklogs.
//!
//! Jira itself is the Jira extension's: it holds your server, your token
//! and Jira's settings, and passes requests on (`jira.request`). Each job
//! here runs as `async` and ends with a [`Done`], which the agenda takes
//! in once the job's answer has come (`Agenda::take_done`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::time::{Duration, Instant};

use fenix_extension_api::run;
use fenix_jira_client::{IssueDetail, JiraClient, Reply, StatusInfo, Transition, Transport};

use crate::model::{self, OpKind, Priority, RemoteComment, RemoteSnapshot, RemoteUpdate, Status, TaskId, WorklogRow};

/// Shown on a linked task whose issue a sync couldn't find.
pub const NOT_FOUND: &str = "not found in Jira -- deleted, moved, or no longer visible to you";

/// How long a sync stays fresh enough that opening the agenda doesn't
/// start another one.
pub const STALE_AFTER: Duration = Duration::from_secs(120);

/// Jira, through the Jira extension.
struct ViaJira;

impl Transport for ViaJira {
    fn send(&self, method: &str, path: &str, body: Option<String>) -> Pin<Box<dyn Future<Output = Reply>>> {
        let payload = serde_json::json!({ "method": method, "path": path, "body": body }).to_string();
        Box::pin(async move {
            let answer = run::call("jira.request", &payload).await?;
            let v: serde_json::Value = serde_json::from_str(&answer).map_err(|e| e.to_string())?;
            let status = v.get("status").and_then(|s| s.as_u64()).unwrap_or(0) as u16;
            Ok((status, v.get("body").and_then(|b| b.as_str()).unwrap_or_default().to_string()))
        })
    }
}

pub fn client() -> JiraClient {
    JiraClient::new(Rc::new(ViaJira))
}

/// What "Blocked" means for one Jira project (`jira.blocked`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocked {
    /// Transition to this status (matched by id; `name` is for display).
    Status { id: String, name: String },
    /// Set the issue's Flagged (impediment) field; leave the status alone.
    Flag,
    /// Don't touch Jira at all.
    Local,
}

impl Blocked {
    /// `flag`, `local`, or a status as `ID: Name`.
    pub fn parse(text: &str) -> Result<Blocked, String> {
        match text.trim() {
            "flag" => Ok(Blocked::Flag),
            "local" => Ok(Blocked::Local),
            other => match other.split_once(':') {
                Some((id, name)) if !id.trim().is_empty() => Ok(Blocked::Status { id: id.trim().into(), name: name.trim().into() }),
                _ => Err(format!("\"{other}\": expected flag, local, or a status as ID: Name")),
            },
        }
    }

    /// As `jira.blocked` holds it.
    pub fn text(&self) -> String {
        match self {
            Blocked::Flag => "flag".into(),
            Blocked::Local => "local".into(),
            Blocked::Status { id, name } => format!("{id}: {name}"),
        }
    }
}

/// What the Jira extension says of Jira (`jira.config`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    /// Where Jira is; none when it isn't set up.
    pub base_url: Option<String>,
    /// The projects tracked: their keys.
    pub projects: Vec<String>,
    /// People to assign to: (username, name).
    pub users: Vec<(String, String)>,
    /// Jira priority -> agenda priority.
    pub priorities: Vec<(String, String)>,
    /// Project -> what Blocked means there.
    pub blocked: Vec<(String, String)>,
    pub sync_minutes: u32,
}

impl Config {
    pub fn blocked_for(&self, project: &str) -> Option<Blocked> {
        self.blocked.iter().find(|(p, _)| p == project).and_then(|(_, meaning)| Blocked::parse(meaning).ok())
    }

    /// Whether Blocked means the Flagged field in any project.
    pub fn flags_anywhere(&self) -> bool {
        self.blocked.iter().any(|(_, meaning)| Blocked::parse(meaning) == Ok(Blocked::Flag))
    }

    /// `jira.priorities` first, then a guess from the name.
    pub fn priority(&self, name: &str) -> Priority {
        self.priorities
            .iter()
            .find(|(jira, _)| jira.eq_ignore_ascii_case(name))
            .and_then(|(_, level)| Priority::ALL.into_iter().find(|p| p.label().eq_ignore_ascii_case(level)))
            .unwrap_or_else(|| model::jira::guess_priority(name))
    }

    /// Which agenda column a Jira status lands in.
    pub fn status_for(&self, project: &str, status_id: &str, category: &str, flagged: bool) -> Status {
        match self.blocked_for(project) {
            Some(Blocked::Status { id, .. }) if id == status_id => Status::Blocked,
            Some(Blocked::Flag) if flagged => Status::Blocked,
            _ => model::jira::status_for_category(category),
        }
    }
}

/// The sync's bookkeeping: none of it worth saving.
#[derive(Default)]
pub struct Sync {
    /// What the Jira extension said of Jira; `None` until it has, and
    /// `Some(Err)` when there's no Jira extension.
    pub config: Option<Result<Config, String>>,
    /// The op being sent -- one at a time, in order, so a title change
    /// can't overtake the transition queued before it.
    pub in_flight: Option<u64>,
    pub pulling: bool,
    /// Pull again once the outbox empties (a comment was posted).
    pub pull_after_drain: bool,
    pub last_pull: Option<Instant>,
    /// The token owner's username, for "still assigned to you".
    pub me: Option<String>,
    /// The Flagged field's id: `None` until asked, `Some(None)` when the
    /// instance has none.
    pub flag_field: Option<Option<String>>,
    pub sending_worklogs: bool,
    /// The periodic sync's timer.
    pub ticker: Option<u64>,
}

impl Sync {
    /// Jira's settings, once the Jira extension has said them.
    pub fn config(&self) -> Option<&Config> {
        self.config.as_ref().and_then(|c| c.as_ref().ok()).filter(|c| c.base_url.is_some())
    }

    /// The Flagged field id to ask for, when any project uses it.
    pub fn flag_wanted(&self) -> Option<String> {
        let wanted = self.config().is_some_and(Config::flags_anywhere);
        if wanted {
            self.flag_field.clone().flatten()
        } else {
            None
        }
    }
}

/// What a picker of the sync's is for.
#[derive(Debug, Clone)]
pub struct PickerCtx {
    pub task: Option<TaskId>,
    pub project: Option<String>,
    /// Where the task's card was before a board move this picker is
    /// finishing -- cancelling puts it back.
    pub revert: Option<(Status, i64)>,
    pub rows: Vec<Pick>,
}

/// One row of a sync picker.
#[derive(Debug, Clone)]
pub enum Pick {
    Issue(Box<IssueDetail>),
    CreateIssue,
    TypeKey,
    Unlink,
    OpenInBrowser,
    Import(Box<IssueDetail>),
    Transition(Transition),
    /// The "Blocked..." row of a linked task's status picker.
    StartBlocked,
    /// What Blocked means in a project, and -- when chosen while blocking
    /// a task -- the transition that gets there.
    Blocked(Blocked, Option<Transition>),
    Priority(String),
    Assignee(String, String),
}

/// What an issue fetch was for.
#[derive(Debug, Clone)]
pub enum IssueFetch {
    LinkPicker(TaskId),
    Import,
    LinkKey(TaskId),
    Created(TaskId),
    AddFromJira { clock: bool },
}

/// How sending one worklog row went.
#[derive(Debug)]
pub struct WorklogSent {
    pub task: TaskId,
    pub entries: Vec<usize>,
    pub key: String,
    pub minutes: i64,
    pub result: Result<(), String>,
}

/// A job's end, for the agenda to take in.
#[derive(Debug)]
pub enum Done {
    Config(Result<Config, String>),
    Pulled { keys: Vec<String>, quiet: bool, me: Option<String>, flag_field: Option<Option<String>>, result: Result<Vec<IssueDetail>, String> },
    OpDone { op_id: u64, key: String, what: String, flag_field: Option<Option<String>>, result: Result<(), String> },
    Issues { purpose: IssueFetch, result: Result<Vec<IssueDetail>, String> },
    Transitions { task: TaskId, target: Option<Status>, prev: (Status, i64), result: Result<Vec<Transition>, String> },
    Priorities { task: TaskId, result: Result<Vec<fenix_jira_client::Priority>, String> },
    ProjectStatuses { project: String, result: Result<Vec<StatusInfo>, String> },
    WorklogsSent(Vec<WorklogSent>),
    /// The Jira extension's new-issue form, filled in from a task: the
    /// key it made, or why not.
    Created { task: TaskId, result: Result<String, String> },
    /// A setting the Jira extension changed for it.
    Saved(Result<(), String>),
}

thread_local! {
    static DONE: RefCell<Vec<Done>> = const { RefCell::new(Vec::new()) };
}

/// Jobs that have ended since the agenda last looked.
pub fn take_done() -> Vec<Done> {
    DONE.with(|d| std::mem::take(&mut *d.borrow_mut()))
}

/// Runs `job`; what it ends with comes to `take_done`.
pub fn spawn(job: impl Future<Output = Done> + 'static) {
    run::spawn(async move {
        let done = job.await;
        DONE.with(|d| d.borrow_mut().push(done));
    });
}

/// `jira.config`: Jira's settings, from the Jira extension.
pub fn fetch_config() {
    spawn(async {
        let config = run::call("jira.config", "null").await.and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()));
        Done::Config(config)
    });
}

/// `project`'s Blocked meaning, kept in `jira.blocked` by the Jira
/// extension.
pub fn save_blocked(project: &str, blocked: &Blocked) {
    let payload = serde_json::json!({ "project": project, "blocked": blocked.text() }).to_string();
    spawn(async move { Done::Saved(run::call("jira.set_blocked", &payload).await.map(|_| ())) });
}

/// Every linked task's issue, in searches of 50.
pub fn pull(keys: Vec<String>, quiet: bool, need_me: bool, want_flag: bool, known_flag: Option<Option<String>>) {
    spawn(async move {
        let client = client();
        let me = if need_me { client.myself().await.ok() } else { None };
        let flag_field = match (&known_flag, want_flag) {
            (None, true) => client.find_flagged_field().await.ok(),
            _ => None,
        };
        let flag = flag_field.clone().or(known_flag).flatten();
        let mut issues = Vec::new();
        for chunk in keys.chunks(50) {
            match client.search_details(&fenix_jira_client::build_keys_jql(chunk), chunk.len() as u32, flag.as_deref()).await {
                Ok(found) => issues.extend(found),
                Err(err) => return Done::Pulled { keys, quiet, me, flag_field, result: Err(err) },
            }
        }
        Done::Pulled { keys, quiet, me, flag_field, result: Ok(issues) }
    });
}

/// One op from the outbox, sent.
pub fn send_op(op: model::PendingOp, key: String, known_flag: Option<Option<String>>) {
    spawn(async move {
        let client = client();
        let mut fetched_flag = None;
        let result = match &op.kind {
            OpKind::SetSummary(s) => client.update_summary(&key, s).await,
            OpKind::SetDescription(s) => client.update_description(&key, s).await,
            OpKind::SetPriority(p) => client.update_priority(&key, p).await,
            OpKind::AddComment(body) => client.add_comment(&key, body).await,
            OpKind::SetAssignee { id, .. } => client.update_assignee(&key, id).await,
            OpKind::Transition { id, .. } => client.apply_transition(&key, id).await,
            OpKind::SetDue(due) => client.update_due(&key, due.as_deref()).await,
            OpKind::SetFlag(on) => {
                let field = match known_flag {
                    Some(field) => Ok(field),
                    None => client.find_flagged_field().await.inspect(|f| fetched_flag = Some(f.clone())),
                };
                match field {
                    Ok(Some(field)) => client.set_flagged(&key, &field, *on).await,
                    Ok(None) => Err("this Jira has no Flagged field -- press b on the project's search on the Jira page (SPC j j) to pick another meaning for Blocked".to_string()),
                    Err(err) => Err(err),
                }
            }
        };
        Done::OpDone { op_id: op.id, key, what: op.kind.describe(), flag_field: fetched_flag, result }
    });
}

pub fn transitions(task: TaskId, key: String, target: Option<Status>, prev: (Status, i64)) {
    spawn(async move { Done::Transitions { task, target, prev, result: client().list_transitions(&key).await } });
}

pub fn priorities(task: TaskId) {
    spawn(async move { Done::Priorities { task, result: client().list_priorities().await } });
}

pub fn search(purpose: IssueFetch, jql: String, most: u32, flag: Option<String>) {
    spawn(async move { Done::Issues { purpose, result: client().search_details(&jql, most, flag.as_deref()).await } });
}

pub fn issue(purpose: IssueFetch, key: String, flag: Option<String>) {
    spawn(async move { Done::Issues { purpose, result: client().get_issue_with(&key, flag.as_deref()).await.map(|d| vec![d]) } });
}

pub fn project_statuses(project: String) {
    spawn(async move {
        let result = client().list_project_statuses(&project).await;
        Done::ProjectStatuses { project, result }
    });
}

pub fn worklogs(rows: Vec<WorklogRow>) {
    spawn(async move {
        let client = client();
        let mut results = Vec::new();
        for row in rows {
            let started = row.started.format("%Y-%m-%dT%H:%M:%S%.3f%z").to_string();
            let result = client.add_worklog_at(&row.key, row.minutes * 60, &started).await;
            results.push(WorklogSent { task: row.task, entries: row.entries, key: row.key, minutes: row.minutes, result });
        }
        Done::WorklogsSent(results)
    });
}

/// The Jira extension's new-issue form, filled in from a task: answered
/// once you've made the issue, or gone.
pub fn new_issue(task: TaskId, summary: String, description: String) {
    let payload = serde_json::json!({ "summary": summary, "description": description }).to_string();
    spawn(async move { Done::Created { task, result: run::call("jira.new_issue", &payload).await } });
}

/// An issue as the agenda keeps it.
pub fn remote_update(config: &Config, me: Option<&str>, d: &IssueDetail) -> RemoteUpdate {
    let project = fenix_jira_client::project_of(&d.key);
    RemoteUpdate {
        snapshot: RemoteSnapshot {
            summary: d.summary.clone(),
            description: d.description.clone().unwrap_or_default(),
            status_id: d.status_id.clone(),
            status_name: d.status.clone(),
            status_category: d.status_category.clone(),
            priority: d.priority.clone(),
            assignee: d.assignee.clone(),
            assignee_id: d.assignee_id.clone(),
            flagged: d.flagged,
            updated: d.updated.clone(),
            due: d.due.clone(),
            comments: d.comments.iter().map(|c| RemoteComment { id: c.id.clone(), author: c.author.clone(), body: c.body.clone(), created: c.created.clone() }).collect(),
        },
        status: config.status_for(project, &d.status_id, &d.status_category, d.flagged),
        priority: d.priority.as_deref().map(|p| config.priority(p)).unwrap_or(Priority::Medium),
        mine: me.map(|me| d.assignee_id.as_deref() == Some(me)),
    }
}

pub fn transition_label(t: &Transition) -> String {
    if t.to_name.is_empty() || t.to_name == t.name {
        t.name.clone()
    } else {
        format!("{}  ->  {}", t.name, t.to_name)
    }
}

pub fn issue_label(d: &IssueDetail) -> String {
    format!("{}  {}  [{}]", d.key, d.summary, d.status)
}

/// Pickers the sync has open, by the id Fenix gave each.
pub type Pickers = HashMap<u64, PickerCtx>;
