//! The client: its connections, the store, and the path every message
//! takes from the socket to the store -- kept to a few additions and one
//! copy a message, so a busy server never finds Fenix slow to read.
//!
//! Nothing here touches Fenix: the network is a [`Net`], time is given,
//! and what the page or the user should hear of is left in `events` and
//! `replies` for the extension to act on. So all of it runs in tests, at
//! full speed, on synthetic streams.

use std::collections::VecDeque;

use fenix_nats::inbox::Nuid;
use fenix_nats::{Msg, Op, ServerInfo};
use rustc_hash::FxHashMap;

use crate::conn::{Addr, Auth, Conn, Net, Phase, Route};
use crate::filter::Rule;
use crate::store::{Limits, Store};

/// What a request waits for.
#[derive(Debug, Clone)]
pub enum Pending {
    /// A request you sent from the page.
    User { subject: String, started_us: u64 },
    /// JetStream's API, for its page.
    Js { want: crate::js::Want, started_us: u64 },
    /// An agent's request.
    Tool { call: u64, started_us: u64 },
}

impl Pending {
    pub fn started_us(&self) -> u64 {
        match self {
            Pending::User { started_us, .. } | Pending::Js { started_us, .. } | Pending::Tool { started_us, .. } => *started_us,
        }
    }
}

/// A message kept whole, outside the store: a reply, a sample.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Owned {
    pub subject: String,
    pub reply: Option<String>,
    pub headers: Option<Vec<u8>>,
    pub payload: Vec<u8>,
    /// Unix microseconds it came.
    pub at_us: u64,
}

impl Owned {
    pub fn of(m: &Msg<'_>, at_us: u64) -> Owned {
        Owned { subject: m.subject.into(), reply: m.reply.map(Into::into), headers: m.headers.map(<[u8]>::to_vec), payload: m.payload.to_vec(), at_us }
    }

    /// A reply's status header: 503 when no one answers.
    pub fn status(&self) -> Option<u16> {
        self.headers.as_deref().and_then(|h| fenix_nats::headers::Headers::parse(h).ok()).and_then(|h| h.status)
    }
}

/// A request answered, or given up on.
#[derive(Debug, Clone)]
pub struct Reply {
    pub conn: u16,
    pub what: Pending,
    pub result: Result<Owned, String>,
    pub latency_us: u64,
}

/// What the extension should hear of.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Up(u16),
    Down(u16, Option<String>),
    /// Signing in was refused.
    AuthFailed(u16, String),
    /// A permission refused: the connection goes on.
    Warning(u16, String),
    /// A highlight rule that notifies matched: how many times since it last
    /// said so, and the latest.
    Alert { rule: usize, conn: u16, subject: String, preview: String, count: u32 },
    /// An agent's sample is ready.
    SampleDone(u64),
}

/// The highlight rules, and which of them notify.
#[derive(Debug, Default)]
pub struct Rules {
    pub list: Vec<Rule>,
    /// Bumped as they change: each subject works out its rules again.
    pub gen: u64,
    pub notify_mask: u64,
}

impl Rules {
    pub fn set(&mut self, list: Vec<Rule>) {
        self.notify_mask = list.iter().enumerate().filter(|(_, r)| r.notify).fold(0, |m, (i, _)| m | 1 << i);
        self.list = list;
        self.gen += 1;
    }

    /// The rules whose subject patterns `subject` matches, a bit each.
    pub fn mask_for(&self, subject: &str) -> u64 {
        self.list.iter().enumerate().filter(|(_, r)| r.filter.subject_ok(subject)).fold(0, |m, (i, _)| m | 1 << i)
    }
}

/// Rules checked as messages come, within a budget, and their
/// notifications held to one a rule every [`ALERT_EVERY_US`].
#[derive(Debug, Default)]
pub struct Alerts {
    /// Rule checks left for this chunk.
    pub budget: u32,
    /// Messages a rule's subject matched but that weren't checked: over
    /// budget.
    pub unchecked: u64,
    last_sent: Vec<u64>,
    held: Vec<Option<(u16, String, String, u32)>>,
}

/// Each rule notifies at most this often.
pub const ALERT_EVERY_US: u64 = 30_000_000;

