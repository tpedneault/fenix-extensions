//! The trackers the agenda finds (`plans/tracker-seam.md` §2): Fenix 1.5
//! can't say who serves a service, so the agenda asks each tracker it
//! knows of -- `fenix_tracker::KNOWN`, then the ids in `agenda.trackers`
//! -- for `<id>.tracker.about`, all at once. One that isn't installed
//! answers at once that nothing offers it; one that is, but answers as
//! another extension, or speaks no seam the agenda does, is said so, and
//! never taken for not installed.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use fenix_tracker::{self as seam, About, SeamError};

use crate::calls;
use crate::legacy::{self, Legacy};

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
    async fn send(&self, call: &str, request: &impl serde::Serialize, limit: Option<std::time::Duration>) -> Result<String, SeamError> {
        let payload = serde_json::to_string(request).map_err(|e| SeamError::new(seam::code::FAILED, e.to_string()))?;
        let text = match &self.legacy {
            Some(legacy) => legacy.serve(call, &payload).await,
            None => {
                let name = seam::service(&self.id, call);
                let reply = match limit {
                    Some(limit) => calls::call_for(&name, &payload, limit),
                    None => calls::call_unbounded(&name, &payload),
                };
                reply.await.map_err(|e| SeamError::from_err(&e))
            }
        }?;
        if seam::clean::too_big(&text) {
            return Err(SeamError::new(seam::code::FAILED, format!("{} answered {call} with {} MiB, more than an answer may be", self.name, text.len() >> 20)));
        }
        Ok(text)
    }

    /// Read `call`, asked with `request`.
    pub async fn ask(&self, call: &str, request: &impl serde::Serialize) -> Result<String, SeamError> {
        self.send(call, request, Some(calls::LIMIT)).await
    }

    /// Write `call`: given longer, since it may make several requests.
    pub async fn write(&self, call: &str, request: &impl serde::Serialize) -> Result<String, SeamError> {
        self.send(call, request, Some(calls::WRITE_LIMIT)).await
    }

    /// `create`, which may wait on you filling in a form.
    pub async fn ask_unbounded(&self, call: &str, request: &impl serde::Serialize) -> Result<String, SeamError> {
        self.send(call, request, None).await
    }

    /// The answer read as `T`.
    pub fn read<T: serde::de::DeserializeOwned>(&self, call: &str, text: &str) -> Result<T, SeamError> {
        serde_json::from_str(text).map_err(|e| SeamError::new(seam::code::FAILED, format!("{} answered {call} oddly: {e}", self.name)))
    }

    /// Read `call`, its answer as `T`.
    pub async fn get<T: serde::de::DeserializeOwned>(&self, call: &str, request: &impl serde::Serialize) -> Result<T, SeamError> {
        let text = self.ask(call, request).await?;
        self.read(call, &text)
    }

    /// Write `call`, its answer as `T`.
    pub async fn put<T: serde::de::DeserializeOwned>(&self, call: &str, request: &impl serde::Serialize) -> Result<T, SeamError> {
        let text = self.write(call, request).await?;
        self.read(call, &text)
    }
}

/// The ids to ask: the known ones, then `agenda.trackers`' (a list,
/// commas between), each a whole extension id.
pub fn ids(setting: Option<&str>) -> Vec<String> {
    let mut ids: Vec<String> = seam::KNOWN.iter().map(|s| s.to_string()).collect();
    for id in setting.unwrap_or_default().split([',', '\n']).map(str::trim).filter(|s| !s.is_empty()) {
        if !ids.iter().any(|i| i == id) && seam::extension_id(id) {
            ids.push(id.to_string());
        }
    }
    ids
}

/// What asking one id found.
#[derive(Clone)]
pub enum Found {
    /// A tracker, ready or not.
    Tracker(Tracker),
    /// Installed, but speaking only other majors of the seam.
    Other { id: String, name: String, seams: Vec<u32> },
    /// Installed, but answering as another extension.
    Impostor { id: String, said: String },
    /// Installed, but its `about` failed.
    Broken { id: String, why: String },
    /// Nothing answers to it.
    Absent,
}

/// A tracker that's there but can't be used: listed, never ready, with
/// `why` as what to do.
fn unusable(id: &str, name: &str, why: String) -> Tracker {
    let about = About {
        seam: seam::SEAM,
        seams: seam::SEAMS.to_vec(),
        minor: 0,
        id: id.into(),
        name: name.into(),
        ready: false,
        not_ready: Some(why),
        can: Vec::new(),
        sync_minutes: 0,
        project_meta: None,
        browse: None,
        ref_hint: None,
        blocked: true,
    };
    Tracker { id: id.into(), about, legacy: None }
}

impl Found {
    /// What the agenda keeps of it: a tracker, ready or not, or nothing.
    pub fn tracker(&self) -> Option<Tracker> {
        match self {
            Found::Tracker(t) => Some(t.clone()),
            Found::Other { id, name, seams } => {
                let (newest, mine) = (seams.iter().max().copied().unwrap_or(0), seam::SEAMS.iter().max().copied().unwrap_or(0));
                let update = if newest > mine { "the agenda".to_string() } else { name.clone() };
                let seams = seams.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
                Some(unusable(id, name, format!("{name} is installed, but speaks the tracker seam {seams} and the agenda {mine} -- update {update} to link its issues")))
            }
            Found::Impostor { id, said } => Some(unusable(id, id, format!("what answers as {id} says it's {said} -- not used; see which extensions serve {id}'s names"))),
            Found::Broken { id, why } => Some(unusable(id, id, format!("{id} is installed but didn't say who it is: {why}"))),
            Found::Absent => None,
        }
    }
}

