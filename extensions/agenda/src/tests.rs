//! The agenda on the test kit's fake host, with trackers behind the
//! tracker seam (`plans/tracker-seam.md` §8): `FakeTracker`s driving it,
//! two at once, the Jira extension 1.3's seam and 1.2's old services on
//! recorded Jira responses, and the old path beside the new.
//!
//! A tracker of your own is tested the same way: `serve` wires a
//! `FakeTracker` (or your own answers) to `<id>.tracker.*`, and `world`
//! starts the agenda with it in a folder of the test's own.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use fenix_test_kit::Fake;
use fenix_tracker::{self as seam, call, code, About, Ask, FakeTracker, Move, Moves, SeamError, State};

use crate::model::Status;
use crate::page::Action as A;
use crate::testing::*;
use crate::trackers::Tracker;

/// A ready tracker that can do everything, as found.
pub fn tracker(id: &str, name: &str) -> Tracker {
    let about = About {
        seam: 1,
        seams: vec![1],
        minor: 0,
        id: id.into(),
        name: name.into(),
        ready: true,
        not_ready: None,
        can: seam::can::ALL.iter().map(|c| c.to_string()).collect(),
        sync_minutes: 10,
        project_meta: Some(id.into()),
        browse: None,
        ref_hint: None,
        blocked: true,
    };
    Tracker { id: id.into(), about, legacy: None }
}

fn json(payload: &str) -> serde_json::Value {
    serde_json::from_str(payload).unwrap()
}

fn two_fakes() -> FakeTracker {
    let t = FakeTracker::new("Fake");
    t.add(FakeTracker::issue("FAKE-1", "Fix the login"));
    t.add(FakeTracker::issue("FAKE-2", "Write the notes"));
    t
}

// -- One tracker ------------------------------------------------------------

#[test]
fn an_import_pages_through_whats_assigned_and_adds_what_you_mark() {
    let t = FakeTracker::new("Fake");
    for i in 1..=5 {
        t.add(FakeTracker::issue(&format!("FAKE-{i}"), &format!("Issue {i}")));
    }
    t.page_size(2);
    let mut w = world("import", &[("acme.fake", &t)]);
    w.h.fake.on_pick_many("Import", &["FAKE-1  Issue 1  [Open]", "FAKE-5  Issue 5  [Open]"]);
    w.h.command("agenda.import").unwrap();

    let pages: Vec<serde_json::Value> = t.calls_of("assigned").iter().map(|p| json(p)).collect();
    assert_eq!(pages.len(), 3, "{pages:?}");
    assert_eq!((pages[0]["limit"].as_u64(), pages[0].get("cursor")), (Some(200), None));
    assert_eq!((pages[1]["cursor"].as_str(), pages[2]["cursor"].as_str()), (Some("2"), Some("4")));
    assert_eq!(pages[1]["limit"], 198, "what's still wanted");
    let picker = w.h.fake.pickers().into_iter().find(|p| p.title.starts_with("Import")).unwrap();
    assert_eq!(picker.items.len(), 5);
    assert!(w.h.ext.store.find_by_key("acme.fake", "FAKE-1").is_some() && w.h.ext.store.find_by_key("acme.fake", "FAKE-5").is_some());
    assert!(w.h.ext.store.find_by_key("acme.fake", "FAKE-2").is_none());
    assert!(w.said().contains(&"Added 2 issue(s) to the agenda".to_string()), "{:?}", w.said());
    assert!(w.said().contains(&"Fetching your open Fake issues...".to_string()));
    w.no_secrets();
}

#[test]
fn linking_lists_your_issues_and_the_trackers_own_form() {
    let t = two_fakes();
    let mut w = world("link", &[("acme.fake", &t)]);
    let id = w.h.ext.store.create_task("Local".into(), "mine".into(), crate::model::Priority::Low, None);
    w.h.fake.on_pick("Link to Fake", "FAKE-2  Write the notes  [Open]");
    w.act(A::Link(id));
    let picker = w.h.fake.pickers().into_iter().find(|p| p.title == "Link to Fake").unwrap();
    let rows: Vec<&str> = picker.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(rows, ["+ Create a new Fake issue from this task...", "Type an issue key...", "FAKE-1  Fix the login  [Open]", "FAKE-2  Write the notes  [Open]"]);
    let task = w.task(id);
    assert_eq!((task.issue_key(), task.title.as_str()), (Some("FAKE-2"), "Write the notes"));
    assert!(task.notes[0].text.contains("mine"), "the local description kept as a note");
    assert_eq!(w.last_said(), "Linked to FAKE-2 -- edits now go to Fake too");

    // The tracker's form: what it made is linked.
    let other = w.h.ext.store.create_task("Ship it".into(), "the body".into(), crate::model::Priority::Low, None);
    w.h.fake.on_pick("Link to Fake", "+ Create a new Fake issue from this task...");
    w.act(A::Link(other));
    let made = json(&t.calls_of("create")[0]);
    assert_eq!((made["title"].as_str(), made["body"].as_str()), (Some("Ship it"), Some("the body")));
    assert_eq!(w.task(other).issue_key(), Some("FAKE-101"));
}

