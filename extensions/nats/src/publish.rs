//! A message to send, written in a scratch buffer as JSON -- its subject,
//! headers, reply subject and payload -- and a message captured, as that
//! JSON again, as a `nats pub` command line, or as a line of JSONL.
//!
//! ```json
//! {
//!   "subject": "orders.new",
//!   "headers": { "Nats-Msg-Id": "42" },
//!   "payload": { "id": 42 }
//! }
//! ```
//!
//! A payload that's a JSON string is sent as its text; any other JSON
//! value as compact JSON; `payload_base64` sends bytes as they are.

use fenix_nats::headers::{HeaderMap, Headers};
use serde_json::{json, Map, Value};

use crate::payload::{self, Kind};
use crate::store::View;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub subject: String,
    pub reply: Option<String>,
    pub headers: HeaderMap,
    pub payload: Vec<u8>,
    /// Which connection: its `host:port`, or the page's.
    pub server: Option<String>,
}

/// The payload as the buffer shows it: JSON as JSON, text as a string,
/// anything else in base64.
fn payload_field(bytes: &[u8], out: &mut Map<String, Value>) {
    match payload::sniff(bytes) {
        Kind::Json => match serde_json::from_slice::<Value>(bytes) {
            Ok(v) => {
                out.insert("payload".into(), v);
            }
            Err(_) => {
                out.insert("payload".into(), Value::String(String::from_utf8_lossy(bytes).into_owned()));
            }
        },
        Kind::Text | Kind::Empty if std::str::from_utf8(bytes).is_ok() => {
            out.insert("payload".into(), Value::String(String::from_utf8_lossy(bytes).into_owned()));
        }
        _ => {
            out.insert("payload_base64".into(), Value::String(fenix_nats::b64::encode(bytes)));
        }
    }
}

/// The buffer for a message to send.
pub fn envelope(subject: &str, reply: Option<&str>, headers: &[(String, String)], body: &[u8], server: Option<&str>) -> String {
    let mut m = Map::new();
    m.insert("subject".into(), Value::String(subject.into()));
    if let Some(s) = server {
        m.insert("server".into(), Value::String(s.into()));
    }
    let mut h = Map::new();
    for (k, v) in headers {
        match h.get_mut(k) {
            Some(Value::Array(a)) => a.push(Value::String(v.clone())),
            Some(old) => *old = Value::Array(vec![old.clone(), Value::String(v.clone())]),
            None => {
                h.insert(k.clone(), Value::String(v.clone()));
            }
        }
    }
    m.insert("headers".into(), Value::Object(h));
    m.insert("reply".into(), Value::String(reply.unwrap_or("").into()));
    payload_field(body, &mut m);
    serde_json::to_string_pretty(&Value::Object(m)).unwrap_or_default()
}

/// A new message's buffer.
pub fn blank(subject: &str, server: Option<&str>) -> String {
    envelope(subject, None, &[], b"{\"hello\": \"world\"}", server)
}

/// A captured message as a buffer to edit and send.
pub fn from_view(v: &View<'_>, server: Option<&str>) -> String {
    let headers: Vec<(String, String)> = v.headers.and_then(|h| Headers::parse(h).ok()).map(|h| h.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()).unwrap_or_default();
    envelope(v.subject, v.reply, &headers, v.payload, server)
}

/// Reads the buffer.
pub fn parse(text: &str) -> Result<Draft, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("the buffer isn't JSON: {e}"))?;
    let o = v.as_object().ok_or("the buffer is a JSON object: subject, headers, reply, payload")?;
    let subject = o.get("subject").and_then(Value::as_str).unwrap_or("").trim().to_string();
    fenix_nats::subject::check_subject(&subject)?;
    let reply = o.get("reply").and_then(Value::as_str).map(str::trim).filter(|r| !r.is_empty()).map(str::to_string);
    let mut headers = HeaderMap::new();
    if let Some(h) = o.get("headers") {
        let h = h.as_object().ok_or("headers is an object: { \"Name\": \"value\" }")?;
        for (k, v) in h {
            match v {
                Value::String(s) => headers.insert(k, s),
                Value::Array(a) => {
                    for x in a {
                        headers.insert(k, &x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string()));
                    }
                }
                other => headers.insert(k, &other.to_string()),
            }
        }
    }
    headers.check()?;
    let payload = match (o.get("payload_base64"), o.get("payload")) {
        (Some(Value::String(b)), _) => fenix_nats::b64::decode(b)?,
        (_, Some(Value::String(s))) => s.clone().into_bytes(),
        (_, Some(Value::Null)) | (_, None) => Vec::new(),
        (_, Some(v)) => serde_json::to_vec(v).unwrap_or_default(),
    };
    let server = o.get("server").and_then(Value::as_str).map(str::to_string).filter(|s| !s.is_empty());
    Ok(Draft { subject, reply, headers, payload, server })
}

