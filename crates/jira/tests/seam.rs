//! The tracker seam served for Jira Server, on recorded responses
//! (`fixtures/`, Jira Server 8's shapes): each call, what it asks Jira,
//! what it answers, its errors, and that no token is ever in an answer.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use fenix_jira_client::recorded::Recorded;
use fenix_jira_client::tracker::{Keys, Learned, Seam, Settings};
use fenix_jira_client::JiraClient;
use fenix_tracker::{code, Found, Moves, Page, SeamError};
use futures_executor::block_on;

const TOKEN: &str = "NjQ1MzIxOTg3NjU0OlRPS0VO-secret";

fn fixtures() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

fn settings() -> Settings {
    Settings {
        base_url: Some("https://jira.example.com/".into()),
        projects: vec!["PROJ".into()],
        users: vec![("jo1".into(), "Jo Bloggs".into()), ("kim".into(), "Kim Park".into())],
        priorities: vec![("Minor".into(), "Medium".into())],
        blocked: vec![("PROJ".into(), "10103: On Hold".into()), ("OPS".into(), "flag".into())],
        sync_minutes: 10,
        keys: Keys::default(),
    }
}

fn seam_with(settings: Settings) -> (Seam, Rc<Recorded>) {
    let jira = Rc::new(Recorded::jira_server(fixtures()));
    (Seam::new(JiraClient::new(jira.clone()), settings, Rc::new(RefCell::new(Learned::default()))), jira)
}

fn call(seam: &Seam, name: &str, payload: serde_json::Value) -> Result<String, String> {
    let answer = block_on(seam.serve(name, &payload.to_string()));
    let text = match &answer {
        Ok(t) | Err(t) => t,
    };
    assert!(!text.contains(TOKEN) && !text.contains("Bearer"), "a token in an answer: {text}");
    answer
}

#[test]
fn linked_issues_come_back_as_the_seam_has_them() {
    let (seam, jira) = seam_with(settings());
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["PROJ-1", "PROJ-2", "OPS-7", "PROJ-404"]})).unwrap()).unwrap();
    assert_eq!(found.missing, ["PROJ-404"]);
    let [one, two, ops] = &found.issues[..] else { panic!("{found:?}") };

    assert_eq!(one.key, "PROJ-1");
    assert_eq!(one.url.as_deref(), Some("https://jira.example.com/browse/PROJ-1"));
    assert_eq!(one.project.as_deref(), Some("PROJ"));
    assert_eq!(one.title, "Fix the login timeout");
    assert_eq!(one.body, "Users are logged out after 5 minutes.\n\n*Steps*\n# Log in\n# Wait", "wiki markup as it is, line ends as \\n");
    assert_eq!((one.state.id.as_str(), one.state.name.as_str(), one.state.stage.as_str(), one.state.column.as_str()), ("3", "In Progress", "doing", "doing"));
    assert_eq!(one.priority.as_ref().map(|p| (p.name.as_str(), p.level.as_str())), Some(("Major", "high")));
    assert_eq!(one.assignee.as_ref().map(|p| (p.id.as_str(), p.name.as_str())), Some(("jo1", "Jo Bloggs")));
    assert_eq!(one.mine, Some(true));
    assert_eq!(one.labels, ["auth", "backend"]);
    assert_eq!((one.due.as_deref(), one.kind.as_deref()), (Some("2026-11-20"), Some("Bug")));
    assert_eq!(one.updated, "2026-11-13T16:05:09.000+0100", "as Jira wrote it");
    assert_eq!(one.comments.len(), 2);
    assert_eq!((one.comments[0].author.as_str(), one.comments[0].created.as_str()), ("Sam Lee", "2026-11-03T09:00:00.000+0100"));

    // On Hold is what Blocked means in PROJ; OPS's Blocked is the flag.
    assert_eq!((two.state.stage.as_str(), two.state.column.as_str()), ("doing", "blocked"));
    assert_eq!(two.priority.as_ref().unwrap().level, "medium", "jira.priorities over the guess");
    assert_eq!((ops.state.column.as_str(), ops.mine), ("blocked", Some(false)));
    assert_eq!(ops.priority.as_ref().unwrap().level, "urgent");

    // As the agenda asked before: who you are, the Flagged field, one search.
    let asked = jira.asked();
    assert_eq!(asked[0], "GET /rest/api/2/myself");
    assert_eq!(asked[1], "GET /rest/api/2/field");
    assert!(asked[2].starts_with("GET /rest/api/2/search?jql=key%20IN%20%28%22PROJ-1%22%2C%22PROJ-2%22%2C%22OPS-7%22%2C%22PROJ-404%22%29&maxResults=4&fields="), "{asked:?}");
    assert!(asked[2].ends_with("issuetype%2Clabels%2Ccustomfield_10000&validateQuery=false"), "a deleted key is left out, not a failed search: {asked:?}");
    assert_eq!((one.id.as_str(), one.asked.as_deref()), ("10201", Some("PROJ-1")));

    // Learned once: the next call asks neither again.
    call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["PROJ-1"]})).unwrap();
    assert_eq!(jira.asked()[3], "GET /rest/api/2/issue/PROJ-1?fields=summary%2Cdescription%2Cstatus%2Cpriority%2Cassignee%2Creporter%2Ccreated%2Cupdated%2Ccomment%2Cduedate%2Cissuetype%2Clabels%2Ccustomfield_10000");
}

