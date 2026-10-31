//! A request of a `.http` file made ready to send: its variables filled
//! in, its body built, its `Authorization` encoded, and how it's sent --
//! a timeout, redirects, certificates, streamed or not -- from its
//! directives and the settings.

use crate::body;
use crate::model::Sent;
use crate::parse::Request;
use crate::vars::{self, Context, Missing};

/// How it's sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opts {
    pub timeout_ms: u32,
    pub follow_redirects: bool,
    pub insecure: bool,
    pub stream: bool,
    /// Cookies sent from the environment's jar, and kept from the answer.
    pub cookie_jar: bool,
    pub log: bool,
}

/// What the settings say, before a request's directives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defaults {
    pub timeout_ms: u32,
    pub follow_redirects: bool,
    pub insecure: bool,
}

impl Default for Defaults {
    fn default() -> Defaults {
        Defaults { timeout_ms: 30_000, follow_redirects: true, insecure: false }
    }
}

/// `# @timeout 5`, `5 s`, `500 ms`, `2 m`: in milliseconds.
pub fn timeout(text: &str) -> Option<u32> {
    let t = text.trim();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let n: f64 = digits.parse().ok()?;
    let unit = t[digits.len()..].trim();
    let ms = match unit {
        "" | "s" | "sec" | "seconds" => n * 1000.0,
        "ms" => n,
        "m" | "min" | "minutes" => n * 60_000.0,
        _ => return None,
    };
    Some(ms.min(u32::MAX as f64) as u32)
}

/// Fills in `text`, its errors as they come.
fn fill(text: &str, cx: &mut dyn Context) -> Result<String, Missing> {
    vars::resolve(text, cx)
}

