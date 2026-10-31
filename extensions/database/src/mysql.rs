//! MySQL's and MariaDB's client/server protocol: the server's handshake,
//! TLS asked for with an SSLRequest, caching_sha2_password (fast, or in
//! full -- over TLS, or to the server's RSA key) and
//! mysql_native_password, auth switches, then COM_QUERY's text results,
//! several statements at once.

use rsa::pkcs8::DecodePublicKey;
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::conn::Tls;
use crate::result::{hex, Column, Kind, Outcome, Rows};
use crate::wire::{Action, Client, Login};

const LONG_PASSWORD: u32 = 0x1;
const LONG_FLAG: u32 = 0x4;
const CONNECT_WITH_DB: u32 = 0x8;
const PROTOCOL_41: u32 = 0x200;
const SSL: u32 = 0x800;
const TRANSACTIONS: u32 = 0x2000;
const SECURE_CONNECTION: u32 = 0x8000;
const MULTI_STATEMENTS: u32 = 0x10000;
const MULTI_RESULTS: u32 = 0x20000;
const PLUGIN_AUTH: u32 = 0x80000;
const PLUGIN_AUTH_LENENC_DATA: u32 = 0x200000;

/// A status flag: another result follows this one.
const MORE_RESULTS: u16 = 0x8;
/// utf8mb4_general_ci.
const UTF8MB4: u8 = 45;
/// The binary character set: a column of bytes.
const BINARY: u16 = 63;

#[derive(Debug, Clone, PartialEq)]
enum State {
    Greeting,
    Handshake,
    /// Signing in with this plugin and the server's nonce.
    Auth {
        plugin: String,
        nonce: Vec<u8>,
    },
    /// caching_sha2_password asked for the server's key.
    PublicKey {
        nonce: Vec<u8>,
    },
    /// `SET SESSION TRANSACTION READ ONLY` sent.
    Setup,
    Idle,
    /// A query's results: columns still to come, then rows.
    Results(Phase),
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    /// The first packet of a result: OK, ERR, or a column count.
    Start,
    Columns(usize),
    /// The EOF after the columns.
    ColumnsEnd,
    Rows,
}

pub struct Mysql {
    login: Login,
    tls: Tls,
    state: State,
    buf: Vec<u8>,
    /// A packet of 16 MiB or more, being put together.
    partial: Vec<u8>,
    seq: u8,
    caps: u32,
    tls_on: bool,
    /// The plugin and nonce to answer once TLS is up.
    pending: Option<(String, Vec<u8>)>,
    outcomes: Vec<Outcome>,
    rows: Rows,
    pub server_version: String,
}

impl Mysql {
    pub fn new(login: Login, tls: Tls) -> Mysql {
        Mysql {
            login,
            tls,
            state: State::Greeting,
            buf: Vec::new(),
            partial: Vec::new(),
            seq: 0,
            caps: 0,
            tls_on: false,
            pending: None,
            outcomes: Vec::new(),
            rows: Rows::default(),
            server_version: String::new(),
        }
    }

    /// A packet of ours, the next in the sequence.
    fn packet(&mut self, payload: &[u8]) -> Vec<u8> {
        self.seq = self.seq.wrapping_add(1);
        frame(payload, self.seq)
    }

