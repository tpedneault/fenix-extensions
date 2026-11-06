//! One server's connection: dialled, signed in, kept alive with PINGs,
//! dialled again with a growing pause when it drops, and its subscriptions
//! made again each time. What it reads goes through `fenix_nats`'s parser;
//! where each message goes is [`crate::core`]'s.

use std::collections::VecDeque;

use fenix_nats::auth::{Creds, KeyPair};
use fenix_nats::inbox::Requests;
use fenix_nats::{encode, Connect, Parser, ServerInfo};
use rustc_hash::FxHashMap;

use crate::core::Pending;

/// What the connection asks of the network: Fenix's sockets, or a test's.
pub trait Net {
    fn connect(&mut self, host: &str, port: u16, tls: bool) -> Result<u64, String>;
    fn start_tls(&mut self, socket: u64, host: &str) -> Result<(), String>;
    fn write(&mut self, socket: u64, data: &[u8]) -> Result<(), String>;
    fn close(&mut self, socket: u64);
}

/// The default port.
pub const PORT: u16 = 4222;

/// A server, from `nats.servers`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addr {
    /// As written.
    pub url: String,
    pub host: String,
    pub port: u16,
    /// `tls://`: TLS from the start.
    pub tls: bool,
}

impl Addr {
    /// Whether it's this machine: a server no one else is on, most likely.
    pub fn is_local(&self) -> bool {
        matches!(self.host.to_ascii_lowercase().as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]")
    }

