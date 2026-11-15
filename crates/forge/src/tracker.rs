//! The tracker seam (`plans/tracker-seam.md`, seam 1 as changed after
//! review) served for GitHub Issues and GitLab Issues over an [`Issues`]
//! client: the issues assigned to you, these issues (by id, ref or
//! address), their moves -- GitHub's close as completed, close as not
//! planned and reopen; GitLab's close and reopen and its scoped
//! `workflow::` labels; `local` moves for the columns a forge doesn't
//! model -- and the agenda's writes.
//!
//! The extensions serve it as `fenix.github-issues.tracker.*` and
//! `fenix.gitlab-issues.tracker.*` over their own HTTP. They own what
//! needs Fenix: `about`'s readiness, `project` (the git remote), `create`
//! (their page's form), and the gate and `op` memory in front of every
//! write ([`Seam::write`] is the one door writes go through here).
//!
//! Feature `tracker`.

use std::cell::RefCell;
use std::rc::Rc;

use fenix_tracker::{self as seam, can, code, column, Change as SeamChange, SeamError};

use crate::issues::{ApiError, Change, Host, Issue, IssueRef, Issues, Paging, Search, User};

/// The trackers' extension ids, which their services start with.
pub fn provider(host: Host) -> &'static str {
    match host {
        Host::GitHub => "fenix.github-issues",
        Host::GitLab => "fenix.gitlab-issues",
    }
}

/// What the extension's settings say, and whether it's signed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Signed in, and the server known.
    pub ready: bool,
    /// What to do about it when it isn't, with the keys.
    pub not_ready: String,
    pub sync_minutes: u32,
    /// The scope of GitLab's scoped labels that are its workflow:
    /// `workflow` for `workflow::Doing`; none when empty.
    pub workflow_scope: String,
    /// Time from the agenda logged on the issue (GitLab).
    pub worklog: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { ready: false, not_ready: String::new(), sync_minutes: 10, workflow_scope: "workflow".into(), worklog: false }
    }
}

/// What it learns once and keeps: who you are, and each project's labels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Learned {
    pub me: Option<User>,
    pub labels: Vec<(String, Vec<String>)>,
}

/// What `host` serves, by capability: per field, and no priority on
/// either; no due date, worklog or Blocked of its own on GitHub.
pub fn capabilities(host: Host, settings: &Settings) -> Vec<&'static str> {
    let mut caps = vec![can::COMMENTS, can::EDIT_TITLE, can::EDIT_BODY, can::EDIT_ASSIGNEE, can::EDIT_LABELS, can::COMMENT, can::CREATE, can::PROJECT];
    if host == Host::GitLab {
        caps.insert(3, can::EDIT_DUE);
        if settings.worklog {
            caps.push(can::WORKLOG);
        }
    }
    caps
}

/// `about` for `host`: answered whatever seam it's asked with, and
/// without asking the forge anything.
pub fn about(host: Host, settings: &Settings) -> seam::About {
    seam::About {
        seam: seam::SEAM,
        seams: seam::SEAMS.to_vec(),
        minor: seam::MINOR,
        id: provider(host).into(),
        name: host.name().into(),
        ready: settings.ready,
        not_ready: (!settings.ready).then(|| settings.not_ready.clone()),
        can: capabilities(host, settings).into_iter().map(str::to_string).collect(),
        sync_minutes: settings.sync_minutes.max(5),
        project_meta: None,
        // A forge's address isn't its ref in a template: each issue
        // carries its `url`.
        browse: None,
        ref_hint: Some(match host {
            Host::GitHub => "owner/repo#123".into(),
            Host::GitLab => "group/project#123".into(),
        }),
        // GitHub has no Blocked; GitLab's workflow may.
        blocked: host == Host::GitLab,
    }
}