    fn handle(&mut self, p: Vec<u8>, out: &mut Vec<Action>) {
        match self.state.clone() {
            State::Greeting => self.greeting(&p, out),
            State::Auth { plugin, nonce } => match p.first() {
                Some(0x00) => self.signed_in(out),
                Some(0xFF) => self.fail(out, error_text(&p)),
                Some(0xFE) => {
                    // Another plugin, and its nonce.
                    let rest = &p[1..];
                    let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                    let plugin = String::from_utf8_lossy(&rest[..end]).into_owned();
                    let mut nonce = rest.get(end + 1..).unwrap_or(&[]).to_vec();
                    if nonce.last() == Some(&0) {
                        nonce.pop();
                    }
                    match scramble(&plugin, &self.login.password, &nonce) {
                        Some(answer) => {
                            let pkt = self.packet(&answer);
                            out.push(Action::Send(pkt));
                            self.state = State::Auth { plugin, nonce };
                        }
                        None => self.fail(out, format!("the server wants {plugin} to sign in, which isn't spoken here")),
                    }
                }
                Some(0x01) if plugin == "caching_sha2_password" => match p.get(1) {
                    Some(3) => {} // fast: an OK follows
                    Some(4) => {
                        if self.tls_on {
                            let mut pw = self.login.password.as_bytes().to_vec();
                            pw.push(0);
                            let pkt = self.packet(&pw);
                            out.push(Action::Send(pkt));
                        } else {
                            let pkt = self.packet(&[2]);
                            out.push(Action::Send(pkt));
                            self.state = State::PublicKey { nonce };
                        }
                    }
                    _ => {}
                },
                _ => {}
            },
            State::PublicKey { nonce } => match p.first() {
                Some(0x01) => match rsa_password(&String::from_utf8_lossy(&p[1..]), &self.login.password, &nonce) {
                    Ok(enc) => {
                        let pkt = self.packet(&enc);
                        out.push(Action::Send(pkt));
                        self.state = State::Auth { plugin: "caching_sha2_password".into(), nonce };
                    }
                    Err(e) => self.fail(out, e),
                },
                Some(0xFF) => self.fail(out, error_text(&p)),
                _ => {}
            },
            State::Setup => {
                self.state = State::Idle;
                out.push(Action::Ready);
            }
            State::Results(phase) => self.result(phase, &p, out),
            State::Idle | State::Handshake | State::Closed => {}
        }
    }

    fn fail(&mut self, out: &mut Vec<Action>, why: String) {
        self.state = State::Closed;
        out.push(Action::Failed(why));
    }

    fn greeting(&mut self, p: &[u8], out: &mut Vec<Action>) {
        if p.first() == Some(&0xFF) {
            return self.fail(out, error_text(p));
        }
        let Some(g) = Greeting::parse(p) else { return self.fail(out, "the server's greeting isn't MySQL's (protocol 10)".into()) };
        self.server_version = g.version.clone();
        let mut caps = LONG_PASSWORD | LONG_FLAG | PROTOCOL_41 | TRANSACTIONS | SECURE_CONNECTION | MULTI_STATEMENTS | MULTI_RESULTS | PLUGIN_AUTH | PLUGIN_AUTH_LENENC_DATA;
        if !self.login.database.is_empty() {
            caps |= CONNECT_WITH_DB;
        }
        caps &= g.caps | PROTOCOL_41;
        let tls = self.tls != Tls::Off && g.caps & SSL != 0;
        if self.tls == Tls::Require && !tls {
            return self.fail(out, "the server doesn't do TLS, and this connection needs it (tls=require)".into());
        }
        if tls {
            caps |= SSL;
        }
        self.caps = caps;
        self.seq = 0;
        self.state = State::Auth { plugin: g.plugin.clone(), nonce: g.nonce.clone() };
        if tls {
            // The handshake's first 32 bytes alone: then TLS.
            let short = self.response_head();
            let pkt = self.packet(&short);
            out.push(Action::Send(pkt));
            out.push(Action::StartTls);
            self.state = State::Handshake;
            self.pending = Some((g.plugin, g.nonce));
        } else {
            let pkt = self.handshake_response(&g.plugin, &g.nonce);
            out.push(Action::Send(pkt));
        }
    }

    /// Capabilities, the largest packet, the character set, 23 zeros.
    fn response_head(&self) -> Vec<u8> {
        let mut b = self.caps.to_le_bytes().to_vec();
        b.extend_from_slice(&(16u32 << 20).to_le_bytes());
        b.push(UTF8MB4);
        b.extend_from_slice(&[0; 23]);
        b
    }

    fn handshake_response(&mut self, plugin: &str, nonce: &[u8]) -> Vec<u8> {
        let mut b = self.response_head();
        b.extend_from_slice(self.login.user.as_bytes());
        b.push(0);
        // A plugin not spoken here: nothing, and the server may switch to
        // one that is.
        let auth = scramble(plugin, &self.login.password, nonce).unwrap_or_default();
        lenenc_int(&mut b, auth.len() as u64);
        b.extend_from_slice(&auth);
        if self.caps & CONNECT_WITH_DB != 0 {
            b.extend_from_slice(self.login.database.as_bytes());
            b.push(0);
        }
        b.extend_from_slice(plugin.as_bytes());
        b.push(0);
        self.packet(&b)
    }

    fn signed_in(&mut self, out: &mut Vec<Action>) {
        if self.login.read_only {
            self.seq = 0;
            self.state = State::Setup;
            out.push(Action::Send(frame(b"\x03SET SESSION TRANSACTION READ ONLY", 0)));
        } else {
            self.state = State::Idle;
            out.push(Action::Ready);
        }
    }

