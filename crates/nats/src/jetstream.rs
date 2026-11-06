//! JetStream's API: requests to `$JS.API.*` subjects, answered in JSON.
//!
//! [`Api`] makes each request's subject and body; [`parse`] reads an
//! answer, or the error JetStream gave instead. Only what a browser needs
//! is here: streams and their messages, consumers, and what key-value
//! buckets ([`crate::kv`]) are made of.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An error JetStream answered with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ApiError {
    pub code: u16,
    pub err_code: u32,
    pub description: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({}, {})", self.description, self.code, self.err_code)
    }
}

/// Reads an answer: `T`, or the error it says.
pub fn parse<T: DeserializeOwned>(payload: &[u8]) -> Result<T, String> {
    let v: serde_json::Value = serde_json::from_slice(payload).map_err(|e| format!("JetStream's answer isn't JSON: {e}"))?;
    if let Some(err) = v.get("error") {
        let err: ApiError = serde_json::from_value(err.clone()).unwrap_or_default();
        return Err(err.to_string());
    }
    serde_json::from_value(v).map_err(|e| format!("JetStream's answer: {e}"))
}

/// The API's subjects: `$JS.API`, or a domain's `$JS.<domain>.API`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Api {
    pub prefix: String,
}

impl Default for Api {
    fn default() -> Api {
        Api { prefix: "$JS.API".into() }
    }
}

impl Api {
    pub fn domain(domain: Option<&str>) -> Api {
        match domain.filter(|d| !d.is_empty()) {
            Some(d) => Api { prefix: format!("$JS.{d}.API") },
            None => Api::default(),
        }
    }

    fn at(&self, rest: &str) -> String {
        format!("{}.{rest}", self.prefix)
    }

    pub fn account_info(&self) -> String {
        self.at("INFO")
    }
    pub fn stream_names(&self) -> String {
        self.at("STREAM.NAMES")
    }
    pub fn stream_list(&self) -> String {
        self.at("STREAM.LIST")
    }
    pub fn stream_info(&self, stream: &str) -> String {
        self.at(&format!("STREAM.INFO.{stream}"))
    }
    pub fn stream_purge(&self, stream: &str) -> String {
        self.at(&format!("STREAM.PURGE.{stream}"))
    }
    pub fn msg_get(&self, stream: &str) -> String {
        self.at(&format!("STREAM.MSG.GET.{stream}"))
    }
    pub fn msg_delete(&self, stream: &str) -> String {
        self.at(&format!("STREAM.MSG.DELETE.{stream}"))
    }
    pub fn consumer_list(&self, stream: &str) -> String {
        self.at(&format!("CONSUMER.LIST.{stream}"))
    }
    pub fn consumer_info(&self, stream: &str, consumer: &str) -> String {
        self.at(&format!("CONSUMER.INFO.{stream}.{consumer}"))
    }
}

