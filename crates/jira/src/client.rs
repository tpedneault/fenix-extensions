//! A Jira client over a [`Transport`]: the one place every request goes
//! through, so the same calls run in the Jira extension (its own HTTP,
//! with your server and token) and in the agenda (asking the Jira
//! extension, which holds them).

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

/// What came back for a request: its status and body.
pub type Reply = Result<(u16, String), String>;

/// How a request reaches Jira. `path` starts at `/rest/...`, its query
/// already on it; `body` is the JSON sent, for a write.
pub trait Transport {
    fn send(&self, method: &str, path: &str, body: Option<String>) -> Pin<Box<dyn Future<Output = Reply>>>;
}

/// A Jira client: cheap to clone, every clone sharing one transport.
#[derive(Clone)]
pub struct JiraClient {
    transport: Rc<dyn Transport>,
}

/// `value` as it goes in a URL's query.
pub fn encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Jira's own words for a failed request, when its body has them.
fn describe(status: u16, body: &str) -> String {
    let said = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| {
        let mut messages: Vec<String> = v.get("errorMessages").and_then(|m| m.as_array()).into_iter().flatten().filter_map(|m| m.as_str().map(str::to_string)).collect();
        if let Some(errors) = v.get("errors").and_then(|e| e.as_object()) {
            messages.extend(errors.iter().map(|(k, v)| format!("{k}: {}", v.as_str().unwrap_or_default())));
        }
        (!messages.is_empty()).then(|| messages.join("; "))
    });
    match said {
        Some(said) => format!("HTTP {status} ({said})"),
        None => format!("HTTP {status}"),
    }
}

impl JiraClient {
    pub fn new(transport: Rc<dyn Transport>) -> Self {
        JiraClient { transport }
    }

    /// `GET {path}?query...`, its JSON body.
    pub(crate) async fn request(&self, path: &str, query: &[(&str, &str)]) -> Result<serde_json::Value, String> {
        let mut full = path.to_string();
        for (i, (key, value)) in query.iter().enumerate() {
            full.push(if i == 0 { '?' } else { '&' });
            full.push_str(&format!("{}={}", encode(key), encode(value)));
        }
        let (status, body) = self.transport.send("GET", &full, None).await?;
        if !(200..300).contains(&status) {
            return Err(describe(status, &body));
        }
        serde_json::from_str(&body).map_err(|err| format!("couldn't parse response as JSON: {err}"))
    }

    /// `method` (`POST`, `PUT`) of `body` to `path`. Jira answers many
    /// writes with an empty 204: that's `Ok(None)`.
    pub(crate) async fn send(&self, method: &str, path: &str, body: &serde_json::Value) -> Result<Option<serde_json::Value>, String> {
        let payload = serde_json::to_string(body).map_err(|err| format!("couldn't encode request body: {err}"))?;
        let (status, text) = self.transport.send(method, path, Some(payload)).await?;
        if !(200..300).contains(&status) {
            return Err(describe(status, &text));
        }
        if text.trim().is_empty() {
            return Ok(None);
        }
        serde_json::from_str(&text).map(Some).map_err(|err| format!("couldn't parse response as JSON: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers every request with `body`, noting what was asked.
    struct Fake {
        asked: std::cell::RefCell<Vec<String>>,
        body: &'static str,
    }

    impl Transport for Fake {
        fn send(&self, method: &str, path: &str, _body: Option<String>) -> Pin<Box<dyn Future<Output = Reply>>> {
            self.asked.borrow_mut().push(format!("{method} {path}"));
            let body = self.body.to_string();
            Box::pin(async move { Ok((200, body)) })
        }
    }

    #[test]
    fn a_call_goes_through_the_transport() {
        let fake = Rc::new(Fake { asked: Default::default(), body: r#"{"name": "tp"}"# });
        let client = JiraClient::new(fake.clone());
        assert_eq!(futures_executor::block_on(client.myself()), Ok("tp".to_string()));
        let search = futures_executor::block_on(client.search_issues("project = FEN", 5));
        assert!(search.is_err(), "not a search response");
        assert_eq!(fake.asked.borrow()[0], "GET /rest/api/2/myself");
        assert!(fake.asked.borrow()[1].starts_with("GET /rest/api/2/search?jql=project%20%3D%20FEN&maxResults=5"), "{:?}", fake.asked.borrow());
    }

    #[test]
    fn a_query_is_encoded() {
        assert_eq!(encode("key IN (\"A-1\")"), "key%20IN%20%28%22A-1%22%29");
    }

    #[test]
    fn jiras_own_words_are_kept() {
        assert_eq!(describe(400, r#"{"errorMessages":["Issue does not exist"],"errors":{}}"#), "HTTP 400 (Issue does not exist)");
        assert_eq!(describe(401, "<html>"), "HTTP 401");
    }
}
