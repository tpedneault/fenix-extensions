//! A [`Transport`] answering from recorded responses, for tests of this
//! crate, the extension and the agenda: a response is chosen by the
//! operation's name (`query Issue(...)`) and, when asked, by a needle in
//! its variables, so a cursor's second page is told from its first. Every
//! body sent is kept.
//!
//! The fixtures in `tests/fixtures` are **not recordings**: no Linear
//! workspace was at hand. They are written from Linear's GraphQL schema
//! (`tests/schema/README.md` says which) and its documentation, in the
//! shapes a real answer has.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;

use crate::gql::{Reply, Sent, Transport};

struct Route {
    operation: String,
    needle: Option<String>,
    reply: Result<Reply, String>,
}

#[derive(Default)]
pub struct Recorded {
    routes: RefCell<Vec<Route>>,
    queue: RefCell<VecDeque<Result<Reply, String>>>,
    sent: RefCell<Vec<String>>,
}

/// `Issue` from `query Issue($id: String!) { ... }`.
pub fn operation_of(body: &str) -> String {
    let query = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| v.get("query").and_then(|q| q.as_str().map(str::to_string))).unwrap_or_default();
    let mut words = query.split_whitespace();
    words.next();
    words.next().map(|n| n.split(['(', '{']).next().unwrap_or("").to_string()).unwrap_or_default()
}

impl Recorded {
    /// The next request, whatever it is, is answered with `reply` (once).
    pub fn reply(&self, reply: Reply) -> &Self {
        self.queue.borrow_mut().push_back(Ok(reply));
        self
    }

    /// The next request doesn't reach Linear.
    pub fn fail(&self, why: &str) -> &Self {
        self.queue.borrow_mut().push_back(Err(why.into()));
        self
    }

    /// Every request for `operation` is answered with `reply`, unless a
    /// later route names it too.
    pub fn on(&self, operation: &str, reply: Reply) -> &Self {
        self.routes.borrow_mut().push(Route { operation: operation.into(), needle: None, reply: Ok(reply) });
        self
    }

    /// As `on`, for a request whose variables hold `needle`.
    pub fn on_with(&self, operation: &str, needle: &str, reply: Reply) -> &Self {
        self.routes.borrow_mut().push(Route { operation: operation.into(), needle: Some(needle.into()), reply: Ok(reply) });
        self
    }

    /// Every body sent, in order.
    pub fn sent(&self) -> Vec<String> {
        self.sent.borrow().clone()
    }

    /// The operations asked, in order.
    pub fn operations(&self) -> Vec<String> {
        self.sent.borrow().iter().map(|b| operation_of(b)).collect()
    }

    /// The variables of every request for `operation`.
    pub fn variables_of(&self, operation: &str) -> Vec<serde_json::Value> {
        self.sent.borrow().iter().filter(|b| operation_of(b) == operation).filter_map(|b| serde_json::from_str::<serde_json::Value>(b).ok()).map(|v| v["variables"].clone()).collect()
    }

    /// Every route's operation and the body it answers with, for a test of
    /// the fixtures themselves.
    pub fn replies(&self) -> Vec<(String, String)> {
        self.routes.borrow().iter().filter_map(|r| r.reply.as_ref().ok().map(|reply| (r.operation.clone(), reply.body.clone()))).collect()
    }

    /// A workspace answering from the fixtures in `dir`: you, your issues
    /// (two pages), `ENG-12` and its states, the teams, and every write
    /// accepted.
    pub fn workspace(dir: &Path) -> Recorded {
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let r = Recorded::default();
        r.on("Viewer", Reply::ok(read("viewer.json")))
            .on("AssignedIssues", Reply::ok(read("assigned-0.json")))
            .on_with("AssignedIssues", "\"after\":\"cursor-2\"", Reply::ok(read("assigned-1.json")))
            .on("Issues", Reply::ok(read("issues-list.json")))
            .on("Issue", Reply::ok(read("issue-ENG-12.json")))
            .on("IssueStates", Reply::ok(read("states-ENG-12.json")))
            .on("IssuePeople", Reply::ok(read("people-ENG-12.json")))
            .on("Teams", Reply::ok(read("teams.json")))
            .on("TeamMeta", Reply::ok(read("team-meta.json")))
            .on("IssueId", Reply::ok(read("issue-id.json")))
            .on("UpdateIssue", Reply::ok(read("update-issue.json")))
            .on("CreateComment", Reply::ok(read("comment-created.json")))
            .on("CreateIssue", Reply::ok(read("issue-created.json")));
        r
    }
}

impl Transport for Recorded {
    fn send(&self, body: String) -> Sent {
        self.sent.borrow_mut().push(body.clone());
        let queued = self.queue.borrow_mut().pop_front();
        let answer = match queued {
            Some(answer) => answer,
            None => {
                let operation = operation_of(&body);
                let routes = self.routes.borrow();
                // A needle that matches beats one that isn't asked, and a
                // later route beats an earlier.
                let found = routes.iter().rev().filter(|r| r.operation == operation).find(|r| r.needle.as_ref().is_some_and(|n| body.contains(n.as_str()))).or_else(|| routes.iter().rev().find(|r| r.operation == operation && r.needle.is_none()));
                match found {
                    Some(route) => route.reply.clone(),
                    None => Err(format!("nothing recorded for {operation}")),
                }
            }
        };
        Box::pin(async move { answer })
    }
}
