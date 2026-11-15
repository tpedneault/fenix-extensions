//! Keeping linked tasks and their issues in step, through the tracker
//! seam (`plans/tracker-seam.md`): finding the trackers, fetching issues
//! and turning them into `RemoteUpdate`s, sending the outbox one op at a
//! time, turning a board move into one of the tracker's moves, linking
//! and importing issues, and sending worklogs.
//!
//! Each tracker is its own extension's: it holds your sign-in and its
//! settings, and answers `<id>.tracker.*`. Each job here runs as `async`
//! (`crate::calls`) and ends with a [`Done`], which the agenda takes in
//! once the job's answer has come (`Agenda::take_done`).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::time::{Duration, Instant};

use fenix_tracker::{self as seam, call, code, Issue, Move, Moves, SeamError};

use crate::calls;
use crate::model::{self, OpKind, PendingOp, RemoteComment, RemoteSnapshot, RemoteUpdate, Status, TaskId, WorklogRow};
use crate::trackers::{self, Found, Tracker, Trackers, Via};

/// Shown on a linked task whose issue a sync couldn't find.
pub fn not_found(tracker: &str) -> String {
    format!("not found in {tracker} -- deleted, moved, or no longer visible to you")
}

/// How long a sync stays fresh enough that opening the agenda doesn't
/// start another one.
pub const STALE_AFTER: Duration = Duration::from_secs(120);

/// How long the trackers found stay found before the page's opening asks
/// again -- one installed while Fenix runs is found by then.
pub const PROBE_AFTER: Duration = Duration::from_secs(300);

/// One sync, across the trackers it asked: what each said, told once the
/// last has answered.
#[derive(Debug, Default)]
pub struct Round {
    pub quiet: bool,
    /// Trackers still to answer.
    pub waiting: usize,
    /// (tracker, issues brought up to date), for each that synced.
    pub synced: Vec<(String, usize)>,
    pub conflicts: usize,
    /// (tracker, why) for each that failed.
    pub failures: Vec<(String, String)>,
}

/// What a command waiting on the trackers' answer will do.
pub type Then = Box<dyn FnOnce(&mut crate::Agenda)>;

/// The sync's bookkeeping: none of it worth saving.
#[derive(Default)]
pub struct Sync {
    pub trackers: Trackers,
    /// Whether the trackers have been asked, and when.
    pub probed: Option<Instant>,
    /// Asks under way, and what they've found so far, in the order asked.
    pub probing: Vec<(String, Option<Found>)>,
    /// Asked again once the asking under way is done.
    pub probe_again: bool,
    /// Commands waiting for the asking to be done.
    pub waiting: Vec<Then>,
    /// The op being sent -- one at a time, in order, whichever tracker
    /// it's for, so a title change can't overtake the move queued before
    /// it.
    pub in_flight: Option<u64>,
    pub round: Option<Round>,
    /// Pull again once the outbox empties (a comment was posted).
    pub pull_after_drain: bool,
    pub last_pull: Option<Instant>,
    pub sending_worklogs: bool,
    /// Trackers said to be unusable, said once.
    pub warned: HashSet<String>,
}

impl Sync {
    pub fn pulling(&self) -> bool {
        self.round.is_some()
    }

    /// Whether any tracker is ready.
    pub fn any_ready(&self) -> bool {
        self.trackers.ready().next().is_some()
    }

    /// A ready tracker, by id.
    pub fn ready(&self, id: &str) -> Option<&Tracker> {
        self.trackers.get(id).filter(|t| t.ready())
    }

    pub fn is_probing(&self) -> bool {
        !self.probing.is_empty()
    }
}

/// What a picker of the sync's is for.
#[derive(Debug, Clone)]
pub struct PickerCtx {
    pub task: Option<TaskId>,
    /// The tracker its rows are of, when one.
    pub tracker: Option<String>,
    /// Where the task's card was before a board move this picker is
    /// finishing -- cancelling puts it back.
    pub revert: Option<(Status, i64)>,
    pub rows: Vec<Pick>,
}

/// One row of a sync picker.
#[derive(Debug, Clone)]
pub enum Pick {
    /// An issue to link to, and its tracker.
    Issue(String, Box<Issue>),
    /// The tracker's new-issue form.
    CreateIssue(String),
    TypeKey,
    Unlink,
    OpenInBrowser,
    Import(String, Box<Issue>),
    Move(Box<Move>),
    /// The "Blocked" row of a linked task's status picker.
    StartBlocked,
    Priority(seam::Priority),
    /// Someone, or nobody.
    Assignee(Option<seam::Person>),
    /// A change that may have been made: send it again, or forget it.
    SendAgain,
    Forget,
}