/// `s` in single quotes, for a shell.
fn quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-/:=@>*$".contains(c)) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The message as a `nats pub` command line that sends it again.
pub fn nats_pub(v: &View<'_>, server_url: Option<&str>) -> String {
    let mut cmd = vec!["nats".to_string(), "pub".to_string()];
    if let Some(url) = server_url {
        cmd.push("-s".into());
        cmd.push(quote(url));
    }
    if let Some(h) = v.headers.and_then(|h| Headers::parse(h).ok()) {
        for (k, val) in h.iter() {
            cmd.push("-H".into());
            cmd.push(quote(&format!("{k}:{val}")));
        }
    }
    if let Some(r) = v.reply {
        cmd.push("--reply".into());
        cmd.push(quote(r));
    }
    cmd.push(quote(v.subject));
    if payload::sniff(v.payload) == Kind::Binary {
        // Binary through a shell: from base64.
        return format!("echo {} | base64 -d | {} --force-stdin", fenix_nats::b64::encode(v.payload), cmd.join(" "));
    }
    cmd.push(quote(&String::from_utf8_lossy(v.payload)));
    cmd.join(" ")
}

/// The message as one line of JSONL.
pub fn jsonl(v: &View<'_>) -> String {
    let mut m = Map::new();
    m.insert("subject".into(), Value::String(v.subject.into()));
    m.insert("time".into(), Value::String(payload::rfc3339(v.time_ms)));
    m.insert("size".into(), json!(v.entry.size));
    if let Some(r) = v.reply {
        m.insert("reply".into(), Value::String(r.into()));
    }
    if let Some(h) = v.headers.and_then(|h| Headers::parse(h).ok()) {
        let mut hm = Map::new();
        for (k, val) in h.iter() {
            hm.insert(k.into(), Value::String(val.into()));
        }
        if let Some(s) = h.status {
            m.insert("status".into(), json!(s));
        }
        m.insert("headers".into(), Value::Object(hm));
    }
    payload_field(v.payload, &mut m);
    if v.entry.truncated {
        m.insert("truncated".into(), Value::Bool(true));
    }
    serde_json::to_string(&Value::Object(m)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Limits, Store};

    fn with(subject: &str, reply: Option<&str>, headers: Option<&[u8]>, payload: &[u8], f: impl FnOnce(&View<'_>)) {
        let mut s = Store::new(Limits::default(), 1_700_000_000_000);
        let id = s.subject(0, subject, 0);
        s.count(id, payload.len(), 0);
        let seq = s.store(id, reply, headers, 1, payload, 1_700_000_000_000);
        f(&s.get(id, seq).unwrap());
    }

    #[test]
    fn a_captured_message_round_trips_through_the_buffer() {
        with("orders.new", Some("_INBOX.r"), Some(b"NATS/1.0\r\nNats-Msg-Id: 7\r\nX: a\r\nX: b\r\n\r\n"), br#"{"id":7}"#, |v| {
            let text = from_view(v, Some("localhost:4222"));
            let d = parse(&text).unwrap();
            assert_eq!(d.subject, "orders.new");
            assert_eq!(d.reply.as_deref(), Some("_INBOX.r"));
            assert_eq!(d.payload, br#"{"id":7}"#);
            assert_eq!(d.headers.entries, [("Nats-Msg-Id".to_string(), "7".to_string()), ("X".to_string(), "a".to_string()), ("X".to_string(), "b".to_string())]);
            assert_eq!(d.server.as_deref(), Some("localhost:4222"));
        });
        with("bin", None, None, &[0, 1, 2, 255], |v| {
            let d = parse(&from_view(v, None)).unwrap();
            assert_eq!(d.payload, [0, 1, 2, 255]);
        });
        with("text", None, None, b"plain words", |v| {
            assert_eq!(parse(&from_view(v, None)).unwrap().payload, b"plain words");
        });
    }

    #[test]
    fn a_bad_buffer_says_why() {
        assert!(parse("nope").is_err());
        assert!(parse(r#"{"subject":"a.*"}"#).unwrap_err().contains("wildcards"));
        assert!(parse(r#"{"subject":"a","headers":{"bad name":"x"}}"#).is_err());
        assert_eq!(parse(r#"{"subject":"a","payload":[1,2]}"#).unwrap().payload, b"[1,2]");
        assert_eq!(parse(&blank("x.y", None)).unwrap().payload, br#"{"hello":"world"}"#);
    }

    #[test]
    fn command_lines_and_jsonl() {
        with("orders.new", None, Some(b"NATS/1.0\r\nK: it's\r\n\r\n"), b"it's here", |v| {
            assert_eq!(nats_pub(v, Some("nats://localhost:4222")), r#"nats pub -s nats://localhost:4222 -H 'K:it'\''s' orders.new 'it'\''s here'"#);
            let line: Value = serde_json::from_str(&jsonl(v)).unwrap();
            assert_eq!(line["subject"], "orders.new");
            assert_eq!(line["headers"]["K"], "it's");
            assert_eq!(line["payload"], "it's here");
            assert_eq!(line["time"], "2023-11-14T22:13:20.000Z");
        });
        with("b", None, None, &[0, 255], |v| {
            assert!(nats_pub(v, None).starts_with("echo AP8= | base64 -d | nats pub b"));
            assert!(jsonl(v).contains("payload_base64"));
        });
    }
}