/// Rule checks a chunk may spend.
pub const CHECKS_PER_CHUNK: u32 = 2_000;

impl Alerts {
    fn hit(&mut self, rule: usize, conn: u16, subject: &str, payload: &[u8]) {
        if self.held.len() <= rule {
            self.held.resize(rule + 1, None);
            self.last_sent.resize(rule + 1, 0);
        }
        let count = self.held[rule].as_ref().map_or(0, |h| h.3) + 1;
        let preview = crate::payload::preview(payload, 120).0;
        self.held[rule] = Some((conn, subject.to_string(), preview, count));
    }

    fn due(&mut self, now: u64) -> Vec<Event> {
        let mut out = Vec::new();
        for (rule, held) in self.held.iter_mut().enumerate() {
            if held.is_some() && now.saturating_sub(self.last_sent[rule]) >= ALERT_EVERY_US {
                let (conn, subject, preview, count) = held.take().expect("held");
                self.last_sent[rule] = now;
                out.push(Event::Alert { rule, conn, subject, preview, count });
            }
        }
        out
    }
}

/// Watches its own load, and sheds storage when it's behind.
///
/// Two signs say it's falling behind: the share of time spent on what
/// came (`busy`), and how late its own timer comes (`lag`) -- events are
/// taken in order, so a timer behind a queue of data comes late by as much
/// as the queue. Either too high and the store keeps half as many of a
/// busy subject's messages; both low for three seconds and it keeps twice
/// as many again, back to all of them. Counting never stops.
#[derive(Debug, Clone)]
pub struct Load {
    pub sampling: u32,
    busy_us: u64,
    window_start: u64,
    pub busy: f64,
    pub lag_ms: u64,
    calm_since: Option<u64>,
    last_raise: u64,
    /// The longest a chunk took since the last tick, in the last ten
    /// seconds, and ever.
    pub chunk_max_us: u64,
    pub chunk_max_recent_us: u64,
    recent_since: u64,
    pub chunk_max_ever_us: u64,
}

/// Past this, storage stops: messages are only counted.
pub const COUNT_ONLY: u32 = 1 << 16;

impl Default for Load {
    fn default() -> Load {
        Load { sampling: 1, busy_us: 0, window_start: 0, busy: 0.0, lag_ms: 0, calm_since: None, last_raise: 0, chunk_max_us: 0, chunk_max_recent_us: 0, recent_since: 0, chunk_max_ever_us: 0 }
    }
}

impl Load {
    fn raise(&mut self, now: u64) {
        if now.saturating_sub(self.last_raise) >= 200_000 && self.sampling < COUNT_ONLY {
            self.sampling = (self.sampling * 2).min(COUNT_ONLY);
            self.last_raise = now;
        }
        self.calm_since = None;
    }

    /// A chunk took `spent_us`.
    pub fn on_chunk(&mut self, now: u64, spent_us: u64) {
        self.busy_us += spent_us;
        self.chunk_max_us = self.chunk_max_us.max(spent_us);
        self.chunk_max_recent_us = self.chunk_max_recent_us.max(spent_us);
        self.chunk_max_ever_us = self.chunk_max_ever_us.max(spent_us);
        let window = now.saturating_sub(self.window_start);
        // Well over half the time since the last tick, already: no need to
        // wait for it.
        if window >= 30_000 && self.busy_us * 10 > window * 7 {
            self.raise(now);
        }
    }

    /// The timer, `interval_us` after the last; `late_us` late.
    pub fn on_tick(&mut self, now: u64, late_us: u64) {
        let window = now.saturating_sub(self.window_start).max(1);
        self.busy = (self.busy_us as f64 / window as f64).min(1.0);
        self.lag_ms = late_us / 1000;
        self.busy_us = 0;
        self.window_start = now;
        self.chunk_max_us = 0;
        if now.saturating_sub(self.recent_since) >= 10_000_000 {
            self.recent_since = now;
            self.chunk_max_recent_us = 0;
        }
        if self.busy > 0.5 || self.lag_ms > 250 {
            self.raise(now);
        } else if self.busy < 0.2 && self.lag_ms < 50 {
            let since = *self.calm_since.get_or_insert(now);
            if now.saturating_sub(since) >= 3_000_000 && self.sampling > 1 {
                self.sampling /= 2;
                self.calm_since = Some(now);
            }
        } else {
            self.calm_since = None;
        }
    }
}

