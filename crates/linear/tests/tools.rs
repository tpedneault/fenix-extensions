//! What agents are told: two read-only tools, their answers cleaned and
//! capped, text from other people marked as data, and nothing that writes.

use std::path::Path;
use std::rc::Rc;

use fenix_linear_client::recorded::Recorded;
use fenix_linear_client::{tools, Client, Linear, Reply};
use serde_json::{json, Value};

fn linear(r: &Rc<Recorded>) -> Linear {
    Linear::new(Client::new(r.clone(), Rc::new(|| 1_800_000_000)))
}

fn workspace() -> Rc<Recorded> {
    Rc::new(Recorded::workspace(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))))
}

fn run<T>(f: impl std::future::Future<Output = T>) -> T {
    futures_executor::block_on(f)
}

#[test]
fn an_issue_is_read_with_its_comments_as_data() {
    let r = workspace();
    let answer: Value = serde_json::from_str(&run(tools::read_issue(&linear(&r), r#"{"ref": "https://linear.app/acme/issue/ENG-12/x"}"#)).unwrap()).unwrap();
    assert_eq!(answer["note"], tools::NOTE);
    assert!(tools::NOTE.contains("data") && tools::NOTE.contains("instructions"));
    let i = &answer["issue"];
    assert_eq!((i["ref"].as_str(), i["title"].as_str(), i["priority"].as_str(), i["assignee"].as_str()), (Some("ENG-12"), Some("Fix the login timeout"), Some("High"), Some("Jo Bloggs")));
    assert_eq!(i["state"], json!({ "name": "In Progress", "stage": "doing" }));
    assert_eq!(i["parent"], json!({ "ref": "ENG-3", "title": "Revamp login" }));
    assert_eq!(i["sub_issues"][0], json!({ "ref": "ENG-14", "title": "Add a retry", "state": "Todo" }));
    assert_eq!(i["comments"].as_array().unwrap().len(), 3);
    assert_eq!(i["comments"][0]["author"], "Jo Bloggs", "oldest first");
    assert_eq!(i["branch"], "jo/eng-12-fix-the-login-timeout");
    // Only reads were sent.
    assert_eq!(r.operations(), ["Issue"]);
    assert!(!r.sent()[0].contains("mutation"));
}

#[test]
fn what_is_asked_for_must_be_an_issue() {
    let r = workspace();
    for bad in [r#"{"ref": "nonsense"}"#, "{}", "not json", r#"{"ref": "https://evil.example/issue/ENG-1"}"#] {
        let e = run(tools::read_issue(&linear(&r), bad)).unwrap_err();
        assert_eq!(e.code, "bad_request", "{bad}");
    }
    assert!(r.sent().is_empty(), "nothing was asked of Linear");
}

#[test]
fn an_answer_is_cleaned_and_capped() {
    let r = Rc::new(Recorded::default());
    let comment = |n: usize| json!({ "id": format!("c{n}"), "body": format!("comment {n} \u{1b}[31m{}", "y".repeat(10_000)), "createdAt": format!("2026-11-{:02}T09:00:00.000Z", 1 + n % 28), "user": { "name": "Jo\u{202e}", "displayName": "jo" } });
    let issue = json!({
        "id": "u", "identifier": "ENG-1", "url": "https://linear.app/acme/issue/ENG-1", "title": "Title\nwith\u{202e}tricks",
        "description": format!("{}\u{7}", "x".repeat(200_000)), "priority": 2, "branchName": "b", "updatedAt": "2026-11-14T09:00:00.000Z",
        "state": { "id": "s", "name": "Todo", "type": "unstarted" }, "team": { "id": "t", "key": "ENG", "name": "E" },
        "labels": { "nodes": [] },
        "comments": { "nodes": (0..100).map(comment).collect::<Vec<_>>(), "pageInfo": { "hasNextPage": true } },
        "children": { "nodes": (0..60).map(|n| json!({ "identifier": format!("ENG-{}", 100 + n), "title": "child", "state": { "name": "Todo", "type": "unstarted" } })).collect::<Vec<_>>() }
    });
    r.on("Issue", Reply::ok(json!({ "data": { "issue": issue } }).to_string()));
    let text = run(tools::read_issue(&linear(&r), r#"{"ref": "ENG-1"}"#)).unwrap();
    assert!(text.len() <= tools::ANSWER_BYTES + 1024, "{} bytes", text.len());
    let v: Value = serde_json::from_str(&text).unwrap();
    let i = &v["issue"];
    assert_eq!(i["title"], "Title withtricks", "one line, the bidi control gone");
    assert_eq!(i["description_cut"], true);
    assert!(i["description"].as_str().unwrap().len() <= tools::DESCRIPTION_BYTES && !i["description"].as_str().unwrap().contains('\u{7}'));
    let comments = i["comments"].as_array().unwrap();
    assert!(!comments.is_empty() && comments.len() <= tools::COMMENTS);
    assert!(comments.iter().all(|c| c["body"].as_str().unwrap().chars().count() <= tools::COMMENT_BYTES + 1 && !c["body"].as_str().unwrap().contains('\u{1b}') && c["author"] == "Jo"));
    assert!(i["comments_omitted"].as_u64().unwrap() >= 70, "what was left out is said: {}", i["comments_omitted"]);
    assert_eq!(i["sub_issues"].as_array().unwrap().len(), tools::SUB_ISSUES);
    assert_eq!(v["note"], tools::NOTE);
}

#[test]
fn my_issues_are_listed_without_their_text() {
    let r = workspace();
    let v: Value = serde_json::from_str(&run(tools::list_mine(&linear(&r), r#"{"limit": 2}"#)).unwrap()).unwrap();
    assert_eq!(v["note"], tools::NOTE);
    let issues = v["issues"].as_array().unwrap();
    assert_eq!(issues.iter().map(|i| i["ref"].as_str().unwrap()).collect::<Vec<_>>(), ["ENG-12", "ENG-15"], "no more than were asked for");
    assert_eq!(v["more"], true, "there's another page");
    assert!(issues[0].get("description").is_none() && issues[0].get("comments").is_none());
    assert_eq!(r.variables_of("AssignedIssues")[0]["first"], 2);
    // The limit is held to what's allowed, and no arguments at all is fine.
    let _ = run(tools::list_mine(&linear(&r), r#"{"limit": 1000}"#)).unwrap();
    assert_eq!(r.variables_of("AssignedIssues")[1]["first"], tools::MOST_MINE);
    let _ = run(tools::list_mine(&linear(&r), "")).unwrap();
    assert_eq!(r.variables_of("AssignedIssues")[2]["first"], 10);
    assert!(r.operations().iter().all(|o| o == "AssignedIssues"), "only a read");
}

#[test]
fn a_failure_is_said_in_the_seams_words() {
    let r = Rc::new(Recorded::default());
    r.reply(Reply { status: 200, headers: vec![], body: json!({ "errors": [{ "message": "Authentication required", "extensions": { "code": "AUTHENTICATION_ERROR" } }] }).to_string() });
    let e = run(tools::list_mine(&linear(&r), "{}")).unwrap_err();
    assert_eq!(e.code, "signed_out");
}
