//! NATS in Fenix, as a WebAssembly extension.
//!
//! `fenix_nats` (the crate) speaks the protocol, sans I/O; [`core`] keeps
//! the connections and the store and is the path every message takes;
//! [`view`] and [`js`] lay out the pages. Here what they ask for is done
//! through Fenix -- sockets, timers, prompts, the clipboard, notifications
//! -- and what Fenix says is given to them.
//!
//! The extension runs on a thread of its own, and a page is laid out
//! between its calls, so each call is kept short: a socket's chunk is
//! counted and stored in well under a millisecond, the page is drawn on a
//! timer (10 times a second, only when something changed and it's in
//! view), and a filter that reads payloads reads them a time budget at a
//! time.

pub mod conn;
pub mod core;
pub mod filter;
pub mod js;
pub mod payload;
pub mod publish;
pub mod store;
pub mod tools;
pub mod view;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use fenix_extension_api::{self as api, export, host, info, socket, warn, Event, Extension, Key, KeyPress, Level, MouseAction, MouseEvent, NamedKey, Notification, Page, PickerItem, Role, SurfaceKey, TlsOptions};
use serde_json::json;

use crate::conn::{Addr, Auth, Net};
use crate::core::{Core, Event as CoreEvent, Pending, Reply};
use crate::js::{JsAction, JsView};
use crate::store::Limits;
use crate::view::{Action, LiveView};

/// Where exports go (`permissions.folders`).
const EXPORTS: &str = "/folders/exports";
/// Where `.creds` files are read from.
const CREDS: &str = "/folders/creds";
/// Its saved filters and rules.
const SAVED: &str = "/data/nats.json";

/// Unix microseconds.
fn now_us() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_micros() as u64)
}

/// Fenix's sockets.
struct Fenix {
    accept_invalid: bool,
}

impl Fenix {
    fn tls(&self) -> TlsOptions {
        if self.accept_invalid {
            TlsOptions::new().accept_invalid()
        } else {
            TlsOptions::new()
        }
    }
}

impl Net for Fenix {
    fn connect(&mut self, host: &str, port: u16, tls: bool) -> Result<u64, String> {
        if tls {
            socket::connect_tls(host, port, &self.tls())
        } else {
            socket::connect(host, port)
        }
    }
    fn start_tls(&mut self, s: u64, host: &str) -> Result<(), String> {
        socket::start_tls(s, &self.tls().server_name(host))
    }
    fn write(&mut self, s: u64, data: &[u8]) -> Result<(), String> {
        socket::write(s, data)
    }
    fn close(&mut self, s: u64) {
        socket::close(s)
    }
}

fn net() -> Fenix {
    Fenix { accept_invalid: setting_bool("nats.accept_invalid_certs", false) }
}

fn setting_text(key: &str) -> String {
    api::setting(key).unwrap_or_default()
}

fn setting_int(key: &str, default: i64) -> i64 {
    api::setting(key).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

fn setting_bool(key: &str, default: bool) -> bool {
    match api::setting(key).as_deref().map(str::trim) {
        Some("true" | "on" | "yes" | "1") => true,
        Some("false" | "off" | "no" | "0") => false,
        _ => default,
    }
}

/// A list setting: JSON, or a line with commas.
fn setting_list(key: &str) -> Vec<String> {
    let text = setting_text(key);
    let items: Vec<String> = serde_json::from_str::<Vec<String>>(text.trim()).unwrap_or_else(|_| text.split([',', '\n']).map(str::to_string).collect());
    items.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

fn limits() -> Limits {
    Limits {
        budget: (setting_int("nats.budget_mib", 64).clamp(4, 80) as usize) << 20,
        per_subject: setting_int("nats.per_subject", 2000).clamp(10, 100_000) as usize,
        per_subject_bytes: (setting_int("nats.per_subject_kib", 4096).clamp(16, 65536) as usize) << 10,
        max_subjects: setting_int("nats.max_subjects", 5000).clamp(10, 50_000) as usize,
    }
}

fn servers() -> (Vec<Addr>, Vec<String>) {
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    for url in setting_list("nats.servers") {
        match Addr::parse(&url) {
            Ok(a) => ok.push(a),
            Err(e) => bad.push(e),
        }
    }
    (ok, bad)
}

fn patterns() -> Vec<String> {
    let p = setting_list("nats.subjects");
    p.into_iter().filter(|s| fenix_nats::subject::check_pattern(s).is_ok()).collect()
}

/// The keychain's entry for a server's credentials.
fn auth_key(addr: &Addr) -> String {
    format!("nats.auth.{}", addr.key())
}

/// How it signs in to `addr`: what the keychain keeps for it.
fn auth_for(addr: &Addr) -> Result<Auth, String> {
    let Some(text) = api::secret(&auth_key(addr)) else { return Ok(Auth::None) };
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| "its credentials in the keychain aren't readable: SPC o m a sets them again".to_string())?;
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    Ok(match v["method"].as_str().unwrap_or("none") {
        "user" => Auth::UserPassword(s("user"), s("password")),
        "token" => Auth::Token(s("token")),
        "nkey" => Auth::NKey(fenix_nats::auth::KeyPair::from_seed(&s("seed"))?),
        "creds" => {
            let file = s("file");
            let bytes = api::read_file(&format!("{CREDS}/{file}")).map_err(|e| format!("{file}: {e}"))?;
            Auth::Creds(Box::new(fenix_nats::auth::parse_creds(&String::from_utf8_lossy(&bytes))?))
        }
        _ => Auth::None,
    })
}

/// What a prompt, a picker or a compose pane was opened for.
#[derive(Debug, Clone)]
enum Ask {
    Subscribe,
    Filter,
    SaveFilter,
    Rules,
    JsSeq(String),
    JsTime(String),
    Purge(String),
    Delete(String, u64),
    PutKey(String),
    PutValue(String, String),
    KvDelete(String, String),
    Clear,
    SavedFilters,
    Unsubscribe,
    Toggle,
    CredServer,
    CredMethod(Addr),
    CredUser(Addr),
    CredPassword(Addr, String),
    CredToken(Addr),
    CredSeed(Addr),
    CredFile(Addr),
}

/// Filters and rules kept between runs.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Saved {
    #[serde(default)]
    filters: BTreeMap<String, String>,
    #[serde(default)]
    rules: String,
}

/// An agent's sample waiting for messages from the server.
struct ToolSample {
    server: String,
    found: Vec<serde_json::Value>,
}

