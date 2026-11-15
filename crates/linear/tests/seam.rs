//! The seam served for Linear, on recorded transports (`tests/fixtures`,
//! written from Linear's schema: not recordings). Every call of the seam,
//! the way the agenda's own tests drive `FakeTracker`; paging, what an
//! issue is called, priorities and states, each error, and that nothing a
//! person typed can change a query.

use std::path::Path;
use std::rc::Rc;

use fenix_linear_client::seam::{self, Keys, Seam, Settings};
use fenix_linear_client::{Client, Linear, Reply};
use fenix_tracker::{self as tracker, call, Found, Moves, Page, SeamError};
use fenix_linear_client::recorded::Recorded;
use serde_json::{json, Value};

/// A time like now, in seconds since 1970.
const NOW: i64 = 1_800_000_000;

fn fixtures() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

fn linear(r: &Rc<Recorded>) -> Linear {
    Linear::new(Client::new(r.clone(), Rc::new(|| NOW)))
}

fn seam_over(r: &Rc<Recorded>) -> Seam {
    Seam::new(linear(r), Settings::default())
}

fn workspace() -> (Rc<Recorded>, Seam) {
    let r = Rc::new(Recorded::workspace(fixtures()));
    let s = seam_over(&r);
    (r, s)
}

/// A call, its request given as JSON, its answer read as `T`.
fn ask<T: serde::de::DeserializeOwned>(s: &Seam, name: &str, mut request: Value) -> Result<T, SeamError> {
    request["seam"] = json!(1);
    match futures_executor::block_on(s.serve(name, &request.to_string())) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name} answered {text}: {e}"))),
        Err(e) => Err(SeamError::from_err(&e)),
    }
}

fn err_reply(status: u16, body: Value) -> Reply {
    Reply { status, headers: Vec::new(), body: body.to_string() }
}

#[test]
fn it_says_who_it_is_and_lists_only_what_it_serves() {
    let about = seam::about(&Settings::default());
    assert_eq!((about.name.as_str(), about.ready, about.sync_minutes), ("Linear", true, 10));
    assert_eq!(about.ref_hint.as_deref(), Some("ENG-123"));
    assert_eq!(about.browse_url("ENG-1").as_deref(), Some("https://linear.app/issue/ENG-1"));
    let mut can = about.can.clone();
    can.sort();
    assert_eq!(can, ["comment", "comments", "create", "edit.assignee", "edit.body", "edit.due", "edit.labels", "edit.priority", "edit.title", "project"], "per field; no worklog and no remember: Linear has neither");
    assert_eq!((about.id.as_str(), about.seams.as_slice(), about.blocked), ("fenix.linear", &[1u32][..], false), "no Blocked of its own");

    let unready = seam::about(&Settings { ready: false, keys: Keys { settings: "C-,".into(), ..Default::default() }, ..Default::default() });
    assert_eq!((unready.ready, unready.not_ready.as_deref()), (false, Some("Linear isn't set up yet -- add a personal API key in C-, (Linear)")));

    // Every call it lists is served, and the ones it doesn't are `unsupported`.
    let (_, s) = workspace();
    assert!(ask::<Value>(&s, call::PRIORITIES, json!({})).is_ok());
    for c in [call::WORKLOG, call::REMEMBER, "nonsense"] {
        let err = ask::<Value>(&s, c, json!({ "ref": "ENG-12", "seconds": 1, "started": "x", "remember": "x" })).unwrap_err();
        assert_eq!(err.code, "unsupported", "{c}");
    }
    let err = ask::<Value>(&s, call::CREATE, json!({ "title": "x" })).unwrap_err();
    assert_eq!(err.code, "unsupported", "create is the extension's: it opens the form");
}

