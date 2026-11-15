//! The agenda driven by this tracker's seam (`fenix_forge::tracker` over
//! GitLab) behind `fenix.gitlab-issues.tracker.*`, with the agenda's own
//! test world (`fenix_agenda::testing`), a self-hosted GitLab answering
//! from responses written from its API documentation
//! (`crates/forge/tests/fixtures`; not recordings): an import across
//! nested groups, a linked task through the workflow, no token in any
//! payload. The gate in front of the seam's writes is the extension's,
//! tested in its own crate.

use std::cell::RefCell;
use std::rc::Rc;

use fenix_agenda::model::Status;
use fenix_agenda::testing::world_with;
use fenix_forge::issues::gitlab::GitLabIssues;
use fenix_forge::issues::Host;
use fenix_forge::recorded::Recorded;
use fenix_forge::tracker::{about, Learned, Seam, Settings};
use fenix_tracker::{self as seam, call};
use futures::executor::block_on;

const ID: &str = "fenix.gitlab-issues";
const TOKEN: &str = "not-a-real-gitlab-token-0001";

#[test]
fn gitlab_issues_seam_drives_the_agenda() {
    let d = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/forge/tests/fixtures/gitlab")).to_path_buf();
    let http = Rc::new(Recorded::default());
    let project = "/projects/acme%2Fplatform%2Fapi";
    http.fixture(&d, "GET", "/api/v4/user", "user.json")
        .fixture(&d, "GET", "/api/v4/issues?", "issues-assigned-1.json")
        .fixture(&d, "GET", &format!("{project}/issues/4"), "issue-acme-platform-api-4.json")
        .fixture(&d, "GET", &format!("{project}/issues/4/notes"), "notes-acme-platform-api-4.json")
        .fixture(&d, "GET", &format!("{project}/labels"), "labels-acme-platform-api.json")
        .on("PUT", "/issues/", 200, "{}")
        // The agenda names the issue by its id: GitLab's GraphQL says where it is now.
        .on("POST", "/api/graphql", 200, r#"{"data": {"issue": {"iid": "4", "reference": "acme/platform/api#4"}}}"#);
    let settings = Settings { ready: true, ..Settings::default() };
    let tracker = Rc::new(Seam::new(GitLabIssues::new(http.clone(), "https://gitlab.example.com", TOKEN), settings, Rc::new(RefCell::new(Learned::default()))));
    let mut w = world_with("gitlab-issues", &[ID], |fake| {
        let a = serde_json::to_string(&about(Host::GitLab, &tracker.settings)).unwrap();
        fake.on_service(&seam::service(ID, call::ABOUT), Ok(&a));
        for c in call::ALL.into_iter().filter(|c| *c != call::ABOUT) {
            let s = tracker.clone();
            fake.on_service_with(&seam::service(ID, c), move |payload| block_on(s.serve(c, payload)));
        }
    });

    w.h.fake.on_pick_many("Import", &["acme/platform/api#4  Rate limiter drops bursts  [Doing]"]);
    w.h.command("agenda.import").unwrap();
    let id = w.find(ID, "acme/platform/api#4").expect("imported, nested groups and all");
    assert_eq!(w.task(id).status, Status::InProgress, "workflow::Doing");
    assert_eq!(w.task(id).due, chrono::NaiveDate::from_ymd_opt(2026, 11, 20));

    // Blocked: the project's own workflow step.
    w.set_status(id, Status::Blocked);
    assert_eq!(http.last().unwrap().body.as_deref(), Some(r#"{"add_labels":"workflow::Blocked"}"#));
    // To do: a step whose name has a space, kept whole.
    w.set_status(id, Status::Todo);
    assert_eq!(http.last().unwrap().body.as_deref(), Some(r#"{"add_labels":"workflow::To Do"}"#));
    // Done: closing, or the Done step -- asked, which.
    w.h.fake.on_pick("acme/platform/api#4", "Close  ->  Closed");
    w.set_status(id, Status::Done);
    assert_eq!(http.last().unwrap().body.as_deref(), Some(r#"{"state_event":"close"}"#));
    assert!(http.sent.borrow().iter().any(|s| s.url.ends_with("/graphql") && s.body.as_deref().is_some_and(|b| b.contains("\"id\""))), "written to the issue by its id");
    w.no_secrets();
    for s in w.h.fake.services() {
        assert!(!s.payload.contains(TOKEN));
    }
}
