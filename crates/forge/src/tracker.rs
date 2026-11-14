//! The tracker seam (`plans/tracker-seam.md`) served for GitHub Issues
//! and GitLab Issues over an [`Issues`] client: the issues assigned to
//! you, these issues, their moves (GitHub's close as completed, close as
//! not planned and reopen; GitLab's close and reopen and its scoped
//! `workflow::` labels; a `blocked` label on either), and the agenda's
//! writes. The extensions serve it as `github-issues.tracker.*` and
//! `gitlab-issues.tracker.*` over their own HTTP; `create`, which opens
//! their page's form, is theirs.
//!
//! Feature `tracker`.

use std::cell::RefCell;
use std::rc::Rc;

use fenix_tracker::{self as seam, can, code, column, Change as SeamChange, SeamError};

use crate::issues::{ApiError, Change, Host, Issue, IssueRef, Issues, Paging, Search, User};

/// What the extension's settings say, and whether it's signed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Signed in, and the server known.
    pub ready: bool,
    /// What to do about it when it isn't, with the keys.
    pub not_ready: String,
    pub sync_minutes: u32,
    /// The label that says an issue is blocked; none when empty.
    pub blocked_label: String,
    /// The label that says an open issue is under way (GitHub).
    pub doing_label: String,
    /// The scope of GitLab's scoped labels that are its workflow:
    /// `workflow` for `workflow::Doing`.
    pub workflow_scope: String,
    /// Time from the agenda logged on the issue (GitLab).
    pub worklog: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { ready: false, not_ready: String::new(), sync_minutes: 10, blocked_label: "blocked".into(), doing_label: "in progress".into(), workflow_scope: "workflow".into(), worklog: false }
    }
}

/// What it learns once and keeps: who you are, and each project's labels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Learned {
    pub me: Option<User>,
    pub labels: Vec<(String, Vec<String>)>,
}

/// `about` for `host`.
pub fn about(host: Host, settings: &Settings) -> seam::About {
    let mut caps = vec![can::COMMENTS, can::EDIT, can::ASSIGN, can::COMMENT, can::CREATE];
    if host == Host::GitLab && settings.worklog {
        caps.push(can::WORKLOG);
    }
    seam::About {
        seam: seam::SEAM,
        minor: seam::MINOR,
        name: host.name().into(),
        ready: settings.ready,
        not_ready: (!settings.ready).then(|| settings.not_ready.clone()),
        can: caps.into_iter().map(str::to_string).collect(),
        sync_minutes: settings.sync_minutes,
        project_meta: None,
        browse: None,
        ref_hint: Some(match host {
            Host::GitHub => "owner/repo#123".into(),
            Host::GitLab => "group/project#123".into(),
        }),
    }
}