    /// `nats://host:4222`, `tls://host:4443`, `host`, `host:port`.
    pub fn parse(url: &str) -> Result<Addr, String> {
        let url = url.trim();
        let (scheme, rest) = url.split_once("://").unwrap_or(("nats", url));
        let tls = match scheme.to_ascii_lowercase().as_str() {
            "nats" | "tcp" => false,
            "tls" => true,
            other => return Err(format!("{url}: {other}:// -- a server is nats://host:port or tls://host:port")),
        };
        let rest = rest.trim_end_matches('/');
        if rest.contains('@') {
            return Err(format!("{url}: credentials don't go in the URL -- SPC o m a keeps them in the keychain"));
        }
        let (host, port) = match rest.strip_prefix('[') {
            Some(v6) => {
                let (h, p) = v6.split_once(']').ok_or(format!("{url}: an IPv6 address ends with ]"))?;
                (h.to_string(), p.strip_prefix(':').unwrap_or(""))
            }
            None => match rest.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), p),
                None => (rest.to_string(), ""),
            },
        };
        if host.is_empty() || host.contains(['/', ' ']) {
            return Err(format!("{url}: no host"));
        }
        let port = if port.is_empty() { PORT } else { port.parse().map_err(|_| format!("{url}: {port} isn't a port"))? };
        Ok(Addr { url: url.to_string(), host: host.to_ascii_lowercase(), port, tls })
    }

    /// `host:port`: its name in the keychain and on the page.
    pub fn key(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// How it signs in.
#[derive(Debug, Clone, Default)]
pub enum Auth {
    #[default]
    None,
    UserPassword(String, String),
    Token(String),
    NKey(KeyPair),
    Creds(Box<Creds>),
}

impl Auth {
    pub fn name(&self) -> &'static str {
        match self {
            Auth::None => "none",
            Auth::UserPassword(..) => "user",
            Auth::Token(_) => "token",
            Auth::NKey(_) => "nkey",
            Auth::Creds(_) => "creds",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Not wanted.
    Down,
    /// Dialling.
    Connecting,
    /// The server wants TLS: the handshake after its INFO.
    Upgrading,
    /// CONNECT sent, waiting for the PONG that says it's taken.
    Handshake,
    Up,
    /// Dropped: dialled again at this time (µs).
    Waiting(u64),
}

/// What a subscription is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// The live view.
    Live,
    /// Replies to requests.
    Inbox,
    /// An agent's sample: the call it answers.
    Sample(u64),
    /// A KV bucket watched.
    Watch(String),
}

/// The pauses before dialling again, in ms; the last repeats.
pub const BACKOFF: [u64; 8] = [250, 500, 1000, 2000, 4000, 8000, 15000, 30000];

/// PINGs: one this often, and a connection with this many unanswered is
/// taken for dead.
pub const PING_MS: u64 = 20_000;
pub const MAX_PINGS: usize = 3;

#[derive(Debug)]
pub struct Conn {
    pub id: u16,
    pub addr: Addr,
    pub phase: Phase,
    pub socket: Option<u64>,
    pub parser: Parser,
    pub info: Option<ServerInfo>,
    pub auth: Auth,
    pub client_name: String,
    /// The last PING's round trip, in microseconds: the server's time and
    /// the way back, behind what's queued for this extension.
    pub rtt_us: Option<u64>,
    pings: VecDeque<u64>,
    pub last_ping_us: u64,
    pub attempts: u32,
    pub error: Option<String>,
    /// The subscriptions you asked for.
    pub patterns: Vec<String>,
    /// The structured filter's subjects: what's subscribed is narrowed to
    /// them on the server.
    pub narrow: Vec<String>,
    /// The live subscriptions on the server: subject and sid.
    pub active: Vec<(String, u64)>,
    pub routes: FxHashMap<u64, Route>,
    next_sid: u64,
    pub inbox: Requests<Pending>,
    inbox_sid: Option<u64>,
    pub up_since: Option<u64>,
    pub reconnects: u32,
    pub msgs_in: u64,
    pub bytes_in: u64,
    /// A permission error from the server: shown, not fatal.
    pub warning: Option<String>,
    /// Signing in was refused: it waits for new credentials.
    pub auth_failed: bool,
}

impl Conn {
    pub fn new(id: u16, addr: Addr, patterns: Vec<String>, inbox: &str, client_name: &str) -> Conn {
        Conn {
            id,
            addr,
            phase: Phase::Down,
            socket: None,
            parser: Parser::new(),
            info: None,
            auth: Auth::None,
            client_name: client_name.to_string(),
            rtt_us: None,
            pings: VecDeque::new(),
            last_ping_us: 0,
            attempts: 0,
            error: None,
            patterns,
            narrow: Vec::new(),
            active: Vec::new(),
            routes: FxHashMap::default(),
            next_sid: 0,
            inbox: Requests::new(inbox),
            inbox_sid: None,
            up_since: None,
            reconnects: 0,
            msgs_in: 0,
            bytes_in: 0,
            warning: None,
            auth_failed: false,
        }
    }

    pub fn is_up(&self) -> bool {
        self.phase == Phase::Up || self.phase == Phase::Handshake
    }

    pub fn wanted(&self) -> bool {
        self.phase != Phase::Down
    }

    /// A word for its state: the modeline's and the page's.
    pub fn state(&self) -> &'static str {
        match self.phase {
            Phase::Down => "off",
            Phase::Connecting | Phase::Upgrading | Phase::Handshake => "connecting",
            Phase::Up => "up",
            Phase::Waiting(_) => "retrying",
        }
    }

    /// Dials it.
    pub fn connect(&mut self, net: &mut dyn Net, now: u64) {
        if let Some(s) = self.socket.take() {
            net.close(s);
        }
        self.parser.reset();
        self.pings.clear();
        self.active.clear();
        self.inbox_sid = None;
        self.routes.retain(|_, r| matches!(r, Route::Sample(_) | Route::Watch(_)));
        match net.connect(&self.addr.host, self.addr.port, self.addr.tls) {
            Ok(s) => {
                self.socket = Some(s);
                self.phase = Phase::Connecting;
            }
            Err(e) => {
                self.phase = Phase::Connecting;
                self.dropped(now, Some(e));
            }
        }
    }

    /// Hangs up, and stays down.
    pub fn disconnect(&mut self, net: &mut dyn Net) -> Vec<Pending> {
        if let Some(s) = self.socket.take() {
            net.close(s);
        }
        self.phase = Phase::Down;
        self.up_since = None;
        self.active.clear();
        self.inbox.drain()
    }

    /// The socket dropped: dialled again after a pause. The requests that
    /// were waiting are given up.
    pub fn dropped(&mut self, now: u64, why: Option<String>) -> Vec<Pending> {
        self.socket = None;
        if self.phase == Phase::Down {
            return self.inbox.drain();
        }
        if self.phase == Phase::Up {
            self.reconnects += 1;
        }
        if let Some(why) = why {
            if self.error.as_deref().is_none_or(|e| !e.contains("Authorization")) {
                self.error = Some(why);
            }
        }
        let wait = BACKOFF[(self.attempts as usize).min(BACKOFF.len() - 1)];
        // A little jitter, so connections dropped together don't all
        // come back at once.
        let jitter = (now.wrapping_mul(2_654_435_761) >> 7) % (wait / 5 + 1);
        let wait = if self.auth_failed { 30_000 } else { wait + jitter };
        self.attempts = self.attempts.saturating_add(1);
        self.phase = Phase::Waiting(now + wait * 1000);
        self.up_since = None;
        self.active.clear();
        self.inbox.drain()
    }

    fn send(&mut self, net: &mut dyn Net, out: &[u8]) {
        if out.is_empty() {
            return;
        }
        if let Some(s) = self.socket {
            if let Err(e) = net.write(s, out) {
                self.error = Some(e);
            }
        }
    }

    /// The socket's up (or its TLS is).
    pub fn connected(&mut self, net: &mut dyn Net, now: u64) {
        if self.phase == Phase::Upgrading {
            self.sign_in(net, now);
        }
        // Otherwise the server speaks first: its INFO.
    }

    /// What the server said of itself: TLS if it wants it, then CONNECT.
    pub fn on_info(&mut self, net: &mut dyn Net, info: ServerInfo, now: u64) {
        let first = self.phase == Phase::Connecting;
        let wants_tls = info.tls_required && !self.addr.tls;
        self.info = Some(info);
        if !first {
            // An INFO later on: the cluster changed, or lame-duck mode.
            return;
        }
        if wants_tls {
            self.phase = Phase::Upgrading;
            if let Some(s) = self.socket {
                if let Err(e) = net.start_tls(s, &self.addr.host) {
                    self.error = Some(format!("TLS: {e}"));
                }
            }
            return;
        }
        self.sign_in(net, now);
    }

    fn sign_in(&mut self, net: &mut dyn Net, now: u64) {
        let nonce = self.info.as_ref().and_then(|i| i.nonce.clone()).unwrap_or_default();
        let mut c = Connect::new(&self.client_name);
        c.tls_required = self.addr.tls || self.phase == Phase::Upgrading;
        c = match &self.auth {
            Auth::None => c,
            Auth::UserPassword(u, p) => c.user_password(u, p),
            Auth::Token(t) => c.token(t),
            Auth::NKey(k) => c.nkey(k, None, &nonce),
            Auth::Creds(creds) => c.nkey(&creds.key, Some(&creds.jwt), &nonce),
        };
        let mut out = Vec::new();
        encode::connect(&mut out, &c);
        encode::ping(&mut out);
        self.pings.push_back(now);
        self.last_ping_us = now;
        self.phase = Phase::Handshake;
        self.resubscribe(&mut out);
        self.send(net, &out);
    }

    /// What it should be subscribed to for the live view: what you asked
    /// for, narrowed to the filter's subjects.
    pub fn wanted_subjects(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in &self.patterns {
            if self.narrow.is_empty() {
                out.push(p.clone());
            } else {
                for n in &self.narrow {
                    if let Some(x) = fenix_nats::subject::intersect(p, n) {
                        out.push(x);
                    }
                }
            }
        }
        // One that another covers is left out: a message would come twice.
        let all = out.clone();
        out.retain(|s| !all.iter().any(|o| o != s && fenix_nats::subject::covers(o, s)));
        out.sort();
        out.dedup();
        out
    }

    fn sid(&mut self) -> u64 {
        self.next_sid += 1;
        self.next_sid
    }

    /// The live subscriptions made to match what's wanted; the inbox, the
    /// samples and the watches made again after a reconnect.
    fn resubscribe(&mut self, out: &mut Vec<u8>) {
        let want = self.wanted_subjects();
        let mut gone = Vec::new();
        self.active.retain(|(s, sid)| {
            let keep = want.contains(s);
            if !keep {
                gone.push(*sid);
            }
            keep
        });
        for sid in gone {
            encode::unsub(out, sid, None);
            self.routes.remove(&sid);
        }
        for s in want {
            if !self.active.iter().any(|(a, _)| *a == s) {
                let sid = self.sid();
                if encode::sub(out, &s, None, sid).is_ok() {
                    self.active.push((s, sid));
                    self.routes.insert(sid, Route::Live);
                }
            }
        }
        if self.phase == Phase::Handshake {
            // After a reconnect, the samples' and watches' own again.
            let others: Vec<(u64, Route)> = self.routes.iter().filter(|(_, r)| matches!(r, Route::Sample(_) | Route::Watch(_))).map(|(s, r)| (*s, r.clone())).collect();
            for (sid, r) in others {
                let subject = match &r {
                    Route::Watch(bucket) => fenix_nats::kv::all(bucket),
                    _ => continue,
                };
                let _ = encode::sub(out, &subject, None, sid);
            }
            if !self.inbox.is_empty() || self.inbox_sid.is_some() {
                self.inbox_sid = None;
                self.ensure_inbox(out);
            }
        }
    }

    /// Makes the server's subscriptions what's wanted now.
    pub fn sync(&mut self, net: &mut dyn Net) {
        if !self.is_up() {
            return;
        }
        let mut out = Vec::new();
        self.resubscribe(&mut out);
        self.send(net, &out);
    }

    fn ensure_inbox(&mut self, out: &mut Vec<u8>) {
        if self.inbox_sid.is_none() {
            let sid = self.sid();
            if encode::sub(out, &self.inbox.subscription(), None, sid).is_ok() {
                self.inbox_sid = Some(sid);
                self.routes.insert(sid, Route::Inbox);
            }
        }
    }

    /// The first PONG: signed in.
    pub fn on_pong(&mut self, now: u64) -> bool {
        if let Some(sent) = self.pings.pop_front() {
            self.rtt_us = Some(now.saturating_sub(sent));
        }
        if self.phase == Phase::Handshake {
            self.phase = Phase::Up;
            self.attempts = 0;
            self.error = None;
            self.auth_failed = false;
            self.up_since = Some(now);
            return true;
        }
        false
    }

    pub fn on_ping(&mut self, net: &mut dyn Net) {
        let mut out = Vec::new();
        encode::pong(&mut out);
        self.send(net, &out);
    }

    /// `-ERR`: a permission refused is said and the connection goes on;
    /// anything else, the server closes it.
    pub fn on_err(&mut self, text: &str) {
        let lower = text.to_ascii_lowercase();
        if lower.contains("permissions violation") {
            self.warning = Some(text.to_string());
            return;
        }
        if lower.contains("authorization") || lower.contains("authentication") || lower.contains("user authentication") {
            self.auth_failed = true;
        }
        self.error = Some(text.to_string());
    }

    /// Every so often: a PING, and a dead one dialled again. Whether it's
    /// to be dialled now.
    pub fn tick(&mut self, net: &mut dyn Net, now: u64) -> bool {
        match self.phase {
            Phase::Waiting(at) if now >= at => return true,
            Phase::Up if now.saturating_sub(self.last_ping_us) >= PING_MS * 1000 => {
                if self.pings.len() >= MAX_PINGS {
                    self.error = Some("stale: no answer to three PINGs".into());
                    if let Some(s) = self.socket.take() {
                        net.close(s);
                    }
                    return false;
                }
                self.pings.push_back(now);
                self.last_ping_us = now;
                let mut out = Vec::new();
                encode::ping(&mut out);
                self.send(net, &out);
            }
            _ => {}
        }
        false
    }

    /// Publishes, with headers when there are some.
    pub fn publish(&mut self, net: &mut dyn Net, subject: &str, reply: Option<&str>, headers: &fenix_nats::headers::HeaderMap, payload: &[u8]) -> Result<(), String> {
        if !self.is_up() {
            return Err(format!("{} isn't connected", self.addr.key()));
        }
        if !headers.is_empty() && self.info.as_ref().is_some_and(|i| !i.headers) {
            return Err("this server doesn't take headers".into());
        }
        if let Some(max) = self.info.as_ref().map(|i| i.max_payload).filter(|m| *m > 0) {
            if (payload.len() + headers.encode().len()) as u64 > max {
                return Err(format!("{} bytes: the server takes at most {max}", payload.len()));
            }
        }
        headers.check()?;
        let mut out = Vec::new();
        encode::message(&mut out, subject, reply, headers, payload)?;
        self.send(net, &out);
        Ok(())
    }

    /// A request: published with a reply subject of its inbox's, `what`
    /// waiting for the answer until `deadline`.
    pub fn request(&mut self, net: &mut dyn Net, subject: &str, headers: &fenix_nats::headers::HeaderMap, payload: &[u8], what: Pending, deadline: u64) -> Result<(), String> {
        if !self.is_up() {
            return Err(format!("{} isn't connected", self.addr.key()));
        }
        let mut out = Vec::new();
        self.ensure_inbox(&mut out);
        self.send(net, &out);
        let reply = self.inbox.add(what, deadline);
        if let Err(e) = self.publish(net, subject, Some(&reply), headers, payload) {
            self.inbox.take(&reply);
            return Err(e);
        }
        Ok(())
    }

    /// A subscription of its own for `route`; its sid.
    pub fn subscribe_route(&mut self, net: &mut dyn Net, subject: &str, route: Route, max: Option<u64>) -> Result<u64, String> {
        if !self.is_up() {
            return Err(format!("{} isn't connected", self.addr.key()));
        }
        let sid = self.sid();
        let mut out = Vec::new();
        encode::sub(&mut out, subject, None, sid)?;
        if let Some(max) = max {
            encode::unsub(&mut out, sid, Some(max));
        }
        self.routes.insert(sid, route);
        self.send(net, &out);
        Ok(sid)
    }

    pub fn unsubscribe_route(&mut self, net: &mut dyn Net, sid: u64) {
        if self.routes.remove(&sid).is_some() {
            let mut out = Vec::new();
            encode::unsub(&mut out, sid, None);
            self.send(net, &out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_are_read() {
        assert_eq!(Addr::parse("nats://Demo.NATS.io:4222").unwrap(), Addr { url: "nats://Demo.NATS.io:4222".into(), host: "demo.nats.io".into(), port: 4222, tls: false });
        assert!(Addr::parse("tls://secure:4443/").unwrap().tls);
        assert_eq!(Addr::parse("localhost").unwrap().port, 4222);
        assert_eq!(Addr::parse("[::1]:5222").unwrap().host, "::1");
        assert!(Addr::parse("nats://u:p@host:4222").is_err());
        assert!(Addr::parse("http://host").is_err());
        assert!(Addr::parse("nats://host:x").is_err());
    }

    fn conn(patterns: &[&str]) -> Conn {
        Conn::new(0, Addr::parse("localhost").unwrap(), patterns.iter().map(|s| s.to_string()).collect(), "_INBOX.t", "fenix")
    }

    #[test]
    fn the_filter_narrows_what_is_subscribed() {
        let mut c = conn(&[">"]);
        assert_eq!(c.wanted_subjects(), [">"]);
        c.narrow = vec!["orders.*".into()];
        assert_eq!(c.wanted_subjects(), ["orders.*"]);
        let mut c = conn(&["orders.>", "billing.>"]);
        c.narrow = vec!["*.eu".into()];
        assert_eq!(c.wanted_subjects(), ["billing.eu", "orders.eu"]);
        c.narrow = vec!["shipping.>".into()];
        assert!(c.wanted_subjects().is_empty());
        let c = conn(&["a.>", "a.b", ">"]);
        assert_eq!(c.wanted_subjects(), [">"], "covered ones go");
    }

    #[test]
    fn it_backs_off_and_tries_again() {
        let mut c = conn(&[">"]);
        c.phase = Phase::Up;
        c.dropped(1_000_000, Some("reset".into()));
        let Phase::Waiting(at) = c.phase else { panic!() };
        assert!((1_250_000..=1_300_000).contains(&at), "{at}");
        assert_eq!(c.reconnects, 1);
        for _ in 0..20 {
            c.dropped(1_000_000, None);
        }
        let Phase::Waiting(at) = c.phase else { panic!() };
        assert!((31_000_000..=37_000_000).contains(&at), "{at}");
    }
}