/// A failed request as the seam's error, the forge's own words kept
/// (§3.3): 401 signed out; 404 and 410 not found; a rate limit, however
/// it came (429, GitHub's 403s), with its wait; 502, 503 and 504 offline;
/// other 4xx refused, other 5xx failed; nothing back offline.
pub fn classify(e: ApiError) -> SeamError {
    let code = match (e.status, e.retry_after) {
        (None, _) => code::OFFLINE,
        (_, Some(_)) => code::RATE_LIMITED,
        (Some(401), _) => code::SIGNED_OUT,
        (Some(404 | 410), _) => code::NOT_FOUND,
        // Transient: tried again by the next sync, never dropped.
        (Some(408 | 425 | 502..=504), _) => code::OFFLINE,
        // A write sent elsewhere wasn't made: `r` tries again once the
        // issue's place is known.
        (Some(300..=399), _) => code::REFUSED,
        (Some(s), _) if (400..500).contains(&s) => code::REFUSED,
        _ => code::FAILED,
    };
    let mut err = SeamError::new(code, e.message);
    err.retry_after = e.retry_after;
    err
}

type Answer = Result<String, SeamError>;

fn json<T: serde::Serialize>(value: &T) -> Answer {
    serde_json::to_string(value).map_err(|e| SeamError::new(code::FAILED, e.to_string()))
}

/// How far along a workflow step's name says an issue is: its stage,
/// and its column (`blocked` for a step that says so).
pub fn workflow_stage(step: &str) -> (&'static str, &'static str) {
    let s = step.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| s.contains(w));
    if has(&["block", "on hold", "waiting"]) {
        (column::DOING, column::BLOCKED)
    } else if has(&["done", "complete", "released", "shipped", "closed", "resolved"]) {
        (column::DONE, column::DONE)
    } else if has(&["todo", "to do", "backlog", "triage", "ready", "new", "open", "planning", "refinement"]) {
        (column::TODO, column::TODO)
    } else {
        (column::DOING, column::DOING)
    }
}

/// The seam over one forge's issues.
pub struct Seam<C> {
    pub client: C,
    pub settings: Settings,
    pub learned: Rc<RefCell<Learned>>,
}

impl<C: Issues> Seam<C> {
    pub fn new(client: C, settings: Settings, learned: Rc<RefCell<Learned>>) -> Self {
        Seam { client, settings, learned }
    }

    fn host(&self) -> Host {
        self.client.host()
    }

    /// `owner/repo#12`, or an issue's address on this forge.
    pub fn parse_ref(&self, r: &str) -> Option<IssueRef> {
        IssueRef::parse(r, self.host(), self.client.web())
    }

    /// An id, a ref or an address as the issue it names: an id asked of
    /// the forge (the issue may have moved), the others read as written,
    /// checked against the forge's own form before reaching a URL.
    async fn locate(&self, r: &str) -> Result<IssueRef, SeamError> {
        if let Some(key) = self.parse_ref(r) {
            return Ok(key);
        }
        if self.client.is_id(r) {
            return self.client.resolve(r.trim()).await.map_err(classify);
        }
        let hint = match self.host() {
            Host::GitHub => "owner/repo#12, its id, or the issue's address",
            Host::GitLab => "group/project#12, its id, or the issue's address",
        };
        Err(SeamError::new(code::BAD_REQUEST, format!("\"{r}\" isn't an issue -- {hint}")))
    }

    /// Where a write goes: the issue as it is now, found by reading it --
    /// GitHub's read follows a transferred issue to its repository (a
    /// GET, so following is safe), and GitLab's moved issue says where it
    /// went (`moved_to_id`, found by its global id). A write never relies
    /// on a redirect: Fenix would turn one into a bodiless GET.
    async fn locate_for_write(&self, r: &str) -> Result<IssueRef, SeamError> {
        let key = self.locate(r).await?;
        let issue = self.client.issue(&key).await.map_err(classify)?;
        match &issue.moved_to {
            Some(id) => self.client.resolve(id).await.map_err(classify),
            None => Ok(issue.key),
        }
    }

