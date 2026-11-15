//! A [`Transport`] answering from recorded responses, for tests: of this
//! crate, and of the agenda, which compares what it made of Jira before
//! the tracker seam with what it makes of it through the seam, on the
//! same responses (`tests/fixtures`, Jira Server 8's shapes).
//!
//! `tests/fixtures/cloud` is Jira Cloud's, and they are *not* recordings:
//! no Cloud site was reachable when they were made. They're written from
//! the shapes Atlassian documents for REST v2 and v3 (the enhanced
//! search's `nextPageToken` and `isLast`, account ids, ADF bodies), and
//! are worth replacing with real ones from a site.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use crate::{Reply, Transport};

/// Answers each request with the last recorded response whose method is
/// its own and whose `needle` is in its path; writes it all down.
#[derive(Default)]
pub struct Recorded {
    routes: RefCell<Vec<(String, String, u16, String)>>,
    /// Routes whose answer says to wait (`Retry-After`), by needle.
    waits: RefCell<Vec<(String, u32)>>,
    /// What the last answer's `Retry-After` said.
    retry_after: Cell<Option<u32>>,
    /// Every request, as `METHOD path` and the body sent.
    pub sent: RefCell<Vec<(String, Option<String>)>>,
}

impl Recorded {
    /// `method` on a path with `needle` in it answers `status` and `body`.
    pub fn on(&self, method: &str, needle: &str, status: u16, body: impl Into<String>) -> &Self {
        self.routes.borrow_mut().push((method.into(), needle.into(), status, body.into()));
        self
    }

    /// `on`, its answer saying `Retry-After: seconds`.
    pub fn on_wait(&self, method: &str, needle: &str, status: u16, body: impl Into<String>, seconds: u32) -> &Self {
        self.waits.borrow_mut().push((needle.into(), seconds));
        self.on(method, needle, status, body)
    }

    /// The requests sent, as `METHOD path`.
    pub fn asked(&self) -> Vec<String> {
        self.sent.borrow().iter().map(|(r, _)| r.clone()).collect()
    }

    /// A Jira Server answering from the fixtures in `dir`: `/myself`,
    /// `/field`, a search for linked issues (by key or id) and one for yours (two
    /// pages), `PROJ-1` and its transitions, the priorities, `NOPE-1`
    /// not there, and every write accepted.
    pub fn jira_server(dir: &Path) -> Recorded {
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let r = Recorded::default();
        r.on("GET", "/rest/api/2/myself", 200, read("myself.json"))
            .on("GET", "/rest/api/2/field", 200, read("field.json"))
            .on("GET", "/rest/api/2/search?jql=key%20IN", 200, read("search-linked.json"))
            .on("GET", "/rest/api/2/search?jql=id%20IN", 200, read("search-linked.json"))
            .on("GET", "/rest/api/2/search?jql=assignee", 200, read("search-assigned-0.json"))
            .on("GET", "&startAt=2&", 200, read("search-assigned-2.json"))
            .on("GET", "/rest/api/2/issue/PROJ-1?", 200, read("issue-PROJ-1.json"))
            .on("GET", "/rest/api/2/issue/PROJ-1/transitions", 200, read("transitions-PROJ-1.json"))
            // PROJ-1 by its id, 10201, as the agenda names it once known.
            .on("GET", "/rest/api/2/issue/10201?", 200, read("issue-PROJ-1.json"))
            .on("GET", "/rest/api/2/issue/10201/transitions", 200, read("transitions-PROJ-1.json"))
            .on("GET", "/rest/api/2/issue/NOPE-1", 404, read("not-found.json"))
            .on("GET", "/rest/api/2/priority", 200, read("priority.json"))
            .on("POST", "/rest/api/2/", 204, "")
            .on("PUT", "/rest/api/2/", 204, "");
        r
    }
}

impl Recorded {
    /// A Jira Cloud site answering from the fixtures in `dir` (written
    /// from Atlassian's documented shapes, not recorded): `serverInfo`,
    /// `/myself`, `/field`, the enhanced search for linked issues and for
    /// yours (two pages, by token), `PROJ-1` (v3, rich text) and its
    /// transitions, the priorities, who can be assigned, `NOPE-1` not
    /// there, a comment made, and every other write accepted.
    pub fn jira_cloud(dir: &Path) -> Recorded {
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let r = Recorded::default();
        r.on("GET", "/rest/api/2/serverInfo", 200, read("server-info.json"))
            .on("GET", "/rest/api/2/myself", 200, read("myself.json"))
            .on("GET", "/rest/api/2/field", 200, read("field.json"))
            .on("GET", "/rest/api/3/search/jql?jql=key%20IN", 200, read("search-jql-linked.json"))
            .on("GET", "/rest/api/3/search/jql?jql=id%20IN", 200, read("search-jql-linked.json"))
            .on("GET", "/rest/api/3/search/jql?jql=assignee", 200, read("search-jql-assigned-0.json"))
            .on("GET", "&nextPageToken=Ch0jU3RyaW5nJlVGSlBTZz09JUludCZOakF3&", 200, read("search-jql-assigned-1.json"))
            .on("GET", "/rest/api/3/issue/PROJ-1?", 200, read("issue-PROJ-1.json"))
            .on("GET", "/rest/api/2/issue/PROJ-1/transitions", 200, read("transitions-PROJ-1.json"))
            .on("GET", "/rest/api/3/issue/10201?", 200, read("issue-PROJ-1.json"))
            .on("GET", "/rest/api/2/issue/10201/transitions", 200, read("transitions-PROJ-1.json"))
            .on("GET", "/rest/api/3/issue/NOPE-1", 404, read("not-found.json"))
            .on("GET", "/rest/api/3/priority/search", 200, read("priority-search.json"))
            .on("GET", "/rest/api/3/user/assignable/search", 200, read("assignable.json"))
            .on("POST", "/rest/api/", 204, "")
            .on("PUT", "/rest/api/", 204, "")
            .on("POST", "/comment", 201, read("comment-created.json"));
        r
    }
}

impl Transport for Recorded {
    fn send(&self, method: &str, path: &str, body: Option<String>) -> Pin<Box<dyn Future<Output = Reply>>> {
        self.sent.borrow_mut().push((format!("{method} {path}"), body));
        let route = self.routes.borrow().iter().rev().find(|(m, needle, _, _)| m == method && path.contains(needle.as_str())).cloned();
        self.retry_after.set(route.as_ref().and_then(|(_, needle, _, _)| self.waits.borrow().iter().rev().find(|(n, _)| n == needle).map(|(_, s)| *s)));
        let found = route.map(|(_, _, status, body)| (status, body));
        let reply = found.ok_or_else(|| format!("request failed: nothing recorded for {method} {path}"));
        Box::pin(async move { reply })
    }

    fn retry_after(&self) -> Option<u32> {
        self.retry_after.get()
    }
}
