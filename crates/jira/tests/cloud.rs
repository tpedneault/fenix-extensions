//! The tracker seam served for Jira Cloud, on responses written from the
//! shapes Atlassian documents (`fixtures/cloud`, not recordings: no Cloud
//! site was reachable): which Jira it is, each call and what it asks,
//! the enhanced search's paging, account ids, rich text read and written,
//! errors with what a 429 said to wait, and no token in any answer.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use fenix_jira_client::recorded::Recorded;
use fenix_jira_client::tracker::{about_for, Keys, Learned, Seam, Settings};
use fenix_jira_client::{Deployment, JiraClient, NewIssue};
use fenix_tracker::{can, code, Found, Moves, Page, SeamError};
use futures_executor::block_on;

const TOKEN: &str = "ATATT3xFfGF0-cloud-secret";

fn fixtures() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/cloud"))
}

fn settings() -> Settings {
    Settings {
        base_url: Some("https://acme.atlassian.net/".into()),
        projects: vec!["PROJ".into()],
        users: vec![],
        priorities: vec![("Low".into(), "Medium".into())],
        blocked: vec![("PROJ".into(), "10103: On Hold".into()), ("OPS".into(), "flag".into())],
        sync_minutes: 10,
        keys: Keys::default(),
    }
}

fn seam() -> (Seam, Rc<Recorded>) {
    let jira = Rc::new(Recorded::jira_cloud(fixtures()));
    (Seam::new(JiraClient::cloud(jira.clone()), settings(), Rc::new(RefCell::new(Learned::default()))), jira)
}

fn call(seam: &Seam, name: &str, payload: serde_json::Value) -> Result<String, String> {
    let answer = block_on(seam.serve(name, &payload.to_string()));
    let text = match &answer {
        Ok(t) | Err(t) => t,
    };
    assert!(!text.contains(TOKEN) && !text.contains("Basic ") && !text.contains("Bearer"), "a token in an answer: {text}");
    answer
}

#[test]
fn server_info_says_its_cloud() {
    let jira = Rc::new(Recorded::jira_cloud(fixtures()));
    // Asked Server's way, as a site on its own domain would be at first.
    let said = block_on(JiraClient::new(jira.clone()).server_info()).unwrap();
    assert_eq!(said, (Deployment::Cloud, "Cloud".to_string()));
    assert_eq!(jira.asked(), ["GET /rest/api/2/serverInfo"]);
}

#[test]
fn about_leaves_out_edit_and_names_the_email() {
    let a = about_for(&settings(), true, Deployment::Cloud);
    assert!(!a.can(can::EDIT), "a description written back from text would lose its rich text");
    for c in [can::COMMENTS, can::PRIORITIES, can::ASSIGN, can::COMMENT, can::WORKLOG, can::CREATE, can::REMEMBER] {
        assert!(a.can(c), "{c}");
    }
    assert_eq!(a.browse.as_deref(), Some("https://acme.atlassian.net/browse/{ref}"));
    assert_eq!(a.name, "Jira");
    let not = about_for(&settings(), false, Deployment::Cloud);
    assert_eq!(not.not_ready.as_deref(), Some("Jira Cloud isn't set up yet -- set its site, your email and an API token in SPC , (Jira)"));
    assert_eq!(about_for(&settings(), true, Deployment::Server).can.len(), can::ALL.len(), "Server keeps all eight");
}

