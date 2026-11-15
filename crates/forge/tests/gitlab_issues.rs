//! GitLab's issues and the tracker seam served over them, on responses
//! written from GitLab's API documentation (`fixtures/gitlab`, its own
//! example responses with values changed -- not recordings): each seam
//! call, paging by `X-Next-Page`, nested groups, a self-hosted instance,
//! the workflow's scoped labels, time logged, every error, and no token
//! in any answer.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use fenix_forge::issues::gitlab::GitLabIssues;
use fenix_forge::issues::{Change, IssueRef, Issues, NewIssue, Paging, Search};
use fenix_forge::recorded::Recorded;
use fenix_forge::tracker::{Learned, Seam, Settings};
use fenix_tracker::{code, Found, Moves, Page, SeamError};
use futures::executor::block_on;

const TOKEN: &str = "not-a-real-gitlab-token-0001";
const BASE: &str = "https://gitlab.example.com";
const API: &str = "https://gitlab.example.com/api/v4";
const PROJECT: &str = "/projects/acme%2Fplatform%2Fapi";

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gitlab")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(dir().join(name)).unwrap()
}

fn gitlab() -> Rc<Recorded> {
    let d = dir();
    let r = Rc::new(Recorded::default());
    r.fixture(&d, "GET", "/api/v4/user", "user.json")
        .fixture_with(&d, "GET", "/api/v4/issues?order_by=updated_at&sort=desc&per_page=100&page=1&state=opened&scope=assigned_to_me", "issues-assigned-1.json", &[("X-Next-Page", "2"), ("X-Page", "1"), ("X-Per-Page", "100")])
        .fixture_with(&d, "GET", "/api/v4/issues?order_by=updated_at&sort=desc&per_page=100&page=2&state=opened&scope=assigned_to_me", "issues-assigned-2.json", &[("X-Next-Page", ""), ("X-Page", "2")])
        .fixture(&d, "GET", &format!("{PROJECT}/issues/4"), "issue-acme-platform-api-4.json")
        .fixture(&d, "GET", &format!("{PROJECT}/issues/4/notes"), "notes-acme-platform-api-4.json")
        .fixture(&d, "GET", &format!("{PROJECT}/issues/4/related_merge_requests"), "related-mrs-acme-platform-api-4.json")
        .fixture(&d, "GET", "/projects/acme%2Fweb/issues/8", "issue-acme-web-8.json")
        .fixture(&d, "GET", "/projects/acme%2Fweb/issues/2", "issue-acme-web-2.json")
        .fixture(&d, "GET", &format!("{PROJECT}/labels"), "labels-acme-platform-api.json")
        .fixture(&d, "GET", "/projects/acme%2Fweb/labels", "labels-acme-web.json")
        .fixture(&d, "GET", &format!("{PROJECT}/members/all"), "members-acme-platform-api.json")
        .fixture(&d, "GET", "/users?username=kim.park", "users-kim.json")
        .on("GET", &format!("{PROJECT}/issues/404"), 404, read("not-found.json"))
        .on("PUT", "/issues/", 200, read("issue-acme-platform-api-4.json"))
        .on("POST", "/notes", 201, r#"{"id": 900, "body": "x"}"#);
    r
}

fn settings() -> Settings {
    Settings { ready: true, not_ready: "set it up".into(), worklog: true, ..Settings::default() }
}

type GlSeam = Seam<GitLabIssues<Rc<Recorded>>>;

fn world_with(settings: Settings) -> (GlSeam, Rc<Recorded>) {
    let http = gitlab();
    (Seam::new(GitLabIssues::new(http.clone(), "https://gitlab.example.com/", TOKEN), settings, Rc::new(RefCell::new(Learned::default()))), http)
}

fn world() -> (GlSeam, Rc<Recorded>) {
    world_with(settings())
}

fn call(seam: &GlSeam, name: &str, payload: serde_json::Value) -> Result<String, String> {
    let answer = block_on(seam.serve(name, &payload.to_string()));
    let text = match &answer {
        Ok(t) | Err(t) => t,
    };
    assert!(!text.contains(TOKEN) && !text.contains("PRIVATE-TOKEN"), "a token in an answer: {text}");
    answer
}

fn err(answer: Result<String, String>) -> SeamError {
    SeamError::from_err(&answer.unwrap_err())
}

fn token_only_to_gitlab(http: &Recorded) {
    for s in http.sent.borrow().iter() {
        assert!(s.url.starts_with(API) || s.url == "https://gitlab.example.com/api/graphql", "{}", s.url);
        assert!(!s.url.contains(TOKEN) && !s.body.as_deref().unwrap_or_default().contains(TOKEN), "{s:?}");
        let with: Vec<&(String, String)> = s.headers.iter().filter(|(_, v)| v.contains(TOKEN)).collect();
        let graphql = s.url.ends_with("/api/graphql");
        let want = if graphql { ("Authorization".to_string(), format!("Bearer {TOKEN}")) } else { ("PRIVATE-TOKEN".to_string(), TOKEN.to_string()) };
        assert_eq!(with, [&want], "{s:?}");
    }
}

#[test]
fn about_says_gitlab_and_time_tracking_only_when_asked_for() {
    let (seam, _) = world();
    let about: fenix_tracker::About = serde_json::from_str(&call(&seam, "about", serde_json::json!({"seam": 1})).unwrap()).unwrap();
    assert_eq!((about.name.as_str(), about.ref_hint.as_deref()), ("GitLab", Some("group/project#123")));
    assert_eq!(about.can, ["comments", "edit.title", "edit.body", "edit.due", "edit.assignee", "edit.labels", "comment", "create", "project", "worklog"]);
    assert_eq!((about.id.as_str(), about.blocked, about.browse.clone()), ("fenix.gitlab-issues", true, None));
    let (seam, _) = world_with(Settings { worklog: false, ..settings() });
    let about: fenix_tracker::About = serde_json::from_str(&call(&seam, "about", serde_json::json!({"seam": 1})).unwrap()).unwrap();
    assert!(!about.can("worklog"));
    assert_eq!(err(call(&seam, "worklog", serde_json::json!({"seam": 1, "ref": "acme/web#8", "seconds": 60, "started": "2026-11-12T09:00:00Z"}))).code, code::UNSUPPORTED);
    let (seam, http) = world_with(Settings { ready: false, not_ready: "Set your GitLab server and token in SPC , (Forges)".into(), ..settings() });
    let e = err(call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["acme/web#8"]})));
    assert_eq!((e.code.as_str(), e.message.as_str()), (code::NOT_SET_UP, "Set your GitLab server and token in SPC , (Forges)"));
    assert!(http.asked().is_empty());
}