    fn result(&mut self, phase: Phase, p: &[u8], out: &mut Vec<Action>) {
        let first = p.first().copied();
        let eof = first == Some(0xFE) && p.len() < 9;
        match phase {
            Phase::Start => match first {
                Some(0x00) => {
                    let mut at = 1;
                    let changed = read_lenenc(p, &mut at);
                    let _insert_id = read_lenenc(p, &mut at);
                    let status = p.get(at..at + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]));
                    // The rest, or (MariaDB's) its length and then it.
                    let rest = p.get(at + 4..).unwrap_or(&[]);
                    let rest = match rest.split_first() {
                        Some((&n, text)) if n as usize == text.len() => text,
                        _ => rest,
                    };
                    let info = String::from_utf8_lossy(rest).into_owned();
                    self.outcomes.push(Outcome::Done { tag: info, changed });
                    self.next_or_done(status, out);
                }
                Some(0xFF) => {
                    self.outcomes.push(Outcome::Error(error_text(p)));
                    self.done(out);
                }
                Some(0xFB) => {
                    // LOCAL INFILE: no file is sent.
                    let pkt = self.packet(&[]);
                    out.push(Action::Send(pkt));
                }
                _ => {
                    let mut at = 0;
                    let n = read_lenenc(p, &mut at).unwrap_or(0) as usize;
                    self.rows = Rows::default();
                    self.state = State::Results(if n == 0 { Phase::ColumnsEnd } else { Phase::Columns(n) });
                }
            },
            Phase::Columns(n) => {
                self.rows.columns.push(column_definition(p));
                self.state = State::Results(if n <= 1 { Phase::ColumnsEnd } else { Phase::Columns(n - 1) });
            }
            Phase::ColumnsEnd => {
                if eof {
                    self.state = State::Results(Phase::Rows);
                } else {
                    // A server that sends no EOF after the columns.
                    self.state = State::Results(Phase::Rows);
                    self.result(Phase::Rows, p, out);
                }
            }
            Phase::Rows => {
                if eof {
                    let status = p.get(3..5).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]));
                    self.outcomes.push(Outcome::Rows(std::mem::take(&mut self.rows)));
                    self.next_or_done(status, out);
                } else if first == Some(0xFF) {
                    self.outcomes.push(Outcome::Error(error_text(p)));
                    self.done(out);
                } else if self.rows.rows.len() >= self.login.max_rows.max(1) {
                    self.rows.truncated = true;
                } else {
                    let row = text_row(p, &self.rows.columns);
                    self.rows.rows.push(row);
                }
            }
        }
    }

    fn next_or_done(&mut self, status: u16, out: &mut Vec<Action>) {
        if status & MORE_RESULTS != 0 {
            self.state = State::Results(Phase::Start);
        } else {
            self.done(out);
        }
    }

    fn done(&mut self, out: &mut Vec<Action>) {
        self.state = State::Idle;
        out.push(Action::Done(std::mem::take(&mut self.outcomes)));
    }
}

/// What the server's greeting says.
#[derive(Debug, Clone, PartialEq)]
struct Greeting {
    version: String,
    caps: u32,
    nonce: Vec<u8>,
    plugin: String,
}

impl Greeting {
    fn parse(p: &[u8]) -> Option<Greeting> {
        if p.first() != Some(&10) {
            return None;
        }
        let mut at = 1;
        let end = at + p[at..].iter().position(|&b| b == 0)?;
        let version = String::from_utf8_lossy(&p[at..end]).into_owned();
        at = end + 1 + 4; // the connection's id
        let mut nonce = p.get(at..at + 8)?.to_vec();
        at += 9;
        let low = u16::from_le_bytes([*p.get(at)?, *p.get(at + 1)?]) as u32;
        at += 2 + 1 + 2; // the character set, the status
        let high = p.get(at..at + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]) as u32);
        let caps = low | high << 16;
        at += 2;
        let auth_len = p.get(at).copied().unwrap_or(0) as usize;
        at += 1 + 10;
        if caps & SECURE_CONNECTION != 0 {
            let n = auth_len.saturating_sub(8).max(13);
            let part = p.get(at..(at + n).min(p.len()))?;
            // Its last byte is a NUL.
            nonce.extend_from_slice(part.strip_suffix(&[0]).unwrap_or(part));
            at += n;
        }
        let plugin = p.get(at..).map(|r| String::from_utf8_lossy(r.split(|&b| b == 0).next().unwrap_or(r)).into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| "mysql_native_password".into());
        Some(Greeting { version, caps, nonce, plugin })
    }
}

