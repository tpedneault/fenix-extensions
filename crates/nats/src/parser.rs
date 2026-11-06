//! What a server sends, read as it comes.
//!
//! [`Parser::feed`] is given each chunk read from the socket and calls back
//! with each operation in it that's whole. An operation in one chunk is
//! read where it is: its subject, headers and payload are slices of the
//! chunk. Only one cut across two chunks is copied, and only that one --
//! into a buffer the parser keeps and reuses, so a stream of messages
//! costs no allocation at all once that buffer has grown to the largest
//! message seen.

use std::fmt;

/// The longest control line taken (`MSG ... 1024\r\n`): a server's own
/// `max_control_line` is 4 KiB; a line longer than this without its end is
/// taken for garbage rather than waited on forever.
pub const MAX_LINE: usize = 64 * 1024;

/// The largest message taken, headers and payload: a server's
/// `max_payload` is at most 64 MiB.
pub const MAX_MESSAGE: usize = 64 * 1024 * 1024;

/// One operation from the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op<'a> {
    /// `INFO {...}`: the server's JSON ([`crate::ServerInfo`]).
    Info(&'a str),
    /// `MSG` or `HMSG`.
    Msg(Msg<'a>),
    Ping,
    Pong,
    /// `+OK`, in verbose mode only.
    Ok,
    /// `-ERR 'text'`: the text, its quotes taken off.
    Err(&'a str),
}

/// A message, borrowed from what was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Msg<'a> {
    pub subject: &'a str,
    /// The subscription it came on.
    pub sid: u64,
    pub reply: Option<&'a str>,
    /// `HMSG`'s header block, `NATS/1.0` and all, with its blank line.
    pub headers: Option<&'a [u8]>,
    pub payload: &'a [u8],
}

impl Msg<'_> {
    /// Its size on the wire, as the server counts it: headers and payload.
    pub fn size(&self) -> usize {
        self.headers.map_or(0, <[u8]>::len) + self.payload.len()
    }
}

/// What made the stream unreadable. The connection can't go on after one:
/// where the next operation starts is lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// A control line longer than [`MAX_LINE`].
    LineTooLong,
    /// A message bigger than [`MAX_MESSAGE`].
    TooBig(usize),
    /// An operation it doesn't know.
    Unknown(String),
    /// One it knows, written wrong.
    Malformed(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::LineTooLong => write!(f, "a control line longer than {MAX_LINE} bytes"),
            ParseError::TooBig(n) => write!(f, "a message of {n} bytes, more than {MAX_MESSAGE}"),
            ParseError::Unknown(op) => write!(f, "unknown operation {op:?}"),
            ParseError::Malformed(line) => write!(f, "malformed: {line:?}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// What one look at the front of the buffer found.
enum Step<'a> {
    /// An operation, and how many bytes it took.
    Done(Option<Op<'a>>, usize),
    /// Not all there: how many bytes the whole operation is, once that's
    /// known (its line has come), else 0.
    More(usize),
}

/// Reads a server's stream, a chunk at a time.
#[derive(Debug, Default)]
pub struct Parser {
    /// The start of an operation cut by the end of a chunk.
    pending: Vec<u8>,
    /// How long that operation is, once its line has come; 0 before.
    need: usize,
}

impl Parser {
    pub fn new() -> Parser {
        Parser::default()
    }

    /// Bytes held back: the start of an operation still coming.
    pub fn buffered(&self) -> usize {
        self.pending.len()
    }

    /// Forgets what it held: a new connection starts clean.
    pub fn reset(&mut self) {
        self.pending.clear();
        self.need = 0;
    }

    /// Reads `data`, what came after what it was given before, calling `f`
    /// with each operation that's whole. Returns how many there were.
    pub fn feed<F: FnMut(Op<'_>)>(&mut self, mut data: &[u8], mut f: F) -> Result<usize, ParseError> {
        let mut ops = 0;
        if !self.pending.is_empty() {
            // Finish the one cut off, copying only what it lacks.
            if self.need == 0 {
                let Some(nl) = memchr(b'\n', data) else {
                    if self.pending.len() + data.len() > MAX_LINE {
                        return Err(ParseError::LineTooLong);
                    }
                    self.pending.extend_from_slice(data);
                    return Ok(0);
                };
                self.pending.extend_from_slice(&data[..=nl]);
                data = &data[nl + 1..];
                match step(&self.pending)? {
                    Step::Done(op, _) => {
                        if let Some(op) = op {
                            f(op);
                            ops += 1;
                        }
                        self.pending.clear();
                    }
                    Step::More(total) => self.need = total,
                }
            }
            if self.need > 0 {
                let want = self.need - self.pending.len();
                if data.len() < want {
                    self.pending.extend_from_slice(data);
                    return Ok(ops);
                }
                self.pending.extend_from_slice(&data[..want]);
                data = &data[want..];
                match step(&self.pending)? {
                    Step::Done(op, _) => {
                        if let Some(op) = op {
                            f(op);
                            ops += 1;
                        }
                    }
                    Step::More(_) => unreachable!("an operation's whole length was there"),
                }
                self.pending.clear();
                self.need = 0;
            }
        }
        // The rest, read in place.
        let mut at = 0;
        while at < data.len() {
            match step(&data[at..])? {
                Step::Done(op, used) => {
                    if let Some(op) = op {
                        f(op);
                        ops += 1;
                    }
                    at += used;
                }
                Step::More(total) => {
                    self.pending.extend_from_slice(&data[at..]);
                    self.need = total;
                    break;
                }
            }
        }
        Ok(ops)
    }
}

/// The first `byte` in `hay`: eight bytes at a time.
#[inline]
fn memchr(byte: u8, hay: &[u8]) -> Option<usize> {
    const LO: u64 = 0x0101_0101_0101_0101;
    const HI: u64 = 0x8080_8080_8080_8080;
    let pat = LO * byte as u64;
    let mut i = 0;
    while i + 8 <= hay.len() {
        let w = u64::from_le_bytes(hay[i..i + 8].try_into().unwrap()) ^ pat;
        if (w.wrapping_sub(LO) & !w & HI) != 0 {
            break;
        }
        i += 8;
    }
    hay[i..].iter().position(|b| *b == byte).map(|p| p + i)
}

fn number(token: &[u8], line: &[u8]) -> Result<u64, ParseError> {
    if token.is_empty() || token.len() > 19 {
        return Err(malformed(line));
    }
    let mut n: u64 = 0;
    for b in token {
        if !b.is_ascii_digit() {
            return Err(malformed(line));
        }
        n = n * 10 + (b - b'0') as u64;
    }
    Ok(n)
}

fn malformed(line: &[u8]) -> ParseError {
    let mut text = String::from_utf8_lossy(&line[..line.len().min(200)]).into_owned();
    text.truncate(text.trim_end().len());
    ParseError::Malformed(text)
}

fn utf8<'a>(bytes: &'a [u8], line: &[u8]) -> Result<&'a str, ParseError> {
    std::str::from_utf8(bytes).map_err(|_| malformed(line))
}

