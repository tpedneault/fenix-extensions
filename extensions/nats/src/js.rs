//! JetStream's page: streams (their configuration, subjects and counts,
//! their messages read by sequence or time, purged, one deleted), their
//! consumers (pending, waiting for an ack, redelivered), and key-value
//! buckets (keys, values and their history, watched, put and deleted).
//!
//! Everything is asked of JetStream's API as requests on the connection's
//! inbox ([`crate::core::Pending::Js`]); answers come back to `on_reply`.

use fenix_extension_api::grid::{self, Grid};
use fenix_extension_api::{Key, Page, Role};
use fenix_nats::headers::HeaderMap;
use fenix_nats::jetstream::{self, Api, ConsumerInfo, ConsumerList, MsgDeleteRequest, MsgGetRequest, PurgeRequest, PurgeResponse, StoredMsg, StreamInfo, StreamInfoRequest, StreamList, Success};
use fenix_nats::kv::{self, Entry, Operation};

use crate::conn::Net;
use crate::core::{Core, Owned, Pending};
use crate::payload;

/// What a JetStream request was for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Want {
    Account,
    Streams { offset: u64 },
    Stream(String),
    Consumers(String),
    /// A page of a stream's messages, one at a time: this many more.
    Browse { stream: String, left: usize },
    Purge(String),
    Delete { stream: String, seq: u64 },
    Keys(String),
    Value { bucket: String, key: String },
    History { bucket: String, key: String, left: usize },
    Put { bucket: String, key: String },
    KvDelete { bucket: String, key: String },
}

/// A page of messages is this many.
pub const PAGE: usize = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Streams,
    Kv,
}

/// What a key asks of the extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsAction {
    None,
    AskSeq(String),
    AskTime(String),
    ConfirmPurge(String),
    ConfirmDelete(String, u64),
    AskPut(String),
    ConfirmKvDelete(String, String),
    Copy(String),
    Open(String, Option<&'static str>),
    Live,
}

#[derive(Debug, Clone, PartialEq)]
enum Row {
    Stream(usize),
    Info(String, Role),
    Consumer(String, usize),
    Msg(usize),
    MsgLine(usize, usize),
    Bucket(usize),
    Key(usize),
    ValueLine(usize),
    History(usize),
    Watch(usize),
}

#[derive(Debug, Default)]
pub struct JsView {
    pub conn: u16,
    pub tab: Option<Tab>,
    pub account: Option<jetstream::AccountInfo>,
    pub streams: Vec<StreamInfo>,
    pub loaded: bool,
    pub error: Option<String>,
    cursor: usize,
    top: usize,
    pub open_stream: Option<String>,
    consumers: Vec<ConsumerInfo>,
    /// The stream being read, and what's been read of it.
    browse: Vec<StoredMsg>,
    browse_next: u64,
    browsing: bool,
    open_msg: Option<u64>,
    pub open_bucket: Option<String>,
    keys: Vec<(String, u64)>,
    value: Option<Entry>,
    history: Vec<Entry>,
    pub watching: Option<String>,
    pub note: Option<(String, Role)>,
    rows: Vec<Row>,
    /// Seconds east of UTC.
    pub offset_s: i64,
}

fn api(core: &Core, conn: usize) -> Api {
    Api::domain(core.conns[conn].info.as_ref().and_then(|i| i.domain.as_deref()))
}

/// Asks JetStream: `subject`, with `body`, for `want`.
pub fn ask(core: &mut Core, net: &mut dyn Net, conn: usize, subject: &str, body: &[u8], want: Want, now: u64) -> Result<(), String> {
    let pending = Pending::Js { want, started_us: now };
    core.conns[conn].request(net, subject, &HeaderMap::new(), body, pending, now + 5_000_000)
}

/// The JSON of an answer, or why there's none.
fn answer(result: &Result<Owned, String>) -> Result<&[u8], String> {
    match result {
        Ok(m) if m.status() == Some(503) => Err("JetStream isn't on for this account (no responders)".into()),
        Ok(m) => Ok(&m.payload),
        Err(e) => Err(e.clone()),
    }
}

impl JsView {
    pub fn tab(&self) -> Tab {
        self.tab.unwrap_or(Tab::Streams)
    }

    /// Loads what it shows, from the start.
    pub fn reload(&mut self, core: &mut Core, net: &mut dyn Net, now: u64) {
        let Some(i) = core.conn_index(self.conn) else {
            self.error = Some("no connection".into());
            return;
        };
        if core.conns[i].info.as_ref().is_some_and(|info| !info.jetstream) {
            self.error = Some("this server has JetStream off".into());
        }
        let a = api(core, i);
        self.streams.clear();
        self.loaded = false;
        let r = ask(core, net, i, &a.stream_list(), &jetstream::body(&jetstream::ListRequest::default()), Want::Streams { offset: 0 }, now).and_then(|_| ask(core, net, i, &a.account_info(), b"", Want::Account, now));
        if let Err(e) = r {
            self.error = Some(e);
        }
        if let Some(s) = self.open_stream.clone() {
            self.load_stream(core, net, &s, now);
        }
        if let Some(b) = self.open_bucket.clone() {
            self.load_keys(core, net, &b, now);
        }
    }

