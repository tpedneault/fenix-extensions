//! An [`Http`] answering from responses written down beforehand, for
//! tests: of this crate's issue clients and tracker seam, and of the
//! extensions and the agenda that drive them.

use std::cell::RefCell;
use std::path::Path;

use crate::{Http, Response};

/// One request as it was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

impl Sent {
    /// `METHOD url`.
    pub fn line(&self) -> String {
        format!("{} {}", self.method, self.url)
    }
}

type Route = (String, String, Response);

/// Answers each request with the last response given for its method
/// and a `needle` in its URL; writes down every request.
#[derive(Default)]
pub struct Recorded {
    routes: RefCell<Vec<Route>>,
    pub sent: RefCell<Vec<Sent>>,
}

impl Recorded {
    /// `method` on a URL with `needle` in it answers `status` and `body`.
    pub fn on(&self, method: &str, needle: &str, status: u16, body: impl Into<String>) -> &Self {
        self.on_with(method, needle, status, &[], body)
    }

    /// The same, with headers.
    pub fn on_with(&self, method: &str, needle: &str, status: u16, headers: &[(&str, &str)], body: impl Into<String>) -> &Self {
        let response = Response { status, headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), body: body.into() };
        self.routes.borrow_mut().push((method.into(), needle.into(), response));
        self
    }

    /// `method` on `needle` answers fixture `name` from `dir`.
    pub fn fixture(&self, dir: &Path, method: &str, needle: &str, name: &str) -> &Self {
        self.fixture_with(dir, method, needle, name, &[])
    }

    pub fn fixture_with(&self, dir: &Path, method: &str, needle: &str, name: &str, headers: &[(&str, &str)]) -> &Self {
        let body = std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        self.on_with(method, needle, 200, headers, body)
    }

    /// The requests sent, as `METHOD url`.
    pub fn asked(&self) -> Vec<String> {
        self.sent.borrow().iter().map(Sent::line).collect()
    }

    /// The last request sent.
    pub fn last(&self) -> Option<Sent> {
        self.sent.borrow().last().cloned()
    }
}

impl Http for Recorded {
    async fn request(&self, method: &str, url: &str, headers: &[(&str, String)], body: Option<String>) -> Result<Response, String> {
        self.sent.borrow_mut().push(Sent { method: method.into(), url: url.into(), headers: headers.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(), body });
        let found = self.routes.borrow().iter().rev().find(|(m, needle, _)| m == method && url.contains(needle.as_str())).map(|(_, _, r)| r.clone());
        found.ok_or_else(|| format!("nothing recorded for {method} {url}"))
    }
}
