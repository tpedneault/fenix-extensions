//! `{{variables}}`, filled in: the file's (`@name = value`), the
//! environment's, the keychain's secrets; system ones (`{{$uuid}}`,
//! `{{$timestamp}}`, `{{$randomInt 1 10}}`, `{{$datetime iso8601}}`,
//! `{{$dotenv NAME}}`, `{{$secret name}}`, `{{$processEnv NAME}}`);
//! another request's answer
//! (`{{login.response.body.$.token}}`, `{{login.response.headers.X-Id}}`);
//! and an OAuth token (`{{$auth.token("id")}}`).
//!
//! A value may hold variables itself; they're filled in too, ten deep at
//! most, so one that names itself is an error rather than a hang.

use crate::json;
use crate::model::Exchange;

/// What filling a variable in needs from outside.
pub trait Context {
    /// A file variable.
    fn file_var(&self, name: &str) -> Option<String>;
    /// The environment's, `$shared`'s with it.
    fn env_var(&self, name: &str) -> Option<String>;
    /// From the keychain (`http.secret.<name>`), or the older secret
    /// `http.secrets`.
    fn secret(&self, name: &str) -> Option<String>;
    /// The last answer to the request called `name`.
    fn response(&self, name: &str) -> Option<&Exchange>;
    /// From the project's `.env`.
    fn dotenv(&self, name: &str) -> Option<String>;
    /// The token OAuth configuration `id` has now, when it's good.
    fn token(&self, id: &str) -> Option<String>;
    /// Now, in milliseconds since 1970, UTC.
    fn now_millis(&self) -> i64;
    /// One of Fenix's environment variables, one `http.env` names.
    fn process_env(&self, _name: &str) -> Option<String> {
        None
    }
    /// The machine's offset from UTC, in seconds east.
    fn utc_offset(&self) -> i32 {
        0
    }
    fn random(&mut self) -> u64;
}

/// Why a variable couldn't be filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// Nothing has it.
    Variable(String),
    /// The request `name` hasn't been sent (or has no answer): send it
    /// first.
    Response(String),
    /// OAuth configuration `id` has no good token: get one first.
    Token(String),
    Error(String),
}

impl Missing {
    pub fn said(&self) -> String {
        match self {
            Missing::Variable(n) => format!("{{{{{n}}}}} isn't set: not in the file, the environment or the secrets"),
            Missing::Response(n) => format!("request {n} hasn't been sent"),
            Missing::Token(id) => format!("no token for {id} yet"),
            Missing::Error(e) => e.clone(),
        }
    }
}

const DEPTH: usize = 10;

/// The `{{...}}` in `text`: each one's inside, trimmed, and its range in
/// bytes.
pub fn find(text: &str) -> Vec<(String, std::ops::Range<usize>)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find("{{").map(|i| i + from) {
        let Some(close) = text[open + 2..].find("}}").map(|i| i + open + 2) else { break };
        out.push((text[open + 2..close].trim().to_string(), open..close + 2));
        from = close + 2;
    }
    out
}

/// `text`, every `{{variable}}` in it filled in.
pub fn resolve(text: &str, cx: &mut dyn Context) -> Result<String, Missing> {
    resolve_depth(text, cx, 0)
}

