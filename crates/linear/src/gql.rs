//! One place every request to Linear goes through: a GraphQL POST whose
//! body is a query this crate wrote (a `&'static str`, so no text a
//! person typed can ever be part of it) and its variables (where all of
//! that text goes), and the answer read as the seam's errors.
//!
//! How it reaches Linear is a [`Transport`]: the extension's own HTTP,
//! with your key, or a recorded one in tests. The key is the transport's:
//! nothing here has it, so nothing here can write it into a message.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use fenix_tracker::{code, SeamError};
use serde_json::{json, Value};

/// Linear's GraphQL endpoint: the one host the extension may reach.
pub const ENDPOINT: &str = "https://api.linear.app/graphql";

/// What came back: the status, the headers (the rate limits are in them)
/// and the body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Reply {
    pub fn ok(body: impl Into<String>) -> Reply {
        Reply { status: 200, headers: Vec::new(), body: body.into() }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim())
    }
}

/// A request in flight: its answer, or why none came (`Err` is "the
/// request didn't reach Linear").
pub type Sent = Pin<Box<dyn Future<Output = Result<Reply, String>>>>;

/// How a request body reaches Linear's endpoint.
pub trait Transport {
    fn send(&self, body: String) -> Sent;
}

/// A client: cheap to clone, every clone sharing one transport.
#[derive(Clone)]
pub struct Client {
    transport: Rc<dyn Transport>,
    /// The time, in seconds since 1970, for the rate limit's reset.
    now: Rc<dyn Fn() -> i64>,
}

impl Client {
    pub fn new(transport: Rc<dyn Transport>, now: Rc<dyn Fn() -> i64>) -> Client {
        Client { transport, now }
    }

    /// `query` with `variables`, and the `data` it answered, or the seam's
    /// error. A response with `errors` is an error, whatever else it has
    /// and whatever its status: Linear answers a failed query with 200
    /// as often as with 400.
    pub async fn run(&self, query: &'static str, variables: Value) -> Result<Value, SeamError> {
        let body = json!({ "query": query, "variables": variables }).to_string();
        let reply = self.transport.send(body).await.map_err(|why| SeamError::new(code::OFFLINE, format!("couldn't reach Linear: {why}")))?;
        let parsed: Option<Value> = serde_json::from_str(&reply.body).ok();
        if let Some(errors) = parsed.as_ref().and_then(|v| v.get("errors")).and_then(Value::as_array).filter(|e| !e.is_empty()) {
            return Err(self.classify(&reply, errors));
        }
        if !(200..300).contains(&reply.status) {
            return Err(self.by_status(&reply));
        }
        match parsed.and_then(|mut v| v.get_mut("data").map(Value::take)) {
            Some(data) if !data.is_null() => Ok(data),
            Some(_) => Err(SeamError::new(code::FAILED, "Linear answered with no data")),
            None => Err(SeamError::new(code::FAILED, "Linear's answer wasn't a GraphQL response")),
        }
    }

    /// A failure with no GraphQL `errors` to read: by its status.
    fn by_status(&self, reply: &Reply) -> SeamError {
        let message = format!("HTTP {}", reply.status);
        let code = match reply.status {
            401 => code::SIGNED_OUT,
            403 => code::REFUSED,
            404 => code::NOT_FOUND,
            429 => code::RATE_LIMITED,
            400..=499 => code::REFUSED,
            _ => code::FAILED,
        };
        let mut err = SeamError::new(code, message);
        if code == code::RATE_LIMITED {
            err.retry_after = Some(self.retry_after(reply));
        }
        err
    }

    /// The first error that says what went wrong, worst first: a rate
    /// limit, then a refused key, then the rest.
    fn classify(&self, reply: &Reply, errors: &[Value]) -> SeamError {
        let rank = |e: &Value| match kind_of(e) {
            Kind::RateLimited => 0,
            Kind::SignedOut => 1,
            Kind::Forbidden => 2,
            Kind::NotFound => 3,
            Kind::BadRequest => 4,
            Kind::Refused => 5,
            Kind::Other => 6,
        };
        let worst = errors.iter().min_by_key(|e| rank(e)).unwrap_or(&errors[0]);
        let message = message_of(worst);
        let (c, retry) = match kind_of(worst) {
            Kind::RateLimited => (code::RATE_LIMITED, true),
            Kind::SignedOut => (code::SIGNED_OUT, false),
            Kind::Forbidden | Kind::Refused => (code::REFUSED, false),
            Kind::NotFound => (code::NOT_FOUND, false),
            Kind::BadRequest => (code::BAD_REQUEST, false),
            Kind::Other if reply.status == 401 => (code::SIGNED_OUT, false),
            Kind::Other if reply.status == 429 => (code::RATE_LIMITED, true),
            Kind::Other => (code::FAILED, false),
        };
        let mut err = SeamError::new(c, message);
        if retry || c == code::RATE_LIMITED {
            err.retry_after = Some(self.retry_after(reply));
        }
        err
    }

