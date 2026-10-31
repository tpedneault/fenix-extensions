//! A request as sent and what came back: what history keeps, what chained
//! requests read, what the response view shows.

use serde::{Deserialize, Serialize};

/// A request as it went: its variables filled in, its body built.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sent {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    #[serde(with = "bytes")]
    pub body: Vec<u8>,
}

/// How long each part took, in microseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timing {
    pub dns: Option<u64>,
    pub connect: Option<u64>,
    pub tls: Option<u64>,
    pub first_byte: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Received {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    #[serde(with = "bytes")]
    pub body: Vec<u8>,
    /// Where it ended up, after redirects.
    pub url: String,
    pub timing: Timing,
}

/// One request sent, and its answer or why there's none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchange {
    pub sent: Sent,
    pub answer: Result<Received, String>,
    /// When, in seconds since 1970.
    pub at: i64,
    /// The `.http` file it came from, below the project, and its name
    /// there.
    pub file: Option<String>,
    pub name: Option<String>,
    /// The environment it was sent in.
    pub env: Option<String>,
}

impl Sent {
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }
}

impl Received {
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }

    /// Every value of header `name`: `Set-Cookie` comes several times.
    pub fn headers_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.headers.iter().filter(move |(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn content_type(&self) -> &str {
        self.header("content-type").unwrap_or("")
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// `200 OK`.
    pub fn status_line(&self) -> String {
        match reason(self.status) {
            "" => self.status.to_string(),
            r => format!("{} {r}", self.status),
        }
    }
}

pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().rev().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// A status's reason phrase.
pub fn reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        418 => "I'm a teapot",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

/// `1.2 kB`, `340 B`.
pub fn size(n: usize) -> String {
    match n {
        n if n < 1000 => format!("{n} B"),
        n if n < 1_000_000 => format!("{:.1} kB", n as f64 / 1000.0),
        n => format!("{:.1} MB", n as f64 / 1_000_000.0),
    }
}

/// `85 ms`, `1.20 s`, from microseconds.
pub fn duration(micros: u64) -> String {
    match micros {
        m if m < 1000 => format!("{m} µs"),
        m if m < 1_000_000 => format!("{} ms", m / 1000),
        m => format!("{:.2} s", m as f64 / 1_000_000.0),
    }
}

/// Bodies kept as text when they're UTF-8, else base64.
mod bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    enum Kept {
        #[serde(rename = "text")]
        Text(String),
        #[serde(rename = "base64")]
        Base64(String),
    }

    pub fn serialize<S: Serializer>(b: &[u8], s: S) -> Result<S::Ok, S::Error> {
        match std::str::from_utf8(b) {
            Ok(t) => Kept::Text(t.to_string()),
            Err(_) => Kept::Base64(crate::auth::base64(b)),
        }
        .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        Ok(match Kept::deserialize(d)? {
            Kept::Text(t) => t.into_bytes(),
            Kept::Base64(b) => crate::auth::unbase64(&b).unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exchange_is_kept_and_read_back() {
        let e = Exchange {
            sent: Sent { method: "POST".into(), url: "https://x.test".into(), headers: vec![("A".into(), "1".into())], body: vec![0xff, 0, 1] },
            answer: Ok(Received { status: 201, body: b"{}".to_vec(), ..Received::default() }),
            at: 5,
            file: Some("api.http".into()),
            name: None,
            env: Some("dev".into()),
        };
        let text = serde_json::to_string(&e).unwrap();
        assert!(text.contains("\"base64\""), "{text}");
        assert_eq!(serde_json::from_str::<Exchange>(&text).unwrap(), e);
    }

    #[test]
    fn sizes_times_and_statuses_read_well() {
        assert_eq!((size(340), size(1234), size(5_600_000)), ("340 B".into(), "1.2 kB".into(), "5.6 MB".into()));
        assert_eq!((duration(420), duration(85_300), duration(1_204_000)), ("420 µs".into(), "85 ms".into(), "1.20 s".into()));
        assert_eq!(Received { status: 404, ..Received::default() }.status_line(), "404 Not Found");
        assert_eq!(Received { status: 299, ..Received::default() }.status_line(), "299");
    }
}
