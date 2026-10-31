//! SQL Server's TDS (7.4, and 8 with `tls=strict`), as a client: PRELOGIN,
//! LOGIN7 with SQL Server's own sign-in, then SQL batches, their tokens
//! read into rows -- the types that come back, decoded to text.
//!
//! TLS: TDS 7's is TLS's records carried inside PRELOGIN packets, which a
//! socket that does TLS itself can't carry; so a server that needs it
//! (`ENCRYPT_REQ`, or Azure SQL) is reached with TDS 8 (`tls=strict`),
//! TLS from the first byte, or not at all.

use crate::conn::Tls;
use crate::result::{hex, Column, Kind, Outcome, Rows};
use crate::wire::{Action, Client, Login};

const PRELOGIN: u8 = 0x12;
const LOGIN7: u8 = 0x10;
const SQL_BATCH: u8 = 0x01;
const REPLY: u8 = 0x04;

/// PRELOGIN's ENCRYPTION values.
const ENCRYPT_OFF: u8 = 0x00;
const ENCRYPT_NOT_SUP: u8 = 0x02;

const TDS_74: u32 = 0x7400_0004;
const TDS_80: u32 = 0x0800_0000;

const PACKET: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
enum State {
    Prelogin,
    Login,
    Idle,
    Busy,
    Closed,
}

pub struct Mssql {
    login: Login,
    tls: Tls,
    host: String,
    state: State,
    buf: Vec<u8>,
    /// A message's packets so far.
    message: Vec<u8>,
    packet_id: u8,
    pub server_version: String,
}

impl Mssql {
    pub fn new(login: Login, tls: Tls, host: &str) -> Mssql {
        Mssql { login, tls, host: host.to_string(), state: State::Prelogin, buf: Vec::new(), message: Vec::new(), packet_id: 0, server_version: String::new() }
    }

    /// `payload` in packets of `kind`, the last marked end-of-message.
    fn packets(&mut self, kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let chunks: Vec<&[u8]> = if payload.is_empty() { vec![&[][..]] } else { payload.chunks(PACKET - 8).collect() };
        for (i, c) in chunks.iter().enumerate() {
            let last = i + 1 == chunks.len();
            self.packet_id = self.packet_id.wrapping_add(1);
            out.push(kind);
            out.push(u8::from(last));
            out.extend_from_slice(&((c.len() + 8) as u16).to_be_bytes());
            out.extend_from_slice(&[0, 0, self.packet_id, 0]);
            out.extend_from_slice(c);
        }
        out
    }

    fn prelogin(&mut self) -> Vec<u8> {
        let encryption = if self.tls == Tls::Strict { ENCRYPT_OFF } else { ENCRYPT_NOT_SUP };
        // VERSION, ENCRYPTION, INSTOPT, THREADID, MARS: offsets after the
        // 5-byte entries and the terminator.
        let data: [&[u8]; 5] = [&[0x10, 0, 0, 0, 0, 0], &[encryption], &[0], &[0, 0, 0, 0], &[0]];
        let mut head = Vec::new();
        let mut body = Vec::new();
        let mut offset = (data.len() * 5 + 1) as u16;
        for (token, d) in data.iter().enumerate() {
            head.push(token as u8);
            head.extend_from_slice(&offset.to_be_bytes());
            head.extend_from_slice(&(d.len() as u16).to_be_bytes());
            body.extend_from_slice(d);
            offset += d.len() as u16;
        }
        head.push(0xFF);
        head.extend(body);
        self.packets(PRELOGIN, &head)
    }

    fn login7(&mut self) -> Vec<u8> {
        let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
        let strings: [Vec<u8>; 9] =
            [utf16("fenix"), utf16(&self.login.user), obfuscate(&self.login.password), utf16("Fenix"), utf16(&self.host), Vec::new(), utf16("fenix-database"), Vec::new(), utf16(&self.login.database)];
        let fixed = 94usize;
        let mut b = Vec::new();
        b.extend_from_slice(&[0; 4]); // its length, below
        b.extend_from_slice(&(if self.tls == Tls::Strict { TDS_80 } else { TDS_74 }).to_le_bytes());
        b.extend_from_slice(&(PACKET as u32).to_le_bytes());
        b.extend_from_slice(&[0, 0, 0, 7]); // the client's version
        b.extend_from_slice(&std::process::id().to_le_bytes());
        b.extend_from_slice(&[0; 4]); // the connection's id
                                      // OptionFlags1: USE_DB_ON, INIT_DB_FATAL, SET_LANG_ON; 2: ODBC
                                      // (ANSI defaults on); TypeFlags; OptionFlags3.
        b.extend_from_slice(&[0xE0, 0x03, 0, 0]);
        b.extend_from_slice(&[0; 4]); // the time zone
        b.extend_from_slice(&0x0409u32.to_le_bytes()); // the LCID: en-US
        let mut offset = fixed;
        let mut tail = Vec::new();
        for s in &strings {
            b.extend_from_slice(&(offset as u16).to_le_bytes());
            b.extend_from_slice(&((s.len() / 2) as u16).to_le_bytes());
            tail.extend_from_slice(s);
            offset += s.len();
        }
        b.extend_from_slice(&[0; 6]); // the client's MAC
        for _ in 0..3 {
            // SSPI, the file to attach, the new password: none.
            b.extend_from_slice(&(offset as u16).to_le_bytes());
            b.extend_from_slice(&[0, 0]);
        }
        b.extend_from_slice(&[0; 4]); // SSPI's long length
        debug_assert_eq!(b.len(), fixed);
        b.extend(tail);
        let len = b.len() as u32;
        b[..4].copy_from_slice(&len.to_le_bytes());
        self.packets(LOGIN7, &b)
    }