/// An agent's sample being gathered.
#[derive(Debug, Clone)]
pub struct Sample {
    pub conn: u16,
    pub sid: u64,
    pub want: usize,
    pub got: Vec<Owned>,
    pub deadline_us: u64,
}

/// A KV change seen by a watch.
#[derive(Debug, Clone)]
pub struct WatchEvent {
    pub conn: u16,
    pub bucket: String,
    pub msg: Owned,
}

enum Ctl {
    Info(String),
    Ping,
    Pong,
    Err(String),
}

pub struct Core {
    pub conns: Vec<Conn>,
    pub store: Store,
    pub rules: Rules,
    pub alerts: Alerts,
    pub load: Load,
    pub replies: Vec<Reply>,
    pub samples: FxHashMap<u64, Sample>,
    pub watch: VecDeque<WatchEvent>,
    pub events: Vec<Event>,
    /// Something changed that the page shows.
    pub dirty: bool,
    pub nuid: Nuid,
    pub client_name: String,
    /// Chunks read.
    pub chunks: u64,
    /// The next connection's id: never one a server had before, whose
    /// subjects the store may still have.
    next_conn: u16,
}

/// KV changes kept from watches.
const WATCH_KEEP: usize = 500;

impl Core {
    pub fn new(limits: Limits, now_us: u64) -> Core {
        Core {
            conns: Vec::new(),
            store: Store::new(limits, now_us / 1000),
            rules: Rules::default(),
            alerts: Alerts::default(),
            load: Load { window_start: now_us, ..Load::default() },
            replies: Vec::new(),
            samples: FxHashMap::default(),
            watch: VecDeque::new(),
            events: Vec::new(),
            dirty: true,
            nuid: Nuid::new(now_us ^ 0x5eed),
            client_name: "fenix".into(),
            chunks: 0,
            next_conn: 0,
        }
    }

    /// The servers wanted: new ones added, gone ones closed, the rest kept
    /// as they are.
    pub fn set_servers(&mut self, net: &mut dyn Net, servers: Vec<Addr>, patterns: &[String]) {
        let mut kept = Vec::new();
        for c in std::mem::take(&mut self.conns) {
            if servers.iter().any(|a| a.key() == c.addr.key() && a.tls == c.addr.tls) {
                kept.push(c);
            } else {
                let mut c = c;
                c.disconnect(net);
            }
        }
        for a in servers {
            if !kept.iter().any(|c| c.addr.key() == a.key() && c.addr.tls == a.tls) {
                let id = self.next_conn;
                self.next_conn += 1;
                let inbox = format!("_INBOX.{}", self.nuid.next_id());
                kept.push(Conn::new(id, a, patterns.to_vec(), &inbox, &self.client_name));
            }
        }
        // Ids stay as they were: the store's subjects are by connection id.
        self.conns = kept;
        self.dirty = true;
    }

    pub fn conn_index(&self, id: u16) -> Option<usize> {
        self.conns.iter().position(|c| c.id == id)
    }

    fn by_socket(&self, socket: u64) -> Option<usize> {
        self.conns.iter().position(|c| c.socket == Some(socket))
    }

    pub fn connect(&mut self, net: &mut dyn Net, i: usize, auth: Auth, now: u64) {
        let c = &mut self.conns[i];
        c.auth = auth;
        c.attempts = 0;
        c.auth_failed = false;
        c.connect(net, now);
        self.dirty = true;
    }

    pub fn disconnect(&mut self, net: &mut dyn Net, i: usize) {
        let gone = self.conns[i].disconnect(net);
        self.give_up(self.conns[i].id, gone, "disconnected");
        self.events.push(Event::Down(self.conns[i].id, None));
        self.dirty = true;
    }

    fn give_up(&mut self, conn: u16, gone: Vec<Pending>, why: &str) {
        for what in gone {
            self.replies.push(Reply { conn, what, result: Err(why.to_string()), latency_us: 0 });
        }
    }