struct Nats {
    core: Core,
    live: LiveView,
    js: JsView,
    /// Servers told what `>` does to their requests.
    told_shared: HashSet<String>,
    live_page: Option<u64>,
    js_page: Option<u64>,
    shown: HashSet<u64>,
    timer: Option<u64>,
    interval_ms: u64,
    last_tick_us: u64,
    last_status_us: u64,
    status: String,
    scratch: Option<u64>,
    asks: HashMap<u64, Ask>,
    saved: Saved,
    samples: HashMap<u64, ToolSample>,
    refreshed_at: u64,
}

impl Default for Nats {
    fn default() -> Nats {
        let now = now_us();
        Nats {
            core: Core::new(Limits::default(), now),
            live: LiveView::default(),
            js: JsView::default(),
            told_shared: HashSet::new(),
            live_page: None,
            js_page: None,
            shown: HashSet::new(),
            timer: None,
            interval_ms: 100,
            last_tick_us: now,
            last_status_us: 0,
            status: String::new(),
            scratch: None,
            asks: HashMap::new(),
            saved: Saved::default(),
            samples: HashMap::new(),
            refreshed_at: 0,
        }
    }
}

const RULES_HELP: &str = "# Highlight rules: one a line, a colour then a filter. What a rule matches is
# coloured, not hidden; ! before the colour also notifies (once a rule every
# 30 seconds at most). Colours: accent, good, warn, bad, #rrggbb.
#
#   warn  orders.* json:.status=failed
#   !bad  re:panic|fatal
#   #4a90d9 h:Nats-Msg-Id size:>64k
";

const FILTER_HELP: &str = "Filter (subject.*  h:K=V  re:...  json:.a=1  size:10..2k  words)";

impl Nats {
    fn configure(&mut self) {
        let (servers, bad) = servers();
        for e in bad {
            warn!("nats.servers: {e}");
        }
        self.core.client_name = { let n = setting_text("nats.client_name"); if n.trim().is_empty() { "fenix".into() } else { n.trim().to_string() } };
        let mut net = net();
        let pats = patterns();
        self.core.set_servers(&mut net, servers, &pats);
        for c in &mut self.core.conns {
            if c.patterns != pats {
                c.patterns = pats.clone();
                c.sync(&mut net);
            }
        }
        self.core.store.set_limits(limits());
        let hz = setting_int("nats.refresh_hz", 10).clamp(1, 30) as u64;
        self.interval_ms = 1000 / hz;
        if let Some(t) = self.timer.take() {
            host::cancel_timer(t);
        }
        self.timer = Some(host::set_timer(self.interval_ms as u32, true));
        if self.core.conn_index(self.live.conn).is_none() {
            self.live.conn = self.core.conns.first().map_or(0, |c| c.id);
            self.js.conn = self.live.conn;
        }
    }

    fn connect_all(&mut self) {
        let mut net = net();
        let now = now_us();
        for i in 0..self.core.conns.len() {
            if !self.core.conns[i].wanted() {
                match auth_for(&self.core.conns[i].addr) {
                    Ok(a) => self.core.connect(&mut net, i, a, now),
                    Err(e) => {
                        self.core.conns[i].error = Some(e);
                        self.core.connect(&mut net, i, Auth::None, now);
                    }
                }
            }
        }
    }

    fn conn_for(&self, server: Option<&str>) -> Option<usize> {
        match server {
            Some(s) => self.core.conns.iter().position(|c| c.addr.key() == s || c.addr.url == s || c.addr.host == s),
            None => self.core.conn_index(self.live.conn).or(if self.core.conns.is_empty() { None } else { Some(0) }),
        }
    }

    fn refresh(&mut self, force: bool) {
        if !(self.core.dirty || force) {
            return;
        }
        let mut any = false;
        for p in [self.live_page, self.js_page].into_iter().flatten() {
            if self.shown.contains(&p) {
                api::page::refresh(p);
                any = true;
            }
        }
        if any {
            self.core.dirty = false;
            self.refreshed_at = now_us();
        }
    }

    /// The modeline: each connection's state, and the rate.
    fn update_status(&mut self, now: u64) {
        if now.saturating_sub(self.last_status_us) < 1_000_000 {
            return;
        }
        self.last_status_us = now;
        let conns = &self.core.conns;
        let text = if conns.is_empty() || conns.iter().all(|c| !c.wanted()) {
            String::new()
        } else {
            let up = conns.iter().filter(|c| c.state() == "up").count();
            let wanted = conns.iter().filter(|c| c.wanted()).count();
            let dot = if up == wanted { "●" } else if up > 0 { "◐" } else { "○" };
            let rate = self.core.store.totals.rate.per_sec(now / 1_000_000);
            let mut t = if wanted == 1 { format!("NATS {dot} {}", conns.iter().find(|c| c.wanted()).map(|c| c.state()).unwrap_or("")) } else { format!("NATS {dot} {up}/{wanted}") };
            if rate > 0.0 {
                t.push_str(&format!(" {}/s", payload::count(rate)));
            }
            if self.core.store.sampling > 1 {
                t.push_str(&format!(" 1:{}", self.core.store.sampling));
            }
            t
        };
        if text != self.status {
            api::status(if text.is_empty() { None } else { Some(&text) });
            self.status = text;
        }
    }