/// Request `r` of `file`, ready: `read` reads a file its body names, by
/// the path written (relative to the `.http` file).
pub fn prepare(r: &Request, cx: &mut dyn Context, read: &mut dyn FnMut(&str) -> Result<Vec<u8>, String>, defaults: &Defaults) -> Result<(Sent, Opts), Missing> {
    let mut url = fill(&r.url, cx)?;
    let mut headers: Vec<(String, String)> = Vec::new();
    for h in &r.headers {
        let value = fill(&h.value, cx)?;
        let value = if h.name.eq_ignore_ascii_case("authorization") { crate::auth::authorization(&value) } else { value };
        headers.push((h.name.clone(), value));
    }
    // REST Client's GraphQL: a header says so, which isn't sent; JetBrains'
    // is a method of its own.
    let graphql = headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("x-request-type") && v.eq_ignore_ascii_case("graphql")) || r.method == "GRAPHQL";
    headers.retain(|(k, v)| !(k.eq_ignore_ascii_case("x-request-type") && v.eq_ignore_ascii_case("graphql")));
    let method = if r.method == "GRAPHQL" { "POST".to_string() } else { r.method.clone() };
    // `GET /path` with a `Host` header.
    if url.starts_with('/') {
        if let Some(host) = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("host")).map(|(_, v)| v.clone()) {
            let scheme = if host.ends_with(":443") { "https" } else { "http" };
            url = format!("{scheme}://{}{url}", host.trim_end_matches(":443"));
        }
    } else if !url.contains("://") {
        url = format!("http://{url}");
    }
    let mut body_bytes = Vec::new();
    if let (Some(b), true) = (&r.body, r.method == "WEBSOCKET") {
        // Its messages, `===` between them: filled in, sent as they are.
        body_bytes = fill(&b.text, cx)?.into_bytes();
    } else if let Some(b) = &r.body {
        let ct = headers.iter().rev().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.clone()).unwrap_or_default();
        let mut failed: Option<Missing> = None;
        let built = {
            let mut fill_text = |t: &str| -> Result<String, String> {
                vars::resolve(t, cx).map_err(|m| {
                    let said = m.said();
                    failed = Some(m);
                    said
                })
            };
            body::build(&ct, graphql, &b.text, &mut fill_text, read)
        };
        match built {
            Ok(built) => {
                if let Some(ct) = built.content_type {
                    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
                        headers.push(("Content-Type".into(), ct));
                    }
                }
                body_bytes = built.bytes;
            }
            Err(e) => return Err(failed.unwrap_or(Missing::Error(e))),
        }
    }
    let timeout_ms = r.directive("timeout").and_then(|d| timeout(&d.value)).unwrap_or(defaults.timeout_ms);
    let stream = r.has("stream") || headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("accept") && v.contains("text/event-stream"));
    let opts = Opts {
        timeout_ms,
        follow_redirects: defaults.follow_redirects && !r.has("no-redirect"),
        insecure: defaults.insecure || r.has("insecure"),
        stream,
        cookie_jar: !r.has("no-cookie-jar"),
        log: !r.has("no-log"),
    };
    Ok((Sent { method, url, headers, body: body_bytes }, opts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::HttpFile;
    use crate::vars::tests::{exchange, Cx};

    fn read(p: &str) -> Result<Vec<u8>, String> {
        if p == "./a.bin" {
            Ok(vec![1, 2, 3])
        } else {
            Err(format!("{p}: no such file"))
        }
    }

    fn prep(text: &str, cx: &mut Cx) -> Result<(Sent, Opts), Missing> {
        let f = HttpFile::parse(text);
        let r = f.requests[0].clone();
        prepare(&r, cx, &mut read, &Defaults::default())
    }

    fn cx() -> Cx {
        let mut cx = Cx::default();
        cx.env.insert("host".into(), "https://api.test".into());
        cx.env.insert("user".into(), "ada".into());
        cx.secrets.insert("pw".into(), "s3cret".into());
        cx
    }

    #[test]
    fn a_request_is_filled_in_and_encoded() {
        let mut cx = cx();
        let (s, o) = prep("# @timeout 5\n# @no-redirect\nPOST {{host}}/login\nAuthorization: Basic {{user}}:{{pw}}\nContent-Type: application/json\n\n{\"u\": \"{{user}}\"}\n", &mut cx).unwrap();
        assert_eq!((s.method.as_str(), s.url.as_str()), ("POST", "https://api.test/login"));
        assert_eq!(s.header("authorization"), Some("Basic YWRhOnMzY3JldA=="));
        assert_eq!(s.body, b"{\"u\": \"ada\"}");
        assert_eq!((o.timeout_ms, o.follow_redirects, o.insecure, o.stream, o.cookie_jar, o.log), (5000, false, false, false, true, true));
    }

    #[test]
    fn what_stops_it_says_what_it_needs() {
        let mut cx = cx();
        assert_eq!(prep("GET {{host}}/me\nAuthorization: Bearer {{login.response.body.$.token}}\n", &mut cx).unwrap_err(), Missing::Response("login".into()));
        cx.responses.insert("login".into(), exchange(r#"{"token":"t"}"#, &[]));
        assert_eq!(prep("GET {{host}}/me\nAuthorization: Bearer {{login.response.body.$.token}}\n", &mut cx).unwrap().0.header("authorization"), Some("Bearer t"));
        assert_eq!(prep("GET {{host}}\nAuthorization: Bearer {{$auth.token(\"api\")}}\n", &mut cx).unwrap_err(), Missing::Token("api".into()));
        assert_eq!(prep("POST {{host}}\nContent-Type: text/plain\n\n{{nope}}\n", &mut cx).unwrap_err(), Missing::Variable("nope".into()), "from the body too");
        assert!(matches!(prep("POST {{host}}\n\n< ./missing.bin\n", &mut cx).unwrap_err(), Missing::Error(e) if e.contains("no such file")));
    }

    #[test]
    fn graphql_hosts_and_streams() {
        let mut cx = cx();
        let (s, _) = prep("POST {{host}}/graphql\nX-Request-Type: GraphQL\n\nquery { me { id } }\n", &mut cx).unwrap();
        assert_eq!(s.headers, [("Content-Type".to_string(), "application/json".to_string())], "the X-Request-Type isn't sent");
        assert_eq!(s.body, br#"{"query":"query { me { id } }"}"#);
        let (s, _) = prep("GRAPHQL {{host}}/graphql\n\n{ me { id } }\n", &mut cx).unwrap();
        assert_eq!(s.method, "POST");
        let (s, _) = prep("GET /status\nHost: example.com:443\n", &mut cx).unwrap();
        assert_eq!(s.url, "https://example.com/status");
        let (s, _) = prep("GET /status\nHost: localhost:8080\n", &mut cx).unwrap();
        assert_eq!(s.url, "http://localhost:8080/status");
        let (s, o) = prep("GET example.com/events\nAccept: text/event-stream\n", &mut cx).unwrap();
        assert_eq!((s.url.as_str(), o.stream), ("http://example.com/events", true));
        let (s, _) = prep("POST {{host}}\nContent-Type: application/octet-stream\n\n< ./a.bin\n", &mut cx).unwrap();
        assert_eq!(s.body, [1, 2, 3]);
        let (_, o) = prep("# @no-cookie-jar\n# @no-log\n# @insecure\n# @stream\nGET {{host}}\n", &mut cx).unwrap();
        assert_eq!((o.cookie_jar, o.log, o.insecure, o.stream), (false, false, true, true));
    }

    #[test]
    fn a_websockets_messages_are_filled_in_and_kept_as_written() {
        let mut cx = cx();
        let text = "WEBSOCKET ws://{{host}}/ws\nContent-Type: application/json\n\n===\n{\"pw\": \"{{pw}}\"}\n=== wait-for-server\nnext\n";
        let (s, _) = prep(&text.replace("{{host}}", "localhost:8080"), &mut cx).unwrap();
        assert_eq!((s.method.as_str(), s.url.as_str()), ("WEBSOCKET", "ws://localhost:8080/ws"));
        assert_eq!(String::from_utf8(s.body).unwrap(), "===\n{\"pw\": \"s3cret\"}\n=== wait-for-server\nnext");
        assert!(matches!(prep("WEBSOCKET ws://h.test\n\n===\n{{nope}}\n", &mut cx), Err(Missing::Variable(_))));
    }

    /// The example collection: each request ready, or saying what it
    /// waits for.
    #[test]
    fn the_examples_are_ready() {
        let envs = crate::env::Environments::parse(Some(include_str!("../examples/http-client.env.json")), Some(include_str!("../examples/http-client.private.env.json"))).unwrap();
        assert_eq!(envs.names(), ["dev", "prod"]);
        let mut cx = Cx::default();
        for (k, v, _) in envs.vars(Some("dev")) {
            cx.env.insert(k, v);
        }
        let f = HttpFile::parse(include_str!("../examples/users.http"));
        assert_eq!(f.requests.len(), 8);
        let mut any = |_: &str| Ok(b"x".to_vec());
        let mut ready = Vec::new();
        for r in &f.requests {
            ready.push(match prepare(r, &mut cx, &mut any, &Defaults::default()) {
                Ok((s, o)) => format!("{} {}{}", s.method, s.url, if o.stream { " stream" } else { "" }),
                Err(m) => m.said(),
            });
        }
        assert_eq!(ready[0], "GET https://httpbin.org/get?page=1&limit=10");
        assert_eq!(ready[2], "request login hasn't been sent");
        assert_eq!(ready[6], "no token for httpbin yet");
        assert_eq!(ready[7], "GET https://sse.dev/test stream");
        assert!(ready.iter().all(|r| !r.contains("isn't set")), "{ready:?}");
        assert_eq!(f.requests[0].tests().count(), 2);
    }

    #[test]
    fn timeouts_read() {
        assert_eq!(timeout("5"), Some(5000));
        assert_eq!(timeout("2.5 s"), Some(2500));
        assert_eq!(timeout("500 ms"), Some(500));
        assert_eq!(timeout("2 m"), Some(120_000));
        assert_eq!(timeout("soon"), None);
    }
}
