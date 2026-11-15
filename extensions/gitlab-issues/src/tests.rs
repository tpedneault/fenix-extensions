//! The extension on the test kit's fake host, a self-hosted GitLab
//! answering from `crates/forge`'s fixtures (written from GitLab's API
//! documentation; not recordings): the seam's services, the page's flows
//! across nested groups, a branch from an issue, the agent tools, and the
//! token nowhere it shouldn't be.

use fenix_test_kit::api::{HttpResponse, Key};
use fenix_test_kit::{Fake, Harness, Ran};

use super::*;

const TOKEN: &str = "not-a-real-gitlab-token-0001";
const API: &str = "https://gitlab.example.com/api/v4";
const PROJECT: &str = "https://gitlab.example.com/api/v4/projects/acme%2Fplatform%2Fapi";

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/../../crates/forge/tests/fixtures/gitlab/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn ok(body: String) -> HttpResponse {
    HttpResponse::new(200, body)
}

/// Your GitLab, answering from the fixtures; the project a clone of
/// acme/platform/api with a clean tree.
fn fake() -> Fake {
    let f = Fake::new();
    f.secret("gitlab.token", TOKEN).setting("gitlab.base_url", "https://gitlab.example.com/");
    f.on_http(&format!("GET {API}/user"), ok(fixture("user.json")))
        .on_http(&format!("GET {API}/issues?order_by=updated_at&sort=desc&per_page=50&page=1&state=opened&scope=assigned_to_me"), ok(fixture("issues-assigned-1.json")))
        .on_http(&format!("GET {PROJECT}/issues/4"), ok(fixture("issue-acme-platform-api-4.json")))
        .on_http(&format!("GET {PROJECT}/issues/4/notes*"), ok(fixture("notes-acme-platform-api-4.json")))
        .on_http(&format!("GET {PROJECT}/issues/4/related_merge_requests"), ok(fixture("related-mrs-acme-platform-api-4.json")))
        .on_http(&format!("GET {PROJECT}/labels*"), ok(fixture("labels-acme-platform-api.json")))
        .on_http(&format!("GET {PROJECT}/templates/issues"), ok(fixture("templates-acme-platform-api.json")))
        .on_http(&format!("GET {PROJECT}/templates/issues/Bug"), ok(fixture("template-bug.json")))
        .on_http(&format!("GET {PROJECT}/templates/issues/Feature"), ok(fixture("template-feature.json")))
        .on_http(&format!("GET {API}/users?username=kim.park"), ok(fixture("users-kim.json")))
        .on_http(&format!("PUT {PROJECT}/issues/4"), ok(fixture("issue-acme-platform-api-4.json")))
        .on_http(&format!("POST {PROJECT}/issues/4/notes"), HttpResponse::new(201, r#"{"id": 900}"#))
        .on_http(&format!("POST {PROJECT}/issues"), HttpResponse::new(201, fixture("issue-acme-platform-api-4.json")));
    f.on_spawn("git remote -v", Ran::ok("origin\tgit@gitlab.example.com:acme/platform/api.git (fetch)\norigin\tgit@gitlab.example.com:acme/platform/api.git (push)"))
        .on_spawn("git status --porcelain", Ran::ok(""))
        .on_spawn("git branch --format=%(refname:short)", Ran::ok("main"))
        .on_spawn("git symbolic-ref refs/remotes/origin/HEAD", Ran::ok("refs/remotes/origin/main"))
        .on_spawn("git check-ref-format", Ran::ok(""))
        .on_spawn("git fetch", Ran::ok(""))
        .on_spawn("git switch", Ran::ok(""));
    f
}

fn harness(f: Fake) -> Harness<IssuesExt> {
    Harness::with_fake(f)
}

/// No token in any answer, notification, log line, payload or URL; and
/// every request with it went to your GitLab's API.
fn no_token(h: &Harness<IssuesExt>) {
    let s = h.fake.state();
    let mut texts: Vec<String> = s.answers.values().map(|a| format!("{a:?}")).collect();
    texts.extend(h.fake.notifications());
    texts.extend(h.fake.logs());
    texts.extend(h.fake.toasts().iter().map(|t| format!("{} {}", t.summary, t.body)));
    texts.extend(h.fake.services().iter().map(|s| s.payload.clone()));
    texts.extend(h.fake.copied());
    texts.extend(h.fake.calls().into_iter().filter(|c| !c.starts_with("http ")));
    for t in texts {
        assert!(!t.contains(TOKEN), "the token in: {t}");
    }
    for (_, req) in h.fake.requests() {
        assert!(req.url.starts_with(API) && !req.url.contains(TOKEN), "{}", req.url);
        assert!(req.headers.iter().all(|(k, v)| !v.contains(TOKEN) || (k == "PRIVATE-TOKEN" && v == TOKEN)));
    }
}

fn answer(h: &Harness<IssuesExt>) -> Result<String, String> {
    h.fake.state().answers.values().last().cloned().expect("an answer")
}

#[test]
fn the_manifest_serves_every_seam_call_and_asks_only_for_your_gitlab() {
    let manifest: toml::Value = toml::from_str(include_str!("../extension.toml")).unwrap();
    let services: Vec<&str> = manifest["contributes"]["services"].as_array().unwrap().iter().filter_map(|s| s.as_str()).collect();
    for call in seam::call::ALL {
        assert!(services.contains(&seam::service(ID, call).as_str()), "{call}");
    }
    let perms = &manifest["permissions"];
    assert_eq!(perms["network"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>(), ["${setting:gitlab.base_url}"]);
    assert_eq!(perms["secrets"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>(), ["gitlab.token"]);
    for tool in manifest["contributes"]["agent_tools"].as_array().unwrap() {
        assert_eq!(tool["class"].as_str(), Some("read"));
    }
}

#[test]
fn about_is_ready_with_fenixs_server_and_token_and_says_how_otherwise() {
    let mut h = harness(fake());
    let about: seam::About = serde_json::from_str(&h.serve("fenix.gitlab-issues.tracker.about", r#"{"seam": 1}"#).unwrap().unwrap()).unwrap();
    assert!(about.ready && about.name == "GitLab" && about.id == ID && !about.can("worklog") && about.can("edit.due"));
    assert!(h.fake.requests().is_empty(), "about asks nothing");
    let f = fake();
    f.setting("gitlab-issues.time_tracking", "true");
    let mut h = harness(f);
    assert!(serde_json::from_str::<seam::About>(&h.serve("fenix.gitlab-issues.tracker.about", r#"{"seam": 1}"#).unwrap().unwrap()).unwrap().can("worklog"));

    for (server, token) in [("", TOKEN), ("https://gitlab.example.com", "")] {
        let f = fake();
        f.setting("gitlab.base_url", server).secret("gitlab.token", token);
        let mut h = harness(f);
        let about: seam::About = serde_json::from_str(&h.serve("fenix.gitlab-issues.tracker.about", r#"{"seam": 1}"#).unwrap().unwrap()).unwrap();
        assert_eq!(about.not_ready.as_deref(), Some("Set your GitLab server and token in SPC , (Forges) -- the ones Review uses; a personal access token with the api scope"));
        h.serve("fenix.gitlab-issues.tracker.issues", r#"{"seam": 1, "refs": ["acme/platform/api#4"]}"#);
        assert_eq!(SeamError::from_err(&answer(&h).unwrap_err()).code, seam::code::NOT_SET_UP);
        assert!(h.fake.requests().is_empty());
    }
}

#[test]
fn the_seams_calls_answer_from_gitlab() {
    let f = fake();
    f.setting("gitlab-issues.time_tracking", "true").setting("gitlab-issues.seam_writes", "always");
    let mut h = harness(f);
    h.serve("fenix.gitlab-issues.tracker.assigned", r#"{"seam": 1, "limit": 50}"#);
    let page: seam::Page = serde_json::from_str(&answer(&h).unwrap()).unwrap();
    assert_eq!(page.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["acme/platform/api#4", "acme/web#8"]);
    h.serve("fenix.gitlab-issues.tracker.update", r#"{"seam": 1, "ref": "acme/platform/api#4", "set": {"due": "2026-12-01"}}"#);
    assert_eq!(answer(&h), Ok("{}".into()));
    h.serve("fenix.gitlab-issues.tracker.worklog", r#"{"seam": 1, "ref": "acme/platform/api#4", "seconds": 5400, "started": "2026-11-12T09:15:00+01:00"}"#);
    assert_eq!(answer(&h), Ok(r#"{"said":"logged 1h30m"}"#.into()));
    let bodies: Vec<String> = h.fake.requests().iter().filter(|(_, r)| r.method != "GET").map(|(_, r)| String::from_utf8_lossy(&r.body).into_owned()).collect();
    assert_eq!(bodies, [r#"{"due_date":"2026-12-01"}"#, r#"{"body":"/spend 1h30m 2026-11-12"}"#]);
    no_token(&h);
}

#[test]
fn the_page_lists_opens_comments_and_moves_an_issue_through_its_workflow() {
    let f = fake();
    f.on_prompt("Comment on acme/platform/api#4", "Patched.")
        .on_prompt("Assignees of acme/platform/api#4", "kim.park")
        .on_pick("Move acme/platform/api#4", "Review   (workflow::Review)");
    let mut h = harness(f);
    h.command("gitlab-issues.open").unwrap();
    let page = h.page(KIND);
    let screen = h.screen(page);
    assert!(screen.contains("4 acme/platf") && screen.contains("acme/platform/api#4  Rate limiter drops bursts"), "{screen}");
    h.key(page, Key::Enter);
    let screen = fenix_test_kit::render::text(&h.render(page, 100, 60));
    for want in ["Assignees  Jo Bloggs (jo), Kim Park (kim.park)", "Labels     workflow::Doing, backend", "Milestone  v4.0", "Due        2026-11-20", "Weight     3", "Time       30m spent of 2h estimated", "MR    acme/platform/api!11  opened -- Burst-friendly limiter", "2 comment(s)", "Jo Bloggs added ~workflow::Doing scoped label"] {
        assert!(screen.contains(want), "{want}:\n{screen}");
    }
    h.key(page, Key::Char('C'));
    h.key(page, Key::Char('A'));
    h.key(page, Key::Char('s'));
    let writes: Vec<(String, String)> = h.fake.requests().iter().filter(|(_, r)| r.method != "GET").map(|(_, r)| (r.method.clone(), String::from_utf8_lossy(&r.body).into_owned())).collect();
    assert_eq!(writes, [("POST".into(), r#"{"body":"Patched."}"#.into()), ("PUT".into(), r#"{"assignee_ids":[12]}"#.into()), ("PUT".into(), r#"{"add_labels":"workflow::Review"}"#.into())]);
    no_token(&h);
}

#[test]
fn the_seams_writes_are_gated_and_the_project_is_the_remotes() {
    const COMMENT: &str = r#"{"seam": 1, "ref": "acme/platform/api#4", "body": "x", "op": "agenda:1:1"}"#;
    let notes = |h: &Harness<IssuesExt>| h.fake.requests().iter().filter(|(_, r)| r.method == "POST").count();
    let f = fake();
    f.setting("gitlab-issues.seam_writes", "never");
    let mut h = harness(f);
    assert_eq!(SeamError::from_err(&h.serve("fenix.gitlab-issues.tracker.comment", COMMENT).unwrap().unwrap_err()).code, seam::code::REFUSED);
    assert_eq!(notes(&h), 0);

    let mut h = harness(fake());
    assert_eq!(h.serve("fenix.gitlab-issues.tracker.comment", COMMENT), None, "held while asking");
    let gate = h.fake.pickers().into_iter().last().unwrap();
    assert_eq!(gate.title, "An extension asks to change issues in GitLab for you (Fenix can't tell which; usually the Agenda) -- allow?");
    assert_eq!(notes(&h), 0);
    h.event(api::Event::Picked((gate.id, Some("allow".into()))));
    assert_eq!((notes(&h), answer(&h)), (1, Ok("{}".to_string())));
    assert_eq!(h.serve("fenix.gitlab-issues.tracker.comment", COMMENT), Some(Ok("{}".into())));
    assert_eq!(notes(&h), 1, "a repeated op isn't done twice");
    // The same op on another call is another change.
    h.serve("fenix.gitlab-issues.tracker.update", r#"{"seam": 1, "ref": "acme/platform/api#4", "set": {"title": "T"}, "op": "agenda:1:1"}"#);
    assert!(h.fake.requests().iter().any(|(_, r)| r.method == "PUT"), "an update with a comment's op is still made");

    // A question that runs out refuses what waited, and the next asks again.
    let mut h = harness(fake());
    h.serve("fenix.gitlab-issues.tracker.comment", COMMENT);
    h.fire_timers();
    let e = SeamError::from_err(&answer(&h).unwrap_err());
    assert_eq!(e.message, "no answer came to whether to allow changes through GitLab -- r on the task asks again");
    h.serve("fenix.gitlab-issues.tracker.comment", COMMENT);
    assert_eq!(h.fake.pickers().len(), 2, "asked again");

    h.serve("fenix.gitlab-issues.tracker.project", r#"{"seam": 1}"#);
    assert_eq!(answer(&h), Ok(r#"{"project":"acme/platform/api"}"#.into()));
}

#[test]
fn a_new_issue_with_a_template_and_the_agendas_create() {
    let f = fake();
    f.on_pick("Issue template", "Bug").on_pick("asks to change issues in GitLab", fenix_tracker::gate::ALLOW);
    let mut h = harness(f);
    h.serve("fenix.gitlab-issues.tracker.create", r#"{"seam": 1, "title": "From a task", "body": ""}"#);
    let page = h.page(KIND);
    assert!(h.screen(page).contains("Project      acme/platform/api"), "{}", h.screen(page));
    h.key(page, Key::Char('t'));
    h.keys(page, "SS");
    let (_, post) = h.fake.requests().into_iter().rev().find(|(_, r)| r.method == "POST").unwrap();
    assert_eq!(String::from_utf8_lossy(&post.body), r###"{"description":"## Summary\n\n## Steps to reproduce\n\n/label ~bug\n","title":"From a task"}"###);
    assert_eq!(answer(&h), Ok(r#"{"ref":"acme/platform/api#4"}"#.into()));
}

#[test]
fn a_branch_is_made_only_in_the_issues_project_by_whatever_name_it_was_cloned() {
    let f = fake();
    f.on_spawn("git remote -v", Ran::ok("origin\tssh://git@gitlab-ssh.internal:2222/acme/platform/api.git (fetch)"));
    f.on_spawn("git symbolic-ref", Ran::fails(1, "not a symbolic ref"));
    f.on_prompt("Branch for acme/platform/api#4", "4-rate-limiter");
    let mut h = harness(f);
    h.command("gitlab-issues.open").unwrap();
    let page = h.page(KIND);
    h.key(page, Key::Char('b'));
    let prompt = h.fake.prompts().into_iter().last().unwrap();
    assert_eq!((prompt.label.as_str(), prompt.initial.as_str()), ("Branch for acme/platform/api#4, from what's checked out:", "4-rate-limiter-drops-bursts"));
    assert!(h.fake.spawned().iter().any(|(_, l)| l == "git switch -c 4-rate-limiter"));

    let f = fake();
    f.on_spawn("git remote -v", Ran::ok("origin\tgit@gitlab.example.com:acme/platform.git (fetch)"));
    let mut h = harness(f);
    h.command("gitlab-issues.open").unwrap();
    let page = h.page(KIND);
    h.key(page, Key::Char('b'));
    assert!(!h.fake.spawned().iter().any(|(_, l)| l.starts_with("git switch")));
    assert!(h.ext.pages[&page].status.as_ref().unwrap().0.starts_with("this project is gitlab.example.com/acme/platform, not acme/platform/api"));
}

#[test]
fn the_agent_tools_read_capped_data_with_a_note() {
    let mut h = harness(fake());
    h.serve("gitlab-issues.read_issue", r#"{"ref": "https://gitlab.example.com/acme/platform/api/-/issues/4"}"#);
    let text = answer(&h).unwrap();
    assert!(text.starts_with(fenix_issues::agent::NOTE));
    assert!(text.contains("Linked: acme/platform/api!11 (opened) Burst-friendly limiter"));
    assert!(!text.contains("added ~workflow::Doing"), "the forge's narration left out");
    h.serve("gitlab-issues.my_issues", r#"{"limit": 50}"#);
    assert!(answer(&h).unwrap().contains("- acme/web#8  Docs: setup page [workflow::Blocked]"));
    assert!(h.fake.requests().iter().all(|(_, r)| r.method == "GET"));
    no_token(&h);
}