/// What an issue fetch was for.
#[derive(Debug, Clone)]
pub enum IssueFetch {
    LinkPicker(TaskId),
    Import,
    LinkKey(TaskId),
    Created(TaskId),
    AddIssue { clock: bool },
}

/// How sending one worklog row went.
#[derive(Debug)]
pub struct WorklogSent {
    pub task: TaskId,
    pub entries: Vec<usize>,
    pub key: String,
    pub minutes: i64,
    pub result: Result<(), SeamError>,
}

/// A job's end, for the agenda to take in.
#[derive(Debug)]
pub enum Done {
    /// What asking one tracker found.
    Probed { id: String, found: Found },
    /// One tracker's part of a sync.
    Pulled { tracker: String, asked: Vec<String>, result: Result<seam::Found, SeamError> },
    OpDone { op_id: u64, key: String, tracker: String, what: String, result: Result<Option<String>, SeamError> },
    /// Issues to pick from, each with its tracker: those assigned to
    /// you, or the one asked for.
    Issues { purpose: IssueFetch, found: Vec<(String, Issue)>, failures: Vec<(String, SeamError)> },
    Transitions { task: TaskId, target: Option<Status>, prev: (Status, i64), result: Result<Moves, SeamError> },
    Priorities { task: TaskId, result: Result<Vec<seam::Priority>, SeamError> },
    People { task: TaskId, result: Result<seam::People, SeamError> },
    WorklogsSent(Vec<WorklogSent>),
    /// The tracker's new-issue form, filled in from a task: the ref it
    /// made, or what it said it opened, or why not.
    Created { task: TaskId, tracker: String, result: Result<seam::Created, SeamError> },
    /// A choice a move carried, kept by the tracker.
    Remembered { tracker: String, result: Result<(), SeamError> },
    /// The project you're in, as each tracker that can tell names it.
    Projects(Vec<(String, String)>),
}

impl std::fmt::Debug for Tracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Tracker({})", self.id)
    }
}

impl std::fmt::Debug for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Found::Tracker(t) => write!(f, "Found({})", t.id),
            Found::Other { id, .. } => write!(f, "Other({id})"),
            Found::Impostor { id, .. } => write!(f, "Impostor({id})"),
            Found::Broken { id, .. } => write!(f, "Broken({id})"),
            Found::Absent => write!(f, "Absent"),
        }
    }
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
    calls::spawn(async move {
        let done = job.await;
        DONE.with(|d| d.borrow_mut().push(done));
    });
}

/// Asks `id` who it is, beside the others: one that hangs holds up none.
pub fn probe(id: String) {
    spawn(async move {
        let found = trackers::probe(id.clone()).await;
        Done::Probed { id, found }
    });
}

/// Issues `asked` (ids, or refs for links that don't know theirs), one
/// call each, merged.
async fn one_by_one(via: &Via, asked: &[String]) -> Result<seam::Found, SeamError> {
    let mut found = seam::Found::default();
    for one in asked {
        match via.get::<seam::Found>(call::ISSUES, &seam::Issues::new(vec![one.clone()])).await {
            Ok(part) => {
                let part = part.cleaned();
                found.issues.extend(part.issues);
                found.missing.extend(part.missing);
            }
            Err(e) if e.is(code::NOT_FOUND) => found.missing.push(one.clone()),
            Err(e) => return Err(e),
        }
    }
    Ok(found)
}

/// One tracker's linked issues, in calls of 50. A tracker that answers
/// `not_found` for a whole call of several is asked one at a time.
pub fn pull(via: Via, asked: Vec<String>) {
    spawn(async move {
        let mut found = seam::Found::default();
        for chunk in asked.chunks(seam::Issues::MAX_REFS) {
            let part = match via.get::<seam::Found>(call::ISSUES, &seam::Issues::new(chunk.to_vec())).await {
                Ok(part) => Ok(part.cleaned()),
                Err(e) if e.is(code::NOT_FOUND) => one_by_one(&via, chunk).await,
                Err(e) => Err(e),
            };
            match part {
                Ok(part) => {
                    found.issues.extend(part.issues);
                    found.missing.extend(part.missing);
                }
                Err(err) => return Done::Pulled { tracker: via.id, asked, result: Err(err) },
            }
        }
        Done::Pulled { tracker: via.id, asked, result: Ok(found) }
    });
}

