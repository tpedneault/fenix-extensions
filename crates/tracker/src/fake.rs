//! A tracker in memory, answering every seam call as a real one would:
//! for tests of anything that calls the seam -- the agenda's, and a
//! tracker's own, to check its answers read the same. Copy it, or wire
//! it to the test kit's fake host:
//!
//! ```rust,ignore
//! let tracker = FakeTracker::new("Fake");
//! for call in fenix_tracker::call::ALL {
//!     let t = tracker.clone();
//!     fake.on_service_with(&fenix_tracker::service("acme.fake", call), move |payload| t.serve(call, payload));
//! }
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::seam::*;
use crate::{call, can, column, SeamError, SEAM};

#[derive(Default)]
struct Inner {
    about: Option<About>,
    issues: Vec<Issue>,
    /// What can be done next, by ref; every issue gets `default_moves`
    /// when it has none of its own.
    moves: HashMap<String, Moves>,
    default_moves: Vec<Move>,
    page_size: Option<usize>,
    /// Calls that fail, by name, until taken away.
    failing: HashMap<String, SeamError>,
    /// Calls that answer as an older tracker, Fenix's `no service`.
    unserved: Vec<String>,
    /// Every call, as `name payload`.
    calls: Vec<(String, String)>,
    worklogs: Vec<Worklog>,
    remembered: Vec<String>,
    people: Vec<Person>,
    priorities: Vec<Priority>,
    next_number: u32,
}

/// A tracker in memory. Cheap to clone; every clone is the same tracker.
#[derive(Clone, Default)]
pub struct FakeTracker(Rc<RefCell<Inner>>);

/// A state, with its stage as its column.
pub fn state(id: &str, name: &str, stage: &str) -> State {
    State { id: id.into(), name: name.into(), stage: stage.into(), column: stage.into() }
}

impl FakeTracker {
    /// Ready, every capability, and the usual three moves: start, block,
    /// finish.
    pub fn new(name: &str) -> FakeTracker {
        let t = FakeTracker::default();
        {
            let mut s = t.0.borrow_mut();
            s.about = Some(About {
                seam: SEAM,
                seams: crate::SEAMS.to_vec(),
                minor: crate::MINOR,
                id: String::new(),
                name: name.into(),
                ready: true,
                not_ready: None,
                can: can::ALL.iter().map(|c| c.to_string()).collect(),
                sync_minutes: 10,
                project_meta: None,
                browse: Some("https://tracker.example/{ref}".into()),
                ref_hint: Some("FAKE-1".into()),
            });
            s.default_moves = vec![
                Move { id: "start".into(), name: "Start".into(), to: state("2", "Doing", column::DOING), ..Default::default() },
                Move { id: "block".into(), name: "Block".into(), to: State { column: column::BLOCKED.into(), ..state("4", "Stuck", column::DOING) }, ..Default::default() },
                Move { id: "finish".into(), name: "Finish".into(), to: state("3", "Done", column::DONE), ..Default::default() },
            ];
            s.people = vec![Person { id: "jo".into(), name: "Jo".into() }];
            s.priorities = vec![Priority { name: "P1".into(), level: "urgent".into() }, Priority { name: "P3".into(), level: "medium".into() }];
            s.next_number = 100;
        }
        t
    }

    /// An open issue assigned to you, as most tests want one; its id is
    /// `id-` and its ref.
    pub fn issue(key: &str, title: &str) -> Issue {
        Issue {
            id: format!("id-{key}"),
            key: key.into(),
            url: Some(format!("https://tracker.example/{key}")),
            project: key.rsplit_once('-').map(|(p, _)| p.to_string()),
            title: title.into(),
            body: format!("{title}, in more words"),
            state: state("1", "Open", column::TODO),
            priority: Some(Priority { name: "P3".into(), level: "medium".into() }),
            assignee: Some(Person { id: "me".into(), name: "Me".into() }),
            mine: Some(true),
            updated: "2026-11-14T09:00:00+00:00".into(),
            ..Default::default()
        }
    }

    pub fn add(&self, issue: Issue) -> &Self {
        self.0.borrow_mut().issues.push(issue);
        self
    }

    /// Changes issue `key` as someone in the tracker would.
    pub fn edit(&self, key: &str, f: impl FnOnce(&mut Issue)) -> &Self {
        if let Some(issue) = self.0.borrow_mut().issues.iter_mut().find(|i| i.key == key) {
            f(issue);
        }
        self
    }

    pub fn get(&self, key: &str) -> Option<Issue> {
        self.0.borrow().issues.iter().find(|i| i.key == key).cloned()
    }

    pub fn about(&self, f: impl FnOnce(&mut About)) -> &Self {
        if let Some(about) = &mut self.0.borrow_mut().about {
            f(about);
        }
        self
    }

