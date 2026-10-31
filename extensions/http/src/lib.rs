//! An HTTP client in Fenix. Requests are `.http` files in the project, as
//! VS Code's REST Client and JetBrains' HTTP client write them: run the
//! one under the cursor (`SPC p r r`, `SPC m r`, or a click on its `▶`)
//! and its status shows after it, its response beside. The client's page
//! (`SPC p r o`) has the collections -- the `.http` files, by folder --
//! and the history in a sidebar, the request with tabs whose every field
//! edits its `.http` text, and the response with its body, headers,
//! cookies, timing and tests.
//!
//! Environments come from `http-client.env.json` and its private twin;
//! secrets from the system's keychain. Auth is Basic, Bearer, API keys, and
//! OAuth 2.0 from the env file (authorization code with PKCE, its redirect
//! listened for on the loopback; client credentials, password, device
//! code), its tokens refreshed and kept in the keychain. WebSocket
//! requests keep their connection open, and a GraphQL query completes
//! from its server's schema. Requests
//! chain (`{{login.response.body.$.token}}`); `# @test` lines test an
//! answer, and the runner sends a file's or a folder's and reports. It
//! imports Postman, Insomnia, OpenAPI and `curl`, and exports to `curl`
//! and code.

mod assert;
mod auth;
mod body;
mod complete;
mod cookies;
mod edit;
mod env;
mod export;
mod graphql;
mod import;
mod json;
mod model;
mod parse;
mod pretty;
mod secrets;
mod send;
mod store;
mod ui;
mod url;
mod vars;
mod ws;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use fenix_extension_api::lsp::types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse, CompletionTextEdit, Diagnostic, DiagnosticSeverity, Position, Range, ServerCapabilities, TextEdit as LspEdit, Uri,
};
use fenix_extension_api::lsp::{self, Documents, LanguageServer, Servers};
use fenix_extension_api::{self as api, decorations, export, host, page as pages, socket, Changed, Decoration, Event, Extension, Focus, HttpOptions, HttpPart, HttpRequest, Key, KeyPress, MouseEvent, Page, PickerItem, Role, TextBuffer, TextEdit, TlsOptions};
use fenix_ui_kit::Key as UiKey;

use auth::{OAuth, Pkce, Token, TokenAnswer, TokenGrant};
use env::Environments;
use model::{Exchange, Received, Sent, Timing};
use parse::{After, HttpFile};
use send::Opts;
use ui::client::{Action, ClientPage, FileEntry, ReqTab};
use ui::report::{Report, RunResult};
use ui::response::{ResponseView, Shown};

const CLIENT: &str = "client";
/// The secrets' names, for completion: their values are in the keychain.
const SECRET_NAMES: &str = "/data/secret-names.json";
/// How long a sign-in waits for the browser's redirect.
const CALLBACK_MS: u32 = 5 * 60 * 1000;
const RESPONSE: &str = "response";
const REPORT: &str = "report";

/// Folders never looked in for `.http` files.
const SKIP: &[&str] = &["node_modules", "target", "dist", "build", "vendor", "__pycache__", "venv", "bin", "obj"];

fn is_http(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".http") || p.ends_with(".rest")
}

fn is_env_file(path: &str) -> bool {
    path.ends_with(env::PUBLIC) || path.ends_with(env::PRIVATE)
}

fn project(rel: &str) -> String {
    format!("/project/{rel}")
}

fn folder(rel: &str) -> &str {
    rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// A path a `.http` file names (`./logo.png`, `../x`), below the project.
fn beside(rel: &str, path: &str) -> String {
    let path = path.trim();
    if let Some(p) = path.strip_prefix("/project/") {
        return p.to_string();
    }
    let mut parts: Vec<&str> = folder(rel).split('/').filter(|p| !p.is_empty()).collect();
    for part in path.trim_start_matches('/').split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn setting_on(key: &str, default: bool) -> bool {
    match api::setting(key).as_deref().map(str::trim) {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

fn setting_int(key: &str, default: u64) -> u64 {
    api::setting(key).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

fn error(message: &str) {
    host::notify(api::Level::Error, message);
}

fn now_millis() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    if getrandom::fill(&mut b).is_err() {
        // No randomness from the host: the clock, stirred.
        let mut x = now_millis() as u64 ^ 0x9e37_79b9_7f4a_7c15;
        for v in b.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *v = x as u8;
        }
    }
    b
}

/// The secrets, from `http.secrets`: `name=value` pairs, a `;` or a line
/// between them.
pub fn parse_secrets(text: &str) -> Vec<(String, String)> {
    text.split([';', '\n']).filter_map(|p| p.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).filter(|(k, _)| !k.is_empty()).collect()
}


/// What chained requests and the system variables read while one is made
/// ready.
struct SendCx<'a> {
    file: &'a HttpFile,
    envs: &'a Environments,
    env: Option<&'a str>,
    secrets: &'a [(String, String)],
    last: &'a HashMap<String, Exchange>,
    tokens: &'a BTreeMap<String, Token>,
    dotenv: Vec<(String, String)>,
    now: i64,
}

impl vars::Context for SendCx<'_> {
    fn file_var(&self, name: &str) -> Option<String> {
        self.file.var(name).map(String::from)
    }
    fn env_var(&self, name: &str) -> Option<String> {
        self.envs.var(self.env, name).map(|v| v.0)
    }
    fn secret(&self, name: &str) -> Option<String> {
        secrets::lookup(name, self.env, &|k| api::secret(k), self.secrets)
    }
    fn process_env(&self, name: &str) -> Option<String> {
        api::env_var(name)
    }
    fn response(&self, name: &str) -> Option<&Exchange> {
        self.last.get(name)
    }
    fn dotenv(&self, name: &str) -> Option<String> {
        self.dotenv.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }
    fn token(&self, id: &str) -> Option<String> {
        let t = self.tokens.get(&store::token_key(self.env, id))?;
        let id_token = self.envs.auth(self.env, id).is_some_and(|c| matches!(c.get("Use ID Token"), Some(serde_json::Value::Bool(true))));
        t.good(self.now / 1000).then(|| t.value(id_token).to_string())
    }
    fn now_millis(&self) -> i64 {
        self.now
    }
    fn utc_offset(&self) -> i32 {
        api::clock::offset_at(api::chrono::Utc::now()).local_minus_utc()
    }
    fn random(&mut self) -> u64 {
        u64::from_le_bytes(random::<8>())
    }
}

/// A request to send: which, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Job {
    file: String,
    index: usize,
    label: String,
    origin: Origin,
    /// How many requests it's the answer to: a chain's.
    depth: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// From the buffer: the response opens beside.
    Buffer,
    /// From the client's page.
    Page,
    Runner,
    /// Another request needs its answer.
    Chain,
}

/// What a request in flight is for.
enum Pending {
    Request { job: Job, sent: Sent, opts: Opts, partial: Option<Received>, env: Option<String>, name: Option<String> },
    /// The token endpoint, for configuration `key`.
    Token { key: String, oauth: Box<OAuth>, refreshing: bool, old_refresh: Option<String>, device: Option<auth::Device> },
    /// The device grant's start.
    Device { key: String, oauth: Box<OAuth> },
    /// A GraphQL server's schema, asked for: by `env` and the URL as it's
    /// written; said aloud when asked for by its command.
    Schema { key: String, url: String, quiet: bool },
}

/// An authorization code grant waiting for the browser's redirect, on a
/// port of the loopback.
struct Callback {
    key: String,
    oauth: Box<OAuth>,
    state: String,
    verifier: Option<String>,
    path: String,
    timer: u64,
}

/// A WebSocket request's connection, by its socket.
struct WsConn {
    job: Job,
    sent: Sent,
    opts: Opts,
    env: Option<String>,
    name: Option<String>,
    key: String,
    target: ws::Target,
    /// The handshake's answer so far, until it's all come.
    head: Vec<u8>,
    open: bool,
    reader: ws::Reader,
    queue: VecDeque<ws::Message>,
    /// The server has said something since the last message went.
    heard: bool,
    /// Both ways, as it went: what the response's body shows.
    transcript: String,
    got: Received,
    started: i64,
    closing: bool,
}

/// What a request waits for before it's sent.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Wait {
    Request(String),
    Token(String),
}

/// A request's status, after it in the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Inline {
    Sending,
    Done { status: u16, micros: u64, size: usize, tests: (usize, usize) },
    Failed(String),
}

impl WsConn {
    fn job_key(&self) -> (String, String) {
        (self.job.file.clone(), self.job.label.clone())
    }

    /// A line of the transcript: `→` sent, `←` received, `×` how it ended.
    fn line(&mut self, way: char, text: &str) {
        let at = api::chrono::DateTime::from_timestamp_millis(now_millis()).map(|t| t.with_timezone(&api::clock::offset_at(t)).format("%H:%M:%S%.3f").to_string()).unwrap_or_default();
        self.transcript.push_str(&format!("{way} {at}  {}\n", text.replace('\n', "\n                ")));
    }

    /// What's shown of it so far: the handshake's answer, the transcript
    /// as its body.
    fn exchange(&self) -> Exchange {
        let mut got = self.got.clone();
        got.body = self.transcript.clone().into_bytes();
        got.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type"));
        got.headers.push(("Content-Type".into(), "text/plain; charset=utf-8".into()));
        got.timing.total = ((now_millis() - self.started).max(0) as u64) * 1000;
        Exchange { sent: self.sent.clone(), answer: Ok(got), at: self.started / 1000, file: Some(self.job.file.clone()), name: self.name.clone(), env: self.env.clone() }
    }
}

/// What a picker or a prompt was for.
enum Ask {
    Env,
    Method { file: String, index: usize },
    Row { file: String, index: usize, tab: ReqTab, row: Option<usize> },
    BodyType { file: String, index: usize },
    Auth { file: String, index: usize },
    AuthValue { file: String, index: usize, kind: String },
    CopyAs(Sent),
    SaveHistory(usize),
    SaveHistoryNew(usize),
    NewRequest(String),
    NewFile,
    Code { key: String, oauth: Box<OAuth>, state: String, verifier: Option<String> },
    OAuthPassword { key: String, oauth: Box<OAuth> },
    SignIn,
    SecretName,
    SecretValue(String),
    ImportPath,
    ImportInto(Box<import::Collection>),
    CurlText,
    SaveBody(Vec<u8>),
    /// Which open WebSocket a message goes to; then the message.
    WsPick,
    WsMessage(u64),
}

/// A timer's job.
enum Timed {
    DevicePoll { key: String, oauth: Box<OAuth>, device: auth::Device },
    /// A redirect that never came: the listener given up.
    Callback(u64),
}

struct Runner {
    queue: VecDeque<Job>,
    page: u64,
}

struct ResponsePage {
    key: (String, String),
    view: ResponseView,
}

#[derive(Default)]
struct Core {
    root: Option<String>,
    store: store::Store,
    store_loaded: bool,
    files: Vec<FileEntry>,
    scanned: bool,
    last: HashMap<String, Exchange>,
    inline: HashMap<(String, String), Inline>,
    shown: HashMap<(String, String), Shown>,
    pending: HashMap<u64, Pending>,
    waiting: Vec<(Wait, Job)>,
    asks: HashMap<u64, Ask>,
    timers: HashMap<u64, Timed>,
    /// OAuth flows under way, by token key.
    flows: Vec<String>,
    runner: Option<Runner>,
    /// What the GUI wrote to a file that Fenix hasn't told back yet.
    texts: HashMap<String, String>,
    server_ids: Vec<u64>,
    /// The secrets' names (never their values), as the keychain can't
    /// list them: for completion.
    secret_names: Vec<String>,
    /// The tokens as last written to the keychain.
    tokens_kept: BTreeMap<String, Token>,
    /// Loopback listeners waiting for an OAuth redirect, by listener; the
    /// browser's connections to them, and what they've sent.
    callbacks: HashMap<u64, Callback>,
    callback_sockets: HashMap<u64, (u64, Vec<u8>)>,
    ws: HashMap<u64, WsConn>,
    /// GraphQL schemas read, by environment and the URL as written; those
    /// asked for once already, so a keystroke doesn't ask again.
    schemas: HashMap<String, Arc<graphql::Schema>>,
    schemas_asked: HashSet<String>,
}

#[derive(Default)]
pub struct Http {
    servers: Servers,
    core: Core,
    clients: HashMap<u64, ClientPage>,
    responses: HashMap<u64, ResponsePage>,
    reports: HashMap<u64, Report>,
}

impl Core {
    fn root(&mut self) -> Option<String> {
        if self.root.is_none() {
            self.root = api::here().and_then(|h| h.project).map(|p| p.root);
        }
        self.root.clone()
    }

    fn store_dir(&mut self) -> Option<String> {
        self.root().map(|r| store::dir(&r))
    }

    fn load_store(&mut self) {
        if self.store_loaded {
            return;
        }
        let Some(dir) = self.store_dir() else { return };
        self.store = store::Store::load(&dir);
        self.store_loaded = true;
        // Its tokens, from the keychain; an older file's are moved there
        // when it's next saved.
        let moved = !self.store.tokens.is_empty();
        for k in std::mem::take(&mut self.store.kept_tokens) {
            match api::secret(&secrets::token_key(&dir, &k)).and_then(|t| serde_json::from_str::<Token>(&t).ok()) {
                Some(t) => {
                    self.tokens_kept.insert(k.clone(), t.clone());
                    self.store.tokens.entry(k).or_insert(t);
                }
                None => {
                    // Gone from the keychain: signed in again when needed.
                }
            }
        }
        if moved {
            self.save_store();
        }
        // The last answer to each named request, for chains, from history.
        for e in self.store.history.iter().rev() {
            if let Some(n) = &e.name {
                self.last.insert(n.clone(), e.clone());
            }
        }
    }

    fn save_store(&mut self) {
        let now = now_millis() / 1000;
        for jar in self.store.cookies.values_mut() {
            jar.purge(now);
        }
        let Some(dir) = self.store_dir() else { return };
        // Tokens to the keychain, those that changed; the file keeps their
        // keys, and a token only when the keychain wouldn't take it.
        let tokens = std::mem::take(&mut self.store.tokens);
        let mut on_file = BTreeMap::new();
        for (k, t) in &tokens {
            if self.tokens_kept.get(k) == Some(t) {
                continue;
            }
            let json = serde_json::to_string(t).unwrap_or_default();
            match api::set_secret(&secrets::token_key(&dir, k), Some(&json)) {
                Ok(()) => {
                    self.tokens_kept.insert(k.clone(), t.clone());
                }
                Err(e) => {
                    api::warn!("token {k} kept in the project's store, not the keychain: {e}");
                    on_file.insert(k.clone(), t.clone());
                }
            }
        }
        let gone: Vec<String> = self.tokens_kept.keys().filter(|k| !tokens.contains_key(*k)).cloned().collect();
        for k in gone {
            self.tokens_kept.remove(&k);
            if let Err(e) = api::set_secret(&secrets::token_key(&dir, &k), None) {
                api::warn!("token {k}: {e}");
            }
        }
        self.store.kept_tokens = self.tokens_kept.keys().cloned().collect();
        self.store.tokens = on_file;
        if let Err(e) = self.store.save(&dir) {
            api::warn!("{e}");
        }
        self.store.tokens = tokens;
    }