impl Mysql {
    fn after_tls(&mut self) -> Vec<Action> {
        let Some((plugin, nonce)) = self.pending.take() else { return Vec::new() };
        self.tls_on = true;
        self.state = State::Auth { plugin: plugin.clone(), nonce: nonce.clone() };
        let pkt = self.handshake_response(&plugin, &nonce);
        vec![Action::Send(pkt)]
    }
}

impl Client for Mysql {
    fn start(&mut self) -> Vec<Action> {
        // The server speaks first.
        Vec::new()
    }

    fn tls_ready(&mut self) -> Vec<Action> {
        self.after_tls()
    }

    fn receive(&mut self, data: &[u8]) -> Vec<Action> {
        let mut out = Vec::new();
        self.buf.extend_from_slice(data);
        let mut at = 0;
        while self.buf.len() - at >= 4 {
            let len = u32::from_le_bytes([self.buf[at], self.buf[at + 1], self.buf[at + 2], 0]) as usize;
            if self.buf.len() - at < 4 + len {
                break;
            }
            self.seq = self.buf[at + 3];
            let payload = self.buf[at + 4..at + 4 + len].to_vec();
            at += 4 + len;
            if len == 0xFF_FFFF {
                self.partial.extend(payload);
                continue;
            }
            let whole = if self.partial.is_empty() {
                payload
            } else {
                let mut p = std::mem::take(&mut self.partial);
                p.extend(payload);
                p
            };
            self.handle(whole, &mut out);
            if self.state == State::Closed {
                break;
            }
        }
        self.buf.drain(..at);
        out
    }

    fn query(&mut self, sql: &str) -> Vec<Action> {
        self.state = State::Results(Phase::Start);
        self.outcomes.clear();
        self.seq = 0;
        let mut p = vec![0x03];
        p.extend_from_slice(sql.as_bytes());
        vec![Action::Send(frames(&p, 0))]
    }

    fn goodbye(&mut self) -> Vec<u8> {
        self.state = State::Closed;
        frame(&[0x01], 0)
    }
}

/// `payload` in one packet, sequence `seq`.
fn frame(payload: &[u8], seq: u8) -> Vec<u8> {
    let mut b = (payload.len() as u32).to_le_bytes()[..3].to_vec();
    b.push(seq);
    b.extend_from_slice(payload);
    b
}

/// `payload` in as many packets as it takes (16 MiB less a byte each).
fn frames(payload: &[u8], seq: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let mut seq = seq;
    let mut chunks = payload.chunks(0xFF_FFFF).peekable();
    if payload.is_empty() {
        return frame(&[], seq);
    }
    while let Some(c) = chunks.next() {
        out.extend(frame(c, seq));
        seq = seq.wrapping_add(1);
        if chunks.peek().is_none() && c.len() == 0xFF_FFFF {
            out.extend(frame(&[], seq));
        }
    }
    out
}

fn lenenc_int(b: &mut Vec<u8>, n: u64) {
    match n {
        0..=250 => b.push(n as u8),
        251..=0xFFFF => {
            b.push(0xFC);
            b.extend_from_slice(&(n as u16).to_le_bytes());
        }
        0x10000..=0xFF_FFFF => {
            b.push(0xFD);
            b.extend_from_slice(&(n as u32).to_le_bytes()[..3]);
        }
        _ => {
            b.push(0xFE);
            b.extend_from_slice(&n.to_le_bytes());
        }
    }
}

/// A length-encoded integer at `at`; none for NULL (0xFB) or past the end.
fn read_lenenc(p: &[u8], at: &mut usize) -> Option<u64> {
    let first = *p.get(*at)?;
    *at += 1;
    let take = |at: &mut usize, n: usize| -> Option<u64> {
        let s = p.get(*at..*at + n)?;
        *at += n;
        let mut x = [0u8; 8];
        x[..n].copy_from_slice(s);
        Some(u64::from_le_bytes(x))
    };
    match first {
        0xFB => None,
        0xFC => take(at, 2),
        0xFD => take(at, 3),
        0xFE => take(at, 8),
        n => Some(n as u64),
    }
}

/// A length-encoded string at `at`; none for NULL.
fn read_lenenc_bytes<'a>(p: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let n = read_lenenc(p, at)? as usize;
    let s = p.get(*at..(*at + n).min(p.len()))?;
    *at += n;
    Some(s)
}

