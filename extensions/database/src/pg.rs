//! PostgreSQL's wire protocol (v3), as a client: TLS asked for with an
//! SSLRequest, the startup message, SCRAM-SHA-256, md5 or a cleartext
//! password, then queries -- simple ones (several statements at once)
//! and extended ones (Parse, Bind, Execute: one statement, which the
//! server holds to).

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::conn::Tls;
use crate::result::{hex, Column, Kind, Outcome, Rows};
use crate::wire::{random, Action, Client, Login};

/// SSLRequest's code.
const SSL_REQUEST: i32 = 80877103;
/// Protocol 3.0.
const PROTOCOL: i32 = 196608;

#[derive(Debug, Clone, PartialEq)]
enum State {
    /// Asked for TLS: one byte comes back, `S` or `N`.
    SslAnswer,
    /// TLS being set up.
    Handshake,
    /// Startup sent: authentication.
    Auth,
    /// SCRAM's first message sent: what the client said, its nonce.
    Scram {
        first_bare: String,
        nonce: String,
    },
    /// SCRAM's last message sent: the server's signature, to check.
    ScramFinal {
        server_signature: Vec<u8>,
    },
    /// Signed in; settings come until it's ready.
    Starting,
    Idle,
    /// A query's results coming.
    Busy,
    Closed,
}

pub struct Postgres {
    login: Login,
    tls: Tls,
    state: State,
    buf: Vec<u8>,
    outcomes: Vec<Outcome>,
    rows: Option<Rows>,
    /// What the server said it is (`16.4`).
    pub server_version: String,
}

impl Postgres {
    pub fn new(login: Login, tls: Tls) -> Postgres {
        Postgres { login, tls, state: State::Auth, buf: Vec::new(), outcomes: Vec::new(), rows: None, server_version: String::new() }
    }

