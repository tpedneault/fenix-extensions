//! The agenda run against Linear's seam on recorded responses
//! (`crates/linear/tests/fixtures`, written from Linear's schema: not
//! recordings): import with paging, a move by Linear's workflow states, a
//! local Blocked, and nothing of a token in what the agenda sent.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use fenix_linear_client::recorded::Recorded;
use fenix_linear_client::seam::{about, Seam, Settings};
use fenix_linear_client::{Client, Linear};
use fenix_test_kit::{Fake, Harness};
use fenix_tracker::{self as seam, call};
use futures_executor::block_on;

use crate::model::Status;
use crate::page::Action as A;
use crate::trackers::KNOWN;
use crate::Agenda;

/// A folder of the test's own, gone when it is.
struct Folder(PathBuf);

impl Folder {
    fn new() -> Folder {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("fenix-agenda-linear-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&path).unwrap();
        Folder(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn linear_drives_the_agenda_on_recorded_responses() {
    let recorded = Rc::new(Recorded::workspace(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/linear/tests/fixtures"))));
    let seam_ = Seam::new(Linear::new(Client::new(recorded.clone(), Rc::new(|| 1_800_000_000))), Settings::default());
    let fake = Fake::new();
    for id in KNOWN.iter().filter(|k| **k != "linear") {
        let about = seam::service(id, call::ABOUT);
        fake.on_service(&about, Err(&format!("nothing offers {about}")));
        if *id == crate::legacy::ID {
            fake.on_service("jira.config", Err("nothing offers jira.config"));
        }
    }
    let a = serde_json::to_string(&about(&seam_.settings)).unwrap();
    fake.on_service(&seam::service("linear", call::ABOUT), Ok(&a));
    for c in call::ALL.into_iter().filter(|c| *c != call::ABOUT) {
        let s = seam_.clone();
        fake.on_service_with(&seam::service("linear", c), move |payload| block_on(s.serve(c, payload)));
    }
    let folder = Folder::new();
    let agenda = Agenda { kept: Some(folder.0.join("agenda.json")), ..Default::default() };
    let mut h = Harness::wrap(fake, agenda);
    h.activate().unwrap();
    let t = h.ext.sync.trackers.get("linear").expect("found by asking linear.tracker.about");
    assert!(t.ready() && t.can(seam::can::COMMENT) && !t.can(seam::can::WORKLOG));
    assert_eq!(t.name(), "Linear");

    // Import: two pages of yours, ENG-12 from the first and OPS-7 from the second.
    h.fake.on_pick_many("Import", &["ENG-12  Fix the login timeout  [In Progress]", "OPS-7  Rotate the signing keys  [In Review]"]);
    h.command("agenda.import").unwrap();
    let eng = h.ext.store.find_by_key("linear", "ENG-12").unwrap_or_else(|| panic!("{:?}", h.fake.notifications()));
    assert!(h.ext.store.find_by_key("linear", "OPS-7").is_some(), "the second page was asked for");
    assert_eq!(recorded.variables_of("AssignedIssues").len(), 2);
    let task = h.ext.store.task(eng).unwrap();
    assert_eq!((task.title.as_str(), task.status), ("Fix the login timeout", Status::InProgress));
    assert_eq!(task.link.as_ref().unwrap().project(), Some("ENG"));

    // Moving it to Done: Linear's states that finish it, asked; the pick is sent by state id.
    h.fake.on_pick("ENG-12 status", "Done  (completed)");
    h.ext.page_action(A::SetStatus(eng, Status::Done));
    h.settle();
    let picker = h.fake.pickers().into_iter().last().expect("several states finish it: a picker");
    assert_eq!(picker.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Done  (completed)", "Canceled  (canceled)", "Duplicate  (canceled)"]);
    assert!(recorded.operations().contains(&"UpdateIssue".to_string()));
    assert_eq!(recorded.variables_of("UpdateIssue").last().unwrap()["input"]["stateId"], "57000000-0000-4000-8000-000000000006");

    // Blocked is the agenda's own: one local move, nothing sent to Linear.
    let writes = recorded.operations().iter().filter(|o| *o == "UpdateIssue").count();
    h.ext.page_action(A::SetStatus(eng, Status::Blocked));
    h.settle();
    assert_eq!(h.ext.store.task(eng).unwrap().status, Status::Blocked);
    assert_eq!(recorded.operations().iter().filter(|o| *o == "UpdateIssue").count(), writes);

    // No token, header or key in anything the agenda sent.
    for s in h.fake.services() {
        for bad in ["Authorization", "Bearer", "lin_api_"] {
            assert!(!s.payload.contains(bad), "{bad} in {} {}", s.name, s.payload);
        }
    }
}