/// A type's name and kind.
fn type_of(t: u8, charset: u16) -> (&'static str, Kind) {
    match t {
        0 | 246 => ("decimal", Kind::Number),
        1 => ("tinyint", Kind::Number),
        2 => ("smallint", Kind::Number),
        3 => ("int", Kind::Number),
        4 => ("float", Kind::Number),
        5 => ("double", Kind::Number),
        6 => ("null", Kind::Text),
        7 => ("timestamp", Kind::Text),
        8 => ("bigint", Kind::Number),
        9 => ("mediumint", Kind::Number),
        10 => ("date", Kind::Text),
        11 => ("time", Kind::Text),
        12 => ("datetime", Kind::Text),
        13 => ("year", Kind::Number),
        16 => ("bit", Kind::Bytes),
        245 => ("json", Kind::Text),
        247 => ("enum", Kind::Text),
        248 => ("set", Kind::Text),
        249..=252 if charset == BINARY => ("blob", Kind::Bytes),
        249..=252 => ("text", Kind::Text),
        15 | 253 if charset == BINARY => ("varbinary", Kind::Bytes),
        254 if charset == BINARY => ("binary", Kind::Bytes),
        15 | 253 => ("varchar", Kind::Text),
        254 => ("char", Kind::Text),
        255 => ("geometry", Kind::Bytes),
        _ => ("", Kind::Text),
    }
}

fn column_definition(p: &[u8]) -> Column {
    let mut at = 0;
    for _ in 0..4 {
        read_lenenc_bytes(p, &mut at); // catalog, schema, table, org_table
    }
    let name = read_lenenc_bytes(p, &mut at).map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_default();
    read_lenenc_bytes(p, &mut at); // org_name
    read_lenenc(p, &mut at); // the fixed fields' length, 0x0c
    let charset = p.get(at..at + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]));
    let t = p.get(at + 6).copied().unwrap_or(253);
    let (type_name, kind) = type_of(t, charset);
    Column::new(name, type_name, kind)
}

fn text_row(p: &[u8], columns: &[Column]) -> Vec<Option<String>> {
    let mut at = 0;
    (0..columns.len())
        .map(|i| {
            if p.get(at) == Some(&0xFB) {
                at += 1;
                return None;
            }
            let v = read_lenenc_bytes(p, &mut at)?;
            Some(if columns[i].kind == Kind::Bytes { hex(v) } else { String::from_utf8_lossy(v).into_owned() })
        })
        .collect()
}

/// An ERR packet as a line: `ERROR 1146 (42S02): Table 'x.y' doesn't exist`.
fn error_text(p: &[u8]) -> String {
    let code = p.get(1..3).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]));
    let rest = p.get(3..).unwrap_or(&[]);
    match rest.strip_prefix(b"#") {
        Some(r) if r.len() >= 5 => format!("ERROR {code} ({}): {}", String::from_utf8_lossy(&r[..5]), String::from_utf8_lossy(&r[5..])),
        _ => format!("ERROR {code}: {}", String::from_utf8_lossy(rest)),
    }
}

/// A plugin's answer to the server's nonce, for the plugins spoken here.
pub fn scramble(plugin: &str, password: &str, nonce: &[u8]) -> Option<Vec<u8>> {
    if password.is_empty() {
        return Some(Vec::new());
    }
    match plugin {
        // SHA1(password) XOR SHA1(nonce + SHA1(SHA1(password))).
        "mysql_native_password" => {
            let a = Sha1::digest(password.as_bytes());
            let b = Sha1::digest(a);
            let mut h = Sha1::new();
            h.update(&nonce[..nonce.len().min(20)]);
            h.update(b);
            Some(a.iter().zip(h.finalize()).map(|(x, y)| x ^ y).collect())
        }
        // SHA256(password) XOR SHA256(SHA256(SHA256(password)) + nonce).
        "caching_sha2_password" => {
            let a = Sha256::digest(password.as_bytes());
            let b = Sha256::digest(a);
            let mut h = Sha256::new();
            h.update(b);
            h.update(&nonce[..nonce.len().min(20)]);
            Some(a.iter().zip(h.finalize()).map(|(x, y)| x ^ y).collect())
        }
        // Over TLS only, as the server asks: the password itself.
        "mysql_clear_password" => {
            let mut p = password.as_bytes().to_vec();
            p.push(0);
            Some(p)
        }
        _ => None,
    }
}