#[test]
fn one_issue_by_its_address_or_typed_in_lower_case() {
    let (seam, _) = seam_with(settings());
    for r in ["https://jira.example.com/browse/PROJ-1", "proj-1"] {
        let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": [r]})).unwrap()).unwrap();
        assert_eq!((found.issues[0].key.as_str(), found.missing.len()), ("PROJ-1", 0), "{r}");
    }
    let err = SeamError::from_err(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["NOPE-1"]})).unwrap_err());
    assert_eq!((err.code.as_str(), err.message.as_str()), (code::NOT_FOUND, "HTTP 404 (Issue Does Not Exist)"));
}

#[test]
fn whats_assigned_pages_by_start_at_and_is_cleaned() {
    let (seam, jira) = seam_with(settings());
    let first: Page = serde_json::from_str(&call(&seam, "assigned", serde_json::json!({"seam": 1, "limit": 2})).unwrap()).unwrap();
    assert_eq!(first.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["PROJ-1", "PROJ-2"]);
    assert_eq!(first.next.as_deref(), Some("2"));
    let search = jira.asked().into_iter().find(|a| a.contains("search")).unwrap();
    assert!(
        search.starts_with("GET /rest/api/2/search?jql=assignee%20%3D%20currentUser%28%29%20AND%20statusCategory%20%21%3D%20Done%20AND%20project%20IN%20%28PROJ%29%20ORDER%20BY%20updated%20DESC&maxResults=2&fields="),
        "the agenda's own JQL: {search}"
    );
    let second: Page = serde_json::from_str(&call(&seam, "assigned", serde_json::json!({"seam": 1, "limit": 2, "cursor": "2"})).unwrap()).unwrap();
    assert_eq!(second.next, None);
    // Someone's escape codes, bidi override and newline: shown, never run.
    assert_eq!(second.issues[0].title, "Write the [31mrelease notes for 2.0");

}