/// A failed request as the seam's error, the forge's own words kept.
pub fn classify(e: ApiError) -> SeamError {
    let code = match (e.status, e.retry_after) {
        (None, _) => code::OFFLINE,
        (_, Some(_)) => code::RATE_LIMITED,
        (Some(401), _) => code::SIGNED_OUT,
        (Some(404 | 410), _) => code::NOT_FOUND,
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

/// Whether seam call `call` changes something on the forge. `create`
/// is one too, served by the extension's form.
pub fn is_write(call: &str) -> bool {
    matches!(call, seam::call::TRANSITION | seam::call::UPDATE | seam::call::COMMENT | seam::call::WORKLOG | seam::call::CREATE | seam::call::REMEMBER)
}

/// How far along a workflow step's name says an issue is.
pub fn workflow_stage(step: &str) -> &'static str {
    let s = step.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| s.contains(w));
    if has(&["done", "complete", "released", "shipped", "closed", "resolved"]) {
        column::DONE
    } else if has(&["todo", "to do", "backlog", "triage", "ready", "new", "open", "planning", "refinement"]) {
        column::TODO
    } else {
        column::DOING
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

    fn id(&self) -> &'static str {
        match self.host() {
            Host::GitHub => "github-issues",
            Host::GitLab => "gitlab-issues",
        }
    }

    /// `owner/repo#12`, or an issue's address on this forge.
    pub fn parse_ref(&self, r: &str) -> Option<IssueRef> {
        IssueRef::parse(r, self.host(), self.client.web())
    }

    fn bad_ref(&self, r: &str) -> SeamError {
        let hint = match self.host() {
            Host::GitHub => "owner/repo#12, or the issue's address",
            Host::GitLab => "group/project#12, or the issue's address",
        };
        SeamError::new(code::BAD_REQUEST, format!("\"{r}\" isn't an issue -- {hint}"))
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

    fn blocked(&self, issue: &Issue) -> bool {
        let b = self.settings.blocked_label.trim();
        !b.is_empty() && issue.has_label(b)
    }

    /// Where an issue stands, as the seam has it.
    pub fn state(&self, issue: &Issue) -> seam::State {
        if !issue.open {
            let (id, name) = match issue.state_reason.as_deref() {
                Some("not_planned") => ("closed:not_planned", "Closed as not planned"),
                Some("duplicate") => ("closed:duplicate", "Closed as a duplicate"),
                _ => ("closed", "Closed"),
            };
            return seam::State { id: id.into(), name: name.into(), stage: column::DONE.into(), column: column::DONE.into() };
        }
        let (id, name, stage) = match self.step(issue) {
            Some(step) => (format!("opened:{step}"), step.to_string(), workflow_stage(step)),
            None if self.host() == Host::GitHub && !self.settings.doing_label.trim().is_empty() && issue.has_label(self.settings.doing_label.trim()) => ("open+doing".into(), "Open".into(), column::DOING),
            None => (if self.host() == Host::GitHub { "open" } else { "opened" }.into(), "Open".into(), column::TODO),
        };
        let column = if self.blocked(issue) { column::BLOCKED } else { stage };
        seam::State { id, name, stage: stage.into(), column: column.into() }
    }

    /// One issue as the seam has it, its comments (people's, not the
    /// forge's narration) with it.
    pub fn issue(&self, issue: &Issue, notes: &[crate::issues::Note], me: Option<&User>) -> Option<seam::Issue> {
        seam::Issue {
            key: issue.key.key(),
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

    /// Answers seam call `call` (`assigned`, `issues`, ...).
    pub async fn serve(&self, call: &str, payload: &str) -> Result<String, String> {
        self.answer(call, payload).await.map_err(|e| e.to_err())
    }

    async fn answer(&self, call: &str, payload: &str) -> Answer {
        let bad = |e: serde_json::Error| SeamError::new(code::BAD_REQUEST, format!("a {} call: {e}", self.host().name()));
        let hello: seam::Hello = serde_json::from_str(payload).map_err(bad)?;
        if hello.seam != seam::SEAM {
            return Err(SeamError::new(code::UNSUPPORTED, format!("{} speaks the tracker seam {}, not {}", self.host().name(), seam::SEAM, hello.seam)));
        }
        if call == seam::call::ABOUT {
            return json(&about(self.host(), &self.settings));
        }
        if !self.settings.ready {
            return Err(SeamError::new(code::NOT_SET_UP, &self.settings.not_ready));
        }
        match call {
            seam::call::ASSIGNED => self.assigned(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::ISSUES => self.issues(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::TRANSITIONS => self.transitions(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::PEOPLE => {
                let one: seam::One = serde_json::from_str(payload).map_err(bad)?;
                let key = self.parse_ref(&one.key).ok_or_else(|| self.bad_ref(&one.key))?;
                let people = self.client.assignable(&key.project).await.map_err(classify)?.into_iter().map(|u| seam::Person { id: u.username, name: u.name }).collect();
                let none = format!("no one can be assigned in {} -- people need access to the {} first", key.project, self.host().place());
                json(&seam::People { people, none: Some(none) }.cleaned())
            }
            _ if is_write(call) => self.write(call, payload).await,
            other => Err(SeamError::new(code::UNSUPPORTED, format!("no service {}", seam::service(self.id(), other)))),
        }
    }

    /// Every call that changes something on the forge, in one place: the
    /// one door a gate on the seam's writes goes in front of. Nothing
    /// here is retried.
    pub async fn write(&self, call: &str, payload: &str) -> Answer {
        let bad = |e: serde_json::Error| SeamError::new(code::BAD_REQUEST, format!("a {} call: {e}", self.host().name()));
        match call {
            seam::call::TRANSITION => self.transition(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::UPDATE => self.update(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::COMMENT => {
                let c: seam::NewComment = serde_json::from_str(payload).map_err(bad)?;
                let key = self.parse_ref(&c.key).ok_or_else(|| self.bad_ref(&c.key))?;
                if c.body.len() > fenix_tracker::clean::BODY_BYTES {
                    return Err(SeamError::new(code::BAD_REQUEST, "a comment is at most 64 KiB"));
                }
                self.client.comment(&key, &c.body).await.map_err(classify)?;
                json(&seam::Said::default())
            }
            seam::call::WORKLOG if self.host() == Host::GitLab && self.settings.worklog => {
                let w: seam::Worklog = serde_json::from_str(payload).map_err(bad)?;
                let key = self.parse_ref(&w.key).ok_or_else(|| self.bad_ref(&w.key))?;
                let day = w.started.get(..10).unwrap_or_default();
                self.client.spend(&key, w.seconds, day).await.map_err(classify)?;
                json(&seam::Said { said: Some(format!("logged {}", crate::issues::gitlab::duration(w.seconds))) })
            }
            other => Err(SeamError::new(code::UNSUPPORTED, format!("no service {}", seam::service(self.id(), other)))),
        }
    }

    /// Open issues assigned to you, most recently updated first. The
    /// cursor is `page:per_page`, so a page asked for with another limit
    /// still follows on.
    async fn assigned(&self, req: seam::Assigned) -> Answer {
        let (page, per_page) = match req.cursor.as_deref().and_then(|c| c.split_once(':')) {
            Some((p, n)) => (p.parse().unwrap_or(1u32), n.parse().unwrap_or(100u32)),
            None => (1, req.limit().min(100)),
        };
        let paging = Paging { cursor: Some(page.to_string()), per_page, since: req.since.clone() };
        let me = self.me().await;
        let listing = self.client.search(&Search::Assigned, &paging).await.map_err(classify)?;
        let issues = listing.issues.iter().filter_map(|i| self.issue(i, &[], me.as_ref())).collect();
        json(&seam::Page { issues, next: listing.next.map(|n| format!("{n}:{per_page}")) }.cleaned())
    }

    /// These issues, one request each, with their comments.
    async fn issues(&self, req: seam::Issues) -> Answer {
        if req.refs.len() > seam::Issues::MAX_REFS {
            return Err(SeamError::new(code::BAD_REQUEST, format!("at most {} refs a call", seam::Issues::MAX_REFS)));
        }
        let me = self.me().await;
        let mut found = seam::Found::default();
        for r in &req.refs {
            let Some(key) = self.parse_ref(r) else {
                found.missing.push(r.clone());
                continue;
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
            found.issues.extend(self.issue(&issue, &notes, me.as_ref()));
        }
        json(&found.cleaned())
    }

    fn to(&self, id: &str, name: &str, stage: &str, column: &str) -> seam::State {
        seam::State { id: id.into(), name: name.into(), stage: stage.into(), column: column.into() }
    }

    /// What can be done next: close or reopen, the workflow's steps, the
    /// blocked label on or off -- worked out from the state the agenda
    /// last saw when it says, else from the issue.
    async fn transitions(&self, req: seam::Transitions) -> Answer {
        let key = self.parse_ref(&req.key).ok_or_else(|| self.bad_ref(&req.key))?;
        json(&self.moves(&key, req.toward.as_deref()).await?)
    }

    /// An issue's moves, as `transitions` answers them (cleaned): for the
    /// seam and for the page's `s`. Changes nothing.
    pub async fn moves(&self, key: &IssueRef, toward: Option<&str>) -> Result<seam::Moves, SeamError> {
        let issue = self.client.issue(key).await.map_err(classify)?;
        let now = self.state(&issue);
        let labels = self.labels(&key.project).await.unwrap_or_default();
        let blocked = self.settings.blocked_label.trim().to_string();
        let has_blocked_label = !blocked.is_empty() && labels.iter().any(|l| l.eq_ignore_ascii_case(&blocked));
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
                let current = self.step(&issue).map(str::to_ascii_lowercase);
                for label in labels.iter().filter(|l| l.get(..scope.len()).is_some_and(|p| p.eq_ignore_ascii_case(&scope))) {
                    let step = &label[scope.len()..];
                    if current.as_deref() == Some(&step.to_ascii_lowercase()) || step.is_empty() {
                        continue;
                    }
                    let stage = workflow_stage(step);
                    let column = if self.blocked(&issue) { column::BLOCKED } else { stage };
                    moves.push(seam::Move {
                        id: format!("label:+{label}"),
                        name: step.to_string(),
                        label: Some(format!("{step}   ({label})")),
                        to: self.to(&format!("opened:{step}"), step, stage, column),
                        said: Some(format!("moved to {label}")),
                        ..Default::default()
                    });
                }
            }
            if has_blocked_label {
                let id = format!("label:{}{blocked}", if self.blocked(&issue) { "-" } else { "+" });
                let to = seam::State { column: if self.blocked(&issue) { now.stage.clone() } else { column::BLOCKED.into() }, ..now.clone() };
                let (name, said) = if self.blocked(&issue) { ("Unblock", format!("took the {blocked} label off")) } else { ("Block", format!("labelled {blocked}")) };
                moves.push(seam::Move { id, name: name.into(), label: Some(format!("{name}   (the {blocked} label)")), to, said: Some(said), ..Default::default() });
            }
        } else {
            let reopened = self.to(if self.host() == Host::GitHub { "open" } else { "opened" }, "Open", column::TODO, column::TODO);
            moves.push(seam::Move { id: "reopen".into(), name: "Reopen".into(), to: reopened, said: Some("reopened".into()), ..Default::default() });
        }
        let mut answer = seam::Moves { transitions: moves, ..Default::default() };
        if !has_blocked_label {
            answer.blocked = Some("Blocked  (in your agenda only)".into());
            if toward == Some(column::BLOCKED) && issue.open {
                let why = if blocked.is_empty() { "no label says an issue is blocked".to_string() } else { format!("{} has no \"{blocked}\" label", key.project) };
                answer.transitions.push(seam::Move {
                    id: "local".into(),
                    name: "Keep Blocked local".into(),
                    local: true,
                    to: seam::State { column: column::BLOCKED.into(), ..now.clone() },
                    note: Some(format!("{why} -- Blocked stays in your agenda")),
                    ..Default::default()
                });
            }
        } else {
            answer.blocked = Some(format!("Blocked  (the {blocked} label)"));
        }
        Ok(answer.cleaned())
    }

    /// Doing a move.
    async fn transition(&self, req: seam::Transition) -> Answer {
        let key = self.parse_ref(&req.key).ok_or_else(|| self.bad_ref(&req.key))?;
        let Some((change, said)) = self.change_for_move(&key, &req.id)? else { return json(&seam::Said::default()) };
        self.client.change(&key, &change).await.map_err(classify)?;
        json(&seam::Said { said: Some(said) })
    }

    /// What move `id` changes on the forge, and what's said once it has;
    /// none for a move that's only the agenda's (`local`).
    pub fn change_for_move(&self, key: &IssueRef, id: &str) -> Result<Option<(Change, String)>, SeamError> {
        Ok(Some(match id {
            "local" => return Ok(None),
            "close" if self.host() == Host::GitHub => (Change::Close(Some("completed".into())), "closed as completed".to_string()),
            "close" => (Change::Close(None), "closed".to_string()),
            "close:not_planned" if self.host() == Host::GitHub => (Change::Close(Some("not_planned".into())), "closed as not planned".to_string()),
            "reopen" => (Change::Reopen, "reopened".to_string()),
            id => match id.strip_prefix("label:+").map(|l| (true, l)).or_else(|| id.strip_prefix("label:-").map(|l| (false, l))) {
                Some((true, label)) => {
                    let label = label.to_string();
                    (Change::AddLabel(label.clone()), match self.scope() {
                        Some(scope) if label.to_ascii_lowercase().starts_with(&scope.to_ascii_lowercase()) => format!("moved to {label}"),
                        _ => format!("labelled {label}"),
                    })
                }
                Some((false, label)) => {
                    let label = label.to_string();
                    (Change::RemoveLabel(label.clone()), format!("took the {label} label off"))
                }
                None => return Err(SeamError::new(code::REFUSED, format!("no move {id} for {key}"))),
            },
        }))
    }

    async fn update(&self, req: seam::Update) -> Answer {
        let key = self.parse_ref(&req.key).ok_or_else(|| self.bad_ref(&req.key))?;
        let name = self.host().name();
        let change = match req.change().map_err(|e| SeamError::new(code::BAD_REQUEST, e))? {
            SeamChange::Title(t) => Change::Title(t),
            SeamChange::Body(b) => Change::Body(b),
            SeamChange::Due(d) if self.host() == Host::GitLab => Change::Due(d),
            SeamChange::Due(_) => return Err(SeamError::new(code::UNSUPPORTED, format!("{name} issues have no due date -- it stays on the task"))),
            SeamChange::Priority(_) => return Err(SeamError::new(code::UNSUPPORTED, format!("{name} issues have no priority -- it stays on the task"))),
            SeamChange::Assignee(p) if p.id.is_empty() => Change::Assignees(Vec::new()),
            SeamChange::Assignee(p) => Change::Assignees(vec![p.id]),
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
            (ApiError::status(502, "HTTP 502 (Bad Gateway)"), code::FAILED),
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
        for (step, want) in [("To Do", "todo"), ("Backlog", "todo"), ("Ready for dev", "todo"), ("Doing", "doing"), ("In review", "doing"), ("Verification", "doing"), ("Done", "done"), ("Released", "done")] {
            assert_eq!(workflow_stage(step), want, "{step}");
        }
    }
}