fn resolve_depth(text: &str, cx: &mut dyn Context, depth: usize) -> Result<String, Missing> {
    if depth > DEPTH {
        return Err(Missing::Error(format!("variables nest too deep (one names itself?): {text}")));
    }
    let found = find(text);
    if found.is_empty() {
        return Ok(text.to_string());
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (inner, range) in found {
        out.push_str(&text[last..range.start]);
        out.push_str(&value(&inner, cx, depth)?);
        last = range.end;
    }
    out.push_str(&text[last..]);
    Ok(out)
}

fn value(inner: &str, cx: &mut dyn Context, depth: usize) -> Result<String, Missing> {
    if let Some(system) = inner.strip_prefix('$') {
        return system_var(system, cx);
    }
    if let Some((request, rest)) = chained(inner) {
        return chain(request, rest, cx);
    }
    let raw = cx.file_var(inner).or_else(|| cx.env_var(inner)).or_else(|| cx.secret(inner)).ok_or_else(|| Missing::Variable(inner.to_string()))?;
    let raw = unquote(&raw);
    resolve_depth(&raw, cx, depth + 1)
}

/// A file variable's value, its quotes taken off: `@name = "ada"` is ada.
fn unquote(v: &str) -> String {
    let t = v.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') && !t[1..t.len() - 1].contains('"') {
        t[1..t.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

/// `login.response.body.$.token`: the request, and what of it.
fn chained(inner: &str) -> Option<(&str, &str)> {
    for part in [".response.", ".request."] {
        if let Some(i) = inner.find(part) {
            let name = &inner[..i];
            if !name.is_empty() && !name.contains(char::is_whitespace) {
                return Some((name, &inner[i + 1..]));
            }
        }
    }
    None
}

fn chain(request: &str, what: &str, cx: &mut dyn Context) -> Result<String, Missing> {
    let exchange = cx.response(request).ok_or_else(|| Missing::Response(request.to_string()))?;
    let (side, rest) = what.split_once('.').ok_or_else(|| Missing::Error(format!("{request}.{what}: say body or headers")))?;
    let (part, path) = rest.split_once('.').unwrap_or((rest, ""));
    let (headers, body): (&[(String, String)], &[u8]) = match side {
        "request" => (&exchange.sent.headers, &exchange.sent.body),
        _ => match &exchange.answer {
            Ok(r) => (&r.headers, &r.body),
            Err(e) => return Err(Missing::Error(format!("request {request} failed: {e}"))),
        },
    };
    match part {
        "headers" => crate::model::header(headers, path).map(str::to_string).ok_or_else(|| Missing::Error(format!("{request}'s {side} has no header {path}"))),
        "body" => {
            let text = String::from_utf8_lossy(body);
            if path.is_empty() || path == "*" {
                return Ok(text.into_owned());
            }
            if !path.starts_with('$') {
                return Err(Missing::Error(format!("{request}.{what}: a body's part is a JSONPath ($.token); XPath isn't read")));
            }
            let doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| Missing::Error(format!("{request}'s body isn't JSON: {e}")))?;
            match json::query_first(&doc, path) {
                Ok(Some(v)) => Ok(crate::env::as_text(&v)),
                Ok(None) => Err(Missing::Error(format!("{path} finds nothing in {request}'s body"))),
                Err(e) => Err(Missing::Error(e)),
            }
        }
        other => Err(Missing::Error(format!("{request}.{side}.{other}: say body or headers"))),
    }
}

/// Words after a system variable's name, split as a shell would but on
/// spaces and commas, quotes taken off.
fn words(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in args.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => quote = Some(c),
            (None, ' ' | ',' | '(' | ')') => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// An offset after a time variable: `-1 d`, `2 h`, in seconds.
fn offset(args: &[String]) -> Result<i64, Missing> {
    let Some(n) = args.first() else { return Ok(0) };
    let n: i64 = n.parse().map_err(|_| Missing::Error(format!("{n}: not a number of units")))?;
    let unit = args.get(1).map(String::as_str).unwrap_or("s");
    let secs = match unit {
        "ms" => return Ok(n / 1000),
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        "w" => 604_800,
        "M" => 2_592_000,
        "y" => 31_536_000,
        u => return Err(Missing::Error(format!("{u}: a unit is ms, s, m, h, d, w, M or y"))),
    };
    Ok(n * secs)
}

pub fn uuid(cx: &mut dyn Context) -> String {
    let (a, b) = (cx.random(), cx.random());
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&a.to_le_bytes());
    bytes[8..].copy_from_slice(&b.to_le_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

fn system_var(text: &str, cx: &mut dyn Context) -> Result<String, Missing> {
    let (name, args) = match text.find([' ', '(']) {
        Some(i) => (&text[..i], &text[i..]),
        None => (text, ""),
    };
    let args = words(args);
    let now = cx.now_millis();
    let at = |cx: &dyn Context, args: &[String], local: bool| -> Result<fenix_extension_api::chrono::DateTime<fenix_extension_api::chrono::FixedOffset>, Missing> {
        use fenix_extension_api::chrono::{FixedOffset, TimeZone, Utc};
        let secs = now / 1000 + offset(args)?;
        let utc = Utc.timestamp_opt(secs, ((now % 1000) * 1_000_000) as u32).single().ok_or_else(|| Missing::Error("time out of range".into()))?;
        let off = FixedOffset::east_opt(if local { cx.utc_offset() } else { 0 }).unwrap_or(FixedOffset::east_opt(0).unwrap());
        Ok(utc.with_timezone(&off))
    };
    match name {
        "guid" | "uuid" | "random.uuid" => Ok(uuid(cx)),
        "timestamp" => Ok((now / 1000 + offset(&args)?).to_string()),
        "isoTimestamp" => Ok(at(cx, &args, false)?.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()),
        "datetime" | "localDatetime" => {
            let local = name == "localDatetime";
            let (format, rest) = match args.split_first() {
                Some((f, rest)) => (f.as_str(), rest),
                None => ("iso8601", &args[..]),
            };
            let t = at(cx, rest, local)?;
            Ok(match format {
                "iso8601" => t.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string(),
                "rfc1123" => t.with_timezone(&fenix_extension_api::chrono::Utc).format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
                custom => t.format(&strftime(custom)).to_string(),
            })
        }
        "randomInt" | "random.integer" => {
            let lo: i64 = args.first().and_then(|a| a.parse().ok()).unwrap_or(0);
            let hi: i64 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(1000);
            if hi <= lo {
                return Err(Missing::Error(format!("$randomInt {lo} {hi}: the second is past the first")));
            }
            Ok((lo + (cx.random() % (hi - lo) as u64) as i64).to_string())
        }
        "random.alphabetic" | "random.alphanumeric" | "random.hexadecimal" => {
            let n: usize = args.first().and_then(|a| a.parse().ok()).unwrap_or(8);
            let set: &[u8] = match name {
                "random.alphabetic" => b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ",
                "random.alphanumeric" => b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
                _ => b"0123456789abcdef",
            };
            Ok((0..n).map(|_| set[(cx.random() % set.len() as u64) as usize] as char).collect())
        }
        "random.email" => Ok(format!("{}@example.com", (0..10).map(|_| (b'a' + (cx.random() % 26) as u8) as char).collect::<String>())),
        "dotenv" => {
            let key = args.first().ok_or_else(|| Missing::Error("$dotenv: name the variable".into()))?;
            cx.dotenv(key).ok_or_else(|| Missing::Error(format!("$dotenv {key}: not in the project's .env")))
        }
        "secret" => {
            let key = args.first().ok_or_else(|| Missing::Error("$secret: name it".into()))?;
            cx.secret(key).ok_or_else(|| Missing::Error(format!("$secret {key}: not in the secrets (HTTP: set a secret)")))
        }
        "auth.token" | "auth.idToken" => {
            let id = args.first().ok_or_else(|| Missing::Error("$auth.token: name the configuration, $auth.token(\"id\")".into()))?;
            cx.token(id).ok_or_else(|| Missing::Token(id.clone()))
        }
        "processEnv" => {
            let arg = args.first().ok_or_else(|| Missing::Error("$processEnv: name the variable".into()))?;
            // `%name`: the variable's name is what the environment's
            // `name` says (REST Client's).
            let name = match arg.strip_prefix('%') {
                Some(var) => cx.env_var(var).or_else(|| cx.file_var(var)).ok_or_else(|| Missing::Error(format!("$processEnv %{var}: {var} isn't in the environment")))?,
                None => arg.clone(),
            };
            cx.process_env(&name).ok_or_else(|| Missing::Error(format!("$processEnv {name}: not set, or not one http.env lets it read")))
        }
        other => Err(Missing::Error(format!("${other}: no such system variable"))),
    }
}

/// A Moment-style format, as REST Client takes (`YYYY-MM-DD`), in
/// chrono's terms.
fn strftime(f: &str) -> String {
    let f = f.trim_matches(['"', '\'']);
    let mut out = f.to_string();
    for (from, to) in [("YYYY", "%Y"), ("YY", "%y"), ("MM", "%m"), ("DD", "%d"), ("HH", "%H"), ("hh", "%I"), ("mm", "%M"), ("ss", "%S"), ("SSS", "%3f"), ("A", "%p")] {
        out = out.replace(from, to);
    }
    out
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::model::{Received, Sent};
    use std::collections::HashMap;

    #[derive(Default)]
    pub struct Cx {
        pub file: HashMap<String, String>,
        pub env: HashMap<String, String>,
        pub secrets: HashMap<String, String>,
        pub responses: HashMap<String, Exchange>,
        pub tokens: HashMap<String, String>,
        pub seed: u64,
    }

    impl Context for Cx {
        fn file_var(&self, n: &str) -> Option<String> {
            self.file.get(n).cloned()
        }
        fn env_var(&self, n: &str) -> Option<String> {
            self.env.get(n).cloned()
        }
        fn secret(&self, n: &str) -> Option<String> {
            self.secrets.get(n).cloned()
        }
        fn response(&self, n: &str) -> Option<&Exchange> {
            self.responses.get(n)
        }
        fn dotenv(&self, n: &str) -> Option<String> {
            (n == "PORT").then(|| "8080".into())
        }
        fn process_env(&self, n: &str) -> Option<String> {
            (n == "HOME" || n == "API_TOKEN_DEV").then(|| format!("<{n}>"))
        }
        fn token(&self, id: &str) -> Option<String> {
            self.tokens.get(id).cloned()
        }
        fn now_millis(&self) -> i64 {
            1_700_000_000_123
        }
        fn random(&mut self) -> u64 {
            self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.seed >> 11
        }
    }

    pub fn exchange(body: &str, headers: &[(&str, &str)]) -> Exchange {
        Exchange {
            sent: Sent { method: "POST".into(), url: "https://x.test/login".into(), headers: vec![("Content-Type".into(), "application/json".into())], body: br#"{"user":"ada"}"#.to_vec() },
            answer: Ok(Received { status: 200, headers: headers.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), body: body.as_bytes().to_vec(), url: String::new(), timing: Default::default() }),
            at: 0,
            file: None,
            name: Some("login".into()),
            env: None,
        }
    }

    fn cx() -> Cx {
        let mut cx = Cx::default();
        cx.file.insert("host".into(), "{{scheme}}://{{domain}}".into());
        cx.file.insert("user".into(), "\"ada\"".into());
        cx.env.insert("scheme".into(), "https".into());
        cx.env.insert("domain".into(), "api.example.com".into());
        cx.env.insert("domain2".into(), "{{domain}}".into());
        cx.secrets.insert("password".into(), "hunter2".into());
        cx.file.insert("loop".into(), "{{loop}}".into());
        cx
    }

    #[test]
    fn variables_are_filled_in_from_the_file_the_env_and_the_secrets() {
        let mut cx = cx();
        assert_eq!(resolve("{{host}}/users?u={{ user }}", &mut cx), Ok("https://api.example.com/users?u=ada".into()));
        assert_eq!(resolve("pw={{password}}", &mut cx), Ok("pw=hunter2".into()));
        assert_eq!(resolve("{{nope}}", &mut cx), Err(Missing::Variable("nope".into())));
        assert!(matches!(resolve("{{loop}}", &mut cx), Err(Missing::Error(e)) if e.contains("too deep")));
        assert_eq!(resolve("no variables", &mut cx), Ok("no variables".into()));
        assert_eq!(resolve("{{unclosed", &mut cx), Ok("{{unclosed".into()));
    }

    #[test]
    fn a_chained_request_is_read_with_jsonpath() {
        let mut cx = cx();
        assert_eq!(resolve("Bearer {{login.response.body.$.token}}", &mut cx), Err(Missing::Response("login".into())));
        cx.responses.insert("login".into(), exchange(r#"{"token":"t0k","user":{"id":7,"roles":["a","b"]}}"#, &[("X-Request-Id", "r-1")]));
        assert_eq!(resolve("Bearer {{login.response.body.$.token}}", &mut cx), Ok("Bearer t0k".into()));
        assert_eq!(resolve("{{login.response.body.$.user.id}}", &mut cx), Ok("7".into()));
        assert_eq!(resolve("{{login.response.body.$.user.roles[1]}}", &mut cx), Ok("b".into()));
        assert_eq!(resolve("{{login.response.body.$.user.roles}}", &mut cx), Ok(r#"["a","b"]"#.into()));
        assert_eq!(resolve("{{login.response.headers.x-request-id}}", &mut cx), Ok("r-1".into()));
        assert_eq!(resolve("{{login.request.body.$.user}}", &mut cx), Ok("ada".into()));
        assert_eq!(resolve("{{login.request.headers.Content-Type}}", &mut cx), Ok("application/json".into()));
        assert!(resolve("{{login.response.body.*}}", &mut cx).unwrap().starts_with("{\"token\""));
        assert!(matches!(resolve("{{login.response.body.$.missing}}", &mut cx), Err(Missing::Error(e)) if e.contains("finds nothing")));
        assert!(matches!(resolve("{{login.response.body.//token}}", &mut cx), Err(Missing::Error(e)) if e.contains("XPath")));
    }

    #[test]
    fn system_variables() {
        let mut cx = cx();
        let id = resolve("{{$uuid}}", &mut cx).unwrap();
        assert_eq!((id.len(), &id[14..15]), (36, "4"), "{id}");
        assert_eq!(resolve("{{$timestamp}}", &mut cx), Ok("1700000000".into()));
        assert_eq!(resolve("{{$timestamp -1 d}}", &mut cx), Ok("1699913600".into()));
        assert_eq!(resolve("{{$isoTimestamp}}", &mut cx), Ok("2023-11-14T22:13:20.123Z".into()));
        assert_eq!(resolve("{{$datetime rfc1123}}", &mut cx), Ok("Tue, 14 Nov 2023 22:13:20 GMT".into()));
        assert_eq!(resolve("{{$datetime \"YYYY-MM-DD\" 1 d}}", &mut cx), Ok("2023-11-15".into()));
        let n: i64 = resolve("{{$randomInt 5 10}}", &mut cx).unwrap().parse().unwrap();
        assert!((5..10).contains(&n));
        let n: i64 = resolve("{{$random.integer(1, 3)}}", &mut cx).unwrap().parse().unwrap();
        assert!((1..3).contains(&n));
        assert_eq!(resolve("{{$random.alphabetic(6)}}", &mut cx).unwrap().len(), 6);
        assert_eq!(resolve("{{$dotenv PORT}}", &mut cx), Ok("8080".into()));
        assert_eq!(resolve("{{$secret password}}", &mut cx), Ok("hunter2".into()));
        assert_eq!(resolve("{{$auth.token(\"api\")}}", &mut cx), Err(Missing::Token("api".into())));
        cx.tokens.insert("api".into(), "abc".into());
        assert_eq!(resolve("Bearer {{$auth.token(\"api\")}}", &mut cx), Ok("Bearer abc".into()));
        assert!(resolve("{{$nope}}", &mut cx).is_err());
        assert_eq!(resolve("{{$processEnv HOME}}", &mut cx), Ok("<HOME>".into()));
        assert!(resolve("{{$processEnv PATH}}", &mut cx).unwrap_err().said().contains("http.env"));
        cx.env.insert("tokenVar".into(), "API_TOKEN_DEV".into());
        assert_eq!(resolve("{{$processEnv %tokenVar}}", &mut cx), Ok("<API_TOKEN_DEV>".into()), "its name from the environment");
        assert!(resolve("{{$processEnv %nope}}", &mut cx).unwrap_err().said().contains("isn't in the environment"));
    }
}