    fn startup(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&PROTOCOL.to_be_bytes());
        let mut param = |k: &str, v: &str| {
            body.extend_from_slice(k.as_bytes());
            body.push(0);
            body.extend_from_slice(v.as_bytes());
            body.push(0);
        };
        param("user", &self.login.user);
        if !self.login.database.is_empty() {
            param("database", &self.login.database);
        }
        param("application_name", "Fenix");
        param("client_encoding", "UTF8");
        param("DateStyle", "ISO");
        if self.login.read_only {
            param("options", "-c default_transaction_read_only=on");
        }
        body.push(0);
        let mut out = ((body.len() + 4) as i32).to_be_bytes().to_vec();
        out.extend(body);
        out
    }

    /// One message's body done with: what it means.
    fn message(&mut self, tag: u8, body: &[u8], out: &mut Vec<Action>) {
        match (tag, &self.state) {
            (b'E', State::Auth | State::Scram { .. } | State::ScramFinal { .. } | State::Starting) => {
                self.state = State::Closed;
                out.push(Action::Failed(error_text(body)));
            }
            (b'R', _) => self.auth(body, out),
            (b'S', _) => {
                let mut f = body.split(|&b| b == 0);
                if let (Some(k), Some(v)) = (f.next(), f.next()) {
                    if k == b"server_version" {
                        self.server_version = String::from_utf8_lossy(v).into_owned();
                    }
                }
            }
            (b'K' | b'N' | b'A' | b'1' | b'2' | b'3' | b'n' | b't' | b's', _) => {}
            (b'Z', State::Starting) => {
                self.state = State::Idle;
                out.push(Action::Ready);
            }
            (b'Z', State::Busy) => {
                self.finish_rows();
                self.state = State::Idle;
                out.push(Action::Done(std::mem::take(&mut self.outcomes)));
            }
            (b'T', State::Busy) => {
                self.finish_rows();
                self.rows = Some(Rows { columns: row_description(body), rows: Vec::new(), truncated: false });
            }
            (b'D', State::Busy) => {
                if let Some(rows) = &mut self.rows {
                    if rows.rows.len() >= self.login.max_rows.max(1) {
                        rows.truncated = true;
                    } else {
                        rows.rows.push(data_row(body, &rows.columns));
                    }
                }
            }
            (b'C', State::Busy) => {
                let tag = cstr(body);
                match self.rows.take() {
                    Some(rows) => self.outcomes.push(Outcome::Rows(rows)),
                    None => {
                        let changed = changed(&tag);
                        self.outcomes.push(Outcome::Done { tag, changed });
                    }
                }
            }
            (b'I', State::Busy) => {}
            (b'E', State::Busy) => {
                self.rows = None;
                self.outcomes.push(Outcome::Error(error_text(body)));
            }
            (b'E', _) => out.push(Action::Failed(error_text(body))),
            _ => {}
        }
    }

    fn finish_rows(&mut self) {
        // Rows with no CommandComplete after them: a portal that stopped.
        if let Some(rows) = self.rows.take() {
            self.outcomes.push(Outcome::Rows(rows));
        }
    }

    fn auth(&mut self, body: &[u8], out: &mut Vec<Action>) {
        if body.len() < 4 {
            return;
        }
        let code = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
        let data = &body[4..];
        match code {
            0 => self.state = State::Starting,
            3 => out.push(Action::Send(password_message(self.login.password.as_bytes()))),
            5 if data.len() >= 4 => out.push(Action::Send(password_message(md5_password(&self.login.user, &self.login.password, &data[..4]).as_bytes()))),
            10 => {
                let mechanisms: Vec<String> = data.split(|&b| b == 0).filter(|m| !m.is_empty()).map(|m| String::from_utf8_lossy(m).into_owned()).collect();
                if !mechanisms.iter().any(|m| m == "SCRAM-SHA-256") {
                    self.state = State::Closed;
                    out.push(Action::Failed(format!("the server wants {} to sign in, and only SCRAM-SHA-256 is spoken here", mechanisms.join(", "))));
                    return;
                }
                let nonce = B64.encode(random(18));
                let first_bare = format!("n=,r={nonce}");
                let first = format!("n,,{first_bare}");
                let mut body = b"SCRAM-SHA-256\0".to_vec();
                body.extend_from_slice(&(first.len() as i32).to_be_bytes());
                body.extend_from_slice(first.as_bytes());
                out.push(Action::Send(message(b'p', &body)));
                self.state = State::Scram { first_bare, nonce };
            }
            11 => {
                let State::Scram { first_bare, nonce } = self.state.clone() else { return };
                match scram_final(&self.login.password, &first_bare, &nonce, &String::from_utf8_lossy(data)) {
                    Ok((reply, server_signature)) => {
                        out.push(Action::Send(message(b'p', reply.as_bytes())));
                        self.state = State::ScramFinal { server_signature };
                    }
                    Err(e) => {
                        self.state = State::Closed;
                        out.push(Action::Failed(e));
                    }
                }
            }
            12 => {
                let State::ScramFinal { server_signature } = &self.state else { return };
                let text = String::from_utf8_lossy(data);
                let got = text.split(',').find_map(|p| p.strip_prefix("v=")).and_then(|v| B64.decode(v).ok());
                if got.as_deref() != Some(server_signature.as_slice()) {
                    self.state = State::Closed;
                    out.push(Action::Failed("the server's SCRAM signature doesn't check out: it isn't the server it says".into()));
                }
            }
            _ => {
                self.state = State::Closed;
                out.push(Action::Failed(format!("the server asks for a way of signing in not spoken here ({code}: Kerberos, GSSAPI or SSPI)")));
            }
        }
    }
}

impl Client for Postgres {
    fn start(&mut self) -> Vec<Action> {
        if self.tls == Tls::Off {
            self.state = State::Auth;
            return vec![Action::Send(self.startup())];
        }
        self.state = State::SslAnswer;
        let mut m = 8i32.to_be_bytes().to_vec();
        m.extend_from_slice(&SSL_REQUEST.to_be_bytes());
        vec![Action::Send(m)]
    }

    fn tls_ready(&mut self) -> Vec<Action> {
        if self.state != State::Handshake {
            return Vec::new();
        }
        self.state = State::Auth;
        vec![Action::Send(self.startup())]
    }