    fn with_conn<T>(&mut self, core: &mut Core, f: impl FnOnce(&mut Core, usize, Api) -> Result<T, String>) -> Option<T> {
        let i = core.conn_index(self.conn)?;
        let a = api(core, i);
        match f(core, i, a) {
            Ok(t) => Some(t),
            Err(e) => {
                self.note = Some((e, Role::Bad));
                None
            }
        }
    }

    fn load_stream(&mut self, core: &mut Core, net: &mut dyn Net, stream: &str, now: u64) {
        self.consumers.clear();
        self.with_conn(core, |core, i, a| {
            ask(core, net, i, &a.stream_info(stream), b"", Want::Stream(stream.into()), now)?;
            ask(core, net, i, &a.consumer_list(stream), &jetstream::body(&jetstream::ListRequest::default()), Want::Consumers(stream.into()), now)
        });
    }

    /// Reads a page of `stream` from sequence `seq`.
    pub fn browse_from(&mut self, core: &mut Core, net: &mut dyn Net, stream: &str, seq: u64, now: u64) {
        self.browse.clear();
        self.open_msg = None;
        self.browse_next = seq;
        self.browsing = true;
        self.next_message(core, net, stream, seq, PAGE, now);
    }

    fn next_message(&mut self, core: &mut Core, net: &mut dyn Net, stream: &str, seq: u64, left: usize, now: u64) {
        self.with_conn(core, |core, i, a| ask(core, net, i, &a.msg_get(stream), &jetstream::body(&MsgGetRequest::next(">", seq.max(1))), Want::Browse { stream: stream.into(), left }, now));
    }

    /// Reads a page of `stream` from the first message at or after a time
    /// (RFC 3339).
    pub fn browse_time(&mut self, core: &mut Core, net: &mut dyn Net, stream: &str, time: &str, now: u64) {
        self.browse.clear();
        self.open_msg = None;
        self.browsing = true;
        self.with_conn(core, |core, i, a| ask(core, net, i, &a.msg_get(stream), &jetstream::body(&MsgGetRequest::at_time(time)), Want::Browse { stream: stream.into(), left: PAGE }, now));
    }

    pub fn purge(&mut self, core: &mut Core, net: &mut dyn Net, stream: &str, now: u64) {
        self.with_conn(core, |core, i, a| ask(core, net, i, &a.stream_purge(stream), &jetstream::body(&PurgeRequest::default()), Want::Purge(stream.into()), now));
    }

    pub fn delete(&mut self, core: &mut Core, net: &mut dyn Net, stream: &str, seq: u64, now: u64) {
        self.with_conn(core, |core, i, a| ask(core, net, i, &a.msg_delete(stream), &jetstream::body(&MsgDeleteRequest { seq, no_erase: true }), Want::Delete { stream: stream.into(), seq }, now));
    }

    fn load_keys(&mut self, core: &mut Core, net: &mut dyn Net, bucket: &str, now: u64) {
        self.keys.clear();
        self.value = None;
        self.history.clear();
        let body = jetstream::body(&StreamInfoRequest { subjects_filter: Some(kv::all(bucket)), offset: None });
        self.with_conn(core, |core, i, a| ask(core, net, i, &a.stream_info(&kv::stream(bucket)), &body, Want::Keys(bucket.into()), now));
    }

    fn load_value(&mut self, core: &mut Core, net: &mut dyn Net, bucket: &str, key: &str, now: u64) {
        self.history.clear();
        let body = jetstream::body(&MsgGetRequest::last(&kv::subject(bucket, key)));
        self.with_conn(core, |core, i, a| ask(core, net, i, &a.msg_get(&kv::stream(bucket)), &body, Want::Value { bucket: bucket.into(), key: key.into() }, now));
    }

    #[allow(clippy::too_many_arguments)]
    fn history_from(&mut self, core: &mut Core, net: &mut dyn Net, bucket: &str, key: &str, seq: u64, left: usize, now: u64) {
        let body = jetstream::body(&MsgGetRequest::next(&kv::subject(bucket, key), seq));
        self.with_conn(core, |core, i, a| ask(core, net, i, &a.msg_get(&kv::stream(bucket)), &body, Want::History { bucket: bucket.into(), key: key.into(), left }, now));
    }

    /// Puts `value` at `key`: a publish to its subject, as a request so
    /// JetStream says it's stored.
    pub fn put(&mut self, core: &mut Core, net: &mut dyn Net, bucket: &str, key: &str, value: &[u8], now: u64) {
        if !kv::valid_key(key) {
            self.note = Some((format!("{key:?} can't be a key: letters, digits, - _ / = and dots"), Role::Bad));
            return;
        }
        self.with_conn(core, |core, i, _| {
            let pending = Pending::Js { want: Want::Put { bucket: bucket.into(), key: key.into() }, started_us: now };
            core.conns[i].request(net, &kv::subject(bucket, key), &HeaderMap::new(), value, pending, now + 5_000_000)
        });
    }

