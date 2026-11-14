//! Jira Server and Data Center ask exactly what they asked before Jira
//! Cloud was added: every request the client and the seam make, with its
//! body, for a script of every call, compared with
//! `fixtures/server-requests.txt` -- written by the client before Cloud
//! (Jira client 1.1.0 after the seam's review) and left alone since.
//! `UPDATE_SERVER_REQUESTS=1` writes it again, which should only ever be
//! done on purpose.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use fenix_jira_client::recorded::Recorded;
use fenix_jira_client::tracker::{Keys, Learned, Seam, Settings};
use fenix_jira_client::{JiraClient, NewIssue};
use futures_executor::block_on;

fn fixtures() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

fn settings() -> Settings {
    Settings {
        base_url: Some("https://jira.example.com/".into()),
        projects: vec!["PROJ".into()],
        users: vec![("jo1".into(), "Jo Bloggs".into())],
        priorities: vec![],
        blocked: vec![("PROJ".into(), "10103: On Hold".into()), ("OPS".into(), "flag".into())],
        sync_minutes: 10,
        keys: Keys::default(),
    }
}

/// Every request a script of every call makes, one a line.
fn script() -> String {
    let jira = Rc::new(Recorded::jira_server(fixtures()));
    let client = JiraClient::new(jira.clone());
    let seam = Seam::new(client.clone(), settings(), Rc::new(RefCell::new(Learned::default())));
    let calls: [(&str, serde_json::Value); 20] = [
        ("assigned", serde_json::json!({"seam": 1, "limit": 2})),
        ("assigned", serde_json::json!({"seam": 1, "limit": 2, "cursor": "2"})),
        ("issues", serde_json::json!({"seam": 1, "refs": ["PROJ-1", "OPS-7"]})),
        ("issues", serde_json::json!({"seam": 1, "refs": ["https://jira.example.com/browse/PROJ-1"]})),
        ("transitions", serde_json::json!({"seam": 1, "ref": "PROJ-1"})),
        ("transitions", serde_json::json!({"seam": 1, "ref": "PROJ-1", "toward": "blocked", "from": {"id": "3", "name": "In Progress", "stage": "doing", "column": "doing"}})),
        ("transition", serde_json::json!({"seam": 1, "ref": "PROJ-1", "id": "41"})),
        ("transition", serde_json::json!({"seam": 1, "ref": "OPS-7", "id": "unflag+21"})),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"title": "New"}})),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"body": "Line one\nline two"}})),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"priority": "Minor"}})),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"due": "2026-12-01"}})),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"assignee": {"id": "jo1", "name": "Jo"}}})),
        ("priorities", serde_json::json!({"seam": 1, "ref": "PROJ-1"})),
        ("comment", serde_json::json!({"seam": 1, "ref": "PROJ-1", "body": "Done.\nReally."})),
        ("worklog", serde_json::json!({"seam": 1, "ref": "PROJ-1", "seconds": 4500, "started": "2026-11-12T09:15:00.000+01:00"})),
        // What the seam's review added: ids, unassigning, labels, op (kept
        // once by the extension, not here).
        ("issues", serde_json::json!({"seam": 1, "refs": ["10201", "PROJ-2", "OPS-7"]})),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"assignee": null}, "op": "agenda:1:1"})),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"labels": ["auth", "web"]}, "op": "agenda:1:2"})),
        ("comment", serde_json::json!({"seam": 1, "ref": "PROJ-1", "body": "With an op.", "op": "agenda:1:3"})),
    ];
    for (name, payload) in calls {
        let _ = block_on(seam.serve(name, &payload.to_string()));
    }
    // The page's own calls.
    block_on(async {
        let _ = client.search_issues("project = PROJ ORDER BY updated DESC", 100).await;
        let _ = client.get_issue("PROJ-1").await;
        let _ = client.list_transitions("PROJ-1").await;
        let _ = client.apply_transition("PROJ-1", "21").await;
        let _ = client.update_summary("PROJ-1", "T").await;
        let _ = client.update_description("PROJ-1", "D").await;
        let _ = client.update_due("PROJ-1", None).await;
        let _ = client.update_priority("PROJ-1", "High").await;
        let _ = client.update_assignee("PROJ-1", "kim").await;
        let _ = client.add_comment("PROJ-1", "c").await;
        let _ = client.add_worklog("PROJ-1", "1h 30m").await;
        let _ = client.add_worklog_at("PROJ-1", 60, "2026-11-12T09:15:00.000+0100").await;
        let _ = client.list_priorities().await;
        let _ = client.myself().await;
        let _ = client.list_project_statuses("PROJ").await;
        let _ = client.find_flagged_field().await;
        let _ = client.set_flagged("PROJ-1", "customfield_10000", true).await;
        let _ = client.issue_types("PROJ").await;
        let _ = client.create_fields("PROJ", "10004").await;
        let _ = client.create_issue("PROJ", "Task", "S").await;
        let issue = NewIssue {
            project: "PROJ".into(),
            issue_type_id: "10004".into(),
            summary: "S".into(),
            description: "D\nE".into(),
            priority: Some("Major".into()),
            assignee: Some("kim".into()),
            extra: vec![("components".into(), serde_json::json!([{"id": "10"}]))],
        };
        let _ = client.create(&issue).await;
    });
    let sent = jira.sent.borrow();
    sent.iter().map(|(request, body)| format!("{request}\t{}\n", body.as_deref().unwrap_or("-"))).collect()
}

#[test]
fn server_and_data_center_ask_what_they_asked_before() {
    let golden = fixtures().join("server-requests.txt");
    let now = script();
    if std::env::var_os("UPDATE_SERVER_REQUESTS").is_some() {
        std::fs::write(&golden, &now).unwrap();
    }
    let before = std::fs::read_to_string(&golden).unwrap();
    for (i, (was, is)) in before.lines().zip(now.lines()).enumerate() {
        assert_eq!(was, is, "request {i}");
    }
    assert_eq!(before.lines().count(), now.lines().count());
}