    fn receive(&mut self, data: &[u8]) -> Vec<Action> {
        let mut out = Vec::new();
        let mut data = data;
        if self.state == State::SslAnswer {
            let Some((&answer, rest)) = data.split_first() else { return out };
            data = rest;
            match answer {
                b'S' => {
                    self.state = State::Handshake;
                    out.push(Action::StartTls);
                    return out;
                }
                b'N' if self.tls == Tls::Require => {
                    self.state = State::Closed;
                    out.push(Action::Failed("the server doesn't do TLS, and this connection needs it (tls=require)".into()));
                    return out;
                }
                b'N' => {
                    self.state = State::Auth;
                    out.push(Action::Send(self.startup()));
                }
                _ => {
                    // An old server's error, in place of an answer.
                    self.state = State::Closed;
                    out.push(Action::Failed("the server didn't answer the TLS request as PostgreSQL does".into()));
                    return out;
                }
            }
        }
        self.buf.extend_from_slice(data);
        let mut at = 0;
        while self.buf.len() - at >= 5 {
            let tag = self.buf[at];
            let len = i32::from_be_bytes([self.buf[at + 1], self.buf[at + 2], self.buf[at + 3], self.buf[at + 4]]) as usize;
            if len < 4 || self.buf.len() - at < 1 + len {
                break;
            }
            let body = self.buf[at + 5..at + 1 + len].to_vec();
            at += 1 + len;
            self.message(tag, &body, &mut out);
            if self.state == State::Closed {
                break;
            }
        }
        self.buf.drain(..at);
        out
    }

    fn query(&mut self, sql: &str) -> Vec<Action> {
        self.state = State::Busy;
        let mut body = sql.as_bytes().to_vec();
        body.push(0);
        vec![Action::Send(message(b'Q', &body))]
    }

    fn query_one(&mut self, sql: &str) -> Vec<Action> {
        self.state = State::Busy;
        let mut m = Vec::new();
        // Parse: unnamed, no parameters.
        let mut parse = vec![0u8];
        parse.extend_from_slice(sql.as_bytes());
        parse.extend_from_slice(&[0, 0, 0]);
        m.extend(message(b'P', &parse));
        // Bind: unnamed portal and statement, no parameters, text results.
        m.extend(message(b'B', &[0, 0, 0, 0, 0, 0, 0, 0]));
        m.extend(message(b'D', b"P\0"));
        m.extend(message(b'E', &[0, 0, 0, 0, 0]));
        m.extend(message(b'S', &[]));
        vec![Action::Send(m)]
    }

    fn goodbye(&mut self) -> Vec<u8> {
        self.state = State::Closed;
        message(b'X', &[])
    }
}

/// A frontend message: its tag, its length, its body.
fn message(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut m = vec![tag];
    m.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
    m.extend_from_slice(body);
    m
}

fn password_message(password: &[u8]) -> Vec<u8> {
    let mut body = password.to_vec();
    body.push(0);
    message(b'p', &body)
}

/// md5's answer: `md5` and md5(md5(password + user) + salt), in hex.
pub fn md5_password(user: &str, password: &str, salt: &[u8]) -> String {
    let inner = format!("{:x}", md5::Md5::digest(format!("{password}{user}").as_bytes()));
    let mut outer = md5::Md5::new();
    outer.update(inner.as_bytes());
    outer.update(salt);
    format!("md5{:x}", outer.finalize())
}

type HmacSha256 = Hmac<Sha256>;

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut m = HmacSha256::new_from_slice(key).expect("HMAC takes a key of any length");
    m.update(data);
    m.finalize().into_bytes().to_vec()
}

/// SCRAM-SHA-256's last message, answering the server's first
/// (`r=...,s=...,i=...`), and the signature the server's to give back
/// (RFC 5802, 7677).
pub fn scram_final(password: &str, first_bare: &str, nonce: &str, server_first: &str) -> Result<(String, Vec<u8>), String> {
    let field = |k: &str| server_first.split(',').find_map(|p| p.strip_prefix(k)).map(str::to_string);
    let server_nonce = field("r=").ok_or("SCRAM: the server sent no nonce")?;
    if !server_nonce.starts_with(nonce) {
        return Err("SCRAM: the server's nonce isn't ours".into());
    }
    let salt = B64.decode(field("s=").ok_or("SCRAM: no salt")?).map_err(|_| "SCRAM: the salt isn't base64")?;
    let iterations: u32 = field("i=").and_then(|i| i.parse().ok()).ok_or("SCRAM: no iteration count")?;
    let mut salted = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, iterations, &mut salted);
    let client_key = hmac(&salted, b"Client Key");
    let stored_key = Sha256::digest(&client_key);
    let without_proof = format!("c=biws,r={server_nonce}");
    let auth_message = format!("{first_bare},{server_first},{without_proof}");
    let signature = hmac(&stored_key, auth_message.as_bytes());
    let proof: Vec<u8> = client_key.iter().zip(&signature).map(|(a, b)| a ^ b).collect();
    let server_key = hmac(&salted, b"Server Key");
    let server_signature = hmac(&server_key, auth_message.as_bytes());
    Ok((format!("{without_proof},p={}", B64.encode(proof)), server_signature))
}

