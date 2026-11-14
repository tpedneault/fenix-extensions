//! The tracker seam (`plans/tracker-seam.md`) served for Jira Server and
//! Data Center, and for Jira Cloud, over a [`JiraClient`]: what the agenda did with Jira
//! itself before -- which requests, how an issue maps onto its columns
//! and priorities, what Blocked means in a project -- moved here, so the
//! Jira extension serves it (`jira.tracker.*`, its own HTTP) and the
//! agenda serves it to itself when the Jira extension is older than the
//! seam (over `jira.request`). The same code either way, so the same
//! answers.
//!
//! On Cloud the same calls, asked Cloud's way (the client's), with these
//! differences: an issue's description and comments are rich text read
//! as plain text, so `about.can` leaves out `edit` (the agenda sends
//! title, description, priority and due date under it, and a description
//! written back from text would lose what's in it); assignees are account
//! ids and `people` is who Jira says can be assigned the issue; `assigned`
//! pages by the enhanced search's token.
//!
//! `about`, `create` and `remember` need the extension (its settings,
//! its form): [`about`] and [`parse_remember`] are for the caller, and
//! [`Seam::serve`] answers the rest.

use std::cell::RefCell;
use std::rc::Rc;

use fenix_tracker::{self as seam, can, code, column, level, Change, SeamError};

use crate::{build_keys_jql, build_my_open_issues_jql, project_of, Deployment, IssueDetail, JiraClient, Transition};

/// The keys the messages name, as the key scheme in use has them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    /// The settings page (`settings.open`).
    pub settings: String,
    /// The Jira page (`jira.open`).
    pub page: String,
}

impl Default for Keys {
    fn default() -> Self {
        Keys { settings: "SPC ,".into(), page: "SPC j j".into() }
    }
}

/// Jira's settings, as `jira.config` has always said them to the agenda.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
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
    #[serde(skip)]
    pub keys: Keys,
}

/// What Blocked means in one project (`jira.blocked`).
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

impl Settings {
    /// From `jira.config`'s answer.
    pub fn from_config(json: &str, keys: Keys) -> Result<Settings, String> {
        let mut settings: Settings = serde_json::from_str(json).map_err(|e| e.to_string())?;
        settings.keys = keys;
        Ok(settings)
    }

    pub fn base(&self) -> Option<&str> {
        self.base_url.as_deref().map(|b| b.trim().trim_end_matches('/')).filter(|b| !b.is_empty())
    }

    pub fn blocked_for(&self, project: &str) -> Option<Blocked> {
        self.blocked.iter().find(|(p, _)| p == project).and_then(|(_, meaning)| Blocked::parse(meaning).ok())
    }

    /// Whether Blocked means the Flagged field in any project.
    pub fn flags_anywhere(&self) -> bool {
        self.blocked.iter().any(|(_, meaning)| Blocked::parse(meaning) == Ok(Blocked::Flag))
    }

    /// The agenda's level for a Jira priority: `jira.priorities` first,
    /// then a guess from the name.
    pub fn level(&self, name: &str) -> &'static str {
        self.priorities
            .iter()
            .find(|(jira, _)| jira.eq_ignore_ascii_case(name))
            .and_then(|(_, mine)| level::ALL.into_iter().find(|l| l.eq_ignore_ascii_case(mine.trim())))
            .unwrap_or_else(|| guess_level(name))
    }

    /// The agenda column an issue in `project`, at status `status_id` of
    /// `category`, belongs in.
    pub fn column(&self, project: &str, status_id: &str, category: &str, flagged: bool) -> &'static str {
        match self.blocked_for(project) {
            Some(Blocked::Status { id, .. }) if id == status_id => column::BLOCKED,
            Some(Blocked::Flag) if flagged => column::BLOCKED,
            _ => stage(category),
        }
    }

    fn url(&self, key: &str) -> Option<String> {
        self.base().map(|b| format!("{b}/browse/{key}"))
    }

    /// The label of the agenda's Blocked row in `project`.
    fn blocked_label(&self, project: &str) -> Option<String> {
        match self.blocked_for(project)? {
            Blocked::Status { name, .. } => Some(format!("Blocked  ({name})")),
            Blocked::Flag => Some("Blocked  (flag as impediment)".into()),
            Blocked::Local => Some("Blocked  (local only)".into()),
        }
    }
}

