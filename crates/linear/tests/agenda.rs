//! The agenda run against Linear's seam on recorded responses
//! (`tests/fixtures`, written from Linear's schema: not recordings), in
//! the agenda's own test world (`fenix_agenda::testing`): found by its id,
//! import across two pages, a card moved by Linear's workflow states,
//! Blocked kept by the agenda alone (Linear has none), and nothing of a
//! token in what the agenda sent.

use std::path::Path;
use std::rc::Rc;

use fenix_agenda::model::Status;
use fenix_agenda::testing::world_with;
use fenix_linear_client::recorded::Recorded;
use fenix_linear_client::seam::{about, Seam, Settings, ID};
use fenix_linear_client::{Client, Linear};
use fenix_tracker::{self as seam, call};
use futures_executor::block_on;

#[test]
fn linear_drives_the_agenda_on_recorded_responses() {
    let recorded = Rc::new(Recorded::workspace(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))));
    let tracker = Seam::new(Linear::new(Client::new(recorded.clone(), Rc::new(|| 1_800_000_000))), Settings { team: Some("ENG".into()), ..Default::default() });
    let mut w = world_with("linear", &[ID], |fake| {
        fake.on_service(&seam::service(ID, call::ABOUT), Ok(&serde_json::to_string(&about(&tracker.settings)).unwrap()));
        for c in call::ALL.into_iter().filter(|c| *c != call::ABOUT) {
            let s = tracker.clone();
            fake.on_service_with(&seam::service(ID, c), move |payload| block_on(s.serve(c, payload)));
        }
    });
    assert!(w.h.fake.services().iter().any(|c| c.name == seam::service(ID, call::ABOUT)), "found by asking fenix.linear.tracker.about");

    // Import: two pages of yours, ENG-12 from the first and OPS-7 from the second.
    w.h.fake.on_pick_many("Import", &["ENG-12  Fix the login timeout  [In Progress]", "OPS-7  Rotate the signing keys  [In Review]"]);
    w.h.command("agenda.import").unwrap();
    let eng = w.find(ID, "ENG-12").unwrap_or_else(|| panic!("{:?}", w.said()));
    assert!(w.find(ID, "OPS-7").is_some(), "the second page was asked for");
    assert_eq!(recorded.variables_of("AssignedIssues").len(), 2);
    assert_eq!((w.task(eng).title.as_str(), w.task(eng).status), ("Fix the login timeout", Status::InProgress));
    // Found again by Linear's own id, which an identifier change leaves alone.
    assert!(w.find(ID, "1550e000-0000-4000-8000-000000000012").is_some());

    // Done: the states that finish it, asked; the pick is sent by state id.
    w.h.fake.on_pick("ENG-12 status", "Done  (completed)");
    w.set_status(eng, Status::Done);
    let picker = w.h.fake.pickers().into_iter().last().expect("several states finish it: a picker");
    assert_eq!(picker.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Done  (completed)", "Canceled  (canceled)", "Duplicate  (canceled)"]);
    let sent = recorded.variables_of("UpdateIssue");
    assert_eq!(sent.last().unwrap()["input"]["stateId"], "57000000-0000-4000-8000-000000000006");
    let op = w.h.fake.services().iter().rev().find(|s| s.name.ends_with(".tracker.transition")).map(|s| s.payload.clone()).unwrap();
    assert!(op.contains("\"op\":\"agenda:"), "every write carries its op: {op}");

    // Blocked is the agenda's own: nothing is sent to Linear.
    let writes = recorded.operations().iter().filter(|o| *o == "UpdateIssue").count();
    w.set_status(eng, Status::Blocked);
    assert_eq!(w.task(eng).status, Status::Blocked);
    assert_eq!(recorded.operations().iter().filter(|o| *o == "UpdateIssue").count(), writes);
    w.no_secrets();
}