    /// What the core left for the extension: events, then answers.
    fn drain(&mut self) {
        for e in std::mem::take(&mut self.core.events) {
            match e {
                CoreEvent::Up(id) => {
                    if let Some(c) = self.core.conn_index(id).map(|i| &self.core.conns[i]) {
                        info!("{}: connected{}", c.addr.key(), c.info.as_ref().map(|i| format!(", NATS {}", i.version)).unwrap_or_default());
                        // Interest in everything is interest in every
                        // request: the server stops saying no one answers.
                        let key = c.addr.key();
                        if !c.addr.is_local() && c.wanted_subjects().iter().any(|s| s == ">") && self.told_shared.insert(key.clone()) {
                            host::notify(Level::Info, &format!("NATS {key}: subscribed to `>`, so a request no one answers now waits for its timeout, for every client of this server. A filter or nats.subjects narrows it."));
                        }
                    }
                    self.last_status_us = 0;
                    // The page was restored, or read, before this.
                    if self.js_page.is_some() && self.js.conn == id && self.js.stale() {
                        let mut net = net();
                        self.js.reload(&mut self.core, &mut net, now_us());
                        self.core.dirty = true;
                    }
                }
                CoreEvent::Down(id, why) => {
                    if let Some(c) = self.core.conn_index(id).map(|i| &self.core.conns[i]) {
                        warn!("{}: dropped{}", c.addr.key(), why.map(|w| format!(": {w}")).unwrap_or_default());
                    }
                    self.last_status_us = 0;
                }
                CoreEvent::AuthFailed(id, why) => {
                    if let Some(c) = self.core.conn_index(id).map(|i| &self.core.conns[i]) {
                        let text = format!("NATS {}: sign-in refused ({why}) -- SPC o m a sets its credentials", c.addr.key());
                        host::notify(Level::Warn, &text);
                    }
                }
                CoreEvent::Warning(id, why) => {
                    if let Some(c) = self.core.conn_index(id).map(|i| &self.core.conns[i]) {
                        warn!("{}: {why}", c.addr.key());
                    }
                }
                CoreEvent::Alert { rule, conn, subject, preview, count } => {
                    if setting_bool("nats.notify", true) {
                        let server = self.core.conn_index(conn).map(|i| self.core.conns[i].addr.key()).unwrap_or_default();
                        let more = if count > 1 { format!(" ({count} matches)") } else { String::new() };
                        let line = self.core.rules.list.get(rule).map(|r| r.filter.source.clone()).unwrap_or_default();
                        Notification::new(format!("NATS: {subject}{more}")).body(format!("{preview}\n{server} · rule: {line}")).tag(format!("nats.rule.{rule}")).action("Show", "nats.open").show();
                    }
                }
                CoreEvent::SampleDone(call) => self.finish_sample(call),
            }
        }
        for r in std::mem::take(&mut self.core.replies) {
            self.on_reply(r);
        }
    }

    fn on_reply(&mut self, r: Reply) {
        let now = now_us();
        match r.what {
            Pending::Js { want, .. } => {
                let mut net = net();
                self.js.on_reply(&mut self.core, &mut net, want, r.result, now);
                self.core.dirty = true;
            }
            Pending::User { subject, .. } => match r.result {
                Ok(m) if m.status() == Some(503) => {
                    let text = format!("{subject}: no one answers (no responders)");
                    host::notify(Level::Warn, &format!("NATS {text}"));
                    self.live.note = Some((text, Role::Warn));
                }
                Ok(m) => {
                    let ms = r.latency_us as f64 / 1000.0;
                    let (text, lang) = payload::full_text(&m.payload);
                    let mut head = String::new();
                    if let Some(h) = m.headers.as_deref().and_then(|h| fenix_nats::headers::Headers::parse(h).ok()) {
                        for (k, v) in h.iter() {
                            head.push_str(&format!("{k}: {v}\n"));
                        }
                    }
                    let body = if head.is_empty() || lang.is_some() { text } else { format!("{head}\n{text}") };
                    api::open_text_beside(&format!("NATS reply · {subject} · {ms:.1} ms"), &body, lang, 0.4);
                    self.live.note = Some((format!("{subject}: answered in {ms:.1} ms, {}", payload::bytes(m.payload.len() as f64)), Role::Good));
                }
                Err(e) => {
                    let everything = self.core.conn_index(r.conn).is_some_and(|i| self.core.conns[i].wanted_subjects().iter().any(|s| s == ">"));
                    let e = if everything { format!("{e} (subscribed to `>`, so the server can't say no one answers)") } else { e };
                    host::notify(Level::Warn, &format!("NATS {subject}: {e}"));
                    self.live.note = Some((format!("{subject}: {e}"), Role::Bad));
                }
            },
            Pending::Tool { call, .. } => {
                let answer = r.result.map(|m| serde_json::to_string(&json!({ "reply": tools::owned(&m), "latency_ms": r.latency_us as f64 / 1000.0 })).unwrap_or_default());
                api::answer(call, answer);
            }
        }
        self.core.dirty = true;
    }

    fn finish_sample(&mut self, call: u64) {
        let Some(mut t) = self.samples.remove(&call) else { return };
        if let Some(s) = self.core.samples.remove(&call) {
            let mut fresh: Vec<serde_json::Value> = s.got.iter().rev().map(tools::owned).collect();
            fresh.append(&mut t.found);
            t.found = fresh;
        }
        let answer = json!({ "server": t.server, "messages": t.found, "count": t.found.len() });
        api::answer(call, Ok(answer.to_string()));
    }

    fn tick(&mut self) {
        let now = now_us();
        let elapsed = now.saturating_sub(self.last_tick_us);
        let late = elapsed.saturating_sub(self.interval_ms * 1000);
        self.last_tick_us = now;
        let mut net = net();
        let was = self.core.store.sampling;
        self.core.tick(&mut net, now, late, &mut |a| auth_for(a).unwrap_or_default());
        if self.core.store.sampling != was {
            info!("load: storing 1 in {} of a busy subject's messages (the timer {} ms late, busy {:.0}%)", self.core.store.sampling, self.core.load.lag_ms, self.core.load.busy * 100.0);
        }
        self.drain();
        let live_shown = self.live_page.is_some_and(|p| self.shown.contains(&p));
        if live_shown && self.live.background(&self.core, 2) {
            self.core.dirty = true;
        }
        // Rates and "last seen" move on by the second even when nothing
        // comes.
        if now.saturating_sub(self.refreshed_at) >= 1_000_000 {
            self.core.dirty = true;
        }
        self.refresh(false);
        self.update_status(now);
    }

    fn open_live(&mut self) {
        match self.live_page {
            Some(p) => api::page::focus(p),
            None => self.live_page = Some(api::page::open("live", "NATS")),
        }
        self.connect_all();
        self.core.dirty = true;
    }

    fn open_js(&mut self) {
        self.js.conn = self.live.conn;
        match self.js_page {
            Some(p) => api::page::focus(p),
            None => self.js_page = Some(api::page::open("jetstream", "JetStream")),
        }
        let mut net = net();
        self.js.reload(&mut self.core, &mut net, now_us());
        self.core.dirty = true;
    }

    fn open_scratch(&mut self, text: &str) {
        if let Some(s) = self.scratch.take() {
            api::scratch::close(s);
        }
        self.scratch = Some(api::scratch::open("NATS publish (SPC o m s sends, SPC o m r requests)", text, Some("json")));
    }