    fn reply(&mut self, msg: Vec<u8>, out: &mut Vec<Action>) {
        match self.state {
            State::Prelogin => {
                let encryption = prelogin_option(&msg, 1).and_then(|d| d.first().copied()).unwrap_or(ENCRYPT_NOT_SUP);
                if let Some(v) = prelogin_option(&msg, 0).filter(|v| v.len() >= 4) {
                    self.server_version = format!("{}.{}.{}", v[0], v[1], u16::from_be_bytes([v[2], v[3]]));
                }
                if self.tls != Tls::Strict && !matches!(encryption, ENCRYPT_NOT_SUP | ENCRYPT_OFF) {
                    self.state = State::Closed;
                    out.push(Action::Failed("the server wants TLS within TDS 7, which can't be done here: give the connection tls=strict for TDS 8 (SQL Server 2022, Azure SQL), or let it go without (Force Encryption off)".into()));
                    return;
                }
                if self.tls == Tls::Require {
                    self.state = State::Closed;
                    out.push(Action::Failed("TLS on SQL Server is TDS 8's: give the connection tls=strict".into()));
                    return;
                }
                self.state = State::Login;
                let pkt = self.login7();
                out.push(Action::Send(pkt));
            }
            State::Login => {
                let tokens = parse_tokens(&msg, self.login.max_rows);
                if let Some(e) = tokens.errors.first() {
                    self.state = State::Closed;
                    out.push(Action::Failed(e.clone()));
                } else if tokens.logged_in {
                    if !tokens.server.is_empty() {
                        self.server_version = tokens.server;
                    }
                    self.state = State::Idle;
                    out.push(Action::Ready);
                } else {
                    self.state = State::Closed;
                    out.push(Action::Failed("the server didn't say it signed in (no LOGINACK)".into()));
                }
            }
            State::Busy => {
                self.state = State::Idle;
                out.push(Action::Done(parse_tokens(&msg, self.login.max_rows).outcomes));
            }
            State::Idle | State::Closed => {}
        }
    }
}

impl Client for Mssql {
    fn start(&mut self) -> Vec<Action> {
        self.state = State::Prelogin;
        let p = self.prelogin();
        vec![Action::Send(p)]
    }

    fn tls_ready(&mut self) -> Vec<Action> {
        Vec::new()
    }

    fn receive(&mut self, data: &[u8]) -> Vec<Action> {
        let mut out = Vec::new();
        self.buf.extend_from_slice(data);
        let mut at = 0;
        while self.buf.len() - at >= 8 {
            let len = u16::from_be_bytes([self.buf[at + 2], self.buf[at + 3]]) as usize;
            if len < 8 || self.buf.len() - at < len {
                break;
            }
            let (kind, status) = (self.buf[at], self.buf[at + 1]);
            self.message.extend_from_slice(&self.buf[at + 8..at + len]);
            at += len;
            if kind == REPLY && status & 1 != 0 {
                let msg = std::mem::take(&mut self.message);
                self.reply(msg, &mut out);
                if self.state == State::Closed {
                    break;
                }
            }
        }
        self.buf.drain(..at);
        out
    }

    fn query(&mut self, sql: &str) -> Vec<Action> {
        self.state = State::Busy;
        // ALL_HEADERS: the transaction descriptor's, none open, one request.
        let mut b = 22u32.to_le_bytes().to_vec();
        b.extend_from_slice(&18u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&[0; 8]);
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend(sql.encode_utf16().flat_map(u16::to_le_bytes));
        let p = self.packets(SQL_BATCH, &b);
        vec![Action::Send(p)]
    }

    fn goodbye(&mut self) -> Vec<u8> {
        self.state = State::Closed;
        Vec::new()
    }
}

/// LOGIN7's password: UTF-16, each byte's halves swapped and XORed with
/// 0xA5.
pub fn obfuscate(password: &str) -> Vec<u8> {
    password.encode_utf16().flat_map(u16::to_le_bytes).map(|b| (b.rotate_left(4)) ^ 0xA5).collect()
}

/// A PRELOGIN option's data, by token.
fn prelogin_option(msg: &[u8], token: u8) -> Option<&[u8]> {
    let mut at = 0;
    while at + 5 <= msg.len() && msg[at] != 0xFF {
        if msg[at] == token {
            let off = u16::from_be_bytes([msg[at + 1], msg[at + 2]]) as usize;
            let len = u16::from_be_bytes([msg[at + 3], msg[at + 4]]) as usize;
            return msg.get(off..off + len);
        }
        at += 5;
    }
    None
}

