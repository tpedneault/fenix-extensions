//! GitHub's issues and the tracker seam served over them, on responses
//! written from GitHub's REST API description (`fixtures/github`, its
//! own examples with values changed -- not recordings): each seam call,
//! what it asks GitHub and what it answers, paging, pull requests left
//! out, an Enterprise Server, every error, and no token in any answer.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use fenix_forge::issues::github::GitHubIssues;
use fenix_forge::issues::{Change, IssueRef, Issues, NewIssue, Paging, Search};
use fenix_forge::recorded::Recorded;
use fenix_forge::tracker::{Learned, Seam, Settings};
use fenix_tracker::{code, Found, Moves, Page, SeamError};
use futures::executor::block_on;

const TOKEN: &str = "ghp_16C7e42F292c6912E7710c838347Ae178B4a";
const API: &str = "https://api.github.com";

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github")
}

/// GitHub answering from the fixtures.
fn github() -> Rc<Recorded> {
    let d = dir();
    let r = Rc::new(Recorded::default());
    let link = r#"<https://api.github.com/search/issues?q=x&page=2>; rel="next", <https://api.github.com/search/issues?q=x&page=2>; rel="last""#;
    r.fixture(&d, "GET", "api.github.com/user", "user.json")
        .fixture_with(&d, "GET", "/search/issues?q=is%3Aissue%20is%3Aopen%20assignee%3A%40me", "search-assigned-1.json", &[("Link", link)])
        .fixture(&d, "GET", "/search/issues?q=is%3Aissue%20is%3Aopen%20assignee%3A%40me%20archived%3Afalse&sort=updated&order=desc&per_page=100&page=2", "search-assigned-2.json")
        .fixture(&d, "GET", "/repos/acme/rocket/issues/12", "issue-acme-rocket-12.json")
        .fixture(&d, "GET", "/repos/acme/rocket/issues/12/comments", "comments-acme-rocket-12.json")
        .fixture(&d, "GET", "/repos/acme/rocket/issues/12/timeline", "timeline-acme-rocket-12.json")
        .fixture(&d, "GET", "/repos/acme/rocket/issues/13", "issue-acme-rocket-13.json")
        .fixture(&d, "GET", "/repos/acme/rocket/issues/7", "issue-acme-rocket-7.json")
        .fixture(&d, "GET", "/repos/acme/web/issues/3", "issue-acme-web-3.json")
        .fixture(&d, "GET", "/repos/acme/rocket/labels", "labels-acme-rocket.json")
        .fixture(&d, "GET", "/repos/acme/web/labels", "labels-acme-web.json")
        .fixture(&d, "GET", "/repos/acme/rocket/assignees", "assignees-acme-rocket.json")
        .on("GET", "/repos/acme/rocket/issues/404", 404, std::fs::read_to_string(d.join("not-found.json")).unwrap())
        .on("PATCH", "/repos/", 200, std::fs::read_to_string(d.join("issue-acme-rocket-12.json")).unwrap())
        .on("POST", "/comments", 201, r#"{"id": 9}"#)
        .on("POST", "/labels", 200, "[]")
        .on("DELETE", "/labels/", 200, "[]");
    r
}

fn settings() -> Settings {
    Settings { ready: true, not_ready: "sign in".into(), ..Settings::default() }
}

fn seam_with(http: Rc<Recorded>, settings: Settings, server: Option<&str>) -> Seam<GitHubIssues<Rc<Recorded>>> {
    Seam::new(GitHubIssues::new(http, server, TOKEN), settings, Rc::new(RefCell::new(Learned::default())))
}

fn world() -> (Seam<GitHubIssues<Rc<Recorded>>>, Rc<Recorded>) {
    let http = github();
    (seam_with(http.clone(), settings(), None), http)
}

/// A seam call; its answer, which never holds the token.
fn call(seam: &Seam<GitHubIssues<Rc<Recorded>>>, name: &str, payload: serde_json::Value) -> Result<String, String> {
    let answer = block_on(seam.serve(name, &payload.to_string()));
    let text = match &answer {
        Ok(t) | Err(t) => t,
    };
    assert!(!text.contains(TOKEN) && !text.contains("Bearer") && !text.contains("Authorization"), "a token in an answer: {text}");
    answer
}

fn err(answer: Result<String, String>) -> SeamError {
    SeamError::from_err(&answer.unwrap_err())
}

/// Every request went to GitHub's API, and only there with the token.
fn token_only_to_github(http: &Recorded, api: &str) {
    for s in http.sent.borrow().iter() {
        assert!(s.url.starts_with(api), "{}", s.url);
        assert!(!s.url.contains(TOKEN) && !s.body.as_deref().unwrap_or_default().contains(TOKEN), "{s:?}");
        let auth: Vec<&(String, String)> = s.headers.iter().filter(|(_, v)| v.contains(TOKEN)).collect();
        assert_eq!(auth.len(), 1, "{s:?}");
        assert_eq!(auth[0], &("Authorization".to_string(), format!("Bearer {TOKEN}")));
    }
}

#[test]
fn about_says_who_it_is_and_what_it_takes() {
    let (seam, http) = world();
    let about: fenix_tracker::About = serde_json::from_str(&call(&seam, "about", serde_json::json!({"seam": 1})).unwrap()).unwrap();
    assert_eq!((about.name.as_str(), about.ready, about.ref_hint.as_deref()), ("GitHub", true, Some("owner/repo#123")));
    assert_eq!(about.can, ["comments", "edit.title", "edit.body", "edit.assignee", "edit.labels", "comment", "create", "project"], "no priority, due date, worklog or remember");
    assert_eq!((about.id.as_str(), about.seams.clone(), about.blocked, about.browse.clone()), ("fenix.github-issues", vec![1], false, None), "no Blocked of its own; each issue carries its url");
    // Answered whatever seam it's asked with.
    let newer: fenix_tracker::About = serde_json::from_str(&call(&seam, "about", serde_json::json!({"seam": 7})).unwrap()).unwrap();
    assert_eq!(newer.seams, [1]);
    assert!(http.asked().is_empty(), "known without asking GitHub");

    let seam = seam_with(github(), Settings { ready: false, not_ready: "Sign GitHub in -- gh auth login, or its token in SPC , (Forges)".into(), ..Settings::default() }, None);
    let about: fenix_tracker::About = serde_json::from_str(&call(&seam, "about", serde_json::json!({"seam": 1})).unwrap()).unwrap();
    assert_eq!(about.not_ready.as_deref(), Some("Sign GitHub in -- gh auth login, or its token in SPC , (Forges)"));
    for c in ["assigned", "issues", "transitions", "transition", "update", "people", "comment"] {
        let e = err(call(&seam, c, serde_json::json!({"seam": 1, "ref": "acme/rocket#12", "refs": []})));
        assert_eq!((e.code.as_str(), e.message.as_str()), (code::NOT_SET_UP, "Sign GitHub in -- gh auth login, or its token in SPC , (Forges)"), "{c}");
    }
}

#[test]
fn whats_assigned_is_the_issue_search_paged_without_pull_requests() {
    let (seam, http) = world();
    let first: Page = serde_json::from_str(&call(&seam, "assigned", serde_json::json!({"seam": 1, "limit": 200})).unwrap()).unwrap();
    assert_eq!(first.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["acme/rocket#12", "acme/web#3"], "the pull request #13 left out");
    assert_eq!(first.next.as_deref(), Some("2:100"));
    assert_eq!(http.asked()[0], format!("GET {API}/user"));
    assert_eq!(http.asked()[1], format!("GET {API}/search/issues?q=is%3Aissue%20is%3Aopen%20assignee%3A%40me%20archived%3Afalse&sort=updated&order=desc&per_page=100&page=1"));

    let one = &first.issues[0];
    assert_eq!(one.url.as_deref(), Some("https://github.com/acme/rocket/issues/12"));
    assert_eq!((one.project.as_deref(), one.title.as_str()), (Some("acme/rocket"), "Login times out after five minutes"));
    assert_eq!(one.body, "Users are logged out after **5 minutes**.\r\n\r\n- [ ] find the timer\r\n- [ ] fix it\r\n", "Markdown as GitHub has it, byte for byte");
    assert_eq!(fenix_tracker::clean::text(&one.body, 10_000).0, "Users are logged out after **5 minutes**.\n\n- [ ] find the timer\n- [ ] fix it\n", "line ends as \\n to be shown");
    assert_eq!((one.state.id.as_str(), one.state.name.as_str(), one.state.stage.as_str(), one.state.column.as_str()), ("open", "Open", "todo", "todo"), "open is To do, whatever its labels");
    assert_eq!(one.id, "I_kwDOACME_ROCKET_12", "GitHub's own id, which outlives a transfer");
    assert_eq!(one.assignee.as_ref().map(|p| (p.id.as_str(), p.name.as_str())), Some(("octocat", "octocat")));
    assert_eq!((one.mine, one.labels.clone(), one.updated.as_str()), (Some(true), vec!["bug".to_string(), "in progress".to_string()], "2026-11-13T16:05:09Z"));
    assert!(one.priority.is_none() && one.due.is_none() && one.comments.is_empty());
    let web = &first.issues[1];
    assert_eq!((web.state.stage.as_str(), web.state.column.as_str()), ("todo", "todo"), "no Blocked of GitHub's own");

    // The next page, with a smaller limit asked: it follows on.
    let second: Page = serde_json::from_str(&call(&seam, "assigned", serde_json::json!({"seam": 1, "limit": 50, "cursor": "2:100"})).unwrap()).unwrap();
    assert_eq!(second.next, None);
    assert!(http.asked().last().unwrap().ends_with("&per_page=100&page=2"), "{:?}", http.asked());
    // Someone's escape codes, bidi override and newline: kept as they came,
    // to write back unchanged; shown cleaned, never run.
    assert_eq!(second.issues[0].title, "Write the \u{1b}[31mrelease notes\u{202e} for\n2.0");
    assert_eq!(fenix_tracker::clean::line(&second.issues[0].title, 200), "Write the [31mrelease notes for 2.0");
    assert_eq!(http.asked().iter().filter(|a| a.ends_with("/user")).count(), 1, "who you are, asked once");
    let e = err(call(&seam, "assigned", serde_json::json!({"seam": 1, "cursor": "not ours"})));
    assert_eq!(e.code, code::BAD_REQUEST, "a cursor that isn't its own");
    token_only_to_github(&http, API);
}

#[test]
fn these_issues_by_key_or_address_and_what_isnt_an_issue_is_missing() {
    let (seam, http) = world();
    let refs = ["acme/rocket#12", "https://github.com/acme/web/issues/3", "acme/rocket#13", "acme/rocket#404", "not a ref", "https://evil.example.com/acme/rocket/issues/12"];
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": refs})).unwrap()).unwrap();
    assert_eq!(found.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["acme/rocket#12", "acme/web#3"], "the address answered under its ref");
    assert_eq!(found.missing, ["acme/rocket#13", "acme/rocket#404", "not a ref", "https://evil.example.com/acme/rocket/issues/12"], "a pull request, a 404, no ref at all, another host: each exactly as asked");
    assert_eq!(found.issues.iter().map(|i| i.asked.as_deref()).collect::<Vec<_>>(), [Some("acme/rocket#12"), Some("https://github.com/acme/web/issues/3")]);
    let twelve = &found.issues[0];
    assert_eq!(twelve.comments.len(), 2, "it has comments, so they're asked for");
    assert_eq!((twelve.comments[0].author.as_str(), twelve.comments[0].body.as_str(), twelve.comments[0].id.as_str()), ("hubot", "I can reproduce it on 1.4.", "501"));
    assert_eq!(twelve.comments[1].body, "The session timer is in `auth/session.rs`.\r\n", "as GitHub has it");
    assert!(found.issues[1].comments.is_empty() && !http.asked().iter().any(|a| a.contains("/web/issues/3/comments")), "none to ask for");

    let closed: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["acme/rocket#7"]})).unwrap()).unwrap();
    let s = &closed.issues[0].state;
    assert_eq!((s.id.as_str(), s.name.as_str(), s.stage.as_str(), s.column.as_str()), ("closed:not_planned", "Closed as not planned", "done", "done"));
    assert!(serde_json::from_str::<Found>(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": (0..51).map(|n| format!("a/b#{}", n + 1)).collect::<Vec<_>>()})).unwrap_err()).is_err());
}

#[test]
fn a_long_thread_brings_its_most_recent_hundred_comments() {
    let d = dir();
    let http = Rc::new(Recorded::default());
    let link = r#"<https://api.github.com/repos/o/r/issues/1/comments?per_page=100&page=2>; rel="next", <https://api.github.com/repos/o/r/issues/1/comments?per_page=100&page=3>; rel="last""#;
    http.on_with("GET", "/comments?per_page=100&page=1", 200, &[("Link", link)], "[]")
        .fixture(&d, "GET", "/comments?per_page=100&page=2", "comments-long-2.json")
        .fixture(&d, "GET", "/comments?per_page=100&page=3", "comments-long-3.json");
    let gh = GitHubIssues::new(http.clone(), None, TOKEN);
    let notes = block_on(gh.notes(&IssueRef::new("o/r", 1))).unwrap();
    assert_eq!(notes.len(), 100);
    assert_eq!((notes[0].body.as_str(), notes[99].body.as_str()), ("comment 105", "comment 204"), "oldest first, the newest last");
}

#[test]
fn moves_are_close_as_completed_or_not_planned_reopen_and_doing_in_the_agenda_only() {
    let (seam, http) = world();
    let open: Moves = serde_json::from_str(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/rocket#12"})).unwrap()).unwrap();
    let rows: Vec<(String, &str, &str, bool)> = open.transitions.iter().map(|m| (m.row(), m.id.as_str(), m.to.column.as_str(), m.local)).collect();
    assert_eq!(rows, [
        ("Close as completed  ->  Closed".to_string(), "close", "done", false),
        ("Close as not planned  ->  Closed as not planned".to_string(), "close:not_planned", "done", false),
        ("Doing in the agenda only  ->  Open".to_string(), "local:doing", "doing", true),
    ]);
    assert!(!open.transitions.iter().any(|m| m.to.column == "blocked"), "GitHub has no Blocked: about says so, and no move is faked");
    assert!(http.sent.borrow().iter().all(|s| s.method == "GET"), "transitions changes nothing");

    let closed: Moves = serde_json::from_str(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/rocket#7"})).unwrap()).unwrap();
    assert_eq!(closed.transitions.iter().map(|m| (m.id.as_str(), m.to.stage.as_str())).collect::<Vec<_>>(), [("reopen", "todo")]);

    for (id, body, said) in [
        ("close", r#"{"state":"closed","state_reason":"completed"}"#, "closed as completed"),
        ("close:not_planned", r#"{"state":"closed","state_reason":"not_planned"}"#, "closed as not planned"),
        ("reopen", r#"{"state":"open","state_reason":"reopened"}"#, "reopened"),
    ] {
        let answer = call(&seam, "transition", serde_json::json!({"seam": 1, "ref": "acme/rocket#12", "id": id, "op": "agenda:1:1"})).unwrap();
        assert_eq!(answer, format!(r#"{{"said":"{said}"}}"#));
        let sent = http.last().unwrap();
        assert_eq!((sent.line(), sent.body.as_deref()), (format!("PATCH {API}/repos/acme/rocket/issues/12"), Some(body)));
    }
    let n = http.sent.borrow().len();
    assert_eq!(call(&seam, "transition", serde_json::json!({"seam": 1, "ref": "acme/rocket#12", "id": "local:doing"})).unwrap(), "{}");
    assert_eq!(http.sent.borrow().len(), n, "a local move sends nothing");
    for id in ["teleport", "label:+anything", "label:+blocked"] {
        assert_eq!(err(call(&seam, "transition", serde_json::json!({"seam": 1, "ref": "acme/rocket#12", "id": id}))).code, code::REFUSED, "{id}: no move adds a label of the caller's choosing");
    }
    assert_eq!(http.sent.borrow().len(), n);
}

#[test]
fn edits_comments_and_people_are_githubs_and_what_it_hasnt_is_unsupported() {
    let (seam, http) = world();
    for (set, body) in [
        (serde_json::json!({"title": "Shorter"}), r#"{"title":"Shorter"}"#),
        (serde_json::json!({"body": "New *body*"}), r#"{"body":"New *body*"}"#),
        (serde_json::json!({"assignee": {"id": "hubot", "name": "hubot"}}), r#"{"assignees":["hubot"]}"#),
        (serde_json::json!({"assignee": null}), r#"{"assignees":[]}"#),
        (serde_json::json!({"labels": ["bug", "needs triage"]}), r#"{"labels":["bug","needs triage"]}"#),
    ] {
        assert_eq!(call(&seam, "update", serde_json::json!({"seam": 1, "ref": "acme/rocket#12", "set": set})).unwrap(), "{}");
        let sent = http.last().unwrap();
        assert_eq!((sent.line(), sent.body.as_deref()), (format!("PATCH {API}/repos/acme/rocket/issues/12"), Some(body)));
    }
    let n = http.sent.borrow().len();
    let due = err(call(&seam, "update", serde_json::json!({"seam": 1, "ref": "acme/rocket#12", "set": {"due": "2026-12-01"}})));
    assert_eq!((due.code.as_str(), due.message.as_str()), (code::UNSUPPORTED, "GitHub issues have no due date -- it stays on the task"));
    assert_eq!(err(call(&seam, "update", serde_json::json!({"seam": 1, "ref": "acme/rocket#12", "set": {"priority": "High"}}))).code, code::UNSUPPORTED);
    assert_eq!(http.sent.borrow().len(), n, "nothing sent for either");
    for c in ["priorities", "worklog", "remember", "create"] {
        assert_eq!(err(call(&seam, c, serde_json::json!({"seam": 1, "ref": "acme/rocket#12", "remember": "x", "seconds": 60, "started": "2026-11-12T09:00:00Z", "title": "t"}))).code, code::UNSUPPORTED, "{c}");
    }

    assert_eq!(call(&seam, "comment", serde_json::json!({"seam": 1, "ref": "acme/rocket#12", "body": "Fixed in #20."})).unwrap(), "{}");
    assert_eq!((http.last().unwrap().line(), http.last().unwrap().body), (format!("POST {API}/repos/acme/rocket/issues/12/comments"), Some(r#"{"body":"Fixed in #20."}"#.to_string())));
    let people: fenix_tracker::People = serde_json::from_str(&call(&seam, "people", serde_json::json!({"seam": 1, "ref": "acme/rocket#12"})).unwrap()).unwrap();
    assert_eq!(people.people.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["octocat", "hubot"]);
    token_only_to_github(&http, API);
}

#[test]
fn errors_are_the_seams_with_githubs_words() {
    let d = dir();
    let read = |n: &str| std::fs::read_to_string(d.join(n)).unwrap();
    let (seam, http) = world();
    http.on("GET", "/repos/acme/rocket/issues/12", 401, read("bad-credentials.json"));
    let e = err(call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/rocket#12"})));
    assert_eq!((e.code.as_str(), e.message.as_str()), (code::SIGNED_OUT, "HTTP 401 (Unauthorized): Bad credentials"));

    let e = err(call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/rocket#404"})));
    assert_eq!((e.code.as_str(), e.message.as_str()), (code::NOT_FOUND, "HTTP 404 (Not Found): Not Found"));

    let date = ("Date", "Sat, 14 Nov 2026 10:00:00 GMT");
    http.on_with("GET", "/repos/acme/web/issues/3", 403, &[date, ("x-ratelimit-limit", "5000"), ("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "1794650700")], read("rate-limited.json"));
    let e = err(call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["acme/web#3"]})));
    assert_eq!((e.code.as_str(), e.retry_after), (code::RATE_LIMITED, Some(300)), "primary: until the reset");
    assert!(e.message.starts_with("HTTP 403 (Forbidden): API rate limit exceeded for user ID 1."));

    http.on_with("POST", "/comments", 403, &[date, ("retry-after", "120")], read("secondary-limit.json"));
    let e = err(call(&seam, "comment", serde_json::json!({"seam": 1, "ref": "acme/rocket#7", "body": "x"})));
    assert_eq!((e.code.as_str(), e.retry_after), (code::RATE_LIMITED, Some(120)), "secondary: Retry-After");

    http.on("PATCH", "/repos/", 422, read("validation-failed.json"));
    let e = err(call(&seam, "update", serde_json::json!({"seam": 1, "ref": "acme/rocket#7", "set": {"assignee": {"id": "nobody", "name": "nobody"}}})));
    assert_eq!((e.code.as_str(), e.message.as_str()), (code::REFUSED, "HTTP 422 (Unprocessable Entity): Validation Failed"));

    http.on("GET", "/repos/acme/rocket/issues/7", 502, "<html>Bad gateway</html>");
    let e = err(call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/rocket#7"})));
    assert_eq!((e.code.as_str(), e.message.as_str()), (code::OFFLINE, "HTTP 502 (Bad Gateway)"), "a proxy or maintenance: the next sync tries again");
    http.on("GET", "/repos/acme/rocket/issues/7", 500, "");
    assert_eq!(err(call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/rocket#7"}))).code, code::FAILED);
    http.on("GET", "/repos/acme/rocket/issues/7", 410, r#"{"message": "This issue was deleted"}"#);
    assert_eq!(err(call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/rocket#7"}))).code, code::NOT_FOUND);

    let e = err(call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "acme/nowhere#1"})));
    assert_eq!(e.code, code::OFFLINE, "nothing came back: {e:?}");
    let e = err(call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "nope here"})));
    assert_eq!((e.code.as_str(), e.message.as_str()), (code::BAD_REQUEST, "\"nope here\" isn't an issue -- owner/repo#12, its id, or the issue's address"));
    assert_eq!(err(call(&seam, "issues", serde_json::json!({"seam": 2, "refs": []}))).code, code::UNSUPPORTED);
    let e = err(call(&seam, "update", serde_json::json!({"seam": 1, "ref": "acme/rocket#7", "set": {"colour": "red"}})));
    assert_eq!(e.code, code::UNSUPPORTED, "a field it doesn't know");
    assert_eq!(SeamError::from_err(&block_on(seam.serve("issues", "not json")).unwrap_err()).code, code::BAD_REQUEST);
    token_only_to_github(&http, API);
}

#[test]
fn an_enterprise_server_is_asked_at_its_own_api() {
    let d = dir();
    let http = Rc::new(Recorded::default());
    http.fixture(&d, "GET", "/api/v3/repos/platform/tools/issues/5", "ghe-issue-platform-tools-5.json").on("GET", "/api/v3/user", 200, r#"{"login": "octocat"}"#);
    let seam = seam_with(http.clone(), settings(), Some("https://ghe.example.com"));
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["https://ghe.example.com/platform/tools/issues/5", "https://github.com/platform/tools/issues/5"]})).unwrap()).unwrap();
    assert_eq!(found.issues[0].key, "platform/tools#5");
    assert_eq!(found.issues[0].url.as_deref(), Some("https://ghe.example.com/platform/tools/issues/5"));
    assert_eq!(found.missing, ["https://github.com/platform/tools/issues/5"], "github.com's address isn't this server's");
    token_only_to_github(&http, "https://ghe.example.com/api/v3/");
}

#[test]
fn the_page_asks_for_searches_linked_requests_templates_and_a_new_issue() {
    let d = dir();
    let http = github();
    let gh = GitHubIssues::new(http.clone(), None, TOKEN);
    let page = Paging::first(30);
    for (search, q) in [
        (Search::Created, "q=is%3Aissue%20is%3Aopen%20author%3A%40me%20archived%3Afalse"),
        (Search::Mentioned, "q=is%3Aissue%20is%3Aopen%20mentions%3A%40me%20archived%3Afalse"),
        (Search::Query("label:bug repo:acme/rocket".into()), "q=is%3Aissue%20label%3Abug%20repo%3Aacme%2Frocket"),
        (Search::Query("is:pr author:@me".into()), "q=is%3Apr%20author%3A%40me&"),
    ] {
        let _ = block_on(gh.search(&search, &page));
        assert!(http.asked().last().unwrap().contains(q), "{search:?}: {:?}", http.asked().last());
    }
    http.on("GET", "/repos/acme/rocket/issues?state=open", 200, std::fs::read_to_string(d.join("search-assigned-1.json")).map(|s| serde_json::from_str::<serde_json::Value>(&s).unwrap()["items"].to_string()).unwrap());
    let listing = block_on(gh.search(&Search::Project("acme/rocket".into()), &page)).unwrap();
    assert_eq!(listing.issues.len(), 2, "pull requests left out of a repository's issues too");
    assert_eq!(http.asked().last().unwrap(), &format!("GET {API}/repos/acme/rocket/issues?state=open&sort=updated&direction=desc&per_page=30&page=1"));

    let linked = block_on(gh.linked(&IssueRef::new("acme/rocket", 12))).unwrap();
    assert_eq!(linked.iter().map(|l| (l.reference.as_str(), l.state.as_str())).collect::<Vec<_>>(), [("acme/rocket#20", "merged")], "a pull request, not the issue that mentions it");

    http.fixture(&d, "GET", "/repos/acme/rocket/contents/.github/ISSUE_TEMPLATE", "templates-acme-rocket.json").fixture(&d, "GET", "/contents/.github/ISSUE_TEMPLATE/bug_report.md", "template-bug_report.md");
    let templates = block_on(gh.templates("acme/rocket")).unwrap();
    assert_eq!(templates.len(), 1, "Markdown templates; an issue form's YAML isn't one");
    assert_eq!((templates[0].name.as_str(), templates[0].title.as_str(), templates[0].labels.clone()), ("Bug report", "[Bug] ", vec!["bug".to_string(), "triage".to_string()]));
    let raw = http.sent.borrow().last().cloned().unwrap();
    assert!(raw.headers.contains(&("Accept".to_string(), "application/vnd.github.raw+json".to_string())));
    assert!(block_on(gh.templates("acme/web")).unwrap_or_default().is_empty() || http.asked().last().unwrap().contains("acme/web"));

    http.on("POST", "/repos/acme/rocket/issues", 201, std::fs::read_to_string(d.join("issue-acme-rocket-12.json")).unwrap());
    let made = block_on(gh.create(&NewIssue { project: "acme/rocket".into(), title: "T".into(), body: "B".into(), labels: vec!["bug".into()], assignees: vec!["octocat".into()] })).unwrap();
    assert_eq!(made.key.key(), "acme/rocket#12");
    assert_eq!(http.last().unwrap().body.as_deref(), Some(r#"{"assignees":["octocat"],"body":"B","labels":["bug"],"title":"T"}"#));
    block_on(gh.change(&IssueRef::new("acme/rocket", 12), &Change::Labels(vec!["a".into(), "b".into()]))).unwrap();
    assert_eq!(http.last().unwrap().body.as_deref(), Some(r#"{"labels":["a","b"]}"#));
    assert_eq!(block_on(gh.change(&IssueRef::new("acme/rocket", 12), &Change::Due(None))).unwrap_err().message, "GitHub issues have no due date");
    // A label that isn't on it: nothing to take off.
    http.on("DELETE", "/labels/", 404, "{}");
    block_on(gh.change(&IssueRef::new("acme/rocket", 12), &Change::RemoveLabel("in progress".into()))).unwrap();
    assert!(http.asked().last().unwrap().ends_with("/labels/in%20progress"));
    token_only_to_github(&http, API);
}

#[test]
fn an_issue_asked_by_its_id_is_found_where_it_lives_now() {
    let (seam, http) = world();
    // It was acme/web#3; transferred, it's acme/rocket#12 now.
    http.on("POST", "api.github.com/graphql", 200, r#"{"data": {"node": {"number": 12, "repository": {"nameWithOwner": "acme/rocket"}}}}"#);
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["I_kwDOACME_ROCKET_12"]})).unwrap()).unwrap();
    assert_eq!((found.issues[0].key.as_str(), found.issues[0].asked.as_deref(), found.issues[0].id.as_str()), ("acme/rocket#12", Some("I_kwDOACME_ROCKET_12"), "I_kwDOACME_ROCKET_12"));
    let ask = http.sent.borrow().iter().find(|s| s.url.ends_with("/graphql")).cloned().unwrap();
    assert_eq!(ask.body.as_deref(), Some(r#"{"query":"query($id:ID!){node(id:$id){... on Issue{number repository{nameWithOwner}}}}","variables":{"id":"I_kwDOACME_ROCKET_12"}}"#));
    http.on("POST", "api.github.com/graphql", 200, r#"{"data": {"node": null}, "errors": [{"type": "NOT_FOUND", "message": "Could not resolve to a node with the global id of 'I_gone'"}]}"#);
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["I_gone"]})).unwrap()).unwrap();
    assert_eq!(found.missing, ["I_gone"]);
    token_only_to_github(&http, API);
}