#[test]
fn nothing_is_served_until_there_is_a_key_and_only_the_seam_it_speaks() {
    let r = Rc::new(Recorded::workspace(fixtures()));
    let s = Seam::new(linear(&r), Settings { ready: false, ..Default::default() });
    for c in [call::ASSIGNED, call::ISSUES, call::TRANSITIONS, call::TRANSITION, call::UPDATE, call::PRIORITIES, call::PEOPLE, call::COMMENT] {
        let err = ask::<Value>(&s, c, json!({ "ref": "ENG-12", "refs": [], "id": "x", "set": {}, "body": "x" })).unwrap_err();
        assert_eq!((err.code.as_str(), err.message.as_str()), ("not_set_up", "Linear isn't set up yet -- add a personal API key in SPC , (Linear)"), "{c}");
    }
    assert!(r.sent().is_empty(), "no request without a key");
    let (_, s) = workspace();
    let err = futures_executor::block_on(s.serve(call::ASSIGNED, r#"{"seam": 2}"#)).unwrap_err();
    assert_eq!(SeamError::from_err(&err).code, "unsupported");
    let err = futures_executor::block_on(s.serve(call::ISSUES, r#"{"seam": 1, "refs": 3}"#)).unwrap_err();
    assert_eq!(SeamError::from_err(&err).code, "bad_request");
}

#[test]
fn assigned_pages_by_cursor_and_maps_every_field() {
    let (r, s) = workspace();
    let first: Page = ask(&s, call::ASSIGNED, json!({ "limit": 3 })).unwrap();
    assert_eq!(first.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["ENG-12", "ENG-15", "ENG-20"]);
    assert_eq!(first.next.as_deref(), Some("cursor-2"), "Linear's endCursor, as it was");
    let second: Page = ask(&s, call::ASSIGNED, json!({ "limit": 3, "cursor": first.next })).unwrap();
    assert_eq!(second.issues.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["OPS-7"]);
    assert_eq!(second.next, None, "the last page has none");

    // What was asked: yours, a page at a time, from the cursor, not finished.
    let asked = r.variables_of("AssignedIssues");
    assert_eq!(asked[0], json!({ "first": 3, "after": null, "filter": { "state": { "type": { "nin": ["completed", "canceled"] } } } }));
    assert_eq!(asked[1]["after"], "cursor-2");
    assert!(r.sent()[0].contains("viewer") && r.sent()[0].contains("assignedIssues"));

    let i = &first.issues[0];
    assert_eq!(i.url.as_deref(), Some("https://linear.app/acme/issue/ENG-12/fix-the-login-timeout"));
    assert_eq!(i.project.as_deref(), Some("ENG"), "the team's key");
    assert_eq!(i.body, "The login times out after **30 s** on slow networks.\n\n- repro: throttle to 3G\n- see ENG-3", "Markdown as it is");
    assert_eq!((i.state.id.as_str(), i.state.name.as_str(), i.state.stage.as_str(), i.state.column.as_str()), ("57000000-0000-4000-8000-000000000004", "In Progress", "doing", "doing"));
    let p = i.priority.as_ref().unwrap();
    assert_eq!((p.name.as_str(), p.level.as_deref()), ("High", Some("high")));
    assert_eq!((i.assignee.as_ref().map(|a| a.name.as_str()), i.mine), (Some("Jo Bloggs"), Some(true)));
    assert_eq!(i.labels, ["bug", "auth"]);
    assert_eq!((i.due.as_deref(), i.updated.as_str()), (Some("2026-11-20"), "2026-11-13T16:40:12.402Z"));
    assert_eq!(i.comments.iter().map(|c| (c.author.as_str(), c.body.as_str())).collect::<Vec<_>>(), [("Jo Bloggs", "Taking this."), ("Sentry", "Linked the Sentry event."), ("Amy Park", "Reproduced on staging.")], "oldest first, a bot by its name");
    // Backlog and Todo are the agenda's todo; no priority is none; \r\n is kept, as Linear has it.
    assert_eq!((first.issues[1].state.stage.as_str(), first.issues[1].body.as_str()), ("todo", "Cover cookie lifetime.\r\nAnd refresh."));
    assert_eq!((first.issues[2].state.stage.as_str(), first.issues[2].state.name.as_str(), first.issues[2].priority.is_none()), ("todo", "Backlog", true));
    assert_eq!(second.issues[0].state.stage, "doing", "In Review is a started state");
    assert_eq!(second.issues[0].priority.as_ref().map(|p| p.level.as_deref()), Some(Some("urgent")));
}

#[test]
fn a_page_is_at_most_fifty_whatever_the_limit() {
    let (r, s) = workspace();
    let _: Page = ask(&s, call::ASSIGNED, json!({ "limit": 200 })).unwrap();
    assert_eq!(r.variables_of("AssignedIssues")[0]["first"], 50, "Linear's cost stays down");
    assert!(ask::<Value>(&s, call::ASSIGNED, json!({ "since": "2026-11-01T00:00:00.000Z" })).is_ok(), "a field it doesn't know is ignored");
    assert!(r.variables_of("AssignedIssues")[1]["filter"].get("updatedAt").is_none());
}

#[test]
fn issues_resolve_identifiers_addresses_and_ids_and_say_which_are_missing() {
    let (r, s) = workspace();
    // One, by its address: fetched on its own, found under its identifier.
    let found: Found = ask(&s, call::ISSUES, json!({ "refs": ["https://linear.app/acme/issue/ENG-12/fix-the-login-timeout"] })).unwrap();
    assert_eq!((found.issues[0].key.as_str(), found.missing.len()), ("ENG-12", 0));
    assert_eq!(r.variables_of("Issue"), [json!({ "id": "ENG-12" })]);

    // Several: one request, a filter of each, in variables. What it didn't
    // find is asked for one at a time (an issue that moved has another
    // identifier now): here, not there either.
    let gone = || err_reply(200, json!({ "errors": [{ "message": "Entity not found: Issue - Could not find referenced Issue.", "extensions": { "type": "invalid input", "code": "INVALID_INPUT", "userError": true } }], "data": null }));
    r.on_with("Issue", "\"ENG-99\"", gone());
    r.on_with("Issue", "3f2504e0", gone());
    let found: Found = ask(&s, call::ISSUES, json!({ "refs": ["eng-12", "ENG-15", "https://linear.app/acme/issue/ENG-99/gone", "bad ref", "3f2504e0-4f89-41d3-9a0c-0305e82c3301"] })).unwrap();
    assert_eq!(found.issues.iter().map(|i| (i.key.as_str(), i.asked.as_deref())).collect::<Vec<_>>(), [("ENG-12", Some("eng-12")), ("ENG-15", Some("ENG-15"))], "each says what asked for it");
    assert_eq!(found.issues[0].id, "1550e000-0000-4000-8000-000000000012", "Linear's own id, which an identifier change doesn't touch");
    assert_eq!(found.missing, ["https://linear.app/acme/issue/ENG-99/gone", "bad ref", "3f2504e0-4f89-41d3-9a0c-0305e82c3301"], "exactly as asked, even what isn't a ref, in the order asked");
    let filter = &r.variables_of("Issues")[0]["filter"];
    assert_eq!(filter["or"][0], json!({ "team": { "key": { "eqIgnoreCase": "ENG" } }, "number": { "eq": 12 } }));
    assert_eq!(filter["or"][2], json!({ "team": { "key": { "eqIgnoreCase": "ENG" } }, "number": { "eq": 99 } }));
    assert_eq!(filter["or"][3], json!({ "id": { "in": ["3f2504e0-4f89-41d3-9a0c-0305e82c3301"] } }));

    // An issue that moved teams: asked by its old identifier, answered under its new one.
    r.on_with("Issue", "\"ENG-7\"", Reply::ok(std::fs::read_to_string(fixtures().join("issue-ENG-12.json")).unwrap()));
    let found: Found = ask(&s, call::ISSUES, json!({ "refs": ["ENG-7", "ENG-15"] })).unwrap();
    assert_eq!(found.issues.iter().map(|i| (i.key.as_str(), i.asked.as_deref())).collect::<Vec<_>>(), [("ENG-12", Some("ENG-7")), ("ENG-15", Some("ENG-15"))]);
    assert!(found.missing.is_empty());

    // One that isn't there is missing, the call still succeeds.
    r.on("Issue", err_reply(200, json!({ "errors": [{ "message": "Entity not found: Issue - Could not find referenced Issue.", "extensions": { "type": "invalid input", "code": "INVALID_INPUT", "userError": true } }], "data": null })));
    let found: Found = ask(&s, call::ISSUES, json!({ "refs": ["ENG-404"] })).unwrap();
    assert_eq!((found.issues.len(), found.missing.as_slice()), (0, ["ENG-404".to_string()].as_slice()));
    let none: Found = ask(&s, call::ISSUES, json!({ "refs": [] })).unwrap();
    assert!(none.issues.is_empty());
    // More than 50 is more than a call takes.
    let many: Vec<String> = (1..=51).map(|n| format!("ENG-{n}")).collect();
    assert_eq!(ask::<Value>(&s, call::ISSUES, json!({ "refs": many })).unwrap_err().code, "bad_request");
}

#[test]
fn one_move_per_workflow_state_in_the_order_of_the_board_but_the_one_it_is_in() {
    let (r, s) = workspace();
    let moves: Moves = ask(&s, call::TRANSITIONS, json!({ "ref": "ENG-12" })).unwrap();
    let names: Vec<(&str, &str, &str)> = moves.transitions.iter().map(|m| (m.name.as_str(), m.to.stage.as_str(), m.to.column.as_str())).collect();
    assert_eq!(
        names,
        [("Triage", "todo", "todo"), ("Backlog", "todo", "todo"), ("Todo", "todo", "todo"), ("In Review", "doing", "doing"), ("Done", "done", "done"), ("Canceled", "done", "done"), ("Duplicate", "done", "done")],
        "by type -- triage, backlog, unstarted, started, completed, canceled -- then by position; In Progress, where it is, is left out"
    );
    assert!(moves.transitions.iter().all(|m| !m.local && m.id == m.to.id && !m.id.is_empty()));
    assert_eq!(moves.transitions[3].row(), "In Review  (started)");
    assert_eq!(moves.blocked, None);
    assert_eq!(r.variables_of("IssueStates"), [json!({ "id": "ENG-12" })]);

    // Linear has no Blocked state, and says so: the agenda keeps Blocked itself.
    assert!(!seam::about(&Settings::default()).blocked);
    assert!(moves.transitions.iter().all(|m| !m.local), "no faked local move");
}

#[test]
fn doing_a_move_sets_the_state_by_id_and_a_local_one_sends_nothing() {
    let (r, s) = workspace();
    let said: tracker::Said = ask(&s, call::TRANSITION, json!({ "ref": "ENG-12", "id": "57000000-0000-4000-8000-000000000005" })).unwrap();
    assert_eq!(said.said.as_deref(), Some("moved to In Review"), "the state Linear says it's in now");
    assert_eq!(r.operations(), ["IssueId", "UpdateIssue"]);
    assert_eq!(r.variables_of("UpdateIssue"), [json!({ "id": "1550e000-0000-4000-8000-000000000012", "input": { "stateId": "57000000-0000-4000-8000-000000000005" } })], "by its id, which was looked up once");
    // The next write to it doesn't look it up again.
    let _: Value = ask(&s, call::TRANSITION, json!({ "ref": "ENG-12", "id": "57000000-0000-4000-8000-000000000006" })).unwrap();
    assert_eq!(r.operations(), ["IssueId", "UpdateIssue", "UpdateIssue"]);
}

#[test]
fn update_changes_one_field_priorities_both_ways() {
    let (r, s) = workspace();
    let set = |set: Value| ask::<Value>(&s, call::UPDATE, json!({ "ref": "ENG-12", "set": set })).unwrap();
    set(json!({ "title": "A new title" }));
    set(json!({ "body": "Text, in **Markdown**" }));
    set(json!({ "priority": "Urgent" }));
    set(json!({ "priority": "no priority" }));
    set(json!({ "due": "2026-12-01" }));
    set(json!({ "due": null }));
    set(json!({ "assignee": { "id": "7a1c2b3d-0000-4000-8000-0000000000a2", "name": "Amy Park" } }));
    set(json!({ "assignee": null }));
    set(json!({ "labels": ["Bug", "docs"] }));
    let inputs: Vec<Value> = r.variables_of("UpdateIssue").into_iter().map(|v| v["input"].clone()).collect();
    assert_eq!(
        inputs,
        [
            json!({ "title": "A new title" }),
            json!({ "description": "Text, in **Markdown**" }),
            json!({ "priority": 1 }),
            json!({ "priority": 0 }),
            json!({ "dueDate": "2026-12-01" }),
            json!({ "dueDate": null }),
            json!({ "assigneeId": "7a1c2b3d-0000-4000-8000-0000000000a2" }),
            json!({ "assigneeId": null }),
            json!({ "labelIds": ["1abe1000-0000-4000-8000-000000000001", "1abe1000-0000-4000-8000-000000000002"] }),
        ]
    );
    // Every priority the seam offers is one update takes back, and means what its level says.
    let offered: tracker::Priorities = ask(&s, call::PRIORITIES, json!({ "ref": "ENG-12" })).unwrap();
    assert_eq!(offered.priorities.iter().map(|p| (p.name.as_str(), p.level.as_deref())).collect::<Vec<_>>(), [("Urgent", Some("urgent")), ("High", Some("high")), ("Medium", Some("medium")), ("Low", Some("low")), ("No priority", None)], "no priority has no level");
    for (n, p) in offered.priorities.iter().enumerate() {
        set(json!({ "priority": p.name }));
        let sent = r.variables_of("UpdateIssue").pop().unwrap();
        assert_eq!(sent["input"]["priority"], json!([1, 2, 3, 4, 0][n]), "{}", p.name);
    }
    let err = ask::<Value>(&s, call::UPDATE, json!({ "ref": "ENG-12", "set": { "priority": "Blocker" } })).unwrap_err();
    assert_eq!(err.code, "bad_request");
    let err = ask::<Value>(&s, call::UPDATE, json!({ "ref": "ENG-12", "set": { "labels": ["no such label"] } })).unwrap_err();
    assert_eq!(err.code, "bad_request", "a label that isn't there isn't made up");
    let err = ask::<Value>(&s, call::UPDATE, json!({ "ref": "ENG-12", "set": { "colour": "red" } })).unwrap_err();
    assert_eq!(err.code, "unsupported", "a field the seam doesn't have");
}

#[test]
fn people_and_comments() {
    let (r, s) = workspace();
    let people: tracker::People = ask(&s, call::PEOPLE, json!({ "ref": "ENG-12" })).unwrap();
    assert_eq!(people.people.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Jo Bloggs", "Amy Park"], "the team's active members");
    assert!(people.none.is_some());
    let _: Value = ask(&s, call::COMMENT, json!({ "ref": "ENG-12", "body": "Looks **good**" })).unwrap();
    assert_eq!(r.variables_of("CreateComment"), [json!({ "input": { "issueId": "1550e000-0000-4000-8000-000000000012", "body": "Looks **good**" } })]);
    let big = "x".repeat(64 * 1024 + 1);
    assert_eq!(ask::<Value>(&s, call::COMMENT, json!({ "ref": "ENG-12", "body": big })).unwrap_err().code, "bad_request");
    // A comment on what isn't an issue.
    assert_eq!(ask::<Value>(&s, call::COMMENT, json!({ "ref": "nonsense", "body": "x" })).unwrap_err().code, "not_found");
}

#[test]
fn errors_are_the_seams_whatever_their_status() {
    let (r, s) = workspace();
    let run = |s: &Seam| ask::<Page>(s, call::ASSIGNED, json!({})).unwrap_err();
    // A refused key, on a 200 with errors in it.
    r.on("AssignedIssues", err_reply(200, json!({ "errors": [{ "message": "Authentication required, not authenticated", "extensions": { "type": "authentication error", "code": "AUTHENTICATION_ERROR", "userPresentableMessage": "You need to authenticate to access this operation." } }], "data": null })));
    let e = run(&s);
    assert_eq!(e.code, "signed_out");
    assert!(e.message.contains("You need to authenticate") && e.message.contains("settings page (SPC ,), under Linear"), "{}", e.message);
    // The same on a 401 with no body.
    r.on("AssignedIssues", err_reply(401, json!(null)));
    assert_eq!(run(&s).code, "signed_out");
    // Forbidden.
    r.on("AssignedIssues", err_reply(200, json!({ "errors": [{ "message": "Forbidden", "extensions": { "code": "FORBIDDEN" } }] })));
    assert_eq!(run(&s).code, "refused");
    // Rate limited: Linear says so with a 400 and an error code, and the headers say when.
    r.on(
        "AssignedIssues",
        Reply { status: 400, headers: vec![("X-RateLimit-Requests-Remaining".into(), "0".into()), ("X-RateLimit-Requests-Reset".into(), "1800000120000".into())], body: json!({ "errors": [{ "message": "Rate limit exceeded", "extensions": { "type": "ratelimited", "code": "RATELIMITED" } }] }).to_string() },
    );
    let e = run(&s);
    assert_eq!((e.code.as_str(), e.retry_after), ("rate_limited", Some(120)));
    // The request didn't get there.
    r.reply(err_reply(0, json!(null)));
    r.fail("connection reset");
    let _ = run(&s);
    let e = run(&s);
    assert_eq!(e.code, "offline");
    // Something else.
    r.on("AssignedIssues", err_reply(200, json!({ "errors": [{ "message": "Internal server error" }] })));
    assert_eq!(run(&s).code, "failed");
    // A failure of a write is the same.
    r.on("UpdateIssue", err_reply(200, json!({ "data": { "issueUpdate": { "success": false, "issue": null } } })));
    let e = ask::<Value>(&s, call::UPDATE, json!({ "ref": "ENG-12", "set": { "title": "x" } })).unwrap_err();
    assert_eq!(e.code, "failed");
    r.on("IssueId", err_reply(200, json!({ "errors": [{ "message": "Entity not found: Issue", "extensions": { "code": "INVALID_INPUT" } }] })));
    assert_eq!(ask::<Value>(&s, call::COMMENT, json!({ "ref": "ENG-404", "body": "x" })).unwrap_err().code, "not_found");
}

/// What a title can't do: change the query. It goes as a variable, and the
/// query is the same text, byte for byte, whatever it holds.
#[test]
fn text_a_person_typed_never_reaches_a_query() {
    let r = Rc::new(Recorded::workspace(fixtures()));
    let s = seam_over(&r);
    let evil = "\"} } } mutation Pwn { issueDelete(id: \"x\") { success } } query { x(a: \"{ $b \\ \u{0000}";
    let _: Value = ask(&s, call::UPDATE, json!({ "ref": "ENG-12", "set": { "title": evil } })).unwrap();
    let _: Value = ask(&s, call::UPDATE, json!({ "ref": "ENG-12", "set": { "body": evil } })).unwrap();
    let _: Value = ask(&s, call::COMMENT, json!({ "ref": "ENG-12", "body": evil })).unwrap();
    let _ = futures_executor::block_on(linear(&r).create(&fenix_linear_client::linear::NewIssue { team_id: "t".into(), title: evil.into(), description: evil.into(), ..Default::default() })).unwrap();
    let filter = fenix_linear_client::filter::parse(evil).unwrap();
    let _ = futures_executor::block_on(linear(&r).list(&fenix_linear_client::View::Filter(filter), 10, Some(evil)));
    let _: Found = ask(&s, call::ISSUES, json!({ "refs": [evil, "ENG-1", "ENG-2"] })).unwrap();
    let mut seen = 0;
    for body in r.sent() {
        let sent: Value = serde_json::from_str(&body).unwrap();
        let query = sent["query"].as_str().unwrap();
        assert!(!query.contains("Pwn") && !query.contains("issueDelete"), "{query}");
        let known = fenix_linear_client::queries::ALL.iter().any(|(_, q)| *q == query);
        assert!(known, "a query that isn't one of the crate's: {query}");
        if sent["variables"].to_string().contains("Pwn") {
            seen += 1;
        }
    }
    assert!(seen >= 5, "the text went as variables ({seen})");
}

#[test]
fn a_new_issue_sends_only_what_was_filled_in() {
    let (r, _) = workspace();
    let l = linear(&r);
    let new = fenix_linear_client::linear::NewIssue { team_id: "team-1".into(), title: "A new thing".into(), ..Default::default() };
    let made = futures_executor::block_on(l.create(&new)).unwrap();
    assert_eq!((made.identifier.as_str(), made.url.as_deref()), ("ENG-21", Some("https://linear.app/acme/issue/ENG-21/new-thing")));
    let full = fenix_linear_client::linear::NewIssue {
        team_id: "team-1".into(),
        title: "T".into(),
        description: "D".into(),
        priority: Some(2),
        label_ids: vec!["l1".into(), "l2".into()],
        assignee_id: Some("u1".into()),
        state_id: Some("s1".into()),
    };
    futures_executor::block_on(l.create(&full)).unwrap();
    let sent = r.variables_of("CreateIssue");
    assert_eq!(sent[0], json!({ "input": { "teamId": "team-1", "title": "A new thing" } }));
    assert_eq!(sent[1], json!({ "input": { "teamId": "team-1", "title": "T", "description": "D", "priority": 2, "labelIds": ["l1", "l2"], "assigneeId": "u1", "stateId": "s1" } }));
    // The identifier it made is one a write to it needn't look up.
    futures_executor::block_on(l.comment("ENG-21", "hi")).unwrap();
    assert!(!r.operations().contains(&"IssueId".to_string()));
}

#[test]
fn the_teams_and_what_an_issue_in_one_can_have() {
    let (r, _) = workspace();
    let l = linear(&r);
    let teams = futures_executor::block_on(l.teams()).unwrap();
    assert_eq!(teams.iter().map(|t| t.key.as_str()).collect::<Vec<_>>(), ["ENG", "OPS"]);
    let meta = futures_executor::block_on(l.team_meta(&teams[0].id)).unwrap();
    assert_eq!(meta.states.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Triage", "Backlog", "Todo", "In Progress", "In Review", "Done", "Canceled", "Duplicate"], "in board order, whatever order they came");
    assert_eq!((meta.people.len(), meta.labels.len()), (2, 3));
    assert_eq!(r.variables_of("TeamMeta"), [json!({ "id": "5e000000-0000-4000-8000-0000000000e1" })]);
}

#[test]
fn an_issue_has_its_whole_page() {
    let (_, _) = workspace();
    let r = Rc::new(Recorded::workspace(fixtures()));
    let d = futures_executor::block_on(linear(&r).issue(&fenix_linear_client::refs::parse("ENG-12").unwrap())).unwrap();
    assert_eq!(d.cycle.as_deref(), Some("Cycle 12: Hardening"));
    assert_eq!((d.project_name.as_deref(), d.creator.as_deref()), (Some("Auth revamp"), Some("Amy Park")));
    assert_eq!(d.parent, Some(("ENG-3".into(), "Revamp login".into())));
    assert_eq!(d.children.iter().map(|c| (c.key.as_str(), c.stage.as_str())).collect::<Vec<_>>(), [("ENG-14", "todo"), ("ENG-13", "done")]);
    assert_eq!(d.branch, "jo/eng-12-fix-the-login-timeout");
    assert_eq!((d.priority, d.state_type.as_str(), d.team.key.as_str()), (2, "started", "ENG"));
}

#[test]
fn it_says_which_team_the_project_is_from_a_setting() {
    let r = Rc::new(Recorded::workspace(fixtures()));
    let s = Seam::new(linear(&r), Settings { team: Some("ENG".into()), ..Default::default() });
    let p: tracker::Project = ask(&s, call::PROJECT, json!({})).unwrap();
    assert_eq!(p.project.as_deref(), Some("ENG"));
    let (_, s) = workspace();
    let p: tracker::Project = ask(&s, call::PROJECT, json!({})).unwrap();
    assert_eq!(p.project, None, "a project that sets no team is in none");
    assert!(r.sent().is_empty(), "no request: it's a setting");
}

#[test]
fn a_cursor_and_state_ids_pass_through_untouched() {
    let (r, s) = workspace();
    let cursor = "  weird cursor with spaces == ";
    let _: Page = ask(&s, call::ASSIGNED, json!({ "cursor": cursor })).unwrap();
    assert_eq!(r.variables_of("AssignedIssues")[0]["after"], cursor, "byte for byte");
    let moves: Moves = ask(&s, call::TRANSITIONS, json!({ "ref": "ENG-12" })).unwrap();
    assert_eq!(moves.transitions[0].id, "57000000-0000-4000-8000-000000000001");
}

#[test]
fn maintenance_is_offline_and_a_gone_issue_is_not_found() {
    let (r, s) = workspace();
    for (status, want) in [(502, "offline"), (503, "offline"), (504, "offline"), (410, "not_found"), (500, "failed")] {
        r.on("AssignedIssues", err_reply(status, json!(null)));
        assert_eq!(ask::<Page>(&s, call::ASSIGNED, json!({})).unwrap_err().code, want, "{status}");
    }
}