    pub fn on_connected(&mut self, net: &mut dyn Net, socket: u64, result: Result<(), String>, now: u64) {
        let Some(i) = self.by_socket(socket) else { return };
        match result {
            Ok(()) => self.conns[i].connected(net, now),
            Err(e) => self.on_closed(net, socket, Some(e), now),
        }
        self.dirty = true;
    }

    pub fn on_closed(&mut self, _net: &mut dyn Net, socket: u64, why: Option<String>, now: u64) {
        let Some(i) = self.by_socket(socket) else { return };
        let was_up = self.conns[i].phase == Phase::Up;
        let why = why.or_else(|| Some("the server closed the connection".into()));
        let gone = self.conns[i].dropped(now, why.clone());
        let c = &self.conns[i];
        let id = c.id;
        if c.auth_failed {
            let e = c.error.clone().unwrap_or_default();
            self.events.push(Event::AuthFailed(id, e));
        } else if was_up {
            self.events.push(Event::Down(id, c.error.clone()));
        }
        self.give_up(id, gone, "the connection dropped");
        self.dirty = true;
    }

    /// What came on `socket`: every message counted, the live ones stored,
    /// replies and samples set aside.
    pub fn on_data(&mut self, net: &mut dyn Net, socket: u64, data: &[u8], now: u64) {
        let Some(i) = self.by_socket(socket) else { return };
        let Core { conns, store, rules, alerts, replies, samples, watch, .. } = self;
        let conn = &mut conns[i];
        let id = conn.id;
        conn.bytes_in += data.len() as u64;
        alerts.budget = CHECKS_PER_CHUNK;
        let mut ctl: Vec<Ctl> = Vec::new();
        let mut done: Vec<u64> = Vec::new();
        let now_ms = now / 1000;
        let Conn { parser, routes, inbox, msgs_in, .. } = conn;
        let result = parser.feed(data, |op| match op {
            Op::Msg(m) => {
                *msgs_in += 1;
                match routes.get(&m.sid) {
                    // Its own replies come on its inbox; a `>` would see
                    // them twice, each a subject of its own.
                    Some(Route::Live) if !inbox.owns(m.subject) => live(store, rules, alerts, id, &m, now_ms),
                    Some(Route::Live) => {}
                    Some(Route::Inbox) => {
                        if let Some(what) = inbox.take(m.subject) {
                            let latency_us = now.saturating_sub(what.started_us());
                            replies.push(Reply { conn: id, what, result: Ok(Owned::of(&m, now)), latency_us });
                        }
                    }
                    Some(Route::Sample(call)) => {
                        if let Some(s) = samples.get_mut(call) {
                            if s.got.len() < s.want {
                                s.got.push(Owned::of(&m, now));
                                if s.got.len() == s.want {
                                    done.push(*call);
                                }
                            }
                        }
                    }
                    Some(Route::Watch(bucket)) => {
                        if watch.len() >= WATCH_KEEP {
                            watch.pop_front();
                        }
                        let mut msg = Owned::of(&m, now);
                        msg.payload.truncate(64 * 1024);
                        watch.push_back(WatchEvent { conn: id, bucket: bucket.clone(), msg });
                    }
                    None => {}
                }
            }
            Op::Info(s) => ctl.push(Ctl::Info(s.to_string())),
            Op::Ping => ctl.push(Ctl::Ping),
            Op::Pong => ctl.push(Ctl::Pong),
            Op::Ok => {}
            Op::Err(e) => ctl.push(Ctl::Err(e.to_string())),
        });
        self.chunks += 1;
        for c in ctl {
            let conn = &mut self.conns[i];
            match c {
                Ctl::Info(json) => match ServerInfo::parse(&json) {
                    Ok(info) => conn.on_info(net, info, now),
                    Err(e) => conn.error = Some(e),
                },
                Ctl::Ping => conn.on_ping(net),
                Ctl::Pong => {
                    if conn.on_pong(now) {
                        self.events.push(Event::Up(id));
                    }
                }
                Ctl::Err(e) => {
                    let permission = e.to_ascii_lowercase().contains("permissions violation");
                    conn.on_err(&e);
                    if permission {
                        self.events.push(Event::Warning(id, e));
                    }
                }
            }
        }
        for call in done {
            self.finish_sample(net, call);
        }
        if let Err(e) = result {
            let conn = &mut self.conns[i];
            conn.error = Some(format!("protocol: {e}"));
            if let Some(s) = conn.socket {
                net.close(s);
                self.on_closed(net, s, Some(format!("protocol: {e}")), now);
            }
        }
        self.dirty = true;
    }