    /// A path below the project: `/project/x`, or a full path in it.
    fn rel(&mut self, path: &str) -> Option<String> {
        let path = path.replace('\\', "/");
        if let Some(r) = path.strip_prefix("/project/") {
            return Some(r.to_string());
        }
        let root = self.root()?.replace('\\', "/");
        let root = root.trim_end_matches('/');
        path.strip_prefix(root).and_then(|r| r.strip_prefix('/')).map(String::from)
    }

    /// File `rel`'s text: what the GUI just wrote, its buffer's, or what's
    /// on disk.
    fn text(&self, rel: &str) -> Option<String> {
        self.texts.get(rel).cloned().or_else(|| api::buffer::text(&project(rel))).or_else(|| std::fs::read_to_string(project(rel)).ok())
    }

    /// The env files for `.http` file `rel`: the nearest folder that has
    /// one, up to the project.
    fn envs_for(&self, rel: &str) -> (Environments, String) {
        for dir in env::folders(rel) {
            let at = |name: &str| if dir.is_empty() { project(name) } else { project(&format!("{dir}/{name}")) };
            let public = api::buffer::text(&at(env::PUBLIC)).or_else(|| std::fs::read_to_string(at(env::PUBLIC)).ok());
            let private = api::buffer::text(&at(env::PRIVATE)).or_else(|| std::fs::read_to_string(at(env::PRIVATE)).ok());
            if public.is_some() || private.is_some() {
                match Environments::parse(public.as_deref(), private.as_deref()) {
                    Ok(e) => return (e, dir),
                    Err(e) => {
                        api::warn!("{e}");
                        return (Environments::default(), dir);
                    }
                }
            }
        }
        (Environments::default(), String::new())
    }

    /// The environment chosen, when the env files have it.
    fn env(&self, envs: &Environments) -> Option<String> {
        let names = envs.names();
        self.store.env.clone().filter(|e| names.contains(e)).or_else(|| (names.len() == 1).then(|| names[0].clone()))
    }

    /// Where a GraphQL schema read is kept, by its key.
    fn schema_path(&mut self, key: &str) -> Option<String> {
        let dir = self.store_dir()?;
        let hex: String = <sha2::Sha256 as sha2::Digest>::digest(key.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect();
        Some(format!("{dir}/graphql/{hex}.json"))
    }

    /// The key a GraphQL request's schema goes by: its environment, and its
    /// URL as it's written (`{{api}}/graphql`).
    fn schema_key(env: Option<&str>, url: &str) -> String {
        format!("{}\n{url}", env.unwrap_or(""))
    }

    /// The schema for GraphQL request `index` of `rel`: read before, kept
    /// on disk, or asked of its server -- once, so typing doesn't ask again.
    fn ensure_schema(&mut self, rel: &str, index: usize, url: &str) {
        let (envs, _) = self.envs_for(rel);
        let env = self.env(&envs);
        let key = Core::schema_key(env.as_deref(), url);
        if self.schemas.contains_key(&key) || !self.schemas_asked.insert(key.clone()) {
            return;
        }
        let cached = self.schema_path(&key).and_then(|p| std::fs::read(p).ok()).and_then(|b| graphql::Schema::from_introspection(&b).ok());
        match cached {
            Some(s) => {
                self.schemas.insert(key, Arc::new(s));
            }
            None => {
                if let Err(e) = self.fetch_schema(rel, index, true) {
                    api::warn!("GraphQL schema of {url}: {e}");
                }
            }
        }
    }

    /// GraphQL request `index` of `rel` sent as the introspection query --
    /// its URL, headers and auth filled in as for the request itself.
    fn fetch_schema(&mut self, rel: &str, index: usize, quiet: bool) -> Result<(), String> {
        self.load_store();
        let text = self.text(rel).ok_or_else(|| format!("{rel} can't be read"))?;
        let file = HttpFile::parse(&text);
        let r = file.requests.get(index).ok_or("no request there")?.clone();
        if !complete::is_graphql(&r) {
            return Err("not a GraphQL request: GRAPHQL as its method, or X-Request-Type: GraphQL".into());
        }
        let (envs, _) = self.envs_for(rel);
        let env = self.env(&envs);
        let secrets = self.secrets();
        let mut read = |_: &str| Err::<Vec<u8>, String>("no files in an introspection query".into());
        let prepared = {
            let mut cx = SendCx { file: &file, envs: &envs, env: env.as_deref(), secrets: &secrets, last: &self.last, tokens: &self.store.tokens, dotenv: self.dotenv(), now: now_millis() };
            let mut bare = r.clone();
            bare.body = None;
            send::prepare(&bare, &mut cx, &mut read, &send::Defaults::default())
        };
        let (mut sent, opts) = prepared.map_err(|m| match m {
            vars::Missing::Token(id) => format!("send the request once first, to sign in to {id}"),
            m => m.said(),
        })?;
        sent.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type") && !k.eq_ignore_ascii_case("accept"));
        sent.headers.push(("Content-Type".into(), "application/json".into()));
        sent.headers.push(("Accept".into(), "application/json".into()));
        let mut o = HttpOptions::new().timeout(opts.timeout_ms);
        if opts.insecure || setting_on("http.insecure", false) {
            o = o.accept_invalid_certs();
        }
        if let Some(proxy) = api::setting("http.proxy").filter(|p| !p.trim().is_empty()) {
            o = o.proxy(proxy.trim());
        }
        let id = (HttpRequest { method: "POST".into(), url: sent.url.clone(), headers: sent.headers, body: graphql::introspection_body(), options: o }).send()?;
        let key = Core::schema_key(env.as_deref(), &r.url);
        self.schemas_asked.insert(key.clone());
        self.pending.insert(id, Pending::Schema { key, url: sent.url, quiet });
        if !quiet {
            api::notify(&format!("GraphQL: asking {} for its schema", r.url));
        }
        Ok(())
    }

    fn schema_failed(&mut self, url: &str, quiet: bool, why: String) {
        let said = format!("GraphQL schema of {url}: {why}");
        if quiet {
            api::warn!("{said}");
        } else {
            error(&said);
        }
    }

    /// The older secrets, `http.secrets`' pairs: read after the keychain's.
    fn secrets(&self) -> Vec<(String, String)> {
        host::secret("http.secrets").or_else(|| api::setting("http.secrets")).map(|t| parse_secrets(&t)).unwrap_or_default()
    }

    /// Secret `name` kept in the keychain (or, with `None`, taken away),
    /// and its name with the others.
    fn keep_secret(&mut self, name: &str, value: Option<&str>) -> Result<(), String> {
        let key = secrets::key(name)?;
        api::set_secret(&key, value.filter(|v| !v.is_empty()))?;
        self.secret_names = secrets::with_name(&self.secret_names, name.trim(), value.is_some_and(|v| !v.is_empty()));
        secrets::save_names(SECRET_NAMES, &self.secret_names)
    }

    /// The older setting's secrets moved into the keychain, a secret each;
    /// the setting emptied once they all are.
    fn move_secrets(&mut self) {
        let old = self.secrets();
        if old.is_empty() {
            return;
        }
        let mut failed = 0;
        for (name, value) in &old {
            if let Err(e) = self.keep_secret(name, Some(value)) {
                api::warn!("secret {name} stays in http.secrets: {e}");
                failed += 1;
            }
        }
        if failed == 0 {
            if let Err(e) = api::set_setting("http.secrets", None) {
                api::warn!("http.secrets: {e}");
            }
            api::notify(&format!("Moved {} secret{} from http.secrets into the keychain", old.len(), if old.len() == 1 { "" } else { "s" }));
        }
    }

    fn dotenv(&self) -> Vec<(String, String)> {
        let Ok(text) = std::fs::read_to_string("/project/.env") else { return Vec::new() };
        text.lines()
            .filter_map(|l| {
                let l = l.trim().strip_prefix("export ").unwrap_or(l.trim());
                if l.starts_with('#') {
                    return None;
                }
                let (k, v) = l.split_once('=')?;
                Some((k.trim().to_string(), v.trim().trim_matches(['"', '\'']).to_string()))
            })
            .collect()
    }

    /// The project's `.http` files, looked for again.
    fn scan(&mut self) {
        fn walk(dir: &std::path::Path, rel: &str, depth: usize, out: &mut Vec<String>) {
            if depth > 8 || out.len() > 2000 {
                return;
            }
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let name = e.file_name().to_string_lossy().into_owned();
                let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
                    if !name.starts_with('.') && !SKIP.contains(&name.as_str()) {
                        walk(&e.path(), &child, depth + 1, out);
                    }
                } else if is_http(&name) {
                    out.push(child);
                }
            }
        }
        let mut found = Vec::new();
        walk(std::path::Path::new("/project"), "", 0, &mut found);
        self.files = found.into_iter().map(|rel| self.entry(&rel)).collect();
        self.scanned = true;
    }

    fn entry(&self, rel: &str) -> FileEntry {
        let text = self.text(rel).unwrap_or_default();
        let f = HttpFile::parse(&text);
        FileEntry { rel: rel.to_string(), requests: f.requests.iter().map(|r| (r.method.clone(), short_label(r), r.line)).collect() }
    }

    /// File `rel` read again for the collections, or added.
    fn rescan(&mut self, rel: &str) {
        let e = self.entry(rel);
        match self.files.iter_mut().find(|f| f.rel == rel) {
            Some(f) => *f = e,
            None => {
                self.files.push(e);
                self.files.sort_by(|a, b| a.rel.cmp(&b.rel));
            }
        }
    }

    fn files(&mut self) -> &[FileEntry] {
        if !self.scanned {
            self.scan();
        }
        &self.files
    }

    /// The statuses after each request of `rel`, and its `▶`s: a click on
    /// one sends it, on the status shows its response.
    fn decorate(&mut self, rel: &str) {
        let path = project(rel);
        let Some(text) = self.texts.get(rel).cloned().or_else(|| api::buffer::text(&path)) else { return };
        let f = HttpFile::parse(&text);
        let lines: Vec<&str> = text.split('\n').collect();
        let marks = setting_on("http.send_marks", true);
        let mut out = Vec::new();
        for (i, r) in f.requests.iter().enumerate() {
            let i = i as u32;
            if marks {
                out.push(Decoration::before((r.line, 0), "▶ ", Role::Accent).tagged(i * 2));
            }
            let last = r.line + r.url_lines - 1;
            let len = lines.get(last as usize).map(|l| l.trim_end_matches('\r').chars().count() as u32).unwrap_or(0);
            let at = (last, len.saturating_sub(1));
            let said = match self.inline.get(&(rel.to_string(), r.label())) {
                None => continue,
                Some(Inline::Sending) => ("  ⋯ sending".to_string(), Role::Muted),
                Some(Inline::Done { status, micros, size, tests }) => {
                    let reason = model::reason(*status);
                    let mut s = format!("  {status}{}{} · {} · {}", if reason.is_empty() { "" } else { " " }, reason, model::duration(*micros), model::size(*size));
                    let mut role = ui::response::status_role(*status);
                    if tests.1 > 0 {
                        s.push_str(&format!(" · tests {}/{}", tests.0, tests.1));
                        if tests.0 < tests.1 {
                            role = fenix_ui_kit::Role::Bad;
                        }
                    }
                    (s, (&role).into())
                }
                Some(Inline::Failed(e)) => (format!("  ✗ {}", e.chars().take(80).collect::<String>()), Role::Bad),
            };
            out.push(Decoration::after(at, said.0, said.1).tagged(i * 2 + 1));
        }
        if let Err(e) = decorations::set(&decorations::file(path), &out) {
            api::warn!("{rel}: {e}");
        }
    }

    /// Edits to `rel`, made in its buffer as one change you can undo, and
    /// saved when the setting says.
    fn edit(&mut self, rel: &str, edits: &[edit::Edit]) -> Result<(), String> {
        let path = project(rel);
        let text = self.text(rel).unwrap_or_default();
        let api_edits: Vec<TextEdit> = edits.iter().map(|e| TextEdit::new(e.start, e.end, e.text.clone())).collect();
        api::buffer::edit(&path, &api_edits)?;
        if setting_on("http.save_edits", true) {
            api::buffer::save(&path)?;
        }
        self.texts.insert(rel.to_string(), edit::apply(&text, edits));
        self.rescan(rel);
        self.decorate(rel);
        Ok(())
    }
}

/// A request's name in the tree: its name or title, else its path.
fn short_label(r: &parse::Request) -> String {
    match r.directive("name").map(|d| d.value.clone()).or_else(|| r.title.clone()).filter(|t| !t.is_empty()) {
        Some(t) => t,
        None => {
            let p = url::path(&r.url);
            if p == "/" {
                r.url.clone()
            } else {
                p
            }
        }
    }
}

impl Http {
    /// Request `job` sent, or why it can't be.
    fn run(&mut self, job: Job) {
        if let Err(e) = self.try_run(job.clone()) {
            self.failed(&job, e);
        }
    }