#[test]
fn moves_are_the_workflows_and_blocked_is_the_projects() {
    // PROJ: Blocked is On Hold, reachable from In Progress.
    let (seam, _) = seam_with(settings());
    let from = serde_json::json!({"id": "3", "name": "In Progress", "stage": "doing", "column": "doing"});
    let all: Moves = serde_json::from_str(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "PROJ-1", "from": from})).unwrap()).unwrap();
    let rows: Vec<(String, &str)> = all.transitions.iter().map(|m| (m.row(), m.to.column.as_str())).collect();
    assert_eq!(rows, [("Stop progress  ->  Open".into(), "todo"), ("Put on hold  ->  On Hold".into(), "blocked"), ("Resolve  ->  Resolved".into(), "done"), ("Close  ->  Closed".into(), "done")]);
    assert_eq!(all.blocked.as_deref(), Some("Blocked  (On Hold)"));
    let blocked: Moves = serde_json::from_str(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "PROJ-1", "toward": "blocked", "from": from})).unwrap()).unwrap();
    assert_eq!(blocked.transitions.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["41"]);

    // Not reachable: every move, and why.
    let mut s = settings();
    s.blocked[0].1 = "999: Parked".into();
    let (seam, _) = seam_with(s);
    let none: Moves = serde_json::from_str(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "PROJ-1", "toward": "blocked", "from": from})).unwrap()).unwrap();
    assert!(none.transitions.iter().all(|m| m.to.column != "blocked"));
    assert_eq!(none.why.as_deref(), Some("\"Parked\" isn't reachable from \"In Progress\" -- pick a step from here"));

    // Not decided yet: asked, each choice remembered.
    let mut s = settings();
    s.blocked.clear();
    let (seam, _) = seam_with(s);
    let ask: Moves = serde_json::from_str(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "PROJ-1", "toward": "blocked", "from": from})).unwrap()).unwrap();
    assert_eq!(ask.ask.as_ref().unwrap().title, "What does Blocked mean in PROJ?");
    assert_eq!(ask.ask.as_ref().unwrap().note.as_deref(), Some("Pick what Blocked means in PROJ -- asked once, then remembered (b on the project in SPC j j changes it)"));
    let rows: Vec<String> = ask.transitions.iter().map(|m| m.row()).collect();
    assert_eq!(rows, ["Open   (via \"Stop progress\")", "On Hold   (via \"Put on hold\")", "Resolved   (via \"Resolve\")", "Closed   (via \"Close\")", "Flag as impediment (keep the status)", "Keep Blocked local for PROJ"]);
    assert!(ask.transitions.iter().all(|m| m.to.column == "blocked" && m.remember.is_some()));
    assert!(ask.transitions.last().unwrap().local);
    assert_eq!(ask.transitions[1].note.as_deref(), Some("Blocked in PROJ now moves the issue to \"On Hold\""));
    assert_eq!(fenix_jira_client::tracker::parse_remember(ask.transitions[1].remember.as_deref().unwrap()).map(|(p, b)| (p, b.text())), Some(("PROJ".into(), "10103: On Hold".into())));
}