    pub fn kv_delete(&mut self, core: &mut Core, net: &mut dyn Net, bucket: &str, key: &str, now: u64) {
        self.with_conn(core, |core, i, _| {
            let pending = Pending::Js { want: Want::KvDelete { bucket: bucket.into(), key: key.into() }, started_us: now };
            core.conns[i].request(net, &kv::subject(bucket, key), &kv::delete_headers(), b"", pending, now + 5_000_000)
        });
    }

    /// An answer.
    pub fn on_reply(&mut self, core: &mut Core, net: &mut dyn Net, want: Want, result: Result<Owned, String>, now: u64) {
        let body = answer(&result);
        match want {
            Want::Account => {
                if let Ok(b) = body {
                    self.account = jetstream::parse(b).ok();
                }
            }
            Want::Streams { offset } => match body.and_then(jetstream::parse::<StreamList>) {
                Ok(list) => {
                    let got = list.streams.unwrap_or_default();
                    let n = got.len() as u64;
                    self.streams.extend(got);
                    self.streams.sort_by(|a, b| a.config.name.cmp(&b.config.name));
                    self.error = None;
                    if offset + n < list.total && n > 0 {
                        let next = offset + n;
                        self.with_conn(core, |core, i, a| ask(core, net, i, &a.stream_list(), &jetstream::body(&jetstream::ListRequest { offset: next, subject: None }), Want::Streams { offset: next }, now));
                    } else {
                        self.loaded = true;
                    }
                }
                Err(e) => {
                    self.error = Some(e);
                    self.loaded = true;
                }
            },
            Want::Stream(name) => match body.and_then(jetstream::parse::<StreamInfo>) {
                Ok(info) => {
                    if let Some(s) = self.streams.iter_mut().find(|s| s.config.name == name) {
                        *s = info;
                    }
                }
                Err(e) => self.note = Some((e, Role::Bad)),
            },
            Want::Consumers(stream) => match body.and_then(jetstream::parse::<ConsumerList>) {
                Ok(list) if self.open_stream.as_deref() == Some(&stream) => self.consumers = list.consumers.unwrap_or_default(),
                Ok(_) => {}
                Err(e) => self.note = Some((e, Role::Bad)),
            },
            Want::Browse { stream, left } => {
                if self.open_stream.as_deref() != Some(&stream) {
                    return;
                }
                match body.and_then(jetstream::parse_stored) {
                    Ok(m) => {
                        let next = m.seq + 1;
                        self.browse_next = next;
                        self.browse.push(m);
                        if left > 1 {
                            self.next_message(core, net, &stream, next, left - 1, now);
                        } else {
                            self.browsing = false;
                        }
                    }
                    Err(e) => {
                        self.browsing = false;
                        if !e.contains("not found") {
                            self.note = Some((e, Role::Bad));
                        } else if self.browse.is_empty() {
                            self.note = Some(("no messages there".into(), Role::Muted));
                        }
                    }
                }
            }
            Want::Purge(stream) => match body.and_then(jetstream::parse::<PurgeResponse>) {
                Ok(p) => {
                    self.note = Some((format!("{stream}: {} messages purged", p.purged), Role::Good));
                    self.browse.clear();
                    self.load_stream(core, net, &stream, now);
                }
                Err(e) => self.note = Some((e, Role::Bad)),
            },
            Want::Delete { stream, seq } => match body.and_then(jetstream::parse::<Success>) {
                Ok(_) => {
                    self.note = Some((format!("{stream}: #{seq} deleted"), Role::Good));
                    self.browse.retain(|m| m.seq != seq);
                    self.load_stream(core, net, &stream, now);
                }
                Err(e) => self.note = Some((e, Role::Bad)),
            },
            Want::Keys(bucket) => match body.and_then(jetstream::parse::<StreamInfo>) {
                Ok(info) => {
                    if self.open_bucket.as_deref() == Some(&bucket) {
                        let subjects = info.state.subjects.clone().unwrap_or_default();
                        self.keys = subjects.into_iter().filter_map(|(s, n)| kv::key_of(&bucket, &s).map(|k| (k.to_string(), n))).collect();
                    }
                    if let Some(s) = self.streams.iter_mut().find(|s| s.config.name == info.config.name) {
                        *s = info;
                    }
                }
                Err(e) => self.note = Some((e, Role::Bad)),
            },
            Want::Value { bucket, key } => match body.and_then(jetstream::parse_stored) {
                Ok(m) => {
                    let mut entry = Entry::from_stored(&bucket, m);
                    entry.key = key.clone();
                    let first = 1;
                    self.value = Some(entry);
                    self.history.clear();
                    self.history_from(core, net, &bucket, &key, first, 64, now);
                }
                Err(e) => self.note = Some((e, Role::Bad)),
            },
            Want::History { bucket, key, left } => {
                if let Ok(m) = body.and_then(jetstream::parse_stored) {
                    let next = m.seq + 1;
                    self.history.push(Entry::from_stored(&bucket, m));
                    if left > 1 {
                        self.history_from(core, net, &bucket, &key, next, left - 1, now);
                    }
                }
            }
            Want::Put { bucket, key } => match body.and_then(jetstream::parse::<serde_json::Value>) {
                Ok(ack) => {
                    self.note = Some((format!("{key} put, revision {}", ack["seq"]), Role::Good));
                    self.load_keys(core, net, &bucket, now);
                }
                Err(e) => self.note = Some((e, Role::Bad)),
            },
            Want::KvDelete { bucket, key } => match body.and_then(jetstream::parse::<serde_json::Value>) {
                Ok(_) => {
                    self.note = Some((format!("{key} deleted"), Role::Good));
                    self.load_keys(core, net, &bucket, now);
                }
                Err(e) => self.note = Some((e, Role::Bad)),
            },
        }
    }