    fn try_run(&mut self, mut job: Job) -> Result<(), String> {
        self.core.load_store();
        let text = self.core.text(&job.file).ok_or_else(|| format!("{} can't be read", job.file))?;
        let file = HttpFile::parse(&text);
        // By its index, unless it moved: then by its label.
        if file.requests.get(job.index).map(|r| r.label()) != Some(job.label.clone()) {
            if let Some(i) = file.requests.iter().position(|r| r.label() == job.label) {
                job.index = i;
            }
        }
        let r = file.requests.get(job.index).ok_or("that request is gone")?.clone();
        job.label = r.label();
        let (envs, env_dir) = self.core.envs_for(&job.file);
        let env = self.core.env(&envs);
        let secrets = self.core.secrets();
        let defaults = send::Defaults { timeout_ms: (setting_int("http.timeout", 30) * 1000).min(u32::MAX as u64) as u32, follow_redirects: setting_on("http.follow_redirects", true), insecure: setting_on("http.insecure", false) || !envs.ssl(env.as_deref()).verify };
        let rel = job.file.clone();
        let mut read = |p: &str| api::read_file(&project(&beside(&rel, p)));
        let prepared = {
            let mut cx = SendCx { file: &file, envs: &envs, env: env.as_deref(), secrets: &secrets, last: &self.core.last, tokens: &self.core.store.tokens, dotenv: self.core.dotenv(), now: now_millis() };
            send::prepare(&r, &mut cx, &mut read, &defaults)
        };
        let (mut sent, opts) = match prepared {
            Ok(p) => p,
            Err(vars::Missing::Response(name)) => {
                if job.depth >= 4 {
                    return Err(format!("requests chain too deep at {name}"));
                }
                let dep = file.requests.iter().position(|q| q.name() == Some(name.as_str())).ok_or_else(|| format!("{name} hasn't been sent, and no request in this file is called that"))?;
                let waiting_already = self.core.waiting.iter().any(|(w, _)| *w == Wait::Request(name.clone()));
                self.core.waiting.push((Wait::Request(name.clone()), job.clone()));
                self.core.inline.insert((job.file.clone(), job.label.clone()), Inline::Sending);
                if !waiting_already {
                    let dep_job = Job { file: job.file.clone(), index: dep, label: file.requests[dep].label(), origin: Origin::Chain, depth: job.depth + 1 };
                    self.run(dep_job);
                }
                return Ok(());
            }
            Err(vars::Missing::Token(id)) => {
                let key = store::token_key(env.as_deref(), &id);
                self.core.waiting.push((Wait::Token(key.clone()), job.clone()));
                self.core.inline.insert((job.file.clone(), job.label.clone()), Inline::Sending);
                self.core.decorate(&job.file);
                self.need_token(&id, &job.file);
                return Ok(());
            }
            Err(m) => return Err(m.said()),
        };
        if opts.cookie_jar && sent.header("cookie").is_none() {
            if let Some(c) = self.core.store.jar(env.as_deref()).header(&sent.url, now_millis() / 1000) {
                sent.headers.push(("Cookie".into(), c));
            }
        }
        if sent.method == "WEBSOCKET" {
            let name = r.name().map(String::from);
            return self.open_ws(job, sent, opts, env, name);
        }
        let mut o = HttpOptions::new().timeout(opts.timeout_ms);
        if !opts.follow_redirects {
            o = o.no_redirects();
        }
        if opts.insecure {
            o = o.accept_invalid_certs();
        }
        let ssl = envs.ssl(env.as_deref());
        if let (Some(cert), Some(key)) = (&ssl.cert, &ssl.key) {
            let at = |p: &str| project(&beside(&format!("{env_dir}/x"), p));
            let read = |p: &str| api::read_file(&at(p)).map(|b| String::from_utf8_lossy(&b).into_owned()).map_err(|e| format!("client certificate {p}: {e}"));
            o = o.client_cert(&read(cert)?, &read(key)?);
        }
        if let Some(proxy) = api::setting("http.proxy").filter(|p| !p.trim().is_empty()) {
            o = o.proxy(proxy.trim());
        }
        if opts.stream {
            o = o.streamed();
        }
        let req = HttpRequest { method: sent.method.clone(), url: sent.url.clone(), headers: sent.headers.clone(), body: sent.body.clone(), options: o };
        let id = req.send()?;
        let key = (job.file.clone(), job.label.clone());
        self.core.inline.insert(key.clone(), Inline::Sending);
        let shown = self.core.shown.entry(key).or_default();
        shown.sending = true;
        shown.title = job.label.clone();
        self.core.decorate(&job.file);
        let name = r.name().map(String::from);
        self.core.pending.insert(id, Pending::Request { job, sent, opts, partial: None, env, name });
        self.refresh_pages();
        Ok(())
    }

    // WebSocket.

    /// A WebSocket request's connection opened: the handshake goes once
    /// it's connected, its messages once the server's said yes.
    fn open_ws(&mut self, job: Job, sent: Sent, opts: Opts, env: Option<String>, name: Option<String>) -> Result<(), String> {
        let target = ws::target(&sent.url)?;
        let socket = if target.tls {
            let mut tls = TlsOptions::new();
            if opts.insecure {
                tls = tls.accept_invalid();
            }
            socket::connect_tls(&target.host, target.port, &tls)?
        } else {
            socket::connect(&target.host, target.port)?
        };
        let key = auth::base64(&random::<16>());
        let queue = ws::messages(&String::from_utf8_lossy(&sent.body)).into();
        let got = Received { status: 0, headers: Vec::new(), body: Vec::new(), url: sent.url.clone(), timing: Timing::default() };
        let label = (job.file.clone(), job.label.clone());
        self.core.inline.insert(label.clone(), Inline::Sending);
        let shown = self.core.shown.entry(label).or_default();
        shown.sending = true;
        shown.title = job.label.clone();
        self.core.decorate(&job.file);
        self.core.ws.insert(socket, WsConn { job, sent, opts, env, name, key, target, head: Vec::new(), open: false, reader: ws::Reader::default(), queue, heard: false, transcript: String::new(), got, started: now_millis(), closing: false });
        self.refresh_pages();
        Ok(())
    }

    fn ws_connected(&mut self, socket: u64, result: Result<(), String>) {
        let Some(c) = self.core.ws.get(&socket) else { return };
        match result {
            Ok(()) => {
                let hs = ws::handshake(&c.target, &c.key, &c.sent.headers);
                if let Err(e) = socket::write(socket, &hs) {
                    self.ws_end(socket, Some(e));
                }
            }
            Err(e) => self.ws_end(socket, Some(e)),
        }
    }

    fn ws_data(&mut self, socket: u64, bytes: &[u8]) {
        let Some(c) = self.core.ws.get_mut(&socket) else { return };
        let mut frames = bytes.to_vec();
        if !c.open {
            c.head.extend_from_slice(bytes);
            let answer = match ws::read_answer(&c.head) {
                None => return,
                Some(Ok(a)) => a,
                Some(Err(e)) => return self.ws_end(socket, Some(e)),
            };
            if let Err(e) = ws::check_answer(&answer, &c.key) {
                return self.ws_end(socket, Some(e));
            }
            c.open = true;
            c.got.status = 101;
            c.got.headers = answer.headers.clone();
            c.got.timing.first_byte = ((now_millis() - c.started).max(0) as u64) * 1000;
            frames = c.head.split_off(answer.len);
            c.head.clear();
            let (key, ex) = (c.job_key(), c.exchange());
            let shown = self.core.shown.entry(key).or_default();
            shown.exchange = Some(ex);
            self.ws_send_queued(socket);
        }
        let Some(c) = self.core.ws.get_mut(&socket) else { return };
        let mut end: Option<Option<String>> = None;
        for e in c.reader.push(&frames) {
            if matches!(e, ws::Event::Text(_) | ws::Event::Binary(_)) {
                c.heard = true;
            }
            match e {
                ws::Event::Text(t) => c.line('←', &t),
                ws::Event::Binary(b) => c.line('←', &ws::show_binary(&b)),
                ws::Event::Ping(p) => {
                    let _ = socket::write(socket, &ws::frame(ws::Op::Pong, &p, random::<4>()));
                }
                ws::Event::Pong(_) => {}
                ws::Event::Close(code, reason) => {
                    if !c.closing {
                        let _ = socket::write(socket, &ws::frame(ws::Op::Close, &code.map(|c| ws::close_payload(c, "")).unwrap_or_default(), random::<4>()));
                    }
                    let why = match code {
                        Some(1000) | None if reason.is_empty() => None,
                        Some(code) => Some(format!("the server closed it: {code} {reason}").trim_end().to_string()),
                        None => Some(format!("the server closed it: {reason}")),
                    };
                    end = Some(why);
                    break;
                }
                ws::Event::Broken(why) => {
                    let _ = socket::write(socket, &ws::frame(ws::Op::Close, &ws::close_payload(1002, ""), random::<4>()));
                    end = Some(Some(why));
                    break;
                }
            }
        }
        self.ws_show(socket);
        match end {
            Some(why) => self.ws_end(socket, why),
            None => self.ws_send_queued(socket),
        }
    }

    /// The request's messages sent, up to one that waits for the server.
    fn ws_send_queued(&mut self, socket: u64) {
        let Some(c) = self.core.ws.get_mut(&socket) else { return };
        while let Some(m) = c.queue.front() {
            if m.wait && !c.heard {
                break;
            }
            let m = c.queue.pop_front().expect("looked at above");
            if let Err(e) = socket::write(socket, &ws::frame(ws::Op::Text, m.text.as_bytes(), random::<4>())) {
                return self.ws_end(socket, Some(e));
            }
            c.line('→', &m.text);
            c.heard = false;
        }
        self.ws_show(socket);
    }

    /// Message `text` sent on open WebSocket `socket`.
    fn ws_say(&mut self, socket: u64, text: &str) -> Result<(), String> {
        let c = self.core.ws.get_mut(&socket).filter(|c| c.open && !c.closing).ok_or("that WebSocket isn't open any more")?;
        socket::write(socket, &ws::frame(ws::Op::Text, text.as_bytes(), random::<4>()))?;
        c.line('→', text);
        c.heard = false;
        self.ws_show(socket);
        Ok(())
    }

    /// Its transcript, on the pages showing it.
    fn ws_show(&mut self, socket: u64) {
        let Some(c) = self.core.ws.get(&socket) else { return };
        if !c.open {
            return;
        }
        let (key, ex) = (c.job_key(), c.exchange());
        if let Some(s) = self.core.shown.get_mut(&key) {
            s.exchange = Some(ex);
        }
        for v in self.clients.values().map(|c| &c.response).chain(self.responses.values().map(|r| &r.view)) {
            v.forget();
        }
        self.refresh_pages();
    }

    /// A WebSocket closed: by the server, by you, or broken (`why`). Its
    /// transcript is its answer, kept as any is.
    fn ws_end(&mut self, socket: u64, why: Option<String>) {
        let Some(mut c) = self.core.ws.remove(&socket) else { return };
        socket::close(socket);
        if !c.open {
            let why = why.unwrap_or_else(|| "closed before the handshake was answered".into());
            return self.finish(c.job, c.sent, c.opts, c.env, c.name, Err(why));
        }
        if let Some(w) = &why {
            c.line('×', w);
        } else {
            c.line('×', "closed");
        }
        let got = c.exchange().answer.unwrap_or_default();
        self.finish(c.job, c.sent, c.opts, c.env, c.name, Ok(got));
    }

    /// The open WebSocket a message goes to: the one, or one picked.
    fn ws_pick(&mut self) -> Result<(), String> {
        let open: Vec<(u64, String)> = self.core.ws.iter().filter(|(_, c)| c.open && !c.closing).map(|(s, c)| (*s, c.job.label.clone())).collect();
        match &open[..] {
            [] => Err("no WebSocket is open: send a WEBSOCKET request first".into()),
            [(s, label)] => {
                let id = api::prompt(&format!("Message to {label}"), "");
                self.core.asks.insert(id, Ask::WsMessage(*s));
                Ok(())
            }
            _ => {
                let items: Vec<PickerItem> = open.iter().map(|(s, l)| PickerItem::new(l.clone(), s.to_string())).collect();
                let id = api::pick("Send to which WebSocket", &items);
                self.core.asks.insert(id, Ask::WsPick);
                Ok(())
            }
        }
    }

    /// Why `job` wasn't sent: after it, on its page, in its report.
    fn failed(&mut self, job: &Job, why: String) {
        let key = (job.file.clone(), job.label.clone());
        self.core.inline.insert(key.clone(), Inline::Failed(why.clone()));
        let shown = self.core.shown.entry(key).or_default();
        shown.sending = false;
        shown.exchange = Some(Exchange { sent: Sent::default(), answer: Err(why.clone()), at: now_millis() / 1000, file: Some(job.file.clone()), name: None, env: None });
        shown.tests.clear();
        self.core.decorate(&job.file);
        // What waited on it fails too.
        let name = self.core.text(&job.file).and_then(|t| HttpFile::parse(&t).requests.get(job.index).and_then(|r| r.name().map(String::from)));
        if let Some(name) = name {
            let (fail, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.core.waiting).into_iter().partition(|(w, _)| *w == Wait::Request(name.clone()));
            self.core.waiting = keep;
            for (_, j) in fail {
                self.failed(&j, format!("{name} failed: {why}"));
            }
        }
        match job.origin {
            Origin::Runner => self.ran(job, Err(why), Vec::new()),
            Origin::Chain => {}
            _ => error(&format!("{}: {why}", job.label)),
        }
        self.refresh_pages();
    }

    /// Its answer, come: kept, tested, shown.
    fn finish(&mut self, job: Job, sent: Sent, opts: Opts, env: Option<String>, name: Option<String>, answer: Result<Received, String>) {
        let now = now_millis() / 1000;
        let ex = Exchange { sent, answer, at: now, file: Some(job.file.clone()), name: name.clone(), env: env.clone() };
        let text = self.core.text(&job.file).unwrap_or_default();
        let file = HttpFile::parse(&text);
        let req = file.requests.iter().find(|r| r.label() == job.label).cloned();
        let mut tests = Vec::new();
        if let Ok(r) = &ex.answer {
            if opts.cookie_jar {
                self.core.store.jar(env.as_deref()).store(&r.url, r.headers_named("set-cookie"), now);
            }
            if let Some(req) = &req {
                tests = assert::check_all(req.tests().map(|d| d.value.as_str()), r, r.timing.total / 1000);
                for a in &req.after {
                    if let After::Save { path, overwrite, .. } = a {
                        if let Err(e) = save_body(&job.file, path, *overwrite, &r.body) {
                            error(&e);
                        }
                    }
                }
            }
        }
        if opts.log {
            self.core.store.remember(ex.clone(), setting_int("http.history", 200) as usize);
        }
        self.core.save_store();
        if let Some(n) = &name {
            self.core.last.insert(n.clone(), ex.clone());
        }
        let key = (job.file.clone(), job.label.clone());
        let inline = match &ex.answer {
            Ok(r) => Inline::Done { status: r.status, micros: r.timing.total, size: r.body.len(), tests: (tests.iter().filter(|t| t.passed).count(), tests.len()) },
            Err(e) => Inline::Failed(e.clone()),
        };
        self.core.inline.insert(key.clone(), inline);
        let jar = self.jar_for(&ex);
        let image = match &ex.answer {
            Ok(r) if pretty::kind(r.content_type(), &r.body) == pretty::Kind::Image => api::image::load(&r.body).ok().map(|i| (i.id, i.width, i.height)),
            _ => None,
        };
        let shown = self.core.shown.entry(key.clone()).or_default();
        if let Some((old, _, _)) = shown.image.take() {
            api::image::free(old);
        }
        *shown = Shown { exchange: Some(ex.clone()), tests: tests.clone(), jar, sending: false, image, title: job.label.clone() };
        self.core.decorate(&job.file);
        // What waited for its answer goes now.
        if let Some(n) = &name {
            let (go, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.core.waiting).into_iter().partition(|(w, _)| *w == Wait::Request(n.clone()));
            self.core.waiting = keep;
            for (_, j) in go {
                self.run(j);
            }
        }
        // A history entry sent again: its new answer shown, the history's
        // first now.
        if job.index == usize::MAX {
            for p in self.clients.values_mut().filter(|p| p.resent) {
                p.resent = false;
                p.viewing = opts.log.then_some(0);
            }
        }
        match job.origin {
            Origin::Buffer => self.show_response(key),
            Origin::Runner => {
                let answer = ex.answer.as_ref().map(|r| (r.status, r.timing.total)).map_err(Clone::clone);
                self.ran(&job, answer, tests);
            }
            _ => {}
        }
        for v in self.clients.values().map(|c| &c.response) {
            v.forget();
        }
        self.refresh_pages();
    }

