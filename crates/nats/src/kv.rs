//! Key-value buckets: a stream `KV_<bucket>` on subjects `$KV.<bucket>.>`,
//! a key's history its messages on `$KV.<bucket>.<key>`.
//!
//! A put is a plain publish to the key's subject; a delete or a purge is
//! one with a `KV-Operation` header (a purge also rolls the key's history
//! up). Reading a key is getting the last message on its subject; its keys
//! are the stream's subjects; watching it is subscribing to them.

use crate::headers::{HeaderMap, Headers};
use crate::jetstream::StoredMsg;

/// The stream a bucket is.
pub fn stream(bucket: &str) -> String {
    format!("KV_{bucket}")
}

/// The bucket a stream is, when it's one.
pub fn bucket_of(stream: &str) -> Option<&str> {
    stream.strip_prefix("KV_")
}

/// A key's subject.
pub fn subject(bucket: &str, key: &str) -> String {
    format!("$KV.{bucket}.{key}")
}

/// Every key's subjects.
pub fn all(bucket: &str) -> String {
    format!("$KV.{bucket}.>")
}

/// The key a subject is, in `bucket`.
pub fn key_of<'a>(bucket: &str, subject: &'a str) -> Option<&'a str> {
    subject.strip_prefix("$KV.")?.strip_prefix(bucket)?.strip_prefix('.')
}

/// Whether `bucket` can name a bucket: letters, digits, `-` and `_`.
pub fn valid_bucket(bucket: &str) -> bool {
    !bucket.is_empty() && bucket.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Whether `key` can be a key: letters, digits, `-`, `_`, `/`, `=` and
/// dots between tokens.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty() && !key.starts_with('.') && !key.ends_with('.') && !key.contains("..") && key.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_/=.".contains(&b))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Put,
    Delete,
    Purge,
}

impl Operation {
    /// From a message's headers.
    pub fn of(headers: Option<&[u8]>) -> Operation {
        match headers.and_then(|h| Headers::parse(h).ok()).and_then(|h| h.get("KV-Operation")) {
            Some(op) if op.eq_ignore_ascii_case("DEL") => Operation::Delete,
            Some(op) if op.eq_ignore_ascii_case("PURGE") => Operation::Purge,
            _ => Operation::Put,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Operation::Put => "put",
            Operation::Delete => "delete",
            Operation::Purge => "purge",
        }
    }
}

/// The headers that delete a key: its value gone, its history kept.
pub fn delete_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("KV-Operation", "DEL");
    h
}

/// The headers that purge a key: its value and its history gone.
pub fn purge_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("KV-Operation", "PURGE");
    h.insert("Nats-Rollup", "sub");
    h
}

/// A key's value at one revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub revision: u64,
    pub operation: Operation,
    pub value: Vec<u8>,
    pub time: String,
}

impl Entry {
    /// From a message read from the bucket's stream.
    pub fn from_stored(bucket: &str, m: StoredMsg) -> Entry {
        Entry {
            key: key_of(bucket, &m.subject).unwrap_or(&m.subject).to_string(),
            revision: m.seq,
            operation: Operation::of(m.headers.as_deref()),
            value: m.payload,
            time: m.time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_and_read() {
        assert_eq!(stream("cfg"), "KV_cfg");
        assert_eq!(bucket_of("KV_cfg"), Some("cfg"));
        assert_eq!(bucket_of("ORDERS"), None);
        assert_eq!(subject("cfg", "app.port"), "$KV.cfg.app.port");
        assert_eq!(key_of("cfg", "$KV.cfg.app.port"), Some("app.port"));
        assert_eq!(key_of("cfg", "$KV.cfgx.a"), None);
        assert!(valid_key("a/b=c.d") && !valid_key("a b") && !valid_key("a..b") && !valid_key("*"));
        assert!(valid_bucket("my-bucket_1") && !valid_bucket("a.b"));
    }

    #[test]
    fn operations_are_read_from_headers() {
        assert_eq!(Operation::of(None), Operation::Put);
        assert_eq!(Operation::of(Some(&delete_headers().encode())), Operation::Delete);
        assert_eq!(Operation::of(Some(&purge_headers().encode())), Operation::Purge);
        assert_eq!(purge_headers().get("nats-rollup"), Some("sub"));
        let e = Entry::from_stored("cfg", StoredMsg { subject: "$KV.cfg.a".into(), seq: 4, headers: None, payload: b"1".to_vec(), time: "t".into() });
        assert_eq!((e.key.as_str(), e.revision, e.operation), ("a", 4, Operation::Put));
    }
}