    fn buckets(&self) -> Vec<usize> {
        (0..self.streams.len()).filter(|i| kv::bucket_of(&self.streams[*i].config.name).is_some()).collect()
    }

    fn plain_streams(&self) -> Vec<usize> {
        (0..self.streams.len()).filter(|i| kv::bucket_of(&self.streams[*i].config.name).is_none()).collect()
    }

    fn build_rows(&self, core: &Core) -> Vec<Row> {
        let mut rows = Vec::new();
        match self.tab() {
            Tab::Streams => {
                for i in self.plain_streams() {
                    rows.push(Row::Stream(i));
                    let s = &self.streams[i];
                    if self.open_stream.as_deref() != Some(s.config.name.as_str()) {
                        continue;
                    }
                    let c = &s.config;
                    let lim = |n: i64| if n < 0 { "∞".to_string() } else { payload::count(n as f64) };
                    let limb = |n: i64| if n < 0 { "∞".to_string() } else { payload::bytes(n as f64) };
                    rows.push(Row::Info(format!("subjects {}", c.subjects.join(", ")), Role::Text));
                    if let Some(d) = c.description.as_ref().filter(|d| !d.is_empty()) {
                        rows.push(Row::Info(d.clone(), Role::Muted));
                    }
                    rows.push(Row::Info(format!("{} retention · {} storage · {} replicas · discard {}", c.retention, c.storage, c.num_replicas, c.discard), Role::Muted));
                    rows.push(Row::Info(format!("limits: {} messages · {} · age {} · {} a subject · message {}", lim(c.max_msgs), limb(c.max_bytes), jetstream::nanos(c.max_age), lim(c.max_msgs_per_subject), limb(c.max_msg_size)), Role::Muted));
                    let st = &s.state;
                    rows.push(Row::Info(format!("#{} – #{} · {} subjects · {} deleted · first {} · last {}", st.first_seq, st.last_seq, st.num_subjects, st.num_deleted, short_time(&st.first_ts), short_time(&st.last_ts)), Role::Muted));
                    if let Some(cl) = &s.cluster {
                        rows.push(Row::Info(format!("cluster {} · leader {}", cl.name.clone().unwrap_or_default(), cl.leader.clone().unwrap_or_default()), Role::Muted));
                    }
                    rows.push(Row::Info(format!("CONSUMERS · {}", self.consumers.len()), Role::Accent));
                    for j in 0..self.consumers.len() {
                        rows.push(Row::Consumer(c.name.clone(), j));
                    }
                    let what = if self.browse.is_empty() && !self.browsing { "b reads them from the first, g from a sequence, t from a time".to_string() } else { format!("{} read{} · n next page", self.browse.len(), if self.browsing { "…" } else { "" }) };
                    rows.push(Row::Info(format!("MESSAGES · {what}"), Role::Accent));
                    for k in 0..self.browse.len() {
                        rows.push(Row::Msg(k));
                        if self.open_msg == Some(self.browse[k].seq) {
                            for l in 0..self.msg_lines(k).len() {
                                rows.push(Row::MsgLine(k, l));
                            }
                        }
                    }
                }
            }
            Tab::Kv => {
                for i in self.buckets() {
                    rows.push(Row::Bucket(i));
                    let name = kv::bucket_of(&self.streams[i].config.name).unwrap_or("");
                    if self.open_bucket.as_deref() != Some(name) {
                        continue;
                    }
                    if self.keys.is_empty() {
                        rows.push(Row::Info("no keys".into(), Role::Muted));
                    }
                    for j in 0..self.keys.len() {
                        rows.push(Row::Key(j));
                        if self.value.as_ref().is_some_and(|v| v.key == self.keys[j].0) {
                            for l in 0..self.value_lines().len() {
                                rows.push(Row::ValueLine(l));
                            }
                            if !self.history.is_empty() {
                                rows.push(Row::Info(format!("HISTORY · {}", self.history.len()), Role::Accent));
                                for h in (0..self.history.len()).rev() {
                                    rows.push(Row::History(h));
                                }
                            }
                        }
                    }
                    if self.watching.as_deref() == Some(name) {
                        let events: Vec<usize> = core.watch.iter().enumerate().filter(|(_, w)| w.bucket == name && w.conn == self.conn).map(|(i, _)| i).collect();
                        rows.push(Row::Info(format!("WATCHING · {} changes (w stops)", events.len()), Role::Accent));
                        for k in events.into_iter().rev().take(200) {
                            rows.push(Row::Watch(k));
                        }
                    }
                }
            }
        }
        rows
    }