    fn jar_for(&mut self, ex: &Exchange) -> Vec<cookies::Cookie> {
        let url = ex.answer.as_ref().map(|r| r.url.clone()).unwrap_or_else(|_| ex.sent.url.clone());
        let host = url::host(&url).map(|h| h.1).unwrap_or_default();
        self.core.store.jar(ex.env.as_deref()).cookies.iter().filter(|c| host == c.domain || host.ends_with(&format!(".{}", c.domain))).cloned().collect()
    }

    /// The response page beside, showing `key`'s.
    fn show_response(&mut self, key: (String, String)) {
        if api::setting("http.response").as_deref().map(str::trim) == Some("none") {
            return;
        }
        if let Some((_, p)) = self.responses.iter_mut().next() {
            p.key = key;
            p.view.forget();
            return;
        }
        let id = pages::open_beside(RESPONSE, "Response", 0.45);
        self.responses.insert(id, ResponsePage { key, view: ResponseView::new() });
    }

    fn refresh_pages(&self) {
        for id in self.clients.keys().chain(self.responses.keys()).chain(self.reports.keys()) {
            pages::refresh(*id);
        }
    }

    // OAuth.

    /// A token for configuration `id`, got: refreshed when it can be,
    /// else by its grant.
    fn need_token(&mut self, id: &str, rel: &str) {
        let (envs, _) = self.core.envs_for(rel);
        let env = self.core.env(&envs);
        let key = store::token_key(env.as_deref(), id);
        if self.core.flows.contains(&key) {
            return;
        }
        let Some(config) = envs.auth(env.as_deref(), id) else {
            self.token_failed(&key, format!("no OAuth configuration {id} in http-client.env.json's Security.Auth for {}", env.as_deref().unwrap_or("$shared")));
            return;
        };
        let secrets = self.core.secrets();
        let file = HttpFile::default();
        let oauth = {
            let mut cx = SendCx { file: &file, envs: &envs, env: env.as_deref(), secrets: &secrets, last: &self.core.last, tokens: &self.core.store.tokens, dotenv: self.core.dotenv(), now: now_millis() };
            OAuth::from_config(id, &config, &mut |s| vars::resolve(s, &mut cx).map_err(|m| m.said()))
        };
        let oauth = match oauth {
            Ok(o) => o,
            Err(e) => {
                self.token_failed(&key, e);
                return;
            }
        };
        self.core.flows.push(key.clone());
        let refresh = self.core.store.tokens.get(&key).and_then(|t| t.refresh.clone());
        match refresh {
            Some(r) => self.token_request(key, oauth, TokenGrant::Refresh { refresh_token: r.clone() }, true, Some(r), None),
            None => self.grant(key, oauth),
        }
    }

    fn grant(&mut self, key: String, oauth: OAuth) {
        match oauth.grant {
            auth::Grant::AuthorizationCode => {
                let mut oauth = oauth;
                let pkce = oauth.pkce.then(|| Pkce::new(&random::<32>()));
                let state = auth::base64url(&random::<12>());
                // The redirect listened for on the loopback, when it's to
                // this machine; else pasted.
                if let Some((port, path)) = auth::loopback(&oauth.redirect_url) {
                    match socket::listen(port) {
                        Ok((listener, got)) => {
                            oauth.redirect_url = auth::redirect_on(&oauth.redirect_url, got);
                            let url = oauth.authorize_url(&state, pkce.as_ref());
                            if let Err(e) = api::open_url(&url) {
                                api::copy(&url);
                                api::notify(&format!("The browser didn't open ({e}): the sign-in address is copied"));
                            } else {
                                api::notify(&format!("{}: sign in in the browser; Fenix waits for it at {}", oauth.id, oauth.redirect_url));
                            }
                            let timer = host::set_timer(CALLBACK_MS, false);
                            self.core.timers.insert(timer, Timed::Callback(listener));
                            self.core.callbacks.insert(listener, Callback { key, oauth: Box::new(oauth), state, verifier: pkce.map(|p| p.verifier), path, timer });
                            return;
                        }
                        Err(e) => api::warn!("{}: can't listen for the redirect ({e}); paste it instead", oauth.id),
                    }
                }
                let url = oauth.authorize_url(&state, pkce.as_ref());
                if let Err(e) = api::open_url(&url) {
                    api::copy(&url);
                    api::notify(&format!("The browser didn't open ({e}): the sign-in address is copied"));
                }
                let id = api::prompt(&format!("{}: once signed in, paste the address the browser went to (or the code)", oauth.id), "");
                self.core.asks.insert(id, Ask::Code { key, oauth: Box::new(oauth), state, verifier: pkce.map(|p| p.verifier) });
            }
            auth::Grant::ClientCredentials => self.token_request(key, oauth, TokenGrant::ClientCredentials, false, None, None),
            auth::Grant::Password if oauth.password.is_empty() => {
                // None in the env file: asked for, shown as dots, kept
                // nowhere but for this sign-in.
                let id = api::prompt_secret(&format!("{}: {}'s password", oauth.id, if oauth.username.is_empty() { "the user" } else { &oauth.username }));
                self.core.asks.insert(id, Ask::OAuthPassword { key, oauth: Box::new(oauth) });
            }
            auth::Grant::Password => self.token_request(key, oauth, TokenGrant::Password, false, None, None),
            auth::Grant::DeviceCode => {
                let r = oauth.device_request();
                match (HttpRequest { method: r.method, url: r.url, headers: r.headers, body: r.body, options: HttpOptions::new() }).send() {
                    Ok(id) => {
                        self.core.pending.insert(id, Pending::Device { key, oauth: Box::new(oauth) });
                    }
                    Err(e) => self.token_failed(&key, e),
                }
            }
        }
    }

    fn token_request(&mut self, key: String, oauth: OAuth, grant: TokenGrant, refreshing: bool, old_refresh: Option<String>, device: Option<auth::Device>) {
        let r = oauth.token_request(&grant);
        match (HttpRequest { method: r.method, url: r.url, headers: r.headers, body: r.body, options: HttpOptions::new() }).send() {
            Ok(id) => {
                self.core.pending.insert(id, Pending::Token { key, oauth: Box::new(oauth), refreshing, old_refresh, device });
            }
            Err(e) => self.token_failed(&key, e),
        }
    }

    fn token_answer(&mut self, key: String, oauth: OAuth, refreshing: bool, old_refresh: Option<String>, device: Option<auth::Device>, answer: Result<(u16, String), String>) {
        let now = now_millis() / 1000;
        let read = match answer {
            Ok((status, body)) => auth::read_token(status, &body, now, old_refresh.as_deref()),
            Err(e) => TokenAnswer::Error(e),
        };
        match read {
            TokenAnswer::Token(t) => {
                self.core.store.tokens.insert(key.clone(), t);
                self.core.save_store();
                self.core.flows.retain(|k| *k != key);
                api::notify(&format!("Signed in: {}", oauth.id));
                let (go, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.core.waiting).into_iter().partition(|(w, _)| *w == Wait::Token(key.clone()));
                self.core.waiting = keep;
                for (_, j) in go {
                    self.run(j);
                }
            }
            TokenAnswer::Pending { slow_down } => match device {
                Some(mut d) if now < d.expires => {
                    if slow_down {
                        d.interval += 5;
                    }
                    let t = host::set_timer(d.interval * 1000, false);
                    self.core.timers.insert(t, Timed::DevicePoll { key, oauth: Box::new(oauth), device: d });
                }
                Some(_) => self.token_failed(&key, "the device code ran out before you signed in".into()),
                None => self.token_failed(&key, "the token endpoint said to wait, outside the device grant".into()),
            },
            TokenAnswer::Error(e) if refreshing => {
                // The refresh token's no good: the grant again.
                api::warn!("{}: refresh failed ({e}), signing in again", oauth.id);
                self.core.store.tokens.remove(&key);
                self.grant(key, oauth);
            }
            TokenAnswer::Error(e) => self.token_failed(&key, format!("{}: {e}", oauth.id)),
        }
    }

    /// What the browser sent to a sign-in's loopback port: once its
    /// request has all come, answered, and the code in it sent for a token.
    fn callback_data(&mut self, socket: u64, bytes: &[u8]) {
        let Some((listener, got)) = self.core.callback_sockets.get_mut(&socket) else { return };
        got.extend_from_slice(bytes);
        let listener = *listener;
        let Some(target) = auth::callback_target(got) else {
            if got.len() > 64 * 1024 {
                self.core.callback_sockets.remove(&socket);
                socket::close(socket);
            }
            return;
        };
        self.core.callback_sockets.remove(&socket);
        let answer = |status: u16, text: &str| {
            let _ = socket::write(socket, &auth::callback_page(status, text));
            socket::close(socket);
        };
        let Some(cb) = self.core.callbacks.get(&listener) else { return answer(404, "This sign-in is over.") };
        // The browser asks for its icon too: not the redirect.
        let Some(target) = target.filter(|t| url::path(t) == cb.path || t.contains("code=") || t.contains("error=")) else {
            return answer(404, "Not the sign-in's address.");
        };
        let cb = self.core.callbacks.remove(&listener).expect("looked up above");
        socket::close(listener);
        self.core.timers.remove(&cb.timer);
        match auth::code_from_paste(&target, &cb.state) {
            Ok(code) => {
                answer(200, &format!("Signed in to {}: you can close this tab and go back to Fenix.", cb.oauth.id));
                self.token_request(cb.key, *cb.oauth, TokenGrant::Code { code, verifier: cb.verifier }, false, None, None);
            }
            Err(e) => {
                answer(200, &format!("{}: {e}", cb.oauth.id));
                self.token_failed(&cb.key, format!("{}: {e}", cb.oauth.id));
            }
        }
    }

    /// Sign-ins waiting for their redirect, given up.
    fn close_callbacks(&mut self, why: &str) {
        for (listener, cb) in std::mem::take(&mut self.core.callbacks) {
            socket::close(listener);
            self.core.timers.remove(&cb.timer);
            self.token_failed(&cb.key, format!("{}: {why}", cb.oauth.id));
        }
        for (s, _) in std::mem::take(&mut self.core.callback_sockets) {
            socket::close(s);
        }
    }

    fn token_failed(&mut self, key: &str, why: String) {
        self.core.flows.retain(|k| k != key);
        let (fail, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.core.waiting).into_iter().partition(|(w, _)| *w == Wait::Token(key.to_string()));
        self.core.waiting = keep;
        if fail.is_empty() {
            error(&why);
        }
        for (_, j) in fail {
            self.failed(&j, why.clone());
        }
    }

    // The runner.

    /// Every request below `path` (a file, a folder, or "" for the
    /// project), sent in turn, and a report of them.
    fn run_all(&mut self, path: &str) -> Result<(), String> {
        self.core.files();
        let files: Vec<FileEntry> = self.core.files.iter().filter(|f| path.is_empty() || f.rel == path || f.rel.starts_with(&format!("{}/", path.trim_end_matches('/')))).cloned().collect();
        let mut queue = VecDeque::new();
        for f in &files {
            let text = self.core.text(&f.rel).unwrap_or_default();
            for (i, r) in HttpFile::parse(&text).requests.iter().enumerate() {
                queue.push_back(Job { file: f.rel.clone(), index: i, label: r.label(), origin: Origin::Runner, depth: 0 });
            }
        }
        if queue.is_empty() {
            return Err(format!("no requests in {}", if path.is_empty() { "the project" } else { path }));
        }
        let title = if path.is_empty() { "the project".to_string() } else { path.to_string() };
        let page = match self.core.runner.as_ref().map(|r| r.page).filter(|p| self.reports.contains_key(p)) {
            Some(p) => p,
            None => {
                let id = pages::open(REPORT, &format!("Run: {title}"));
                id
            }
        };
        self.reports.insert(page, Report::new(&title, queue.len()));
        pages::refresh(page);
        let first = queue.pop_front();
        self.core.runner = Some(Runner { queue, page });
        if let Some(j) = first {
            self.run(j);
        }
        Ok(())
    }

    /// One of the runner's sent: in the report, and the next sent.
    fn ran(&mut self, job: &Job, answer: Result<(u16, u64), String>, tests: Vec<assert::Outcome>) {
        let Some(runner) = self.core.runner.as_mut() else { return };
        let line = self.core.files.iter().find(|f| f.rel == job.file).and_then(|f| f.requests.get(job.index)).map(|r| r.2).unwrap_or(0);
        let method = self.core.files.iter().find(|f| f.rel == job.file).and_then(|f| f.requests.get(job.index)).map(|r| r.0.clone()).unwrap_or_default();
        if let Some(report) = self.reports.get_mut(&runner.page) {
            report.results.push(RunResult { file: job.file.clone(), label: job.label.clone(), method, line, answer, tests });
        }
        let next = runner.queue.pop_front();
        let page = runner.page;
        match next {
            Some(j) => self.run(j),
            None => {
                if let Some(report) = self.reports.get_mut(&page) {
                    report.done = true;
                    let passed = report.results.iter().filter(|r| r.passed()).count();
                    api::notify(&format!("Run: {passed}/{} passed", report.results.len()));
                }
                self.core.runner = None;
            }
        }
        pages::refresh(page);
    }

    // The buffer.

    /// The request under the cursor in the `.http` file you're in.
    fn here_request(&mut self) -> Result<(String, usize, String), String> {
        let here = api::here().ok_or("not in a file")?;
        let path = here.path.ok_or("not in a file")?;
        if self.core.root.is_none() {
            self.core.root = here.project.map(|p| p.root);
        }
        if !is_http(&path) {
            return Err("not in a .http file".into());
        }
        let rel = self.core.rel(&path).ok_or("the .http file isn't in the project")?;
        let line = api::cursor().map(|c| c.at.0).unwrap_or(here.line.saturating_sub(1));
        let text = self.core.text(&rel).ok_or("can't read it")?;
        let f = HttpFile::parse(&text);
        let i = f.at(line).ok_or("no request here")?;
        Ok((rel, i, f.requests[i].label()))
    }