    /// The publish buffer, sent: published, or as a request.
    fn send(&mut self, request: bool) -> Result<(), String> {
        let Some(s) = self.scratch else {
            let server = self.conn_for(None).map(|i| self.core.conns[i].addr.key());
            let subject = self.live.focused_subject().map(|id| self.core.store.subjects[id as usize].name.to_string()).unwrap_or_else(|| "greeting".into());
            self.open_scratch(&publish::blank(&subject, server.as_deref()));
            return Err("write the message, then send it again".into());
        };
        let text = api::scratch::text(s).ok_or("the publish buffer was closed")?;
        let d = publish::parse(&text)?;
        let i = self.conn_for(d.server.as_deref()).ok_or_else(|| format!("no connection {}", d.server.clone().unwrap_or_default()))?;
        let mut net = net();
        let now = now_us();
        let c = &mut self.core.conns[i];
        if request {
            c.request(&mut net, &d.subject, &d.headers, &d.payload, Pending::User { subject: d.subject.clone(), started_us: now }, now + 5_000_000)?;
            self.live.note = Some((format!("asked {}…", d.subject), Role::Muted));
        } else {
            c.publish(&mut net, &d.subject, d.reply.as_deref(), &d.headers, &d.payload)?;
            let msg = format!("sent to {} ({})", d.subject, payload::bytes(d.payload.len() as f64));
            api::notify(&msg);
            self.live.note = Some((msg, Role::Good));
        }
        self.core.dirty = true;
        Ok(())
    }

    fn server_url(&self, id: u32) -> Option<String> {
        let conn = self.core.store.subjects.get(id as usize)?.conn;
        self.core.conn_index(conn).map(|i| self.core.conns[i].addr.url.clone())
    }

    fn action(&mut self, a: Action) {
        let now = now_us();
        let store = &self.core.store;
        match a {
            Action::None => {}
            Action::Filter => {
                let initial = self.live.structured.as_ref().map(|f| f.source.clone()).unwrap_or_default();
                self.asks.insert(api::prompt(FILTER_HELP, &initial), Ask::Filter);
            }
            Action::SavedFilters => {
                let mut items = vec![PickerItem::new("Save the filter as…", "save").detail(self.live.structured.as_ref().map(|f| f.source.clone()).unwrap_or_default()), PickerItem::new("No filter", "clear")];
                for (name, q) in &self.saved.filters {
                    items.push(PickerItem::new(name.clone(), format!("use:{name}")).detail(q.clone()));
                }
                for name in self.saved.filters.keys() {
                    items.push(PickerItem::new(format!("Forget {name}"), format!("forget:{name}")));
                }
                self.asks.insert(api::pick("NATS filters", &items), Ask::SavedFilters);
            }
            Action::Rules => {
                let text = if self.saved.rules.trim().is_empty() { RULES_HELP.to_string() } else { format!("{RULES_HELP}\n{}", self.saved.rules) };
                self.asks.insert(api::compose("NATS highlight rules", "Save", &text, true), Ask::Rules);
            }
            Action::Publish => {
                let server = self.conn_for(None).map(|i| self.core.conns[i].addr.key());
                let subject = self.live.focused_subject().map(|id| store.subjects[id as usize].name.to_string()).unwrap_or_else(|| "greeting".into());
                self.open_scratch(&publish::blank(&subject, server.as_deref()));
            }
            Action::Request => {
                if let Err(e) = self.send(true) {
                    self.live.note = Some((e, Role::Warn));
                }
            }
            Action::Edit(id, seq) => {
                if let Some(v) = store.get(id, seq) {
                    let server = self.core.conn_index(store.subjects[id as usize].conn).map(|i| self.core.conns[i].addr.key());
                    let text = publish::from_view(&v, server.as_deref());
                    self.open_scratch(&text);
                }
            }
            Action::Republish(id, seq) => {
                let Some(v) = store.get(id, seq) else { return };
                let headers = v.headers.and_then(|h| fenix_nats::headers::Headers::parse(h).ok()).map(|h| fenix_nats::headers::HeaderMap::from_headers(&h)).unwrap_or_default();
                let (subject, reply, body) = (v.subject.to_string(), v.reply.map(str::to_string), v.payload.to_vec());
                let conn = store.subjects[id as usize].conn;
                let mut net = net();
                let result = match self.core.conn_index(conn) {
                    Some(i) => self.core.conns[i].publish(&mut net, &subject, reply.as_deref(), &headers, &body),
                    None => Err("its connection is gone".into()),
                };
                self.live.note = Some(match result {
                    Ok(()) => (format!("sent to {subject} again"), Role::Good),
                    Err(e) => (e, Role::Bad),
                });
            }
            Action::Copy(id, seq) => {
                if let Some(v) = store.get(id, seq) {
                    let text = match payload::sniff(v.payload) {
                        payload::Kind::Binary => payload::full_text(v.payload).0,
                        _ => String::from_utf8_lossy(v.payload).into_owned(),
                    };
                    api::copy(&text);
                    self.live.note = Some(("payload copied".into(), Role::Muted));
                }
            }
            Action::CopyCommand(id, seq) => {
                if let Some(v) = store.get(id, seq) {
                    api::copy(&publish::nats_pub(&v, self.server_url(id).as_deref()));
                    self.live.note = Some(("copied as a nats pub command".into(), Role::Muted));
                }
            }
            Action::OpenWhole(id, seq) => {
                if let Some(v) = store.get(id, seq) {
                    let (text, lang) = payload::full_text(v.payload);
                    api::open_text(&format!("{} #{}", v.subject, seq), &text, lang);
                }
            }
            Action::ExportSubject(id) => {
                let Some(s) = store.subjects.get(id as usize) else { return };
                let seqs: Vec<(u32, u32)> = s.msgs.iter().map(|e| (id, e.seq)).collect();
                let name = s.name.replace(['>', '*', '/', '\\', ':'], "_");
                self.export(&name, &seqs);
            }
            Action::ExportView => {
                let seqs = self.live.matching(&self.core);
                self.export("view", &seqs);
            }
            Action::Subscribe => {
                self.asks.insert(api::prompt("Subscribe to (* a token, > the rest)", ""), Ask::Subscribe);
            }
            Action::Unsubscribe => {
                let items: Vec<PickerItem> = patterns().into_iter().map(|p| PickerItem::new(p.clone(), p)).collect();
                if items.is_empty() {
                    self.live.note = Some(("not subscribed to anything".into(), Role::Muted));
                } else {
                    self.asks.insert(api::pick("Unsubscribe from", &items), Ask::Unsubscribe);
                }
            }
            Action::NextConnection => {
                let ids: Vec<u16> = self.core.conns.iter().map(|c| c.id).collect();
                if let Some(p) = ids.iter().position(|i| *i == self.live.conn) {
                    self.live.conn = ids[(p + 1) % ids.len()];
                    self.live.focus = None;
                    self.js.conn = self.live.conn;
                }
            }
            Action::ToggleConnection => {
                if let Some(i) = self.core.conn_index(self.live.conn) {
                    let mut net = net();
                    if self.core.conns[i].wanted() {
                        self.core.disconnect(&mut net, i);
                    } else {
                        let a = auth_for(&self.core.conns[i].addr).unwrap_or_default();
                        self.core.connect(&mut net, i, a, now);
                    }
                }
            }
            Action::JetStream => self.open_js(),
            Action::Clear => {
                self.asks.insert(api::prompt("Forget every stored message and subject? y", ""), Ask::Clear);
            }
        }
        self.drain();
        self.core.dirty = true;
    }