    fn msg_lines(&self, k: usize) -> Vec<payload::Line> {
        let m = &self.browse[k];
        let mut lines: Vec<payload::Line> = vec![vec![(format!("{} · #{} · {}", m.subject, m.seq, m.time), Role::Muted)]];
        if let Some(h) = m.headers.as_deref().and_then(|h| fenix_nats::headers::Headers::parse(h).ok()) {
            for (k, v) in h.iter() {
                lines.push(vec![(format!("{k}: "), Role::Syntax("property".into())), (v.to_string(), Role::Text)]);
            }
        }
        lines.extend(payload::detail(&m.payload).0);
        lines
    }

    fn value_lines(&self) -> Vec<payload::Line> {
        let Some(v) = &self.value else { return Vec::new() };
        let mut lines = vec![vec![(format!("revision {} · {} · {}", v.revision, v.operation.name(), short_time(&v.time)), Role::Muted)]];
        if v.operation == Operation::Put {
            lines.extend(payload::detail(&v.value).0);
        }
        lines
    }

    pub fn render(&mut self, core: &Core, width: u32, height: u32) -> Page {
        let width = (width as usize).max(40);
        let height = (height as usize).max(10);
        self.rows = self.build_rows(core);
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
        let mut g = Grid::new();
        let conn = core.conn_index(self.conn).map(|i| &core.conns[i]);
        let x = g.put(0, 1, "JetStream", Role::Title);
        let x = g.put(0, x + 2, &conn.map(|c| c.addr.key()).unwrap_or_default(), Role::Text);
        if let Some(a) = &self.account {
            let text = format!("{} streams · {} consumers · memory {} · storage {}{}", a.streams, a.consumers, payload::bytes(a.memory as f64), payload::bytes(a.storage as f64), a.domain.as_ref().map(|d| format!(" · domain {d}")).unwrap_or_default());
            g.put(0, x + 2, &grid::fit(&text, width.saturating_sub(x + 3)), Role::Muted);
        }
        let mut x = 1;
        for (tab, label) in [(Tab::Streams, format!("Streams {}", self.plain_streams().len())), (Tab::Kv, format!("Key-value {}", self.buckets().len()))] {
            let role = if self.tab() == tab { Role::Accent } else { Role::Muted };
            x = g.put(1, x, &label, role) + 3;
        }
        if let Some(e) = &self.error {
            g.put(1, x, &grid::fit(e, width.saturating_sub(x + 1)), Role::Bad);
        } else if !self.loaded {
            g.put(1, x, "loading…", Role::Muted);
        }
        g.rule(2, 0..width);
        let list = height - 6;
        if self.cursor < self.top {
            self.top = self.cursor;
        } else if self.cursor >= self.top + list {
            self.top = self.cursor + 1 - list;
        }
        for (n, r) in self.rows.iter().enumerate().skip(self.top).take(list) {
            let line = 3 + n - self.top;
            self.draw(&mut g, core, r, line, width);
            if n == self.cursor {
                g.focus(line, 0..width);
            }
        }
        if self.rows.is_empty() && self.loaded {
            let what = if self.tab() == Tab::Streams { "No streams." } else { "No key-value buckets." };
            g.put(3, 2, what, Role::Muted);
        }
        let foot = height - 3;
        g.rule(foot, 0..width);
        if let Some((t, role)) = &self.note {
            g.put(foot + 1, 1, &grid::fit(t, width - 2), role.clone());
        } else {
            let keys: &[(&str, &str)] = match self.tab() {
                Tab::Streams => &[("Tab", "key-value"), ("Enter", "open"), ("b", "read"), ("n", "next"), ("g", "from #"), ("t", "from a time"), ("P", "purge"), ("d", "delete it"), ("y", "copy"), ("r", "reload"), ("L", "live")],
                Tab::Kv => &[("Tab", "streams"), ("Enter", "open"), ("p", "put"), ("d", "delete"), ("w", "watch"), ("y", "copy"), ("r", "reload"), ("L", "live")],
            };
            let mut x = 1;
            for (k, label) in keys {
                if x + k.len() + label.len() + 2 > width {
                    break;
                }
                x = g.put(foot + 1, x, k, Role::Accent) + 1;
                x = g.put(foot + 1, x, label, Role::Muted) + 3;
            }
        }
        Page::canvas(g.finish())
    }