/// What a reply's tokens said.
#[derive(Debug, Default)]
struct Tokens {
    outcomes: Vec<Outcome>,
    errors: Vec<String>,
    logged_in: bool,
    server: String,
}

/// A column's type, as COLMETADATA gives it.
#[derive(Debug, Clone, PartialEq)]
struct Type {
    code: u8,
    /// The length its values give: 0 none (fixed), 1, 2 or 4 bytes; or
    /// PLP (`max`).
    len_size: u8,
    plp: bool,
    /// Fixed types' size.
    fixed: usize,
    scale: u8,
    /// Byte-length types' size, as declared: which of `intn`'s it is
    /// (`bigint`, `tinyint`), `floatn`'s (`real`).
    size: u8,
}

/// Reads bytes off a message.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at + n)?;
        self.at += n;
        Some(s)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_le_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|s| u64::from_le_bytes(s.try_into().unwrap_or_default()))
    }

    /// A string of `n` UTF-16 characters.
    fn utf16(&mut self, n: usize) -> Option<String> {
        let s = self.take(n * 2)?;
        Some(String::from_utf16_lossy(&s.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()))
    }

    fn b_varchar(&mut self) -> Option<String> {
        let n = self.u8()? as usize;
        self.utf16(n)
    }

    fn us_varchar(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        self.utf16(n)
    }
}

fn type_info(r: &mut Reader) -> Option<Type> {
    let code = r.u8()?;
    let fixed = |n: usize| Some(Type { code, len_size: 0, plp: false, fixed: n, scale: 0, size: 0 });
    match code {
        0x1F => fixed(0),
        0x30 | 0x32 => fixed(1),
        0x34 => fixed(2),
        0x38 | 0x3A | 0x3B | 0x7A => fixed(4),
        0x3C | 0x3D | 0x3E | 0x7F => fixed(8),
        // Byte-length types: their size, then for decimals the precision
        // and scale.
        0x24 | 0x26 | 0x68 | 0x6D | 0x6E | 0x6F => {
            let size = r.u8()?;
            Some(Type { code, len_size: 1, plp: false, fixed: 0, scale: 0, size })
        }
        0x6A | 0x6C | 0x37 | 0x3F => {
            r.u8()?;
            r.u8()?;
            let scale = r.u8()?;
            Some(Type { code, len_size: 1, plp: false, fixed: 0, scale, size: 0 })
        }
        0x28 => Some(Type { code, len_size: 1, plp: false, fixed: 0, scale: 0, size: 0 }),
        0x29..=0x2B => {
            let scale = r.u8()?;
            Some(Type { code, len_size: 1, plp: false, fixed: 0, scale, size: 0 })
        }
        // Up to 8,000 bytes, or `max` (0xFFFF): PLP.
        0xA5 | 0xA7 | 0xAD | 0xAF | 0xE7 | 0xEF => {
            let max = r.u16()?;
            if matches!(code, 0xA7 | 0xAF | 0xE7 | 0xEF) {
                r.take(5)?; // the collation
            }
            Some(Type { code, len_size: 2, plp: max == 0xFFFF, fixed: 0, scale: 0, size: 0 })
        }
        // text, ntext, image: a 4-byte length, the table's name after.
        0x22 | 0x23 | 0x63 => {
            r.u32()?;
            if code != 0x22 {
                r.take(5)?;
            }
            let parts = r.u8()?;
            for _ in 0..parts {
                r.us_varchar()?;
            }
            Some(Type { code, len_size: 4, plp: false, fixed: 0, scale: 0, size: 0 })
        }
        // xml: a schema maybe, then PLP.
        0xF1 => {
            if r.u8()? == 1 {
                r.b_varchar()?;
                r.b_varchar()?;
                r.us_varchar()?;
            }
            Some(Type { code, len_size: 0, plp: true, fixed: 0, scale: 0, size: 0 })
        }
        // sql_variant: a 4-byte length.
        0x62 => {
            r.u32()?;
            Some(Type { code, len_size: 4, plp: false, fixed: 0, scale: 0, size: 0 })
        }
        _ => None,
    }
}

