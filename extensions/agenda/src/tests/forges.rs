//! The agenda driven by the real GitHub Issues and GitLab Issues seams
//! (`fenix_forge::tracker`) behind `github-issues.tracker.*` and
//! `gitlab-issues.tracker.*`, on responses written from the forges' API
//! documentation (`crates/forge/tests/fixtures`; not recordings): an
//! import, a linked task's moves, and no token in any payload.

use super::*;
use fenix_forge::issues::github::GitHubIssues;
use fenix_forge::issues::gitlab::GitLabIssues;
use fenix_forge::issues::Issues;
use fenix_forge::recorded::Recorded;
use fenix_forge::tracker::{about, Learned, Seam, Settings};
use futures_executor::block_on;

fn fixtures(forge: &str) -> std::path::PathBuf {
    std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/forge/tests/fixtures")).join(forge)
}

/// `seam` behind tracker `id`'s services.
fn wire<C: Issues + 'static>(fake: &Fake, id: &str, seam: Rc<Seam<C>>, host: fenix_forge::issues::Host) {
    let a = serde_json::to_string(&about(host, &seam.settings)).unwrap();
    fake.on_service(&seam::service(id, call::ABOUT), Ok(&a));
    for c in call::ALL.into_iter().filter(|c| *c != call::ABOUT) {
        let s = seam.clone();
        fake.on_service_with(&seam::service(id, c), move |payload| block_on(s.serve(c, payload)));
    }
}

fn ready() -> Settings {
    Settings { ready: true, ..Settings::default() }
}

#[test]
fn github_issues_seam_drives_the_agenda() {
    let d = fixtures("github");
    let http = Rc::new(Recorded::default());
    http.fixture(&d, "GET", "api.github.com/user", "user.json")
        .fixture(&d, "GET", "/search/issues", "search-assigned-1.json")
        .fixture(&d, "GET", "/repos/acme/rocket/issues/12", "issue-acme-rocket-12.json")
        .fixture(&d, "GET", "/repos/acme/rocket/issues/12/comments", "comments-acme-rocket-12.json")
        .fixture(&d, "GET", "/repos/acme/rocket/labels", "labels-acme-rocket.json")
        .on("PATCH", "/repos/", 200, "{}")
        .on("POST", "/labels", 200, "[]");
    let seam = Rc::new(Seam::new(GitHubIssues::new(http.clone(), None, TOKEN), ready(), Rc::new(RefCell::new(Learned::default()))));
    let mut w = world_with("github-issues", &["github-issues"], |fake| wire(fake, "github-issues", seam.clone(), fenix_forge::issues::Host::GitHub));
    assert!(w.h.ext.sync.trackers.get("github-issues").unwrap().ready());

    w.h.fake.on_pick_many("Import", &["acme/rocket#12  Login times out after five minutes  [Open]"]);
    w.h.command("agenda.import").unwrap();
    let id = w.h.ext.store.find_by_key("github-issues", "acme/rocket#12").expect("imported");
    assert_eq!(w.task(id).status, Status::InProgress, "the in-progress label");
    assert!(!w.h.ext.store.tasks.iter().any(|t| t.title.contains("Fix the login timer")), "the pull request isn't offered");

    // Blocked: the one move that gets there, the blocked label.
    w.set_status(id, Status::Blocked);
    assert_eq!(http.last().map(|s| (s.line(), s.body)), Some(("POST https://api.github.com/repos/acme/rocket/issues/12/labels".into(), Some(r#"{"labels":["blocked"]}"#.into()))));
    assert_eq!(w.last_said(), "acme/rocket#12: labelled blocked");

    // Done: two moves get there -- asked, which.
    w.h.fake.on_pick("acme/rocket#12", "Close as not planned  ->  Closed as not planned");
    w.set_status(id, Status::Done);
    let picker = w.h.fake.pickers().into_iter().last().unwrap();
    assert!(picker.items.iter().any(|i| i.label == "Close as completed  ->  Closed"), "{:?}", picker.items);
    assert_eq!(http.last().unwrap().body.as_deref(), Some(r#"{"state":"closed","state_reason":"not_planned"}"#));
    w.no_secrets();
}

#[test]
fn gitlab_issues_seam_drives_the_agenda() {
    let d = fixtures("gitlab");
    let http = Rc::new(Recorded::default());
    let project = "/projects/acme%2Fplatform%2Fapi";
    http.fixture(&d, "GET", "/api/v4/user", "user.json")
        .fixture(&d, "GET", "/api/v4/issues?", "issues-assigned-1.json")
        .fixture(&d, "GET", &format!("{project}/issues/4"), "issue-acme-platform-api-4.json")
        .fixture(&d, "GET", &format!("{project}/issues/4/notes"), "notes-acme-platform-api-4.json")
        .fixture(&d, "GET", &format!("{project}/labels"), "labels-acme-platform-api.json")
        .on("PUT", "/issues/", 200, "{}");
    let seam = Rc::new(Seam::new(GitLabIssues::new(http.clone(), "https://gitlab.example.com", "not-a-real-gitlab-token-0001"), ready(), Rc::new(RefCell::new(Learned::default()))));
    let mut w = world_with("gitlab-issues", &["gitlab-issues"], |fake| wire(fake, "gitlab-issues", seam.clone(), fenix_forge::issues::Host::GitLab));

    w.h.fake.on_pick_many("Import", &["acme/platform/api#4  Rate limiter drops bursts  [Doing]"]);
    w.h.command("agenda.import").unwrap();
    let id = w.h.ext.store.find_by_key("gitlab-issues", "acme/platform/api#4").expect("imported, nested groups and all");
    assert_eq!(w.task(id).status, Status::InProgress, "workflow::Doing");
    assert_eq!(w.task(id).due, chrono::NaiveDate::from_ymd_opt(2026, 11, 20));

    w.set_status(id, Status::Blocked);
    assert_eq!(http.last().unwrap().body.as_deref(), Some(r#"{"add_labels":"blocked"}"#));
    w.h.fake.on_pick("acme/platform/api#4", "Close  ->  Closed");
    w.set_status(id, Status::Done);
    assert_eq!(http.last().unwrap().body.as_deref(), Some(r#"{"state_event":"close"}"#));
    for s in w.h.fake.services() {
        assert!(!s.payload.contains("glpat-"), "{}", s.payload);
    }
    w.no_secrets();
}