/// Asks `id` who it is.
pub async fn probe(id: String) -> Found {
    let hello = serde_json::to_string(&seam::Hello::default()).unwrap_or_default();
    let answer = calls::call(&seam::service(&id, seam::call::ABOUT), &hello).await.map_err(|e| SeamError::from_err(&e));
    match answer {
        Ok(text) if seam::clean::too_big(&text) => Found::Broken { id, why: "its answer is too big".into() },
        Ok(text) => match serde_json::from_str::<About>(&text) {
            Ok(about) if about.id != id => Found::Impostor { said: seam::clean::line(&about.id, 100), id },
            Ok(about) if seam::common_seam(&seam::SEAMS, &about.seams).is_none() => Found::Other { name: seam::clean::name(&about.name), seams: about.seams, id },
            Ok(about) => Found::Tracker(Tracker { id, about: about.cleaned(), legacy: None }),
            Err(e) => Found::Broken { id, why: e.to_string() },
        },
        Err(err) if err.absent() || err.message.starts_with("no service ") => {
            if id == legacy::ID {
                return match legacy::probe().await {
                    Some(old) => Found::Tracker(Tracker { about: old.about().cleaned(), id, legacy: Some(old) }),
                    None => Found::Absent,
                };
            }
            Found::Absent
        }
        Err(err) => Found::Broken { id, why: err.message },
    }
}

/// The trackers found, in the order asked, and what each is due.
#[derive(Default, Clone)]
pub struct Trackers {
    pub list: Vec<Tracker>,
    /// When each was last synced, and left alone until (`retry_after`).
    pub synced: HashMap<String, Instant>,
    pub backoff: HashMap<String, Instant>,
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

    /// Tracker `id` doesn't take `capability` after all (it answered
    /// `unsupported`): not offered again this session.
    pub fn drop_capability(&mut self, id: &str, capability: &str) {
        if let Some(t) = self.list.iter_mut().find(|t| t.id == id) {
            t.about.can.retain(|c| c != capability);
        }
    }

    /// Whether tracker `id` is being left alone at `now`.
    pub fn backing_off(&self, id: &str, now: Instant) -> bool {
        self.backoff.get(id).is_some_and(|until| *until > now)
    }

    /// Whether tracker `id` is due its timed sync at `now`.
    pub fn due(&self, id: &str, now: Instant) -> bool {
        let Some(t) = self.get(id).filter(|t| t.ready()) else { return false };
        let minutes = t.about.sync_minutes;
        minutes > 0 && !self.backing_off(id, now) && self.synced.get(id).is_none_or(|at| now.duration_since(*at).as_secs() >= u64::from(minutes) * 60)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker(id: &str, name: &str, ready: bool) -> Tracker {
        let mut t = unusable(id, name, String::new());
        t.about.ready = ready;
        t.about.can = vec!["edit.title".into()];
        t.about.sync_minutes = 10;
        t
    }

    #[test]
    fn the_ids_asked_are_the_known_ones_then_yours() {
        assert_eq!(ids(None), seam::KNOWN);
        assert_eq!(ids(Some("acme.youtrack, fenix.jira,, bad id, jira")), ["fenix.jira", "fenix.github-issues", "fenix.gitlab-issues", "fenix.linear", "acme.youtrack"]);
    }

    #[test]
    fn words_name_the_one_tracker_or_say_tracker() {
        let mut t = Trackers { list: vec![tracker("fenix.jira", "Jira", true)], ..Default::default() };
        assert_eq!(t.label(), "Jira");
        t.list.push(tracker("fenix.linear", "Linear", false));
        assert_eq!(t.label(), "Jira", "the only one ready");
        t.list[1].about.ready = true;
        assert_eq!(t.label(), "tracker");
        assert_eq!(t.names(&["fenix.jira".into(), "fenix.linear".into()]), "Jira and Linear");
        assert!(t.can("fenix.jira", "edit.title") && !t.can("fenix.jira", "worklog") && !t.can("gone", "edit.title"));
        t.drop_capability("fenix.jira", "edit.title");
        assert!(!t.can("fenix.jira", "edit.title"));
        assert_eq!(t.name("gone"), "gone");
    }

    #[test]
    fn each_tracker_is_due_on_its_own_schedule() {
        let now = Instant::now();
        let mut t = Trackers { list: vec![tracker("a.fast", "Fast", true), tracker("a.slow", "Slow", true)], ..Default::default() };
        t.list[0].about.sync_minutes = 5;
        t.list[1].about.sync_minutes = 60;
        t.synced.insert("a.fast".into(), now);
        t.synced.insert("a.slow".into(), now);
        let later = now + std::time::Duration::from_secs(6 * 60);
        assert!(t.due("a.fast", later) && !t.due("a.slow", later));
        t.backoff.insert("a.fast".into(), later + std::time::Duration::from_secs(1));
        assert!(!t.due("a.fast", later), "left alone until retry_after");
    }

    #[test]
    fn a_tracker_found_but_unusable_is_never_not_installed() {
        let other = Found::Other { id: "acme.x".into(), name: "X".into(), seams: vec![2] }.tracker().unwrap();
        assert!(!other.ready());
        assert_eq!(other.about.not_ready.as_deref(), Some("X is installed, but speaks the tracker seam 2 and the agenda 1 -- update the agenda to link its issues"));
        let old = Found::Other { id: "acme.x".into(), name: "X".into(), seams: vec![0] }.tracker().unwrap();
        assert!(old.about.not_ready.unwrap().ends_with("update X to link its issues"));
        assert!(Found::Impostor { id: "fenix.jira".into(), said: "acme.evil".into() }.tracker().unwrap().about.not_ready.unwrap().contains("says it's acme.evil"));
        assert!(Found::Absent.tracker().is_none());
    }
}