/// The password, NUL-ended and XORed with the nonce, encrypted to the
/// server's key (RSA-OAEP with SHA-1), as caching_sha2_password's full
/// authentication without TLS has it.
fn rsa_password(pem: &str, password: &str, nonce: &[u8]) -> Result<Vec<u8>, String> {
    let key = rsa::RsaPublicKey::from_public_key_pem(pem.trim_end_matches('\0').trim()).map_err(|e| format!("the server's RSA key: {e}"))?;
    let mut pw = password.as_bytes().to_vec();
    pw.push(0);
    if !nonce.is_empty() {
        for (i, b) in pw.iter_mut().enumerate() {
            *b ^= nonce[i % nonce.len()];
        }
    }
    key.encrypt(&mut Rng, rsa::Oaep::new::<Sha1>(), &pw).map_err(|e| format!("encrypting the password: {e}"))
}

/// The system's randomness, for RSA's padding.
struct Rng;

impl rand_core::RngCore for Rng {
    fn next_u32(&mut self) -> u32 {
        u32::from_le_bytes(crate::wire::random(4).try_into().unwrap_or_default())
    }

    fn next_u64(&mut self) -> u64 {
        u64::from_le_bytes(crate::wire::random(8).try_into().unwrap_or_default())
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        dest.copy_from_slice(&crate::wire::random(dest.len()));
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl rand_core::CryptoRng for Rng {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A MySQL 8 server's greeting: protocol 10, its version, a 20-byte
    /// nonce in two parts, caching_sha2_password.
    fn greeting(caps: u32) -> Vec<u8> {
        let mut g = vec![10];
        g.extend_from_slice(b"8.0.36\0");
        g.extend_from_slice(&7u32.to_le_bytes());
        g.extend_from_slice(b"abcdefgh");
        g.push(0);
        g.extend_from_slice(&(caps as u16).to_le_bytes());
        g.push(255);
        g.extend_from_slice(&2u16.to_le_bytes());
        g.extend_from_slice(&((caps >> 16) as u16).to_le_bytes());
        g.push(21);
        g.extend_from_slice(&[0; 10]);
        g.extend_from_slice(b"ijklmnopqrst\0");
        g.extend_from_slice(b"caching_sha2_password\0");
        frame(&g, 0)
    }

    const SERVER: u32 = !0x0100_0000;

    fn login() -> Login {
        Login { user: "ada".into(), password: "pw".into(), database: "shop".into(), max_rows: 100, read_only: false }
    }

    #[test]
    fn the_scrambles() {
        // As Python's hashlib makes them.
        let nonce = b"abcdefghijklmnopqrst";
        assert_eq!(hex(&scramble("mysql_native_password", "pw", nonce).unwrap()), "\\x12909d72d875523f8c9d6da44cbd3a6c1ce794db");
        assert_eq!(hex(&scramble("caching_sha2_password", "pw", nonce).unwrap()), "\\xad439090e1914151d1a7f6bf7c21561d283f2fba52fe39ce9dcd29d687813c2c");
        assert_eq!(scramble("caching_sha2_password", "", nonce), Some(Vec::new()), "no password: nothing");
        assert_eq!(scramble("client_ed25519", "pw", nonce), None);
    }

    #[test]
    fn the_greeting_is_answered_with_a_handshake_response() {
        let mut m = Mysql::new(login(), Tls::Off);
        assert_eq!(m.start(), []);
        let got = m.receive(&greeting(SERVER));
        let [Action::Send(p)] = &got[..] else { panic!("{got:?}") };
        assert_eq!(m.server_version, "8.0.36");
        assert_eq!(p[3], 1, "sequence 1");
        let caps = u32::from_le_bytes(p[4..8].try_into().unwrap());
        assert_eq!(caps & SSL, 0);
        assert_ne!(caps & CONNECT_WITH_DB, 0);
        assert_eq!(p[12], UTF8MB4);
        let rest = &p[4 + 32..];
        assert!(rest.starts_with(b"ada\0\x20"), "the user, then a 32-byte answer");
        assert_eq!(&rest[5..37], scramble("caching_sha2_password", "pw", b"abcdefghijklmnopqrst").unwrap().as_slice());
        assert_eq!(&rest[37..], b"shop\0caching_sha2_password\0");
        // Fast authentication, then OK.
        assert_eq!(m.receive(&frame(&[1, 3], 2)), []);
        assert_eq!(m.receive(&frame(&[0, 0, 0, 2, 0, 0, 0], 3)), [Action::Ready]);
    }

    #[test]
    fn tls_first_then_the_response() {
        let mut m = Mysql::new(login(), Tls::Prefer);
        let got = m.receive(&greeting(SERVER));
        let [Action::Send(short), Action::StartTls] = &got[..] else { panic!("{got:?}") };
        assert_eq!(short.len(), 4 + 32);
        assert_ne!(u32::from_le_bytes(short[4..8].try_into().unwrap()) & SSL, 0);
        let [Action::Send(full)] = &m.tls_ready()[..] else { panic!() };
        assert_eq!(full[3], 2, "sequence 2, after the SSLRequest");
        // Full authentication over TLS: the password itself.
        assert_eq!(m.receive(&frame(&[1, 4], 3)), [Action::Send(frame(b"pw\0", 4))]);
        let mut m = Mysql::new(login(), Tls::Require);
        assert!(matches!(&m.receive(&greeting(SERVER & !SSL))[..], [Action::Failed(e)] if e.contains("tls=require")));
    }

    #[test]
    fn full_authentication_without_tls_asks_for_the_key() {
        let mut m = Mysql::new(login(), Tls::Off);
        m.receive(&greeting(SERVER));
        assert_eq!(m.receive(&frame(&[1, 4], 2)), [Action::Send(frame(&[2], 3))]);
        // An auth switch to mysql_native_password: answered with its own
        // scramble.
        let mut m = Mysql::new(login(), Tls::Off);
        m.receive(&greeting(SERVER));
        let mut switch = vec![0xFE];
        switch.extend_from_slice(b"mysql_native_password\0abcdefghijklmnopqrst\0");
        let expected = frame(&scramble("mysql_native_password", "pw", b"abcdefghijklmnopqrst").unwrap(), 3);
        assert_eq!(m.receive(&frame(&switch, 2)), [Action::Send(expected)]);
        let mut err = vec![0xFF, 0x15, 0x04, b'#'];
        err.extend_from_slice(b"28000Access denied for user 'ada'");
        assert_eq!(m.receive(&frame(&err, 4)), [Action::Failed("ERROR 1045 (28000): Access denied for user 'ada'".into())]);
    }

    fn coldef(name: &str, t: u8, charset: u16) -> Vec<u8> {
        let mut c = Vec::new();
        for s in ["def", "shop", "t", "t", name, name] {
            lenenc_int(&mut c, s.len() as u64);
            c.extend_from_slice(s.as_bytes());
        }
        c.push(0x0c);
        c.extend_from_slice(&charset.to_le_bytes());
        c.extend_from_slice(&11u32.to_le_bytes());
        c.push(t);
        c.extend_from_slice(&[0, 0, 0, 0, 0]);
        c
    }

    #[test]
    fn a_query_brings_rows_then_an_ok() {
        let mut m = Mysql::new(login(), Tls::Off);
        m.receive(&greeting(SERVER));
        m.receive(&frame(&[0, 0, 0, 2, 0, 0, 0], 2));
        assert_eq!(m.query("select id, name, b from t; update t set x = 1"), [Action::Send(frame(b"\x03select id, name, b from t; update t set x = 1", 0))]);
        let mut s = frame(&[3], 1);
        s.extend(frame(&coldef("id", 3, 63), 2));
        s.extend(frame(&coldef("name", 253, 45), 3));
        s.extend(frame(&coldef("b", 252, 63), 4));
        s.extend(frame(&[0xFE, 0, 0, 0x22, 0], 5));
        s.extend(frame(b"\x011\x03ann\x02\x00\xff", 6));
        s.extend(frame(b"\x012\xfb\xfb", 7));
        // The last EOF says another result follows: the UPDATE's OK.
        s.extend(frame(&[0xFE, 0, 0, 0x0a, 0], 8));
        // MariaDB's: the info's length first.
        let mut ok = vec![0, 3, 0, 0x02, 0, 0, 0, 40];
        ok.extend_from_slice(b"Rows matched: 3  Changed: 3  Warnings: 0");
        s.extend(frame(&ok, 1));
        let got = m.receive(&s);
        let [Action::Done(outcomes)] = &got[..] else { panic!("{got:?}") };
        let Outcome::Rows(rows) = &outcomes[0] else { panic!() };
        assert_eq!(rows.columns.iter().map(|c| (c.name.as_str(), c.kind)).collect::<Vec<_>>(), [("id", Kind::Number), ("name", Kind::Text), ("b", Kind::Bytes)]);
        assert_eq!(rows.rows, [vec![Some("1".into()), Some("ann".into()), Some("\\x00ff".into())], vec![Some("2".into()), None, None]]);
        assert_eq!(outcomes[1], Outcome::Done { tag: "Rows matched: 3  Changed: 3  Warnings: 0".into(), changed: Some(3) });
        // An error ends the statements.
        m.query("select nope");
        let mut err = vec![0xFF, 0x7A, 0x04, b'#'];
        err.extend_from_slice(b"42S02Table 'shop.nope' doesn't exist");
        assert_eq!(m.receive(&frame(&err, 1)), [Action::Done(vec![Outcome::Error("ERROR 1146 (42S02): Table 'shop.nope' doesn't exist".into())])]);
        assert_eq!(m.goodbye(), [1, 0, 0, 0, 1]);
    }

    #[test]
    fn read_only_sets_the_session_first() {
        let mut m = Mysql::new(Login { read_only: true, ..login() }, Tls::Off);
        m.receive(&greeting(SERVER));
        assert_eq!(m.receive(&frame(&[0, 0, 0, 2, 0, 0, 0], 2)), [Action::Send(frame(b"\x03SET SESSION TRANSACTION READ ONLY", 0))]);
        assert_eq!(m.receive(&frame(&[0, 0, 0, 2, 0, 0, 0], 1)), [Action::Ready]);
    }

    #[test]
    fn length_encoded_integers() {
        for n in [0u64, 250, 251, 65535, 65536, 0xFF_FFFF, 0x100_0000, u64::MAX] {
            let mut b = Vec::new();
            lenenc_int(&mut b, n);
            let mut at = 0;
            assert_eq!(read_lenenc(&b, &mut at), Some(n));
            assert_eq!(at, b.len());
        }
        let big = vec![b'x'; 0xFF_FFFF];
        let f = frames(&big, 0);
        assert_eq!(f.len(), 4 + 0xFF_FFFF + 4, "a packet of exactly 16 MiB less a byte, then an empty one");
    }

    /// What MariaDB 10.11 sent, captured: its greeting and OK, then
    /// `select * from users order by id; select 1/0; select 2`, and the
    /// catalog's four queries.
    #[test]
    fn what_a_real_server_sent() {
        let mut m = Mysql::new(Login { password: "adapw".into(), ..login() }, Tls::Off);
        let got = m.receive(include_bytes!("testdata/mariadb-sign-in.bin"));
        assert!(matches!(&got[..], [Action::Send(_), Action::Ready]), "{got:?}");
        assert!(m.server_version.contains("MariaDB"));
        m.query("select * from users order by id; select 1/0; select 2");
        let got = m.receive(include_bytes!("testdata/mariadb-query.bin"));
        let [Action::Done(outcomes)] = &got[..] else { panic!("{got:?}") };
        assert_eq!(outcomes.len(), 3);
        let Outcome::Rows(users) = &outcomes[0] else { panic!() };
        assert_eq!(users.columns.iter().map(|c| c.type_name.as_str()).collect::<Vec<_>>(), ["int", "varchar", "varchar", "decimal", "tinyint", "blob"]);
        assert_eq!(users.rows[1], [Some("2".into()), Some("grace".into()), None, Some("2.25".into()), Some("0".into()), None]);
        assert_eq!(users.rows[0][5].as_deref(), Some("\\x0102"));
        assert!(matches!(&outcomes[1], Outcome::Rows(r) if r.rows == [vec![None]]), "1/0 is NULL, with a warning");
        m.query(&crate::schema::catalog(crate::conn::Engine::Mysql, "shop").join(";"));
        let [Action::Done(outcomes)] = &m.receive(include_bytes!("testdata/mariadb-catalog.bin"))[..] else { panic!() };
        let schema = crate::schema::from_outcomes(crate::conn::Engine::Mysql, outcomes).unwrap();
        let orders = schema.table("shop", "orders").unwrap();
        assert_eq!(orders.primary_key(), ["id", "line"]);
        assert_eq!(orders.keys.iter().find(|k| k.kind == crate::schema::KeyKind::Foreign).and_then(|k| k.refers.clone()), Some(("shop".into(), "users".into(), vec!["id".into()])));
        assert!(schema.table("shop", "names").unwrap().view);
    }
}