fn type_name(t: &Type) -> (&'static str, Kind) {
    match t.code {
        0x30 => ("tinyint", Kind::Number),
        0x32 | 0x68 => ("bit", Kind::Bool),
        0x34 => ("smallint", Kind::Number),
        0x38 => ("int", Kind::Number),
        0x7F => ("bigint", Kind::Number),
        0x26 => match t.size {
            1 => ("tinyint", Kind::Number),
            2 => ("smallint", Kind::Number),
            8 => ("bigint", Kind::Number),
            _ => ("int", Kind::Number),
        },
        0x3B => ("real", Kind::Number),
        0x6D if t.size == 4 => ("real", Kind::Number),
        0x3E | 0x6D => ("float", Kind::Number),
        0x7A => ("smallmoney", Kind::Number),
        0x6E if t.size == 4 => ("smallmoney", Kind::Number),
        0x3C | 0x6E => ("money", Kind::Number),
        0x6A | 0x6C | 0x37 | 0x3F => ("decimal", Kind::Number),
        0x3A => ("smalldatetime", Kind::Text),
        0x6F if t.size == 4 => ("smalldatetime", Kind::Text),
        0x3D | 0x6F => ("datetime", Kind::Text),
        0x28 => ("date", Kind::Text),
        0x29 => ("time", Kind::Text),
        0x2A => ("datetime2", Kind::Text),
        0x2B => ("datetimeoffset", Kind::Text),
        0x24 => ("uniqueidentifier", Kind::Text),
        0xA7 | 0xAF | 0x23 => ("varchar", Kind::Text),
        0xE7 | 0xEF | 0x63 => ("nvarchar", Kind::Text),
        0xA5 | 0xAD | 0x22 => ("varbinary", Kind::Bytes),
        0xF1 => ("xml", Kind::Text),
        0x62 => ("sql_variant", Kind::Bytes),
        _ => ("", Kind::Text),
    }
}

/// A value's bytes, by its type; none for NULL. PLP's chunks are joined.
fn value_bytes(r: &mut Reader, t: &Type) -> Option<Option<Vec<u8>>> {
    if t.plp {
        let total = r.u64()?;
        if total == u64::MAX {
            return Some(None);
        }
        let mut v = Vec::new();
        loop {
            let n = r.u32()? as usize;
            if n == 0 {
                break;
            }
            v.extend_from_slice(r.take(n)?);
        }
        return Some(Some(v));
    }
    match t.len_size {
        0 => Some(if t.code == 0x1F { None } else { Some(r.take(t.fixed)?.to_vec()) }),
        1 => {
            let n = r.u8()? as usize;
            Some(if n == 0 { None } else { Some(r.take(n)?.to_vec()) })
        }
        2 => {
            let n = r.u16()?;
            Some(if n == 0xFFFF { None } else { Some(r.take(n as usize)?.to_vec()) })
        }
        _ => {
            // text, ntext, image: a text pointer, a timestamp, then it.
            if matches!(t.code, 0x22 | 0x23 | 0x63) {
                let ptr = r.u8()? as usize;
                if ptr == 0 {
                    return Some(None);
                }
                r.take(ptr + 8)?;
            }
            let n = r.u32()? as usize;
            Some(Some(r.take(n)?.to_vec()))
        }
    }
}