/// One op from the outbox, sent, with its `op`.
pub fn send_op(via: Via, op: PendingOp, key: String) {
    spawn(async move {
        let uid = (!op.uid.is_empty()).then(|| op.uid.clone());
        let update = |change: seam::Change| seam::Update::new(&key, &change, uid.clone());
        let transition = |id: String| seam::Transition { seam: seam::SEAM, key: key.clone(), id, op: uid.clone() };
        let result = match &op.kind {
            OpKind::SetSummary(s) => via.put::<seam::Said>(call::UPDATE, &update(seam::Change::Title(s.clone()))).await,
            OpKind::SetDescription(s) => via.put::<seam::Said>(call::UPDATE, &update(seam::Change::Body(s.clone()))).await,
            OpKind::SetPriority(p) => via.put::<seam::Said>(call::UPDATE, &update(seam::Change::Priority(p.clone()))).await,
            OpKind::SetDue(due) => via.put::<seam::Said>(call::UPDATE, &update(seam::Change::Due(due.clone()))).await,
            OpKind::SetAssignee { id, name } => {
                let who = (!id.is_empty()).then(|| seam::Person { id: id.clone(), name: name.clone() });
                via.put::<seam::Said>(call::UPDATE, &update(seam::Change::Assignee(who))).await
            }
            OpKind::AddComment(body) => via.put::<seam::Said>(call::COMMENT, &seam::NewComment { seam: seam::SEAM, key: key.clone(), body: body.clone(), op: uid.clone() }).await,
            OpKind::Transition { id, .. } => via.put::<seam::Said>(call::TRANSITION, &transition(id.clone())).await,
            OpKind::SetFlag(on) => via.put::<seam::Said>(call::TRANSITION, &transition(crate::legacy::flag_move(*on).into())).await,
        };
        let result = result.map(|said| said.said.map(|s| fenix_tracker::clean::message(&s)));
        Done::OpDone { op_id: op.id, key, tracker: via.id, what: op.kind.describe(), result }
    });
}

/// What can be done next with `key`, toward `target` when a card moved.
pub fn transitions(via: Via, task: TaskId, key: String, from: seam::State, target: Option<Status>, prev: (Status, i64)) {
    spawn(async move {
        let req = seam::Transitions { seam: seam::SEAM, key, toward: target.map(|s| model::link::column_for_status(s).to_string()), from: Some(from) };
        let result = via.get::<Moves>(call::TRANSITIONS, &req).await.map(Moves::cleaned);
        Done::Transitions { task, target, prev, result }
    });
}

pub fn priorities(via: Via, task: TaskId, key: String) {
    spawn(async move {
        let result = via.get::<seam::Priorities>(call::PRIORITIES, &seam::One { seam: seam::SEAM, key }).await.map(|p| p.cleaned().priorities);
        Done::Priorities { task, result }
    });
}

pub fn people(via: Via, task: TaskId, key: String) {
    spawn(async move {
        let result = via.get::<seam::People>(call::PEOPLE, &seam::One { seam: seam::SEAM, key }).await.map(seam::People::cleaned);
        Done::People { task, result }
    });
}

/// Every ready tracker's open issues assigned to you, up to `most` each,
/// a page at a time; each issue once, however the pages drift.
pub fn assigned(purpose: IssueFetch, vias: Vec<Via>, most: u32) {
    spawn(async move {
        let (mut found, mut failures) = (Vec::new(), Vec::new());
        for via in vias {
            let mut cursor: Option<String> = None;
            let mut seen: HashSet<String> = HashSet::new();
            let mut got = 0u32;
            loop {
                let limit = (most - got).min(seam::Assigned::MAX_LIMIT);
                let page = match via.get::<seam::Page>(call::ASSIGNED, &seam::Assigned::new(limit, cursor.clone())).await.and_then(seam::Page::cleaned) {
                    Ok(page) => page,
                    Err(err) => {
                        failures.push((via.id.clone(), err));
                        break;
                    }
                };
                if page.issues.is_empty() {
                    break;
                }
                got += page.issues.len() as u32;
                for issue in page.issues {
                    if seen.insert(issue.id.clone()) {
                        found.push((via.id.clone(), issue));
                    }
                }
                match page.next {
                    Some(next) if got < most && cursor.as_ref() != Some(&next) => cursor = Some(next),
                    _ => break,
                }
            }
        }
        Done::Issues { purpose, found, failures }
    });
}

/// Issue `key`, from the first of `vias` that has it.
pub fn issue(purpose: IssueFetch, vias: Vec<Via>, key: String) {
    spawn(async move {
        let mut failures = Vec::new();
        for via in vias {
            match via.get::<seam::Found>(call::ISSUES, &seam::Issues::new(vec![key.clone()])).await {
                Ok(found) => {
                    if let Some(issue) = found.cleaned().issues.into_iter().next() {
                        return Done::Issues { purpose, found: vec![(via.id, issue)], failures: Vec::new() };
                    }
                    failures.push((via.id.clone(), SeamError::new(code::NOT_FOUND, format!("{} returned no issue", via.name))));
                }
                Err(err) => failures.push((via.id.clone(), err)),
            }
        }
        Done::Issues { purpose, found: Vec::new(), failures }
    });
}