/// Splits a control line's arguments on spaces and tabs, into `out`;
/// how many there were (at most `out.len()`, or `None` for more).
#[inline]
fn split<'a>(mut s: &'a [u8], out: &mut [&'a [u8]]) -> Option<usize> {
    let mut n = 0;
    loop {
        while let [b' ' | b'\t', rest @ ..] = s {
            s = rest;
        }
        if s.is_empty() {
            return Some(n);
        }
        let end = s.iter().position(|b| *b == b' ' || *b == b'\t').unwrap_or(s.len());
        if n == out.len() {
            return None;
        }
        out[n] = &s[..end];
        n += 1;
        s = &s[end..];
    }
}

/// Reads the operation at the front of `buf`.
#[inline]
fn step(buf: &[u8]) -> Result<Step<'_>, ParseError> {
    let Some(nl) = memchr(b'\n', buf) else {
        return if buf.len() > MAX_LINE { Err(ParseError::LineTooLong) } else { Ok(Step::More(0)) };
    };
    if nl > MAX_LINE {
        return Err(ParseError::LineTooLong);
    }
    let line = &buf[..nl];
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let after = nl + 1;
    let (name, rest) = match line.iter().position(|b| *b == b' ' || *b == b'\t') {
        Some(i) => (&line[..i], &line[i + 1..]),
        None => (line, &line[line.len()..]),
    };
    let is = |op: &[u8]| name.eq_ignore_ascii_case(op);
    if is(b"MSG") || is(b"HMSG") {
        let headed = name.len() == 4;
        let mut args: [&[u8]; 5] = [&[]; 5];
        let n = split(rest, &mut args).ok_or_else(|| malformed(line))?;
        // MSG subject sid [reply] size; HMSG subject sid [reply] hsize size
        let fixed = if headed { 4 } else { 3 };
        if n != fixed && n != fixed + 1 {
            return Err(malformed(line));
        }
        let reply = (n == fixed + 1).then(|| args[2]);
        let total = number(args[n - 1], line)? as usize;
        let head = if headed { number(args[n - 2], line)? as usize } else { 0 };
        if head > total {
            return Err(malformed(line));
        }
        if total > MAX_MESSAGE {
            return Err(ParseError::TooBig(total));
        }
        let end = after + total + 2;
        if buf.len() < end {
            return Ok(Step::More(end));
        }
        if &buf[end - 2..end] != b"\r\n" {
            return Err(malformed(line));
        }
        let body = &buf[after..after + total];
        let msg = Msg {
            subject: utf8(args[0], line)?,
            sid: number(args[1], line)?,
            reply: reply.map(|r| utf8(r, line)).transpose()?,
            headers: headed.then(|| &body[..head]),
            payload: &body[head..],
        };
        return Ok(Step::Done(Some(Op::Msg(msg)), end));
    }
    let op = if is(b"PING") {
        Op::Ping
    } else if is(b"PONG") {
        Op::Pong
    } else if is(b"+OK") {
        Op::Ok
    } else if is(b"-ERR") {
        let text = utf8(rest, line)?.trim();
        Op::Err(text.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')).unwrap_or(text))
    } else if is(b"INFO") {
        Op::Info(utf8(rest, line)?.trim())
    } else if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(Step::Done(None, after));
    } else {
        return Err(ParseError::Unknown(String::from_utf8_lossy(&name[..name.len().min(40)]).into_owned()));
    };
    Ok(Step::Done(Some(op), after))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An owned copy of an operation, to compare.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Owned {
        Info(String),
        Msg { subject: String, sid: u64, reply: Option<String>, headers: Option<Vec<u8>>, payload: Vec<u8> },
        Ping,
        Pong,
        Ok,
        Err(String),
    }

    fn own(op: Op<'_>) -> Owned {
        match op {
            Op::Info(s) => Owned::Info(s.into()),
            Op::Msg(m) => Owned::Msg { subject: m.subject.into(), sid: m.sid, reply: m.reply.map(Into::into), headers: m.headers.map(<[u8]>::to_vec), payload: m.payload.to_vec() },
            Op::Ping => Owned::Ping,
            Op::Pong => Owned::Pong,
            Op::Ok => Owned::Ok,
            Op::Err(s) => Owned::Err(s.into()),
        }
    }

    fn all(chunks: &[&[u8]]) -> Result<Vec<Owned>, ParseError> {
        let mut p = Parser::new();
        let mut out = Vec::new();
        for c in chunks {
            p.feed(c, |op| out.push(own(op)))?;
        }
        Ok(out)
    }

    const STREAM: &[u8] = b"INFO {\"server_id\":\"x\",\"version\":\"2.10.1\",\"headers\":true}\r\n\
PING\r\n\
MSG orders.new 1 5\r\nhello\r\n\
MSG orders.new 1 _INBOX.abc 0\r\n\r\n\
HMSG svc.time 2 18 23\r\nNATS/1.0\r\nA: b\r\n\r\nhello\r\n\
HMSG svc.time 2 reply.to 12 12\r\nNATS/1.0\r\n\r\n\r\n\
+OK\r\n\
PONG\r\n\
-ERR 'Unknown Subject'\r\n\
msg\tlower.case\t9\t3\r\n\r\n\n\r\n";

    fn expected() -> Vec<Owned> {
        vec![
            Owned::Info("{\"server_id\":\"x\",\"version\":\"2.10.1\",\"headers\":true}".into()),
            Owned::Ping,
            Owned::Msg { subject: "orders.new".into(), sid: 1, reply: None, headers: None, payload: b"hello".to_vec() },
            Owned::Msg { subject: "orders.new".into(), sid: 1, reply: Some("_INBOX.abc".into()), headers: None, payload: vec![] },
            Owned::Msg { subject: "svc.time".into(), sid: 2, reply: None, headers: Some(b"NATS/1.0\r\nA: b\r\n\r\n".to_vec()), payload: b"hello".to_vec() },
            Owned::Msg { subject: "svc.time".into(), sid: 2, reply: Some("reply.to".into()), headers: Some(b"NATS/1.0\r\n\r\n".to_vec()), payload: vec![] },
            Owned::Ok,
            Owned::Pong,
            Owned::Err("Unknown Subject".into()),
            Owned::Msg { subject: "lower.case".into(), sid: 9, reply: None, headers: None, payload: b"\r\n\n".to_vec() },
        ]
    }

    #[test]
    fn a_stream_is_read_whole() {
        assert_eq!(all(&[STREAM]).unwrap(), expected());
    }

    #[test]
    fn a_stream_cut_at_every_byte_reads_the_same() {
        for cut in 0..=STREAM.len() {
            let (a, b) = STREAM.split_at(cut);
            assert_eq!(all(&[a, b]).unwrap(), expected(), "cut at {cut}");
        }
    }

    #[test]
    fn a_stream_cut_twice_anywhere_reads_the_same() {
        for i in 0..=STREAM.len() {
            for j in i..=STREAM.len() {
                assert_eq!(all(&[&STREAM[..i], &STREAM[i..j], &STREAM[j..]]).unwrap(), expected(), "cut at {i} and {j}");
            }
        }
    }

    #[test]
    fn a_stream_a_byte_at_a_time_reads_the_same() {
        let bytes: Vec<&[u8]> = STREAM.chunks(1).collect();
        assert_eq!(all(&bytes).unwrap(), expected());
    }

    #[test]
    fn only_a_cut_message_is_kept() {
        let mut p = Parser::new();
        let mut n = 0;
        p.feed(b"MSG a 1 3\r\nabc\r\nMSG b 1 4\r\nab", |_| n += 1).unwrap();
        assert_eq!((n, p.buffered()), (1, 13));
        p.feed(b"cd\r\nPING\r\n", |_| n += 1).unwrap();
        assert_eq!((n, p.buffered()), (3, 0));
    }

    #[test]
    fn what_isnt_nats_is_an_error() {
        assert!(matches!(all(&[b"HELLO there\r\n"]), Err(ParseError::Unknown(_))));
        assert!(matches!(all(&[b"MSG a 1\r\n"]), Err(ParseError::Malformed(_))));
        assert!(matches!(all(&[b"MSG a 1 x\r\n"]), Err(ParseError::Malformed(_))));
        assert!(matches!(all(&[b"MSG a 1 3\r\nabcde\r\n"]), Err(ParseError::Malformed(_))));
        assert!(matches!(all(&[b"HMSG a 1 9 3\r\n"]), Err(ParseError::Malformed(_))));
        assert!(matches!(all(&[b"MSG a 1 999999999999\r\n"]), Err(ParseError::TooBig(_))));
        let long = vec![b'x'; MAX_LINE + 1];
        assert_eq!(all(&[&long]), Err(ParseError::LineTooLong));
        let mut p = Parser::new();
        p.feed(&long[..MAX_LINE / 2], |_| {}).unwrap();
        assert_eq!(p.feed(&long[..MAX_LINE / 2 + 2], |_| {}), Err(ParseError::LineTooLong));
    }

    #[test]
    fn memchr_finds_the_first() {
        let hay: Vec<u8> = (0..100u8).map(|i| if i == 37 || i == 80 { b'\n' } else { b'a' }).collect();
        assert_eq!(memchr(b'\n', &hay), Some(37));
        assert_eq!(memchr(b'\n', &hay[38..]), Some(42));
        assert_eq!(memchr(b'\n', b"abc"), None);
        for n in 0..20 {
            let mut v = vec![b'x'; n];
            v.push(b'\n');
            assert_eq!(memchr(b'\n', &v), Some(n));
        }
    }

    #[test]
    fn many_messages_in_odd_chunks() {
        let mut wire = Vec::new();
        for i in 0..500u32 {
            let payload = vec![b'a' + (i % 26) as u8; (i as usize * 7) % 300];
            if i % 3 == 0 {
                let h = b"NATS/1.0\r\nK: v\r\n\r\n";
                wire.extend_from_slice(format!("HMSG s.{i} {i} {} {}\r\n", h.len(), h.len() + payload.len()).as_bytes());
                wire.extend_from_slice(h);
            } else {
                wire.extend_from_slice(format!("MSG s.{i} {i} {}\r\n", payload.len()).as_bytes());
            }
            wire.extend_from_slice(&payload);
            wire.extend_from_slice(b"\r\n");
        }
        for size in [1, 2, 3, 7, 64, 333, 4096] {
            let mut p = Parser::new();
            let mut seen = Vec::new();
            for c in wire.chunks(size) {
                p.feed(c, |op| {
                    if let Op::Msg(m) = op {
                        seen.push((m.sid, m.payload.len(), m.headers.is_some(), m.subject.to_string()));
                    }
                })
                .unwrap();
            }
            assert_eq!(seen.len(), 500, "chunks of {size}");
            for (i, (sid, len, headed, subject)) in seen.into_iter().enumerate() {
                assert_eq!((sid, len, headed, subject), (i as u64, (i * 7) % 300, i % 3 == 0, format!("s.{i}")));
            }
        }
    }
}