    fn export(&mut self, name: &str, seqs: &[(u32, u32)]) {
        let store = &self.core.store;
        let mut out = String::new();
        for (id, seq) in seqs {
            if let Some(v) = store.get(*id, *seq) {
                out.push_str(&publish::jsonl(&v));
                out.push('\n');
            }
        }
        let stamp = payload::rfc3339(now_us() / 1000).replace([':', '.'], "-");
        let path = format!("{EXPORTS}/{name}-{stamp}.jsonl");
        match std::fs::create_dir_all(EXPORTS).and_then(|_| std::fs::write(&path, out)) {
            Ok(()) => {
                self.live.note = Some((format!("{} messages written to {path}", seqs.len()), Role::Good));
                host::open_file(&path, None);
            }
            Err(e) => self.live.note = Some((format!("{path}: {e}"), Role::Bad)),
        }
    }

    fn load_saved(&mut self) {
        self.saved = std::fs::read_to_string(SAVED).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        match filter::parse_rules(&self.saved.rules) {
            Ok(r) => self.core.rules.set(r),
            Err(e) => warn!("highlight rules: {e}"),
        }
    }

    fn store_saved(&self) {
        if let Err(e) = std::fs::write(SAVED, serde_json::to_string_pretty(&self.saved).unwrap_or_default()) {
            warn!("{SAVED}: {e}");
        }
    }

    fn set_filter(&mut self, text: &str) -> Result<(), String> {
        let f = filter::Filter::parse(text)?;
        let narrow = f.subjects.clone();
        self.live.structured = if f.is_empty() { None } else { Some(f) };
        self.live.refilter();
        // Its subjects become what's subscribed on the server.
        let mut net = net();
        for c in &mut self.core.conns {
            if c.narrow != narrow {
                c.narrow = narrow.clone();
                c.sync(&mut net);
            }
        }
        self.core.dirty = true;
        Ok(())
    }

    fn set_patterns(&mut self, pats: Vec<String>) {
        if let Err(e) = api::set_setting("nats.subjects", Some(&serde_json::to_string(&pats).unwrap_or_default())) {
            warn!("nats.subjects: {e}");
        }
        let mut net = net();
        for c in &mut self.core.conns {
            c.patterns = pats.clone();
            c.sync(&mut net);
        }
        self.core.dirty = true;
    }