    /// The scoped label prefix, `workflow::`; none on GitHub or when
    /// unset.
    fn scope(&self) -> Option<String> {
        let s = self.settings.workflow_scope.trim();
        (self.host() == Host::GitLab && !s.is_empty()).then(|| format!("{s}::"))
    }

    /// The workflow step an issue is at: its scoped label's value.
    fn step<'a>(&self, issue: &'a Issue) -> Option<&'a str> {
        let scope = self.scope()?;
        issue.labels.iter().find_map(|l| l.get(..scope.len()).filter(|p| p.eq_ignore_ascii_case(&scope)).map(|_| &l[scope.len()..]))
    }

    fn open_id(&self) -> &'static str {
        if self.host() == Host::GitHub {
            "open"
        } else {
            "opened"
        }
    }

    /// Where an issue stands, as the seam has it: GitHub open `todo`,
    /// closed `done`; GitLab the same unless its workflow step says more.
    pub fn state(&self, issue: &Issue) -> seam::State {
        if !issue.open {
            let (id, name) = match issue.state_reason.as_deref() {
                Some("not_planned") => ("closed:not_planned", "Closed as not planned"),
                Some("duplicate") => ("closed:duplicate", "Closed as a duplicate"),
                _ => ("closed", "Closed"),
            };
            return self.to(id, name, column::DONE, column::DONE);
        }
        match self.step(issue) {
            Some(step) => {
                let (stage, col) = workflow_stage(step);
                self.to(&format!("opened:{step}"), step, stage, col)
            }
            None => self.to(self.open_id(), "Open", column::TODO, column::TODO),
        }
    }

    /// One issue as the seam has it, its comments (people's, not the
    /// forge's narration) with it.
    pub fn issue(&self, issue: &Issue, notes: &[crate::issues::Note], me: Option<&User>, asked: Option<&str>) -> Option<seam::Issue> {
        seam::Issue {
            id: issue.id.clone(),
            key: issue.key.key(),
            asked: asked.map(str::to_string),
            url: Some(issue.url.clone()).filter(|u| !u.is_empty()),
            project: Some(issue.key.project.clone()),
            title: issue.title.clone(),
            body: issue.body.clone(),
            body_cut: false,
            state: self.state(issue),
            kind: None,
            priority: None,
            assignee: issue.assignees.first().map(|a| seam::Person { id: a.username.clone(), name: a.name.clone() }),
            mine: me.map(|me| issue.assigned_to(&me.username)),
            labels: issue.labels.clone(),
            due: issue.due.clone(),
            updated: issue.updated.clone(),
            comments: notes.iter().filter(|n| !n.system).map(|n| seam::Comment { id: n.id.clone(), author: n.author.clone(), body: n.body.clone(), created: n.created.clone() }).collect(),
        }
        .cleaned()
    }

    /// The token's owner, asked once.
    async fn me(&self) -> Option<User> {
        let known = self.learned.borrow().me.clone();
        if known.is_some() {
            return known;
        }
        let me = self.client.me().await.ok();
        self.learned.borrow_mut().me = me.clone();
        me
    }

    /// A project's labels, asked once.
    async fn labels(&self, project: &str) -> Result<Vec<String>, SeamError> {
        let known = self.learned.borrow().labels.iter().find(|(p, _)| p.eq_ignore_ascii_case(project)).map(|(_, l)| l.clone());
        if let Some(labels) = known {
            return Ok(labels);
        }
        let labels: Vec<String> = self.client.labels(project).await.map_err(classify)?.into_iter().map(|l| l.name).collect();
        self.learned.borrow_mut().labels.push((project.to_string(), labels.clone()));
        Ok(labels)
    }

    /// Answers seam call `call` (`assigned`, `issues`, ...), reads and
    /// writes alike. A write reaches [`Seam::write`]; the extension puts
    /// its gate and its `op` memory in front of this.
    pub async fn serve(&self, call: &str, payload: &str) -> Result<String, String> {
        self.answer(call, payload).await.map_err(|e| e.to_err())
    }

    fn bad(&self, e: serde_json::Error) -> SeamError {
        SeamError::new(code::BAD_REQUEST, format!("a {} call: {e}", self.host().name()))
    }

    async fn answer(&self, call: &str, payload: &str) -> Answer {
        let hello: seam::Hello = serde_json::from_str(payload).map_err(|e| self.bad(e))?;
        if call == seam::call::ABOUT {
            return json(&about(self.host(), &self.settings));
        }
        if !seam::SEAMS.contains(&hello.seam) {
            return Err(SeamError::new(code::UNSUPPORTED, format!("{} speaks the tracker seam {:?}, not {}", self.host().name(), seam::SEAMS, hello.seam)));
        }
        if !self.settings.ready {
            return Err(SeamError::new(code::NOT_SET_UP, &self.settings.not_ready));
        }
        match call {
            seam::call::ASSIGNED => self.assigned(serde_json::from_str(payload).map_err(|e| self.bad(e))?).await,
            seam::call::ISSUES => self.issues(serde_json::from_str(payload).map_err(|e| self.bad(e))?).await,
            seam::call::TRANSITIONS => self.transitions(serde_json::from_str(payload).map_err(|e| self.bad(e))?).await,
            seam::call::PEOPLE => {
                let one: seam::One = serde_json::from_str(payload).map_err(|e| self.bad(e))?;
                let key = self.locate(&one.key).await?;
                let people = self.client.assignable(&key.project).await.map_err(classify)?.into_iter().map(|u| seam::Person { id: u.username, name: u.name }).collect();
                let none = format!("no one can be assigned in {} -- people need access to the {} first", key.project, self.host().place());
                json(&seam::People { people, none: Some(none) }.cleaned())
            }
            _ if seam::call::writes(call) => self.write(call, payload).await,
            other => Err(SeamError::new(code::UNSUPPORTED, format!("no service {}", seam::service(provider(self.host()), other)))),
        }
    }

    /// Every call that changes something on the forge, in one place: the
    /// one door through which the seam's writes reach the forge. Nothing
    /// here is retried.
    pub async fn write(&self, call: &str, payload: &str) -> Answer {
        let name = self.host().name();
        match call {
            seam::call::TRANSITION => self.transition(serde_json::from_str(payload).map_err(|e| self.bad(e))?).await,
            seam::call::UPDATE => self.update(serde_json::from_str(payload).map_err(|e| self.bad(e))?).await,
            seam::call::COMMENT => {
                let c: seam::NewComment = serde_json::from_str(payload).map_err(|e| self.bad(e))?;
                if c.body.len() > fenix_tracker::clean::BODY_BYTES {
                    return Err(SeamError::new(code::BAD_REQUEST, "a comment is at most 64 KiB"));
                }
                let key = self.locate_for_write(&c.key).await?;
                self.client.comment(&key, &c.body).await.map_err(classify)?;
                json(&seam::Said::default())
            }
            seam::call::WORKLOG if self.host() == Host::GitLab && self.settings.worklog => {
                let w: seam::Worklog = serde_json::from_str(payload).map_err(|e| self.bad(e))?;
                let key = self.locate_for_write(&w.key).await?;
                let day = w.started.get(..10).unwrap_or_default();
                self.client.spend(&key, w.seconds, day).await.map_err(classify)?;
                json(&seam::Said { said: Some(format!("logged {}", crate::issues::gitlab::duration(w.seconds))) })
            }
            seam::call::WORKLOG => Err(SeamError::new(code::UNSUPPORTED, format!("{name} doesn't take time logged from the agenda"))),
            seam::call::REMEMBER => Err(SeamError::new(code::UNSUPPORTED, format!("{name} keeps no choices from the agenda"))),
            other => Err(SeamError::new(code::UNSUPPORTED, format!("no service {}", seam::service(provider(self.host()), other)))),
        }
    }

    /// Open issues assigned to you, most recently updated first. The
    /// cursor is `page:per_page`, so a page asked for with another limit
    /// still follows on.
    async fn assigned(&self, req: seam::Assigned) -> Answer {
        let (page, per_page) = match req.cursor.as_deref() {
            Some(c) => {
                let (p, n) = c.split_once(':').ok_or_else(|| SeamError::new(code::BAD_REQUEST, format!("\"{c}\" isn't a cursor of {}'s", self.host().name())))?;
                (p.parse::<u32>().unwrap_or(1).max(1), n.parse::<u32>().unwrap_or(100).clamp(1, 100))
            }
            None => (1, req.limit().min(100)),
        };
        let paging = Paging { cursor: Some(page.to_string()), per_page, since: None };
        let me = self.me().await;
        let listing = self.client.search(&Search::Assigned, &paging).await.map_err(classify)?;
        let issues = listing.issues.iter().filter_map(|i| self.issue(i, &[], me.as_ref(), None)).collect();
        json(&seam::Page { issues, next: listing.next.map(|n| format!("{n}:{per_page}")) }.cleaned()?)
    }

    /// These issues, by id, ref or address, one request each, with their
    /// comments; what isn't there in `missing` exactly as it was asked.
    async fn issues(&self, req: seam::Issues) -> Answer {
        if req.refs.len() > seam::Issues::MAX_REFS {
            return Err(SeamError::new(code::BAD_REQUEST, format!("at most {} refs a call", seam::Issues::MAX_REFS)));
        }
        let me = self.me().await;
        let mut found = seam::Found::default();
        let gone = |e: &SeamError| e.is(code::NOT_FOUND) || e.is(code::BAD_REQUEST);
        for r in &req.refs {
            let key = match self.locate(r).await {
                Ok(key) => key,
                Err(e) if gone(&e) => {
                    found.missing.push(r.clone());
                    continue;
                }
                Err(e) => return Err(e),
            };
            let issue = match self.client.issue(&key).await {
                Ok(issue) => issue,
                Err(e) if matches!(e.status, Some(404 | 410)) && e.retry_after.is_none() => {
                    found.missing.push(r.clone());
                    continue;
                }
                Err(e) => return Err(classify(e)),
            };
            let notes = if issue.comments > 0 { self.client.notes(&issue.key).await.map_err(classify)? } else { Vec::new() };
            match self.issue(&issue, &notes, me.as_ref(), Some(r)) {
                Some(i) => found.issues.push(i),
                None => found.missing.push(r.clone()),
            }
        }
        json(&found.cleaned())
    }

    fn to(&self, id: &str, name: &str, stage: &str, column: &str) -> seam::State {
        seam::State { id: id.into(), name: name.into(), stage: stage.into(), column: column.into() }
    }

    async fn transitions(&self, req: seam::Transitions) -> Answer {
        let key = self.locate(&req.key).await?;
        json(&self.moves(&key, req.toward.as_deref()).await?)
    }

    /// An issue's moves, as `transitions` answers them (cleaned): for the
    /// seam and for the page's `s`. Changes nothing. A `local` move goes
    /// to each column nothing else reaches (GitHub's Doing; GitLab's
    /// Doing and Blocked without a workflow step for them).
    pub async fn moves(&self, key: &IssueRef, toward: Option<&str>) -> Result<seam::Moves, SeamError> {
        let issue = self.client.issue(key).await.map_err(classify)?;
        let now = self.state(&issue);
        let mut moves = Vec::new();
        if issue.open {
            match self.host() {
                Host::GitHub => {
                    moves.push(seam::Move { id: "close".into(), name: "Close as completed".into(), to: self.to("closed", "Closed", column::DONE, column::DONE), said: Some("closed as completed".into()), ..Default::default() });
                    moves.push(seam::Move { id: "close:not_planned".into(), name: "Close as not planned".into(), to: self.to("closed:not_planned", "Closed as not planned", column::DONE, column::DONE), said: Some("closed as not planned".into()), ..Default::default() });
                }
                Host::GitLab => moves.push(seam::Move { id: "close".into(), name: "Close".into(), to: self.to("closed", "Closed", column::DONE, column::DONE), said: Some("closed".into()), ..Default::default() }),
            }
            if let Some(scope) = self.scope() {
                let labels = self.labels(&key.project).await.unwrap_or_default();
                let current = self.step(&issue).map(str::to_ascii_lowercase);
                for label in labels.iter().filter(|l| l.get(..scope.len()).is_some_and(|p| p.eq_ignore_ascii_case(&scope))) {
                    let step = &label[scope.len()..];
                    if current.as_deref() == Some(&step.to_ascii_lowercase()) || step.is_empty() {
                        continue;
                    }
                    let (stage, col) = workflow_stage(step);
                    moves.push(seam::Move {
                        id: format!("label:+{label}"),
                        name: step.to_string(),
                        label: Some(format!("{step}   ({label})")),
                        to: self.to(&format!("opened:{step}"), step, stage, col),
                        said: Some(format!("moved to {label}")),
                        ..Default::default()
                    });
                }
            }
            // The columns no move reaches stay the agenda's own.
            let mut local = vec![column::DOING];
            if self.host() == Host::GitLab {
                local.push(column::BLOCKED);
            }
            for col in local {
                if now.column != col && !moves.iter().any(|m| m.to.column == col) {
                    let name = if col == column::DOING { "Doing" } else { "Blocked" };
                    moves.push(seam::Move {
                        id: format!("local:{col}"),
                        name: format!("{name} in the agenda only"),
                        local: true,
                        to: seam::State { column: col.into(), ..now.clone() },
                        note: Some(format!("{} has no {name} of its own -- the card moves in your agenda, and nothing is sent", self.host().name())),
                        ..Default::default()
                    });
                }
            }
        } else {
            moves.push(seam::Move { id: "reopen".into(), name: "Reopen".into(), to: self.to(self.open_id(), "Open", column::TODO, column::TODO), said: Some("reopened".into()), ..Default::default() });
        }
        let _ = toward;
        Ok(seam::Moves { transitions: moves, ..Default::default() }.cleaned())
    }

    /// Doing a move.
    async fn transition(&self, req: seam::Transition) -> Answer {
        let key = self.locate(&req.key).await?;
        let Some((change, said)) = self.change_for_move(&key, &req.id)? else { return json(&seam::Said::default()) };
        let key = self.locate_for_write(&req.key).await?;
        self.client.change(&key, &change).await.map_err(classify)?;
        json(&seam::Said { said: Some(said) })
    }

    /// What move `id` changes on the forge, and what's said once it has;
    /// none for a move that's only the agenda's (`local:...`).
    pub fn change_for_move(&self, key: &IssueRef, id: &str) -> Result<Option<(Change, String)>, SeamError> {
        if id.starts_with("local") {
            return Ok(None);
        }
        Ok(Some(match id {
            "close" if self.host() == Host::GitHub => (Change::Close(Some("completed".into())), "closed as completed".to_string()),
            "close" => (Change::Close(None), "closed".to_string()),
            "close:not_planned" if self.host() == Host::GitHub => (Change::Close(Some("not_planned".into())), "closed as not planned".to_string()),
            "reopen" => (Change::Reopen, "reopened".to_string()),
            id => match (id.strip_prefix("label:+"), self.scope()) {
                // Only the project's workflow labels: a move is never "add
                // any label the caller names".
                (Some(label), Some(scope)) if label.to_ascii_lowercase().starts_with(&scope.to_ascii_lowercase()) && label.len() > scope.len() => (Change::AddLabel(label.to_string()), format!("moved to {label}")),
                _ => return Err(SeamError::new(code::REFUSED, format!("no move {id} for {key}"))),
            },
        }))
    }

    async fn update(&self, req: seam::Update) -> Answer {
        let name = self.host().name();
        let change = req.change()?;
        if !capabilities(self.host(), &self.settings).contains(&can::of(&change)) {
            let what = match &change {
                SeamChange::Due(_) => "no due date",
                SeamChange::Priority(_) => "no priority",
                _ => "nothing of that",
            };
            return Err(SeamError::new(code::UNSUPPORTED, format!("{name} issues have {what} -- it stays on the task")));
        }
        let key = self.locate_for_write(&req.key).await?;
        let change = match change {
            SeamChange::Title(t) => Change::Title(t),
            SeamChange::Body(b) => Change::Body(b),
            SeamChange::Due(d) => Change::Due(d),
            SeamChange::Labels(l) => Change::Labels(l),
            SeamChange::Assignee(None) => Change::Assignees(Vec::new()),
            SeamChange::Assignee(Some(p)) => Change::Assignees(vec![p.id]),
            SeamChange::Priority(_) => unreachable!("not a capability of either forge"),
        };
        self.client.change(&key, &change).await.map_err(classify)?;
        json(&seam::Said::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_classed_with_the_forges_words_kept() {
        let cases = [
            (ApiError::offline("timed out"), code::OFFLINE),
            (ApiError::status(401, "HTTP 401 (Unauthorized): Bad credentials"), code::SIGNED_OUT),
            (ApiError::status(404, "HTTP 404 (Not Found)"), code::NOT_FOUND),
            (ApiError::status(410, "HTTP 410: This issue was deleted"), code::NOT_FOUND),
            (ApiError::status(403, "HTTP 403 (Forbidden): Resource not accessible"), code::REFUSED),
            (ApiError::status(422, "HTTP 422 (Unprocessable Entity): Validation Failed"), code::REFUSED),
            (ApiError::status(500, "HTTP 500 (Internal Server Error)"), code::FAILED),
            (ApiError::status(502, "HTTP 502 (Bad Gateway)"), code::OFFLINE),
            (ApiError::status(503, "HTTP 503 (Service Unavailable)"), code::OFFLINE),
            (ApiError { retry_after: Some(30), ..ApiError::status(429, "HTTP 429") }, code::RATE_LIMITED),
            (ApiError { retry_after: Some(60), ..ApiError::status(403, "HTTP 403: rate limit") }, code::RATE_LIMITED),
        ];
        for (e, want) in cases {
            let message = e.message.clone();
            let err = classify(e);
            assert_eq!((err.code.as_str(), err.message.as_str()), (want, message.as_str()));
        }
        assert_eq!(classify(ApiError { retry_after: Some(30), ..ApiError::status(429, "x") }).retry_after, Some(30));
    }

    #[test]
    fn a_workflow_steps_name_says_how_far_along_it_is() {
        for (step, want) in [("To Do", ("todo", "todo")), ("Backlog", ("todo", "todo")), ("Doing", ("doing", "doing")), ("In review", ("doing", "doing")), ("Blocked", ("doing", "blocked")), ("Done", ("done", "done")), ("Released", ("done", "done"))] {
            assert_eq!(workflow_stage(step), want, "{step}");
        }
    }

    #[test]
    fn each_forge_lists_only_what_it_serves() {
        let s = Settings { ready: true, ..Settings::default() };
        let gh = about(Host::GitHub, &s);
        assert_eq!(gh.can, ["comments", "edit.title", "edit.body", "edit.assignee", "edit.labels", "comment", "create", "project"]);
        assert_eq!((gh.id.as_str(), gh.blocked, gh.browse.is_none(), gh.seams.clone()), ("fenix.github-issues", false, true, vec![1]));
        let gl = about(Host::GitLab, &Settings { worklog: true, ..s });
        assert_eq!(gl.can, ["comments", "edit.title", "edit.body", "edit.due", "edit.assignee", "edit.labels", "comment", "create", "project", "worklog"]);
        assert!(gl.blocked);
    }
}
