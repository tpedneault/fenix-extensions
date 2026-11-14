//! The trackers the agenda finds (`plans/tracker-seam.md` §2): Fenix 1.5
//! can't say who serves a service, so the agenda asks each tracker it
//! knows of -- [`KNOWN`], then the ids in `agenda.trackers` -- for
//! `<id>.tracker.about`. One that isn't installed answers at once that
//! nothing offers it.

use std::rc::Rc;

use fenix_tracker::{self as seam, About, SeamError};

use crate::calls;
use crate::legacy::{self, Legacy};

/// The trackers in this repository, by the name their services start
/// with -- a tracker added to the store is added here, in a minor
/// release of the agenda.
pub const KNOWN: [&str; 4] = ["jira", "github-issues", "gitlab-issues", "linear"];

/// One tracker found: who it is, and how it's reached.
#[derive(Clone)]
pub struct Tracker {
    pub id: String,
    pub about: About,
    /// Served by the agenda itself, for a Jira extension from before the
    /// seam.
    pub legacy: Option<Rc<Legacy>>,
}

impl Tracker {
    pub fn name(&self) -> &str {
        &self.about.name
    }

    pub fn can(&self, capability: &str) -> bool {
        self.about.can(capability)
    }

    pub fn ready(&self) -> bool {
        self.about.ready
    }

    /// How a job reaches it.
    pub fn via(&self) -> Via {
        Via { id: self.id.clone(), name: self.about.name.clone(), legacy: self.legacy.clone() }
    }
}

/// How to reach one tracker, for a job to keep.
#[derive(Clone)]
pub struct Via {
    pub id: String,
    pub name: String,
    pub legacy: Option<Rc<Legacy>>,
}

impl Via {
    /// Seam call `call`, asked with `request`.
    pub async fn ask(&self, call: &str, request: &impl serde::Serialize) -> Result<String, SeamError> {
        let payload = serde_json::to_string(request).map_err(|e| SeamError::new(seam::code::FAILED, e.to_string()))?;
        match &self.legacy {
            Some(legacy) => legacy.serve(call, &payload).await,
            None => calls::call(&seam::service(&self.id, call), &payload).await.map_err(|e| SeamError::from_err(&e)),
        }
    }

    /// `create`, which waits on you filling in a form.
    pub async fn ask_unbounded(&self, call: &str, request: &impl serde::Serialize) -> Result<String, SeamError> {
        let payload = serde_json::to_string(request).map_err(|e| SeamError::new(seam::code::FAILED, e.to_string()))?;
        match &self.legacy {
            Some(legacy) => legacy.serve(call, &payload).await,
            None => calls::call_unbounded(&seam::service(&self.id, call), &payload).await.map_err(|e| SeamError::from_err(&e)),
        }
    }

    /// Seam call `call`, its answer read as `T`.
    pub async fn get<T: serde::de::DeserializeOwned>(&self, call: &str, request: &impl serde::Serialize) -> Result<T, SeamError> {
        let text = self.ask(call, request).await?;
        serde_json::from_str(&text).map_err(|e| SeamError::new(seam::code::FAILED, format!("{} answered {call} oddly: {e}", self.name)))
    }
}

/// The ids to ask: the known ones, then `agenda.trackers`' (a list,
/// commas between).
pub fn ids(setting: Option<&str>) -> Vec<String> {
    let mut ids: Vec<String> = KNOWN.iter().map(|s| s.to_string()).collect();
    for id in setting.unwrap_or_default().split([',', '\n']).map(str::trim).filter(|s| !s.is_empty()) {
        if !ids.iter().any(|i| i == id) && fenix_tracker::clean::key(id).is_some() {
            ids.push(id.to_string());
        }
    }
    ids
}

/// What asking one id found.
pub enum Found {
    /// A tracker, ready or not.
    Tracker(Tracker),
    /// Installed, but speaking another major of the seam.
    Other { id: String, name: String, seam: u32 },
    /// Nothing answers to it.
    Absent,
}