    fn send_here(&mut self) -> Result<(), String> {
        let (file, index, label) = self.here_request()?;
        self.run(Job { file, index, label, origin: Origin::Buffer, depth: 0 });
        Ok(())
    }

    fn open_client(&mut self, history: bool) {
        self.core.load_store();
        if let Some((&id, p)) = self.clients.iter_mut().next() {
            if history {
                p.side.active = 1;
                p.area = ui::client::Area::Side;
            }
            pages::focus(id);
            pages::refresh(id);
            return;
        }
        let id = pages::open(CLIENT, "HTTP");
        let mut p = ClientPage::new();
        if history {
            p.side.active = 1;
        }
        // The request you're in, chosen.
        if let Ok((rel, i, _)) = self.here_request() {
            p.selected = Some((rel, i));
            p.area = ui::client::Area::Request;
        }
        self.core.files();
        p.open_new(&ui::client::tree(&self.core.files));
        if let Some((rel, i)) = p.selected.clone() {
            p.select(&self.core.files, &rel, i);
        }
        if history {
            p.area = ui::client::Area::Side;
        }
        self.clients.insert(id, p);
    }

    fn pick_env(&mut self, rel: Option<String>) -> Result<(), String> {
        let rel = rel.or_else(|| self.here_request().ok().map(|r| r.0)).or_else(|| self.core.files().first().map(|f| f.rel.clone())).unwrap_or_default();
        let (envs, dir) = self.core.envs_for(&rel);
        let names = envs.names();
        if names.is_empty() {
            return Err(format!("no environments: add an {} beside your .http files", env::PUBLIC));
        }
        let now = self.core.env(&envs);
        let mut items: Vec<PickerItem> = names
            .iter()
            .map(|n| {
                let count = envs.vars(Some(n)).len();
                PickerItem::new(n.clone(), n.clone()).detail(format!("{count} variable{}{}", if count == 1 { "" } else { "s" }, if now.as_deref() == Some(n) { " · now" } else { "" }))
            })
            .collect();
        items.push(PickerItem::new("No environment", "").detail("$shared only"));
        let id = api::pick(&format!("Environment ({})", if dir.is_empty() { env::PUBLIC.to_string() } else { format!("{dir}/{}", env::PUBLIC) }), &items);
        self.core.asks.insert(id, Ask::Env);
        Ok(())
    }

    /// The request a page shows, as its file has it now.
    fn page_request(&self, page: &ClientPage) -> Option<(String, usize, parse::Request)> {
        let (rel, i) = page.selected.clone()?;
        let text = self.core.text(&rel)?;
        let r = HttpFile::parse(&text).requests.get(i)?.clone();
        Some((rel, i, r))
    }

    /// Copy as curl or code: the request, its variables filled in.
    fn copy_as(&mut self, file: &str, index: usize) -> Result<(), String> {
        let text = self.core.text(file).ok_or("can't read it")?;
        let f = HttpFile::parse(&text);
        let r = f.requests.get(index).ok_or("no request")?.clone();
        let (envs, _) = self.core.envs_for(file);
        let env = self.core.env(&envs);
        let secrets = self.core.secrets();
        let rel = file.to_string();
        let mut read = |p: &str| api::read_file(&project(&beside(&rel, p)));
        let mut cx = SendCx { file: &f, envs: &envs, env: env.as_deref(), secrets: &secrets, last: &self.core.last, tokens: &self.core.store.tokens, dotenv: self.core.dotenv(), now: now_millis() };
        let (sent, _) = send::prepare(&r, &mut cx, &mut read, &send::Defaults::default()).map_err(|m| format!("copied with its variables filled in, and {}", m.said()))?;
        let items: Vec<PickerItem> = export::Lang::ALL.iter().map(|l| PickerItem::new(l.name(), l.name())).collect();
        let id = api::pick("Copy as", &items);
        self.core.asks.insert(id, Ask::CopyAs(sent));
        Ok(())
    }

    // The page's actions.

    fn client_action(&mut self, page: u64, action: Action) -> Result<(), String> {
        let Some(p) = self.clients.get(&page) else { return Ok(()) };
        let selected = self.page_request(p);
        let need = || selected.clone().ok_or_else(|| "choose a request first".to_string());
        match action {
            Action::Nothing | Action::Redraw | Action::ShowHistory(_) => {}
            Action::Select(_, _) => {
                if let Some(p) = self.clients.get_mut(&page) {
                    p.response.scroll.scroll_to(0, 0);
                }
            }
            Action::Send => {
                if let Some(i) = p.viewing {
                    // A history entry: sent again as it was.
                    let e = self.core.store.history.get(i).cloned().ok_or("gone from the history")?;
                    let req = HttpRequest { method: e.sent.method.clone(), url: e.sent.url.clone(), headers: e.sent.headers.clone(), body: e.sent.body.clone(), options: HttpOptions::new() };
                    let id = req.send()?;
                    let job = Job { file: e.file.clone().unwrap_or_default(), index: usize::MAX, label: format!("history {}", e.sent.url), origin: Origin::Page, depth: 0 };
                    self.core.pending.insert(id, Pending::Request { job, sent: e.sent, opts: Opts { timeout_ms: 30_000, follow_redirects: true, insecure: false, stream: false, cookie_jar: false, log: true }, partial: None, env: e.env, name: None });
                    if let Some(p) = self.clients.get_mut(&page) {
                        p.viewing = None;
                        p.resent = true;
                    }
                    return Ok(());
                }
                let (rel, i, r) = need()?;
                self.run(Job { file: rel, index: i, label: r.label(), origin: Origin::Page, depth: 0 });
            }
            Action::PickEnv => self.pick_env(selected.map(|s| s.0))?,
            Action::Reload => {
                self.core.texts.clear();
                self.core.scan();
                let files = self.core.files.clone();
                if let Some(p) = self.clients.get_mut(&page) {
                    p.open_new(&ui::client::tree(&files));
                }
            }
            Action::OpenFile(line) => {
                let (rel, _, r) = need()?;
                host::open_file(&project(&rel), Some(line.unwrap_or(r.line) + 1));
            }
            Action::SaveHistory(i) => {
                let mut items: Vec<PickerItem> = self.core.files().iter().map(|f| PickerItem::new(f.rel.clone(), f.rel.clone()).detail(format!("{} requests", f.requests.len()))).collect();
                items.insert(0, PickerItem::new("A new file…", ""));
                let id = api::pick("Save to which collection?", &items);
                self.core.asks.insert(id, Ask::SaveHistory(i));
            }
            Action::Method => {
                let (rel, i, r) = need()?;
                let items: Vec<PickerItem> = parse::METHODS.iter().map(|m| PickerItem::new(*m, *m).detail(if *m == r.method { "now" } else { "" })).collect();
                let id = api::pick("Method", &items);
                self.core.asks.insert(id, Ask::Method { file: rel, index: i });
            }
            Action::SetUrl(u) => {
                let (rel, _, r) = need()?;
                let text = self.core.text(&rel).unwrap_or_default();
                let lines: Vec<&str> = text.split('\n').collect();
                let e = edit::url(&lines, &r, u.trim());
                self.core.edit(&rel, &[e])?;
            }
            Action::AddRow(tab) | Action::EditRow(tab, _) => {
                let (rel, i, r) = need()?;
                let row = if let Action::EditRow(_, row) = action { Some(row) } else { None };
                let rows = ui::client::rows(&r, tab);
                let (label, initial) = match tab {
                    ReqTab::Params => ("Parameter, name=value", row.and_then(|n| rows.get(n)).map(|(k, v)| if v.is_empty() { k.clone() } else { format!("{k}={v}") })),
                    ReqTab::Headers => ("Header, Name: value", row.and_then(|n| rows.get(n)).map(|(k, v)| format!("{k}: {v}"))),
                    ReqTab::Tests => ("Test: status == 200, $.id exists, header Content-Type contains json", row.and_then(|n| rows.get(n)).map(|(k, _)| k.clone())),
                    _ => return Ok(()),
                };
                let id = api::prompt(label, initial.as_deref().unwrap_or(""));
                self.core.asks.insert(id, Ask::Row { file: rel, index: i, tab, row });
            }
            Action::DeleteRow(tab, row) => {
                let (rel, _, r) = need()?;
                let text = self.core.text(&rel).unwrap_or_default();
                let lines: Vec<&str> = text.split('\n').collect();
                let e = match tab {
                    ReqTab::Params => {
                        let mut params = url::split(&r.url).1;
                        if row < params.len() {
                            params.remove(row);
                        }
                        Some(edit::params(&lines, &r, &params))
                    }
                    ReqTab::Headers => edit::remove_header(&lines, &r, row),
                    ReqTab::Body => Some(edit::body(&lines, &r, None)),
                    ReqTab::Tests => r.tests().nth(row).map(|d| edit::remove_line(&lines, d.line)),
                    ReqTab::Auth => r.headers.iter().position(|h| h.name.eq_ignore_ascii_case("authorization")).and_then(|i| edit::remove_header(&lines, &r, i)),
                };
                if let Some(e) = e {
                    self.core.edit(&rel, &[e])?;
                }
            }
            Action::BodyType => {
                let (rel, i, _) = need()?;
                let items = [("JSON", "application/json"), ("Form", "application/x-www-form-urlencoded"), ("Multipart, with files", "multipart/form-data"), ("Text", "text/plain"), ("XML", "application/xml"), ("A file's bytes", "application/octet-stream"), ("GraphQL", "graphql"), ("None", "none")];
                let items: Vec<PickerItem> = items.iter().map(|(l, v)| PickerItem::new(*l, *v)).collect();
                let id = api::pick("Body", &items);
                self.core.asks.insert(id, Ask::BodyType { file: rel, index: i });
            }
            Action::Auth => {
                let (rel, i, _) = need()?;
                let (envs, _) = self.core.envs_for(&rel);
                let env = self.core.env(&envs);
                let mut items = vec![PickerItem::new("Bearer token", "bearer"), PickerItem::new("Basic (user and password)", "basic"), PickerItem::new("API key (a header)", "apikey")];
                for id in envs.auth_ids(env.as_deref()) {
                    items.push(PickerItem::new(format!("OAuth 2.0: {id}"), format!("oauth:{id}")).detail("from http-client.env.json"));
                }
                if envs.auth_ids(env.as_deref()).is_empty() {
                    items.push(PickerItem::new("OAuth 2.0…", "oauth-help").detail("set one up in http-client.env.json first"));
                }
                items.push(PickerItem::new("None", "none"));
                let id = api::pick("Auth", &items);
                self.core.asks.insert(id, Ask::Auth { file: rel, index: i });
            }
            Action::CopyAs => {
                let (rel, i, _) = need()?;
                self.copy_as(&rel, i)?;
            }
            Action::NewRequest => {
                let rel = selected.map(|s| s.0).or_else(|| self.core.files().first().map(|f| f.rel.clone()));
                match rel {
                    Some(rel) => {
                        let id = api::prompt(&format!("New request in {rel}: METHOD URL"), "GET https://");
                        self.core.asks.insert(id, Ask::NewRequest(rel));
                    }
                    None => {
                        let id = api::prompt("New .http file, below the project", "requests.http");
                        self.core.asks.insert(id, Ask::NewFile);
                    }
                }
            }
            Action::RunAll(path) => self.run_all(&path)?,
            Action::OpenBody | Action::SaveBody => {
                let shown = self.client_shown(page);
                let Some(Ok(r)) = shown.exchange.as_ref().map(|e| &e.answer) else { return Err("no response".into()) };
                if action == Action::OpenBody {
                    let kind = pretty::kind(r.content_type(), &r.body);
                    let (text, lang) = match kind {
                        pretty::Kind::Json => (json::pretty(&r.text()).unwrap_or_else(|| r.text()), Some("json")),
                        pretty::Kind::Xml => (pretty::markup(&r.text(), false), Some("xml")),
                        pretty::Kind::Html => (pretty::markup(&r.text(), true), Some("html")),
                        _ => (r.text(), None),
                    };
                    api::open_text_beside(&format!("Response {}", r.status), &text, lang, 0.5);
                } else {
                    let id = api::prompt("Save the body to, below the project", "response.json");
                    self.core.asks.insert(id, Ask::SaveBody(r.body.clone()));
                }
            }
        }
        Ok(())
    }

    /// What a client page's response view shows.
    fn client_shown(&self, page: u64) -> Shown {
        let Some(p) = self.clients.get(&page) else { return Shown::default() };
        if let Some(e) = p.viewing.and_then(|i| self.core.store.history.get(i)) {
            let host = url::host(&e.sent.url).map(|h| h.1).unwrap_or_default();
            let jar = self.core.store.cookies.get(e.env.as_deref().unwrap_or("")).map(|j| j.cookies.iter().filter(|c| host.ends_with(&c.domain)).cloned().collect()).unwrap_or_default();
            return Shown { exchange: Some(e.clone()), jar, title: e.sent.url.clone(), ..Shown::default() };
        }
        let Some((rel, _, r)) = self.page_request(p) else { return Shown::default() };
        self.core.shown.get(&(rel, r.label())).cloned().unwrap_or_default()
    }