/// A body: its fields that are set, as JSON.
pub fn body<T: Serialize>(request: &T) -> Vec<u8> {
    serde_json::to_vec(request).expect("a request is JSON")
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct StreamConfig {
    pub name: String,
    pub description: Option<String>,
    pub subjects: Vec<String>,
    /// `limits`, `interest` or `workqueue`.
    pub retention: String,
    pub max_consumers: i64,
    pub max_msgs: i64,
    pub max_bytes: i64,
    /// Nanoseconds; 0 for no limit.
    pub max_age: i64,
    pub max_msgs_per_subject: i64,
    pub max_msg_size: i64,
    /// `file` or `memory`.
    pub storage: String,
    /// `old` or `new`.
    pub discard: String,
    pub num_replicas: i64,
    pub duplicate_window: i64,
    pub allow_direct: bool,
    pub allow_rollup_hdrs: bool,
    pub deny_delete: bool,
    pub deny_purge: bool,
    pub sealed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct StreamState {
    pub messages: u64,
    pub bytes: u64,
    pub first_seq: u64,
    pub first_ts: String,
    pub last_seq: u64,
    pub last_ts: String,
    pub consumer_count: u64,
    pub num_subjects: u64,
    pub num_deleted: u64,
    /// With `subjects_filter` asked for: each subject's count.
    pub subjects: Option<BTreeMap<String, u64>>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ClusterInfo {
    pub name: Option<String>,
    pub leader: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct StreamInfo {
    pub config: StreamConfig,
    pub state: StreamState,
    pub created: String,
    pub cluster: Option<ClusterInfo>,
}

/// `STREAM.INFO`'s body: each subject's count when `subjects_filter` is
/// set (a KV bucket's keys).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StreamInfoRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subjects_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
}

/// `STREAM.NAMES`' and `STREAM.LIST`'s body.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ListRequest {
    pub offset: u64,
    /// Only the streams with a subject matching this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct StreamNames {
    pub total: u64,
    pub offset: u64,
    pub limit: u64,
    /// `null` when there are none.
    pub streams: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct StreamList {
    pub total: u64,
    pub offset: u64,
    pub limit: u64,
    pub streams: Option<Vec<StreamInfo>>,
}

/// `STREAM.MSG.GET`'s body: one of a sequence; the last on a subject; or
/// the first on a subject (wildcards too) at or after a sequence; or the
/// first at or after a time (RFC 3339; NATS 2.11).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MsgGetRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_by_subj: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_by_subj: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
}

impl MsgGetRequest {
    pub fn seq(seq: u64) -> MsgGetRequest {
        MsgGetRequest { seq: Some(seq), ..Default::default() }
    }
    /// The first message at or after `seq` on `subject` (`>` any).
    pub fn next(subject: &str, seq: u64) -> MsgGetRequest {
        MsgGetRequest { seq: Some(seq), next_by_subj: Some(subject.into()), ..Default::default() }
    }
    pub fn last(subject: &str) -> MsgGetRequest {
        MsgGetRequest { last_by_subj: Some(subject.into()), ..Default::default() }
    }
    pub fn at_time(rfc3339: &str) -> MsgGetRequest {
        MsgGetRequest { start_time: Some(rfc3339.into()), ..Default::default() }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct RawStoredMsg {
    subject: String,
    seq: u64,
    hdrs: Option<String>,
    data: Option<String>,
    time: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct MsgGetResponse {
    message: RawStoredMsg,
}

/// A message read from a stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredMsg {
    pub subject: String,
    pub seq: u64,
    /// The header block, `NATS/1.0` and all, when it had headers.
    pub headers: Option<Vec<u8>>,
    pub payload: Vec<u8>,
    /// RFC 3339.
    pub time: String,
}

/// Reads `STREAM.MSG.GET`'s answer.
pub fn parse_stored(payload: &[u8]) -> Result<StoredMsg, String> {
    let r: MsgGetResponse = parse(payload)?;
    let m = r.message;
    Ok(StoredMsg {
        subject: m.subject,
        seq: m.seq,
        headers: m.hdrs.map(|h| crate::b64::decode(&h)).transpose()?,
        payload: m.data.map(|d| crate::b64::decode(&d)).transpose()?.unwrap_or_default(),
        time: m.time,
    })
}

/// `STREAM.PURGE`'s body: every message, or those on a subject, or before
/// a sequence, or all but the last few.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PurgeRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct PurgeResponse {
    pub success: bool,
    pub purged: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MsgDeleteRequest {
    pub seq: u64,
    /// Only marked deleted, not overwritten.
    pub no_erase: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Success {
    pub success: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ConsumerConfig {
    pub durable_name: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub deliver_policy: String,
    pub ack_policy: String,
    pub ack_wait: i64,
    pub max_deliver: i64,
    pub filter_subject: Option<String>,
    pub filter_subjects: Option<Vec<String>>,
    pub replay_policy: String,
    pub max_ack_pending: i64,
    pub deliver_subject: Option<String>,
    pub deliver_group: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SequenceInfo {
    pub consumer_seq: u64,
    pub stream_seq: u64,
    pub last_active: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ConsumerInfo {
    pub stream_name: String,
    pub name: String,
    pub created: String,
    pub config: ConsumerConfig,
    pub delivered: SequenceInfo,
    pub ack_floor: SequenceInfo,
    pub num_ack_pending: u64,
    pub num_redelivered: u64,
    pub num_waiting: u64,
    pub num_pending: u64,
    pub paused: bool,
}

impl ConsumerInfo {
    /// Push consumers deliver to a subject; pull consumers wait to be
    /// asked.
    pub fn is_push(&self) -> bool {
        self.config.deliver_subject.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ConsumerList {
    pub total: u64,
    pub offset: u64,
    pub limit: u64,
    pub consumers: Option<Vec<ConsumerInfo>>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct AccountLimits {
    pub max_memory: i64,
    pub max_storage: i64,
    pub max_streams: i64,
    pub max_consumers: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct AccountInfo {
    pub memory: u64,
    pub storage: u64,
    pub streams: u64,
    pub consumers: u64,
    pub domain: Option<String>,
    pub limits: AccountLimits,
}

/// A duration JetStream gives in nanoseconds, said shortly: `30d`, `5m`,
/// `forever` for 0.
pub fn nanos(ns: i64) -> String {
    if ns <= 0 {
        return "forever".into();
    }
    let s = ns / 1_000_000_000;
    match s {
        0 => format!("{}ms", ns / 1_000_000),
        s if s % 86_400 == 0 => format!("{}d", s / 86_400),
        s if s % 3_600 == 0 => format!("{}h", s / 3_600),
        s if s % 60 == 0 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_follow_the_domain() {
        assert_eq!(Api::default().stream_info("ORDERS"), "$JS.API.STREAM.INFO.ORDERS");
        assert_eq!(Api::domain(Some("hub")).msg_get("KV_cfg"), "$JS.hub.API.STREAM.MSG.GET.KV_cfg");
        assert_eq!(Api::domain(Some("")).consumer_info("S", "C"), "$JS.API.CONSUMER.INFO.S.C");
    }

    #[test]
    fn requests_leave_out_what_isnt_set() {
        assert_eq!(body(&MsgGetRequest::next(">", 5)), br#"{"seq":5,"next_by_subj":">"}"#);
        assert_eq!(body(&MsgGetRequest::last("$KV.b.k")), br#"{"last_by_subj":"$KV.b.k"}"#);
        assert_eq!(body(&PurgeRequest::default()), b"{}");
        assert_eq!(body(&StreamInfoRequest { subjects_filter: Some("$KV.b.>".into()), offset: None }), br#"{"subjects_filter":"$KV.b.>"}"#);
    }

    #[test]
    fn a_stream_list_is_read() {
        let json = br#"{"type":"io.nats.jetstream.api.v1.stream_list_response","total":1,"offset":0,"limit":256,"streams":[{"config":{"name":"ORDERS","subjects":["orders.>"],"retention":"limits","max_consumers":-1,"max_msgs":-1,"max_bytes":-1,"max_age":86400000000000,"storage":"file","num_replicas":3,"discard":"old","future_field":1},"created":"2024-01-01T00:00:00Z","state":{"messages":12,"bytes":3400,"first_seq":1,"first_ts":"2024-01-01T00:00:00Z","last_seq":12,"last_ts":"2024-01-02T00:00:00Z","consumer_count":2}}]}"#;
        let list: StreamList = parse(json).unwrap();
        let s = &list.streams.unwrap()[0];
        assert_eq!((s.config.name.as_str(), s.config.num_replicas, s.state.messages, s.state.consumer_count), ("ORDERS", 3, 12, 2));
        assert_eq!(nanos(s.config.max_age), "1d");
    }

    #[test]
    fn an_error_is_said() {
        let err = parse::<StreamInfo>(br#"{"type":"x","error":{"code":404,"err_code":10059,"description":"stream not found"}}"#).unwrap_err();
        assert_eq!(err, "stream not found (404, 10059)");
        assert!(parse::<StreamInfo>(b"nope").is_err());
    }

    #[test]
    fn a_stored_message_is_decoded() {
        let json = br#"{"type":"io.nats.jetstream.api.v1.stream_msg_get_response","message":{"subject":"orders.new","seq":7,"hdrs":"TkFUUy8xLjANCkE6IGINCg0K","data":"aGVsbG8=","time":"2024-01-01T00:00:00.5Z"}}"#;
        let m = parse_stored(json).unwrap();
        assert_eq!((m.subject.as_str(), m.seq, m.payload.as_slice()), ("orders.new", 7, b"hello".as_slice()));
        assert_eq!(m.headers.as_deref(), Some(b"NATS/1.0\r\nA: b\r\n\r\n".as_slice()));
    }

    #[test]
    fn consumers_are_read() {
        let json = br#"{"total":1,"offset":0,"limit":256,"consumers":[{"stream_name":"ORDERS","name":"proc","config":{"durable_name":"proc","ack_policy":"explicit","deliver_policy":"all","filter_subject":"orders.new"},"delivered":{"consumer_seq":10,"stream_seq":12},"ack_floor":{"consumer_seq":8,"stream_seq":9},"num_ack_pending":2,"num_redelivered":1,"num_waiting":0,"num_pending":40}]}"#;
        let list: ConsumerList = parse(json).unwrap();
        let c = &list.consumers.unwrap()[0];
        assert_eq!((c.num_pending, c.num_ack_pending, c.num_redelivered, c.is_push()), (40, 2, 1, false));
        assert_eq!(nanos(0), "forever");
        assert_eq!(nanos(90_000_000_000), "90s");
        assert_eq!(nanos(5_000_000), "5ms");
    }
}