    /// What `transitions` answers for `key`.
    pub fn moves(&self, key: &str, moves: Moves) -> &Self {
        self.0.borrow_mut().moves.insert(key.into(), moves);
        self
    }

    /// `assigned` answers at most `n` a page.
    pub fn page_size(&self, n: usize) -> &Self {
        self.0.borrow_mut().page_size = Some(n);
        self
    }

    /// Call `name` fails with `error` until [`FakeTracker::heal`].
    pub fn fail(&self, name: &str, error: SeamError) -> &Self {
        self.0.borrow_mut().failing.insert(name.into(), error);
        self
    }

    pub fn heal(&self, name: &str) -> &Self {
        self.0.borrow_mut().failing.remove(name);
        self
    }

    /// Call `name` isn't served, as by an older tracker.
    pub fn unserve(&self, name: &str) -> &Self {
        self.0.borrow_mut().unserved.push(name.into());
        self
    }

    /// Every call so far, as `(name, payload)`.
    pub fn calls(&self) -> Vec<(String, String)> {
        self.0.borrow().calls.clone()
    }

    /// The calls named `name`, their payloads.
    pub fn calls_of(&self, name: &str) -> Vec<String> {
        self.0.borrow().calls.iter().filter(|(n, _)| n == name).map(|(_, p)| p.clone()).collect()
    }

    pub fn worklogs(&self) -> Vec<Worklog> {
        self.0.borrow().worklogs.clone()
    }

    pub fn remembered(&self) -> Vec<String> {
        self.0.borrow().remembered.clone()
    }

    /// Answers `call` (`about`, `issues`, ...) with `payload`, as the
    /// service `<id>.tracker.<call>` would.
    pub fn serve(&self, name: &str, payload: &str) -> Result<String, String> {
        let mut s = self.0.borrow_mut();
        s.calls.push((name.into(), payload.into()));
        if s.unserved.iter().any(|u| u == name) {
            return Err(format!("no service fake.tracker.{name}"));
        }
        if let Some(err) = s.failing.get(name) {
            return Err(err.to_err());
        }
        let bad = |e: serde_json::Error| SeamError::new(crate::code::BAD_REQUEST, e.to_string()).to_err();
        let json = |v: &dyn erased::Json| Ok(v.json());
        match name {
            // Answered whatever seam it's asked with.
            call::ABOUT => json(s.about.as_ref().ok_or("no about")?),
            call::ASSIGNED => {
                let req: Assigned = serde_json::from_str(payload).map_err(bad)?;
                let mine: Vec<Issue> = s.issues.iter().filter(|i| i.mine == Some(true) && i.state.stage != column::DONE).cloned().collect();
                let start: usize = req.cursor.as_deref().and_then(|c| c.parse().ok()).unwrap_or(0);
                let size = (req.limit() as usize).min(s.page_size.unwrap_or(usize::MAX));
                let issues: Vec<Issue> = mine.iter().skip(start).take(size).cloned().collect();
                let next = (start + issues.len() < mine.len()).then(|| (start + issues.len()).to_string());
                json(&Page { issues, next })
            }
            call::ISSUES => {
                let req: Issues = serde_json::from_str(payload).map_err(bad)?;
                let mut found = Found::default();
                for r in &req.refs {
                    let key = r.rsplit('/').next().unwrap_or(r);
                    match s.issues.iter().find(|i| i.key == *r || i.key == key || i.id == *r) {
                        Some(issue) => found.issues.push(Issue { asked: Some(r.clone()), ..issue.clone() }),
                        None => found.missing.push(r.clone()),
                    }
                }
                json(&found)
            }
            call::TRANSITIONS => {
                let req: Transitions = serde_json::from_str(payload).map_err(bad)?;
                if !s.issues.iter().any(|i| i.key == req.key || i.id == req.key) {
                    return Err(SeamError::new(crate::code::NOT_FOUND, format!("{} isn't there", req.key)).to_err());
                }
                let moves = s.moves.get(&req.key).cloned().unwrap_or_else(|| Moves { transitions: s.default_moves.clone(), ..Default::default() });
                json(&moves)
            }
            call::TRANSITION => {
                let req: Transition = serde_json::from_str(payload).map_err(bad)?;
                let moves = s.moves.get(&req.key).map(|m| m.transitions.clone()).unwrap_or_else(|| s.default_moves.clone());
                let Some(m) = moves.into_iter().find(|m| m.id == req.id) else {
                    return Err(SeamError::new(crate::code::REFUSED, format!("no move {} from here", req.id)).to_err());
                };
                let issue = s.issues.iter_mut().find(|i| i.key == req.key || i.id == req.key).ok_or_else(|| SeamError::new(crate::code::NOT_FOUND, "gone").to_err())?;
                if !m.local {
                    issue.state = m.to.clone();
                }
                json(&Said { said: m.said.clone() })
            }
            call::UPDATE => {
                let req: Update = serde_json::from_str(payload).map_err(bad)?;
                let change = req.change().map_err(|e| e.to_err())?;
                let can = crate::can::of(&change);
                if !s.about.as_ref().is_some_and(|a| a.can(can)) {
                    return Err(SeamError::new(crate::code::UNSUPPORTED, format!("no {can} here")).to_err());
                }
                let issue = s.issues.iter_mut().find(|i| i.key == req.key || i.id == req.key).ok_or_else(|| SeamError::new(crate::code::NOT_FOUND, "gone").to_err())?;
                match change {
                    Change::Title(t) => issue.title = t,
                    Change::Body(b) => issue.body = b,
                    Change::Priority(p) => issue.priority = Some(Priority { level: if p == "P1" { "urgent".into() } else { "medium".into() }, name: p }),
                    Change::Due(d) => issue.due = d,
                    Change::Assignee(p) => {
                        issue.mine = Some(p.as_ref().is_some_and(|p| p.id == "me"));
                        issue.assignee = p;
                    }
                    Change::Labels(l) => issue.labels = l,
                }
                json(&Said::default())
            }
            call::PRIORITIES => json(&Priorities { priorities: s.priorities.clone() }),
            call::PEOPLE => json(&People { people: s.people.clone(), none: Some("no one to assign to".into()) }),
            call::COMMENT => {
                let req: NewComment = serde_json::from_str(payload).map_err(bad)?;
                let issue = s.issues.iter_mut().find(|i| i.key == req.key || i.id == req.key).ok_or_else(|| SeamError::new(crate::code::NOT_FOUND, "gone").to_err())?;
                let id = issue.comments.len().to_string();
                issue.comments.push(Comment { id, author: "Me".into(), body: req.body, created: "2026-11-14T10:00:00+00:00".into() });
                json(&Said::default())
            }
            call::WORKLOG => {
                let req: Worklog = serde_json::from_str(payload).map_err(bad)?;
                s.worklogs.push(req);
                json(&Said::default())
            }
            call::CREATE => {
                let req: Create = serde_json::from_str(payload).map_err(bad)?;
                s.next_number += 1;
                let key = format!("FAKE-{}", s.next_number);
                let issue = Issue { body: req.body, ..FakeTracker::issue(&key, &req.title) };
                s.issues.push(issue);
                json(&Created { key: Some(key) })
            }
            call::REMEMBER => {
                let req: Remember = serde_json::from_str(payload).map_err(bad)?;
                s.remembered.push(req.remember);
                json(&Said::default())
            }
            other => Err(format!("no service fake.tracker.{other}")),
        }
    }
}