    fn picked(&mut self, ask: Ask, value: String) -> Result<(), String> {
        match ask {
            Ask::Env => {
                self.core.load_store();
                self.core.store.env = Some(value.clone()).filter(|v| !v.is_empty());
                self.core.save_store();
                api::notify(&format!("Environment: {}", if value.is_empty() { "none" } else { &value }));
                api::status(self.core.store.env.as_deref().map(|e| format!("http: {e}")).as_deref());
                let files: Vec<String> = self.core.inline.keys().map(|k| k.0.clone()).collect();
                for f in files {
                    self.core.decorate(&f);
                }
                self.publish_all();
            }
            Ask::Method { file, index } => {
                let r = self.request_at(&file, index)?;
                let text = self.core.text(&file).unwrap_or_default();
                let lines: Vec<&str> = text.split('\n').collect();
                self.core.edit(&file, &[edit::method(&lines, &r, &value)])?;
            }
            Ask::Row { file, index, tab, row } => {
                let r = self.request_at(&file, index)?;
                let text = self.core.text(&file).unwrap_or_default();
                let lines: Vec<&str> = text.split('\n').collect();
                let e = match tab {
                    ReqTab::Params => {
                        let mut params = url::split(&r.url).1;
                        let pair = match value.split_once('=') {
                            Some((k, v)) => (k.trim().to_string(), v.trim().to_string()),
                            None => (value.trim().to_string(), String::new()),
                        };
                        match row {
                            Some(n) if n < params.len() => params[n] = pair,
                            _ => params.push(pair),
                        }
                        edit::params(&lines, &r, &params)
                    }
                    ReqTab::Headers => {
                        let (k, v) = parse::header(&value).ok_or("a header is Name: value")?;
                        edit::header(&lines, &r, row, &k, &v)
                    }
                    ReqTab::Tests => match row.and_then(|n| r.tests().nth(n)) {
                        Some(d) => edit::directive(&lines, &r, "test", value.trim(), Some(d.line)),
                        None => edit::add_directive(&lines, &r, "test", value.trim()),
                    },
                    _ => return Ok(()),
                };
                self.core.edit(&file, &[e])?;
            }
            Ask::BodyType { file, index } => {
                let r = self.request_at(&file, index)?;
                let text = self.core.text(&file).unwrap_or_default();
                let lines: Vec<&str> = text.split('\n').collect();
                let (ct, template): (Option<&str>, Option<&str>) = match value.as_str() {
                    "application/json" => (Some("application/json"), Some("{\n  \n}")),
                    "application/x-www-form-urlencoded" => (Some(value.as_str()), Some("name=value\n&other=value")),
                    "multipart/form-data" => (Some("multipart/form-data; boundary=----FenixFormBoundary"), Some("------FenixFormBoundary\nContent-Disposition: form-data; name=\"field\"\n\nvalue\n------FenixFormBoundary\nContent-Disposition: form-data; name=\"file\"; filename=\"file.txt\"\n\n< ./file.txt\n------FenixFormBoundary--")),
                    "application/octet-stream" => (Some(value.as_str()), Some("< ./file.bin")),
                    "graphql" => (None, Some("query {\n  \n}")),
                    "none" => (None, None),
                    other => (Some(other), Some("")),
                };
                // All against the text as it is: Fenix makes them as one.
                let mut edits = Vec::new();
                if let Some(ct) = ct {
                    edits.push(edit::set_header(&lines, &r, "Content-Type", ct));
                }
                if value == "graphql" {
                    edits.push(edit::set_header(&lines, &r, "X-Request-Type", "GraphQL"));
                    if r.header("content-type").is_none() {
                        edits.push(edit::set_header(&lines, &r, "Content-Type", "application/json"));
                    }
                }
                let keep = r.body.is_some() && value != "none";
                if !keep {
                    edits.push(edit::body(&lines, &r, template.filter(|t| !t.is_empty())));
                }
                self.core.edit(&file, &edit::merge(edits))?;
            }
            Ask::Auth { file, index } => {
                let r = self.request_at(&file, index)?;
                let text = self.core.text(&file).unwrap_or_default();
                let lines: Vec<&str> = text.split('\n').collect();
                match value.as_str() {
                    "none" => {
                        if let Some(i) = r.headers.iter().position(|h| h.name.eq_ignore_ascii_case("authorization")) {
                            if let Some(e) = edit::remove_header(&lines, &r, i) {
                                self.core.edit(&file, &[e])?;
                            }
                        }
                    }
                    "oauth-help" => {
                        api::open_text_beside("OAuth 2.0 in http-client.env.json", OAUTH_HELP, Some("json"), 0.5);
                    }
                    v if v.starts_with("oauth:") => {
                        let id = &v[6..];
                        self.core.edit(&file, &[edit::set_header(&lines, &r, "Authorization", &format!("Bearer {{{{$auth.token(\"{id}\")}}}}"))])?;
                    }
                    kind => {
                        let label = match kind {
                            "bearer" => "Bearer token (a {{variable}} keeps it out of the file)",
                            "basic" => "user:password (a {{variable}} keeps it out of the file)",
                            _ => "API key header, Name: value",
                        };
                        let initial = match kind {
                            "bearer" => "{{token}}",
                            "basic" => "{{username}}:{{password}}",
                            _ => "X-API-Key: {{apiKey}}",
                        };
                        let id = api::prompt(label, initial);
                        self.core.asks.insert(id, Ask::AuthValue { file, index, kind: kind.to_string() });
                    }
                }
            }
            Ask::AuthValue { file, index, kind } => {
                let r = self.request_at(&file, index)?;
                let text = self.core.text(&file).unwrap_or_default();
                let lines: Vec<&str> = text.split('\n').collect();
                let e = match kind.as_str() {
                    "bearer" => edit::set_header(&lines, &r, "Authorization", &format!("Bearer {}", value.trim())),
                    "basic" => edit::set_header(&lines, &r, "Authorization", &format!("Basic {}", value.trim())),
                    _ => {
                        let (k, v) = parse::header(&value).ok_or("a header is Name: value")?;
                        edit::set_header(&lines, &r, &k, &v)
                    }
                };
                self.core.edit(&file, &[e])?;
            }
            Ask::CopyAs(sent) => {
                let lang = export::Lang::by_name(&value).ok_or("no such language")?;
                let code = export::export(lang, &sent);
                api::copy(&code);
                api::open_text_beside(&format!("{} {}", sent.method, lang.name()), &code, Some(lang.language()), 0.5);
                api::notify(&format!("Copied as {}", lang.name()));
            }
            Ask::SaveHistory(i) => {
                if value.is_empty() {
                    let id = api::prompt("New .http file, below the project", "saved.http");
                    self.core.asks.insert(id, Ask::SaveHistoryNew(i));
                    return Ok(());
                }
                self.save_history_to(i, &value)?;
            }
            Ask::SaveHistoryNew(i) => {
                let rel = value.trim().trim_start_matches('/').to_string();
                let rel = if is_http(&rel) { rel } else { format!("{rel}.http") };
                new_file(&rel, "")?;
                self.save_history_to(i, &rel)?;
            }
            Ask::NewRequest(rel) => {
                let text = self.core.text(&rel).unwrap_or_default();
                let lines: Vec<&str> = text.split('\n').collect();
                let line = value.trim();
                self.core.edit(&rel, &[edit::append(&lines, &format!("###\n{line}"))])?;
                let n = HttpFile::parse(&self.core.text(&rel).unwrap_or_default()).requests.len();
                for p in self.clients.values_mut() {
                    p.selected = Some((rel.clone(), n.saturating_sub(1)));
                    p.viewing = None;
                }
            }
            Ask::NewFile => {
                let rel = value.trim().trim_start_matches('/').to_string();
                let rel = if is_http(&rel) { rel } else { format!("{rel}.http") };
                new_file(&rel, "### A request\nGET https://httpbin.org/get\nAccept: application/json\n")?;
                self.core.rescan(&rel);
                host::open_file(&project(&rel), Some(2));
            }
            Ask::Code { key, oauth, state, verifier } => match auth::code_from_paste(&value, &state) {
                Ok(code) => self.token_request(key, *oauth, TokenGrant::Code { code, verifier }, false, None, None),
                Err(e) => self.token_failed(&key, e),
            },
            Ask::OAuthPassword { key, mut oauth } => {
                oauth.password = value;
                self.token_request(key, *oauth, TokenGrant::Password, false, None, None);
            }
            Ask::SignIn => {
                let rel = self.here_request().ok().map(|r| r.0).or_else(|| self.core.files().first().map(|f| f.rel.clone())).unwrap_or_default();
                let (envs, _) = self.core.envs_for(&rel);
                let env = self.core.env(&envs);
                self.core.store.tokens.remove(&store::token_key(env.as_deref(), &value));
                self.need_token(&value, &rel);
            }
            Ask::SecretName => {
                let name = value.trim().to_string();
                secrets::key(&name)?;
                let id = api::prompt_secret(&format!("{name}'s value, kept in the system's keychain (nothing: forget it)"));
                self.core.asks.insert(id, Ask::SecretValue(name));
            }
            Ask::SecretValue(name) => {
                self.core.keep_secret(&name, Some(&value))?;
                if value.is_empty() {
                    api::notify(&format!("Secret {name} forgotten"));
                } else {
                    api::notify(&format!("Secret {name} kept in the keychain: {{{{{name}}}}} reads it"));
                }
            }
            Ask::WsPick => {
                let socket: u64 = value.parse().map_err(|_| "no such WebSocket")?;
                let label = self.core.ws.get(&socket).map(|c| c.job.label.clone()).ok_or("that WebSocket isn't open any more")?;
                let id = api::prompt(&format!("Message to {label}"), "");
                self.core.asks.insert(id, Ask::WsMessage(socket));
            }
            Ask::WsMessage(socket) => self.ws_say(socket, &value)?,
            Ask::ImportPath => {
                let rel = value.trim().trim_start_matches("/project/").trim_start_matches('/').to_string();
                self.import_file(&rel)?;
            }
            Ask::ImportInto(c) => {
                let dir = value.trim().trim_matches('/').to_string();
                self.write_import(&c, &dir)?;
            }
            Ask::CurlText => self.import_curl(&value)?,
            Ask::SaveBody(body) => {
                let rel = value.trim().trim_start_matches('/').to_string();
                if rel.contains("..") {
                    return Err("a path below the project".into());
                }
                let at = project(&rel);
                if let Some(dir) = std::path::Path::new(&at).parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                std::fs::write(&at, &body).map_err(|e| format!("{rel}: {e}"))?;
                api::notify(&format!("Saved to {rel}"));
            }
        }
        self.refresh_pages();
        Ok(())
    }

    fn request_at(&self, file: &str, index: usize) -> Result<parse::Request, String> {
        let text = self.core.text(file).ok_or("can't read it")?;
        HttpFile::parse(&text).requests.get(index).cloned().ok_or_else(|| "that request is gone".into())
    }

    fn save_history_to(&mut self, i: usize, rel: &str) -> Result<(), String> {
        let e = self.core.store.history.get(i).cloned().ok_or("gone from the history")?;
        let mut d = import::Draft { name: e.name.clone().unwrap_or_else(|| format!("{} {}", e.sent.method, url::path(&e.sent.url))), method: e.sent.method.clone(), url: e.sent.url.clone(), ..Default::default() };
        d.headers = e.sent.headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("cookie")).cloned().collect();
        if !e.sent.body.is_empty() {
            d.body = Some(String::from_utf8(e.sent.body.clone()).map_err(|_| "its body isn't text: save it to a file and use < ./file")?);
        }
        let text = self.core.text(rel).unwrap_or_default();
        let lines: Vec<&str> = text.split('\n').collect();
        self.core.edit(rel, &[edit::append(&lines, &import::to_http(&[d]))])?;
        api::notify(&format!("Saved to {rel} (its variables as they were filled in)"));
        Ok(())
    }

    // Imports.

    fn import_file(&mut self, rel: &str) -> Result<(), String> {
        let bytes = api::read_file(&project(rel))?;
        let text = String::from_utf8(bytes).map_err(|_| format!("{rel} isn't text"))?;
        let c = import::import(&text)?;
        let default = format!("{}/{}", folder(rel), import::slug(&c.name)).trim_start_matches('/').to_string();
        let id = api::prompt(&format!("Import {} into which folder of the project?", c.name), &default);
        self.core.asks.insert(id, Ask::ImportInto(Box::new(c)));
        Ok(())
    }

    fn write_import(&mut self, c: &import::Collection, dir: &str) -> Result<(), String> {
        if dir.contains("..") {
            return Err("a folder below the project".into());
        }
        let at = |rel: &str| if dir.is_empty() { rel.to_string() } else { format!("{dir}/{rel}") };
        let mut first = None;
        for (rel, drafts) in &c.files {
            let rel = at(rel);
            let path = project(&rel);
            if std::path::Path::new(&path).exists() {
                return Err(format!("{rel} is there already: import into another folder"));
            }
            new_file(&rel, &import::to_http(drafts))?;
            first.get_or_insert(rel);
        }
        if !c.envs.is_empty() {
            let env_rel = at(env::PUBLIC);
            let existing = std::fs::read_to_string(project(&env_rel)).ok();
            let merged = import::merge_env(existing.as_deref(), &c.envs)?;
            new_file(&env_rel, &merged)?;
        }
        self.core.scan();
        let n: usize = c.files.iter().map(|(_, d)| d.len()).sum();
        let mut said = format!("Imported {n} request{} into {}", if n == 1 { "" } else { "s" }, if dir.is_empty() { "the project" } else { dir });
        for note in &c.notes {
            api::warn!("import: {note}");
        }
        if !c.notes.is_empty() {
            said.push_str(&format!(" ({} things not brought: SPC h l)", c.notes.len()));
        }
        api::notify(&said);
        if let Some(rel) = first {
            host::open_file(&project(&rel), Some(1));
        }
        self.refresh_pages();
        Ok(())
    }

    /// A `curl` command as a request: at the end of the `.http` file you're
    /// in, else in a new one.
    fn import_curl(&mut self, cmd: &str) -> Result<(), String> {
        let d = import::curl::parse(cmd)?;
        let text = import::to_http(&[d]);
        match self.here_request().map(|r| r.0).or_else(|_| api::here().and_then(|h| h.path).filter(|p| is_http(p)).and_then(|p| self.core.rel(&p)).ok_or(())) {
            Ok(rel) => {
                let old = self.core.text(&rel).unwrap_or_default();
                let lines: Vec<&str> = old.split('\n').collect();
                self.core.edit(&rel, &[edit::append(&lines, &text)])?;
                api::notify(&format!("Added to {rel}"));
            }
            Err(_) => {
                let rel = "curl.http";
                let old = std::fs::read_to_string(project(rel)).unwrap_or_default();
                new_file(rel, &format!("{}{}{text}", old, if old.is_empty() || old.ends_with("\n\n") { "" } else { "\n" }))?;
                self.core.rescan(rel);
                host::open_file(&project(rel), None);
            }
        }
        Ok(())
    }

    fn publish_all(&mut self) {
        for server in self.core.server_ids.clone() {
            let Some(docs) = self.servers.documents(server) else { continue };
            let uris: Vec<Uri> = docs.uris().filter(|u| is_http(u)).filter_map(|u| u.parse().ok()).collect();
            for uri in uris {
                Server(&mut self.core).publish(server, docs, &uri);
            }
        }
    }
}

/// A new file below the project, its folders made.
fn new_file(rel: &str, text: &str) -> Result<(), String> {
    if rel.contains("..") {
        return Err("a path below the project".into());
    }
    let at = project(rel);
    if let Some(dir) = std::path::Path::new(&at).parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{rel}: {e}"))?;
    }
    std::fs::write(&at, text).map_err(|e| format!("{rel}: {e}"))
}