    /// Seconds to wait: `Retry-After` if Linear sent one, else until the
    /// request limit or the complexity limit resets, whichever it has
    /// used up (`X-RateLimit-*-Reset`, a time in seconds or in
    /// milliseconds since 1970), else a minute. At least 1, at most an
    /// hour.
    fn retry_after(&self, reply: &Reply) -> u64 {
        let now = (self.now)();
        let secs = |v: &str| v.parse::<i64>().ok();
        if let Some(wait) = reply.header("Retry-After").and_then(secs) {
            return wait.clamp(1, 3600) as u64;
        }
        let reset = |which: &str| -> Option<i64> {
            let at = reply.header(&format!("X-RateLimit-{which}-Reset")).and_then(secs)?;
            let at = if at > 100_000_000_000 { at / 1000 } else { at };
            Some(at - now)
        };
        let spent = |which: &str| reply.header(&format!("X-RateLimit-{which}-Remaining")).and_then(|v| v.parse::<f64>().ok()).is_some_and(|r| r < 1.0);
        let waits: Vec<i64> = ["Requests", "Complexity"].into_iter().filter(|w| spent(w)).filter_map(reset).collect();
        let waits = if waits.is_empty() { ["Requests", "Complexity"].into_iter().filter_map(reset).collect() } else { waits };
        waits.into_iter().max().unwrap_or(60).clamp(1, 3600) as u64
    }
}

enum Kind {
    RateLimited,
    SignedOut,
    Forbidden,
    NotFound,
    BadRequest,
    /// The change was refused: Linear's "invalid input".
    Refused,
    Other,
}

/// What one GraphQL error is, by its `extensions` (`code`, `type`) and,
/// for what Linear says only in words, its message.
fn kind_of(e: &Value) -> Kind {
    let ext = e.get("extensions");
    let field = |name: &str| ext.and_then(|x| x.get(name)).and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
    let (c, t) = (field("code"), field("type"));
    let message = e.get("message").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
    if c == "ratelimited" || t == "ratelimited" || t == "rate limited" || message.contains("rate limit") {
        Kind::RateLimited
    } else if c == "authentication_error" || t == "authentication error" || message.starts_with("authentication required") {
        Kind::SignedOut
    } else if c == "forbidden" || t == "forbidden" || message.starts_with("forbidden") {
        Kind::Forbidden
    } else if c == "not_found" || message.contains("entity not found") || message.contains("could not find") || message.contains("not found") {
        Kind::NotFound
    } else if c == "graphql_validation_failed" || c == "bad_user_input" || t == "graphql validation failed" {
        Kind::BadRequest
    } else if c == "invalid_input" || t == "invalid input" || ext.and_then(|x| x.get("userError")).and_then(Value::as_bool) == Some(true) {
        Kind::Refused
    } else {
        Kind::Other
    }
}