#[test]
fn linked_issues_come_from_the_enhanced_search_their_rich_text_as_text() {
    let (seam, jira) = seam();
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["PROJ-1", "PROJ-2", "OPS-7", "PROJ-404"]})).unwrap()).unwrap();
    assert_eq!(found.missing, ["PROJ-404"]);
    let [one, two, ops] = &found.issues[..] else { panic!("{found:?}") };

    assert_eq!(one.url.as_deref(), Some("https://acme.atlassian.net/browse/PROJ-1"));
    assert_eq!(
        one.body,
        "Users are logged out after 5 minutes.\n\nSteps\n\n1. Log in\n2. Wait\n\n```text\nsession expired: idle 300s\n```\n\n@Sam Lee saw it on staging (https://staging.example.com) :warning:\n\n[attachment: logout.png]"
    );
    assert!(!one.body_cut);
    assert_eq!(one.assignee.as_ref().map(|p| (p.id.as_str(), p.name.as_str())), Some(("5b10ac8d82e05b22cc7d4ef5", "Jo Bloggs")), "an account id");
    assert_eq!(one.mine, Some(true), "the account id /myself gave");
    assert_eq!(one.comments.iter().map(|c| c.body.as_str()).collect::<Vec<_>>(), ["Seen on staging too.", "Looking into it, @Sam Lee.\n\n- cookie TTL\n- idle timer"]);
    assert_eq!((one.state.stage.as_str(), one.state.column.as_str()), ("doing", "doing"));
    assert_eq!(one.updated, "2026-11-13T16:05:09.402+0100");

    assert_eq!((two.body.as_str(), two.state.column.as_str()), ("", "blocked"));
    assert_eq!(two.priority.as_ref().map(|p| p.level.as_str()), Some("medium"), "jira.priorities");
    assert_eq!((ops.mine, ops.state.column.as_str(), ops.body.as_str()), (Some(false), "blocked", "Expires on the 2026-11-30."));

    let asked = jira.asked();
    assert_eq!(asked[0], "GET /rest/api/2/myself");
    assert_eq!(asked[1], "GET /rest/api/2/field");
    assert!(
        asked[2].starts_with("GET /rest/api/3/search/jql?jql=key%20IN%20%28%22PROJ-1%22%2C%22PROJ-2%22%2C%22OPS-7%22%2C%22PROJ-404%22%29&maxResults=4&fields=summary%2Cdescription%2C"),
        "{asked:?}"
    );
    assert!(asked[2].ends_with("%2Ccustomfield_10021"), "{asked:?}");
    assert_eq!(asked.len(), 3, "isLast: no second page");

    // One alone: v3's issue, by its address too.
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["https://acme.atlassian.net/jira/software/projects/PROJ/boards/1?selectedIssue=PROJ-1"]})).unwrap()).unwrap();
    assert_eq!((found.issues[0].key.as_str(), found.missing.len()), ("PROJ-1", 0));
    assert!(jira.asked()[3].starts_with("GET /rest/api/3/issue/PROJ-1?fields="), "{:?}", jira.asked());
    let err = SeamError::from_err(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["NOPE-1"]})).unwrap_err());
    assert_eq!((err.code.as_str(), err.message.as_str()), (code::NOT_FOUND, "HTTP 404 (Issue does not exist or you do not have permission to see it.)"));
}

#[test]
fn whats_assigned_pages_by_the_searchs_token() {
    let (seam, jira) = seam();
    let first: Page = serde_json::from_str(&call(&seam, "assigned", serde_json::json!({"seam": 1, "limit": 2})).unwrap()).unwrap();
    assert_eq!(first.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["PROJ-1", "PROJ-2"]);
    let next = first.next.clone().unwrap();
    assert_eq!(next, "Ch0jU3RyaW5nJlVGSlBTZz09JUludCZOakF3", "Jira's own token, as it gave it");
    let search = jira.asked().into_iter().find(|a| a.contains("search")).unwrap();
    assert!(
        search.starts_with("GET /rest/api/3/search/jql?jql=assignee%20%3D%20currentUser%28%29%20AND%20statusCategory%20%21%3D%20Done%20AND%20project%20IN%20%28PROJ%29%20ORDER%20BY%20updated%20DESC&maxResults=2&fields="),
        "{search}"
    );
    let second: Page = serde_json::from_str(&call(&seam, "assigned", serde_json::json!({"seam": 1, "limit": 2, "cursor": next})).unwrap()).unwrap();
    assert_eq!(second.next, None, "isLast");
    assert_eq!(second.issues[0].title, "Write the [31mrelease notes for 2.0", "cleaned");
    assert!(jira.asked().last().unwrap().contains("&nextPageToken=Ch0jU3RyaW5nJlVGSlBTZz09JUludCZOakF3&maxResults=2&"), "{:?}", jira.asked());

    call(&seam, "assigned", serde_json::json!({"seam": 1, "since": "2026-11-10T00:00:00.000+0100"})).unwrap();
    assert!(jira.asked().last().unwrap().contains("updated%20%3E%3D%20%222026%2F11%2F10%2000%3A00%22%20ORDER%20BY"));
}

#[test]
fn a_site_without_the_enhanced_search_is_asked_the_old_way() {
    let (seam, jira) = seam();
    jira.on("GET", "/rest/api/3/search/jql", 404, "").on("GET", "/rest/api/2/search?jql=assignee", 200, std::fs::read_to_string(fixtures().join("../search-assigned-2.json")).unwrap());
    let page: Page = serde_json::from_str(&call(&seam, "assigned", serde_json::json!({"seam": 1, "limit": 2})).unwrap()).unwrap();
    assert_eq!(page.issues[0].key, "PROJ-9");
    assert_eq!(page.next, None, "its first page only");
    assert!(jira.asked().last().unwrap().starts_with("GET /rest/api/2/search?jql=assignee"));
}