/// Jira's status category as a stage: `new`, `indeterminate`, `done`.
pub fn stage(category: &str) -> &'static str {
    match category {
        "done" => column::DONE,
        "indeterminate" => column::DOING,
        _ => column::TODO,
    }
}

/// A best guess at which agenda level a Jira priority name means, for
/// instances that don't configure one -- the usual Blocker/Critical/
/// Major/Minor/Trivial and Highest/High/Medium/Low/Lowest schemes both
/// land where you'd expect.
pub fn guess_level(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if ["highest", "blocker", "critical", "urgent"].iter().any(|w| lower.contains(w)) {
        level::URGENT
    } else if lower.contains("high") || lower.contains("major") {
        level::HIGH
    } else if ["low", "minor", "trivial"].iter().any(|w| lower.contains(w)) {
        level::LOW
    } else {
        level::MEDIUM
    }
}

/// What it learns once and keeps: the token owner's username, and the
/// Flagged field's id (`None` until asked, `Some(None)` when there's
/// none).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Learned {
    pub me: Option<String>,
    pub flag_field: Option<Option<String>>,
}

/// `about`: Jira, ready when it has a server (and, for the extension, a
/// token).
pub fn about(settings: &Settings, ready: bool) -> seam::About {
    about_for(settings, ready, Deployment::Server)
}

/// `about` for `deployment`. Cloud needs your email too, and can't take
/// `edit`: see the module's words.
pub fn about_for(settings: &Settings, ready: bool, deployment: Deployment) -> seam::About {
    let not_ready = match deployment {
        Deployment::Server => format!("Jira isn't set up yet -- set its server and token in {} (Jira)", settings.keys.settings),
        Deployment::Cloud => format!("Jira Cloud isn't set up yet -- set its site, your email and an API token in {} (Jira)", settings.keys.settings),
    };
    let can = can::ALL.iter().filter(|c| !(deployment.is_cloud() && **c == can::EDIT)).map(|c| c.to_string()).collect();
    seam::About {
        seam: seam::SEAM,
        minor: seam::MINOR,
        name: "Jira".into(),
        ready,
        not_ready: (!ready).then_some(not_ready),
        can,
        sync_minutes: settings.sync_minutes,
        project_meta: Some("jira".into()),
        browse: settings.base().map(|b| format!("{b}/browse/{{ref}}")),
        ref_hint: Some("PROJ-123".into()),
    }
}

/// What a move's `remember` keeps: (project, what Blocked means there).
pub fn parse_remember(remember: &str) -> Option<(String, Blocked)> {
    let v: serde_json::Value = serde_json::from_str(remember).ok()?;
    let project = v.get("project")?.as_str()?.to_string();
    let blocked = Blocked::parse(v.get("blocked")?.as_str()?).ok()?;
    Some((project, blocked))
}

fn remember(project: &str, blocked: &Blocked) -> String {
    serde_json::json!({ "project": project, "blocked": blocked.text() }).to_string()
}

/// A failed request as the seam's error, Jira's own words kept.
pub fn classify(message: String) -> SeamError {
    let code = if message.starts_with("HTTP 401") {
        code::SIGNED_OUT
    } else if message.starts_with("HTTP 404") {
        code::NOT_FOUND
    } else if message.starts_with("HTTP 429") {
        code::RATE_LIMITED
    } else if message.starts_with("HTTP 4") {
        code::REFUSED
    } else if message.starts_with("request failed") {
        code::OFFLINE
    } else if message.starts_with("Jira isn't set up") {
        code::NOT_SET_UP
    } else {
        code::FAILED
    };
    SeamError::new(code, message)
}

/// `PROJ-12` from `PROJ-12`, `https://jira.example.com/browse/PROJ-12`,
/// or a Cloud board's `...?selectedIssue=PROJ-12`.
fn key_of(r: &str) -> String {
    if r.starts_with("http") {
        if let Some((_, rest)) = r.split_once("selectedIssue=") {
            return rest.split(['&', '#']).next().unwrap_or(rest).to_string();
        }
    }
    match r.split_once("/browse/") {
        Some((_, rest)) if r.starts_with("http") => rest.split(['?', '#', '/']).next().unwrap_or(rest).to_string(),
        _ => r.trim().to_uppercase(),
    }
}

