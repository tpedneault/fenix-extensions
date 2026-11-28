//! A request that's open, edited: what each forge is sent to change its
//! title, description, target, draft state, labels and assignees -- and
//! that only what changed is sent where a forge needs a call for each.

use std::rc::Rc;

use fenix_forge::github::GitHub;
use fenix_forge::gitlab::GitLab;
use fenix_forge::recorded::Recorded;
use fenix_forge::{DiffRefs, Forge, MergeRequest, MrState, NewRequest};
use futures::executor::block_on;
use serde_json::Value;

fn open(number: u64, draft: bool, url: &str) -> MergeRequest {
    MergeRequest {
        number,
        title: if draft { "Draft: Status page".into() } else { "Status page".into() },
        description: "before".into(),
        state: MrState::Open,
        draft,
        source_branch: "feature/status".into(),
        target_branch: "main".into(),
        author: "kim".into(),
        web_url: url.into(),
        has_conflicts: false,
        sha: "abc".into(),
        diff_refs: DiffRefs::default(),
        comment_count: 0,
        pipeline: None,
        updated_at: String::new(),
        assignees: vec!["kim".into()],
        labels: vec!["ui".into()],
    }
}

fn edit(draft: bool) -> NewRequest {
    NewRequest {
        source_branch: "feature/status".into(),
        target_branch: "release".into(),
        title: "Status page, live".into(),
        description: "after".into(),
        draft,
        labels: vec!["ui".into(), "git".into()],
        assignees: vec!["sam".into()],
    }
}

fn body(sent: &fenix_forge::recorded::Sent) -> Value {
    serde_json::from_str(sent.body.as_deref().unwrap_or("null")).unwrap()
}

#[test]
fn gitlab_changes_it_in_one_call_its_draft_in_the_title() {
    let http = Rc::new(Recorded::default());
    http.on("GET", "/users?username=sam", 200, r#"[{"id": 7, "username": "sam"}]"#)
        .on("PUT", "/merge_requests/42", 200, r#"{"iid": 42, "title": "Draft: Status page, live", "draft": true, "labels": ["ui", "git"], "assignees": [{"username": "sam"}], "web_url": "https://gitlab.example.com/acme/api/-/merge_requests/42"}"#);
    let gitlab = GitLab::new(http.clone(), "https://gitlab.example.com", "token", "acme/api");
    let made = block_on(gitlab.update_request(&open(42, false, "https://gitlab.example.com/acme/api/-/merge_requests/42"), &edit(true))).unwrap();
    let put = http.sent.borrow().iter().find(|s| s.method == "PUT").cloned().expect("a PUT");
    let sent = body(&put);
    assert_eq!(sent["title"], "Draft: Status page, live");
    assert_eq!((sent["description"].as_str(), sent["target_branch"].as_str(), sent["labels"].as_str()), (Some("after"), Some("release"), Some("ui,git")));
    assert_eq!(sent["assignee_ids"], serde_json::json!([7]));
    assert!(made.draft);
    assert_eq!(made.plain_title(), "Status page, live", "the draft marker isn't part of the title edited");
    assert_eq!((made.assignees, made.labels), (vec!["sam".to_string()], vec!["ui".to_string(), "git".to_string()]));
}

#[test]
fn github_patches_it_moves_its_draft_by_graphql_and_sends_only_what_changed() {
    let http = Rc::new(Recorded::default());
    http.on("PATCH", "/pulls/7", 200, r#"{"number": 7, "title": "Status page, live", "body": "after", "draft": false, "html_url": "https://github.com/acme/api/pull/7", "head": {"ref": "feature/status"}, "base": {"ref": "release"}}"#)
        .on("GET", "/pulls/7", 200, r#"{"number": 7, "node_id": "PR_n7"}"#)
        .on("POST", "/graphql", 200, r#"{"data": {"convertPullRequestToDraft": {"clientMutationId": null}}}"#)
        .on("PUT", "/issues/7/labels", 200, "[]")
        .on("DELETE", "/issues/7/assignees", 200, "{}")
        .on("POST", "/issues/7/assignees", 201, "{}");
    let github = GitHub::new(http.clone(), "token", "acme", "api");
    let made = block_on(github.update_request(&open(7, false, "https://github.com/acme/api/pull/7"), &edit(true))).unwrap();
    let asked = http.asked();
    assert!(asked.iter().any(|a| a.starts_with("PATCH ") && a.ends_with("/pulls/7")), "{asked:?}");
    let graphql = http.sent.borrow().iter().find(|s| s.url.ends_with("/graphql")).cloned().expect("the draft moved by GraphQL");
    assert!(body(&graphql)["query"].as_str().unwrap().contains("convertPullRequestToDraft"));
    assert_eq!(body(&graphql)["variables"]["id"], "PR_n7");
    let removed = http.sent.borrow().iter().find(|s| s.method == "DELETE").cloned().unwrap();
    assert_eq!(body(&removed)["assignees"], serde_json::json!(["kim"]));
    let added = http.sent.borrow().iter().find(|s| s.method == "POST" && s.url.ends_with("/assignees")).cloned().unwrap();
    assert_eq!(body(&added)["assignees"], serde_json::json!(["sam"]));
    assert!(made.draft);
    assert_eq!(made.assignees, vec!["sam".to_string()]);

    // Nothing of the draft, labels or assignees changed: one PATCH.
    let http = Rc::new(Recorded::default());
    http.on("PATCH", "/pulls/7", 200, r#"{"number": 7, "title": "t", "html_url": "https://github.com/acme/api/pull/7", "head": {"ref": "feature/status"}, "base": {"ref": "main"}}"#);
    let github = GitHub::new(http.clone(), "token", "acme", "api");
    let before = open(7, false, "https://github.com/acme/api/pull/7");
    let same = NewRequest { source_branch: before.source_branch.clone(), target_branch: "main".into(), title: "t".into(), description: "d".into(), draft: false, labels: before.labels.clone(), assignees: before.assignees.clone() };
    block_on(github.update_request(&before, &same)).unwrap();
    assert_eq!(http.asked().len(), 1, "{:?}", http.asked());
}