#[test]
fn whats_assigned_pages_by_x_next_page_across_nested_groups() {
    let (seam, http) = world();
    let first: Page = serde_json::from_str(&call(&seam, "assigned", serde_json::json!({"seam": 1, "limit": 200})).unwrap()).unwrap();
    assert_eq!(first.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["acme/platform/api#4", "acme/web#8"]);
    assert_eq!(first.next.as_deref(), Some("2:100"));
    let a = &first.issues[0];
    assert_eq!((a.project.as_deref(), a.url.as_deref()), (Some("acme/platform/api"), Some("https://gitlab.example.com/acme/platform/api/-/issues/4")));
    assert_eq!((a.state.id.as_str(), a.state.name.as_str(), a.state.stage.as_str(), a.state.column.as_str()), ("opened:Doing", "Doing", "doing", "doing"));
    assert_eq!((a.due.as_deref(), a.mine), (Some("2026-11-20"), Some(true)));
    assert_eq!(a.assignee.as_ref().map(|p| (p.id.as_str(), p.name.as_str())), Some(("jo", "Jo Bloggs")), "assigned by username");
    assert_eq!(a.body, "Bursts over **100 rps** are dropped.\r\n\r\n```\r\n429 Too Many Requests\r\n```", "as GitLab has it, byte for byte");
    assert_eq!(fenix_tracker::clean::text(&a.body, 10_000).0, "Bursts over **100 rps** are dropped.\n\n```\n429 Too Many Requests\n```", "line ends as \\n to be shown");
    assert_eq!((first.issues[1].state.name.as_str(), first.issues[1].state.stage.as_str(), first.issues[1].state.column.as_str()), ("Blocked", "doing", "blocked"), "a workflow step that says so");
    assert_eq!(a.id, "1004", "GitLab's global id");

    let second: Page = serde_json::from_str(&call(&seam, "assigned", serde_json::json!({"seam": 1, "limit": 3, "cursor": "2:100"})).unwrap()).unwrap();
    assert_eq!((second.next, second.issues[0].title.as_str()), (None, "Write the \u{1b}[31mrelease notes\u{202e}"), "kept as it came");
    assert_eq!(fenix_tracker::clean::line(&second.issues[0].title, 200), "Write the [31mrelease notes", "shown cleaned");
    // `since` is gone from seam 1: an old caller's is ignored.
    http.on("GET", "/api/v4/issues?", 200, "[]");
    call(&seam, "assigned", serde_json::json!({"seam": 1, "since": "2026-11-10T00:00:00Z"})).unwrap();
    assert!(!http.asked().last().unwrap().contains("updated_after"), "{:?}", http.asked());
    token_only_to_gitlab(&http);
}