pub fn worklogs(rows: Vec<(Via, WorklogRow, String)>) {
    spawn(async move {
        let mut results = Vec::new();
        for (via, row, op) in rows {
            let started = row.started.to_rfc3339_opts(chrono::SecondsFormat::Millis, false);
            let req = seam::Worklog { seam: seam::SEAM, key: row.key.clone(), seconds: row.minutes * 60, started, op: Some(op) };
            let result = via.write(call::WORKLOG, &req).await.map(|_| ());
            results.push(WorklogSent { task: row.task, entries: row.entries, key: row.key, minutes: row.minutes, result });
        }
        Done::WorklogsSent(results)
    });
}

/// The tracker's new-issue form, filled in from a task: answered once
/// you've made the issue, or at once with what it opened.
pub fn create(via: Via, task: TaskId, title: String, body: String, op: String) {
    spawn(async move {
        let req = seam::Create { seam: seam::SEAM, title, body, op: Some(op) };
        let result = match via.ask_unbounded(call::CREATE, &req).await {
            Ok(text) => via.read::<seam::Created>(call::CREATE, &text).map(|c| seam::Created { key: c.key.and_then(|k| fenix_tracker::clean::key(&k)), opened: c.opened.map(|o| fenix_tracker::clean::message(&o)) }),
            Err(err) => Err(err),
        };
        Done::Created { task, tracker: via.id, result }
    });
}

/// A choice a move carried, for the tracker to keep.
pub fn remember(via: Via, key: String, remember: String, op: String) {
    spawn(async move {
        let result = via.write(call::REMEMBER, &seam::Remember { seam: seam::SEAM, key, remember, op: Some(op) }).await.map(|_| ());
        Done::Remembered { tracker: via.id, result }
    });
}

/// The project you're in, asked of each of `vias` (those that list
/// `project`).
pub fn projects(vias: Vec<Via>) {
    spawn(async move {
        let mut found = Vec::new();
        for via in vias {
            if let Ok(p) = via.get::<seam::Project>(call::PROJECT, &seam::Hello::default()).await {
                if let Some(project) = p.project.as_deref().map(fenix_tracker::clean::name).filter(|p| !p.is_empty()) {
                    found.push((via.id, project));
                }
            }
        }
        Done::Projects(found)
    });
}

/// An issue as the agenda keeps it.
pub fn remote_update(i: &Issue) -> RemoteUpdate {
    RemoteUpdate {
        snapshot: RemoteSnapshot {
            summary: i.title.clone(),
            description: i.body.clone(),
            status_id: i.state.id.clone(),
            status_name: i.state.name.clone(),
            status_category: i.state.stage.clone(),
            column: Some(i.state.column.clone()),
            priority: i.priority.as_ref().map(|p| p.name.clone()),
            assignee: i.assignee.as_ref().map(|p| p.name.clone()).filter(|n| !n.is_empty()),
            assignee_id: i.assignee.as_ref().map(|p| p.id.clone()).filter(|n| !n.is_empty()),
            flagged: false,
            updated: i.updated.clone(),
            due: i.due.clone(),
            comments: i.comments.iter().map(|c| RemoteComment { id: c.id.clone(), author: c.author.clone(), body: c.body.clone(), created: c.created.clone() }).collect(),
            url: i.url.clone(),
            project: i.project.clone(),
            description_cut: i.body_cut,
        },
        status: model::link::status_for_column(&i.state.column),
        priority: model::link::priority_for_level(i.priority.as_ref().and_then(|p| p.level.as_deref())),
        mine: i.mine,
    }
}

/// The state the agenda last saw of a link, as `transitions` takes it.
pub fn seen(base: &RemoteSnapshot) -> seam::State {
    seam::State { id: base.status_id.clone(), name: base.status_name.clone(), stage: base.status_category.clone(), column: base.column.clone().unwrap_or_default() }
}

/// An issue as a picker's row, with its tracker's name when several are
/// listed.
pub fn issue_label(i: &Issue, tracker: Option<&str>) -> String {
    let label = format!("{}  {}  [{}]", i.key, i.title, i.state.name);
    match tracker {
        Some(name) => format!("{label}  · {name}"),
        None => label,
    }
}

/// Pickers the sync has open, by the id Fenix gave each.
pub type Pickers = HashMap<u64, PickerCtx>;