/// A NUL-ended string.
fn cstr(b: &[u8]) -> String {
    String::from_utf8_lossy(b.split(|&c| c == 0).next().unwrap_or(b)).into_owned()
}

/// An ErrorResponse as a line: `ERROR 42P01: relation "x" does not exist`,
/// its detail and hint after.
pub fn error_text(body: &[u8]) -> String {
    let mut fields = std::collections::HashMap::new();
    let mut at = 0;
    while at < body.len() && body[at] != 0 {
        let code = body[at];
        let end = body[at + 1..].iter().position(|&b| b == 0).map_or(body.len(), |n| at + 1 + n);
        fields.insert(code, String::from_utf8_lossy(&body[at + 1..end]).into_owned());
        at = end + 1;
    }
    let get = |c: u8| fields.get(&c).cloned().unwrap_or_default();
    let mut s = format!("{} {}: {}", if fields.contains_key(&b'V') { get(b'V') } else { get(b'S') }, get(b'C'), get(b'M'));
    if fields.contains_key(&b'P') {
        s.push_str(&format!(" (at character {})", get(b'P')));
    }
    for (c, what) in [(b'D', "detail"), (b'H', "hint")] {
        if fields.contains_key(&c) {
            s.push_str(&format!(" -- {what}: {}", get(c)));
        }
    }
    s
}

/// A type's name and kind, by its oid: the common ones.
fn type_of(oid: u32) -> (&'static str, Kind) {
    match oid {
        16 => ("bool", Kind::Bool),
        17 => ("bytea", Kind::Bytes),
        18 => ("char", Kind::Text),
        19 => ("name", Kind::Text),
        20 => ("int8", Kind::Number),
        21 => ("int2", Kind::Number),
        23 => ("int4", Kind::Number),
        25 => ("text", Kind::Text),
        26 => ("oid", Kind::Number),
        114 => ("json", Kind::Text),
        700 => ("float4", Kind::Number),
        701 => ("float8", Kind::Number),
        790 => ("money", Kind::Text),
        869 => ("inet", Kind::Text),
        1042 => ("bpchar", Kind::Text),
        1043 => ("varchar", Kind::Text),
        1082 => ("date", Kind::Text),
        1083 => ("time", Kind::Text),
        1114 => ("timestamp", Kind::Text),
        1184 => ("timestamptz", Kind::Text),
        1186 => ("interval", Kind::Text),
        1700 => ("numeric", Kind::Number),
        2950 => ("uuid", Kind::Text),
        3802 => ("jsonb", Kind::Text),
        _ => ("", Kind::Text),
    }
}

fn row_description(body: &[u8]) -> Vec<Column> {
    let mut cols = Vec::new();
    if body.len() < 2 {
        return cols;
    }
    let n = u16::from_be_bytes([body[0], body[1]]) as usize;
    let mut at = 2;
    for _ in 0..n {
        let Some(end) = body[at..].iter().position(|&b| b == 0).map(|e| at + e) else { break };
        let name = String::from_utf8_lossy(&body[at..end]).into_owned();
        at = end + 1;
        if body.len() < at + 18 {
            break;
        }
        let oid = u32::from_be_bytes([body[at + 6], body[at + 7], body[at + 8], body[at + 9]]);
        at += 18;
        let (t, kind) = type_of(oid);
        cols.push(Column::new(name, if t.is_empty() { format!("oid {oid}") } else { t.to_string() }, kind));
    }
    cols
}