#[test]
fn these_issues_by_key_or_address_with_peoples_comments_not_gitlabs() {
    let (seam, http) = world();
    let refs = ["https://gitlab.example.com/acme/platform/api/-/issues/4", "acme/web#8", "acme/platform/api#404", "https://gitlab.com/acme/web/-/issues/8"];
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": refs})).unwrap()).unwrap();
    assert_eq!(found.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["acme/platform/api#4", "acme/web#8"]);
    assert_eq!(found.missing, ["acme/platform/api#404", "https://gitlab.com/acme/web/-/issues/8"], "not there, and another instance's address");
    let comments: Vec<(&str, &str)> = found.issues[0].comments.iter().map(|c| (c.author.as_str(), c.body.as_str())).collect();
    assert_eq!(comments, [("Jo Bloggs", "Seen in production too."), ("Kim Park", "The limiter's window is 1 s.\r\n")], "oldest first, the system note left out");
    assert!(http.asked().contains(&format!("GET {API}{PROJECT}/issues/4/notes?sort=desc&order_by=created_at&per_page=100")));
    let closed: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["acme/web#2"]})).unwrap()).unwrap();
    assert_eq!((closed.issues[0].state.id.as_str(), closed.issues[0].state.column.as_str()), ("closed", "done"));
}

#[test]
fn moves_are_close_reopen_the_workflows_steps_and_local_ones_for_the_rest() {
    let (seam, http) = world();
    let moves: Moves = serde_json::from_str(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/platform/api#4"})).unwrap()).unwrap();
    let rows: Vec<(&str, &str, &str)> = moves.transitions.iter().map(|m| (m.id.as_str(), m.to.name.as_str(), m.to.column.as_str())).collect();
    assert_eq!(rows, [("close", "Closed", "done"), ("label:+workflow::To Do", "To Do", "todo"), ("label:+workflow::Review", "Review", "doing"), ("label:+workflow::Done", "Done", "done"), ("label:+workflow::Blocked", "Blocked", "blocked")], "an id with a space kept whole");
    assert_eq!(moves.transitions[1].row(), "To Do   (workflow::To Do)");
    assert!(http.sent.borrow().iter().all(|s| s.method == "GET"));

    // A project with no workflow labels: Doing and Blocked are the agenda's.
    http.on("GET", "/projects/acme%2Fweb/labels", 200, "[]");
    let learned_fresh = Seam::new(GitLabIssues::new(http.clone(), "https://gitlab.example.com", TOKEN), settings(), Rc::new(RefCell::new(Learned::default())));
    let plain: Moves = serde_json::from_str(&call(&learned_fresh, "transitions", serde_json::json!({"seam": 1, "ref": "acme/web#8"})).unwrap()).unwrap();
    assert_eq!(plain.transitions.iter().map(|m| (m.id.as_str(), m.to.column.as_str(), m.local)).collect::<Vec<_>>(), [("close", "done", false), ("local:doing", "doing", true)], "already Blocked by its step: no local move to it");
    let reopen: Moves = serde_json::from_str(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/web#2"})).unwrap()).unwrap();
    assert_eq!(reopen.transitions.iter().map(|m| (m.id.as_str(), m.to.stage.as_str())).collect::<Vec<_>>(), [("reopen", "todo")]);

    for (r, id, body, said) in [
        ("acme/platform/api#4", "close", r#"{"state_event":"close"}"#, "closed"),
        ("acme/platform/api#4", "reopen", r#"{"state_event":"reopen"}"#, "reopened"),
        ("acme/platform/api#4", "label:+workflow::To Do", r#"{"add_labels":"workflow::To Do"}"#, "moved to workflow::To Do"),
    ] {
        let answer = call(&seam, "transition", serde_json::json!({"seam": 1, "ref": r, "id": id, "op": "agenda:1:2"})).unwrap();
        assert_eq!(answer, format!(r#"{{"said":"{said}"}}"#));
        assert_eq!(http.last().unwrap().body.as_deref(), Some(body), "{id}");
        assert_eq!(http.last().unwrap().method, "PUT");
    }
    let n = http.sent.borrow().len();
    for id in ["close:not_planned", "label:+bug", "label:-blocked"] {
        assert_eq!(err(call(&seam, "transition", serde_json::json!({"seam": 1, "ref": "acme/web#8", "id": id}))).code, code::REFUSED, "{id}");
    }
    assert_eq!(call(&seam, "transition", serde_json::json!({"seam": 1, "ref": "acme/web#8", "id": "local:doing"})).unwrap(), "{}");
    assert_eq!(http.sent.borrow().len(), n, "nothing sent");
}

#[test]
fn edits_due_dates_assignees_comments_and_time() {
    let (seam, http) = world();
    for (set, body) in [
        (serde_json::json!({"title": "Shorter"}), r#"{"title":"Shorter"}"#),
        (serde_json::json!({"body": "New"}), r#"{"description":"New"}"#),
        (serde_json::json!({"due": "2026-12-01"}), r#"{"due_date":"2026-12-01"}"#),
        (serde_json::json!({"due": null}), r#"{"due_date":null}"#),
        (serde_json::json!({"assignee": {"id": "kim.park", "name": "Kim Park"}}), r#"{"assignee_ids":[12]}"#),
        (serde_json::json!({"assignee": null}), r#"{"assignee_ids":[0]}"#),
        (serde_json::json!({"labels": ["backend", "workflow::In Review"]}), r#"{"labels":"backend,workflow::In Review"}"#),
    ] {
        assert_eq!(call(&seam, "update", serde_json::json!({"seam": 1, "ref": "acme/platform/api#4", "set": set})).unwrap(), "{}");
        let sent = http.last().unwrap();
        assert_eq!((sent.line(), sent.body.as_deref()), (format!("PUT {API}{PROJECT}/issues/4"), Some(body)));
    }
    assert!(http.asked().contains(&format!("GET {API}/users?username=kim.park")), "a username to GitLab's id");
    assert_eq!(err(call(&seam, "update", serde_json::json!({"seam": 1, "ref": "acme/platform/api#4", "set": {"priority": "P1"}}))).code, code::UNSUPPORTED);

    call(&seam, "comment", serde_json::json!({"seam": 1, "ref": "acme/platform/api#4", "body": "On it."})).unwrap();
    assert_eq!((http.last().unwrap().line(), http.last().unwrap().body), (format!("POST {API}{PROJECT}/issues/4/notes"), Some(r#"{"body":"On it."}"#.to_string())));
    let said = call(&seam, "worklog", serde_json::json!({"seam": 1, "ref": "acme/platform/api#4", "seconds": 4500, "started": "2026-11-12T09:15:00.000+01:00"})).unwrap();
    assert_eq!(said, r#"{"said":"logged 1h15m"}"#);
    assert_eq!(http.last().unwrap().body.as_deref(), Some(r#"{"body":"/spend 1h15m 2026-11-12"}"#), "filed on the day it started");
    let people: fenix_tracker::People = serde_json::from_str(&call(&seam, "people", serde_json::json!({"seam": 1, "ref": "acme/platform/api#4"})).unwrap()).unwrap();
    assert_eq!(people.people.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect::<Vec<_>>(), [("jo", "Jo Bloggs"), ("kim.park", "Kim Park")]);
    token_only_to_gitlab(&http);
}

#[test]
fn errors_are_the_seams_with_gitlabs_words() {
    let (seam, http) = world();
    http.on("GET", "/projects/acme%2Fweb/issues/8", 401, read("unauthorized.json"));
    let e = err(call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/web#8"})));
    assert_eq!((e.code.as_str(), e.message.as_str()), (code::SIGNED_OUT, "HTTP 401 (Unauthorized): 401 Unauthorized"));
    let e = err(call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/platform/api#404"})));
    assert_eq!((e.code.as_str(), e.message.as_str()), (code::NOT_FOUND, "HTTP 404 (Not Found): 404 Not found"));
    http.on_with("GET", "/projects/acme%2Fweb/issues/2", 429, &[("Retry-After", "30"), ("RateLimit-Remaining", "0"), ("RateLimit-Reset", "1794650430")], "Retry later\n");
    let e = err(call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["acme/web#2"]})));
    assert_eq!((e.code.as_str(), e.retry_after, e.message.as_str()), (code::RATE_LIMITED, Some(30), "HTTP 429 (Too Many Requests)"));
    http.on("POST", "/notes", 403, read("forbidden.json"));
    let e = err(call(&seam, "comment", serde_json::json!({"seam": 1, "ref": "acme/platform/api#4", "body": "x"})));
    assert_eq!((e.code.as_str(), e.message.as_str()), (code::REFUSED, "HTTP 403 (Forbidden): 403 Forbidden"));
    http.on("PUT", "/issues/", 400, read("bad-request.json"));
    let e = err(call(&seam, "update", serde_json::json!({"seam": 1, "ref": "acme/platform/api#4", "set": {"title": ""}})));
    assert_eq!((e.code.as_str(), e.message.as_str()), (code::REFUSED, "HTTP 400 (Bad Request): title can't be blank"));
    http.on("GET", &format!("{PROJECT}/issues/4"), 500, "");
    assert_eq!(err(call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/platform/api#4"}))).code, code::FAILED);
    assert_eq!(err(call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/elsewhere#1"}))).code, code::OFFLINE);
    assert_eq!(err(call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "project#1"}))).code, code::BAD_REQUEST, "a project lives under a group");
    token_only_to_gitlab(&http);
}

#[test]
fn the_page_asks_for_searches_linked_requests_templates_and_a_new_issue() {
    let d = dir();
    let http = gitlab();
    let gl = GitLabIssues::new(http.clone(), BASE, TOKEN);
    let page = Paging::first(30);
    http.fixture(&d, "GET", "/api/v4/todos", "todos-mentioned.json");
    let mentioned = block_on(gl.search(&Search::Mentioned, &page)).unwrap();
    assert_eq!(mentioned.issues[0].key.key(), "acme/platform/api#4", "a to-do item's issue");
    assert_eq!(http.asked().last().unwrap(), &format!("GET {API}/todos?action=mentioned&type=Issue&state=pending&per_page=30&page=1"));
    for (search, want) in [
        (Search::Created, format!("GET {API}/issues?order_by=updated_at&sort=desc&per_page=30&page=1&state=opened&scope=created_by_me")),
        (Search::Project("acme/platform/api".into()), format!("GET {API}{PROJECT}/issues?order_by=updated_at&sort=desc&per_page=30&page=1&state=opened")),
        (Search::Query("project:acme/platform/api label:backend burst".into()), format!("GET {API}{PROJECT}/issues?order_by=updated_at&sort=desc&per_page=30&page=1&labels=backend&search=burst&state=opened")),
    ] {
        let _ = block_on(gl.search(&search, &page));
        assert_eq!(http.asked().last().unwrap(), &want, "{search:?}");
    }

    let linked = block_on(gl.linked(&IssueRef::new("acme/platform/api", 4))).unwrap();
    assert_eq!(linked.iter().map(|l| (l.reference.as_str(), l.state.as_str())).collect::<Vec<_>>(), [("acme/platform/api!11", "opened")]);

    http.fixture(&d, "GET", &format!("{PROJECT}/templates/issues"), "templates-acme-platform-api.json")
        .fixture(&d, "GET", &format!("{PROJECT}/templates/issues/Bug"), "template-bug.json")
        .fixture(&d, "GET", &format!("{PROJECT}/templates/issues/Feature"), "template-feature.json");
    let templates = block_on(gl.templates("acme/platform/api")).unwrap();
    assert_eq!(templates.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Bug", "Feature"]);
    assert!(templates[0].body.starts_with("## Summary"));

    http.on("POST", &format!("{PROJECT}/issues"), 201, read("issue-acme-platform-api-4.json"));
    let made = block_on(gl.create(&NewIssue { project: "acme/platform/api".into(), title: "T".into(), body: "B".into(), labels: vec!["backend".into(), "workflow::To Do".into()], assignees: vec!["kim.park".into()] })).unwrap();
    assert_eq!((made.key.key(), made.id.as_str()), ("acme/platform/api#4".to_string(), "1004"));
    assert_eq!(http.last().unwrap().body.as_deref(), Some(r#"{"assignee_ids":[12],"description":"B","labels":"backend,workflow::To Do","title":"T"}"#));
    block_on(gl.change(&IssueRef::new("acme/platform/api", 4), &Change::Labels(vec!["a".into(), "b".into()]))).unwrap();
    assert_eq!(http.last().unwrap().body.as_deref(), Some(r#"{"labels":"a,b"}"#));
    let issue = block_on(gl.issue(&IssueRef::new("acme/platform/api", 4))).unwrap();
    assert_eq!((issue.weight, issue.estimate, issue.spent, issue.milestone.as_deref()), (Some(3), Some(7200), Some(1800), Some("v4.0")));
    token_only_to_gitlab(&http);
}

#[test]
fn an_issue_asked_by_its_global_id_is_found_where_it_lives_now() {
    let (seam, http) = world();
    http.on("POST", "/api/graphql", 200, r#"{"data": {"issue": {"iid": "4", "reference": "acme/platform/api#4"}}}"#);
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["1004"]})).unwrap()).unwrap();
    assert_eq!((found.issues[0].key.as_str(), found.issues[0].asked.as_deref()), ("acme/platform/api#4", Some("1004")));
    let ask = http.sent.borrow().iter().find(|s| s.url.ends_with("/api/graphql")).cloned().unwrap();
    assert_eq!(ask.body.as_deref(), Some(r#"{"query":"query($id:IssueID!){issue(id:$id){iid reference(full:true)}}","variables":{"id":"gid://gitlab/Issue/1004"}}"#));
    http.on("POST", "/api/graphql", 200, r#"{"data": {"issue": null}}"#);
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["999", "acme/web#8"]})).unwrap()).unwrap();
    assert_eq!((found.missing.clone(), found.issues.len()), (vec!["999".to_string()], 1));
}

#[test]
fn a_write_to_a_moved_issue_goes_to_where_it_was_moved() {
    let (seam, http) = world();
    // acme/web#2 was moved: it stays behind, closed, saying where it went.
    let mut moved: serde_json::Value = serde_json::from_str(&read("issue-acme-web-2.json")).unwrap();
    moved["moved_to_id"] = serde_json::json!(1004);
    http.on("GET", "/projects/acme%2Fweb/issues/2", 200, moved.to_string());
    http.on("POST", "/api/graphql", 200, r#"{"data": {"issue": {"iid": "4", "reference": "acme/platform/api#4"}}}"#);
    call(&seam, "comment", serde_json::json!({"seam": 1, "ref": "acme/web#2", "body": "x", "op": "agenda:4:1"})).unwrap();
    assert_eq!(http.last().unwrap().line(), format!("POST {API}{PROJECT}/issues/4/notes"), "not the old, closed issue");
}
