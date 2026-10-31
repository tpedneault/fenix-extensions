//! WebSocket requests, as JetBrains writes them: `WEBSOCKET ws://...`,
//! its headers, and the messages to send after a line `===` each (`===
//! wait-for-server` holds one back until the server has said something).
//! The client side of RFC 6455 over one of Fenix's sockets: the opening
//! handshake, frames masked as a client's are, and the server's read back
//! -- fragments joined, pings answered, a close answered and told.

use sha1::{Digest, Sha1};

use crate::auth::base64;
use crate::url;

/// What RFC 6455 hashes the key with.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// The most a message (its fragments together) may be.
pub const MAX_MESSAGE: usize = 16 << 20;

/// A message of the request's, and whether it waits for the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub wait: bool,
}

/// The messages a body holds: what follows each `===` line, to the next
/// (blank lines at either end of one dropped). Text before the first
/// `===` is one too, sent first.
pub fn messages(body: &str) -> Vec<Message> {
    let mut out = Vec::new();
    let mut cur: Option<(Vec<&str>, bool)> = None;
    let mut first: Vec<&str> = Vec::new();
    for line in body.lines() {
        let t = line.trim_end_matches('\r');
        if let Some(rest) = t.strip_prefix("===") {
            if let Some((lines, wait)) = cur.take() {
                push(&mut out, &lines, wait);
            }
            cur = Some((Vec::new(), rest.trim().eq_ignore_ascii_case("wait-for-server")));
        } else if let Some((lines, _)) = cur.as_mut() {
            lines.push(t);
        } else {
            first.push(t);
        }
    }
    if let Some((lines, wait)) = cur.take() {
        push(&mut out, &lines, wait);
    }
    let mut all = Vec::new();
    push(&mut all, &first, false);
    all.extend(out);
    all
}

fn push(out: &mut Vec<Message>, lines: &[&str], wait: bool) {
    let text = lines.join("\n");
    let text = text.trim_matches('\n');
    if !text.trim().is_empty() {
        out.push(Message { text: text.to_string(), wait });
    }
}

/// Where a `ws://` or `wss://` URL goes: the host, its port, TLS or not,
/// and what's asked for (path and query).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub resource: String,
    /// The `Host` header: the port only when it isn't the scheme's.
    pub authority: String,
}

pub fn target(u: &str) -> Result<Target, String> {
    let (scheme, _) = u.split_once("://").ok_or_else(|| format!("{u}: a ws:// or wss:// URL"))?;
    let tls = match scheme.to_ascii_lowercase().as_str() {
        "ws" | "http" => false,
        "wss" | "https" => true,
        other => return Err(format!("{other}:// isn't a WebSocket's: ws:// or wss://")),
    };
    let rest = &u[scheme.len() + 3..];
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let given_port = authority.rsplit(']').next().and_then(|t| t.rsplit_once(':')).and_then(|(_, p)| p.parse::<u16>().ok());
    let (_, host, _) = url::host(u).ok_or_else(|| format!("{u}: no host"))?;
    if host.is_empty() {
        return Err(format!("{u}: no host"));
    }
    let default = if tls { 443 } else { 80 };
    let port = given_port.unwrap_or(default);
    let after = &rest[authority.len()..];
    let resource = match after.split('#').next().unwrap_or("") {
        "" => "/".to_string(),
        r if r.starts_with('?') => format!("/{r}"),
        r => r.to_string(),
    };
    let shown = if host.contains(':') { format!("[{host}]") } else { host.clone() };
    let authority = if port == default { shown } else { format!("{shown}:{port}") };
    Ok(Target { host, port, tls, resource, authority })
}

/// What `Sec-WebSocket-Accept` must say for `key`.
pub fn accept_for(key: &str) -> String {
    base64(&Sha1::digest(format!("{key}{GUID}").as_bytes()))
}

