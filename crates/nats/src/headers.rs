//! The `NATS/1.0` header block: read in place ([`Headers`]), or built and
//! written ([`HeaderMap`]).
//!
//! ```text
//! NATS/1.0 503 No Responders\r\n
//! Key: value\r\n
//! \r\n
//! ```

/// A header block, read where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Headers<'a> {
    /// The status after `NATS/1.0`, when there's one: 503 no responders,
    /// 100 a heartbeat or flow control, 404 and 408 from JetStream.
    pub status: Option<u16>,
    /// What follows the status.
    pub description: &'a str,
    lines: &'a str,
}

impl<'a> Headers<'a> {
    pub fn parse(raw: &'a [u8]) -> Result<Headers<'a>, String> {
        let text = std::str::from_utf8(raw).map_err(|_| "headers aren't UTF-8".to_string())?;
        let rest = text.strip_prefix("NATS/1.0").ok_or("headers don't start NATS/1.0")?;
        let (first, lines) = rest.split_once("\r\n").unwrap_or((rest, ""));
        let first = first.trim();
        let (status, description) = match first.split_once(' ') {
            Some((code, desc)) => (code.parse().ok(), desc.trim()),
            None if !first.is_empty() => (first.parse().ok(), ""),
            None => (None, ""),
        };
        Ok(Headers { status, description, lines })
    }

    /// Its headers, name and value, in order; a name may come more than
    /// once.
    pub fn iter(&self) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
        self.lines.split("\r\n").filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            let name = name.trim();
            (!name.is_empty()).then(|| (name, value.trim()))
        })
    }

    /// The first value of `name`, its case ignored.
    pub fn get(&self, name: &str) -> Option<&'a str> {
        self.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.iter().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// How many headers a block has, without reading them: its lines with a
/// colon, after the first. What a list shows for each message.
pub fn count(raw: &[u8]) -> usize {
    let mut n = 0;
    let mut line_start = true;
    let mut first = true;
    let mut colon = false;
    for &b in raw {
        if b == b'\n' {
            if !first && colon {
                n += 1;
            }
            first = false;
            colon = false;
            line_start = true;
        } else {
            if b == b':' && !line_start && !first {
                colon = true;
            }
            line_start = false;
        }
    }
    n
}

/// Headers to send.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderMap {
    pub entries: Vec<(String, String)>,
}

impl HeaderMap {
    pub fn new() -> HeaderMap {
        HeaderMap::default()
    }

    /// Adds `name: value`, after those it has.
    pub fn insert(&mut self, name: &str, value: &str) {
        self.entries.push((name.to_string(), value.to_string()));
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// A block read, as one to send again.
    pub fn from_headers(h: &Headers<'_>) -> HeaderMap {
        HeaderMap { entries: h.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect() }
    }

    /// Whether every name and value can be sent: no line ends in them, and
    /// no colon or space in a name.
    pub fn check(&self) -> Result<(), String> {
        for (n, v) in &self.entries {
            if n.is_empty() || n.bytes().any(|b| b <= b' ' || b == b':' || b == 127) {
                return Err(format!("header name {n:?}: no spaces or colons"));
            }
            if v.contains(['\r', '\n']) {
                return Err(format!("header {n}: its value has a line end"));
            }
        }
        Ok(())
    }

    /// `NATS/1.0\r\n`, a line each, and the blank line.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = b"NATS/1.0\r\n".to_vec();
        for (n, v) in &self.entries {
            out.extend_from_slice(n.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(v.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_is_read() {
        let raw = b"NATS/1.0\r\nNats-Msg-Id: 1\r\nX-Trace: a:b\r\nx-trace: second\r\n\r\n";
        let h = Headers::parse(raw).unwrap();
        assert_eq!(h.status, None);
        assert_eq!(h.iter().collect::<Vec<_>>(), [("Nats-Msg-Id", "1"), ("X-Trace", "a:b"), ("x-trace", "second")]);
        assert_eq!(h.get("x-TRACE"), Some("a:b"));
        assert_eq!(count(raw), 3);
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn a_status_is_read() {
        let h = Headers::parse(b"NATS/1.0 503\r\n\r\n").unwrap();
        assert_eq!((h.status, h.description, h.len()), (Some(503), "", 0));
        let h = Headers::parse(b"NATS/1.0 100 Idle Heartbeat\r\nNats-Last-Consumer: 2\r\n\r\n").unwrap();
        assert_eq!((h.status, h.description, h.get("nats-last-consumer")), (Some(100), "Idle Heartbeat", Some("2")));
        assert_eq!(count(b"NATS/1.0 100 Idle: Heartbeat\r\n\r\n"), 0);
        assert!(Headers::parse(b"HTTP/1.1 200\r\n\r\n").is_err());
    }

    #[test]
    fn a_map_is_written_and_read_back() {
        let mut m = HeaderMap::new();
        m.insert("A", "1");
        m.insert("B-C", "two words");
        let raw = m.encode();
        assert_eq!(raw, b"NATS/1.0\r\nA: 1\r\nB-C: two words\r\n\r\n");
        assert_eq!(HeaderMap::from_headers(&Headers::parse(&raw).unwrap()), m);
        assert!(m.check().is_ok());
        m.insert("bad name", "x");
        assert!(m.check().is_err());
        let mut m = HeaderMap::new();
        m.insert("A", "x\r\nB: y");
        assert!(m.check().is_err());
    }
}