/// What to say of an error: Linear's own words for a person, when it has
/// them, else its message.
fn message_of(e: &Value) -> String {
    let said = e.get("extensions").and_then(|x| x.get("userPresentableMessage")).and_then(Value::as_str).filter(|m| !m.trim().is_empty());
    said.or_else(|| e.get("message").and_then(Value::as_str)).unwrap_or("Linear refused the request").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorded::Recorded;

    fn client(r: &Rc<Recorded>) -> Client {
        Client::new(r.clone(), Rc::new(|| 1_800_000_000))
    }

    fn fails(status: u16, headers: &[(&str, &str)], body: &str) -> SeamError {
        let r = Rc::new(Recorded::default());
        r.reply(Reply { status, headers: headers.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(), body: body.into() });
        futures_executor::block_on(client(&r).run("query Viewer { viewer { id } }", json!({}))).unwrap_err()
    }

    #[test]
    fn a_query_goes_with_its_variables_and_data_comes_back() {
        let r = Rc::new(Recorded::default());
        r.reply(Reply::ok(r#"{"data": {"viewer": {"id": "u1"}}}"#));
        let data = futures_executor::block_on(client(&r).run("query Viewer { viewer { id } }", json!({"a": 1}))).unwrap();
        assert_eq!(data["viewer"]["id"], "u1");
        let sent: Value = serde_json::from_str(&r.sent()[0]).unwrap();
        assert_eq!(sent, json!({"query": "query Viewer { viewer { id } }", "variables": {"a": 1}}));
    }

    #[test]
    fn errors_come_with_a_200_too() {
        let e = fails(200, &[], r#"{"errors": [{"message": "Authentication required, not authenticated", "extensions": {"type": "authentication error", "code": "AUTHENTICATION_ERROR", "userPresentableMessage": "You need to authenticate to access this operation."}}], "data": null}"#);
        assert_eq!((e.code.as_str(), e.message.as_str()), ("signed_out", "You need to authenticate to access this operation."));
        let e = fails(200, &[], r#"{"errors": [{"message": "Entity not found: Issue - Could not find referenced Issue.", "extensions": {"type": "invalid input", "code": "INVALID_INPUT", "userError": true}}], "data": null}"#);
        assert_eq!(e.code, "not_found");
        let e = fails(200, &[], r#"{"errors": [{"message": "Forbidden", "extensions": {"code": "FORBIDDEN"}}]}"#);
        assert_eq!(e.code, "refused");
        let e = fails(400, &[], r#"{"errors": [{"message": "title must be shorter than or equal to 255 characters", "extensions": {"type": "invalid input", "userError": true}}]}"#);
        assert_eq!((e.code.as_str(), e.message.as_str()), ("refused", "title must be shorter than or equal to 255 characters"));
        let e = fails(400, &[], r#"{"errors": [{"message": "Cannot query field \"nope\" on type \"Issue\".", "extensions": {"code": "GRAPHQL_VALIDATION_FAILED"}}]}"#);
        assert_eq!(e.code, "bad_request");
        let e = fails(200, &[], r#"{"errors": [{"message": "Something broke"}]}"#);
        assert_eq!(e.code, "failed");
        // The worst of several is the one that's said.
        let e = fails(200, &[], r#"{"errors": [{"message": "odd"}, {"message": "slow down", "extensions": {"code": "RATELIMITED"}}]}"#);
        assert_eq!(e.code, "rate_limited");
    }

    #[test]
    fn a_rate_limit_says_how_long_from_the_headers() {
        let limited = r#"{"errors": [{"message": "Rate limit exceeded", "extensions": {"code": "RATELIMITED", "type": "ratelimited"}}]}"#;
        // Retry-After wins.
        let e = fails(400, &[("Retry-After", "42")], limited);
        assert_eq!((e.code.as_str(), e.retry_after), ("rate_limited", Some(42)));
        // Else the limit that's used up, its reset a time in milliseconds...
        let headers = [("X-RateLimit-Requests-Remaining", "0"), ("X-RateLimit-Requests-Reset", "1801090000"), ("X-RateLimit-Complexity-Remaining", "900"), ("X-RateLimit-Complexity-Reset", "1800000030000")];
        // ...or in seconds, as Linear's documentation has both ways.
        assert_eq!(fails(400, &headers, limited).retry_after, Some(3600), "(1,090,000 s later, held to an hour)");
        let headers = [("x-ratelimit-requests-remaining", "0"), ("x-ratelimit-requests-reset", "1800000090000"), ("x-ratelimit-complexity-remaining", "900"), ("x-ratelimit-complexity-reset", "1800000030000")];
        assert_eq!(fails(400, &headers, limited).retry_after, Some(90), "in milliseconds, the used-up one, not the other");
        let headers = [("x-ratelimit-requests-remaining", "0"), ("x-ratelimit-requests-reset", "1800000075")];
        assert_eq!(fails(400, &headers, limited).retry_after, Some(75), "in seconds");
        // Nothing to go by: a minute.
        assert_eq!(fails(400, &[], limited).retry_after, Some(60));
        // A bare 429 is one too.
        let e = fails(429, &[("Retry-After", "7")], "slow down");
        assert_eq!((e.code.as_str(), e.retry_after), ("rate_limited", Some(7)));
    }

    #[test]
    fn a_failure_with_no_graphql_in_it_is_read_by_its_status() {
        for (status, want) in [(401, "signed_out"), (403, "refused"), (404, "not_found"), (400, "refused"), (502, "failed"), (500, "failed")] {
            let e = fails(status, &[], "<html>bad gateway</html>");
            assert_eq!((e.code.as_str(), e.message.as_str()), (want, format!("HTTP {status}").as_str()));
        }
        let e = fails(200, &[], "not json");
        assert_eq!(e.code, "failed");
        let e = fails(200, &[], r#"{"data": null}"#);
        assert_eq!(e.code, "failed");
    }

    #[test]
    fn no_answer_is_offline() {
        let r = Rc::new(Recorded::default());
        r.fail("connection refused");
        let e = futures_executor::block_on(client(&r).run("query Viewer { viewer { id } }", json!({}))).unwrap_err();
        assert_eq!((e.code.as_str(), e.message.as_str()), ("offline", "couldn't reach Linear: connection refused"));
    }
}