    fn draw(&self, g: &mut Grid, core: &Core, row: &Row, line: usize, width: usize) {
        let fit = |s: &str, x: usize| grid::fit(s, width.saturating_sub(x + 1));
        match row {
            Row::Stream(i) => {
                let s = &self.streams[*i];
                let open = self.open_stream.as_deref() == Some(s.config.name.as_str());
                let x = g.put(line, 1, if open { "▾ " } else { "▸ " }, Role::Muted);
                let x = g.put(line, x, &grid::fit(&s.config.name, 24), Role::Title);
                let x = x.max(28);
                let text = format!("{} msgs · {} · {} consumers · {} R{}   {}", payload::count(s.state.messages as f64), payload::bytes(s.state.bytes as f64), s.state.consumer_count, s.config.storage, s.config.num_replicas, s.config.subjects.join(" "));
                g.put(line, x, &fit(&text, x), Role::Text);
            }
            Row::Info(text, role) => {
                g.put(line, 5, &fit(text, 5), role.clone());
            }
            Row::Consumer(_, j) => {
                let c = &self.consumers[*j];
                let x = g.put(line, 5, &grid::fit(&c.name, 24), Role::Text).max(30);
                let text = format!(
                    "pending {} · ack pending {} · redelivered {} · {} · {}{}",
                    payload::count(c.num_pending as f64),
                    payload::count(c.num_ack_pending as f64),
                    payload::count(c.num_redelivered as f64),
                    if c.is_push() { "push" } else { "pull" },
                    c.config.ack_policy,
                    c.config.filter_subject.as_ref().map(|f| format!(" · {f}")).unwrap_or_default()
                );
                let role = if c.num_redelivered > 0 || c.num_ack_pending > 0 { Role::Warn } else { Role::Muted };
                g.put(line, x, &fit(&text, x), role);
            }
            Row::Msg(k) => {
                let m = &self.browse[*k];
                let x = g.put(line, 5, &format!("#{:<8}", m.seq), Role::Accent);
                let x = g.put(line, x + 1, &grid::fit(&m.subject, 28), Role::Title).max(x + 30);
                let x = g.put(line, x, &format!("{:>9}  ", payload::bytes(m.payload.len() as f64)), Role::Muted);
                g.put(line, x, &payload::preview(&m.payload, width.saturating_sub(x + 1)).0, Role::Text);
            }
            Row::MsgLine(k, l) => {
                if let Some(parts) = self.msg_lines(*k).get(*l) {
                    put_line(g, line, 9, width, parts);
                }
            }
            Row::Bucket(i) => {
                let s = &self.streams[*i];
                let name = kv::bucket_of(&s.config.name).unwrap_or("");
                let open = self.open_bucket.as_deref() == Some(name);
                let x = g.put(line, 1, if open { "▾ " } else { "▸ " }, Role::Muted);
                let x = g.put(line, x, &grid::fit(name, 24), Role::Title).max(28);
                let text = format!("{} keys · {} values · {} · history {} · ttl {}", s.state.num_subjects, payload::count(s.state.messages as f64), payload::bytes(s.state.bytes as f64), s.config.max_msgs_per_subject, jetstream::nanos(s.config.max_age));
                g.put(line, x, &fit(&text, x), Role::Text);
            }
            Row::Key(j) => {
                let (k, n) = &self.keys[*j];
                let x = g.put(line, 5, &grid::fit(k, 40), Role::Text).max(46);
                g.put(line, x, &format!("{n} kept"), Role::Muted);
            }
            Row::ValueLine(l) => {
                if let Some(parts) = self.value_lines().get(*l) {
                    put_line(g, line, 9, width, parts);
                }
            }
            Row::History(h) => {
                let e = &self.history[*h];
                let x = g.put(line, 9, &format!("rev {:<6}", e.revision), Role::Accent);
                let x = g.put(line, x + 1, &format!("{:<7}", e.operation.name()), if e.operation == Operation::Put { Role::Text } else { Role::Warn });
                let x = g.put(line, x + 1, &short_time(&e.time), Role::Muted);
                g.put(line, x + 2, &payload::preview(&e.value, width.saturating_sub(x + 3)).0, Role::Text);
            }
            Row::Watch(k) => {
                let Some(w) = core.watch.get(*k) else { return };
                let key = kv::key_of(&w.bucket, &w.msg.subject).unwrap_or(&w.msg.subject);
                let op = Operation::of(w.msg.headers.as_deref());
                let x = g.put(line, 7, &payload::clock(w.msg.at_us / 1000, self.offset_s), Role::Muted);
                let x = g.put(line, x + 2, &format!("{:<7}", op.name()), if op == Operation::Put { Role::Good } else { Role::Warn });
                let x = g.put(line, x + 1, &grid::fit(key, 30), Role::Title).max(x + 32);
                g.put(line, x, &payload::preview(&w.msg.payload, width.saturating_sub(x + 1)).0, Role::Text);
            }
        }
    }

    fn stream_at(&self, row: usize) -> Option<String> {
        self.rows[..=row.min(self.rows.len().saturating_sub(1))].iter().rev().find_map(|r| match r {
            Row::Stream(i) => Some(self.streams[*i].config.name.clone()),
            _ => None,
        })
    }

    fn bucket_at(&self, row: usize) -> Option<String> {
        self.rows[..=row.min(self.rows.len().saturating_sub(1))].iter().rev().find_map(|r| match r {
            Row::Bucket(i) => kv::bucket_of(&self.streams[*i].config.name).map(str::to_string),
            _ => None,
        })
    }

    fn key_at(&self, row: usize) -> Option<String> {
        self.rows[..=row.min(self.rows.len().saturating_sub(1))].iter().rev().find_map(|r| match r {
            Row::Key(j) => Some(Some(self.keys[*j].0.clone())),
            Row::Bucket(_) => Some(None),
            _ => None,
        })?
    }