/// `>> file` and `>>! file`: the body saved beside the `.http` file; a
/// file there already is kept, the body going to `name-1.ext`, unless `!`.
fn save_body(rel: &str, path: &str, overwrite: bool, body: &[u8]) -> Result<(), String> {
    let target = beside(rel, path);
    let mut at = target.clone();
    if !overwrite {
        let (stem, ext) = match target.rsplit_once('.') {
            Some((s, e)) if !e.contains('/') => (s.to_string(), format!(".{e}")),
            _ => (target.clone(), String::new()),
        };
        let mut n = 1;
        while std::path::Path::new(&project(&at)).exists() {
            at = format!("{stem}-{n}{ext}");
            n += 1;
        }
    }
    new_file(&at, "")?;
    std::fs::write(project(&at), body).map_err(|e| format!("{at}: {e}"))
}

const OAUTH_HELP: &str = r#"{
  "dev": {
    "Security": {
      "Auth": {
        "my-api": {
          "Type": "OAuth2",
          "Grant Type": "Authorization Code",
          "Auth URL": "https://auth.example.com/authorize",
          "Token URL": "https://auth.example.com/token",
          "Redirect URL": "http://localhost/callback",
          "Client ID": "my-client",
          "Scope": "openid profile",
          "PKCE": true
        }
      }
    }
  }
}

// In http-client.env.json (and the client secret, if it has one, in
// http-client.private.env.json, under the same names). Then:
//
//   Authorization: Bearer {{$auth.token("my-api")}}
//
// The first request that needs it signs in: the browser opens at Auth URL,
// and Fenix listens for the redirect on 127.0.0.1 -- the Redirect URL's
// port, or any free one when it has none (or there's no Redirect URL at
// all). A redirect elsewhere is pasted back. "Client Credentials",
// "Password" (Username, Password; a password left out is asked for) and
// "Device Authorization" (Device Auth URL) need no redirect. Tokens are
// kept in the system's keychain, and refreshed when they run out.
"#;

/// The language server, over the `Core`.
struct Server<'a>(&'a mut Core);

impl Server<'_> {
    fn known(&mut self, rel: Option<&str>) -> complete::Known {
        let (envs, _) = self.0.envs_for(rel.unwrap_or(""));
        let env = self.0.env(&envs);
        let mut secrets: Vec<String> = self.0.secret_names.iter().cloned().chain(self.0.secrets().into_iter().map(|(k, _)| k)).collect();
        if let Some(e) = &env {
            secrets = secrets.into_iter().map(|s| s.strip_prefix(&format!("{e}.")).map(String::from).unwrap_or(s)).collect();
        }
        secrets.sort();
        secrets.dedup();
        let prefix = Core::schema_key(env.as_deref(), "");
        let schemas = self.0.schemas.iter().filter_map(|(k, s)| k.strip_prefix(&prefix).map(|u| (u.to_string(), s.clone()))).collect();
        complete::Known { env: envs.vars(env.as_deref()).into_iter().map(|(k, v, _)| (k, v)).collect(), auth_ids: envs.auth_ids(env.as_deref()), env_name: env, secrets, schemas }
    }

    fn rel_of_uri(&mut self, uri: &str) -> Option<String> {
        let path = uri.strip_prefix("file://")?;
        let path = url::decode(path);
        // file:///C:/x on Windows.
        let path = if path.len() > 3 && path.as_bytes()[2] == b':' { path[1..].to_string() } else { path };
        self.0.rel(&path)
    }

    fn publish(&mut self, server: u64, docs: &Documents, uri: &Uri) {
        if !is_http(uri.as_str()) {
            return;
        }
        let Some(text) = docs.text(uri) else { return };
        let rel = self.rel_of_uri(uri.as_str());
        let known = self.known(rel.as_deref());
        let lines: Vec<&str> = text.split('\n').collect();
        let at = |line: u32, col: usize| Position { line, character: lsp::char_to_utf16(lines.get(line as usize).copied().unwrap_or(""), col) };
        let diagnostics = complete::unset(text, &known)
            .into_iter()
            .map(|(line, from, to, message)| Diagnostic { range: Range { start: at(line, from), end: at(line, to) }, severity: Some(DiagnosticSeverity::WARNING), source: Some("http".into()), message, ..Default::default() })
            .collect();
        lsp::publish_diagnostics(server, uri.clone(), diagnostics);
    }
}

impl LanguageServer for Server<'_> {
    fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities { completion_provider: Some(CompletionOptions { trigger_characters: Some(["{", "@", ":", " ", "$", "."].map(String::from).to_vec()), ..Default::default() }), ..Default::default() }
    }

    fn initialized(&mut self, server: u64, root: Option<&str>) {
        if !self.0.server_ids.contains(&server) {
            self.0.server_ids.push(server);
        }
        if self.0.root.is_none() {
            self.0.root = root.and_then(|r| r.strip_prefix("file://")).map(url::decode).map(|p| if p.len() > 3 && p.as_bytes()[2] == b':' { p[1..].to_string() } else { p });
        }
    }

    fn changed(&mut self, server: u64, docs: &Documents, uri: &Uri) {
        self.publish(server, docs, uri);
    }

    fn completion(&mut self, _server: u64, docs: &Documents, params: CompletionParams) -> Option<CompletionResponse> {
        let at = &params.text_document_position;
        let uri = &at.text_document.uri;
        let text = docs.text(uri)?;
        let line = docs.line(uri, at.position.line).unwrap_or("");
        let col = lsp::utf16_to_char(line, at.position.character);
        let rel = self.rel_of_uri(uri.as_str());
        // In a GraphQL query: its server's schema, read if it hasn't been.
        if let Some(rel) = &rel {
            let file = HttpFile::parse(text);
            if let Some(r) = complete::graphql_query_at(&file, at.position.line) {
                if let Some(i) = file.requests.iter().position(|q| q.line == r.line) {
                    let url = r.url.clone();
                    self.0.ensure_schema(rel, i, &url);
                }
            }
        }
        let known = self.known(rel.as_deref());
        let (from, items) = complete::at(text, at.position.line, col, &known)?;
        let range = Range { start: Position { line: at.position.line, character: lsp::char_to_utf16(line, from) }, end: at.position };
        let items = items
            .into_iter()
            .map(|i| CompletionItem {
                label: i.label.clone(),
                kind: Some(match i.kind {
                    complete::Kind::Variable => CompletionItemKind::VARIABLE,
                    complete::Kind::Header => CompletionItemKind::FIELD,
                    complete::Kind::Value => CompletionItemKind::VALUE,
                    complete::Kind::Method => CompletionItemKind::KEYWORD,
                    complete::Kind::Directive => CompletionItemKind::KEYWORD,
                    complete::Kind::Field => CompletionItemKind::FIELD,
                    complete::Kind::Type => CompletionItemKind::CLASS,
                }),
                detail: (!i.detail.is_empty()).then_some(i.detail),
                filter_text: Some(i.label),
                text_edit: Some(CompletionTextEdit::Edit(LspEdit { range, new_text: i.insert })),
                ..Default::default()
            })
            .collect();
        Some(CompletionResponse::Array(items))
    }
}

fn key_of_press(press: &KeyPress) -> Option<UiKey> {
    fenix_ui_kit::KeyPress::from_api(press).map(|p| p.key)
}

impl Extension for Http {
    fn activate(&mut self) -> Result<(), String> {
        self.core.secret_names = secrets::load_names(SECRET_NAMES);
        self.core.move_secrets();
        Ok(())
    }