/// Any answer as JSON, without naming its type at each arm.
mod erased {
    pub trait Json {
        fn json(&self) -> String;
    }

    impl<T: serde::Serialize> Json for T {
        fn json(&self) -> String {
            serde_json::to_string(self).unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_pages_whats_assigned_and_leaves_out_whats_done_or_not_mine() {
        let t = FakeTracker::new("Fake");
        for i in 1..=5 {
            t.add(FakeTracker::issue(&format!("F-{i}"), "x"));
        }
        t.edit("F-2", |i| i.mine = Some(false)).edit("F-3", |i| i.state.stage = "done".into()).page_size(2);
        let first: Page = serde_json::from_str(&t.serve("assigned", r#"{"seam":1,"limit":10}"#).unwrap()).unwrap();
        assert_eq!(first.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["F-1", "F-4"]);
        let second: Page = serde_json::from_str(&t.serve("assigned", &format!(r#"{{"seam":1,"cursor":"{}"}}"#, first.next.unwrap())).unwrap()).unwrap();
        assert_eq!(second.issues[0].key, "F-5");
        assert_eq!(second.next, None);
    }

    #[test]
    fn a_move_lands_where_it_says_and_failures_are_the_seams_errors() {
        let t = FakeTracker::new("Fake");
        t.add(FakeTracker::issue("F-1", "x"));
        t.serve("transition", r#"{"seam":1,"ref":"F-1","id":"finish"}"#).unwrap();
        assert_eq!(t.get("F-1").unwrap().state.stage, "done");
        t.fail("issues", SeamError::new(crate::code::OFFLINE, "no network"));
        let err = SeamError::from_err(&t.serve("issues", r#"{"refs":["F-1"]}"#).unwrap_err());
        assert!(err.is(crate::code::OFFLINE));
        t.unserve("worklog");
        assert!(SeamError::from_err(&t.serve("worklog", "{}").unwrap_err()).is(crate::code::UNSUPPORTED));
    }
}
