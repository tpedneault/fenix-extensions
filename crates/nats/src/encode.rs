//! What a client writes: each command appended to a buffer, so several go
//! out in one write.

use crate::subject;
use crate::Connect;

/// `CONNECT {...}`.
pub fn connect(out: &mut Vec<u8>, options: &Connect) {
    out.extend_from_slice(b"CONNECT ");
    out.extend_from_slice(options.json().as_bytes());
    out.extend_from_slice(b"\r\n");
}

pub fn ping(out: &mut Vec<u8>) {
    out.extend_from_slice(b"PING\r\n");
}

pub fn pong(out: &mut Vec<u8>) {
    out.extend_from_slice(b"PONG\r\n");
}

fn decimal(out: &mut Vec<u8>, mut n: u64) {
    let mut digits = [0u8; 20];
    let mut i = digits.len();
    loop {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(&digits[i..]);
}

/// `SUB subject [queue] sid`: `subject` may have wildcards.
pub fn sub(out: &mut Vec<u8>, subject: &str, queue: Option<&str>, sid: u64) -> Result<(), String> {
    subject::check_pattern(subject)?;
    if let Some(q) = queue {
        if q.is_empty() || q.bytes().any(|b| b <= b' ') {
            return Err(format!("queue group {q:?}: no spaces"));
        }
    }
    out.extend_from_slice(b"SUB ");
    out.extend_from_slice(subject.as_bytes());
    if let Some(q) = queue {
        out.push(b' ');
        out.extend_from_slice(q.as_bytes());
    }
    out.push(b' ');
    decimal(out, sid);
    out.extend_from_slice(b"\r\n");
    Ok(())
}

/// `UNSUB sid [max]`: gone now, or after `max` more messages.
pub fn unsub(out: &mut Vec<u8>, sid: u64, max: Option<u64>) {
    out.extend_from_slice(b"UNSUB ");
    decimal(out, sid);
    if let Some(max) = max {
        out.push(b' ');
        decimal(out, max);
    }
    out.extend_from_slice(b"\r\n");
}

fn check_reply(reply: Option<&str>) -> Result<(), String> {
    match reply {
        Some(r) => subject::check_subject(r).map_err(|e| format!("reply-to: {e}")),
        None => Ok(()),
    }
}

/// `PUB subject [reply] size`, then the payload.
pub fn publish(out: &mut Vec<u8>, subject: &str, reply: Option<&str>, payload: &[u8]) -> Result<(), String> {
    subject::check_subject(subject)?;
    check_reply(reply)?;
    out.extend_from_slice(b"PUB ");
    out.extend_from_slice(subject.as_bytes());
    if let Some(r) = reply {
        out.push(b' ');
        out.extend_from_slice(r.as_bytes());
    }
    out.push(b' ');
    decimal(out, payload.len() as u64);
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\r\n");
    Ok(())
}

/// `HPUB subject [reply] hsize size`, then the header block
/// ([`crate::headers::HeaderMap::encode`]) and the payload.
pub fn hpublish(out: &mut Vec<u8>, subject: &str, reply: Option<&str>, headers: &[u8], payload: &[u8]) -> Result<(), String> {
    subject::check_subject(subject)?;
    check_reply(reply)?;
    if !headers.starts_with(b"NATS/1.0") || !headers.ends_with(b"\r\n\r\n") {
        return Err("a header block starts NATS/1.0 and ends with a blank line".into());
    }
    out.extend_from_slice(b"HPUB ");
    out.extend_from_slice(subject.as_bytes());
    if let Some(r) = reply {
        out.push(b' ');
        out.extend_from_slice(r.as_bytes());
    }
    out.push(b' ');
    decimal(out, headers.len() as u64);
    out.push(b' ');
    decimal(out, (headers.len() + payload.len()) as u64);
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(headers);
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\r\n");
    Ok(())
}

/// A message with headers when it has any, `PUB` when it hasn't.
pub fn message(out: &mut Vec<u8>, subject: &str, reply: Option<&str>, headers: &crate::headers::HeaderMap, payload: &[u8]) -> Result<(), String> {
    if headers.is_empty() {
        publish(out, subject, reply, payload)
    } else {
        hpublish(out, subject, reply, &headers.encode(), payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headers::HeaderMap;
    use crate::{Op, Parser};

    #[test]
    fn commands_are_written_as_the_protocol_has_them() {
        let mut out = Vec::new();
        sub(&mut out, "orders.>", None, 1).unwrap();
        sub(&mut out, "work.*", Some("workers"), 22).unwrap();
        unsub(&mut out, 1, None);
        unsub(&mut out, 22, Some(5));
        publish(&mut out, "a.b", None, b"hi").unwrap();
        publish(&mut out, "a.b", Some("_INBOX.x"), b"").unwrap();
        ping(&mut out);
        pong(&mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "SUB orders.> 1\r\nSUB work.* workers 22\r\nUNSUB 1\r\nUNSUB 22 5\r\nPUB a.b 2\r\nhi\r\nPUB a.b _INBOX.x 0\r\n\r\nPING\r\nPONG\r\n");
    }

    #[test]
    fn bad_subjects_are_refused() {
        let mut out = Vec::new();
        assert!(publish(&mut out, "a.*", None, b"").is_err());
        assert!(publish(&mut out, "a b", None, b"").is_err());
        assert!(publish(&mut out, "a", Some("r.>"), b"").is_err());
        assert!(sub(&mut out, "a..b", None, 1).is_err());
        assert!(sub(&mut out, "a", Some("q q"), 1).is_err());
        assert!(out.is_empty());
    }

    #[test]
    fn hpub_reads_back_as_hmsg() {
        let mut h = HeaderMap::new();
        h.insert("Nats-Msg-Id", "42");
        let mut out = Vec::new();
        message(&mut out, "a.b", Some("r"), &h, b"body").unwrap();
        let text = String::from_utf8(out.clone()).unwrap();
        assert!(text.starts_with("HPUB a.b r 29 33\r\nNATS/1.0\r\nNats-Msg-Id: 42\r\n\r\nbody\r\n"), "{text}");
        // What the server would send back for it.
        let line_end = out.windows(2).position(|w| w == b"\r\n").unwrap();
        let wire = [b"HMSG a.b 7 r 29 33".as_slice(), &out[line_end..]].concat();
        let mut p = Parser::new();
        let mut got = None;
        p.feed(&wire, |op| {
            if let Op::Msg(m) = op {
                got = Some((m.sid, m.reply.map(str::to_string), m.payload.to_vec(), crate::headers::Headers::parse(m.headers.unwrap()).unwrap().get("nats-msg-id").map(str::to_string)));
            }
        })
        .unwrap();
        assert_eq!(got, Some((7, Some("r".into()), b"body".to_vec(), Some("42".into()))));
    }

    #[test]
    fn numbers_are_written_in_decimal() {
        let mut out = Vec::new();
        for n in [0u64, 9, 10, 12345, u64::MAX] {
            out.clear();
            decimal(&mut out, n);
            assert_eq!(String::from_utf8(out.clone()).unwrap(), n.to_string());
        }
    }
}