/// Days since 0001-01-01 as a date (the proleptic Gregorian calendar).
fn date_from_days(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's civil_from_days, from 1970-01-01.
    let z = days - 719_162 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn le_uint(b: &[u8]) -> u64 {
    b.iter().rev().fold(0u64, |acc, &x| acc << 8 | x as u64)
}

/// `units` of 10^-scale seconds as `hh:mm:ss[.fffffff]`.
fn time_text(units: u64, scale: u8) -> String {
    let per = 10u64.pow(scale as u32);
    let secs = units / per;
    let frac = units % per;
    let mut s = format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    if scale > 0 {
        s.push_str(&format!(".{:0width$}", frac, width = scale as usize));
    }
    s
}

/// Bytes of time: 3 to 5, by its scale.
fn time_len(scale: u8) -> usize {
    match scale {
        0..=2 => 3,
        3 | 4 => 4,
        _ => 5,
    }
}

/// A value, as text.
fn decode(v: &[u8], t: &Type) -> String {
    let int = |b: &[u8]| -> i64 {
        match b.len() {
            1 => b[0] as i64,
            2 => i16::from_le_bytes([b[0], b[1]]) as i64,
            4 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64,
            8 => i64::from_le_bytes(b.try_into().unwrap_or_default()),
            _ => 0,
        }
    };
    match t.code {
        0x30 | 0x34 | 0x38 | 0x7F | 0x26 => int(v).to_string(),
        0x32 | 0x68 => (if v.first() == Some(&0) { "0" } else { "1" }).into(),
        0x3B => f32::from_le_bytes(v[..4].try_into().unwrap_or_default()).to_string(),
        0x3E => f64::from_le_bytes(v[..8].try_into().unwrap_or_default()).to_string(),
        0x6D if v.len() == 4 => f32::from_le_bytes(v.try_into().unwrap_or_default()).to_string(),
        0x6D => f64::from_le_bytes(v[..8].try_into().unwrap_or_default()).to_string(),
        0x3C | 0x7A | 0x6E => {
            // Money: ten-thousandths; the 8-byte kind high half first.
            let n = if v.len() == 4 { int(v) } else { (i32::from_le_bytes(v[..4].try_into().unwrap_or_default()) as i64) << 32 | u32::from_le_bytes(v[4..8].try_into().unwrap_or_default()) as i64 };
            scaled(n.unsigned_abs() as u128, 4, n < 0)
        }
        0x6A | 0x6C | 0x37 | 0x3F => {
            let positive = v.first() == Some(&1);
            let n = v[1..].iter().rev().fold(0u128, |acc, &x| acc << 8 | x as u128);
            scaled(n, t.scale, !positive)
        }
        0x3D | 0x3A | 0x6F => {
            let (days, secs) = if v.len() == 4 {
                (u16::from_le_bytes([v[0], v[1]]) as i64, u16::from_le_bytes([v[2], v[3]]) as u64 * 60 * 300)
            } else {
                (i32::from_le_bytes(v[..4].try_into().unwrap_or_default()) as i64, u32::from_le_bytes(v[4..8].try_into().unwrap_or_default()) as u64)
            };
            // Days from 1900-01-01; ticks of a 300th of a second.
            let (y, m, d) = date_from_days(days + 693_595);
            let ms = (secs * 10 + 1) / 3;
            format!("{y:04}-{m:02}-{d:02} {}", time_text(ms, 3))
        }
        0x28 => {
            let (y, m, d) = date_from_days(le_uint(&v[..3]) as i64);
            format!("{y:04}-{m:02}-{d:02}")
        }
        0x29 => time_text(le_uint(v), t.scale),
        0x2A | 0x2B => {
            let tl = time_len(t.scale);
            let time = time_text(le_uint(&v[..tl]), t.scale);
            let (y, m, d) = date_from_days(le_uint(&v[tl..tl + 3]) as i64);
            let mut s = format!("{y:04}-{m:02}-{d:02} {time}");
            if t.code == 0x2B && v.len() >= tl + 5 {
                // Stored as UTC, its offset after: shown as UTC with it.
                let off = i16::from_le_bytes([v[tl + 3], v[tl + 4]]);
                s.push_str(&format!(" +00:00 (at {}{:02}:{:02})", if off < 0 { '-' } else { '+' }, off.unsigned_abs() / 60, off.unsigned_abs() % 60));
            }
            s
        }
        0x24 if v.len() == 16 => format!(
            "{:08X}-{:04X}-{:04X}-{}-{}",
            u32::from_le_bytes(v[..4].try_into().unwrap_or_default()),
            u16::from_le_bytes([v[4], v[5]]),
            u16::from_le_bytes([v[6], v[7]]),
            v[8..10].iter().map(|b| format!("{b:02X}")).collect::<String>(),
            v[10..].iter().map(|b| format!("{b:02X}")).collect::<String>()
        ),
        0xE7 | 0xEF | 0x63 | 0xF1 => String::from_utf16_lossy(&v.chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect::<Vec<_>>()),
        // varchar in the collation's code page: Latin-1 read as such.
        0xA7 | 0xAF | 0x23 => match std::str::from_utf8(v) {
            Ok(s) => s.to_string(),
            Err(_) => v.iter().map(|&b| b as char).collect(),
        },
        _ => hex(v),
    }
}

/// `n` with `scale` decimals.
fn scaled(n: u128, scale: u8, negative: bool) -> String {
    let s = n.to_string();
    let sign = if negative && n != 0 { "-" } else { "" };
    if scale == 0 {
        return format!("{sign}{s}");
    }
    let scale = scale as usize;
    let s = format!("{s:0>width$}", width = scale + 1);
    let (int, frac) = s.split_at(s.len() - scale);
    format!("{sign}{int}.{frac}")
}

/// A reply's tokens: results, errors, whether it signed in.
fn parse_tokens(msg: &[u8], max_rows: usize) -> Tokens {
    let mut t = Tokens::default();
    let mut r = Reader { b: msg, at: 0 };
    let mut columns: Vec<(Column, Type)> = Vec::new();
    let mut rows: Option<Rows> = None;
    let mut error: Option<String> = None;
    while let Some(token) = r.u8() {
        match token {
            0x81 => {
                // COLMETADATA.
                let Some(n) = r.u16() else { break };
                columns.clear();
                if n == 0xFFFF {
                    continue;
                }
                for _ in 0..n {
                    let (Some(_user), Some(_flags)) = (r.u32(), r.u16()) else { break };
                    let Some(ty) = type_info(&mut r) else {
                        t.outcomes.push(Outcome::Error("a column of a type not read here".into()));
                        return t;
                    };
                    let name = r.b_varchar().unwrap_or_default();
                    let (tn, kind) = type_name(&ty);
                    columns.push((Column::new(name, tn, kind), ty));
                }
                rows = Some(Rows { columns: columns.iter().map(|c| c.0.clone()).collect(), rows: Vec::new(), truncated: false });
            }
            0xD1 | 0xD2 => {
                // ROW, NBCROW: a NULL bitmap first.
                let nulls = if token == 0xD2 { r.take(columns.len().div_ceil(8)).map(<[u8]>::to_vec) } else { Some(Vec::new()) };
                let Some(nulls) = nulls else { break };
                let mut row = Vec::new();
                for (i, (_, ty)) in columns.iter().enumerate() {
                    if token == 0xD2 && nulls[i / 8] & (1 << (i % 8)) != 0 {
                        row.push(None);
                        continue;
                    }
                    match value_bytes(&mut r, ty) {
                        Some(v) => row.push(v.map(|v| decode(&v, ty))),
                        None => return t,
                    }
                }
                if let Some(rs) = &mut rows {
                    if rs.rows.len() >= max_rows.max(1) {
                        rs.truncated = true;
                    } else {
                        rs.rows.push(row);
                    }
                }
            }
            0xFD..=0xFF => {
                // DONE, DONEPROC, DONEINPROC.
                let (Some(status), Some(_cmd), Some(count)) = (r.u16(), r.u16(), r.u64()) else { break };
                if let Some(e) = error.take() {
                    t.outcomes.push(Outcome::Error(e));
                    rows = None;
                } else if let Some(rs) = rows.take() {
                    t.outcomes.push(Outcome::Rows(rs));
                } else if token == 0xFD && (status & 0x10 != 0 || t.outcomes.is_empty() && status & 0x01 == 0) {
                    t.outcomes.push(Outcome::Done { tag: String::new(), changed: (status & 0x10 != 0).then_some(count) });
                }
            }
            0xAA | 0xAB => {
                // ERROR, INFO.
                let Some(len) = r.u16() else { break };
                let start = r.at;
                let (Some(number), Some(_state), Some(class)) = (r.u32(), r.u8(), r.u8()) else { break };
                let text = r.us_varchar().unwrap_or_default();
                let _server = r.b_varchar();
                let _proc = r.b_varchar();
                let line = r.u32().unwrap_or(0);
                r.at = start + len as usize;
                if token == 0xAA {
                    let e = format!("Msg {number}, level {class}, line {line}: {text}");
                    t.errors.push(e.clone());
                    error = Some(e);
                }
            }
            0xAD => {
                // LOGINACK: its server's name and version.
                let Some(len) = r.u16() else { break };
                let start = r.at;
                r.take(1 + 4);
                let name = r.b_varchar().unwrap_or_default();
                let v = r.take(4).unwrap_or(&[0, 0, 0, 0]).to_vec();
                t.server = format!("{name} {}.{}.{}", v[0], v[1], u16::from_be_bytes([v[2], v[3]]));
                t.logged_in = true;
                r.at = start + len as usize;
            }
            0x79 => {
                r.take(4);
            }
            // ENVCHANGE, ORDER, TABNAME, COLINFO, RETURNVALUE: by length.
            0xE3 | 0xA9 | 0xA4 | 0xA5 | 0xAC => {
                let Some(len) = r.u16() else { break };
                r.at += len as usize;
            }
            // FEATUREEXTACK: features until 0xFF.
            0xAE => loop {
                match r.u8() {
                    Some(0xFF) | None => break,
                    Some(_) => {
                        let n = r.u32().unwrap_or(0) as usize;
                        r.at += n;
                    }
                }
            },
            0xE4 => {
                let n = r.u32().unwrap_or(0) as usize;
                r.at += n;
            }
            _ => {
                t.outcomes.push(Outcome::Error(format!("a token not read here (0x{token:02X})")));
                break;
            }
        }
    }
    if let Some(e) = error {
        t.outcomes.push(Outcome::Error(e));
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login() -> Login {
        Login { user: "sa".into(), password: "Pa$$w0rd".into(), database: "shop".into(), max_rows: 100, read_only: false }
    }

    /// A reply message from the server, in one packet.
    fn reply(payload: &[u8]) -> Vec<u8> {
        let mut p = vec![REPLY, 1];
        p.extend_from_slice(&((payload.len() + 8) as u16).to_be_bytes());
        p.extend_from_slice(&[0, 0, 1, 0]);
        p.extend_from_slice(payload);
        p
    }

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn the_password_is_obfuscated() {
        // 'a' (0x61 0x00): halves swapped, 0x16 0x00; XOR 0xA5: 0xB3 0xA5.
        assert_eq!(obfuscate("a"), [0xB3, 0xA5]);
        assert_eq!(obfuscate("").len(), 0);
    }

    #[test]
    fn prelogin_then_login7() {
        let mut m = Mssql::new(login(), Tls::Off, "db.example.com");
        let [Action::Send(p)] = &m.start()[..] else { panic!() };
        assert_eq!(&p[..2], &[PRELOGIN, 1]);
        assert_eq!(u16::from_be_bytes([p[2], p[3]]) as usize, p.len());
        assert_eq!(prelogin_option(&p[8..], 1), Some(&[ENCRYPT_NOT_SUP][..]));
        // The server: version 16.0.1000, encryption not supported either.
        let mut answer = vec![0, 0, 11, 0, 6, 1, 0, 17, 0, 1, 0xFF];
        answer.extend_from_slice(&[16, 0, 0x03, 0xE8, 0, 0, ENCRYPT_NOT_SUP]);
        let got = m.receive(&reply(&answer));
        let [Action::Send(login7)] = &got[..] else { panic!("{got:?}") };
        assert_eq!(m.server_version, "16.0.1000");
        assert_eq!(login7[0], LOGIN7);
        let body = &login7[8..];
        assert_eq!(u32::from_le_bytes(body[..4].try_into().unwrap()) as usize, body.len());
        assert_eq!(u32::from_le_bytes(body[4..8].try_into().unwrap()), TDS_74);
        // The user's name, where its offset says.
        let (off, len) = (u16::from_le_bytes([body[40], body[41]]) as usize, u16::from_le_bytes([body[42], body[43]]) as usize);
        assert_eq!(&body[off..off + len * 2], utf16("sa").as_slice());
        let (off, len) = (u16::from_le_bytes([body[44], body[45]]) as usize, u16::from_le_bytes([body[46], body[47]]) as usize);
        assert_eq!(&body[off..off + len * 2], obfuscate("Pa$$w0rd").as_slice());
        // A server that wants TLS within TDS 7: refused, with what to do.
        let mut m = Mssql::new(login(), Tls::Off, "db");
        m.start();
        let answer = [1, 0, 6, 0, 1, 0xFF, 3];
        assert!(matches!(&m.receive(&reply(&answer))[..], [Action::Failed(e)] if e.contains("tls=strict")));
    }

    #[test]
    fn a_login_ack_or_its_error() {
        let mut m = Mssql::new(login(), Tls::Off, "db");
        m.start();
        m.receive(&reply(&[0, 0, 6, 0, 1, 0xFF, ENCRYPT_NOT_SUP]));
        let name = utf16("Microsoft SQL Server");
        let mut ack = vec![1, 0x74, 0, 0, 4, 20];
        ack.extend_from_slice(&name);
        ack.extend_from_slice(&[16, 0, 0x03, 0xE8]);
        let mut msg = vec![0xE3, 3, 0, 1, 0, 0];
        msg.push(0xAD);
        msg.extend_from_slice(&(ack.len() as u16).to_le_bytes());
        msg.extend(ack);
        msg.extend_from_slice(&[0xFD, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(m.receive(&reply(&msg)), [Action::Ready]);
        assert_eq!(m.server_version, "Microsoft SQL Server 16.0.1000");

        let mut m = Mssql::new(login(), Tls::Off, "db");
        m.start();
        m.receive(&reply(&[0, 0, 6, 0, 1, 0xFF, ENCRYPT_NOT_SUP]));
        let text = utf16("Login failed for user 'sa'.");
        let mut e = 18456u32.to_le_bytes().to_vec();
        e.extend_from_slice(&[1, 14]);
        e.extend_from_slice(&(text.len() as u16 / 2).to_le_bytes());
        e.extend(text);
        e.extend_from_slice(&[0, 0, 1, 0, 0, 0]);
        let mut msg = vec![0xAA];
        msg.extend_from_slice(&(e.len() as u16).to_le_bytes());
        msg.extend(e);
        msg.extend_from_slice(&[0xFD, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(m.receive(&reply(&msg)), [Action::Failed("Msg 18456, level 14, line 1: Login failed for user 'sa'.".into())]);
    }

    fn col(ty: &[u8], name: &str) -> Vec<u8> {
        let mut c = vec![0, 0, 0, 0, 1, 0];
        c.extend_from_slice(ty);
        c.push(name.len() as u8);
        c.extend(utf16(name));
        c
    }

    #[test]
    fn a_batch_brings_rows_of_each_type() {
        let mut m = Mssql::new(login(), Tls::Off, "db");
        m.state = State::Idle;
        let [Action::Send(p)] = &m.query("select 1")[..] else { panic!() };
        assert_eq!(p[0], SQL_BATCH);
        assert_eq!(&p[8..12], &22u32.to_le_bytes());
        assert_eq!(&p[30..], utf16("select 1").as_slice());

        let mut msg = vec![0x81, 8, 0];
        msg.extend(col(&[0x26, 4], "id"));
        msg.extend(col(&[0xE7, 0x40, 0, 9, 4, 0xD0, 0, 0x34], "name"));
        msg.extend(col(&[0x6C, 9, 10, 2], "price"));
        msg.extend(col(&[0x28], "day"));
        msg.extend(col(&[0x2A, 7], "at"));
        msg.extend(col(&[0x68, 1], "ok"));
        msg.extend(col(&[0x24, 16], "uid"));
        msg.extend(col(&[0xA5, 0xFF, 0xFF], "blob"));
        // 7, N'Zoë', 12.34, 2024-02-29, 2024-02-29 13:45:30.1234567, 1,
        // a uuid, 0x0102 in two PLP chunks.
        msg.push(0xD1);
        msg.extend_from_slice(&[4, 7, 0, 0, 0]);
        let zoe = utf16("Zoë");
        msg.extend_from_slice(&(zoe.len() as u16).to_le_bytes());
        msg.extend(zoe);
        msg.extend_from_slice(&[5, 1]);
        msg.extend_from_slice(&1234u32.to_le_bytes());
        let days = 738_944u32; // 2024-02-29 from 0001-01-01
        msg.push(3);
        msg.extend_from_slice(&days.to_le_bytes()[..3]);
        let units: u64 = (13 * 3600 + 45 * 60 + 30) * 10_000_000 + 1_234_567;
        msg.push(8);
        msg.extend_from_slice(&units.to_le_bytes()[..5]);
        msg.extend_from_slice(&days.to_le_bytes()[..3]);
        msg.extend_from_slice(&[1, 1]);
        msg.push(16);
        msg.extend_from_slice(&[0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        msg.extend_from_slice(&2u64.to_le_bytes());
        msg.extend_from_slice(&[1, 0, 0, 0, 1, 1, 0, 0, 0, 2, 0, 0, 0, 0]);
        // A row of NULLs, as NBCROW: all eight bits set.
        msg.extend_from_slice(&[0xD2, 0xFF]);
        msg.extend_from_slice(&[0xFF, 0x10, 0, 0xC1, 0, 2, 0, 0, 0, 0, 0, 0, 0]);
        // Then an UPDATE's count, and an error.
        msg.extend_from_slice(&[0xFD, 0x11, 0, 0xC5, 0, 3, 0, 0, 0, 0, 0, 0, 0]);
        let text = utf16("Invalid object name 'nope'.");
        let mut e = 208u32.to_le_bytes().to_vec();
        e.extend_from_slice(&[1, 16]);
        e.extend_from_slice(&(text.len() as u16 / 2).to_le_bytes());
        e.extend(text);
        e.extend_from_slice(&[0, 0, 3, 0, 0, 0]);
        msg.push(0xAA);
        msg.extend_from_slice(&(e.len() as u16).to_le_bytes());
        msg.extend(e);
        msg.extend_from_slice(&[0xFD, 0x02, 0, 0xC1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        // In two packets.
        let (a, b) = msg.split_at(40);
        let mut first = reply(a);
        first[1] = 0;
        assert_eq!(m.receive(&first), []);
        let got = m.receive(&reply(b));
        let [Action::Done(outcomes)] = &got[..] else { panic!("{got:?}") };
        let Outcome::Rows(rows) = &outcomes[0] else { panic!("{outcomes:?}") };
        assert_eq!(rows.columns.iter().map(|c| c.type_name.as_str()).collect::<Vec<_>>(), ["int", "nvarchar", "decimal", "date", "datetime2", "bit", "uniqueidentifier", "varbinary"]);
        let cell = |i: usize| rows.rows[0][i].clone().unwrap_or_default();
        assert_eq!((0..8).map(cell).collect::<Vec<_>>(), ["7", "Zoë", "12.34", "2024-02-29", "2024-02-29 13:45:30.1234567", "1", "00112233-4455-6677-8899-AABBCCDDEEFF", "\\x0102"]);
        assert_eq!(rows.rows[1], vec![None; 8]);
        assert_eq!(outcomes[1], Outcome::Done { tag: String::new(), changed: Some(3) });
        assert_eq!(outcomes[2], Outcome::Error("Msg 208, level 16, line 3: Invalid object name 'nope'.".into()));
    }

    #[test]
    fn dates_and_numbers() {
        assert_eq!(date_from_days(0), (1, 1, 1));
        assert_eq!(date_from_days(693_595), (1900, 1, 1));
        assert_eq!(scaled(5, 2, true), "-0.05");
        assert_eq!(scaled(0, 0, true), "0");
        let dt = Type { code: 0x3D, len_size: 0, plp: false, fixed: 8, scale: 0, size: 0 };
        let mut v = 45_000i32.to_le_bytes().to_vec(); // 2023-03-17
        v.extend_from_slice(&(300u32 * 3600 + 150).to_le_bytes());
        assert_eq!(decode(&v, &dt), "2023-03-17 01:00:00.500");
        let money = Type { code: 0x3C, len_size: 0, plp: false, fixed: 8, scale: 0, size: 0 };
        let n: i64 = -12_345_678;
        let mut v = ((n >> 32) as i32).to_le_bytes().to_vec();
        v.extend_from_slice(&(n as u32).to_le_bytes());
        assert_eq!(decode(&v, &money), "-1234.5678");
    }

    #[test]
    fn byte_length_types_are_named_by_their_size() {
        // As SQL Server 2022 sends bigint, smallint, tinyint, real,
        // smallmoney and smalldatetime columns: intn, floatn, moneyn and
        // datetimen with their size.
        let named = |info: &[u8]| {
            let mut r = Reader { b: info, at: 0 };
            type_name(&type_info(&mut r).unwrap()).0
        };
        assert_eq!([named(&[0x26, 1]), named(&[0x26, 2]), named(&[0x26, 4]), named(&[0x26, 8])], ["tinyint", "smallint", "int", "bigint"]);
        assert_eq!([named(&[0x6D, 4]), named(&[0x6D, 8])], ["real", "float"]);
        assert_eq!([named(&[0x6E, 4]), named(&[0x6E, 8]), named(&[0x7A])], ["smallmoney", "money", "smallmoney"]);
        assert_eq!([named(&[0x6F, 4]), named(&[0x6F, 8]), named(&[0x3A])], ["smalldatetime", "datetime", "smalldatetime"]);
    }
}