    fn socket_accepted(&mut self, listener: u64, socket: u64, _peer: &str) {
        if self.core.callbacks.contains_key(&listener) {
            self.core.callback_sockets.insert(socket, (listener, Vec::new()));
        } else {
            socket::close(socket);
        }
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "http.send" => self.send_here(),
            "http.send_all" => {
                let (rel, _, _) = self.here_request()?;
                self.run_all(&rel)
            }
            "http.run_project" => self.run_all(""),
            "http.open" => {
                self.open_client(false);
                Ok(())
            }
            "http.history" => {
                self.open_client(true);
                Ok(())
            }
            "http.env" => self.pick_env(None),
            "http.response" => {
                let (rel, _, label) = self.here_request()?;
                if !self.core.shown.contains_key(&(rel.clone(), label.clone())) {
                    return Err("not sent yet".into());
                }
                self.show_response((rel, label));
                self.refresh_pages();
                Ok(())
            }
            "http.copy_as" => {
                let (rel, i, _) = self.here_request()?;
                self.copy_as(&rel, i)
            }
            "http.import" => {
                let picked = api::explorer_selected().and_then(|p| self.core.rel(&p)).filter(|p| [".json", ".yaml", ".yml"].iter().any(|e| p.ends_with(e)));
                match picked {
                    Some(rel) => self.import_file(&rel),
                    None => {
                        let id = api::prompt("Import which file (Postman, Insomnia, OpenAPI), below the project?", "");
                        self.core.asks.insert(id, Ask::ImportPath);
                        Ok(())
                    }
                }
            }
            "http.import_curl" => match api::pasted_text().filter(|t| t.trim_start().starts_with("curl")) {
                Some(cmd) => self.import_curl(&cmd),
                None => {
                    let id = api::prompt("curl command", "curl ");
                    self.core.asks.insert(id, Ask::CurlText);
                    Ok(())
                }
            },
            "http.new" => {
                let id = api::prompt("New .http file, below the project", "requests.http");
                self.core.asks.insert(id, Ask::NewFile);
                Ok(())
            }
            "http.sign_in" => {
                let rel = self.here_request().ok().map(|r| r.0).or_else(|| self.core.files().first().map(|f| f.rel.clone())).unwrap_or_default();
                let (envs, _) = self.core.envs_for(&rel);
                let env = self.core.env(&envs);
                let ids = envs.auth_ids(env.as_deref());
                if ids.is_empty() {
                    api::open_text_beside("OAuth 2.0 in http-client.env.json", OAUTH_HELP, Some("json"), 0.5);
                    return Err("no OAuth configurations in http-client.env.json's Security.Auth".into());
                }
                let items: Vec<PickerItem> = ids.iter().map(|i| PickerItem::new(i.clone(), i.clone())).collect();
                let id = api::pick("Sign in to", &items);
                self.core.asks.insert(id, Ask::SignIn);
                Ok(())
            }
            "http.sign_out" => {
                self.core.load_store();
                let n = self.core.store.tokens.len();
                self.core.store.tokens.clear();
                self.core.save_store();
                api::notify(&format!("Forgot {n} token{}", if n == 1 { "" } else { "s" }));
                Ok(())
            }
            "http.secret" => {
                let id = api::prompt("Secret's name ({{name}} reads it; dev.name is dev's alone)", "");
                self.core.asks.insert(id, Ask::SecretName);
                Ok(())
            }
            "http.ws_send" => self.ws_pick(),
            "http.graphql_schema" => {
                let (rel, i, _) = self.here_request()?;
                self.core.fetch_schema(&rel, i, false)
            }
            "http.cookies_clear" => {
                self.core.load_store();
                let rel = self.here_request().ok().map(|r| r.0).unwrap_or_default();
                let (envs, _) = self.core.envs_for(&rel);
                let env = self.core.env(&envs);
                let n = self.core.store.jar(env.as_deref()).cookies.len();
                self.core.store.jar(env.as_deref()).cookies.clear();
                self.core.save_store();
                api::notify(&format!("Cleared {n} cookies of {}", env.as_deref().unwrap_or("no environment")));
                Ok(())
            }
            "http.cancel" => {
                let sockets: Vec<u64> = self.core.ws.keys().copied().collect();
                for s in &sockets {
                    if let Some(c) = self.core.ws.get_mut(s) {
                        c.closing = true;
                        if c.open {
                            let _ = socket::write(*s, &ws::frame(ws::Op::Close, &ws::close_payload(1000, ""), random::<4>()));
                        }
                    }
                    self.ws_end(*s, Some("closed by you".into()));
                }
                let signing_in = self.core.callbacks.len();
                self.close_callbacks("sign-in cancelled");
                let ids: Vec<u64> = self.core.pending.keys().copied().collect();
                for id in &ids {
                    api::http_cancel(*id);
                    if let Some(Pending::Request { job, .. }) = self.core.pending.remove(id) {
                        self.failed(&job, "cancelled".into());
                    }
                }
                self.core.pending.clear();
                self.core.waiting.clear();
                if let Some(r) = self.core.runner.as_mut() {
                    r.queue.clear();
                }
                api::notify(&format!("Cancelled {}", ids.len() + sockets.len() + signing_in));
                Ok(())
            }
            _ => Err(format!("no command {id}")),
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            // A whole answer: as a stream's first part would be.
            Event::HttpResponse((id, answer)) if self.core.pending.contains_key(&id) => self.http_event(id, HttpPart::Response(answer)),
            Event::Picked((id, Some(value))) => {
                if let Some(ask) = self.core.asks.remove(&id) {
                    if let Err(e) = self.picked(ask, value) {
                        error(&e);
                    }
                }
            }
            Event::Prompted((id, Some(value))) => {
                if let Some(ask) = self.core.asks.remove(&id) {
                    if let Err(e) = self.picked(ask, value) {
                        error(&e);
                    }
                }
            }
            Event::Picked((id, None)) | Event::Prompted((id, None)) => {
                if let Some(Ask::Code { key, .. } | Ask::OAuthPassword { key, .. }) = self.core.asks.remove(&id) {
                    self.token_failed(&key, "sign-in cancelled".into());
                }
            }
            Event::Timer(id) => match self.core.timers.remove(&id) {
                Some(Timed::DevicePoll { key, oauth, device }) => {
                    let grant = TokenGrant::Device { device_code: device.device_code.clone() };
                    self.token_request(key, *oauth, grant, false, None, Some(device));
                }
                Some(Timed::Callback(listener)) => {
                    if let Some(cb) = self.core.callbacks.remove(&listener) {
                        socket::close(listener);
                        self.token_failed(&cb.key, format!("{}: the browser didn't come back in {} minutes", cb.oauth.id, CALLBACK_MS / 60_000));
                    }
                }
                None => {}
            },
            Event::SocketConnected((s, result)) => {
                if self.core.ws.contains_key(&s) {
                    self.ws_connected(s, result);
                }
            }
            Event::SocketData((s, bytes)) => {
                if self.core.ws.contains_key(&s) {
                    self.ws_data(s, &bytes);
                } else if self.core.callback_sockets.contains_key(&s) {
                    self.callback_data(s, &bytes);
                }
            }
            Event::SocketClosed((s, why)) => {
                if self.core.ws.contains_key(&s) {
                    self.ws_end(s, why);
                } else {
                    self.core.callback_sockets.remove(&s);
                }
            }
            Event::SettingChanged(_) => {
                let files: Vec<String> = self.core.inline.keys().map(|k| k.0.clone()).collect();
                for f in files {
                    self.core.decorate(&f);
                }
            }
            _ => {}
        }
    }

    fn http_event(&mut self, request: u64, part: HttpPart) {
        let Some(pending) = self.core.pending.remove(&request) else { return };
        let received = |r: api::HttpResponse| Received {
            status: r.status,
            headers: r.headers,
            body: r.body,
            url: r.url,
            timing: Timing { dns: r.timing.dns, connect: r.timing.connect, tls: r.timing.tls, first_byte: r.timing.first_byte, total: r.timing.total },
        };
        match pending {
            Pending::Request { job, sent, opts, partial, env, name } => match part {
                HttpPart::Response(Ok(r)) if opts.stream => {
                    let key = (job.file.clone(), job.label.clone());
                    let got = received(r);
                    let shown = self.core.shown.entry(key).or_default();
                    shown.exchange = Some(Exchange { sent: sent.clone(), answer: Ok(got.clone()), at: now_millis() / 1000, file: Some(job.file.clone()), name: name.clone(), env: env.clone() });
                    shown.sending = true;
                    self.core.pending.insert(request, Pending::Request { job, sent, opts, partial: Some(got), env, name });
                    self.refresh_pages();
                }
                HttpPart::Response(answer) => self.finish(job, sent, opts, env, name, answer.map(received)),
                HttpPart::Chunk(bytes) => {
                    let mut got = partial.unwrap_or_default();
                    got.body.extend(bytes);
                    let key = (job.file.clone(), job.label.clone());
                    if let Some(Ok(r)) = self.core.shown.get_mut(&key).and_then(|s| s.exchange.as_mut()).map(|e| &mut e.answer) {
                        r.body = got.body.clone();
                    }
                    for v in self.clients.values().map(|c| &c.response).chain(self.responses.values().map(|r| &r.view)) {
                        v.forget();
                    }
                    self.core.pending.insert(request, Pending::Request { job, sent, opts, partial: Some(got), env, name });
                    self.refresh_pages();
                }
                HttpPart::End(err) => {
                    let got = partial.unwrap_or_default();
                    if let Some(e) = err {
                        api::warn!("{}: the stream broke off: {e}", job.label);
                    }
                    self.finish(job, sent, opts, env, name, Ok(got));
                }
            },
            Pending::Token { key, oauth, refreshing, old_refresh, device } => {
                let answer = match part {
                    HttpPart::Response(Ok(r)) => Ok((r.status, String::from_utf8_lossy(&r.body).into_owned())),
                    HttpPart::Response(Err(e)) => Err(e),
                    _ => return,
                };
                self.token_answer(key, *oauth, refreshing, old_refresh, device, answer);
            }
            Pending::Schema { key, url, quiet } => {
                let body = match part {
                    HttpPart::Response(Ok(r)) if (200..300).contains(&r.status) => r.body,
                    HttpPart::Response(Ok(r)) => return self.core.schema_failed(&url, quiet, format!("the server answered {} {}", r.status, model::reason(r.status))),
                    HttpPart::Response(Err(e)) => return self.core.schema_failed(&url, quiet, e),
                    _ => return,
                };
                match graphql::Schema::from_introspection(&body) {
                    Ok(schema) => {
                        let n = schema.types.keys().filter(|t| !t.starts_with("__")).count();
                        if let Some(path) = self.core.schema_path(&key) {
                            if let Some(dir) = std::path::Path::new(&path).parent() {
                                let _ = std::fs::create_dir_all(dir);
                            }
                            if let Err(e) = std::fs::write(&path, &body) {
                                api::warn!("{path}: {e}");
                            }
                        }
                        self.core.schemas.insert(key, Arc::new(schema));
                        api::notify(&format!("GraphQL: {url}'s schema read, {n} types"));
                    }
                    Err(e) => self.core.schema_failed(&url, quiet, e),
                }
            }
            Pending::Device { key, oauth } => {
                let body = match part {
                    HttpPart::Response(Ok(r)) => String::from_utf8_lossy(&r.body).into_owned(),
                    HttpPart::Response(Err(e)) => return self.token_failed(&key, e),
                    _ => return,
                };
                match auth::read_device(&body, now_millis() / 1000) {
                    Ok(d) => {
                        api::copy(&d.user_code);
                        let go = d.verification_uri_complete.clone().unwrap_or_else(|| d.verification_uri.clone());
                        let _ = api::open_url(&go);
                        api::notify(&format!("{}: enter {} at {} (it's copied)", oauth.id, d.user_code, d.verification_uri));
                        let t = host::set_timer(d.interval * 1000, false);
                        self.core.timers.insert(t, Timed::DevicePoll { key, oauth, device: d });
                    }
                    Err(e) => self.token_failed(&key, format!("{}: {e}", oauth.id)),
                }
            }
        }
    }

    fn buffer_changed(&mut self, what: Changed) {
        let Changed::File(path) = what else { return };
        let Some(rel) = self.core.rel(&path) else { return };
        self.core.texts.remove(&rel);
        if is_http(&rel) {
            self.core.rescan(&rel);
            self.core.decorate(&rel);
            self.refresh_pages();
        } else if is_env_file(&rel) {
            self.publish_all();
            self.refresh_pages();
        }
    }

    fn file_saved(&mut self, path: &str) {
        let Some(rel) = self.core.rel(path) else { return };
        self.core.texts.remove(&rel);
        if is_http(&rel) {
            self.core.rescan(&rel);
            self.refresh_pages();
        }
    }

    fn focus_moved(&mut self, focus: Focus) {
        let Focus::File(Some(path)) = focus else { return };
        if !is_http(&path) {
            return;
        }
        if self.core.root.is_none() {
            self.core.root();
        }
        if let Some(rel) = self.core.rel(&path) {
            self.core.load_store();
            self.core.decorate(&rel);
            let (envs, _) = self.core.envs_for(&rel);
            let env = self.core.env(&envs);
            api::status(env.map(|e| format!("http: {e}")).as_deref());
        }
    }

    fn decoration_clicked(&mut self, buffer: TextBuffer, id: u32) {
        let TextBuffer::File(path) = buffer else { return };
        let Some(rel) = self.core.rel(&path) else { return };
        let index = (id / 2) as usize;
        let Ok(r) = self.request_at(&rel, index) else { return };
        let label = r.label();
        if id.is_multiple_of(2) {
            self.run(Job { file: rel, index, label, origin: Origin::Buffer, depth: 0 });
        } else {
            self.show_response((rel, label));
            self.refresh_pages();
        }
    }

    fn lsp_message(&mut self, server: u64, message: &str) {
        self.servers.handle(server, message, &mut Server(&mut self.core));
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        let (w, h) = (width as usize, height as usize);
        if self.clients.contains_key(&page) {
            self.core.files();
            let shown = self.client_shown(page);
            let p = &self.clients[&page];
            let request = self.page_request(p).map(|s| s.2);
            let rel = p.selected.as_ref().map(|s| s.0.clone()).unwrap_or_default();
            let (envs, _) = self.core.envs_for(&rel);
            let env = self.core.env(&envs);
            let d = ui::client::Data { files: &self.core.files, request: request.as_ref(), env: env.as_deref(), history: &self.core.store.history, shown: &shown, envs: envs.names().len() };
            let c = p.render(&d, w, h);
            return c.to_page();
        }
        if let Some(p) = self.responses.get(&page) {
            let shown = self.core.shown.get(&p.key).cloned().unwrap_or_default();
            let mut c = fenix_ui_kit::Canvas::new();
            c.set_mouse(true);
            c.put(0, 0, &shown.title, fenix_ui_kit::Role::Title);
            p.view.render(&shown, fenix_ui_kit::Rect::new(0, 2, w, h.saturating_sub(3)), true, &mut c);
            c.put_fit(0, h.saturating_sub(1), w, "[ ] tabs  j k scroll  p raw  r send again  O open the body  o the client", fenix_ui_kit::Role::Muted);
            return c.to_page();
        }
        if let Some(r) = self.reports.get(&page) {
            return r.render(w, h).to_page();
        }
        Page::new()
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let key: UiKey = key.into();
        self.page_ui_key(page, key);
    }

    fn page_key_press(&mut self, page: u64, press: KeyPress) {
        // Ctrl+Enter sends, as in the others.
        let ctrl_enter = press.mods.ctrl && matches!(press.key, api::SurfaceKey::Named(api::NamedKey::Enter));
        if ctrl_enter && self.clients.contains_key(&page) {
            if let Err(e) = self.client_action(page, Action::Send) {
                error(&e);
            }
            return;
        }
        if let Some(k) = key_of_press(&press) {
            if matches!(k, UiKey::PageUp | UiKey::PageDown) {
                self.page_ui_key(page, k);
            }
        }
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        if let Some(p) = self.clients.get_mut(&page) {
            p.paste(text);
        }
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        let m = fenix_ui_kit::Mouse::from(&event);
        if self.clients.contains_key(&page) {
            let shown = self.client_shown(page);
            let request = self.clients.get(&page).and_then(|p| self.page_request(p)).map(|s| s.2);
            let files = self.core.files.clone();
            let history = self.core.store.history.clone();
            let action = {
                let d = ui::client::Data { files: &files, request: request.as_ref(), env: None, history: &history, shown: &shown, envs: 0 };
                self.clients.get_mut(&page).map(|p| p.mouse(&m, &d))
            };
            if let Some(a) = action {
                if let Err(e) = self.client_action(page, a) {
                    error(&e);
                }
            }
        } else if let Some(p) = self.responses.get_mut(&page) {
            p.view.mouse(&m);
        }
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        if let Some(p) = self.clients.get(&page) {
            return Some(serde_json::json!({ "file": p.selected.as_ref().map(|s| s.0.clone()), "index": p.selected.as_ref().map(|s| s.1) }).to_string());
        }
        self.responses.get(&page).map(|p| serde_json::json!({ "file": p.key.0, "label": p.key.1 }).to_string())
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        let v: serde_json::Value = serde_json::from_str(state).unwrap_or_default();
        match kind {
            CLIENT => {
                self.core.load_store();
                let mut p = ClientPage::new();
                if let (Some(f), Some(i)) = (v["file"].as_str(), v["index"].as_u64()) {
                    p.selected = Some((f.to_string(), i as usize));
                }
                self.core.files();
                p.open_new(&ui::client::tree(&self.core.files));
                if let Some((f, i)) = p.selected.clone() {
                    p.select(&self.core.files, &f, i);
                }
                self.clients.insert(page, p);
                true
            }
            RESPONSE => {
                let key = (v["file"].as_str().unwrap_or("").to_string(), v["label"].as_str().unwrap_or("").to_string());
                self.responses.insert(page, ResponsePage { key, view: ResponseView::new() });
                true
            }
            _ => false,
        }
    }

    fn page_closed(&mut self, page: u64) {
        self.clients.remove(&page);
        self.responses.remove(&page);
        self.reports.remove(&page);
    }
}

impl Http {
    fn page_ui_key(&mut self, page: u64, key: UiKey) {
        if self.clients.contains_key(&page) {
            let shown = self.client_shown(page);
            let request = self.clients.get(&page).and_then(|p| self.page_request(p)).map(|s| s.2);
            let rel = self.clients.get(&page).and_then(|p| p.selected.as_ref().map(|s| s.0.clone())).unwrap_or_default();
            let (envs, _) = self.core.envs_for(&rel);
            let env = self.core.env(&envs);
            self.core.files();
            let action = {
                let d = ui::client::Data { files: &self.core.files, request: request.as_ref(), env: env.as_deref(), history: &self.core.store.history, shown: &shown, envs: envs.names().len() };
                self.clients.get_mut(&page).map(|p| p.key(key, &d))
            };
            if let Some(a) = action {
                if let Err(e) = self.client_action(page, a) {
                    error(&e);
                }
            }
            return;
        }
        if let Some(p) = self.responses.get_mut(&page) {
            match key {
                UiKey::Char('r') => {
                    let (file, label) = p.key.clone();
                    if let Ok(i) = self.request_index(&file, &label) {
                        self.run(Job { file, index: i, label, origin: Origin::Buffer, depth: 0 });
                    }
                }
                UiKey::Char('o') => self.open_client(false),
                UiKey::Char('O') => {
                    let shown = self.core.shown.get(&p.key).cloned().unwrap_or_default();
                    if let Some(Ok(r)) = shown.exchange.as_ref().map(|e| &e.answer) {
                        let (text, lang) = match pretty::kind(r.content_type(), &r.body) {
                            pretty::Kind::Json => (json::pretty(&r.text()).unwrap_or_else(|| r.text()), Some("json")),
                            pretty::Kind::Xml => (pretty::markup(&r.text(), false), Some("xml")),
                            pretty::Kind::Html => (pretty::markup(&r.text(), true), Some("html")),
                            _ => (r.text(), None),
                        };
                        api::open_text(&format!("Response {}", r.status), &text, lang);
                    }
                }
                k => {
                    p.view.key(k);
                }
            }
            return;
        }
        if let Some(r) = self.reports.get_mut(&page) {
            match r.key(key) {
                ui::report::Action::Open(file, line) => host::open_file(&project(&file), Some(line + 1)),
                ui::report::Action::Again => {
                    let title = r.title.clone();
                    let path = if title == "the project" { String::new() } else { title };
                    if let Err(e) = self.run_all(&path) {
                        error(&e);
                    }
                }
                ui::report::Action::Nothing => {}
            }
        }
    }

    fn request_index(&self, file: &str, label: &str) -> Result<usize, String> {
        let text = self.core.text(file).ok_or("can't read it")?;
        HttpFile::parse(&text).requests.iter().position(|r| r.label() == label).ok_or_else(|| "that request is gone".into())
    }
}

export!(Http);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_a_file_names_are_beside_it() {
        assert_eq!(beside("api/users.http", "./logo.png"), "api/logo.png");
        assert_eq!(beside("api/users.http", "../data/x.json"), "data/x.json");
        assert_eq!(beside("top.http", "out/a.json"), "out/a.json");
        assert_eq!(beside("a/b.http", "/project/c.txt"), "c.txt");
        assert_eq!(folder("a/b/c.http"), "a/b");
        assert!(is_http("x.HTTP") && is_http("y.rest") && !is_http("z.txt"));
    }

    #[test]
    fn secrets_are_pairs() {
        let s = parse_secrets("token=abc; dev.password = p=w;\nkey=1");
        assert_eq!(s, [("token".to_string(), "abc".to_string()), ("dev.password".into(), "p=w".into()), ("key".into(), "1".into())]);
        assert!(parse_secrets("").is_empty());
    }

    #[test]
    fn a_tree_label_is_short() {
        let f = HttpFile::parse("GET https://x.test/a/b?c\n\n###\n# @name login\nPOST https://x.test/login\n\n### Root\nhttps://x.test\n\n###\nhttps://x.test\n");
        let labels: Vec<String> = f.requests.iter().map(short_label).collect();
        assert_eq!(labels, ["/a/b", "login", "Root", "https://x.test"]);
    }
}