#[test]
fn the_flag_is_a_move_in_and_out_of_blocked() {
    let (seam, jira) = seam_with(settings());
    let open = serde_json::json!({"id": "1", "name": "Open", "stage": "todo", "column": "todo"});
    let flag: Moves = serde_json::from_str(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "OPS-7", "toward": "blocked", "from": open})).unwrap()).unwrap();
    assert_eq!((flag.transitions[0].id.as_str(), flag.transitions[0].to.column.as_str(), flag.transitions[0].to.name.as_str()), ("flag", "blocked", "Open"));
    assert!(jira.asked().is_empty(), "known without asking Jira");

    let flagged = serde_json::json!({"id": "1", "name": "Open", "stage": "todo", "column": "blocked"});
    let back: Moves = serde_json::from_str(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "OPS-7", "toward": "todo", "from": flagged})).unwrap()).unwrap();
    assert_eq!((back.transitions[0].id.as_str(), back.transitions[0].to.column.as_str()), ("unflag", "todo"));

    let said = call(&seam, "transition", serde_json::json!({"seam": 1, "ref": "OPS-7", "id": "flag"})).unwrap();
    assert_eq!(said, r#"{"said":"flagged"}"#);
    let sent = jira.sent.borrow().clone();
    assert_eq!(sent.last().unwrap(), &("PUT /rest/api/2/issue/OPS-7".to_string(), Some(r#"{"fields":{"customfield_10000":[{"value":"Impediment"}]}}"#.to_string())));
    call(&seam, "transition", serde_json::json!({"seam": 1, "ref": "OPS-7", "id": "unflag+21"})).unwrap();
    let sent = jira.sent.borrow().clone();
    assert_eq!(sent[sent.len() - 2].1.as_deref(), Some(r#"{"fields":{"customfield_10000":null}}"#));
    assert_eq!(sent[sent.len() - 1], ("POST /rest/api/2/issue/OPS-7/transitions".to_string(), Some(r#"{"transition":{"id":"21"}}"#.to_string())));
}

#[test]
fn writes_are_the_requests_the_agenda_sent_before() {
    let (seam, jira) = seam_with(settings());
    let writes = [
        ("transition", serde_json::json!({"seam": 1, "ref": "PROJ-1", "id": "41"}), "POST /rest/api/2/issue/PROJ-1/transitions", r#"{"transition":{"id":"41"}}"#),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"title": "New"}}), "PUT /rest/api/2/issue/PROJ-1", r#"{"fields":{"summary":"New"}}"#),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"body": "B"}}), "PUT /rest/api/2/issue/PROJ-1", r#"{"fields":{"description":"B"}}"#),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"priority": "Minor"}}), "PUT /rest/api/2/issue/PROJ-1", r#"{"fields":{"priority":{"name":"Minor"}}}"#),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"due": null}}), "PUT /rest/api/2/issue/PROJ-1", r#"{"fields":{"duedate":null}}"#),
        ("update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"assignee": {"id": "kim", "name": "Kim Park"}}}), "PUT /rest/api/2/issue/PROJ-1/assignee", r#"{"name":"kim"}"#),
        ("comment", serde_json::json!({"seam": 1, "ref": "PROJ-1", "body": "Done."}), "POST /rest/api/2/issue/PROJ-1/comment", r#"{"body":"Done."}"#),
        (
            "worklog",
            serde_json::json!({"seam": 1, "ref": "PROJ-1", "seconds": 4500, "started": "2026-11-12T09:15:00.000+01:00"}),
            "POST /rest/api/2/issue/PROJ-1/worklog",
            r#"{"started":"2026-11-12T09:15:00.000+0100","timeSpentSeconds":4500}"#,
        ),
    ];
    for (name, payload, request, body) in writes {
        call(&seam, name, payload).unwrap();
        let sent = jira.sent.borrow().last().cloned().unwrap();
        assert_eq!((sent.0.as_str(), sent.1.as_deref()), (request, Some(body)), "{name}");
    }
    let priorities: fenix_tracker::Priorities = serde_json::from_str(&call(&seam, "priorities", serde_json::json!({"seam": 1, "ref": "PROJ-1"})).unwrap()).unwrap();
    assert_eq!(priorities.priorities.iter().map(|p| format!("{} {}", p.name, p.level)).collect::<Vec<_>>(), ["Highest urgent", "High high", "Major high", "Minor medium", "Lowest low"]);
    let people: fenix_tracker::People = serde_json::from_str(&call(&seam, "people", serde_json::json!({"seam": 1, "ref": "PROJ-1"})).unwrap()).unwrap();
    assert_eq!(people.people.len(), 2);
    assert_eq!(people.none.as_deref(), Some("no people to assign to -- a on the Jira page (SPC j j) adds one"));
}

#[test]
fn errors_are_the_seams_with_jiras_words() {
    let (seam, jira) = seam_with(Settings::default());
    let err = SeamError::from_err(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["PROJ-1"]})).unwrap_err());
    assert_eq!((err.code.as_str(), err.message.as_str()), (code::NOT_SET_UP, "Jira isn't set up yet -- set its server and token in SPC , (Jira)"));
    assert!(jira.asked().is_empty());

    let (seam, jira) = seam_with(settings());
    jira.on("GET", "/rest/api/2/issue/PROJ-1/transitions", 401, "");
    let err = SeamError::from_err(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "PROJ-1"})).unwrap_err());
    assert_eq!((err.code.as_str(), err.message.as_str()), (code::SIGNED_OUT, "HTTP 401"));
    jira.on("GET", "/rest/api/2/myself", 429, r#"{"errorMessages":["Rate limit exceeded"]}"#).on("GET", "/rest/api/2/issue/PROJ-1?", 429, r#"{"errorMessages":["Rate limit exceeded"]}"#);
    let err = SeamError::from_err(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["PROJ-1"]})).unwrap_err());
    assert_eq!((err.code.as_str(), err.message.as_str()), (code::RATE_LIMITED, "HTTP 429 (Rate limit exceeded)"));
    // No answer at all: the request never got there.
    let err = SeamError::from_err(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "ZZZ-1"})).unwrap_err());
    assert_eq!((err.code.as_str(), err.message.as_str()), (code::OFFLINE, "request failed: nothing recorded for GET /rest/api/2/issue/ZZZ-1/transitions"));
    jira.on("POST", "/comment", 403, r#"{"errorMessages":["You do not have the permission to comment on this issue."]}"#);
    let err = SeamError::from_err(&call(&seam, "comment", serde_json::json!({"seam": 1, "ref": "PROJ-1", "body": "x"})).unwrap_err());
    assert_eq!((err.code.as_str(), err.message.as_str()), (code::REFUSED, "HTTP 403 (You do not have the permission to comment on this issue.)"));
}

#[test]
fn a_newer_seam_or_an_odd_payload_is_refused_plainly() {
    let (seam, _) = seam_with(settings());
    let err = SeamError::from_err(&call(&seam, "issues", serde_json::json!({"seam": 2, "refs": []})).unwrap_err());
    assert_eq!(err.code, code::UNSUPPORTED);
    let err = SeamError::from_err(&block_on(seam.serve("issues", "not json")).unwrap_err());
    assert_eq!(err.code, code::BAD_REQUEST);
    let err = SeamError::from_err(&call(&seam, "teleport", serde_json::json!({"seam": 1})).unwrap_err());
    assert_eq!(err.code, code::UNSUPPORTED);
}

#[test]
fn refs_are_checked_before_they_reach_a_url() {
    let (seam, jira) = seam_with(settings());
    for bad in ["../../secure/admin", "A-1?x=1", "PROJ-1/../2", "proj 1", ""] {
        for (name, payload) in [
            ("transitions", serde_json::json!({"seam": 1, "ref": bad})),
            ("transition", serde_json::json!({"seam": 1, "ref": bad, "id": "1"})),
            ("comment", serde_json::json!({"seam": 1, "ref": bad, "body": "x"})),
            ("update", serde_json::json!({"seam": 1, "ref": bad, "set": {"title": "x"}})),
        ] {
            let err = SeamError::from_err(&call(&seam, name, payload).unwrap_err());
            assert_eq!(err.code, code::BAD_REQUEST, "{name} {bad:?}");
        }
    }
    assert!(jira.asked().is_empty(), "nothing sent: {:?}", jira.asked());
    // In a batch, a bad one is only missing.
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["PROJ-1", "../x"]})).unwrap()).unwrap();
    assert_eq!(found.missing, ["../x"]);
}