    fn answered(&mut self, id: u64, value: Option<String>) {
        let Some(ask) = self.asks.remove(&id) else { return };
        let now = now_us();
        let mut net = net();
        let Some(value) = value else { return };
        let v = value.trim().to_string();
        let result: Result<(), String> = (|| match ask {
            Ask::Subscribe => {
                fenix_nats::subject::check_pattern(&v)?;
                let mut pats = patterns();
                if !pats.contains(&v) {
                    pats.push(v);
                }
                self.set_patterns(pats);
                Ok(())
            }
            Ask::Unsubscribe => {
                let pats: Vec<String> = patterns().into_iter().filter(|p| *p != v).collect();
                self.set_patterns(pats);
                Ok(())
            }
            Ask::Filter => self.set_filter(&v),
            Ask::SavedFilters => {
                if v == "save" {
                    self.asks.insert(api::prompt("Save the filter as", ""), Ask::SaveFilter);
                } else if v == "clear" {
                    self.set_filter("")?;
                } else if let Some(name) = v.strip_prefix("use:") {
                    let q = self.saved.filters.get(name).cloned().unwrap_or_default();
                    self.set_filter(&q)?;
                } else if let Some(name) = v.strip_prefix("forget:") {
                    self.saved.filters.remove(name);
                    self.store_saved();
                }
                Ok(())
            }
            Ask::SaveFilter => {
                let q = self.live.structured.as_ref().map(|f| f.source.clone()).filter(|q| !q.is_empty()).or_else(|| (!self.live.quick.is_empty()).then(|| self.live.quick.text.clone())).ok_or("no filter to save: f writes one")?;
                if v.is_empty() {
                    return Err("a saved filter needs a name".into());
                }
                self.saved.filters.insert(v.clone(), q);
                self.store_saved();
                self.live.note = Some((format!("filter {v} saved"), Role::Good));
                Ok(())
            }
            Ask::Rules => {
                let body: String = value.lines().filter(|l| !RULES_HELP.lines().any(|h| h == *l)).collect::<Vec<_>>().join("\n");
                let rules = filter::parse_rules(&body)?;
                let n = rules.len();
                self.core.rules.set(rules);
                self.saved.rules = body.trim().to_string();
                self.store_saved();
                self.live.note = Some((format!("{n} highlight rules"), Role::Good));
                Ok(())
            }
            Ask::JsSeq(stream) => {
                let seq: u64 = v.trim_start_matches('#').parse().map_err(|_| format!("{v}: a sequence is a number"))?;
                self.js.open_stream = Some(stream.clone());
                self.js.browse_from(&mut self.core, &mut net, &stream, seq, now);
                Ok(())
            }
            Ask::JsTime(stream) => {
                let t = parse_time(&v, api::clock::now().offset().local_minus_utc() as i64).ok_or(format!("{v}: a time is 2024-05-01T10:00:00Z, 2024-05-01 10:00, 10:00, or 15m ago"))?;
                self.js.open_stream = Some(stream.clone());
                self.js.browse_time(&mut self.core, &mut net, &stream, &t, now);
                Ok(())
            }
            Ask::Purge(stream) => {
                if v != stream {
                    return Err(format!("not purged: type {stream} to purge it"));
                }
                self.js.purge(&mut self.core, &mut net, &stream, now);
                Ok(())
            }
            Ask::Delete(stream, seq) => {
                if v.eq_ignore_ascii_case("y") || v.eq_ignore_ascii_case("yes") {
                    self.js.delete(&mut self.core, &mut net, &stream, seq, now);
                }
                Ok(())
            }
            Ask::PutKey(bucket) => {
                if !fenix_nats::kv::valid_key(&v) {
                    return Err(format!("{v:?} can't be a key"));
                }
                self.asks.insert(api::compose(&format!("{bucket} · {v}"), "Put", "", true), Ask::PutValue(bucket, v));
                Ok(())
            }
            Ask::PutValue(bucket, key) => {
                self.js.put(&mut self.core, &mut net, &bucket, &key, value.as_bytes(), now);
                Ok(())
            }
            Ask::KvDelete(bucket, key) => {
                if v.eq_ignore_ascii_case("y") || v.eq_ignore_ascii_case("yes") {
                    self.js.kv_delete(&mut self.core, &mut net, &bucket, &key, now);
                }
                Ok(())
            }
            Ask::Clear => {
                if v.eq_ignore_ascii_case("y") || v.eq_ignore_ascii_case("yes") {
                    self.core.store.clear();
                    self.live.forget();
                }
                Ok(())
            }
            Ask::Toggle => {
                if let Some(i) = self.core.conns.iter().position(|c| c.addr.key() == v) {
                    if self.core.conns[i].wanted() {
                        self.core.disconnect(&mut net, i);
                    } else {
                        let a = auth_for(&self.core.conns[i].addr)?;
                        self.core.connect(&mut net, i, a, now);
                    }
                }
                Ok(())
            }
            Ask::CredServer => {
                let addr = self.core.conns.iter().find(|c| c.addr.key() == v).map(|c| c.addr.clone()).ok_or("no such server")?;
                self.ask_method(addr);
                Ok(())
            }
            Ask::CredMethod(addr) => {
                match v.as_str() {
                    "user" => {
                        self.asks.insert(api::prompt(&format!("{}: user", addr.key()), ""), Ask::CredUser(addr));
                    }
                    "token" => {
                        self.asks.insert(api::prompt_secret(&format!("{}: token", addr.key())), Ask::CredToken(addr));
                    }
                    "nkey" => {
                        self.asks.insert(api::prompt_secret(&format!("{}: NKey seed (SU...)", addr.key())), Ask::CredSeed(addr));
                    }
                    "creds" => {
                        let _ = std::fs::create_dir_all(CREDS);
                        let files: Vec<PickerItem> = api::folder_list(CREDS).unwrap_or_default().into_iter().filter(|f| f.name.ends_with(".creds")).map(|f| PickerItem::new(f.name.clone(), f.name)).collect();
                        if files.is_empty() {
                            return Err(format!("no .creds files in the credentials folder (nats.creds_folder, else {CREDS} in Fenix's data folder)"));
                        }
                        self.asks.insert(api::pick(&format!("{}: .creds file", addr.key()), &files), Ask::CredFile(addr));
                    }
                    _ => {
                        api::set_secret(&auth_key(&addr), None)?;
                        self.reconnect(&addr);
                    }
                }
                Ok(())
            }
            Ask::CredUser(addr) => {
                self.asks.insert(api::prompt_secret(&format!("{}: password for {v}", addr.key())), Ask::CredPassword(addr, v));
                Ok(())
            }
            Ask::CredPassword(addr, user) => self.keep_auth(&addr, json!({ "method": "user", "user": user, "password": value })),
            Ask::CredToken(addr) => self.keep_auth(&addr, json!({ "method": "token", "token": v })),
            Ask::CredSeed(addr) => {
                fenix_nats::auth::KeyPair::from_seed(&v)?;
                self.keep_auth(&addr, json!({ "method": "nkey", "seed": v }))
            }
            Ask::CredFile(addr) => {
                let bytes = api::read_file(&format!("{CREDS}/{v}"))?;
                fenix_nats::auth::parse_creds(&String::from_utf8_lossy(&bytes))?;
                self.keep_auth(&addr, json!({ "method": "creds", "file": v }))
            }
        })();
        if let Err(e) = result {
            host::notify(Level::Warn, &format!("NATS: {e}"));
            self.live.note = Some((e, Role::Bad));
        }
        self.drain();
        self.core.dirty = true;
        self.refresh(true);
    }

    fn ask_method(&mut self, addr: Addr) {
        let items = [
            PickerItem::new("User and password", "user"),
            PickerItem::new("Token", "token"),
            PickerItem::new("NKey seed", "nkey"),
            PickerItem::new(".creds file (JWT and NKey)", "creds"),
            PickerItem::new("None: forget what's kept", "none"),
        ];
        self.asks.insert(api::pick(&format!("{}: sign in with", addr.key()), &items), Ask::CredMethod(addr));
    }

    fn keep_auth(&mut self, addr: &Addr, v: serde_json::Value) -> Result<(), String> {
        api::set_secret(&auth_key(addr), Some(&v.to_string()))?;
        self.reconnect(addr);
        Ok(())
    }

    fn reconnect(&mut self, addr: &Addr) {
        if let Some(i) = self.core.conns.iter().position(|c| c.addr.key() == addr.key()) {
            let mut net = net();
            let a = auth_for(addr).unwrap_or_default();
            self.core.connect(&mut net, i, a, now_us());
            api::notify(&format!("NATS {}: signing in again", addr.key()));
        }
    }

    fn js_action(&mut self, a: JsAction) {
        match a {
            JsAction::None => {}
            JsAction::AskSeq(s) => {
                self.asks.insert(api::prompt(&format!("{s}: read from sequence"), "1"), Ask::JsSeq(s));
            }
            JsAction::AskTime(s) => {
                self.asks.insert(api::prompt(&format!("{s}: read from (2024-05-01 10:00, 10:00, 15m ago)"), "1h ago"), Ask::JsTime(s));
            }
            JsAction::ConfirmPurge(s) => {
                self.asks.insert(api::prompt(&format!("Purge every message of {s}? Type {s}"), ""), Ask::Purge(s));
            }
            JsAction::ConfirmDelete(s, seq) => {
                self.asks.insert(api::prompt(&format!("Delete {s} #{seq}? y"), ""), Ask::Delete(s, seq));
            }
            JsAction::AskPut(b) => {
                self.asks.insert(api::prompt(&format!("{b}: put at key"), ""), Ask::PutKey(b));
            }
            JsAction::ConfirmKvDelete(b, k) => {
                self.asks.insert(api::prompt(&format!("Delete {k} from {b}? y"), ""), Ask::KvDelete(b, k));
            }
            JsAction::Copy(t) => {
                api::copy(&t);
                self.js.note = Some(("copied".into(), Role::Muted));
            }
            JsAction::Open(text, lang) => api::open_text("NATS value", &text, lang),
            JsAction::Live => self.open_live(),
        }
    }