/// `2026-11-12T09:15:00.000+01:00` as Jira writes a worklog's start,
/// `2026-11-12T09:15:00.000+0100`.
pub fn jira_time(rfc3339: &str) -> Result<String, String> {
    chrono::DateTime::parse_from_rfc3339(rfc3339).map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3f%z").to_string()).map_err(|e| format!("started: {e}"))
}

/// The seam over one Jira.
#[derive(Clone)]
pub struct Seam {
    pub client: JiraClient,
    pub settings: Settings,
    pub learned: Rc<RefCell<Learned>>,
}

type Answer = Result<String, SeamError>;

fn json<T: serde::Serialize>(value: &T) -> Answer {
    serde_json::to_string(value).map_err(|e| SeamError::new(code::FAILED, e.to_string()))
}

fn bad(e: serde_json::Error) -> SeamError {
    SeamError::new(code::BAD_REQUEST, format!("a Jira call: {e}"))
}

impl Seam {
    pub fn new(client: JiraClient, settings: Settings, learned: Rc<RefCell<Learned>>) -> Self {
        Seam { client, settings, learned }
    }

    /// A failed request as the seam's error, with what a `429` said to
    /// wait when the transport saw it.
    fn fail(&self, message: String) -> SeamError {
        let mut err = classify(message);
        if err.code == code::RATE_LIMITED {
            err.retry_after = self.client.retry_after().map(u64::from);
        }
        err
    }

    fn deployment(&self) -> Deployment {
        self.client.deployment()
    }

    /// One issue, as the seam has it.
    pub fn issue(&self, d: &IssueDetail, me: Option<&str>) -> Option<seam::Issue> {
        let project = project_of(&d.key);
        seam::Issue {
            key: d.key.clone(),
            url: self.settings.url(&d.key),
            project: Some(project.to_string()),
            title: d.summary.clone(),
            body: d.description.clone().unwrap_or_default(),
            body_cut: false,
            state: seam::State { id: d.status_id.clone(), name: d.status.clone(), stage: stage(&d.status_category).into(), column: self.settings.column(project, &d.status_id, &d.status_category, d.flagged).into() },
            kind: d.issue_type.clone(),
            priority: d.priority.as_ref().map(|p| seam::Priority { name: p.clone(), level: self.settings.level(p).into() }),
            assignee: (d.assignee.is_some() || d.assignee_id.is_some()).then(|| seam::Person { id: d.assignee_id.clone().unwrap_or_default(), name: d.assignee.clone().unwrap_or_default() }),
            mine: me.map(|me| d.assignee_id.as_deref() == Some(me)),
            labels: d.labels.clone(),
            due: d.due.clone(),
            updated: d.updated.clone(),
            comments: d.comments.iter().map(|c| seam::Comment { id: c.id.clone(), author: c.author.clone(), body: c.body.clone(), created: c.created.clone() }).collect(),
        }
        .cleaned()
    }

    /// The token owner, asked once.
    async fn me(&self) -> Option<String> {
        let known = self.learned.borrow().me.clone();
        if known.is_some() {
            return known;
        }
        let me = self.client.myself().await.ok();
        if me.is_some() {
            self.learned.borrow_mut().me = me.clone();
        }
        me
    }

    /// The Flagged field to read, when a project's Blocked is the flag:
    /// asked once (`fetch`), or only what's known.
    async fn flag(&self, fetch: bool) -> Option<String> {
        if !self.settings.flags_anywhere() {
            return None;
        }
        let known = self.learned.borrow().flag_field.clone();
        match known {
            Some(field) => field,
            None if fetch => {
                let found = self.client.find_flagged_field().await.ok();
                if let Some(field) = &found {
                    self.learned.borrow_mut().flag_field = Some(field.clone());
                }
                found.flatten()
            }
            None => None,
        }
    }

    /// Answers seam call `call` (`assigned`, `issues`, ...).
    pub async fn serve(&self, call: &str, payload: &str) -> Result<String, String> {
        self.answer(call, payload).await.map_err(|e| e.to_err())
    }

