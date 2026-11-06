//! What coding agents may ask: a sample of a subject's messages, the
//! subjects and their rates (both read), and a publish (which runs). Each
//! answer is short JSON: payloads cut at a few KiB, at most 50 messages.

use serde_json::{json, Value};

use crate::core::{Core, Owned};
use crate::payload::{self, Kind};

/// What a sample asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleArgs {
    pub subject: String,
    pub count: usize,
    pub server: Option<String>,
    /// How long to wait for more on the server when the store has too
    /// few; 0 answers from the store at once.
    pub wait_ms: u64,
}

pub const MAX_SAMPLE: usize = 50;
const PAYLOAD_CUT: usize = 4096;

fn args(payload: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(if payload.trim().is_empty() { "{}" } else { payload }).map_err(|e| format!("arguments: {e}"))?;
    if !v.is_object() {
        return Err("arguments are an object".into());
    }
    Ok(v)
}

pub fn sample_args(payload: &str) -> Result<SampleArgs, String> {
    let v = args(payload)?;
    let subject = v["subject"].as_str().unwrap_or(">").trim().to_string();
    fenix_nats::subject::check_pattern(&subject)?;
    Ok(SampleArgs {
        subject,
        count: v["count"].as_u64().unwrap_or(10).clamp(1, MAX_SAMPLE as u64) as usize,
        server: v["server"].as_str().map(str::to_string),
        wait_ms: v["wait_ms"].as_u64().unwrap_or(3000).min(10_000),
    })
}

/// A message as an agent reads it.
pub fn message(subject: &str, reply: Option<&str>, headers: Option<&[u8]>, body: &[u8], time_ms: u64, size: usize) -> Value {
    let mut m = json!({ "subject": subject, "time": payload::rfc3339(time_ms), "size": size });
    if let Some(r) = reply {
        m["reply"] = json!(r);
    }
    if let Some(h) = headers.and_then(|h| fenix_nats::headers::Headers::parse(h).ok()) {
        let hm: serde_json::Map<String, Value> = h.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        m["headers"] = Value::Object(hm);
    }
    let cut = body.len() > PAYLOAD_CUT;
    let part = &body[..body.len().min(PAYLOAD_CUT)];
    match payload::sniff(part) {
        Kind::Binary => m["payload_base64"] = json!(fenix_nats::b64::encode(&part[..part.len().min(PAYLOAD_CUT * 3 / 4)])),
        Kind::Json if !cut => m["payload"] = serde_json::from_slice(part).unwrap_or_else(|_| json!(String::from_utf8_lossy(part))),
        _ => m["payload"] = json!(String::from_utf8_lossy(part)),
    }
    if cut {
        m["payload_cut_at"] = json!(PAYLOAD_CUT);
    }
    m
}

pub fn owned(o: &Owned) -> Value {
    message(&o.subject, o.reply.as_deref(), o.headers.as_deref(), &o.payload, o.at_us / 1000, o.headers.as_ref().map_or(0, Vec::len) + o.payload.len())
}

/// The newest stored messages on subjects `pattern` matches, newest first.
pub fn stored(core: &Core, conn: u16, pattern: &str, count: usize) -> Vec<Value> {
    let store = &core.store;
    let mut found: Vec<(u64, u32, u32)> = Vec::new();
    for (id, s) in store.subjects.iter().enumerate() {
        if s.conn != conn || s.overflow || !fenix_nats::subject::matches(pattern, &s.name) {
            continue;
        }
        for e in s.msgs.iter().rev().take(count) {
            found.push((e.time as u64, id as u32, e.seq));
        }
    }
    found.sort_unstable_by(|a, b| b.cmp(a));
    found
        .into_iter()
        .take(count)
        .filter_map(|(_, id, seq)| {
            let v = store.get(id, seq)?;
            Some(message(v.subject, v.reply, v.headers, v.payload, v.time_ms, v.entry.size as usize))
        })
        .collect()
}