#[test]
fn one_issue_by_its_key_or_address_and_a_sync_takes_what_changed() {
    let t = two_fakes();
    let mut w = world("one", &[("acme.fake", &t)]);
    let one = w.add("acme.fake", "FAKE-1");
    // The notebook's links, and Jira 1.2, say `key` and no tracker.
    let two = {
        assert_eq!(w.h.serve("agenda.add_issue", r#"{"tracker": "acme.fake", "key": "https://tracker.example/FAKE-2"}"#), Some(Ok(String::new())));
        w.h.ext.store.find_by_key("acme.fake", "FAKE-2").unwrap()
    };
    assert_eq!(json(&t.calls_of("issues")[1])["refs"][0], "https://tracker.example/FAKE-2");
    assert_eq!(w.h.serve("agenda.open_task", r#"{"key": "FAKE-1"}"#), Some(Ok(String::new())));

    t.edit("FAKE-1", |i| {
        i.title = "Fix the login for good".into();
        i.state = seam::fake::state("2", "Doing", "doing");
        i.mine = Some(false);
    });
    t.add(FakeTracker::issue("FAKE-3", "Soon gone"));
    let three = w.add("acme.fake", "FAKE-3");
    // FAKE-2 moves to another project: same id, a new ref. FAKE-3 is deleted.
    t.edit("FAKE-2", |i| i.key = "MOVED-9".into());
    t.edit("FAKE-3", |i| {
        i.id = "gone".into();
        i.key = "GONE-1".into();
    });
    w.h.command("agenda.sync").unwrap();
    assert_eq!(json(t.calls_of("issues").last().unwrap())["refs"], serde_json::json!(["id-FAKE-1", "id-FAKE-2", "id-FAKE-3"]), "asked by id");
    let task = w.task(one);
    assert_eq!((task.title.as_str(), task.status, task.link.as_ref().unwrap().not_mine), ("Fix the login for good", Status::InProgress, true));
    let moved = w.task(two).link.as_ref().unwrap();
    assert_eq!((moved.key.as_str(), moved.error.as_deref()), ("MOVED-9", None), "followed to its new ref");
    assert_eq!(w.find("acme.fake", "MOVED-9"), Some(two));
    assert_eq!(w.task(three).link.as_ref().unwrap().error.as_deref(), Some("not found in Fake -- deleted, moved, or no longer visible to you"));
    assert_eq!(w.toast().0, "Synced 2 issue(s) with Fake");
}

#[test]
fn edits_go_to_the_tracker_in_order_and_one_it_doesnt_take_stays_here() {
    let t = two_fakes();
    let mut w = world("edits", &[("acme.fake", &t)]);
    let id = w.add("acme.fake", "FAKE-1");
    w.act(A::SetTitle(id, "Renamed here".into()));
    w.act(A::SetDue(id, chrono::NaiveDate::from_ymd_opt(2026, 12, 1)));
    let updates: Vec<serde_json::Value> = t.calls_of("update").iter().map(|p| json(p)).collect();
    assert_eq!(updates[0]["set"], serde_json::json!({ "title": "Renamed here" }));
    assert_eq!(updates[1]["set"], serde_json::json!({ "due": "2026-12-01" }));
    assert_eq!((t.get("FAKE-1").unwrap().title, t.get("FAKE-1").unwrap().due), ("Renamed here".to_string(), Some("2026-12-01".to_string())));
    assert!(w.said().contains(&"FAKE-1: title updated".to_string()) && w.said().contains(&"FAKE-1: due 2026-12-01".to_string()));
    assert!(w.h.ext.store.outbox.is_empty());

    let plain = FakeTracker::new("Plain");
    plain.about(|a| a.can.clear()).add(FakeTracker::issue("P-1", "Read only"));
    let mut w = world("acme.plain", &[("acme.plain", &plain)]);
    let id = w.add("acme.plain", "P-1");
    w.act(A::SetTitle(id, "Mine".into()));
    assert!(plain.calls_of("update").is_empty() && w.h.ext.store.outbox.is_empty());
    assert_eq!(w.task(id).title, "Mine");
    w.act(A::Comment(id));
    assert_eq!(w.last_said(), "Plain doesn't take comments from the agenda -- post it on its own page");
}

// -- Moving it on -------------------------------------------------------------

#[test]
fn a_card_moved_takes_the_one_move_that_gets_there() {
    let t = two_fakes();
    let mut w = world("move-one", &[("acme.fake", &t)]);
    let id = w.add("acme.fake", "FAKE-1");
    w.set_status(id, Status::Done);
    let asked = json(&t.calls_of("transitions")[0]);
    assert_eq!((asked["toward"].as_str(), asked["from"]["id"].as_str(), asked["from"]["column"].as_str()), (Some("done"), Some("1"), Some("todo")));
    assert_eq!(json(&t.calls_of("transition")[0])["id"], "finish");
    assert_eq!((w.task(id).status, t.get("FAKE-1").unwrap().state.stage.as_str()), (Status::Done, "done"));
    assert_eq!(w.last_said(), "FAKE-1: moved to Done");
}

#[test]
fn a_local_move_sends_nothing_and_a_column_already_there_isnt_asked() {
    let t = two_fakes();
    t.edit("FAKE-1", |i| i.state = seam::fake::state("2", "Doing", "doing"));
    let local = Move { id: "local".into(), name: "Keep it here".into(), local: true, to: State { column: "blocked".into(), ..seam::fake::state("2", "Doing", "doing") }, ..Default::default() };
    t.moves("FAKE-1", Moves { transitions: vec![local], ..Default::default() });
    let mut w = world("move-local", &[("acme.fake", &t)]);
    let id = w.add("acme.fake", "FAKE-1");
    w.set_status(id, Status::Blocked);
    assert_eq!(w.task(id).status, Status::Blocked);
    assert!(t.calls_of("transition").is_empty() && w.h.ext.store.outbox.is_empty());
    let asked = t.calls_of("transitions").len();
    w.set_status(id, Status::InProgress);
    assert_eq!(t.calls_of("transitions").len(), asked, "the tracker has it in Doing already");
    assert_eq!(w.task(id).status, Status::InProgress);
}

#[test]
fn several_moves_ask_and_esc_puts_the_card_back() {
    let t = two_fakes();
    let done = |id: &str, name: &str| Move { id: id.into(), name: name.into(), to: seam::fake::state("3", "Done", "done"), ..Default::default() };
    t.moves("FAKE-1", Moves { transitions: vec![done("resolve", "Resolve"), done("close", "Close")], ..Default::default() });
    let mut w = world("move-many", &[("acme.fake", &t)]);
    let id = w.add("acme.fake", "FAKE-1");
    w.h.fake.on_pick_nothing("FAKE-1 status");
    w.set_status(id, Status::Done);
    let picker = w.h.fake.pickers().into_iter().last().unwrap();
    assert_eq!(picker.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Resolve  ->  Done", "Close  ->  Done"], "no Blocked row when finishing a move");
    assert_eq!((w.task(id).status, w.last_said()), (Status::Todo, "Move cancelled".to_string()));
    assert!(t.calls_of("transition").is_empty());

    w.h.fake.on_pick("FAKE-1 status", "Close  ->  Done");
    w.set_status(id, Status::Done);
    assert_eq!(json(&t.calls_of("transition")[0])["id"], "close");
}

#[test]
fn a_tracker_that_asks_gets_a_picker_and_its_choice_is_remembered() {
    let t = two_fakes();
    let park = Move {
        id: "park".into(),
        name: "Park".into(),
        to: State { column: "blocked".into(), ..seam::fake::state("4", "Parked", "doing") },
        remember: Some("park-it".into()),
        note: Some("Blocked in FAKE now parks it".into()),
        said: Some("parked".into()),
        ..Default::default()
    };
    t.moves("FAKE-1", Moves { transitions: vec![park], ask: Some(Ask { title: "What does Blocked mean in FAKE?".into(), note: Some("Asked once".into()) }), ..Default::default() });
    let mut w = world("move-ask", &[("acme.fake", &t)]);
    let id = w.add("acme.fake", "FAKE-1");
    w.h.fake.on_pick("What does Blocked mean", "Park  ->  Parked");
    w.set_status(id, Status::Blocked);
    assert_eq!(t.remembered(), ["park-it"]);
    assert_eq!(json(&t.calls_of("transition")[0])["id"], "park");
    assert_eq!(w.task(id).status, Status::Blocked);
    for said in ["Asked once", "Blocked in FAKE now parks it", "FAKE-1: parked"] {
        assert!(w.said().contains(&said.to_string()), "{said}: {:?}", w.said());
    }
}

#[test]
fn nothing_reaching_blocked_says_why_and_offers_a_step_from_here() {
    let t = two_fakes();
    let start = Move { id: "start".into(), name: "Start".into(), to: seam::fake::state("2", "Doing", "doing"), ..Default::default() };
    t.moves("FAKE-1", Moves { transitions: vec![start], blocked: Some("Blocked  (Parked)".into()), why: Some("\"Parked\" isn't reachable from \"Open\" -- pick a step from here".into()), ..Default::default() });
    let mut w = world("move-why", &[("acme.fake", &t)]);
    let id = w.add("acme.fake", "FAKE-1");
    w.set_status(id, Status::Blocked);
    assert!(w.said().contains(&"FAKE-1: \"Parked\" isn't reachable from \"Open\" -- pick a step from here".to_string()), "{:?}", w.said());
    assert_eq!(w.task(id).status, Status::Todo, "the card back");
    let picker = w.h.fake.pickers().into_iter().last().unwrap();
    assert_eq!((picker.title.as_str(), picker.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()), ("FAKE-1 status", vec!["Start  ->  Doing", "Blocked  (Parked)"]));

    // `s` is the same list, and changes nothing until you pick.
    let before = t.calls_of("transition").len();
    w.h.fake.on_pick("FAKE-1 status", "Start  ->  Doing");
    w.act(A::TrackerStatus(id));
    assert_eq!(t.calls_of("transition").len(), before + 1);
    assert_eq!(w.task(id).status, Status::InProgress);
}

#[test]
fn priorities_and_people_are_the_trackers() {
    let t = two_fakes();
    let mut w = world("pick", &[("acme.fake", &t)]);
    let id = w.add("acme.fake", "FAKE-1");
    w.h.fake.on_pick("FAKE-1 priority", "P1").on_pick("Assign FAKE-1", "Jo (jo)");
    w.act(A::TrackerPriority(id));
    w.act(A::Assign(id));
    assert_eq!(w.task(id).priority, crate::model::Priority::Urgent, "P1's level");
    let sets: Vec<serde_json::Value> = t.calls_of("update").iter().map(|p| json(p)["set"].clone()).collect();
    assert_eq!(sets, [serde_json::json!({ "priority": "P1" }), serde_json::json!({ "assignee": { "id": "jo", "name": "Jo" } })]);
}

#[test]
fn worklogs_go_to_the_tracker_that_takes_them_on_their_day() {
    let t = two_fakes();
    let mut w = world("worklogs", &[("acme.fake", &t)]);
    let id = w.add("acme.fake", "FAKE-1");
    w.h.ext.store.log_manual_time(id, chrono::Duration::minutes(50));
    w.act(A::SendWorklogs);
    let sent = t.worklogs();
    assert_eq!((sent[0].key.as_str(), sent[0].seconds), ("FAKE-1", 45 * 60), "rounded to 15 minutes");
    assert!(chrono::DateTime::parse_from_rfc3339(&sent[0].started).is_ok(), "{}", sent[0].started);
    assert!(w.h.ext.store.worklog_batch(15, |_| true).is_empty());

    let plain = FakeTracker::new("Plain");
    plain.about(|a| a.can.retain(|c| c != "worklog")).add(FakeTracker::issue("P-1", "x"));
    let mut w = world("no-worklogs", &[("acme.plain", &plain)]);
    let id = w.add("acme.plain", "P-1");
    w.h.ext.store.log_manual_time(id, chrono::Duration::minutes(50));
    w.act(A::SendWorklogs);
    assert_eq!(w.last_said(), "Nothing to send");
}

// -- Errors -------------------------------------------------------------------

#[test]
fn no_tracker_or_one_not_set_up_says_so() {
    let mut w = world("none", &[]);
    w.h.command("agenda.sync").unwrap();
    assert_eq!(w.last_said(), "Nothing syncs the agenda -- that needs a tracker extension, such as Jira (SPC h x)");

    let t = two_fakes();
    t.about(|a| {
        a.ready = false;
        a.not_ready = Some("Fake isn't signed in -- set its token on the settings page".into());
    });
    let mut w = world("not-set-up", &[("acme.fake", &t)]);
    w.h.command("agenda.import").unwrap();
    assert_eq!(w.last_said(), "Fake isn't signed in -- set its token on the settings page");
    assert!(t.calls_of("assigned").is_empty());
}

#[test]
fn offline_and_rate_limited_wait_for_the_timed_sync_and_refused_waits_for_you() {
    for (error, retried) in [(SeamError::new(code::OFFLINE, "request failed: no route"), true), (SeamError { retry_after: Some(30), ..SeamError::new(code::RATE_LIMITED, "HTTP 429") }, true), (SeamError::new(code::REFUSED, "HTTP 403"), false)] {
        let t = two_fakes();
        let mut w = world(&format!("op-{}", error.code), &[("acme.fake", &t)]);
        let id = w.add("acme.fake", "FAKE-1");
        t.fail("update", error.clone());
        w.act(A::SetTitle(id, "Mine".into()));
        assert_eq!(w.last_said(), format!("FAKE-1: Fake refused a change (title updated): {} -- r on the task retries", error.message));
        let link = w.task(id).link.clone().unwrap();
        assert_eq!((link.error.as_deref(), link.transient), (Some(error.message.as_str()), retried));
        t.heal("update");
        w.later(Duration::from_secs(3600));
        assert_eq!(t.get("FAKE-1").unwrap().title == "Mine", retried, "{}", error.code);
        if !retried {
            w.act(A::Retry(id));
            assert_eq!(t.get("FAKE-1").unwrap().title, "Mine", "r sends it");
        }
        assert!(w.task(id).link.as_ref().unwrap().error.is_none());
    }
}

#[test]
fn a_sync_that_fails_says_why_once_and_one_issue_not_found_is_that_tasks() {
    let t = two_fakes();
    let mut w = world("sync-fails", &[("acme.fake", &t)]);
    let (one, _) = (w.add("acme.fake", "FAKE-1"), w.add("acme.fake", "FAKE-2"));
    t.fail("issues", SeamError::new(code::FAILED, "HTTP 500"));
    w.h.command("agenda.sync").unwrap();
    assert_eq!(w.toast(), ("Fake sync failed".to_string(), "HTTP 500".to_string()));

    let t = two_fakes();
    let mut w = world("sync-not-found", &[("acme.fake", &t)]);
    let one_only = w.add("acme.fake", "FAKE-1");
    let toasts = w.h.fake.toasts().len();
    t.fail("issues", SeamError::new(code::NOT_FOUND, "HTTP 404 (Issue Does Not Exist)"));
    w.h.command("agenda.sync").unwrap();
    assert_eq!(w.task(one_only).link.as_ref().unwrap().error.as_deref(), Some("not found in Fake -- deleted, moved, or no longer visible to you"));
    assert_eq!(w.h.fake.toasts().len(), toasts + 1);
    assert_eq!(w.toast().0, "Synced 0 issue(s) with Fake");
    let _ = one;
}

#[test]
fn an_older_tracker_without_a_call_and_one_that_never_answers() {
    let t = two_fakes();
    t.unserve("people");
    let mut w = world("older", &[("acme.fake", &t)]);
    let id = w.add("acme.fake", "FAKE-1");
    w.act(A::Assign(id));
    assert_eq!(w.last_said(), "couldn't list who to assign to: no service acme.fake.tracker.people");

    let t = two_fakes();
    let mut w = world_with("silent", &["acme.fake"], |fake| serve_but(fake, "acme.fake", &t, &["issues"]));
    t.add(FakeTracker::issue("FAKE-3", "x"));
    w.h.fake.on_pick_many("Import", &["FAKE-1  Fix the login  [Open]"]);
    w.h.command("agenda.import").unwrap();
    w.h.command("agenda.sync").unwrap();
    assert!(w.h.ext.sync.pulling());
    w.later(Duration::from_secs(91));
    assert!(!w.h.ext.sync.pulling());
    assert_eq!(w.toast(), ("Fake sync failed".to_string(), "no answer from acme.fake.tracker.issues in 90 s".to_string()));
    assert_eq!(crate::calls::waiting(), 0);
}

#[test]
fn a_tracker_speaking_another_seam_is_passed_over_and_said_once() {
    let fake_two = FakeTracker::new("Future");
    fake_two.about(|a| a.seam = 2);
    fake_two.about(|a| a.seams = vec![2]);
    let mut w = world("seam-2", &[("acme.future", &fake_two)]);
    let t = w.h.ext.sync.trackers.get("acme.future").expect("installed, never 'not installed'");
    assert!(!t.ready());
    let said = "Future is installed, but speaks the tracker seam 2 and the agenda 1 -- update the agenda to link its issues";
    assert_eq!(w.last_said(), said);
    w.h.command("agenda.import").unwrap();
    assert_eq!(w.last_said(), said, "and says so when asked to sync");
}

// -- Untrusted text -------------------------------------------------------------

#[test]
fn what_a_tracker_sends_is_cleaned_and_capped_and_a_cut_body_isnt_written_back() {
    let t = FakeTracker::new("Fake");
    let mut issue = FakeTracker::issue("FAKE-1", "Fix\u{1b}[2J the\u{202e} thing\nnow");
    issue.body = "x".repeat(70 * 1024);
    issue.url = Some("javascript:alert(document.cookie)".into());
    issue.comments = vec![seam::Comment { id: "1".into(), author: "Mallory\u{7}\u{9b}".into(), body: "hi\u{0}".into(), created: "2026-11-14T09:00:00+00:00".into() }];
    issue.state.name = "Open\r\nfor good".into();
    t.add(issue);
    t.add(seam::Issue { key: "BAD REF".into(), ..FakeTracker::issue("X-1", "dropped") });
    let mut w = world("untrusted", &[("acme.fake", &t)]);
    let id = w.add("acme.fake", "FAKE-1");
    let task = w.task(id);
    assert_eq!(task.title, "Fix[2J the thing now");
    assert!(task.description.len() <= seam::clean::BODY_BYTES);
    let base = &task.link.as_ref().unwrap().base;
    assert!(base.description_cut);
    assert_eq!((base.url.as_deref(), base.status_name.as_str()), (None, "Open for good"));
    assert_eq!((base.comments[0].author.as_str(), base.comments[0].body.as_str()), ("Mallory", "hi"));

    w.h.fake.on_prompt("Description", "shorter");
    w.act(A::EditDescription(id));
    assert!(t.calls_of("update").is_empty(), "a cut body would cut it in the tracker too");
    assert_eq!(w.last_said(), "FAKE-1's description is too long to edit here -- yours stays on the task; edit it in Fake");

    w.h.fake.on_pick_many("Import", &[]);
    w.h.command("agenda.import").unwrap();
    let picker = w.h.fake.pickers().into_iter().find(|p| p.title.starts_with("Import"));
    assert!(picker.is_none_or(|p| p.items.iter().all(|i| !i.label.contains("dropped"))), "an issue whose ref isn't one is dropped");
    w.no_secrets();
}

// -- Several trackers -------------------------------------------------------------

#[test]
fn two_trackers_each_keep_their_own_and_are_told_apart() {
    let fake_one = FakeTracker::new("Fake");
    fake_one.add(FakeTracker::issue("ENG-1", "Fake's one"));
    let other = FakeTracker::new("Other");
    other.add(FakeTracker::issue("ENG-1", "Other's one"));
    let mut w = world("two", &[("acme.fake", &fake_one), ("acme.other", &other)]);
    w.h.fake.on_pick_many("Import", &["ENG-1  Fake's one  [Open]  · Fake", "ENG-1  Other's one  [Open]  · Other"]);
    w.h.command("agenda.import").unwrap();
    let (a, b) = (w.h.ext.store.find_by_key("acme.fake", "ENG-1").unwrap(), w.h.ext.store.find_by_key("acme.other", "ENG-1").unwrap());
    assert_ne!(a, b);
    assert_eq!((w.task(a).title.as_str(), w.task(b).title.as_str()), ("Fake's one", "Other's one"));

    other.edit("ENG-1", |i| i.title = "Other's, renamed".into());
    w.h.command("agenda.sync").unwrap();
    assert_eq!((w.task(a).title.as_str(), w.task(b).title.as_str()), ("Fake's one", "Other's, renamed"));
    assert_eq!(w.toast().0, "Synced 2 issue(s) with Fake and Other");
    assert_eq!(json(fake_one.calls_of("issues").last().unwrap())["refs"], serde_json::json!(["id-ENG-1"]));

    assert_eq!(w.h.serve("agenda.linked", r#"{"tracker": "acme.other"}"#), Some(Ok(r#"["ENG-1"]"#.into())));
    assert_eq!(w.h.serve("agenda.linked", "null"), Some(Ok(r#"["ENG-1","ENG-1"]"#.into())));
    w.h.serve("agenda.open_task", r#"{"tracker": "acme.other", "ref": "ENG-1"}"#);
    let page = w.h.page("agenda");
    assert!(w.h.screen(page).contains("Other's, renamed"));

    // One failing leaves the other's done, and says which.
    other.fail("issues", SeamError::new(code::OFFLINE, "Other is unreachable"));
    w.h.command("agenda.sync").unwrap();
    assert_eq!(w.toast(), ("Other sync failed".to_string(), "Other is unreachable Synced 1 issue(s) with Fake.".to_string()), "every failure, and what did sync");
    w.no_secrets();
}

// -- Jira, before the seam and after ------------------------------------------------

mod jira {
    use super::*;
    use fenix_jira_client::recorded::Recorded;
    use fenix_jira_client::tracker::{Keys, Learned, Seam, Settings};
    use fenix_jira_client::{IssueDetail, JiraClient, Transport};
    use futures_executor::block_on;

    pub fn fixtures() -> &'static std::path::Path {
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/jira/tests/fixtures"))
    }

    pub fn config() -> serde_json::Value {
        serde_json::json!({
            "base_url": "https://jira.example.com",
            "projects": ["PROJ"],
            "users": [["jo1", "Jo Bloggs"]],
            "priorities": [["Minor", "Medium"]],
            "blocked": [["PROJ", "10103: On Hold"], ["OPS", "flag"]],
            "sync_minutes": 10,
        })
    }

    /// The agenda 1.2's own reading of an issue, as it was
    /// (`extensions/agenda/src/sync.rs` at 1.2.0): the old path.
    mod old {
        use crate::model::{Priority, RemoteComment, RemoteSnapshot, RemoteUpdate, Status};
        use fenix_jira_client::IssueDetail;

        pub struct Config {
            pub priorities: Vec<(String, String)>,
            pub blocked: Vec<(String, String)>,
        }

        fn guess_priority(name: &str) -> Priority {
            let lower = name.to_lowercase();
            if ["highest", "blocker", "critical", "urgent"].iter().any(|w| lower.contains(w)) {
                Priority::Urgent
            } else if lower.contains("high") || lower.contains("major") {
                Priority::High
            } else if ["low", "minor", "trivial"].iter().any(|w| lower.contains(w)) {
                Priority::Low
            } else {
                Priority::Medium
            }
        }

        fn status_for_category(category: &str) -> Status {
            match category {
                "done" => Status::Done,
                "indeterminate" => Status::InProgress,
                _ => Status::Todo,
            }
        }

        impl Config {
            fn priority(&self, name: &str) -> Priority {
                self.priorities
                    .iter()
                    .find(|(jira, _)| jira.eq_ignore_ascii_case(name))
                    .and_then(|(_, level)| Priority::ALL.into_iter().find(|p| p.label().eq_ignore_ascii_case(level)))
                    .unwrap_or_else(|| guess_priority(name))
            }

            fn status_for(&self, project: &str, status_id: &str, category: &str, flagged: bool) -> Status {
                let blocked = self.blocked.iter().find(|(p, _)| p == project).map(|(_, m)| m.as_str());
                match blocked {
                    Some("flag") if flagged => Status::Blocked,
                    Some(meaning) if meaning.split_once(':').is_some_and(|(id, _)| id.trim() == status_id) => Status::Blocked,
                    _ => status_for_category(category),
                }
            }
        }

        pub fn remote_update(config: &Config, me: Option<&str>, d: &IssueDetail) -> RemoteUpdate {
            let project = fenix_jira_client::project_of(&d.key);
            RemoteUpdate {
                snapshot: RemoteSnapshot {
                    summary: d.summary.clone(),
                    description: d.description.clone().unwrap_or_default(),
                    status_id: d.status_id.clone(),
                    status_name: d.status.clone(),
                    status_category: d.status_category.clone(),
                    priority: d.priority.clone(),
                    assignee: d.assignee.clone(),
                    assignee_id: d.assignee_id.clone(),
                    flagged: d.flagged,
                    updated: d.updated.clone(),
                    due: d.due.clone(),
                    comments: d.comments.iter().map(|c| RemoteComment { id: c.id.clone(), author: c.author.clone(), body: c.body.clone(), created: c.created.clone() }).collect(),
                    ..Default::default()
                },
                status: config.status_for(project, &d.status_id, &d.status_category, d.flagged),
                priority: d.priority.as_deref().map(|p| config.priority(p)).unwrap_or(Priority::Medium),
                mine: me.map(|me| d.assignee_id.as_deref() == Some(me)),
            }
        }
    }

    fn settings() -> Settings {
        Settings::from_config(&config().to_string(), Keys::default()).unwrap()
    }

    #[test]
    fn the_seam_reads_jira_as_the_agenda_did_on_the_same_responses() {
        let keys: Vec<String> = ["PROJ-1", "PROJ-2", "OPS-7"].map(String::from).to_vec();
        // The old path: the agenda's own requests through `jira.request`.
        let jira = Rc::new(Recorded::jira_server(fixtures()));
        let client = JiraClient::new(jira.clone());
        let me = block_on(client.myself()).ok();
        let flag = block_on(client.find_flagged_field()).unwrap();
        let details: Vec<IssueDetail> = block_on(client.search_details(&fenix_jira_client::build_keys_jql(&keys), 3, flag.as_deref())).unwrap();
        let config = old::Config { priorities: vec![("Minor".into(), "Medium".into())], blocked: vec![("PROJ".into(), "10103: On Hold".into()), ("OPS".into(), "flag".into())] };
        let before: Vec<_> = details.iter().map(|d| old::remote_update(&config, me.as_deref(), d)).collect();
        let old_requests = jira.asked();

        // The new: the Jira extension 1.3's seam, read by the agenda.
        let jira = Rc::new(Recorded::jira_server(fixtures()));
        let seam = Seam::new(JiraClient::new(jira.clone()), settings(), Rc::new(RefCell::new(Learned::default())));
        let found: seam::Found = serde_json::from_str(&block_on(seam.serve("issues", &serde_json::to_string(&seam::Issues::new(keys.clone())).unwrap())).unwrap()).unwrap();
        let after: Vec<_> = found.cleaned().issues.iter().map(crate::sync::remote_update).collect();
        // The same requests, the search told not to fail on a deleted key.
        let mut want = old_requests.clone();
        want[2].push_str("&validateQuery=false");
        assert_eq!(jira.asked(), want);

        assert_eq!(before.len(), 3);
        for (old, new) in before.iter().zip(&after) {
            let (o, n) = (&old.snapshot, &new.snapshot);
            assert_eq!((old.status, old.priority, old.mine), (new.status, new.priority, new.mine), "{}", o.summary);
            assert_eq!((&o.summary, &o.status_id, &o.status_name, &o.priority), (&n.summary, &n.status_id, &n.status_name, &n.priority));
            assert_eq!((&o.assignee, &o.assignee_id, &o.updated, &o.due, &o.comments), (&n.assignee, &n.assignee_id, &n.updated, &n.due, &n.comments));
            // What differs is what the seam says another way: line ends
            // cleaned, the stage in its own words, the Flagged field as
            // the column it puts the issue in.
            assert_eq!(o.description.replace("\r\n", "\n"), n.description);
            assert_eq!(fenix_jira_client::tracker::stage(&o.status_category), n.status_category);
            assert_eq!(o.flagged, n.column.as_deref() == Some("blocked") && n.status_id != "10103");
        }
        assert_eq!(after[1].status, Status::Blocked, "On Hold is what Blocked means in PROJ");
        assert_eq!(after[2].status, Status::Blocked, "OPS-7 is flagged, and Blocked is the flag in OPS");
    }

    /// `jira.request`, answered from recorded responses, as Jira 1.2's
    /// is from your server.
    fn jira_1_2(fake: &Fake, jira: Rc<Recorded>, set_blocked: Rc<RefCell<Vec<String>>>) {
        absent(fake, "fenix.jira");
        // Jira 1.2 has no seam -- but has its old services.
        fake.on_service("jira.config", Ok(&config().to_string()));
        fake.on_service_with("jira.request", move |payload| {
            let req = json(payload);
            let body = req["body"].as_str().map(str::to_string);
            let (status, text) = block_on(jira.send(req["method"].as_str().unwrap(), req["path"].as_str().unwrap(), body))?;
            Ok(serde_json::json!({ "status": status, "body": text }).to_string())
        });
        fake.on_service_with("jira.set_blocked", move |payload| {
            set_blocked.borrow_mut().push(payload.to_string());
            Ok(String::new())
        });
    }

    #[test]
    fn a_new_agenda_with_jira_1_2_serves_the_seam_to_itself() {
        let jira = Rc::new(Recorded::jira_server(fixtures()));
        let blocked = Rc::new(RefCell::new(Vec::new()));
        let mut w = world_with("jira-1-2", &[], |fake| jira_1_2(fake, jira.clone(), blocked.clone()));
        let t = w.h.ext.sync.trackers.get("fenix.jira").expect("found through jira.config");
        assert!(t.legacy.is_some() && t.ready());
        assert_eq!(t.name(), "Jira");

        // Jira 1.2's page sends `key`, and no tracker.
        assert_eq!(w.h.serve("agenda.add_issue", r#"{"key": "PROJ-1", "clock": true}"#), Some(Ok(String::new())));
        let id = w.h.ext.store.find_by_key("fenix.jira", "PROJ-1").unwrap();
        assert_eq!((w.task(id).title.as_str(), w.task(id).status), ("Fix the login timeout", Status::InProgress));
        assert!(jira.asked().iter().any(|a| a.starts_with("GET /rest/api/2/issue/PROJ-1?")));

        // Blocked in PROJ is On Hold: one transition, by Jira's own id.
        w.set_status(id, Status::Blocked);
        assert_eq!(jira.sent.borrow().last().unwrap(), &("POST /rest/api/2/issue/PROJ-1/transitions".to_string(), Some(r#"{"transition":{"id":"41"}}"#.to_string())));
        assert_eq!(w.last_said(), "PROJ-1: moved to On Hold");
        w.no_secrets();
        assert!(blocked.borrow().is_empty());
    }

    #[test]
    fn with_jira_1_2_what_blocked_means_is_asked_and_kept_by_jira_set_blocked() {
        let jira = Rc::new(Recorded::jira_server(fixtures()));
        let blocked = Rc::new(RefCell::new(Vec::new()));
        let mut w = world_with("jira-1-2-ask", &[], |fake| {
            jira_1_2(fake, jira.clone(), blocked.clone());
            let mut undecided = config();
            undecided["blocked"] = serde_json::json!([]);
            fake.on_service("jira.config", Ok(&undecided.to_string()));
        });
        let id = w.add("fenix.jira", "PROJ-1");
        w.h.fake.on_pick("What does Blocked mean in PROJ?", "Flag as impediment (keep the status)");
        w.set_status(id, Status::Blocked);
        assert_eq!(blocked.borrow().as_slice(), [r#"{"blocked":"flag","project":"PROJ"}"#]);
        assert!(w.said().contains(&"Pick what Blocked means in PROJ -- asked once, then remembered (b on the project in SPC j j changes it)".to_string()), "{:?}", w.said());
        assert!(w.said().contains(&"Blocked in PROJ now flags the issue as an impediment".to_string()));
        let sent = jira.sent.borrow().clone();
        assert_eq!(sent.last().unwrap(), &("PUT /rest/api/2/issue/PROJ-1".to_string(), Some(r#"{"fields":{"customfield_10000":[{"value":"Impediment"}]}}"#.to_string())));
        assert_eq!((w.task(id).status, w.last_said()), (Status::Blocked, "PROJ-1: flagged".to_string()));

        // Kept: the next block asks nothing.
        w.set_status(id, Status::InProgress);
        let pickers = w.h.fake.pickers().len();
        w.set_status(id, Status::Blocked);
        assert_eq!(w.h.fake.pickers().len(), pickers);
    }

    #[test]
    fn jira_1_3s_seam_drives_the_agenda_on_recorded_responses() {
        let jira = Rc::new(Recorded::jira_server(fixtures()));
        let seam = Seam::new(JiraClient::new(jira.clone()), settings(), Rc::new(RefCell::new(Learned::default())));
        let mut w = world_with("jira-1-3", &["fenix.jira"], |fake| {
            let about = serde_json::to_string(&fenix_jira_client::tracker::about(&seam.settings, true)).unwrap();
            fake.on_service(&seam::service("fenix.jira", call::ABOUT), Ok(&about));
            for c in call::ALL.into_iter().filter(|c| *c != call::ABOUT) {
                let s = seam.clone();
                fake.on_service_with(&seam::service("fenix.jira", c), move |payload| block_on(s.serve(c, payload)));
            }
        });
        let t = w.h.ext.sync.trackers.get("fenix.jira").unwrap();
        assert!(t.legacy.is_none() && t.can(seam::can::WORKLOG));
        w.h.fake.on_pick_many("Import", &["PROJ-1  Fix the login timeout  [In Progress]"]);
        w.h.command("agenda.import").unwrap();
        let id = w.h.ext.store.find_by_key("fenix.jira", "PROJ-1").unwrap();
        assert_eq!(w.task(id).link.as_ref().unwrap().project(), Some("PROJ"));
        assert!(jira.asked().iter().any(|a| a.contains("search?jql=assignee%20%3D%20currentUser%28%29") && a.contains("maxResults=200")), "{:?}", jira.asked());
        w.act(A::Comment(id));
        w.set_status(id, Status::Done);
        let picker = w.h.fake.pickers().into_iter().last().unwrap();
        assert_eq!(picker.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Resolve  ->  Resolved", "Close  ->  Closed"]);
        w.no_secrets();
    }
}

// -- After review: each finding's own test ------------------------------------

/// C3: an unanswered write is never sent again by itself; `r` asks, and
/// a resend carries the same `op`.
#[test]
fn an_unanswered_change_is_never_resent_by_itself() {
    let t = two_fakes();
    let mut w = world_with("unknown", &["acme.fake"], |fake| serve_but(fake, "acme.fake", &t, &["update"]));
    let id = w.add("acme.fake", "FAKE-1");
    w.act(A::SetTitle(id, "Mine".into()));
    w.later(Duration::from_secs(181));
    let link = w.task(id).link.clone().unwrap();
    assert!(link.unknown && !link.transient, "{link:?}");
    assert!(w.last_said().contains("no answer from Fake about a change (title updated) -- it may have been made"), "{}", w.last_said());
    let sent = |w: &World| w.h.fake.services().into_iter().filter(|c| c.name == "acme.fake.tracker.update").map(|c| c.payload).collect::<Vec<_>>();
    assert_eq!(sent(&w).len(), 1);
    w.later(Duration::from_secs(3600));
    assert_eq!(sent(&w).len(), 1, "a timed sync doesn't send it again");
    // r asks; sending it again carries the same op.
    w.h.fake.on_pick("was it made?", "Send it again (title updated)");
    w.act(A::Retry(id));
    let sent = sent(&w);
    assert_eq!(sent.len(), 2);
    assert_eq!(json(&sent[0])["op"], json(&sent[1])["op"], "the same op, for the tracker to do once");
    assert!(json(&sent[0])["op"].as_str().unwrap().starts_with("agenda:"));
}

#[test]
fn a_change_that_may_have_been_made_can_be_forgotten() {
    let t = two_fakes();
    let mut w = world_with("unknown-forget", &["acme.fake"], |fake| serve_but(fake, "acme.fake", &t, &["comment"]));
    let id = w.add("acme.fake", "FAKE-1");
    w.h.fake.on_prompt("Fake comment", "Done.");
    w.act(A::Comment(id));
    w.later(Duration::from_secs(181));
    assert!(w.task(id).link.as_ref().unwrap().unknown);
    w.h.fake.on_pick("was it made?", "It was made -- forget it (comment posted)");
    w.act(A::Retry(id));
    assert_eq!(w.queued(), 0);
    assert!(w.task(id).link.as_ref().unwrap().error.is_none());
}

/// C4: a field the tracker doesn't take is dropped, said once, not
/// offered again -- and the task's later changes still go.
#[test]
fn an_unsupported_field_is_dropped_and_doesnt_hold_up_later_changes() {
    let t = two_fakes();
    let mut w = world("unsupported", &[("acme.fake", &t)]);
    let id = w.add("acme.fake", "FAKE-1");
    let inner = t.clone();
    w.h.fake.on_service_with("acme.fake.tracker.update", move |payload| {
        if json(payload)["set"].get("due").is_some() {
            Err(SeamError::new(code::UNSUPPORTED, "no due dates here").to_err())
        } else {
            inner.serve("update", payload)
        }
    });
    w.act(A::SetDue(id, chrono::NaiveDate::from_ymd_opt(2026, 12, 1)));
    assert_eq!(w.last_said(), "FAKE-1: Fake doesn't take that change (due 2026-12-01): no due dates here -- it stays on the task");
    assert_eq!(w.task(id).due, chrono::NaiveDate::from_ymd_opt(2026, 12, 1), "kept here");
    w.act(A::SetTitle(id, "Mine".into()));
    assert_eq!(t.get("FAKE-1").unwrap().title, "Mine", "not stalled");
    assert!(!w.h.ext.sync.trackers.can("acme.fake", "edit.due"), "not offered again");
    let before = w.h.fake.services().len();
    w.act(A::SetDue(id, None));
    assert_eq!(w.h.fake.services().len(), before, "kept local now");
}

/// Must fix 3: a tracker set up after it was last asked is asked again,
/// and the command runs.
#[test]
fn a_tracker_set_up_just_now_is_ready_for_the_command() {
    let t = two_fakes();
    t.about(|a| {
        a.ready = false;
        a.not_ready = Some("Fake isn't signed in".into());
    });
    let mut w = world("ready-now", &[("acme.fake", &t)]);
    t.about(|a| a.ready = true);
    w.h.fake.on_pick_many("Import", &[]);
    w.h.command("agenda.import").unwrap();
    assert_eq!(t.calls_of("assigned").len(), 1, "asked again, then imported: {:?}", w.said());
    // A tracker that says it changed is asked at once too.
    t.about(|a| a.name = "Renamed".into());
    w.h.serve("agenda.tracker_changed", r#"{"tracker": "acme.fake"}"#);
    assert_eq!(w.h.ext.sync.trackers.name("acme.fake"), "Renamed");
}

/// C1: an answer as another extension isn't used.
#[test]
fn a_tracker_answering_as_another_extension_isnt_used() {
    let t = two_fakes();
    let w = world_with("impostor", &["acme.fake"], |fake| {
        serve(fake, "acme.fake", &t);
        t.about(|a| a.id = "acme.evil".into());
    });
    let found = w.h.ext.sync.trackers.get("acme.fake").unwrap();
    assert!(!found.ready());
    assert!(w.last_said().contains("says it's acme.evil -- not used"), "{}", w.last_said());
}

/// C8: `about` asked of a tracker speaking only seam 2 is never "not
/// installed".
#[test]
fn a_tracker_with_no_common_seam_is_installed_but_unusable() {
    let t = two_fakes();
    t.about(|a| {
        a.seam = 2;
        a.seams = vec![2, 3];
    });
    let w = world("seams", &[("acme.fake", &t)]);
    assert!(w.h.ext.sync.trackers.get("acme.fake").is_some_and(|f| !f.ready()));
}

/// C9: each tracker on its own schedule, none before its retry_after.
#[test]
fn each_tracker_syncs_on_its_own_schedule_and_backs_off() {
    let (fast, slow) = (two_fakes(), FakeTracker::new("Slow"));
    fast.about(|a| a.sync_minutes = 5);
    slow.about(|a| a.sync_minutes = 60);
    slow.add(FakeTracker::issue("SLOW-1", "x"));
    let mut w = world("schedule", &[("acme.fast", &fast), ("acme.slow", &slow)]);
    w.add("acme.fast", "FAKE-1");
    w.add("acme.slow", "SLOW-1");
    w.later(Duration::from_secs(1));
    let (f, s) = (fast.calls_of("issues").len(), slow.calls_of("issues").len());
    w.later(Duration::from_secs(6 * 60));
    assert_eq!((fast.calls_of("issues").len(), slow.calls_of("issues").len()), (f + 1, s), "only the fast one");
    fast.fail("issues", SeamError { retry_after: Some(3600), ..SeamError::new(code::RATE_LIMITED, "slow down") });
    w.later(Duration::from_secs(12 * 60));
    let f = fast.calls_of("issues").len();
    w.later(Duration::from_secs(18 * 60));
    assert_eq!(fast.calls_of("issues").len(), f, "left alone until retry_after");
}

/// C9: a whole-call not_found for several is asked one at a time.
#[test]
fn a_whole_call_not_found_is_asked_one_by_one() {
    let t = two_fakes();
    let mut w = world("whole-not-found", &[("acme.fake", &t)]);
    let (one, two) = (w.add("acme.fake", "FAKE-1"), w.add("acme.fake", "FAKE-2"));
    let inner = t.clone();
    w.h.fake.on_service_with("acme.fake.tracker.issues", move |payload| {
        let refs = json(payload)["refs"].as_array().unwrap().len();
        if refs > 1 {
            return Err(SeamError::new(code::NOT_FOUND, "one of them").to_err());
        }
        if json(payload)["refs"][0] == "id-FAKE-2" {
            return Err(SeamError::new(code::NOT_FOUND, "gone").to_err());
        }
        inner.serve("issues", payload)
    });
    w.h.command("agenda.sync").unwrap();
    assert_eq!(w.toast().0, "Synced 1 issue(s) with Fake", "not a failed sync");
    assert!(w.task(one).link.as_ref().unwrap().error.is_none());
    assert!(w.task(two).link.as_ref().unwrap().error.as_deref().unwrap().starts_with("not found in Fake"));
}

/// S4: one tracker that never says who it is holds up none of the others.
#[test]
fn trackers_are_asked_all_at_once() {
    let t = two_fakes();
    // acme.hangs is asked first, and is wired to nothing: it never answers.
    let w = world_with("concurrent", &["acme.hangs", "acme.fake"], |fake| serve(fake, "acme.fake", &t));
    assert!(w.h.ext.sync.is_probing());
    let asked: Vec<String> = w.h.fake.services().into_iter().map(|c| c.name).collect();
    assert!(asked.contains(&"acme.fake.tracker.about".to_string()), "asked without waiting for acme.hangs: {asked:?}");
    let mut w = w;
    w.later(Duration::from_secs(91));
    assert!(w.h.ext.sync.trackers.get("acme.fake").is_some_and(|f| f.ready()));
    assert!(w.h.ext.sync.trackers.get("acme.hangs").is_some_and(|f| !f.ready()), "installed (it didn't say nothing offers), not ready");
}

/// S5: an answer too big isn't read.
#[test]
fn an_answer_too_big_isnt_read() {
    let t = two_fakes();
    let mut w = world("too-big", &[("acme.fake", &t)]);
    w.add("acme.fake", "FAKE-1");
    let huge = format!("{{\"issues\": [], \"x\": \"{}\"}}", "x".repeat(9 * 1024 * 1024));
    w.h.fake.on_service_with("acme.fake.tracker.issues", move |_| Ok(huge.clone()));
    w.h.command("agenda.sync").unwrap();
    assert_eq!(w.toast().0, "Fake sync failed");
    assert!(w.toast().1.contains("more than an answer may be"));
}

/// Item 7 (Linear, GitHub): a tracker with no Blocked keeps it local.
#[test]
fn a_tracker_with_no_blocked_keeps_blocked_to_the_agenda() {
    let t = two_fakes();
    t.about(|a| a.blocked = false);
    let mut w = world("no-blocked", &[("acme.fake", &t)]);
    let id = w.add("acme.fake", "FAKE-1");
    w.set_status(id, Status::Blocked);
    w.set_status(id, Status::Todo);
    w.set_status(id, Status::Blocked);
    assert_eq!(w.task(id).status, Status::Blocked);
    assert!(t.calls_of("transitions").is_empty(), "nothing asked");
}

/// Items 1, 2, 5 (Linear, GitHub, GitLab): `project`, `create` answered
/// at once, a priority meaning none, and unassigning.
#[test]
fn project_create_opened_no_priority_and_nobody() {
    let t = two_fakes();
    t.project("ENG");
    t.edit("FAKE-1", |i| i.priority = Some(seam::Priority { name: "No priority".into(), level: None }));
    let mut w = world("forge-bits", &[("acme.fake", &t)]);
    t.about(|a| a.project_meta = None);
    w.ask_trackers();
    w.h.command("agenda.open").unwrap();
    assert_eq!(w.h.ext.project.as_ref().map(|p| p.issues.clone()), None, "no Fenix project in the fake: nothing to map");
    let id = w.add("acme.fake", "FAKE-1");
    assert_eq!(w.task(id).priority, crate::model::Priority::Medium);
    // Unassign.
    w.h.fake.on_pick("Assign FAKE-1", "Nobody (unassign)");
    w.act(A::Assign(id));
    assert_eq!(json(t.calls_of("update").last().unwrap())["set"], serde_json::json!({ "assignee": null }));
    assert_eq!(t.get("FAKE-1").unwrap().assignee, None);
    // A create answered at once.
    let local = w.local("Ship it");
    w.h.fake.on_service("acme.fake.tracker.create", Ok(r#"{"ref": null, "opened": "Opened a new issue in your browser -- I links it once it's made"}"#));
    w.h.fake.on_pick("Link to Fake", "+ Create a new Fake issue from this task...");
    w.act(A::Link(local));
    assert_eq!(w.last_said(), "Opened a new issue in your browser -- I links it once it's made");
}

/// The page asks a tracker with no metadata key which project you're in.
#[test]
fn a_tracker_says_which_of_its_projects_you_are_in() {
    let t = two_fakes();
    t.project("ENG");
    let mut w = world("project", &[("acme.fake", &t)]);
    t.about(|a| a.project_meta = None);
    w.ask_trackers();
    w.h.fake.project("app", "/project");
    w.h.command("agenda.open").unwrap();
    assert_eq!(w.h.ext.project.as_ref().map(|p| p.issues.clone()), Some(vec![("acme.fake".to_string(), "ENG".to_string())]));
}

/// Startup asks only the trackers your linked tasks are with.
#[test]
fn at_start_only_the_trackers_in_use_are_asked() {
    let folder = Folder::new("lazy");
    let mut store = crate::model::AgendaStore::default();
    let id = store.create_task("x".into(), String::new(), crate::model::Priority::Low, None);
    store.link(id, "acme.fake", "FAKE-1".into(), crate::sync::remote_update(&FakeTracker::issue("FAKE-1", "x")));
    crate::model::save(&folder.0.join("agenda.json"), &store).unwrap();
    let fake = Fake::new();
    let t = two_fakes();
    serve(&fake, "acme.fake", &t);
    for id in fenix_tracker::KNOWN {
        absent(&fake, id);
    }
    let agenda = crate::Agenda { kept: Some(folder.0.join("agenda.json")), ..Default::default() };
    let mut h = fenix_test_kit::Harness::wrap(fake, agenda);
    h.activate().unwrap();
    let abouts: Vec<String> = h.fake.services().into_iter().filter(|c| c.name.ends_with(".tracker.about")).map(|c| c.name).collect();
    assert_eq!(abouts, ["acme.fake.tracker.about"], "fenix.jira and the rest not woken");
}

/// C9: pages that drift give each issue once, and an empty page ends.
#[test]
fn drifting_pages_give_each_issue_once() {
    let t = two_fakes();
    let mut w = world("drift", &[("acme.fake", &t)]);
    let one = serde_json::to_value(FakeTracker::issue("FAKE-1", "x")).unwrap();
    let pages = Rc::new(RefCell::new(vec![
        serde_json::json!({ "issues": [one.clone()], "next": "a" }),
        serde_json::json!({ "issues": [one], "next": "b" }),
        serde_json::json!({ "issues": [], "next": "c" }),
    ]));
    let p = pages.clone();
    w.h.fake.on_service_with("acme.fake.tracker.assigned", move |_| Ok(if p.borrow().is_empty() { "{\"issues\": []}".into() } else { p.borrow_mut().remove(0).to_string() }));
    w.h.fake.on_pick_many("Import", &["FAKE-1  x  [Open]"]);
    w.h.command("agenda.import").unwrap();
    let picker = w.h.fake.pickers().into_iter().find(|p| p.title.starts_with("Import")).unwrap();
    assert_eq!(picker.items.len(), 1);
    let calls = w.h.fake.services().into_iter().filter(|c| c.name == "acme.fake.tracker.assigned").count();
    assert_eq!(calls, 3, "stopped at the empty page, though it named a next");
}