/// The opening handshake: a GET asking to upgrade, `key` (16 random
/// bytes, base64) its `Sec-WebSocket-Key`, the request's own headers
/// after (`Authorization`, `Sec-WebSocket-Protocol`), those it sets
/// itself left out.
pub fn handshake(t: &Target, key: &str, headers: &[(String, String)]) -> Vec<u8> {
    let mut s = format!("GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n", t.resource, t.authority);
    const OWN: &[&str] = &["host", "upgrade", "connection", "sec-websocket-key", "sec-websocket-version", "content-length", "content-type"];
    for (k, v) in headers {
        if !OWN.contains(&k.to_ascii_lowercase().as_str()) {
            s.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    s.push_str("\r\n");
    s.into_bytes()
}

/// The server's answer to the handshake, read: its status, reason and
/// headers, and how many bytes it took (frames may follow at once).
/// None until it's all come.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub len: usize,
}

pub fn read_answer(buf: &[u8]) -> Option<Result<Answer, String>> {
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buf[..end]);
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.splitn(3, ' ');
    let (version, status) = (parts.next().unwrap_or(""), parts.next().and_then(|s| s.parse::<u16>().ok()));
    let Some(status) = status.filter(|_| version.starts_with("HTTP/")) else {
        return Some(Err(format!("the server didn't answer as HTTP does: {first:?}")));
    };
    let reason = parts.next().unwrap_or("").to_string();
    let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
    Some(Ok(Answer { status, reason, headers, len: end + 4 }))
}