    pub fn key(&mut self, core: &mut Core, net: &mut dyn Net, key: Key, now: u64) -> JsAction {
        self.note = None;
        let row = self.rows.get(self.cursor).cloned();
        match key {
            Key::Char('j') | Key::Down => self.cursor = (self.cursor + 1).min(self.rows.len().saturating_sub(1)),
            Key::Char('k') | Key::Up => self.cursor = self.cursor.saturating_sub(1),
            Key::Char('g') if self.tab() == Tab::Kv => self.cursor = 0,
            Key::Home => self.cursor = 0,
            Key::Char('G') | Key::End => self.cursor = self.rows.len().saturating_sub(1),
            Key::Tab | Key::BackTab => {
                self.tab = Some(if self.tab() == Tab::Streams { Tab::Kv } else { Tab::Streams });
                self.cursor = 0;
                self.top = 0;
            }
            Key::Char('r') => self.reload(core, net, now),
            Key::Char('L') => return JsAction::Live,
            Key::Enter | Key::Char('l') => match row {
                Some(Row::Stream(i)) => {
                    let name = self.streams[i].config.name.clone();
                    if self.open_stream.as_deref() == Some(&name) {
                        self.open_stream = None;
                    } else {
                        self.open_stream = Some(name.clone());
                        self.browse.clear();
                        self.load_stream(core, net, &name, now);
                    }
                }
                Some(Row::Msg(k)) => {
                    let seq = self.browse[k].seq;
                    self.open_msg = if self.open_msg == Some(seq) { None } else { Some(seq) };
                }
                Some(Row::MsgLine(..)) => self.open_msg = None,
                Some(Row::Bucket(i)) => {
                    let name = kv::bucket_of(&self.streams[i].config.name).unwrap_or("").to_string();
                    if self.open_bucket.as_deref() == Some(&name) {
                        self.open_bucket = None;
                    } else {
                        self.open_bucket = Some(name.clone());
                        self.load_keys(core, net, &name, now);
                    }
                }
                Some(Row::Key(j)) => {
                    let k = self.keys[j].0.clone();
                    if self.value.as_ref().is_some_and(|v| v.key == k) {
                        self.value = None;
                        self.history.clear();
                    } else if let Some(b) = self.open_bucket.clone() {
                        self.load_value(core, net, &b, &k, now);
                    }
                }
                _ => {}
            },
            Key::Char('b') => {
                if let Some(s) = self.stream_at(self.cursor).filter(|_| self.tab() == Tab::Streams) {
                    self.open_stream = Some(s.clone());
                    let first = self.streams.iter().find(|x| x.config.name == s).map_or(1, |x| x.state.first_seq.max(1));
                    self.browse_from(core, net, &s, first, now);
                }
            }
            Key::Char('n') => {
                if let Some(s) = self.open_stream.clone() {
                    let next = self.browse_next;
                    self.browse_from(core, net, &s, next, now);
                }
            }
            Key::Char('g') => {
                if let Some(s) = self.stream_at(self.cursor) {
                    return JsAction::AskSeq(s);
                }
            }
            Key::Char('t') => {
                if let Some(s) = self.stream_at(self.cursor) {
                    return JsAction::AskTime(s);
                }
            }
            Key::Char('P') => {
                if let Some(s) = self.stream_at(self.cursor).filter(|_| self.tab() == Tab::Streams) {
                    return JsAction::ConfirmPurge(s);
                }
            }
            Key::Char('d') => match (self.tab(), row) {
                (Tab::Streams, Some(Row::Msg(k) | Row::MsgLine(k, _))) => {
                    if let Some(s) = self.open_stream.clone() {
                        return JsAction::ConfirmDelete(s, self.browse[k].seq);
                    }
                }
                (Tab::Kv, _) => {
                    if let (Some(b), Some(k)) = (self.bucket_at(self.cursor), self.key_at(self.cursor)) {
                        return JsAction::ConfirmKvDelete(b, k);
                    }
                }
                _ => {}
            },
            Key::Char('p') if self.tab() == Tab::Kv => {
                if let Some(b) = self.bucket_at(self.cursor) {
                    return JsAction::AskPut(b);
                }
            }
            Key::Char('w') if self.tab() == Tab::Kv => {
                if let Some(b) = self.bucket_at(self.cursor) {
                    let on = self.watching.as_deref() != Some(&b);
                    if let Some(old) = self.watching.take() {
                        if let Some(i) = core.conn_index(self.conn) {
                            let _ = core.watch_bucket(net, i, &old, false);
                        }
                    }
                    if on {
                        if let Some(i) = core.conn_index(self.conn) {
                            match core.watch_bucket(net, i, &b, true) {
                                Ok(()) => self.watching = Some(b),
                                Err(e) => self.note = Some((e, Role::Bad)),
                            }
                        }
                    }
                }
            }
            Key::Char('y') | Key::Char('o') => {
                let bytes = match row {
                    Some(Row::Msg(k) | Row::MsgLine(k, _)) => Some(self.browse[k].payload.clone()),
                    Some(Row::Key(_) | Row::ValueLine(_)) => self.value.as_ref().map(|v| v.value.clone()),
                    Some(Row::History(h)) => Some(self.history[h].value.clone()),
                    Some(Row::Watch(k)) => core.watch.get(k).map(|w| w.msg.payload.clone()),
                    _ => None,
                };
                if let Some(b) = bytes {
                    let (text, lang) = payload::full_text(&b);
                    return if key == Key::Char('y') { JsAction::Copy(String::from_utf8_lossy(&b).into_owned()) } else { JsAction::Open(text, lang) };
                }
            }
            _ => {}
        }
        JsAction::None
    }
}