fn data_row(body: &[u8], columns: &[Column]) -> Vec<Option<String>> {
    let mut row = Vec::new();
    if body.len() < 2 {
        return row;
    }
    let n = u16::from_be_bytes([body[0], body[1]]) as usize;
    let mut at = 2;
    for i in 0..n {
        if body.len() < at + 4 {
            break;
        }
        let len = i32::from_be_bytes([body[at], body[at + 1], body[at + 2], body[at + 3]]);
        at += 4;
        if len < 0 {
            row.push(None);
            continue;
        }
        let end = (at + len as usize).min(body.len());
        let bytes = &body[at..end];
        at = end;
        row.push(Some(match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            // Text it isn't: a binary column.
            Err(_) if columns.get(i).is_some_and(|c| c.kind == Kind::Bytes) => hex(bytes),
            Err(_) => String::from_utf8_lossy(bytes).into_owned(),
        }));
    }
    row
}

/// The rows a CommandComplete's tag says changed: `UPDATE 3`, `INSERT 0 1`.
fn changed(tag: &str) -> Option<u64> {
    let mut words = tag.split_whitespace();
    match words.next()? {
        "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "COPY" | "MOVE" | "FETCH" | "SELECT" => tag.split_whitespace().last()?.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend message, as the server sends it.
    fn backend(tag: u8, body: &[u8]) -> Vec<u8> {
        message(tag, body)
    }

    fn login() -> Login {
        Login { user: "ada".into(), password: "pw".into(), database: "shop".into(), max_rows: 2, read_only: true }
    }

    #[test]
    fn the_startup_message_and_its_tls_request() {
        let mut pg = Postgres::new(login(), Tls::Prefer);
        assert_eq!(pg.start(), [Action::Send(vec![0, 0, 0, 8, 4, 210, 22, 47])], "SSLRequest");
        assert_eq!(pg.receive(b"S"), [Action::StartTls]);
        let Action::Send(startup) = &pg.tls_ready()[0] else { panic!() };
        let expected = b"\x00\x03\x00\x00user\0ada\0database\0shop\0application_name\0Fenix\0client_encoding\0UTF8\0DateStyle\0ISO\0options\0-c default_transaction_read_only=on\0\0";
        assert_eq!(&startup[4..], &expected[..]);
        assert_eq!(u32::from_be_bytes(startup[..4].try_into().unwrap()) as usize, startup.len());
        // No TLS there: on without it, unless it's required.
        let mut pg = Postgres::new(login(), Tls::Prefer);
        pg.start();
        assert!(matches!(&pg.receive(b"N")[..], [Action::Send(m)] if m[4..8] == PROTOCOL.to_be_bytes()));
        let mut pg = Postgres::new(login(), Tls::Require);
        pg.start();
        assert!(matches!(&pg.receive(b"N")[..], [Action::Failed(e)] if e.contains("tls=require")));
    }

    #[test]
    fn md5_and_cleartext_passwords() {
        // md5(md5("pw" + "ada") + salt), as Python's hashlib makes it.
        assert_eq!(md5_password("ada", "pw", &[1, 2, 3, 4]), "md52f78d09189bb1f19c38433d30d99bfbb");
        let mut pg = Postgres::new(login(), Tls::Off);
        pg.start();
        let mut r = 5i32.to_be_bytes().to_vec();
        r.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(pg.receive(&backend(b'R', &r)), [Action::Send(password_message(b"md52f78d09189bb1f19c38433d30d99bfbb"))]);
        let mut pg = Postgres::new(login(), Tls::Off);
        pg.start();
        assert_eq!(pg.receive(&backend(b'R', &3i32.to_be_bytes())), [Action::Send(b"p\x00\x00\x00\x07pw\x00".to_vec())]);
    }

    #[test]
    fn scram_sha_256_as_rfc_7677_has_it() {
        let first_bare = "n=user,r=rOprNGfwEbeRWgbNEkqO";
        let server_first = "r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096";
        let (reply, sig) = scram_final("pencil", first_bare, "rOprNGfwEbeRWgbNEkqO", server_first).unwrap();
        assert_eq!(reply, "c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,p=dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=");
        assert_eq!(B64.encode(sig), "6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=");
        assert!(scram_final("pencil", first_bare, "someone-else", server_first).is_err());
    }

    #[test]
    fn a_scram_sign_in_and_a_forged_signature() {
        let mut pg = Postgres::new(login(), Tls::Off);
        pg.start();
        let mut sasl = 10i32.to_be_bytes().to_vec();
        sasl.extend_from_slice(b"SCRAM-SHA-256-PLUS\0SCRAM-SHA-256\0\0");
        let Action::Send(initial) = &pg.receive(&backend(b'R', &sasl))[0] else { panic!() };
        assert!(initial.starts_with(b"p") && initial[5..].starts_with(b"SCRAM-SHA-256\0"));
        let State::Scram { nonce, .. } = pg.state.clone() else { panic!() };
        let mut cont = 11i32.to_be_bytes().to_vec();
        cont.extend_from_slice(format!("r={nonce}srv,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=16").as_bytes());
        assert!(matches!(&pg.receive(&backend(b'R', &cont))[..], [Action::Send(m)] if m.starts_with(b"p")));
        let mut fin = 12i32.to_be_bytes().to_vec();
        fin.extend_from_slice(b"v=AAAA");
        assert!(matches!(&pg.receive(&backend(b'R', &fin))[..], [Action::Failed(e)] if e.contains("signature")));
        // Only the -PLUS kind offered: none spoken here.
        let mut pg = Postgres::new(login(), Tls::Off);
        pg.start();
        let mut sasl = 10i32.to_be_bytes().to_vec();
        sasl.extend_from_slice(b"SCRAM-SHA-256-PLUS\0\0");
        assert!(matches!(&pg.receive(&backend(b'R', &sasl))[..], [Action::Failed(_)]));
    }

    /// What a server sends for `select 1 as n, null::text as t, 'x' ...;
    /// update ...`: rows, then a CommandComplete; then an error.
    #[test]
    fn results_come_in_as_rows_tags_and_errors() {
        let mut pg = Postgres::new(login(), Tls::Off);
        pg.start();
        let mut bytes = backend(b'R', &0i32.to_be_bytes());
        bytes.extend(backend(b'S', b"server_version\x0016.4\0"));
        bytes.extend(backend(b'K', &[0, 0, 0, 1, 0, 0, 0, 2]));
        bytes.extend(backend(b'Z', b"I"));
        // Split anywhere: messages are put together as they come.
        let (a, b) = bytes.split_at(7);
        assert_eq!(pg.receive(a), []);
        assert_eq!(pg.receive(b), [Action::Ready]);
        assert_eq!(pg.server_version, "16.4");

        assert_eq!(pg.query("select ...; update ...; bad"), [Action::Send(b"Q\x00\x00\x00\x20select ...; update ...; bad\0".to_vec())]);
        let mut rd = vec![0, 2];
        for (name, oid) in [("n", 23u32), ("t", 25)] {
            rd.extend_from_slice(name.as_bytes());
            rd.push(0);
            rd.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
            rd.extend_from_slice(&oid.to_be_bytes());
            rd.extend_from_slice(&[0, 4, 255, 255, 255, 255, 0, 0]);
        }
        let mut stream = backend(b'T', &rd);
        for (n, t) in [(&b"1"[..], None), (b"2", Some(&b"x"[..])), (b"3", None)] {
            let mut d = vec![0, 2];
            d.extend_from_slice(&(n.len() as i32).to_be_bytes());
            d.extend_from_slice(n);
            match t {
                Some(t) => {
                    d.extend_from_slice(&(t.len() as i32).to_be_bytes());
                    d.extend_from_slice(t);
                }
                None => d.extend_from_slice(&(-1i32).to_be_bytes()),
            }
            stream.extend(backend(b'D', &d));
        }
        stream.extend(backend(b'C', b"SELECT 3\0"));
        stream.extend(backend(b'C', b"UPDATE 7\0"));
        stream.extend(backend(b'E', b"SERROR\0VERROR\0C42601\0Msyntax error at or near \"bad\"\0P25\0\0"));
        stream.extend(backend(b'Z', b"I"));
        let got = pg.receive(&stream);
        let [Action::Done(outcomes)] = &got[..] else { panic!("{got:?}") };
        let Outcome::Rows(rows) = &outcomes[0] else { panic!() };
        assert_eq!(rows.columns, [Column::new("n", "int4", Kind::Number), Column::new("t", "text", Kind::Text)]);
        assert_eq!(rows.rows, [vec![Some("1".to_string()), None], vec![Some("2".to_string()), Some("x".to_string())]]);
        assert!(rows.truncated, "max_rows is 2");
        assert_eq!(outcomes[1], Outcome::Done { tag: "UPDATE 7".into(), changed: Some(7) });
        assert_eq!(outcomes[2], Outcome::Error("ERROR 42601: syntax error at or near \"bad\" (at character 25)".into()));
    }

    #[test]
    fn an_extended_query_is_parse_bind_describe_execute_sync() {
        let mut pg = Postgres::new(login(), Tls::Off);
        let [Action::Send(m)] = &pg.query_one("select 1")[..] else { panic!() };
        let tags: Vec<u8> = {
            let mut at = 0;
            let mut t = Vec::new();
            while at < m.len() {
                t.push(m[at]);
                at += 1 + u32::from_be_bytes(m[at + 1..at + 5].try_into().unwrap()) as usize;
            }
            t
        };
        assert_eq!(tags, b"PBDES");
        assert_eq!(&m[..17], b"P\x00\x00\x00\x10\0select 1\0\0\0");
        let mut stream = backend(b'1', &[]);
        stream.extend(backend(b'2', &[]));
        stream.extend(backend(b'n', &[]));
        stream.extend(backend(b'C', b"CREATE TABLE\0"));
        stream.extend(backend(b'Z', b"I"));
        assert_eq!(pg.receive(&stream), [Action::Done(vec![Outcome::Done { tag: "CREATE TABLE".into(), changed: None }])]);
        assert_eq!(pg.goodbye(), b"X\x00\x00\x00\x04");
    }

    #[test]
    fn a_failed_sign_in() {
        let mut pg = Postgres::new(login(), Tls::Off);
        pg.start();
        let got = pg.receive(&backend(b'E', b"SFATAL\0C28P01\0Mpassword authentication failed for user \"ada\"\0\0"));
        assert_eq!(got, [Action::Failed("FATAL 28P01: password authentication failed for user \"ada\"".into())]);
    }

    /// What PostgreSQL 16 sent, captured: `select * from app.users order
    /// by id; select 1/0; select 2`, and the catalog's four queries.
    #[test]
    fn what_a_real_server_sent() {
        let ready = || {
            let mut pg = Postgres::new(login(), Tls::Off);
            pg.start();
            let mut b = backend(b'R', &0i32.to_be_bytes());
            b.extend(backend(b'Z', b"I"));
            assert_eq!(pg.receive(&b), [Action::Ready]);
            pg.login.max_rows = 100;
            pg
        };
        let mut pg = ready();
        pg.query("select * from app.users order by id; select 1/0; select 2");
        let got = pg.receive(include_bytes!("testdata/postgres-query.bin"));
        let [Action::Done(outcomes)] = &got[..] else { panic!("{got:?}") };
        let Outcome::Rows(users) = &outcomes[0] else { panic!() };
        assert_eq!(users.columns.iter().map(|c| c.type_name.as_str()).collect::<Vec<_>>(), ["int4", "text", "varchar", "numeric", "bool", "bytea"]);
        assert_eq!(users.rows[0], [Some("1".into()), Some("ada".into()), Some("a@x".into()), Some("1.50".into()), Some("t".into()), Some("\\x0102".into())]);
        assert_eq!(outcomes[1], Outcome::Error("ERROR 22012: division by zero".into()), "and the statement after isn't run");
        assert_eq!(outcomes.len(), 2);
        let mut pg = ready();
        pg.query(&crate::schema::catalog(crate::conn::Engine::Postgres, "").join(";"));
        let [Action::Done(outcomes)] = &pg.receive(include_bytes!("testdata/postgres-catalog.bin"))[..] else { panic!() };
        let schema = crate::schema::from_outcomes(crate::conn::Engine::Postgres, outcomes).unwrap();
        let orders = schema.table("app", "orders").unwrap();
        assert_eq!(orders.primary_key(), ["id", "line"]);
        assert_eq!(orders.columns.iter().map(|c| c.ty.as_str()).collect::<Vec<_>>(), ["integer", "integer", "integer", "double precision"]);
        assert!(schema.table("app", "names").unwrap().view);
        assert_eq!(schema.table("app", "users").unwrap().column("email").map(|c| c.ty.as_str()), Some("character varying(200)"));
    }
}