    fn finish_sample(&mut self, net: &mut dyn Net, call: u64) {
        if let Some(s) = self.samples.get(&call) {
            if let Some(i) = self.conn_index(s.conn) {
                let sid = s.sid;
                self.conns[i].unsubscribe_route(net, sid);
            }
            self.events.push(Event::SampleDone(call));
        }
    }

    /// The timer: PINGs, redials, requests and samples given up on, the
    /// load looked at, the held notifications sent.
    pub fn tick(&mut self, net: &mut dyn Net, now: u64, late_us: u64, auth: &mut dyn FnMut(&Addr) -> Auth) {
        self.load.on_tick(now, late_us);
        if self.store.sampling != self.load.sampling {
            self.store.sampling = self.load.sampling;
            self.dirty = true;
        }
        for i in 0..self.conns.len() {
            if self.conns[i].tick(net, now) {
                let a = auth(&self.conns[i].addr);
                self.conns[i].auth = a;
                self.conns[i].connect(net, now);
                self.dirty = true;
            }
            if self.conns[i].socket.is_none() && self.conns[i].phase == Phase::Up {
                // Closed as stale.
                let gone = self.conns[i].dropped(now, None);
                let id = self.conns[i].id;
                self.give_up(id, gone, "the connection dropped");
                self.events.push(Event::Down(id, self.conns[i].error.clone()));
            }
            let id = self.conns[i].id;
            for what in self.conns[i].inbox.expired(now) {
                let latency_us = now.saturating_sub(what.started_us());
                self.replies.push(Reply { conn: id, what, result: Err("no reply in time".into()), latency_us });
            }
        }
        let late: Vec<u64> = self.samples.iter().filter(|(_, s)| s.deadline_us <= now).map(|(c, _)| *c).collect();
        for call in late {
            self.finish_sample(net, call);
        }
        let alerts = self.alerts.due(now);
        self.events.extend(alerts);
    }

    /// A chunk's time, for the load.
    pub fn spent(&mut self, now: u64, spent_us: u64) {
        self.load.on_chunk(now, spent_us);
        if self.store.sampling != self.load.sampling {
            self.store.sampling = self.load.sampling;
        }
    }

    /// Starts gathering `want` messages on `subject` for an agent.
    pub fn sample(&mut self, net: &mut dyn Net, conn: usize, subject: &str, call: u64, want: usize, deadline_us: u64) -> Result<(), String> {
        let id = self.conns[conn].id;
        let sid = self.conns[conn].subscribe_route(net, subject, Route::Sample(call), Some(want as u64))?;
        self.samples.insert(call, Sample { conn: id, sid, want, got: Vec::new(), deadline_us });
        Ok(())
    }

    /// Watches a KV bucket's changes; or stops.
    pub fn watch_bucket(&mut self, net: &mut dyn Net, conn: usize, bucket: &str, on: bool) -> Result<(), String> {
        let c = &mut self.conns[conn];
        let existing: Vec<u64> = c.routes.iter().filter(|(_, r)| **r == Route::Watch(bucket.to_string())).map(|(s, _)| *s).collect();
        if on {
            if existing.is_empty() {
                c.subscribe_route(net, &fenix_nats::kv::all(bucket), Route::Watch(bucket.to_string()), None)?;
            }
        } else {
            for sid in existing {
                c.unsubscribe_route(net, sid);
            }
        }
        Ok(())
    }
}