    fn serve_tool(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let now = now_us();
        match name {
            "nats.subjects" => {
                let v: serde_json::Value = serde_json::from_str(if payload.trim().is_empty() { "{}" } else { payload }).unwrap_or_default();
                let pattern = v["subject"].as_str().unwrap_or(">");
                let i = match self.conn_for(v["server"].as_str()) {
                    Some(i) => i,
                    None => return Some(Err("no NATS connection: nats.servers lists none".into())),
                };
                let mut out = tools::subjects(&self.core, self.core.conns[i].id, pattern, v["limit"].as_u64().unwrap_or(50).min(200) as usize, now / 1000);
                out["server"] = json!(self.core.conns[i].addr.key());
                out["state"] = json!(self.core.conns[i].state());
                Some(Ok(out.to_string()))
            }
            "nats.sample" => {
                let args = match tools::sample_args(payload) {
                    Ok(a) => a,
                    Err(e) => return Some(Err(e)),
                };
                self.connect_all();
                let Some(i) = self.conn_for(args.server.as_deref()) else { return Some(Err("no NATS connection: nats.servers lists none".into())) };
                let conn = self.core.conns[i].id;
                let found = tools::stored(&self.core, conn, &args.subject, args.count);
                let server = self.core.conns[i].addr.key();
                if found.len() >= args.count || args.wait_ms == 0 || !self.core.conns[i].is_up() {
                    return Some(Ok(json!({ "server": server, "messages": found, "count": found.len() }).to_string()));
                }
                let mut net = net();
                let want = args.count - found.len();
                match self.core.sample(&mut net, i, &args.subject, call, want, now + args.wait_ms * 1000) {
                    Ok(()) => {
                        self.samples.insert(call, ToolSample { server, found });
                        None
                    }
                    Err(e) => Some(Err(e)),
                }
            }
            "nats.publish" => {
                let args = match tools::publish_args(payload) {
                    Ok(a) => a,
                    Err(e) => return Some(Err(e)),
                };
                let Some(i) = self.conn_for(args.server.as_deref()) else { return Some(Err("no NATS connection".into())) };
                let mut net = net();
                Some(self.core.conns[i].publish(&mut net, &args.subject, None, &args.headers, &args.payload).map(|_| json!({ "published": args.subject, "bytes": args.payload.len(), "server": self.core.conns[i].addr.key() }).to_string()))
            }
            "nats.request" => {
                let args = match tools::publish_args(payload) {
                    Ok(a) => a,
                    Err(e) => return Some(Err(e)),
                };
                let Some(i) = self.conn_for(args.server.as_deref()) else { return Some(Err("no NATS connection".into())) };
                let mut net = net();
                match self.core.conns[i].request(&mut net, &args.subject, &args.headers, &args.payload, Pending::Tool { call, started_us: now }, now + 10_000_000) {
                    Ok(()) => None,
                    Err(e) => Some(Err(e)),
                }
            }
            _ => Some(Err(format!("no service {name}"))),
        }
    }
}