fn put_line(g: &mut Grid, line: usize, x: usize, width: usize, parts: &[(String, Role)]) {
    let mut x = x;
    for (t, r) in parts {
        if x + 1 >= width {
            break;
        }
        x = g.put(line, x, &grid::fit(t, width - 1 - x), r.clone());
    }
}

/// `2024-01-02T03:04:05.123456Z` as `2024-01-02 03:04:05`.
fn short_time(t: &str) -> String {
    if t.starts_with("0001-") || t.is_empty() {
        return "—".into();
    }
    let t = t.replace('T', " ");
    t.chars().take(19).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tests::up;
    use crate::conn::Route;

    fn reply(core: &mut Core, net: &mut crate::core::tests::FakeNet, s: u64, token: u64, json: &str) {
        let inbox = core.conns[0].inbox.prefix.clone();
        let sid = core.conns[0].routes.iter().find(|(_, r)| **r == Route::Inbox).map(|(s, _)| *s).unwrap();
        core.on_data(net, s, format!("MSG {inbox}.{token} {sid} {}\r\n{json}\r\n", json.len()).as_bytes(), 1_000_100_000);
    }

    #[test]
    fn streams_and_buckets_are_listed() {
        let (mut core, mut net, s) = up(&[]);
        let mut js = JsView::default();
        js.reload(&mut core, &mut net, 1_000_002_000);
        assert!(net.text().contains("PUB $JS.API.STREAM.LIST "), "{}", net.text());
        reply(&mut core, &mut net, s, 1, r#"{"total":2,"offset":0,"limit":256,"streams":[{"config":{"name":"ORDERS","subjects":["orders.>"],"storage":"file","num_replicas":1},"state":{"messages":3,"bytes":30,"first_seq":1,"last_seq":3}},{"config":{"name":"KV_cfg","subjects":["$KV.cfg.>"],"max_msgs_per_subject":5},"state":{"messages":2,"num_subjects":2}}]}"#);
        for r in std::mem::take(&mut core.replies) {
            if let Pending::Js { want, .. } = r.what {
                js.on_reply(&mut core, &mut net, want, r.result, 1_000_100_000);
            }
        }
        assert!(js.loaded);
        let page = js.render(&core, 100, 30);
        let t = page.canvas.unwrap().text;
        assert!(t.contains("ORDERS") && t.contains("Streams 1") && t.contains("Key-value 1"), "{t}");
        js.key(&mut core, &mut net, Key::Tab, 0);
        let t = js.render(&core, 100, 30).canvas.unwrap().text;
        assert!(t.contains("cfg") && t.contains("history 5"), "{t}");
        net.written.clear();
        js.key(&mut core, &mut net, Key::Enter, 1_000_200_000);
        assert!(net.text().contains("STREAM.INFO.KV_cfg") && net.text().contains("subjects_filter"), "{}", net.text());
    }

    #[test]
    fn a_page_of_messages_is_read_one_by_one() {
        let (mut core, mut net, s) = up(&[]);
        let mut js = JsView { open_stream: Some("ORDERS".into()), ..JsView::default() };
        js.browse_from(&mut core, &mut net, "ORDERS", 1, 1_000_002_000);
        assert!(net.text().contains(r#"{"seq":1,"next_by_subj":">"}"#), "{}", net.text());
        reply(&mut core, &mut net, s, 1, r#"{"message":{"subject":"orders.new","seq":4,"data":"aGk=","time":"2024-01-01T00:00:00Z"}}"#);
        let r = core.replies.pop().unwrap();
        let Pending::Js { want, .. } = r.what else { panic!() };
        net.written.clear();
        js.on_reply(&mut core, &mut net, want, r.result, 1_000_100_000);
        assert_eq!(js.browse.len(), 1);
        assert!(net.text().contains(r#"{"seq":5,"next_by_subj":">"}"#), "the next one asked for: {}", net.text());
        reply(&mut core, &mut net, s, 2, r#"{"error":{"code":404,"err_code":10037,"description":"no message found"}}"#);
        let r = core.replies.pop().unwrap();
        let Pending::Js { want, .. } = r.what else { panic!() };
        js.on_reply(&mut core, &mut net, want, r.result, 1_000_100_000);
        assert!(!js.browsing);
        assert_eq!(js.browse_next, 5);
    }

    #[test]
    fn kv_delete_sends_its_header() {
        let (mut core, mut net, _) = up(&[]);
        let mut js = JsView::default();
        js.kv_delete(&mut core, &mut net, "cfg", "app.port", 0);
        assert!(net.text().contains("HPUB $KV.cfg.app.port ") && net.text().contains("KV-Operation: DEL"), "{}", net.text());
        js.put(&mut core, &mut net, "cfg", "bad key", b"x", 0);
        assert!(js.note.is_some());
    }

    #[test]
    fn no_jetstream_is_said() {
        let r: Result<Owned, String> = Ok(Owned { headers: Some(b"NATS/1.0 503\r\n\r\n".to_vec()), ..Owned::default() });
        assert!(answer(&r).unwrap_err().contains("JetStream isn't on"));
    }
}
