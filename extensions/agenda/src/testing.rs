//! The agenda's test world, for the agenda's own tests and a tracker's
//! (`plans/tracker-seam.md` §8): the agenda started on the test kit's fake
//! host, with the services you wire, keeping `agenda.json` in a folder of
//! the test's own.
//!
//! In a tracker's crate:
//!
//! ```toml
//! [dev-dependencies]
//! fenix-agenda = { path = "../agenda", features = ["testing"] }
//! fenix-test-kit = { path = "../../crates/test-kit" }
//! ```
//!
//! ```rust,ignore
//! use fenix_agenda::testing::{serve, world_with};
//! let tracker = fenix_tracker::FakeTracker::new("Mine");   // or your own answers:
//! let mut w = world_with("my-test", &["acme.mine"], |fake| serve(fake, "acme.mine", &tracker));
//! // fake.on_service_with(&fenix_tracker::service("acme.mine", "issues"), |payload| my_seam(payload))
//! let task = w.add("acme.mine", "MINE-1");
//! w.set_status(task, fenix_agenda::model::Status::Done);
//! ```
//!
//! Every known tracker you don't wire answers as not installed; a
//! `FakeTracker` wired with `serve` answers `about` with the id it's
//! served under.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use fenix_test_kit::{Fake, Harness};
use fenix_tracker::{self as seam, call, FakeTracker, KNOWN};

use crate::model::{Status, Task, TaskId};
use crate::page::Action;
use crate::Agenda;

/// What no payload the agenda sends may hold: the token the fake host
/// keeps, and the headers a token travels in.
pub const TOKEN: &str = "NjQ1MzIxOTg3NjU0OlRPS0VO-secret";

/// A folder of the test's own, gone when it is.
pub struct Folder(pub PathBuf);

impl Folder {
    pub fn new(name: &str) -> Folder {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("fenix-agenda-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&path).unwrap();
        Folder(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Tracker `id` isn't installed: Fenix says nothing offers its calls.
pub fn absent(fake: &Fake, id: &str) {
    let about = seam::service(id, call::ABOUT);
    fake.on_service(&about, Err(&format!("nothing offers {about}")));
    if id == crate::legacy::ID {
        fake.on_service("jira.config", Err("nothing offers jira.config"));
    }
}

/// `t` answers `<id>.tracker.*`, every call but those in `silent` (which
/// never answer), and says it's `id`.
pub fn serve_but(fake: &Fake, id: &str, t: &FakeTracker, silent: &[&str]) {
    t.about(|a| a.id = id.to_string());
    for c in call::ALL.into_iter().filter(|c| !silent.contains(c)) {
        let t = t.clone();
        fake.on_service_with(&seam::service(id, c), move |payload| t.serve(c, payload));
    }
}

/// `t` answers `<id>.tracker.*`.
pub fn serve(fake: &Fake, id: &str, t: &FakeTracker) {
    serve_but(fake, id, t, &[]);
}

/// The agenda on a fake host.
pub struct World {
    pub h: Harness<Agenda>,
    _folder: Folder,
}

/// A fake host with `ids` set up by `wire` and every other known tracker
/// absent; the agenda started on it, keeping `agenda.json` in a folder of
/// the test's own, and every tracker asked (as opening its page would).
pub fn world_with(name: &str, ids: &[&str], wire: impl FnOnce(&Fake)) -> World {
    let fake = Fake::new();
    fake.secret("jira.token", TOKEN);
    for id in KNOWN.iter().filter(|k| !ids.contains(k)) {
        absent(&fake, id);
    }
    let extra: Vec<&str> = ids.iter().copied().filter(|i| !KNOWN.contains(i)).collect();
    if !extra.is_empty() {
        fake.setting("agenda.trackers", &extra.join(","));
    }
    wire(&fake);
    let folder = Folder::new(name);
    let agenda = Agenda { kept: Some(folder.0.join("agenda.json")), ..Default::default() };
    let mut h = Harness::wrap(fake, agenda);
    h.activate().unwrap();
    let mut w = World { h, _folder: folder };
    w.ask_trackers();
    w
}

/// `world_with` with `FakeTracker`s served under their ids.
pub fn world(name: &str, trackers: &[(&str, &FakeTracker)]) -> World {
    let ids: Vec<&str> = trackers.iter().map(|(id, _)| *id).collect();
    world_with(name, &ids, |fake| {
        for (id, t) in trackers {
            serve(fake, id, t);
        }
    })
}

impl World {
    /// Every tracker asked who it is, as the page's opening does.
    pub fn ask_trackers(&mut self) {
        self.h.ext.probe();
        self.h.settle();
    }

    /// Issue `key` of tracker `tracker` added as a task, as a tracker's
    /// page adds one (`agenda.add_issue`).
    pub fn add(&mut self, tracker: &str, key: &str) -> TaskId {
        let answer = self.h.serve("agenda.add_issue", &serde_json::json!({ "tracker": tracker, "ref": key }).to_string());
        assert_eq!(answer, Some(Ok(String::new())));
        self.find(tracker, key).unwrap_or_else(|| panic!("{key} not added: {:?}", self.h.fake.notifications()))
    }

    /// The task linked to `tracker`'s issue `key` (a ref or an id).
    pub fn find(&self, tracker: &str, key: &str) -> Option<TaskId> {
        self.h.ext.store.find_by_key(tracker, key)
    }

    pub fn task(&self, id: TaskId) -> &Task {
        self.h.ext.store.task(id).unwrap()
    }

    /// A task of its own, not linked.
    pub fn local(&mut self, title: &str) -> TaskId {
        self.h.ext.store.create_task(title.into(), String::new(), crate::model::Priority::Low, None)
    }

    /// What a key on the page asks for, carried out.
    pub fn act(&mut self, action: Action) {
        self.h.ext.page_action(action);
        self.h.settle();
    }

    /// A card moved, as `H`/`L` on the board or `s` on a local task.
    pub fn set_status(&mut self, id: TaskId, status: Status) {
        self.act(Action::SetStatus(id, status));
    }

    /// The timed syncs `later` from now, and the calls unanswered by then
    /// given up.
    pub fn later(&mut self, later: Duration) {
        let at = Instant::now() + later;
        crate::calls::expire(at);
        self.h.ext.take_done();
        self.h.ext.sync_due(at);
        self.h.settle();
    }

    /// How many changes wait to be sent.
    pub fn queued(&self) -> usize {
        self.h.ext.store.outbox.len()
    }

    /// What's been said on the message line, in order.
    pub fn said(&self) -> Vec<String> {
        self.h.fake.notifications()
    }

    pub fn last_said(&self) -> String {
        self.said().last().cloned().unwrap_or_default()
    }

    /// The last notification: its summary and body.
    pub fn toast(&self) -> (String, String) {
        let t = self.h.fake.toasts().last().cloned().expect("a notification");
        (t.summary, t.body)
    }

    /// Every payload the agenda sent another extension: none holds a
    /// token or a header.
    pub fn no_secrets(&self) {
        for s in self.h.fake.services() {
            for bad in [TOKEN, "Authorization", "Bearer", "PRIVATE-TOKEN"] {
                assert!(!s.payload.contains(bad), "{bad} in {} {}", s.name, s.payload);
            }
        }
    }
}