    async fn answer(&self, call: &str, payload: &str) -> Answer {
        let hello: seam::Hello = serde_json::from_str(payload).map_err(bad)?;
        if hello.seam != seam::SEAM {
            return Err(SeamError::new(code::UNSUPPORTED, format!("Jira speaks the tracker seam {}, not {}", seam::SEAM, hello.seam)));
        }
        if self.settings.base().is_none() {
            return Err(SeamError::new(code::NOT_SET_UP, about_for(&self.settings, false, self.deployment()).not_ready.unwrap_or_default()));
        }
        match call {
            seam::call::ASSIGNED => self.assigned(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::ISSUES => self.issues(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::TRANSITIONS => self.transitions(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::TRANSITION | seam::call::UPDATE | seam::call::COMMENT | seam::call::WORKLOG => self.write(call, payload).await,
            seam::call::PRIORITIES => {
                let ps = self.client.list_priorities().await.map_err(|e| self.fail(e))?;
                json(&seam::Priorities { priorities: ps.into_iter().map(|p| seam::Priority { level: self.settings.level(&p.name).into(), name: p.name }).collect() }.cleaned())
            }
            seam::call::PEOPLE if self.deployment().is_cloud() => {
                let one: seam::One = serde_json::from_str(payload).map_err(bad)?;
                let key = key_of(&one.key);
                let found = self.client.assignable(&key).await.map_err(|e| self.fail(e))?;
                let people = found.into_iter().map(|(id, name)| seam::Person { id, name }).collect();
                json(&seam::People { people, none: Some(format!("no one can be assigned {key} in Jira -- the project's Assignable User permission says who")) }.cleaned())
            }
            seam::call::PEOPLE => {
                let people = self.settings.users.iter().map(|(id, name)| seam::Person { id: id.clone(), name: name.clone() }).collect();
                json(&seam::People { people, none: Some(format!("no people to assign to -- a on the Jira page ({}) adds one", self.settings.keys.page)) }.cleaned())
            }
            other => Err(SeamError::new(code::UNSUPPORTED, format!("no service jira.tracker.{other}"))),
        }
    }

    /// Every call that changes something in Jira, in one place: whatever
    /// is to stand in front of the seam's writes stands here.
    async fn write(&self, call: &str, payload: &str) -> Answer {
        match call {
            seam::call::TRANSITION => self.transition(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::UPDATE => self.update(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::COMMENT => {
                let c: seam::NewComment = serde_json::from_str(payload).map_err(bad)?;
                self.client.add_comment(&c.key, &c.body).await.map_err(|e| self.fail(e))?;
                json(&seam::Said::default())
            }
            seam::call::WORKLOG => {
                let w: seam::Worklog = serde_json::from_str(payload).map_err(bad)?;
                let started = jira_time(&w.started).map_err(|e| SeamError::new(code::BAD_REQUEST, e))?;
                self.client.add_worklog_at(&w.key, w.seconds, &started).await.map_err(|e| self.fail(e))?;
                json(&seam::Said::default())
            }
            other => Err(SeamError::new(code::UNSUPPORTED, format!("no service jira.tracker.{other}"))),
        }
    }

    /// Open issues assigned to you, as the agenda's import always asked.
    async fn assigned(&self, req: seam::Assigned) -> Answer {
        let mut jql = build_my_open_issues_jql(&self.settings.projects);
        if let Some(since) = req.since.as_deref().and_then(jql_time) {
            jql = jql.replacen(" ORDER BY", &format!(" AND updated >= \"{since}\" ORDER BY"), 1);
        }
        let me = self.me().await;
        let flag = self.flag(false).await;
        if self.deployment().is_cloud() {
            let (found, next) = self.client.search_details_after(&jql, req.cursor.as_deref(), req.limit(), flag.as_deref()).await.map_err(|e| self.fail(e))?;
            return json(&seam::Page { issues: found.iter().filter_map(|d| self.issue(d, me.as_deref())).collect(), next }.cleaned());
        }
        let start: u32 = req.cursor.as_deref().and_then(|c| c.parse().ok()).unwrap_or(0);
        let (found, total) = self.client.search_details_page(&jql, start, req.limit(), flag.as_deref()).await.map_err(|e| self.fail(e))?;
        let end = start + found.len() as u32;
        let next = (!found.is_empty() && end < total).then(|| end.to_string());
        json(&seam::Page { issues: found.iter().filter_map(|d| self.issue(d, me.as_deref())).collect(), next }.cleaned())
    }

    /// These issues: one fetched directly, more in one search.
    async fn issues(&self, req: seam::Issues) -> Answer {
        if req.refs.len() > seam::Issues::MAX_REFS {
            return Err(SeamError::new(code::BAD_REQUEST, format!("at most {} refs a call", seam::Issues::MAX_REFS)));
        }
        let keys: Vec<String> = req.refs.iter().map(|r| key_of(r)).collect();
        let me = self.me().await;
        let flag = self.flag(true).await;
        let details = match keys.as_slice() {
            [] => Vec::new(),
            [key] => vec![self.client.get_issue_with(key, flag.as_deref()).await.map_err(|e| self.fail(e))?],
            many => self.client.search_details(&build_keys_jql(many), many.len() as u32, flag.as_deref()).await.map_err(|e| self.fail(e))?,
        };
        let issues: Vec<seam::Issue> = details.iter().filter_map(|d| self.issue(d, me.as_deref())).collect();
        let missing = req.refs.iter().zip(&keys).filter(|(_, k)| !issues.iter().any(|i| &i.key == *k)).map(|(r, _)| r.clone()).collect();
        json(&seam::Found { issues, missing }.cleaned())
    }

    fn moved(&self, t: &Transition, project: &str, flagged: bool) -> seam::Move {
        seam::Move {
            id: t.id.clone(),
            name: t.name.clone(),
            to: seam::State { id: t.to_id.clone(), name: t.to_name.clone(), stage: stage(&t.to_category).into(), column: self.settings.column(project, &t.to_id, &t.to_category, flagged).into() },
            ..Default::default()
        }
    }

    /// What can be done next: the workflow's transitions, and what
    /// Blocked means in the issue's project.
    async fn transitions(&self, req: seam::Transitions) -> Answer {
        let project = project_of(&req.key).to_string();
        let blocked = self.settings.blocked_for(&project);
        let from = req.from.clone().unwrap_or_default();
        let flagged = from.column == column::BLOCKED && blocked == Some(Blocked::Flag);
        let here = |column: &str| seam::State { column: column.into(), ..from.clone() };
        let label = self.settings.blocked_label(&project);
        let answer = |transitions: Vec<seam::Move>| seam::Moves { transitions, blocked: label.clone(), ..Default::default() };
        let page = &self.settings.keys.page;
        match req.toward.as_deref() {
            Some(column::BLOCKED) => match blocked {
                Some(Blocked::Local) => json(&answer(vec![seam::Move { id: "local".into(), name: "Keep Blocked local".into(), local: true, to: here(column::BLOCKED), ..Default::default() }]).cleaned()),
                Some(Blocked::Flag) => json(&answer(vec![seam::Move { id: "flag".into(), name: "Flag".into(), to: here(column::BLOCKED), said: Some("flagged".into()), ..Default::default() }]).cleaned()),
                Some(Blocked::Status { id, name }) => {
                    let ts = self.client.list_transitions(&req.key).await.map_err(|e| self.fail(e))?;
                    let moves: Vec<seam::Move> = ts.iter().map(|t| self.moved(t, &project, flagged)).collect();
                    match moves.iter().find(|m| m.to.id == id) {
                        Some(m) => json(&answer(vec![m.clone()]).cleaned()),
                        None => json(&seam::Moves { why: Some(format!("\"{name}\" isn't reachable from \"{}\" -- pick a step from here", from.name)), ..answer(moves) }.cleaned()),
                    }
                }
                None => {
                    let ts = self.client.list_transitions(&req.key).await.map_err(|e| self.fail(e))?;
                    let mut moves: Vec<seam::Move> = ts
                        .iter()
                        .filter(|t| !t.to_id.is_empty())
                        .map(|t| {
                            let meaning = Blocked::Status { id: t.to_id.clone(), name: t.to_name.clone() };
                            let mut m = self.moved(t, &project, flagged);
                            m.to.column = column::BLOCKED.into();
                            m.label = Some(format!("{}   (via \"{}\")", t.to_name, t.name));
                            m.note = Some(format!("Blocked in {project} now moves the issue to \"{}\"", t.to_name));
                            m.remember = Some(remember(&project, &meaning));
                            m
                        })
                        .collect();
                    moves.push(seam::Move {
                        id: "flag".into(),
                        name: "Flag".into(),
                        label: Some("Flag as impediment (keep the status)".into()),
                        to: here(column::BLOCKED),
                        remember: Some(remember(&project, &Blocked::Flag)),
                        note: Some(format!("Blocked in {project} now flags the issue as an impediment")),
                        said: Some("flagged".into()),
                        ..Default::default()
                    });
                    moves.push(seam::Move {
                        id: "local".into(),
                        name: "Keep Blocked local".into(),
                        label: Some(format!("Keep Blocked local for {project}")),
                        local: true,
                        to: here(column::BLOCKED),
                        remember: Some(remember(&project, &Blocked::Local)),
                        note: Some(format!("Blocked in {project} now stays in your agenda only")),
                        ..Default::default()
                    });
                    let ask = seam::Ask { title: format!("What does Blocked mean in {project}?"), note: Some(format!("Pick what Blocked means in {project} -- asked once, then remembered (b on the project in {page} changes it)")) };
                    json(&seam::Moves { ask: Some(ask), ..answer(moves) }.cleaned())
                }
            },
            // Out of Blocked while flagged: unflag, and move on too when
            // the column needs it.
            Some(toward) if flagged => {
                if from.stage == toward {
                    return json(&answer(vec![seam::Move { id: "unflag".into(), name: "Unflag".into(), to: here(toward), said: Some("unflagged".into()), ..Default::default() }]).cleaned());
                }
                let ts = self.client.list_transitions(&req.key).await.map_err(|e| self.fail(e))?;
                let moves = ts
                    .iter()
                    .map(|t| {
                        let mut m = self.moved(t, &project, false);
                        m.said = Some(format!("unflagged, and moved to {}", t.to_name));
                        m.id = format!("unflag+{}", t.id);
                        m
                    })
                    .collect();
                json(&answer(moves).cleaned())
            }
            _ => {
                let ts = self.client.list_transitions(&req.key).await.map_err(|e| self.fail(e))?;
                json(&answer(ts.iter().map(|t| self.moved(t, &project, flagged)).collect()).cleaned())
            }
        }
    }

    async fn set_flag(&self, key: &str, on: bool) -> Result<(), SeamError> {
        let known = self.learned.borrow().flag_field.clone();
        let field = match known {
            Some(field) => field,
            None => {
                let found = self.client.find_flagged_field().await.map_err(|e| self.fail(e))?;
                self.learned.borrow_mut().flag_field = Some(found.clone());
                found
            }
        };
        match field {
            Some(field) => self.client.set_flagged(key, &field, on).await.map_err(|e| self.fail(e)),
            None => Err(SeamError::new(
                code::REFUSED,
                format!("this Jira has no Flagged field -- press b on the project's search on the Jira page ({}) to pick another meaning for Blocked", self.settings.keys.page),
            )),
        }
    }

    /// Doing a move: a workflow transition, the Flagged field, or both.
    async fn transition(&self, req: seam::Transition) -> Answer {
        let said = match req.id.as_str() {
            "local" => None,
            "flag" => {
                self.set_flag(&req.key, true).await?;
                Some("flagged".to_string())
            }
            "unflag" => {
                self.set_flag(&req.key, false).await?;
                Some("unflagged".to_string())
            }
            id => {
                let id = match id.strip_prefix("unflag+") {
                    Some(id) => {
                        self.set_flag(&req.key, false).await?;
                        id
                    }
                    None => id,
                };
                self.client.apply_transition(&req.key, id).await.map_err(|e| self.fail(e))?;
                None
            }
        };
        json(&seam::Said { said })
    }

    async fn update(&self, req: seam::Update) -> Answer {
        let key = req.key.clone();
        let c = &self.client;
        let done = match req.change().map_err(|e| SeamError::new(code::BAD_REQUEST, e))? {
            Change::Title(s) => c.update_summary(&key, &s).await,
            Change::Body(_) if c.is_cloud() => return Err(SeamError::new(code::REFUSED, crate::CLOUD_DESCRIPTION)),
            Change::Body(s) => c.update_description(&key, &s).await,
            Change::Priority(p) => c.update_priority(&key, &p).await,
            Change::Due(d) => c.update_due(&key, d.as_deref()).await,
            Change::Assignee(p) => c.update_assignee(&key, &p.id).await,
        };
        done.map_err(|e| self.fail(e))?;
        json(&seam::Said::default())
    }
}

/// An `updated` as JQL takes it, `yyyy/MM/dd HH:mm`.
fn jql_time(updated: &str) -> Option<String> {
    let b = updated.as_bytes();
    let ok = b.len() >= 16 && b[4] == b'-' && b[7] == b'-' && b[10] == b'T' && b[13] == b':';
    ok.then(|| format!("{}/{}/{} {}", &updated[0..4], &updated[5..7], &updated[8..10], &updated[11..16]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_reads_as_jira_blocked_holds_it() {
        assert_eq!(Blocked::parse("flag"), Ok(Blocked::Flag));
        assert_eq!(Blocked::parse(" 10103: On Hold "), Ok(Blocked::Status { id: "10103".into(), name: "On Hold".into() }));
        assert!(Blocked::parse("nonsense").is_err());
        assert_eq!(parse_remember(&remember("PROJ", &Blocked::Local)), Some(("PROJ".to_string(), Blocked::Local)));
    }

    #[test]
    fn guess_level_covers_both_common_schemes() {
        for (name, want) in [("Highest", "urgent"), ("Blocker", "urgent"), ("High", "high"), ("Major", "high"), ("Medium", "medium"), ("Lowest", "low"), ("Trivial", "low"), ("P3 - whatever", "medium")] {
            assert_eq!(guess_level(name), want, "{name}");
        }
        let s = Settings { priorities: vec![("P1".into(), "Urgent".into())], ..Default::default() };
        assert_eq!(s.level("p1"), "urgent");
        assert_eq!(s.level("Minor"), "low");
    }

    #[test]
    fn errors_are_classed_with_jiras_words_kept() {
        let cases = [
            ("HTTP 401", code::SIGNED_OUT),
            ("HTTP 404 (Issue Does Not Exist)", code::NOT_FOUND),
            ("HTTP 429", code::RATE_LIMITED),
            ("HTTP 400 (priority: bad)", code::REFUSED),
            ("HTTP 403", code::REFUSED),
            ("HTTP 503", code::FAILED),
            ("request failed: timed out", code::OFFLINE),
            ("Jira isn't set up yet -- set its server", code::NOT_SET_UP),
        ];
        for (message, want) in cases {
            let err = classify(message.to_string());
            assert_eq!((err.code.as_str(), err.message.as_str()), (want, message));
        }
    }

    #[test]
    fn a_ref_may_be_its_address() {
        assert_eq!(key_of("PROJ-1"), "PROJ-1");
        assert_eq!(key_of(" proj-1 "), "PROJ-1", "typed in lower case, as the agenda took it");
        assert_eq!(key_of("https://jira.example.com/browse/PROJ-1?focusedCommentId=3"), "PROJ-1");
    }

    #[test]
    fn times_are_written_as_jira_and_jql_want_them() {
        assert_eq!(jira_time("2026-11-12T09:15:00.000+01:00").unwrap(), "2026-11-12T09:15:00.000+0100");
        assert_eq!(jira_time("2026-11-12T09:15:00Z").unwrap(), "2026-11-12T09:15:00.000+0000");
        assert!(jira_time("Tuesday").is_err());
        assert_eq!(jql_time("2026-11-14T09:30:00.000+0000").as_deref(), Some("2026/11/14 09:30"));
        assert_eq!(jql_time("soon"), None);
    }

    #[test]
    fn about_says_what_the_agenda_said_before_when_its_not_set_up() {
        let s = Settings { keys: Keys { settings: "C-,".into(), page: "C-k j j".into() }, sync_minutes: 5, ..Default::default() };
        let a = about(&s, false);
        assert_eq!(a.not_ready.as_deref(), Some("Jira isn't set up yet -- set its server and token in C-, (Jira)"));
        assert_eq!((a.sync_minutes, a.browse), (5, None));
        let s = Settings { base_url: Some("https://jira.example.com/".into()), ..s };
        assert_eq!(about(&s, true).browse.as_deref(), Some("https://jira.example.com/browse/{ref}"));
    }
}