/// A time to read from: RFC 3339, `2024-05-01 10:00`, `10:00` today, or
/// `15m ago` -- as RFC 3339, UTC.
fn parse_time(text: &str, offset_s: i64) -> Option<String> {
    let t = text.trim();
    let now_ms = now_us() / 1000;
    if let Some(ago) = t.strip_suffix("ago").map(str::trim) {
        let (n, unit) = ago.split_at(ago.find(|c: char| !c.is_ascii_digit())?);
        let n: u64 = n.parse().ok()?;
        let ms = match unit.trim() {
            "s" | "sec" | "seconds" => n * 1000,
            "m" | "min" | "minutes" => n * 60_000,
            "h" | "hours" => n * 3_600_000,
            "d" | "days" => n * 86_400_000,
            _ => return None,
        };
        return Some(payload::rfc3339(now_ms.saturating_sub(ms)));
    }
    if t.contains('T') && (t.ends_with('Z') || t.contains('+')) {
        return Some(t.to_string());
    }
    let (date, clock) = match t.split_once(' ') {
        Some((d, c)) => (Some(d), c),
        None if t.contains(':') => (None, t),
        None => (Some(t), "00:00"),
    };
    let mut hm = clock.split(':').map(|x| x.parse::<i64>().ok());
    let (h, m) = (hm.next()??, hm.next().flatten().unwrap_or(0));
    let s = hm.next().flatten().unwrap_or(0);
    let day_ms: i64 = match date {
        Some(d) => {
            let mut p = d.split('-').map(|x| x.parse::<i64>().ok());
            let (y, mo, da) = (p.next()??, p.next()??, p.next()??);
            days_from_civil(y, mo, da) * 86_400_000
        }
        None => ((now_ms as i64 + offset_s * 1000).div_euclid(86_400_000)) * 86_400_000,
    };
    let local = day_ms + ((h * 60 + m) * 60 + s) * 1000;
    Some(payload::rfc3339((local - offset_s * 1000).max(0) as u64))
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

impl Extension for Nats {
    fn activate(&mut self) -> Result<(), String> {
        self.live.offset_s = api::clock::now().offset().local_minus_utc() as i64;
        self.js.offset_s = self.live.offset_s;
        self.load_saved();
        self.configure();
        self.connect_all();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "nats.open" => self.open_live(),
            "nats.jetstream" => self.open_js(),
            "nats.publish" => self.action(Action::Publish),
            "nats.send" => {
                self.send(false)?;
            }
            "nats.request" => {
                self.send(true)?;
            }
            "nats.subscribe" => self.action(Action::Subscribe),
            "nats.filter" => self.action(Action::Filter),
            "nats.rules" => self.action(Action::Rules),
            "nats.export" => self.action(Action::ExportView),
            "nats.connect" => {
                if self.core.conns.is_empty() {
                    return Err("nats.servers lists no servers: SPC , adds them".into());
                }
                let items: Vec<PickerItem> = self.core.conns.iter().map(|c| PickerItem::new(c.addr.key(), c.addr.key()).detail(format!("{} · {}", c.state(), c.addr.url))).collect();
                self.asks.insert(api::pick("NATS: connect or disconnect", &items), Ask::Toggle);
            }
            "nats.credentials" => {
                match self.core.conns.len() {
                    0 => return Err("nats.servers lists no servers: SPC , adds them".into()),
                    1 => {
                        let a = self.core.conns[0].addr.clone();
                        self.ask_method(a);
                    }
                    _ => {
                        let items: Vec<PickerItem> = self.core.conns.iter().map(|c| PickerItem::new(c.addr.key(), c.addr.key()).detail(c.auth.name())).collect();
                        self.asks.insert(api::pick("NATS: credentials for", &items), Ask::CredServer);
                    }
                }
            }
            _ => return Err(format!("no command {id}")),
        }
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::SocketData((s, data)) => {
                let start = Instant::now();
                let now = now_us();
                let mut net = net();
                self.core.on_data(&mut net, s, &data, now);
                let spent = start.elapsed().as_micros() as u64;
                self.core.spent(now, spent);
                if !self.core.events.is_empty() || !self.core.replies.is_empty() {
                    self.drain();
                }
            }
            Event::Timer(t) if Some(t) == self.timer => self.tick(),
            Event::SocketConnected((s, result)) => {
                let mut net = net();
                self.core.on_connected(&mut net, s, result, now_us());
                self.drain();
            }
            Event::SocketClosed((s, why)) => {
                let mut net = net();
                self.core.on_closed(&mut net, s, why, now_us());
                self.drain();
            }
            Event::Prompted((id, v)) | Event::Picked((id, v)) | Event::Composed((id, v)) => self.answered(id, v),
            Event::PageShown((p, shown)) => {
                if shown {
                    self.shown.insert(p);
                    self.core.dirty = true;
                } else {
                    self.shown.remove(&p);
                }
            }
            Event::SettingChanged(key) if key.starts_with("nats.") => {
                self.configure();
                if key == "nats.servers" && self.live_page.is_some() {
                    self.connect_all();
                }
                self.core.dirty = true;
            }
            _ => {}
        }
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        // Being drawn means being seen.
        self.shown.insert(page);
        let now_ms = now_us() / 1000;
        if Some(page) == self.live_page {
            self.live.render(&self.core, width, height, now_ms)
        } else if Some(page) == self.js_page {
            self.js.render(&self.core, width, height)
        } else {
            Page::new().line([api::page::muted("This page is gone: SPC o m m opens NATS again.")])
        }
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let now = now_us();
        if Some(page) == self.live_page {
            let a = self.live.key(&mut self.core, key, now / 1000);
            self.action(a);
        } else if Some(page) == self.js_page {
            let mut net = net();
            let a = self.js.key(&mut self.core, &mut net, key, now);
            self.js_action(a);
        }
        self.core.dirty = true;
    }

    fn page_key_press(&mut self, page: u64, key: KeyPress) {
        if Some(page) != self.live_page {
            return;
        }
        let now_ms = now_us() / 1000;
        match key.key {
            SurfaceKey::Named(NamedKey::PageDown) => self.live.page(&self.core, now_ms, true),
            SurfaceKey::Named(NamedKey::PageUp) => self.live.page(&self.core, now_ms, false),
            _ => {}
        }
        self.core.dirty = true;
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        if Some(page) != self.live_page {
            return;
        }
        let now_ms = now_us() / 1000;
        match event.action {
            MouseAction::Press(p) => self.live.mouse(&self.core, now_ms, event.line as usize, p.clicks, 0),
            MouseAction::Wheel(w) => self.live.mouse(&self.core, now_ms, event.line as usize, 0, w),
            _ => {}
        }
        self.core.dirty = true;
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        if Some(page) == self.live_page {
            self.live.paste(text);
        }
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        if Some(page) == self.live_page {
            Some(json!({ "tree": self.live.tree, "filter": self.live.structured.as_ref().map(|f| f.source.clone()) }).to_string())
        } else if Some(page) == self.js_page {
            Some("{}".into())
        } else {
            None
        }
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        let v: serde_json::Value = serde_json::from_str(state).unwrap_or_default();
        match kind {
            "live" => {
                self.live_page = Some(page);
                self.live.tree = v["tree"].as_bool().unwrap_or(false);
                if let Some(f) = v["filter"].as_str() {
                    let _ = self.set_filter(f);
                }
                self.connect_all();
                true
            }
            "jetstream" => {
                self.js_page = Some(page);
                let mut net = net();
                self.js.reload(&mut self.core, &mut net, now_us());
                true
            }
            _ => false,
        }
    }

    fn page_closed(&mut self, page: u64) {
        if Some(page) == self.live_page {
            self.live_page = None;
        }
        if Some(page) == self.js_page {
            self.js_page = None;
            if let Some(b) = self.js.watching.take() {
                if let Some(i) = self.core.conn_index(self.js.conn) {
                    let mut net = net();
                    let _ = self.core.watch_bucket(&mut net, i, &b, false);
                }
            }
        }
        self.shown.remove(&page);
    }

    fn scratch_closed(&mut self, scratch: u64) {
        if Some(scratch) == self.scratch {
            self.scratch = None;
        }
    }

    fn serve(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let r = self.serve_tool(call, name, payload);
        self.drain();
        r
    }

    fn service_cancelled(&mut self, call: u64) {
        self.samples.remove(&call);
        if let Some(s) = self.core.samples.remove(&call) {
            if let Some(i) = self.core.conn_index(s.conn) {
                let mut net = net();
                self.core.conns[i].unsubscribe_route(&mut net, s.sid);
            }
        }
    }
}

export!(Nats);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_to_read_from_are_understood() {
        assert_eq!(parse_time("2024-05-01T10:00:00Z", 0).as_deref(), Some("2024-05-01T10:00:00Z"));
        assert_eq!(parse_time("2024-05-01 10:30", 3600).as_deref(), Some("2024-05-01T09:30:00.000Z"));
        assert_eq!(parse_time("2024-05-01", 0).as_deref(), Some("2024-05-01T00:00:00.000Z"));
        assert!(parse_time("15m ago", 0).is_some());
        assert!(parse_time("10:00", 0).unwrap().ends_with("T10:00:00.000Z"));
        assert!(parse_time("whenever", 0).is_none());
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }
}
