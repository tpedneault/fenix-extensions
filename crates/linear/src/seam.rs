//! The tracker seam (`plans/tracker-seam.md`) served for Linear over a
//! [`Linear`]: what the agenda asks of a tracker, answered from Linear's
//! GraphQL API. The extension serves it as `linear.tracker.*`, and the
//! agenda's tests run it on recorded responses.
//!
//! `about` needs the extension (is there a key?): [`about`] is for the
//! caller. `create` is the extension's too, since it's Linear's page that
//! opens the form: [`Seam::serve`] answers `unsupported` to it.

use fenix_tracker::{self as seam, can, code, SeamError};

use crate::linear::{Linear, View};
use crate::model::{self, PRIORITIES};
use crate::refs;

/// This tracker's extension id: what its services start with
/// (`fenix_tracker::service(ID, call)`).
pub const ID: &str = "fenix.linear";

/// The keys the messages name, as the key scheme in use writes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    /// The settings page (`settings.open`).
    pub settings: String,
    /// Linear's page.
    pub page: String,
}

impl Default for Keys {
    fn default() -> Self {
        Keys { settings: "SPC ,".into(), page: "SPC o w w".into() }
    }
}

/// What the seam reads of the extension's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// There is an API key.
    pub ready: bool,
    pub sync_minutes: u32,
    pub keys: Keys,
    /// The key of the team the Fenix project you're in belongs to
    /// (`linear.team`, a setting a project may set): `project`'s answer.
    pub team: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { ready: true, sync_minutes: 10, keys: Keys::default(), team: None }
    }
}

/// What to do when there's no key.
pub fn not_ready(keys: &Keys) -> String {
    format!("Linear isn't set up yet -- add a personal API key in {} (Linear)", keys.settings)
}

/// `about`: Linear, ready once it has a key. Answered whatever `seam` it
/// was asked with, from no more than the settings.
pub fn about(settings: &Settings) -> seam::About {
    seam::About {
        seam: seam::SEAM,
        seams: seam::SEAMS.to_vec(),
        minor: seam::MINOR,
        id: ID.into(),
        name: "Linear".into(),
        ready: settings.ready,
        not_ready: (!settings.ready).then(|| not_ready(&settings.keys)),
        can: [can::COMMENTS, can::EDIT_TITLE, can::EDIT_BODY, can::EDIT_PRIORITY, can::EDIT_DUE, can::EDIT_ASSIGNEE, can::EDIT_LABELS, can::COMMENT, can::CREATE, can::PROJECT].iter().map(|c| c.to_string()).collect(),
        sync_minutes: settings.sync_minutes.max(5),
        // Fenix's project metadata has a Jira key and nothing for Linear:
        // `project` says which team the project is.
        project_meta: None,
        browse: Some("https://linear.app/issue/{ref}".into()),
        ref_hint: Some("ENG-123".into()),
        // Linear has no Blocked state of its own: the agenda keeps it.
        blocked: false,
    }
}

/// A key Linear refused, said with what to do about it.
pub fn refit(err: SeamError, keys: &Keys) -> SeamError {
    if err.is(code::SIGNED_OUT) {
        let message = format!("Linear didn't accept your API key ({}) -- make a new one and set it on the settings page ({}), under Linear", err.message, keys.settings);
        return SeamError { message: fenix_tracker::clean::message(&message), ..err };
    }
    err
}

type Answer = Result<String, SeamError>;

fn json<T: serde::Serialize>(value: &T) -> Answer {
    serde_json::to_string(value).map_err(|e| SeamError::new(code::FAILED, e.to_string()))
}

fn bad(e: serde_json::Error) -> SeamError {
    SeamError::new(code::BAD_REQUEST, format!("a Linear call: {e}"))
}

/// The seam over one Linear.
#[derive(Clone)]
pub struct Seam {
    pub linear: Linear,
    pub settings: Settings,
}

impl Seam {
    pub fn new(linear: Linear, settings: Settings) -> Seam {
        Seam { linear, settings }
    }

    /// Answers seam call `call` (`assigned`, `issues`, ...).
    pub async fn serve(&self, call: &str, payload: &str) -> Result<String, String> {
        self.answer(call, payload).await.map_err(|e| refit(e, &self.settings.keys).to_err())
    }