/// Asks `id` who it is.
pub async fn probe(id: String) -> Found {
    let answer = calls::call(&seam::service(&id, seam::call::ABOUT), &serde_json::to_string(&seam::Hello::default()).unwrap_or_default()).await;
    match answer.map_err(|e| SeamError::from_err(&e)).and_then(|text| serde_json::from_str::<About>(&text).map_err(|e| SeamError::new(seam::code::FAILED, e.to_string()))) {
        Ok(about) if about.seam != seam::SEAM => Found::Other { id, name: fenix_tracker::clean::name(&about.name), seam: about.seam },
        Ok(about) => Found::Tracker(Tracker { id, about: about.cleaned(), legacy: None }),
        Err(err) if id == legacy::ID && err.is(seam::code::UNSUPPORTED) => match legacy::probe().await {
            Some(old) => Found::Tracker(Tracker { about: old.about().cleaned(), id, legacy: Some(old) }),
            None => Found::Absent,
        },
        Err(_) => Found::Absent,
    }
}

/// The trackers found, in the order asked.
#[derive(Default, Clone)]
pub struct Trackers {
    pub list: Vec<Tracker>,
}

impl Trackers {
    pub fn get(&self, id: &str) -> Option<&Tracker> {
        self.list.iter().find(|t| t.id == id)
    }

    pub fn ready(&self) -> impl Iterator<Item = &Tracker> {
        self.list.iter().filter(|t| t.ready())
    }

    /// What tracker `id` is called; its id when it isn't installed.
    pub fn name(&self, id: &str) -> String {
        self.get(id).map_or_else(|| id.to_string(), |t| t.name().to_string())
    }

    /// Whether tracker `id` is ready and can do `capability`.
    pub fn can(&self, id: &str, capability: &str) -> bool {
        self.get(id).is_some_and(|t| t.ready() && t.can(capability))
    }

    /// The one tracker's name, when there's one: the only one ready, or
    /// the only one there.
    pub fn one(&self) -> Option<String> {
        let mut ready = self.ready();
        match (ready.next(), ready.next()) {
            (Some(one), None) => Some(one.name().to_string()),
            (Some(_), Some(_)) => None,
            _ => match self.list.as_slice() {
                [one] => Some(one.name().to_string()),
                _ => None,
            },
        }
    }

    /// The one tracker's name when there's one, else "tracker" -- for
    /// words about all of them ("Synced 3 issues with Jira").
    pub fn label(&self) -> String {
        self.one().unwrap_or_else(|| "tracker".to_string())
    }

    /// The names of `ids`, as a sentence: "Jira and Linear".
    pub fn names(&self, ids: &[String]) -> String {
        let names: Vec<String> = ids.iter().map(|id| self.name(id)).collect();
        match names.as_slice() {
            [] => self.label(),
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        }
    }

    /// The shortest refresh any ready tracker asks for, in minutes.
    pub fn sync_minutes(&self) -> u32 {
        self.ready().map(|t| t.about.sync_minutes).filter(|m| *m > 0).min().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker(id: &str, name: &str, ready: bool) -> Tracker {
        let about = About { seam: 1, minor: 0, name: name.into(), ready, not_ready: None, can: vec!["edit".into()], sync_minutes: 10, project_meta: None, browse: None, ref_hint: None };
        Tracker { id: id.into(), about, legacy: None }
    }

    #[test]
    fn the_ids_asked_are_the_known_ones_then_yours() {
        assert_eq!(ids(None), KNOWN);
        assert_eq!(ids(Some("acme.youtrack, jira,, bad id")), ["jira", "github-issues", "gitlab-issues", "linear", "acme.youtrack"]);
    }

    #[test]
    fn words_name_the_one_tracker_or_say_tracker() {
        let mut t = Trackers { list: vec![tracker("jira", "Jira", true)] };
        assert_eq!(t.label(), "Jira");
        t.list.push(tracker("linear", "Linear", false));
        assert_eq!(t.label(), "Jira", "the only one ready");
        t.list[1].about.ready = true;
        assert_eq!(t.label(), "tracker");
        assert_eq!(t.names(&["jira".into(), "linear".into()]), "Jira and Linear");
        assert!(t.can("jira", "edit") && !t.can("jira", "worklog") && !t.can("gone", "edit"));
        assert_eq!(t.name("gone"), "gone");
    }
}