/// A live message: counted, stored if sampling keeps it, and the rules that
/// notify checked on it if there's budget left.
#[inline]
fn live(store: &mut Store, rules: &Rules, alerts: &mut Alerts, conn: u16, m: &Msg<'_>, now_ms: u64) {
    let id = store.subject(conn, m.subject, now_ms);
    if !store.count(id, m.size(), now_ms) {
        return;
    }
    let header_count = m.headers.map_or(0, fenix_nats::headers::count);
    store.store(id, m.reply, m.headers, header_count, m.payload, now_ms);
    if rules.notify_mask == 0 {
        return;
    }
    let s = &mut store.subjects[id as usize];
    if s.rule_gen != rules.gen {
        s.rule_mask = rules.mask_for(&s.name);
        s.rule_gen = rules.gen;
    }
    let mask = s.rule_mask & rules.notify_mask;
    if mask == 0 {
        return;
    }
    let Some(view) = store.newest(id, 0) else { return };
    for (i, r) in rules.list.iter().enumerate() {
        if mask & 1 << i == 0 {
            continue;
        }
        if alerts.budget == 0 {
            alerts.unchecked += 1;
            return;
        }
        alerts.budget -= 1;
        if r.filter.message_ok(&view, r.filter.words_in_subject(view.subject)) {
            alerts.hit(i, conn, view.subject, view.payload);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::conn::Addr;

    /// The network, recorded.
    #[derive(Default)]
    pub struct FakeNet {
        pub next: u64,
        pub written: Vec<(u64, Vec<u8>)>,
        pub closed: Vec<u64>,
        pub tls: Vec<u64>,
    }

    impl Net for FakeNet {
        fn connect(&mut self, _host: &str, _port: u16, _tls: bool) -> Result<u64, String> {
            self.next += 1;
            Ok(self.next)
        }
        fn start_tls(&mut self, socket: u64, _host: &str) -> Result<(), String> {
            self.tls.push(socket);
            Ok(())
        }
        fn write(&mut self, socket: u64, data: &[u8]) -> Result<(), String> {
            self.written.push((socket, data.to_vec()));
            Ok(())
        }
        fn close(&mut self, socket: u64) {
            self.closed.push(socket);
        }
    }

    impl FakeNet {
        pub fn text(&self) -> String {
            self.written.iter().map(|(_, d)| String::from_utf8_lossy(d).into_owned()).collect()
        }
    }

    /// A core with one connection, signed in.
    pub fn up(patterns: &[&str]) -> (Core, FakeNet, u64) {
        let mut net = FakeNet::default();
        let mut core = Core::new(Limits::default(), 1_000_000_000);
        core.set_servers(&mut net, vec![Addr::parse("nats://localhost:4222").unwrap()], &patterns.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        core.connect(&mut net, 0, Auth::None, 1_000_000_000);
        let socket = core.conns[0].socket.unwrap();
        core.on_connected(&mut net, socket, Ok(()), 1_000_000_000);
        core.on_data(&mut net, socket, b"INFO {\"server_id\":\"S\",\"version\":\"2.10.0\",\"headers\":true,\"max_payload\":1048576,\"jetstream\":true}\r\n", 1_000_000_000);
        core.on_data(&mut net, socket, b"PONG\r\n", 1_000_001_000);
        (core, net, socket)
    }

    #[test]
    fn it_signs_in_and_subscribes() {
        let (core, net, _) = up(&[">"]);
        let text = net.text();
        assert!(text.starts_with("CONNECT {"), "{text}");
        assert!(text.contains("\r\nPING\r\nSUB > 1\r\n"), "{text}");
        assert_eq!(core.conns[0].phase, Phase::Up);
        assert_eq!(core.conns[0].rtt_us, Some(1000));
        assert_eq!(core.events, [Event::Up(0)]);
        assert_eq!(core.conns[0].info.as_ref().unwrap().version, "2.10.0");
    }

    #[test]
    fn messages_go_to_the_store_and_pings_are_answered() {
        let (mut core, mut net, s) = up(&[">"]);
        core.on_data(&mut net, s, b"MSG a.b 1 2\r\nhi\r\nPING\r\nHMSG a.c 1 _INBOX.x 18 21\r\nNATS/1.0\r\nA: 1\r\n\r\nyo!\r\n", 1_000_002_000);
        assert!(net.text().ends_with("PONG\r\n"));
        let b = core.store.find(0, "a.b").unwrap();
        assert_eq!(core.store.newest(b, 0).unwrap().payload, b"hi");
        let c = core.store.find(0, "a.c").unwrap();
        let v = core.store.newest(c, 0).unwrap();
        assert_eq!((v.reply, v.entry.header_count, v.payload), (Some("_INBOX.x"), 1, b"yo!".as_slice()));
    }

    #[test]
    fn a_request_finds_its_reply_or_times_out() {
        let (mut core, mut net, s) = up(&[]);
        let hm = fenix_nats::headers::HeaderMap::new();
        core.conns[0].request(&mut net, "svc.time", &hm, b"now?", Pending::User { subject: "svc.time".into(), started_us: 1_000_002_000 }, 1_005_000_000).unwrap();
        let text = net.text();
        let inbox = core.conns[0].inbox.prefix.clone();
        assert!(text.contains(&format!("SUB {inbox}.* ")), "{text}");
        assert!(text.contains(&format!("PUB svc.time {inbox}.1 4\r\nnow?\r\n")), "{text}");
        let sid = core.conns[0].routes.iter().find(|(_, r)| **r == Route::Inbox).map(|(s, _)| *s).unwrap();
        core.on_data(&mut net, s, format!("MSG {inbox}.1 {sid} 5\r\n12:00\r\n").as_bytes(), 1_000_003_500);
        let r = core.replies.pop().unwrap();
        assert_eq!((r.result.unwrap().payload, r.latency_us), (b"12:00".to_vec(), 1500));
        core.conns[0].request(&mut net, "svc.none", &hm, b"", Pending::User { subject: "svc.none".into(), started_us: 1_000_004_000 }, 1_005_000_000).unwrap();
        core.tick(&mut net, 1_005_000_000, 0, &mut |_| Auth::None);
        assert_eq!(core.replies.pop().unwrap().result.unwrap_err(), "no reply in time");
    }

    #[test]
    fn its_own_replies_are_not_live_subjects() {
        let (mut core, mut net, s) = up(&[">"]);
        let hm = fenix_nats::headers::HeaderMap::new();
        core.conns[0].request(&mut net, "svc", &hm, b"", Pending::User { subject: "svc".into(), started_us: 0 }, u64::MAX).unwrap();
        let inbox = core.conns[0].inbox.prefix.clone();
        let sid = core.conns[0].routes.iter().find(|(_, r)| **r == Route::Inbox).map(|(s, _)| *s).unwrap();
        // The server sends it on both subscriptions: `>` and the inbox's.
        core.on_data(&mut net, s, format!("MSG {inbox}.1 1 2\r\nok\r\nMSG {inbox}.1 {sid} 2\r\nok\r\nMSG _INBOX.other.1 1 2\r\nok\r\n").as_bytes(), 1_000_002_000);
        assert_eq!(core.replies.len(), 1);
        assert!(core.store.find(0, &format!("{inbox}.1")).is_none());
        assert!(core.store.find(0, "_INBOX.other.1").is_some(), "others' replies are traffic like any");
    }

    #[test]
    fn a_drop_redials_and_subscribes_again() {
        let (mut core, mut net, s) = up(&["orders.>"]);
        core.on_closed(&mut net, s, Some("reset".into()), 2_000_000_000);
        assert!(matches!(core.conns[0].phase, Phase::Waiting(_)));
        assert!(core.events.contains(&Event::Down(0, Some("reset".into()))));
        net.written.clear();
        core.tick(&mut net, 2_100_000_000, 0, &mut |_| Auth::None);
        let s2 = core.conns[0].socket.unwrap();
        assert_ne!(s, s2);
        core.on_connected(&mut net, s2, Ok(()), 2_100_000_000);
        core.on_data(&mut net, s2, b"INFO {}\r\n", 2_100_000_000);
        assert!(net.text().contains("SUB orders.> "), "{}", net.text());
    }

    #[test]
    fn tls_is_started_when_the_server_wants_it() {
        let mut net = FakeNet::default();
        let mut core = Core::new(Limits::default(), 0);
        core.set_servers(&mut net, vec![Addr::parse("localhost").unwrap()], &[">".into()]);
        core.connect(&mut net, 0, Auth::Token("t0k".into()), 0);
        let s = core.conns[0].socket.unwrap();
        core.on_connected(&mut net, s, Ok(()), 0);
        core.on_data(&mut net, s, b"INFO {\"tls_required\":true}\r\n", 0);
        assert_eq!(net.tls, [s]);
        assert!(net.written.is_empty(), "nothing in plain text");
        core.on_connected(&mut net, s, Ok(()), 0);
        let text = net.text();
        assert!(text.contains("\"auth_token\":\"t0k\"") && text.contains("\"tls_required\":true"), "{text}");
    }

    #[test]
    fn a_refused_sign_in_is_said() {
        let (mut core, mut net, s) = up(&[">"]);
        core.on_data(&mut net, s, b"-ERR 'Authorization Violation'\r\n", 2_000_000_000);
        core.on_closed(&mut net, s, None, 2_000_000_000);
        assert!(core.events.iter().any(|e| matches!(e, Event::AuthFailed(0, _))));
        let Phase::Waiting(at) = core.conns[0].phase else { panic!() };
        assert_eq!(at, 2_030_000_000);
        core.on_data(&mut net, s, b"-ERR 'Permissions Violation for Subscription to \"x\"'\r\n", 0);
    }

    #[test]
    fn a_rule_that_notifies_is_held_to_one_a_while() {
        let (mut core, mut net, s) = up(&[">"]);
        core.rules.set(crate::filter::parse_rules("!bad orders.* re:fail").unwrap());
        for i in 0..5 {
            core.on_data(&mut net, s, format!("MSG orders.{i} 1 6\r\nfailed\r\nMSG orders.ok 1 2\r\nok\r\nMSG other 1 6\r\nfailed\r\n").as_bytes(), 1_000_002_000);
        }
        core.tick(&mut net, 1_100_000_000, 0, &mut |_| Auth::None);
        let alerts: Vec<&Event> = core.events.iter().filter(|e| matches!(e, Event::Alert { .. })).collect();
        assert_eq!(alerts.len(), 1);
        assert!(matches!(alerts[0], Event::Alert { count: 5, subject, .. } if subject == "orders.4"));
        core.events.clear();
        core.on_data(&mut net, s, b"MSG orders.x 1 4\r\nfail\r\n", 1_100_000_000);
        core.tick(&mut net, 1_110_000_000, 0, &mut |_| Auth::None);
        assert!(core.events.iter().all(|e| !matches!(e, Event::Alert { .. })), "not again within 30 s");
        core.tick(&mut net, 1_140_000_000, 0, &mut |_| Auth::None);
        assert!(core.events.iter().any(|e| matches!(e, Event::Alert { count: 1, .. })));
    }

    #[test]
    fn under_load_it_samples_and_recovers() {
        let mut load = Load::default();
        let mut now = 0;
        for _ in 0..10 {
            now += 100_000;
            load.on_chunk(now, 90_000);
            load.on_tick(now, 0);
        }
        assert!(load.sampling >= 16, "{}", load.sampling);
        let high = load.sampling;
        for _ in 0..100 {
            now += 100_000;
            load.on_chunk(now, 1_000);
            load.on_tick(now, 0);
        }
        assert!(load.sampling < high);
        for _ in 0..1000 {
            now += 100_000;
            load.on_tick(now, 0);
        }
        assert_eq!(load.sampling, 1);
        load.on_tick(now + 100_000, 400_000);
        assert_eq!(load.sampling, 2, "a late timer is load too");
    }

    #[test]
    fn a_sample_gathers_then_unsubscribes() {
        let (mut core, mut net, s) = up(&[]);
        core.sample(&mut net, 0, "orders.*", 77, 2, 2_000_000_000).unwrap();
        let text = net.text();
        assert!(text.contains("SUB orders.* 1\r\nUNSUB 1 2\r\n"), "{text}");
        core.on_data(&mut net, s, b"MSG orders.a 1 1\r\na\r\nMSG orders.b 1 1\r\nb\r\nMSG orders.c 1 1\r\nc\r\n", 1_000_002_000);
        assert!(core.events.contains(&Event::SampleDone(77)));
        assert_eq!(core.samples[&77].got.len(), 2);
    }
}