/// Whether `a` is the upgrade `key` asked for, or why not.
pub fn check_answer(a: &Answer, key: &str) -> Result<(), String> {
    if a.status != 101 {
        return Err(format!("the server answered {} {} rather than switching to WebSocket", a.status, a.reason));
    }
    let get = |n: &str| a.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.as_str());
    if !get("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket")) {
        return Err("the server's 101 doesn't say Upgrade: websocket".into());
    }
    if get("sec-websocket-accept") != Some(accept_for(key).as_str()) {
        return Err("the server's Sec-WebSocket-Accept isn't the one for the key sent".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The frames a client sends: messages are text, as JetBrains' are.
pub enum Op {
    Text = 1,
    Close = 8,
    Pong = 10,
}

/// A frame as a client sends it: whole (FIN), masked with `mask`.
pub fn frame(op: Op, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut f = vec![0x80 | op as u8];
    match payload.len() {
        n @ 0..=125 => f.push(0x80 | n as u8),
        n @ 126..=0xFFFF => {
            f.push(0x80 | 126);
            f.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            f.push(0x80 | 127);
            f.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    f.extend_from_slice(&mask);
    f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    f
}

/// A close frame's payload: its code, and why.
pub fn close_payload(code: u16, reason: &str) -> Vec<u8> {
    let mut p = code.to_be_bytes().to_vec();
    p.extend_from_slice(reason.as_bytes());
    p
}

/// What came from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Text(String),
    Binary(Vec<u8>),
    /// To be answered with a pong of the same bytes.
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    /// It's closing: its code and reason, when it gave them.
    Close(Option<u16>, String),
    /// What it sent breaks the protocol: the connection's to be dropped.
    Broken(String),
}

/// The server's frames, read as they come.
#[derive(Debug, Default)]
pub struct Reader {
    buf: Vec<u8>,
    /// A fragmented message so far: whether it's text, and its bytes.
    partial: Option<(bool, Vec<u8>)>,
    broken: bool,
}

impl Reader {
    /// `data` added; the events it completes.
    pub fn push(&mut self, data: &[u8]) -> Vec<Event> {
        let mut out = Vec::new();
        if self.broken {
            return out;
        }
        self.buf.extend_from_slice(data);
        loop {
            match self.next() {
                Ok(Some(e)) => {
                    let stop = matches!(e, Event::Broken(_));
                    out.push(e);
                    if stop {
                        self.broken = true;
                        self.buf.clear();
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    self.broken = true;
                    self.buf.clear();
                    out.push(Event::Broken(e));
                    break;
                }
            }
        }
        out
    }

    /// One frame off the front, when it's all there: an event, or none
    /// for a fragment that isn't the last.
    fn next(&mut self) -> Result<Option<Event>, String> {
        loop {
            let b = &self.buf;
            if b.len() < 2 {
                return Ok(None);
            }
            let fin = b[0] & 0x80 != 0;
            if b[0] & 0x70 != 0 {
                return Err("a frame with reserved bits set (no extension was agreed)".into());
            }
            let op = b[0] & 0x0F;
            let masked = b[1] & 0x80 != 0;
            let (len, mut at) = match b[1] & 0x7F {
                126 if b.len() >= 4 => (u16::from_be_bytes([b[2], b[3]]) as u64, 4),
                127 if b.len() >= 10 => (u64::from_be_bytes(b[2..10].try_into().unwrap_or_default()), 10),
                126 | 127 => return Ok(None),
                n => (n as u64, 2),
            };
            if len > MAX_MESSAGE as u64 {
                return Err(format!("a frame of {len} bytes, past the {} MiB kept", MAX_MESSAGE >> 20));
            }
            let mask = if masked {
                if b.len() < at + 4 {
                    return Ok(None);
                }
                let m = [b[at], b[at + 1], b[at + 2], b[at + 3]];
                at += 4;
                Some(m)
            } else {
                None
            };
            let len = len as usize;
            if b.len() < at + len {
                return Ok(None);
            }
            let mut payload = b[at..at + len].to_vec();
            if let Some(m) = mask {
                for (i, x) in payload.iter_mut().enumerate() {
                    *x ^= m[i % 4];
                }
            }
            self.buf.drain(..at + len);
            if op >= 8 {
                if !fin || len > 125 {
                    return Err("a control frame fragmented, or past 125 bytes".into());
                }
                return Ok(Some(match op {
                    8 => {
                        let code = (payload.len() >= 2).then(|| u16::from_be_bytes([payload[0], payload[1]]));
                        let reason = payload.get(2..).map(|r| String::from_utf8_lossy(r).into_owned()).unwrap_or_default();
                        Event::Close(code, reason)
                    }
                    9 => Event::Ping(payload),
                    10 => Event::Pong(payload),
                    other => return Err(format!("control frame {other:#x} isn't one RFC 6455 has")),
                }));
            }
            let (text, mut bytes) = match (op, self.partial.take()) {
                (0, Some(p)) => {
                    let (text, mut so_far) = p;
                    so_far.extend(payload);
                    (text, so_far)
                }
                (0, None) => return Err("a continuation with no message begun".into()),
                (1 | 2, None) => (op == 1, payload),
                (1 | 2, Some(_)) => return Err("a new message before the last one's end".into()),
                (other, _) => return Err(format!("frame {other:#x} isn't one RFC 6455 has")),
            };
            if bytes.len() > MAX_MESSAGE {
                return Err(format!("a message past the {} MiB kept", MAX_MESSAGE >> 20));
            }
            if !fin {
                self.partial = Some((text, std::mem::take(&mut bytes)));
                continue;
            }
            return Ok(Some(if text {
                match String::from_utf8(bytes) {
                    Ok(s) => Event::Text(s),
                    Err(_) => return Err("a text message that isn't UTF-8".into()),
                }
            } else {
                Event::Binary(bytes)
            }));
        }
    }
}

/// A binary message as the transcript shows it: hex, the first 64 bytes.
pub fn show_binary(b: &[u8]) -> String {
    let hex: String = b.iter().take(64).map(|x| format!("{x:02x}")).collect();
    let more = if b.len() > 64 { "…" } else { "" };
    format!("[{} bytes] {hex}{more}", b.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_follow_their_separators() {
        let body = "===\n{\n  \"a\": 1\n}\n\n=== wait-for-server\nsecond\n===\n\n";
        assert_eq!(messages(body), [Message { text: "{\n  \"a\": 1\n}".into(), wait: false }, Message { text: "second".into(), wait: true }]);
        assert_eq!(messages("hello"), [Message { text: "hello".into(), wait: false }]);
        assert_eq!(messages("first\n===\nthen"), [Message { text: "first".into(), wait: false }, Message { text: "then".into(), wait: false }]);
        assert!(messages("").is_empty());
        assert!(messages("===\n\n===").is_empty());
    }

    #[test]
    fn urls_become_where_to_connect() {
        assert_eq!(target("wss://echo.example.com/socket?x=1").unwrap(), Target { host: "echo.example.com".into(), port: 443, tls: true, resource: "/socket?x=1".into(), authority: "echo.example.com".into() });
        assert_eq!(target("ws://localhost:8080").unwrap(), Target { host: "localhost".into(), port: 8080, tls: false, resource: "/".into(), authority: "localhost:8080".into() });
        assert_eq!(target("ws://h.test?q").unwrap().resource, "/?q");
        assert_eq!(target("ws://[::1]:9000/a").unwrap().authority, "[::1]:9000");
        assert_eq!(target("wss://h.test:443/").unwrap().authority, "h.test");
        assert!(target("ftp://h.test").is_err());
        assert!(target("h.test").is_err());
    }

    #[test]
    fn the_handshake_and_rfc_6455s_example_key() {
        // RFC 6455, 1.3.
        assert_eq!(accept_for("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
        let t = target("ws://server.example.com/chat").unwrap();
        let h = String::from_utf8(handshake(&t, "dGhlIHNhbXBsZSBub25jZQ==", &[("Sec-WebSocket-Protocol".into(), "chat".into()), ("Host".into(), "other".into()), ("Content-Type".into(), "application/json".into())])).unwrap();
        assert_eq!(h, "GET /chat HTTP/1.1\r\nHost: server.example.com\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: chat\r\n\r\n");
        let answer = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n\x81\x02hi";
        assert_eq!(read_answer(&answer[..20]), None);
        let a = read_answer(answer).unwrap().unwrap();
        assert_eq!((a.status, a.len), (101, answer.len() - 4));
        assert_eq!(check_answer(&a, "dGhlIHNhbXBsZSBub25jZQ=="), Ok(()));
        assert!(check_answer(&a, "another").unwrap_err().contains("Accept"));
        let refused = read_answer(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n").unwrap().unwrap();
        assert!(check_answer(&refused, "k").unwrap_err().contains("403 Forbidden"));
        assert!(read_answer(b"SSH-2.0-OpenSSH\r\n\r\n").unwrap().is_err());
    }

    #[test]
    fn frames_go_masked_and_come_back() {
        // RFC 6455, 5.7: "Hello", masked.
        assert_eq!(frame(Op::Text, b"Hello", [0x37, 0xfa, 0x21, 0x3d]), [0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58]);
        let long = frame(Op::Text, &[7; 300], [0; 4]);
        assert_eq!(&long[..4], &[0x81, 0xFE, 0x01, 0x2C]);
        assert_eq!(long.len(), 4 + 4 + 300);
        let huge = frame(Op::Pong, &vec![0; 70_000], [0; 4]);
        assert_eq!(&huge[..10], &[0x8A, 0xFF, 0, 0, 0, 0, 0, 1, 0x11, 0x70]);
        let mut r = Reader::default();
        // The server's, unmasked: "Hel" then "lo" as fragments, a ping
        // between, then a close -- in pieces.
        let mut wire = vec![0x01, 0x03, b'H', b'e', b'l', 0x89, 0x02, b'p', b'!', 0x80, 0x02, b'l', b'o'];
        wire.extend_from_slice(&[0x88, 0x05, 0x03, 0xE8, b'b', b'y', b'e']);
        assert_eq!(r.push(&wire[..4]), []);
        assert_eq!(r.push(&wire[4..12]), [Event::Ping(b"p!".to_vec())]);
        assert_eq!(r.push(&wire[12..]), [Event::Text("Hello".into()), Event::Close(Some(1000), "bye".into())]);
        // A masked frame is read too, and a 16-bit length.
        let mut r = Reader::default();
        assert_eq!(r.push(&frame(Op::Text, "é".repeat(100).as_bytes(), [1, 2, 3, 4])), [Event::Text("é".repeat(100))]);
        assert_eq!(r.push(&[0x82, 0x7E, 0x00, 0x7E]), []);
        assert_eq!(r.push(&[9; 126]), [Event::Binary(vec![9; 126])]);
    }

    #[test]
    fn what_breaks_the_protocol_is_told_once() {
        let mut r = Reader::default();
        assert!(matches!(&r.push(&[0x80, 0x01, b'x'])[..], [Event::Broken(e)] if e.contains("continuation")));
        assert_eq!(r.push(&[0x81, 0x01, b'x']), [], "nothing more once it's broken");
        let mut r = Reader::default();
        assert!(matches!(&r.push(&[0x81, 0x02, 0xff, 0xfe])[..], [Event::Broken(e)] if e.contains("UTF-8")));
        let mut r = Reader::default();
        assert!(matches!(&r.push(&[0x09, 0x00])[..], [Event::Broken(e)] if e.contains("control")));
        let mut r = Reader::default();
        assert!(matches!(&r.push(&[0xC1, 0x00])[..], [Event::Broken(e)] if e.contains("reserved")));
        let mut r = Reader::default();
        assert!(matches!(&r.push(&[0x82, 0x7F, 0, 0, 0, 0, 0x10, 0, 0, 0])[..], [Event::Broken(e)] if e.contains("MiB")));
        assert_eq!(close_payload(1000, "done"), [0x03, 0xE8, b'd', b'o', b'n', b'e']);
        assert_eq!(show_binary(&[1, 2, 255]), "[3 bytes] 0102ff");
    }
}