    async fn answer(&self, call: &str, payload: &str) -> Answer {
        let hello: seam::Hello = serde_json::from_str(payload).map_err(bad)?;
        if !seam::SEAMS.contains(&hello.seam) {
            return Err(SeamError::new(code::UNSUPPORTED, format!("Linear speaks the tracker seam {:?}, not {}", seam::SEAMS, hello.seam)));
        }
        if !self.settings.ready {
            return Err(SeamError::new(code::NOT_SET_UP, not_ready(&self.settings.keys)));
        }
        match call {
            seam::call::ASSIGNED => self.assigned(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::ISSUES => self.issues(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::TRANSITIONS => self.transitions(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::TRANSITION | seam::call::UPDATE | seam::call::COMMENT => self.write(call, payload).await,
            seam::call::PRIORITIES => json(&seam::Priorities { priorities: PRIORITIES.iter().map(|(_, name, level)| seam::Priority { name: (*name).into(), level: level.map(Into::into) }).collect() }.cleaned()),
            seam::call::PEOPLE => {
                let one: seam::One = serde_json::from_str(payload).map_err(bad)?;
                let people = self.linear.people(&one.key).await?;
                json(&seam::People { people, none: Some("no one to assign to -- this issue's team has no active members".into()) }.cleaned())
            }
            seam::call::PROJECT => json(&seam::Project { project: self.settings.team.clone().filter(|t| !t.trim().is_empty()) }),
            other => Err(SeamError::new(code::UNSUPPORTED, format!("no service {}", seam::service(ID, other)))),
        }
    }

    /// Every write the seam makes goes through here, so a gate (the
    /// provider asking the user before the first write of a session) and an
    /// `op` id for de-duplication have one place to go. Writes are never
    /// retried here: a write that got no answer is the caller's to treat as
    /// unknown.
    async fn write(&self, call: &str, payload: &str) -> Answer {
        match call {
            seam::call::TRANSITION => self.transition(serde_json::from_str(payload).map_err(bad)?).await,
            seam::call::UPDATE => self.update(serde_json::from_str(payload).map_err(bad)?).await,
            _ => {
                let c: seam::NewComment = serde_json::from_str(payload).map_err(bad)?;
                if c.body.len() > 64 * 1024 {
                    return Err(SeamError::new(code::BAD_REQUEST, "a comment is at most 64 KiB"));
                }
                self.linear.comment(&c.key, &c.body).await?;
                json(&seam::Said::default())
            }
        }
    }

    /// Your open issues.
    async fn assigned(&self, req: seam::Assigned) -> Answer {
        let page = self.linear.list(&View::Assigned, req.limit(), req.cursor.as_deref()).await?;
        json(&seam::Page { issues: page.issues.into_iter().map(|d| d.issue).collect(), next: page.next }.cleaned()?)
    }

    async fn issues(&self, req: seam::Issues) -> Answer {
        if req.refs.len() > seam::Issues::MAX_REFS {
            return Err(SeamError::new(code::BAD_REQUEST, format!("at most {} refs a call", seam::Issues::MAX_REFS)));
        }
        let (found, missing) = self.linear.issues(&req.refs).await?;
        json(&seam::Found { issues: found.into_iter().map(|d| d.issue).collect(), missing }.cleaned())
    }

    /// What can be done next: one move per workflow state of the issue's
    /// team, but the one it's in, in the order of Linear's board. Linear
    /// has no Blocked (`about.blocked` is false), so the agenda never asks
    /// toward it.
    async fn transitions(&self, req: seam::Transitions) -> Answer {
        let (states, current) = self.linear.states(&req.key).await?;
        let moves = states
            .into_iter()
            .filter(|s| Some(&s.id) != current.as_ref())
            .map(|s| {
                let stage = model::stage_of(&s.kind);
                seam::Move {
                    id: s.id.clone(),
                    label: Some(format!("{}  ({})", s.name, s.kind)),
                    to: seam::State { id: s.id, name: s.name.clone(), stage: stage.into(), column: stage.into() },
                    name: s.name,
                    ..Default::default()
                }
            })
            .collect();
        json(&seam::Moves { transitions: moves, ..Default::default() }.cleaned())
    }

    /// Doing a move: the issue's state set to the one it names.
    async fn transition(&self, req: seam::Transition) -> Answer {
        let moved = self.linear.set_state(&req.key, &req.id).await?;
        json(&seam::Said { said: Some(format!("moved to {}", moved.issue.state.name)) })
    }

    async fn update(&self, req: seam::Update) -> Answer {
        let change = req.change()?;
        self.linear.change(&req.key, &change).await?;
        json(&seam::Said::default())
    }
}

/// `ENG-12` for what the agenda calls an issue: its identifier, or an
/// address. For messages and the page.
pub fn identifier_of(r: &str) -> Option<String> {
    refs::parse(r).map(|n| n.text())
}
