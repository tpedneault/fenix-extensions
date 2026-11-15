//! The agenda driven by this tracker's seam (`fenix_forge::tracker` over
//! GitHub) behind `fenix.github-issues.tracker.*`, with the agenda's own
//! test world (`fenix_agenda::testing`), GitHub answering from responses
//! written from its REST API description (`crates/forge/tests/fixtures`;
//! not recordings): an import, a linked task's moves, no token in any
//! payload. The gate in front of the seam's writes is the extension's,
//! tested in its own crate; here the agenda meets the seam itself.

use std::cell::RefCell;
use std::rc::Rc;

use fenix_agenda::model::Status;
use fenix_agenda::testing::world_with;
use fenix_forge::issues::github::GitHubIssues;
use fenix_forge::issues::Host;
use fenix_forge::recorded::Recorded;
use fenix_forge::tracker::{about, Learned, Seam, Settings};
use fenix_tracker::{self as seam, call};
use futures::executor::block_on;

const ID: &str = "fenix.github-issues";
const TOKEN: &str = "ghp_16C7e42F292c6912E7710c838347Ae178B4a";

#[test]
fn github_issues_seam_drives_the_agenda() {
    let d = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/forge/tests/fixtures/github")).to_path_buf();
    let http = Rc::new(Recorded::default());
    http.fixture(&d, "GET", "api.github.com/user", "user.json")
        .fixture(&d, "GET", "/search/issues", "search-assigned-1.json")
        .fixture(&d, "GET", "/repos/acme/rocket/issues/12", "issue-acme-rocket-12.json")
        .fixture(&d, "GET", "/repos/acme/rocket/issues/12/comments", "comments-acme-rocket-12.json")
        .on("PATCH", "/repos/", 200, "{}")
        // The agenda names the issue by its id: GitHub's GraphQL says where it is now.
        .on("POST", "api.github.com/graphql", 200, r#"{"data": {"node": {"number": 12, "repository": {"nameWithOwner": "acme/rocket"}}}}"#);
    let settings = Settings { ready: true, ..Settings::default() };
    let tracker = Rc::new(Seam::new(GitHubIssues::new(http.clone(), None, TOKEN), settings, Rc::new(RefCell::new(Learned::default()))));
    let mut w = world_with("github-issues", &[ID], |fake| {
        let a = serde_json::to_string(&about(Host::GitHub, &tracker.settings)).unwrap();
        fake.on_service(&seam::service(ID, call::ABOUT), Ok(&a));
        for c in call::ALL.into_iter().filter(|c| *c != call::ABOUT) {
            let s = tracker.clone();
            fake.on_service_with(&seam::service(ID, c), move |payload| block_on(s.serve(c, payload)));
        }
    });

    w.h.fake.on_pick_many("Import", &["acme/rocket#12  Login times out after five minutes  [Open]"]);
    w.h.command("agenda.import").unwrap();
    let id = w.find(ID, "acme/rocket#12").expect("imported");
    assert_eq!(w.task(id).status, Status::Todo, "open is To do");
    assert!(w.find(ID, "acme/rocket#13").is_none(), "the pull request isn't offered");

    // Doing: GitHub has none -- the card moves, nothing is sent.
    let sent = http.sent.borrow().len();
    w.set_status(id, Status::InProgress);
    assert_eq!(w.task(id).status, Status::InProgress);
    let read = |s: &fenix_forge::recorded::Sent| s.method == "GET" || s.url.ends_with("/graphql");
    assert!(http.sent.borrow()[sent..].iter().all(read), "nothing written for Doing");
    // Blocked: GitHub has none, so the agenda keeps it to itself.
    w.set_status(id, Status::Blocked);
    assert_eq!(w.task(id).status, Status::Blocked);
    assert!(http.sent.borrow().iter().all(read));

    // Done: two moves get there -- asked, which.
    w.h.fake.on_pick("acme/rocket#12", "Close as not planned  ->  Closed as not planned");
    w.set_status(id, Status::Done);
    let picker = w.h.fake.pickers().into_iter().last().unwrap();
    assert!(picker.items.iter().any(|i| i.label == "Close as completed  ->  Closed"), "{:?}", picker.items);
    assert_eq!(http.last().unwrap().body.as_deref(), Some(r#"{"state":"closed","state_reason":"not_planned"}"#));
    assert!(http.sent.borrow().iter().any(|s| s.url.ends_with("/graphql") && s.body.as_deref().is_some_and(|b| b.contains("\"id\""))), "written to the issue by its id");
    w.no_secrets();
    for s in w.h.fake.services() {
        assert!(!s.payload.contains(TOKEN));
    }
}