#[test]
fn moves_are_the_workflows_on_cloud_too() {
    let (seam, jira) = seam();
    let from = serde_json::json!({"id": "3", "name": "In Progress", "stage": "doing", "column": "doing"});
    let blocked: Moves = serde_json::from_str(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "PROJ-1", "toward": "blocked", "from": from})).unwrap()).unwrap();
    assert_eq!(blocked.transitions.iter().map(|m| (m.id.as_str(), m.to.column.as_str())).collect::<Vec<_>>(), [("41", "blocked")]);
    let all: Moves = serde_json::from_str(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "PROJ-1", "from": from})).unwrap()).unwrap();
    assert_eq!(all.transitions.iter().map(|m| m.to.column.as_str()).collect::<Vec<_>>(), ["todo", "blocked", "done"]);
    assert_eq!(call(&seam, "transition", serde_json::json!({"seam": 1, "ref": "PROJ-1", "id": "31"})).unwrap(), "{}");
    assert_eq!(jira.sent.borrow().last().unwrap(), &("POST /rest/api/2/issue/PROJ-1/transitions".to_string(), Some(r#"{"transition":{"id":"31"}}"#.to_string())));
    // OPS's Blocked is the flag: Cloud's Flagged field.
    assert_eq!(call(&seam, "transition", serde_json::json!({"seam": 1, "ref": "OPS-7", "id": "flag"})).unwrap(), r#"{"said":"flagged"}"#);
    assert_eq!(jira.sent.borrow().last().unwrap(), &("PUT /rest/api/2/issue/OPS-7".to_string(), Some(r#"{"fields":{"customfield_10021":[{"value":"Impediment"}]}}"#.to_string())));
}

#[test]
fn writes_go_as_cloud_takes_them() {
    let (seam, jira) = seam();
    let writes = [
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"title": "New"}}), "PUT /rest/api/2/issue/PROJ-1", r#"{"fields":{"summary":"New"}}"#.to_string()),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"priority": "Low"}}), "PUT /rest/api/2/issue/PROJ-1", r#"{"fields":{"priority":{"name":"Low"}}}"#.to_string()),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"due": null}}), "PUT /rest/api/2/issue/PROJ-1", r#"{"fields":{"duedate":null}}"#.to_string()),
        (
            "update",
            serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"assignee": {"id": "557058:f58131cb-b67d-43c7-b30d-6b58d40bd077", "name": "Kim Park"}}}),
            "PUT /rest/api/3/issue/PROJ-1/assignee",
            r#"{"accountId":"557058:f58131cb-b67d-43c7-b30d-6b58d40bd077"}"#.to_string(),
        ),
        (
            "comment",
            serde_json::json!({"seam": 1, "ref": "PROJ-1", "body": "Fixed in 2.1.\nDeployed.\n\nThanks @Sam"}),
            "POST /rest/api/3/issue/PROJ-1/comment",
            serde_json::json!({"body": {"type": "doc", "version": 1, "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Fixed in 2.1."}, {"type": "hardBreak"}, {"type": "text", "text": "Deployed."}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "Thanks @Sam"}]}
            ]}})
            .to_string(),
        ),
        (
            "worklog",
            serde_json::json!({"seam": 1, "ref": "PROJ-1", "seconds": 4500, "started": "2026-11-12T09:15:00.000+01:00"}),
            "POST /rest/api/2/issue/PROJ-1/worklog",
            r#"{"started":"2026-11-12T09:15:00.000+0100","timeSpentSeconds":4500}"#.to_string(),
        ),
    ];
    for (name, payload, request, body) in writes {
        assert_eq!(call(&seam, name, payload).unwrap(), "{}", "{name}");
        let sent = jira.sent.borrow().last().cloned().unwrap();
        assert_eq!((sent.0.as_str(), sent.1.as_deref()), (request, Some(body.as_str())), "{name}");
    }

    // The description is never written from text on Cloud.
    let before = jira.sent.borrow().len();
    let err = SeamError::from_err(&call(&seam, "update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"body": "flattened"}})).unwrap_err());
    assert_eq!(err.code, code::REFUSED);
    assert!(err.message.starts_with("Jira Cloud keeps a description as rich text"), "{}", err.message);
    assert_eq!(jira.sent.borrow().len(), before, "nothing sent");

    let priorities: fenix_tracker::Priorities = serde_json::from_str(&call(&seam, "priorities", serde_json::json!({"seam": 1, "ref": "PROJ-1"})).unwrap()).unwrap();
    assert_eq!(priorities.priorities.iter().map(|p| format!("{} {}", p.name, p.level)).collect::<Vec<_>>(), ["Highest urgent", "High high", "Medium medium", "Low medium", "Lowest low"]);
    assert!(jira.asked().last().unwrap().starts_with("GET /rest/api/3/priority/search?maxResults=100"));

    let people: fenix_tracker::People = serde_json::from_str(&call(&seam, "people", serde_json::json!({"seam": 1, "ref": "PROJ-1"})).unwrap()).unwrap();
    assert_eq!(
        people.people.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect::<Vec<_>>(),
        [("5b10ac8d82e05b22cc7d4ef5", "Jo Bloggs"), ("557058:f58131cb-b67d-43c7-b30d-6b58d40bd077", "Kim Park")],
        "people, not apps; active ones"
    );
    assert_eq!(jira.asked().last().unwrap(), "GET /rest/api/3/user/assignable/search?issueKey=PROJ-1&maxResults=500");
}

#[test]
fn a_new_issue_is_v3_with_its_description_as_rich_text() {
    let jira = Rc::new(Recorded::jira_cloud(fixtures()));
    jira.on("POST", "/rest/api/3/issue", 201, r#"{"id":"10400","key":"PROJ-13","self":"https://acme.atlassian.net/rest/api/3/issue/10400"}"#);
    let client = JiraClient::cloud(jira.clone());
    let issue = NewIssue {
        project: "PROJ".into(),
        issue_type_id: "10004".into(),
        summary: "Crash on start".into(),
        description: "Seen twice.\nOn 2.1.".into(),
        assignee: Some("5b10ac8d82e05b22cc7d4ef5".into()),
        ..Default::default()
    };
    assert_eq!(block_on(client.create(&issue)), Ok("PROJ-13".to_string()));
    let (request, body) = jira.sent.borrow().last().cloned().unwrap();
    assert_eq!(request, "POST /rest/api/3/issue");
    let body: serde_json::Value = serde_json::from_str(&body.unwrap()).unwrap();
    assert_eq!(body["fields"]["assignee"], serde_json::json!({"accountId": "5b10ac8d82e05b22cc7d4ef5"}));
    assert_eq!(body["fields"]["description"]["content"][0]["content"][1], serde_json::json!({"type": "hardBreak"}));
    // The page's description edit is refused, as the seam's is.
    assert!(block_on(client.update_description("PROJ-1", "x")).unwrap_err().starts_with("Jira Cloud keeps"));
    assert_eq!(block_on(client.get_issue("PROJ-1")).unwrap().id, "10201", "Jira's own id, kept beside the key");
    assert_eq!(block_on(client.myself()), Ok("5b10ac8d82e05b22cc7d4ef5".to_string()), "an account id, not a username");
    jira.on("GET", "/rest/api/3/search/jql?jql=project", 200, std::fs::read_to_string(fixtures().join("search-jql-linked.json")).unwrap());
    let rows = block_on(client.search_issues("project = PROJ", 50)).unwrap();
    assert_eq!(rows.len(), 3);
    assert!(jira.asked().last().unwrap().starts_with("GET /rest/api/3/search/jql?jql=project%20%3D%20PROJ&maxResults=50&fields=summary%2Cstatus"));
}

#[test]
fn errors_are_the_seams_with_what_a_429_said_to_wait() {
    let (seam, jira) = seam();
    jira.on("GET", "/rest/api/2/issue/PROJ-1/transitions", 401, r#"{"errorMessages":["You are not authenticated. Authentication required to perform this operation."],"errors":{}}"#);
    let err = SeamError::from_err(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "PROJ-1"})).unwrap_err());
    assert_eq!(err.code, code::SIGNED_OUT);

    jira.on("POST", "/comment", 403, r#"{"errorMessages":["You do not have permission to add comments to this issue."],"errors":{}}"#);
    let err = SeamError::from_err(&call(&seam, "comment", serde_json::json!({"seam": 1, "ref": "PROJ-1", "body": "x"})).unwrap_err());
    assert_eq!((err.code.as_str(), err.retry_after), (code::REFUSED, None));

    jira.on_wait("GET", "/rest/api/3/issue/PROJ-1?", 429, "Rate limit exceeded", 30).on("GET", "/rest/api/2/myself", 200, std::fs::read_to_string(fixtures().join("myself.json")).unwrap());
    let err = SeamError::from_err(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["PROJ-1"]})).unwrap_err());
    assert_eq!((err.code.as_str(), err.message.as_str(), err.retry_after), (code::RATE_LIMITED, "HTTP 429", Some(30)));

    jira.on("GET", "/rest/api/3/search/jql", 429, r#"{"errorMessages":["Too many requests"]}"#);
    let err = SeamError::from_err(&call(&seam, "assigned", serde_json::json!({"seam": 1})).unwrap_err());
    assert_eq!((err.code.as_str(), err.retry_after), (code::RATE_LIMITED, None), "no Retry-After, no advice");

    let none = Seam::new(JiraClient::cloud(jira.clone()), Settings::default(), Rc::new(RefCell::new(Learned::default())));
    let err = SeamError::from_err(&call(&none, "issues", serde_json::json!({"seam": 1, "refs": ["PROJ-1"]})).unwrap_err());
    assert_eq!((err.code.as_str(), err.message.as_str()), (code::NOT_SET_UP, "Jira Cloud isn't set up yet -- set its site, your email and an API token in SPC , (Jira)"));
}