/// Subjects `pattern` matches, busiest first: their counts and rates.
pub fn subjects(core: &Core, conn: u16, pattern: &str, limit: usize, now_ms: u64) -> Value {
    let store = &core.store;
    let now_sec = now_ms / 1000;
    let mut rows: Vec<&crate::store::Subject> = store.subjects.iter().filter(|s| s.conn == conn && !s.overflow && fenix_nats::subject::matches(pattern, &s.name)).collect();
    rows.sort_by(|a, b| b.rate.per_sec(now_sec).total_cmp(&a.rate.per_sec(now_sec)).then(b.count.cmp(&a.count)));
    let total = rows.len();
    let list: Vec<Value> = rows
        .into_iter()
        .take(limit)
        .map(|s| json!({ "subject": &*s.name, "messages": s.count, "per_second": (s.rate.per_sec(now_sec) * 10.0).round() / 10.0, "average_size": s.average_size().round(), "last_seen": payload::rfc3339(s.last_ms), "stored": s.msgs.len() }))
        .collect();
    json!({ "subjects": list, "matching": total, "listed": total.min(limit), "sampling": store.sampling, "messages_per_second": store.totals.rate.per_sec(now_sec) })
}

/// What a publish asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishArgs {
    pub subject: String,
    pub payload: Vec<u8>,
    pub headers: fenix_nats::headers::HeaderMap,
    pub server: Option<String>,
}

pub fn publish_args(payload: &str) -> Result<PublishArgs, String> {
    let v = args(payload)?;
    let subject = v["subject"].as_str().ok_or("subject: the subject to publish to")?.trim().to_string();
    fenix_nats::subject::check_subject(&subject)?;
    let body = match &v["payload"] {
        Value::String(s) => s.clone().into_bytes(),
        Value::Null => Vec::new(),
        other => serde_json::to_vec(other).unwrap_or_default(),
    };
    let mut headers = fenix_nats::headers::HeaderMap::new();
    if let Some(h) = v["headers"].as_object() {
        for (k, val) in h {
            headers.insert(k, val.as_str().map(str::to_string).unwrap_or_else(|| val.to_string()).as_str());
        }
    }
    headers.check()?;
    Ok(PublishArgs { subject, payload: body, headers, server: v["server"].as_str().map(str::to_string) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tests::up;

    #[test]
    fn arguments_are_read_and_held_to_limits() {
        let a = sample_args(r#"{"subject":"orders.*","count":500}"#).unwrap();
        assert_eq!((a.subject.as_str(), a.count, a.wait_ms), ("orders.*", MAX_SAMPLE, 3000));
        assert_eq!(sample_args("").unwrap().subject, ">");
        assert!(sample_args(r#"{"subject":"a..b"}"#).is_err());
        let p = publish_args(r#"{"subject":"a.b","payload":{"x":1},"headers":{"K":"v"}}"#).unwrap();
        assert_eq!((p.payload.as_slice(), p.headers.get("k")), (br#"{"x":1}"#.as_slice(), Some("v")));
        assert!(publish_args(r#"{"subject":"a.>"}"#).is_err());
        assert!(publish_args("{}").is_err());
    }

    #[test]
    fn a_sample_comes_from_the_store_newest_first() {
        let (mut core, mut net, s) = up(&[">"]);
        core.on_data(&mut net, s, b"MSG orders.a 1 7\r\n{\"n\":1}\r\nMSG billing.x 1 1\r\nx\r\n", 1_000_002_000);
        core.on_data(&mut net, s, b"MSG orders.b 1 7\r\n{\"n\":2}\r\n", 1_000_005_000);
        let got = stored(&core, 0, "orders.*", 10);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0]["payload"]["n"], 2);
        assert_eq!(got[1]["subject"], "orders.a");
        let list = subjects(&core, 0, ">", 10, 1_000_006);
        assert_eq!(list["matching"], 3);
    }

    #[test]
    fn long_and_binary_payloads_are_cut() {
        let m = message("a", None, None, &vec![b'x'; 10_000], 0, 10_000);
        assert_eq!(m["payload"].as_str().unwrap().len(), PAYLOAD_CUT);
        assert_eq!(m["payload_cut_at"], PAYLOAD_CUT);
        assert!(message("a", None, None, &[0, 1, 2], 0, 3).get("payload_base64").is_some());
    }
}