#[test]
fn issues_are_asked_by_id_and_a_moved_one_answers_what_asked_for_it() {
    let (seam, jira) = seam_with(settings());
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["10201", "10202", "OPS-7"]})).unwrap()).unwrap();
    assert!(jira.asked().iter().any(|a| a.starts_with("GET /rest/api/2/search?jql=id%20IN%20%2810201%2C10202%29%20OR%20key%20IN%20%28%22OPS-7%22%29")), "{:?}", jira.asked());
    assert_eq!(found.issues.iter().map(|i| i.asked.as_deref().unwrap()).collect::<Vec<_>>(), ["10201", "10202", "OPS-7"]);

    // OLD-1 moved: Jira answers its fetch as PROJ-1, which is what OLD-1 asked.
    jira.on("GET", "/rest/api/2/issue/OLD-1?", 200, std::fs::read_to_string(fixtures().join("issue-PROJ-1.json")).unwrap());
    let moved: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["OLD-1"]})).unwrap()).unwrap();
    assert_eq!((moved.issues[0].key.as_str(), moved.issues[0].id.as_str(), moved.issues[0].asked.as_deref()), ("PROJ-1", "10201", Some("OLD-1")));
}

#[test]
fn a_server_that_still_refuses_the_search_is_asked_one_at_a_time() {
    let (seam, jira) = seam_with(settings());
    jira.on("GET", "/rest/api/2/search?jql=key%20IN", 400, r#"{"errorMessages":["An issue with key 'PROJ-404' does not exist for field 'key'."]}"#);
    let found: Found = serde_json::from_str(&call(&seam, "issues", serde_json::json!({"seam": 1, "refs": ["PROJ-1", "NOPE-1"]})).unwrap()).unwrap();
    assert_eq!((found.issues.len(), found.missing.as_slice()), (1, &["NOPE-1".to_string()][..]));
}

#[test]
fn unassigning_and_labels_and_more_errors() {
    let (seam, jira) = seam_with(settings());
    call(&seam, "update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"assignee": null}})).unwrap();
    assert_eq!(jira.sent.borrow().last().unwrap(), &("PUT /rest/api/2/issue/PROJ-1/assignee".to_string(), Some(r#"{"name":null}"#.to_string())));
    call(&seam, "update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"labels": ["a", "b c"]}})).unwrap();
    assert_eq!(jira.sent.borrow().last().unwrap().1.as_deref(), Some(r#"{"fields":{"labels":["a","b c"]}}"#));
    let odd = SeamError::from_err(&call(&seam, "update", serde_json::json!({"seam": 1, "ref": "PROJ-1", "set": {"colour": "red"}})).unwrap_err());
    assert_eq!(odd.code, code::UNSUPPORTED);
    for (status, want) in [(410, code::NOT_FOUND), (503, code::OFFLINE), (502, code::OFFLINE), (500, code::FAILED)] {
        jira.on("GET", "/rest/api/2/issue/PROJ-1/transitions", status, "");
        let err = SeamError::from_err(&call(&seam, "transitions", serde_json::json!({"seam": 1, "ref": "PROJ-1"})).unwrap_err());
        assert_eq!(err.code, want, "{status}");
    }
}

#[test]
fn what_it_learned_is_forgotten_when_the_server_changes() {
    let jira = Rc::new(Recorded::jira_server(fixtures()));
    let learned = Rc::new(RefCell::new(Learned::default()));
    let one = Seam::new(JiraClient::new(jira.clone()), settings(), learned.clone());
    call(&one, "issues", serde_json::json!({"seam": 1, "refs": ["PROJ-1"]})).unwrap();
    assert_eq!(learned.borrow().me.as_deref(), Some("jo1"));
    let asked = jira.asked().iter().filter(|a| a.ends_with("/myself")).count();
    let other = Seam::new(JiraClient::new(jira.clone()), Settings { base_url: Some("https://other.example.com".into()), ..settings() }, learned.clone());
    call(&other, "issues", serde_json::json!({"seam": 1, "refs": ["PROJ-1"]})).unwrap();
    assert_eq!(jira.asked().iter().filter(|a| a.ends_with("/myself")).count(), asked + 1, "asked again of the new server");
    assert_eq!(learned.borrow().base.as_deref(), Some("https://other.example.com"));
}

#[test]
fn about_is_fenix_jiras_whatever_seam_and_names_are_by_full_id() {
    let a = fenix_jira_client::tracker::about(&settings(), true);
    assert_eq!((a.id.as_str(), a.seams.as_slice()), ("fenix.jira", &[1u32][..]));
    assert!(a.can.iter().any(|c| c == "edit.assignee") && !a.can.iter().any(|c| c == "edit"));
    assert_eq!(fenix_tracker::service(fenix_jira_client::tracker::ID, "about"), "fenix.jira.tracker.about");
}
