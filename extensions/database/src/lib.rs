//! A database browser, as a WebAssembly extension: the connections you
//! save (`database.connections`, their passwords asked for once and kept
//! in the system's keychain), each one's schema in a sidebar, the statement under the
//! cursor of a `.sql` buffer run on the buffer's connection, its results
//! as a table to sort, copy and export, a table's rows paged through and
//! changed, the statements you ran kept and searched, and completion of
//! the schema's names in `.sql` buffers. SQLite files open straight into
//! it.
//!
//! Engines: SQLite (Turso's, in Rust: `lite`), and over Fenix's sockets
//! PostgreSQL (`pg`), MySQL and MariaDB (`mysql`) and SQL Server (`tds`),
//! each a client fed what its socket brings, so nothing waits.

pub mod browser;
pub mod complete;
pub mod conn;
pub mod edit;
pub mod history;
pub mod lite;
pub mod mysql;
pub mod pg;
pub mod result;
pub mod schema;
pub mod session;
pub mod sql;
pub mod tableview;
pub mod tds;
pub mod view;
pub mod wire;

use std::collections::{BTreeMap, HashMap, HashSet};

use fenix_extension_api::lsp::types::{CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse, ServerCapabilities};
use fenix_extension_api::lsp::{self, Documents, LanguageServer, Servers};
use fenix_extension_api::{buffer, export, host, page as pages, scratch, socket, Changed, Event, Extension, Focus, Key, KeyPress, MouseEvent, Page, PickerItem, TlsOptions};
use fenix_ui_kit::{Role, Style, TreeNode};

use browser::{Browser, Intent, Ran};
use conn::{Conn, Engine, Verdict};
use edit::Target;
use history::{Entry, History};
use result::Outcome;
use session::{Finished, Job, Net, Purpose, Session, State};
use tableview::{Ask, TableView};

const BROWSER: &str = "browser";
const TABLE: &str = "table";
const HISTORY: &str = "/data/history.jsonl";
const BINDINGS: &str = "/data/bindings.json";
const COPIES: &str = "/data/copies";

/// Fenix's sockets.
struct Sockets;

fn tls_options(conn: &Conn) -> TlsOptions {
    let t = TlsOptions::new();
    if conn.insecure {
        t.accept_invalid()
    } else {
        t
    }
}

impl Net for Sockets {
    fn connect(&mut self, host: &str, port: u16, tls: Option<&Conn>) -> Result<u64, String> {
        match tls {
            Some(c) => socket::connect_tls(host, port, &tls_options(c)),
            None => socket::connect(host, port),
        }
    }

    fn start_tls(&mut self, s: u64, conn: &Conn) -> Result<(), String> {
        socket::start_tls(s, &tls_options(conn))
    }

    fn write(&mut self, s: u64, data: &[u8]) -> Result<(), String> {
        socket::write(s, data)
    }

    fn close(&mut self, s: u64) {
        socket::close(s)
    }
}

fn int_setting(key: &str, default: usize) -> usize {
    fenix_extension_api::setting(key).and_then(|s| s.trim().parse().ok()).filter(|&n: &usize| n > 0).unwrap_or(default)
}

fn max_rows() -> usize {
    int_setting("database.max_rows", 5000).min(1_000_000)
}

fn page_rows() -> usize {
    int_setting("database.page_rows", 200).min(10_000)
}

/// A path as one key whatever wrote it: `/` for `\`, a drive's letter
/// lower-case.
fn norm(path: &str) -> String {
    let p = path.replace('\\', "/");
    match p.as_bytes() {
        [d, b':', ..] => format!("{}{}", (*d as char).to_ascii_lowercase(), &p[1..]),
        _ => p,
    }
}

/// A `file://` URI's path.
fn uri_path(uri: &str) -> String {
    let rest = uri.strip_prefix("file://").unwrap_or(uri);
    let rest = if rest.len() > 2 && rest.as_bytes()[0] == b'/' && rest.as_bytes()[2] == b':' { &rest[1..] } else { rest };
    let mut out = Vec::new();
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A buffer's `-- connection: name` (or `-- database:`) in its first
/// lines.
fn named_in(text: &str) -> Option<String> {
    text.lines().take(5).find_map(|l| {
        let l = l.trim().strip_prefix("--")?.trim();
        let (k, v) = l.split_once(':')?;
        matches!(k.trim().to_ascii_lowercase().as_str(), "connection" | "database" | "db").then(|| v.trim().to_string()).filter(|v| !v.is_empty())
    })
}

/// A statement without the `-- connection:` lines before it.
fn without_naming(statement: &str) -> String {
    let mut rest = statement;
    while let Some((line, after)) = rest.split_once('\n') {
        if named_in(line).is_none() {
            break;
        }
        rest = after;
    }
    rest.trim().to_string()
}

/// Where a full path is under `/project`, if it is.
fn in_project(full: &str) -> Option<String> {
    let root = fenix_extension_api::here()?.project?.root;
    let (f, r) = (norm(full), norm(&root));
    let r = r.trim_end_matches('/');
    (f.len() > r.len() && f.starts_with(r) && f.as_bytes()[r.len()] == b'/').then(|| format!("/project{}", &f[r.len()..]))
}

/// What the buffer you ran a command in is.
#[derive(Debug, Clone, PartialEq)]
enum Buffer {
    File(String),
    Scratch(u64),
}

impl Buffer {
    fn key(&self) -> String {
        match self {
            Buffer::File(p) => norm(p),
            Buffer::Scratch(id) => format!("scratch:{id}"),
        }
    }

    fn text(&self) -> Option<String> {
        match self {
            Buffer::File(p) => buffer::text(p),
            Buffer::Scratch(id) => scratch::text(*id),
        }
    }
}

/// A run waiting for something: its connection to be picked, a yes.
#[derive(Debug, Clone)]
struct PendingRun {
    statements: Vec<String>,
    /// What's sent: the text itself, so a statement keeps how it's written.
    sql: String,
}

/// What a picker or a prompt was for.
#[derive(Debug, Clone)]
enum Asked {
    /// Which connection a run (or the buffer) uses.
    Connection {
        buffer: Buffer,
        run: Option<PendingRun>,
    },
    Confirm {
        conn: String,
        run: PendingRun,
    },
    Password(String),
    /// Whose kept password to forget.
    Forget,
    Export {
        text: String,
    },
    History {
        into: Option<Buffer>,
    },
}

/// A run's parts (SQL Server's statements go a batch each), as they come.
#[derive(Debug, Default)]
struct Gathering {
    conn: String,
    statements: Vec<String>,
    parts: usize,
    outcomes: Vec<(usize, Vec<Outcome>)>,
    millis: u64,
}

#[derive(Default)]
struct App {
    saved: Vec<Conn>,
    /// Rows of the setting that aren't connections: why.
    errors: Vec<String>,
    /// SQLite files opened: connections of their own.
    files: Vec<Conn>,
    sessions: BTreeMap<String, Session>,
    /// Passwords typed, kept until Fenix closes.
    passwords: HashMap<String, String>,
    /// Of those, the ones not yet kept in the keychain: kept once they
    /// sign in.
    typed: HashSet<String>,
    /// Jobs for a connection still waiting for its password.
    held: HashMap<String, Vec<Job>>,
    cataloged: HashSet<String>,
    browser_page: Option<u64>,
    browser: Browser,
    tables: HashMap<u64, TableView>,
    /// Table pages waiting for their connection's schema (restored).
    awaiting: HashSet<u64>,
    /// Buffer -> connection, kept in /data.
    bindings: BTreeMap<String, String>,
    last_conn: Option<String>,
    /// The buffer focused last, if it's SQL.
    focus: Option<Buffer>,
    scratches: HashSet<u64>,
    history: History,
    asked: HashMap<u64, Asked>,
    runs: HashMap<u64, Gathering>,
    next_job: u64,
    lite_timer: Option<u64>,
    /// The browser's width as last laid out: where its sidebar ends.
    browser_w: usize,
    servers: Servers,
}

impl App {
    fn load_bindings(&mut self) {
        self.bindings = std::fs::read_to_string(BINDINGS).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    }

    fn save_bindings(&self) {
        if let Ok(s) = serde_json::to_string_pretty(&self.bindings) {
            let _ = std::fs::write(BINDINGS, s);
        }
    }

    /// `database.connections` read again: sessions whose connection
    /// changed, or went, closed.
    fn reload(&mut self) {
        lite::set_timeout(int_setting("database.timeout", (lite::DEFAULT_TIMEOUT_MS / 1000) as usize) as u64 * 1000);
        let text = fenix_extension_api::setting("database.connections").unwrap_or_default();
        let (mut saved, mut errors) = (Vec::new(), Vec::new());
        for r in conn::parse_all(&text) {
            match r {
                Ok(c) if saved.iter().any(|s: &Conn| s.name == c.name) => errors.push(format!("{}: a second connection of that name", c.name)),
                Ok(c) => saved.push(c),
                Err(e) => errors.push(e),
            }
        }
        let stale: Vec<String> =
            self.sessions.iter().filter(|(name, s)| !self.files.iter().any(|f| &f.name == *name) && saved.iter().find(|c| &c.name == *name) != Some(&s.conn)).map(|(n, _)| n.clone()).collect();
        for name in stale {
            if let Some(mut s) = self.sessions.remove(&name) {
                s.close(&mut Sockets);
            }
            self.cataloged.remove(&name);
        }
        self.saved = saved;
        self.errors = errors;
    }

    fn conn(&self, name: &str) -> Option<Conn> {
        self.saved.iter().chain(&self.files).find(|c| c.name == name).cloned()
    }

    fn all_conns(&self) -> Vec<Conn> {
        self.saved.iter().chain(&self.files).cloned().collect()
    }

    fn job(&mut self, sql: String, single: bool, purpose: Purpose) -> Job {
        self.next_job += 1;
        Job { id: self.next_job, sql, single, purpose }
    }

    /// The password for a connection: typed this time, the one kept in the
    /// keychain, or the older map's (`database.passwords`).
    fn password(&self, name: &str) -> Option<String> {
        self.passwords
            .get(name)
            .cloned()
            .or_else(|| fenix_extension_api::secret(&conn::secret_key(name)))
            .or_else(|| host::secret("database.passwords").and_then(|s| conn::password(&s, name)))
    }

    /// The password typed for a connection, and the one kept, forgotten:
    /// asked for when it next connects. Whether one was kept.
    fn forget_password(&mut self, name: &str) -> bool {
        self.passwords.remove(name);
        self.typed.remove(name);
        let key = conn::secret_key(name);
        let kept = fenix_extension_api::secret(&key).is_some();
        if kept {
            if let Err(e) = fenix_extension_api::set_secret(&key, None) {
                fenix_extension_api::notify(&format!("{name}: {e}"));
            }
        }
        kept
    }

    /// Passwords typed that signed in, kept in the keychain
    /// (`database.keep_passwords`).
    fn keep_passwords(&mut self) {
        let signed_in: Vec<String> = self.typed.iter().filter(|n| self.sessions.get(*n).is_some_and(|s| s.state == State::Ready)).cloned().collect();
        let keep = fenix_extension_api::setting("database.keep_passwords").is_none_or(|v| v.trim() != "false");
        for name in signed_in {
            self.typed.remove(&name);
            let Some(pw) = self.passwords.get(&name) else { continue };
            if keep {
                if let Err(e) = fenix_extension_api::set_secret(&conn::secret_key(&name), Some(pw)) {
                    fenix_extension_api::notify(&format!("{name}: its password isn't kept: {e}"));
                }
            }
        }
    }

    /// Its session, started if it isn't (a password asked for first, when
    /// there's none).
    fn start(&mut self, name: &str) {
        if self.sessions.get(name).is_some_and(|s| !matches!(s.state, State::Failed(_) | State::Closed)) {
            return;
        }
        // Signing in was refused: the password typed, or kept, is wrong.
        if let Some(State::Failed(why)) = self.sessions.get(name).map(|s| &s.state) {
            if conn::refused_sign_in(why) && self.forget_password(name) {
                fenix_extension_api::notify(&format!("{name}: the password kept for it was refused; it's asked for again"));
            }
        }
        let Some(c) = self.conn(name) else { return fenix_extension_api::notify(&format!("No connection called {name}")) };
        let password = match c.engine {
            Engine::Sqlite => Some(String::new()),
            _ if c.user.is_empty() => Some(String::new()),
            _ => self.password(name),
        };
        let Some(password) = password else {
            if !self.asked.values().any(|a| matches!(a, Asked::Password(n) if n == name)) {
                let id = fenix_extension_api::prompt_secret(&format!("{name}: {}'s password", c.user));
                self.asked.insert(id, Asked::Password(name.to_string()));
            }
            return;
        };
        let mut s = Session::new(c, &password, max_rows());
        s.connect(&mut Sockets);
        let lite = s.is_lite();
        self.cataloged.remove(name);
        self.sessions.insert(name.to_string(), s);
        for job in self.held.remove(name).unwrap_or_default() {
            if let Some(s) = self.sessions.get_mut(name) {
                s.submit(&mut Sockets, job);
            }
        }
        if lite {
            self.tick();
        }
        self.after();
    }

    fn submit(&mut self, name: &str, job: Job) {
        match self.sessions.get_mut(name) {
            Some(s) if !matches!(s.state, State::Failed(_) | State::Closed) => {
                let lite = s.is_lite();
                s.submit(&mut Sockets, job);
                if lite {
                    self.tick();
                }
            }
            _ => {
                self.held.entry(name.to_string()).or_default().push(job);
                self.start(name);
            }
        }
        self.after();
    }

    /// SQLite's jobs run soon, in a call of their own: a timer's.
    fn tick(&mut self) {
        if self.lite_timer.is_none() {
            self.lite_timer = Some(host::set_timer(1, false));
        }
    }

    /// What's finished, handled; schemas asked for once connected.
    fn after(&mut self) {
        let ready: Vec<String> = self.sessions.iter().filter(|(n, s)| s.state == State::Ready && !self.cataloged.contains(*n)).map(|(n, _)| n.clone()).collect();
        for name in ready {
            self.cataloged.insert(name.clone());
            let engine = self.sessions[&name].conn.engine;
            let database = self.sessions[&name].conn.database.clone();
            let sql = schema::catalog(engine, &database).join(";\n");
            let job = self.job(sql, false, Purpose::Catalog);
            if let Some(s) = self.sessions.get_mut(&name) {
                s.submit(&mut Sockets, job);
                if s.is_lite() {
                    self.tick();
                }
            }
        }
        let done: Vec<(String, Finished)> = self.sessions.iter_mut().flat_map(|(n, s)| s.take_finished().into_iter().map(move |f| (n.clone(), f))).collect();
        let any = !done.is_empty();
        for (name, f) in done {
            self.finished(&name, f);
        }
        if any {
            // A job done may have queued another.
            let more = self.sessions.values().any(|s| s.state == State::Ready && s.busy() && s.is_lite());
            if more {
                self.tick();
            }
        }
        self.keep_passwords();
        self.refresh_browser();
        self.update_status();
    }

    fn finished(&mut self, name: &str, f: Finished) {
        match f.job.purpose {
            Purpose::Catalog => {
                let engine = self.sessions.get(name).map(|s| s.conn.engine);
                if let (Some(engine), Some(s)) = (engine, self.sessions.get_mut(name)) {
                    if engine == Engine::Sqlite {
                        if let Some(Outcome::Error(e)) = f.outcomes.first() {
                            s.schema_error = Some(e.clone());
                        }
                    } else {
                        match schema::from_outcomes(engine, &f.outcomes) {
                            Ok(sc) => {
                                s.schema = Some(sc);
                                s.schema_error = None;
                            }
                            Err(e) => s.schema_error = Some(e),
                        }
                    }
                }
                // Table pages put back waiting for it.
                let waiting: Vec<u64> = self.awaiting.iter().copied().filter(|p| self.tables.get(p).is_some_and(|t| t.conn == name)).collect();
                for page in waiting {
                    self.awaiting.remove(&page);
                    let target = self.tables.get(&page).and_then(|t| self.target(name, t.target.schema.as_deref().unwrap_or(""), &t.target.table));
                    if let (Some(t), Some(target)) = (self.tables.get_mut(&page), target) {
                        t.target = target;
                    }
                    self.fetch(page);
                }
            }
            Purpose::Run(run) => {
                let Some(g) = self.runs.get_mut(&run) else { return };
                let part = g.outcomes.len();
                g.millis += f.millis;
                g.outcomes.push((part, f.outcomes));
                if g.outcomes.len() < g.parts {
                    return;
                }
                let g = self.runs.remove(&run).expect("gathered");
                let outcomes: Vec<Outcome> = g.outcomes.into_iter().flat_map(|(_, o)| o).collect();
                let at = fenix_extension_api::chrono::Utc::now().timestamp();
                let ok = !outcomes.iter().any(Outcome::is_error);
                let summary = outcomes.iter().map(Outcome::summary).collect::<Vec<_>>().join(" · ");
                self.history.push(Entry { at, conn: g.conn.clone(), sql: g.statements.join(";\n"), ok, summary: summary.clone(), millis: g.millis });
                if outcomes.iter().any(|o| matches!(o, Outcome::Done { .. } | Outcome::Error(_))) && !outcomes.iter().any(|o| matches!(o, Outcome::Rows(_))) {
                    fenix_extension_api::notify(&format!("{}: {}", g.conn, history::one_line(&summary, 120)));
                }
                self.browser.show(Ran { conn: g.conn.clone(), millis: g.millis, statements: g.statements.len() }, &g.statements, outcomes);
                // A schema change: the schema read again.
                if g.statements.iter().any(|s| sql::classify(s, self.conn(&g.conn).map(|c| c.engine.dialect()).unwrap_or(sql::Dialect::Postgres)) == sql::Kind::Schema) {
                    self.cataloged.remove(&g.conn);
                }
                self.show_browser();
            }
            Purpose::Page(page) => {
                if let Some(t) = self.tables.get_mut(&page) {
                    t.rows_came(f.outcomes);
                    pages::refresh(page);
                }
            }
            Purpose::Count(page) => {
                if let Some(t) = self.tables.get_mut(&page) {
                    t.count_came(f.outcomes);
                    pages::refresh(page);
                }
            }
            Purpose::Update(page) => {
                let refetch = self.tables.get_mut(&page).is_some_and(|t| t.update_came(&f.outcomes));
                if let Some(t) = self.tables.get(&page) {
                    let at = fenix_extension_api::chrono::Utc::now().timestamp();
                    let ok = !f.outcomes.iter().any(Outcome::is_error);
                    let summary = f.outcomes.iter().map(Outcome::summary).collect::<Vec<_>>().join(" · ");
                    self.history.push(Entry { at, conn: t.conn.clone(), sql: f.job.sql.clone(), ok, summary, millis: f.millis });
                }
                if refetch {
                    self.fetch(page);
                }
                pages::refresh(page);
            }
        }
    }

    /// The sidebar's tree: a node a connection, its schema under it.
    fn roots(&self) -> Vec<TreeNode> {
        let mut roots: Vec<TreeNode> = self
            .all_conns()
            .iter()
            .map(|c| {
                let nid = schema::id::connection(&c.name);
                let mut detail = c.engine.name().to_string();
                let mut flags = Vec::new();
                if c.read_only {
                    flags.push("read-only");
                }
                if c.production {
                    flags.push("production");
                }
                if !flags.is_empty() {
                    detail = format!("{detail} · {}", flags.join(", "));
                }
                let style = if c.production { Style::new(Role::Warn) } else { Style::default() };
                match self.sessions.get(&c.name) {
                    Some(s) => {
                        let mut children = match (&s.schema, &s.schema_error) {
                            (Some(sc), _) => schema::tree(&c.name, sc),
                            (None, Some(e)) => vec![TreeNode::leaf(format!("{nid}{}error", schema::id::SEP), e.clone()).style(Role::Bad)],
                            (None, None) => vec![TreeNode::leaf(format!("{nid}{}reading", schema::id::SEP), "reading the schema…").style(Role::Muted)],
                        };
                        if let State::Failed(e) = &s.state {
                            children = vec![TreeNode::leaf(format!("{nid}{}failed", schema::id::SEP), e.clone()).style(Role::Bad)];
                        }
                        let mark = match s.state {
                            State::Ready => "● ",
                            State::Connecting => "◌ ",
                            _ => "○ ",
                        };
                        TreeNode { id: nid, label: format!("{mark}{}", c.name), style, detail: Some(format!("{detail} · {}", s.status())), children, lazy: false }
                    }
                    None => TreeNode { lazy: true, ..TreeNode::leaf(nid, format!("○ {}", c.name)).style(style).detail(detail) },
                }
            })
            .collect();
        for (i, e) in self.errors.iter().enumerate() {
            roots.push(TreeNode::leaf(format!("x{}{i}", schema::id::SEP), e.clone()).style(Role::Bad));
        }
        roots
    }

    fn refresh_browser(&self) {
        if let Some(p) = self.browser_page {
            pages::refresh(p);
        }
    }

    /// The browser, brought up: beside you when it isn't open.
    fn show_browser(&mut self) {
        match self.browser_page {
            Some(p) => pages::refresh(p),
            None => self.browser_page = Some(pages::open_beside(BROWSER, "Database", 0.5)),
        }
    }

    fn open_browser(&mut self) {
        match self.browser_page {
            Some(p) => pages::focus(p),
            None => self.browser_page = Some(pages::open(BROWSER, "Database")),
        }
    }

    /// The modeline says the SQL buffer's connection.
    fn update_status(&self) {
        let Some(b) = &self.focus else { return fenix_extension_api::status(None) };
        let text = b.text().unwrap_or_default();
        match self.connection_for(b, &text) {
            Some(name) => {
                let state = self.sessions.get(&name).map_or("not connected".to_string(), |s| match &s.state {
                    State::Failed(_) => "failed".into(),
                    _ => s.status(),
                });
                fenix_extension_api::status(Some(&format!("db {name} · {state}")));
            }
            None => fenix_extension_api::status(Some("db · no connection (SPC m c)")),
        }
    }

    /// The connection a buffer runs on: its `-- connection:` line, the
    /// one picked for it, or the only one connected.
    fn connection_for(&self, b: &Buffer, text: &str) -> Option<String> {
        if let Some(n) = named_in(text).filter(|n| self.conn(n).is_some()) {
            return Some(n);
        }
        if let Some(n) = self.bindings.get(&b.key()).filter(|n| self.conn(n).is_some()) {
            return Some(n.clone());
        }
        let live: Vec<&String> = self.sessions.iter().filter(|(_, s)| s.state == State::Ready).map(|(n, _)| n).collect();
        match live.as_slice() {
            [only] => Some((*only).clone()),
            _ => None,
        }
    }

    /// The buffer you're in, if it's one a statement runs from.
    fn here_buffer(&self) -> Option<Buffer> {
        match fenix_extension_api::here().and_then(|h| h.path) {
            Some(p) => Some(Buffer::File(p)),
            None => self.focus.clone().filter(|b| matches!(b, Buffer::Scratch(_))),
        }
    }

    /// `database.run` (all: `database.run_all`).
    fn run(&mut self, all: bool) -> Result<(), String> {
        let b = self.here_buffer().ok_or("Run it from a .sql buffer")?;
        let text = b.text().ok_or("The buffer's text can't be read")?;
        let name = self.connection_for(&b, &text);
        let dialect = name.as_ref().and_then(|n| self.conn(n)).map(|c| c.engine.dialect()).unwrap_or(sql::Dialect::Postgres);
        let cursor = fenix_extension_api::cursor();
        let selection = cursor.as_ref().and_then(|c| c.selection);
        let piece = if all {
            text.clone()
        } else if let Some(((l1, c1), (l2, c2))) = selection {
            let (a, z) = (sql::offset(&text, l1, c1), sql::offset(&text, l2, c2));
            text[a.min(z)..a.max(z)].to_string()
        } else {
            let at = cursor.map(|c| sql::offset(&text, c.at.0, c.at.1)).unwrap_or(0);
            sql::statement_at(&text, at, dialect).map(|s| s.text).ok_or("No statement here")?
        };
        // Shown and kept without the `-- connection:` line it starts with.
        let statements: Vec<String> = sql::split(&piece, dialect).into_iter().map(|s| without_naming(&s.text)).collect();
        if statements.is_empty() {
            return Err("No statement here".into());
        }
        let run = PendingRun { statements, sql: piece };
        match name {
            Some(n) => self.go(&n, run),
            None => self.pick_connection(b, Some(run)),
        }
        Ok(())
    }

    fn pick_connection(&mut self, buffer: Buffer, run: Option<PendingRun>) {
        let conns = self.all_conns();
        if conns.is_empty() {
            fenix_extension_api::notify("No connections: add them in database.connections, or open a .sqlite file");
            return;
        }
        let items: Vec<PickerItem> = conns.iter().map(|c| PickerItem::new(&c.name, &c.name).detail(format!("{} · {}", c.engine.name(), c.place()))).collect();
        let id = fenix_extension_api::pick("The connection this buffer runs on", &items);
        self.asked.insert(id, Asked::Connection { buffer, run });
    }

    /// A run on `name`, past the safety rules.
    fn go(&mut self, name: &str, run: PendingRun) {
        let Some(c) = self.conn(name) else { return };
        let texts: Vec<&str> = run.statements.iter().map(String::as_str).collect();
        match conn::check(&c, &texts) {
            Verdict::Run => self.send(name, run),
            Verdict::Refuse(why) => fenix_extension_api::notify(&why),
            Verdict::Ask(why) => {
                let items = [PickerItem::new("Run it", "run").detail(history::one_line(&run.sql, 80)), PickerItem::new("Don't", "cancel")];
                let id = fenix_extension_api::pick(&why, &items);
                self.asked.insert(id, Asked::Confirm { conn: name.to_string(), run });
            }
        }
    }

    fn send(&mut self, name: &str, run: PendingRun) {
        self.last_conn = Some(name.to_string());
        let engine = self.conn(name).map(|c| c.engine);
        self.next_job += 1;
        let id = self.next_job;
        // SQL Server: a batch a statement, as `GO` splits them.
        let parts: Vec<String> = if engine == Some(Engine::Mssql) { run.statements.clone() } else { vec![run.sql.clone()] };
        self.runs.insert(id, Gathering { conn: name.to_string(), statements: run.statements.clone(), parts: parts.len(), outcomes: Vec::new(), millis: 0 });
        for p in parts {
            let job = self.job(p, false, Purpose::Run(id));
            self.submit(name, job);
        }
    }

    /// A table's rows asked for again.
    fn fetch(&mut self, page: u64) {
        let Some(t) = self.tables.get(&page) else { return };
        let (select, count) = t.queries();
        let name = t.conn.clone();
        let dialect = t.target.dialect;
        let one = sql::split(&select, dialect).len() == 1 && sql::classify(&select, dialect) == sql::Kind::Read;
        if !one {
            if let Some(t) = self.tables.get_mut(&page) {
                t.loading = false;
                t.error = Some("The filter is a WHERE's condition: one, with nothing after it".into());
            }
            pages::refresh(page);
            return;
        }
        let a = self.job(select, true, Purpose::Page(page));
        let b = self.job(count, true, Purpose::Count(page));
        self.submit(&name, a);
        self.submit(&name, b);
    }

    /// What a table view keys its rows by, from the schema.
    fn target(&self, conn: &str, schema: &str, table: &str) -> Option<Target> {
        let c = self.conn(conn)?;
        let s = self.sessions.get(conn)?.schema.as_ref()?;
        let t = s.table(schema, table)?;
        let key = t.primary_key();
        Some(Target {
            dialect: c.engine.dialect(),
            schema: if c.engine == Engine::Sqlite { None } else { Some(schema.to_string()) },
            table: table.to_string(),
            rowid: c.engine == Engine::Sqlite && key.is_empty() && !t.view,
            key,
        })
    }

    fn open_table(&mut self, conn: &str, schema: &str, table: &str) {
        let Some(target) = self.target(conn, schema, table) else { return };
        let ro = self.conn(conn).is_some_and(|c| c.read_only);
        let view = TableView::new(conn, target, page_rows(), ro);
        let page = pages::open(TABLE, &format!("{} · {}", view.title(), conn));
        self.tables.insert(page, view);
        self.fetch(page);
    }

    fn intent(&mut self, i: Intent) {
        match i {
            Intent::Connect(n) => {
                if self.sessions.get(&n).is_some_and(|s| s.state == State::Ready) {
                    return;
                }
                if let Some(mut s) = self.sessions.remove(&n) {
                    // Failed: a password typed may be why; it's asked again.
                    if matches!(s.state, State::Failed(_)) {
                        self.passwords.remove(&n);
                        self.typed.remove(&n);
                    }
                    s.close(&mut Sockets);
                }
                self.browser.tree.expand(&schema::id::connection(&n));
                self.start(&n);
            }
            Intent::Disconnect(n) => {
                if let Some(mut s) = self.sessions.remove(&n) {
                    s.close(&mut Sockets);
                }
                self.cataloged.remove(&n);
                self.browser.tree.collapse(&schema::id::connection(&n));
                self.browser.note = Some((format!("{n}: disconnected"), Role::Muted));
            }
            Intent::Refresh(n) => {
                if let Some(s) = self.sessions.get_mut(&n) {
                    s.schema = None;
                }
                self.cataloged.remove(&n);
            }
            Intent::OpenTable { conn, schema, table } => self.open_table(&conn, &schema, &table),
            Intent::NewQuery(n) => {
                let id = scratch::open(&format!("{n}.sql"), &format!("-- connection: {n}\n\n"), Some("sql"));
                self.scratches.insert(id);
                self.bindings.insert(Buffer::Scratch(id).key(), n);
            }
            Intent::Use(n) => match self.focus.clone() {
                Some(b) => {
                    self.bindings.insert(b.key(), n.clone());
                    self.save_bindings();
                    self.browser.note = Some((format!("The .sql buffer runs on {n} now"), Role::Good));
                }
                None => self.browser.note = Some(("No .sql buffer to use it for: go to one first".into(), Role::Warn)),
            },
            Intent::Copy(text) => {
                fenix_extension_api::copy(&text);
                self.browser.note = Some((format!("Copied {} character{}", text.chars().count(), if text.chars().count() == 1 { "" } else { "s" }), Role::Muted));
            }
            Intent::Scratch { title, text, language } => {
                scratch::open_beside(&title, &text, Some(&language), 0.5);
            }
            Intent::ExportFile { text, extension } => {
                let id = fenix_extension_api::prompt("Export to (a path in the project)", &format!("results.{extension}"));
                self.asked.insert(id, Asked::Export { text });
            }
            Intent::History => self.show_history(None),
            Intent::Say(s) => self.browser.note = Some((s, Role::Muted)),
        }
        self.after();
    }

    fn show_history(&mut self, into: Option<Buffer>) {
        if self.history.entries.is_empty() {
            fenix_extension_api::notify("Nothing run yet");
            return;
        }
        let items: Vec<PickerItem> = self
            .history
            .search("")
            .iter()
            .enumerate()
            .take(500)
            .map(|(i, e)| {
                let when = fenix_extension_api::clock::local(fenix_extension_api::chrono::DateTime::from_timestamp(e.at, 0).unwrap_or_default()).format("%Y-%m-%d %H:%M");
                PickerItem::new(history::one_line(&e.sql, 100), i.to_string()).detail(format!("{} · {when} · {}{}", e.conn, if e.ok { "" } else { "failed: " }, history::one_line(&e.summary, 60)))
            })
            .collect();
        let id = fenix_extension_api::pick("History: the statements you ran (type to search)", &items);
        self.asked.insert(id, Asked::History { into });
    }

    fn picked(&mut self, id: u64, value: Option<String>) {
        let Some(asked) = self.asked.remove(&id) else { return };
        let Some(value) = value else { return };
        match asked {
            Asked::Connection { buffer, run } => {
                self.bindings.insert(buffer.key(), value.clone());
                self.save_bindings();
                match run {
                    Some(r) => self.go(&value, r),
                    None => self.start(&value),
                }
            }
            Asked::Forget => {
                let note = if self.forget_password(&value) { "its password forgotten" } else { "no password was kept for it" };
                fenix_extension_api::notify(&format!("{value}: {note}"));
            }
            Asked::Confirm { conn, run } => {
                if value == "run" {
                    self.send(&conn, run);
                }
            }
            Asked::History { into } => {
                let Some(e) = value.parse::<usize>().ok().and_then(|i| self.history.search("").get(i).map(|e| (*e).clone())) else { return };
                let sql = if e.sql.trim_end().ends_with(';') { e.sql.clone() } else { format!("{};", e.sql) };
                match into {
                    Some(Buffer::File(_)) => {
                        if let Err(why) = buffer::insert(&sql) {
                            fenix_extension_api::notify(&why);
                        }
                    }
                    _ => {
                        let id = scratch::open(&format!("{}.sql", e.conn), &format!("-- connection: {}\n\n{sql}\n", e.conn), Some("sql"));
                        self.scratches.insert(id);
                        self.bindings.insert(Buffer::Scratch(id).key(), e.conn.clone());
                    }
                }
            }
            Asked::Password(_) | Asked::Export { .. } => {}
        }
        self.after();
    }

    fn prompted(&mut self, id: u64, text: Option<String>) {
        let Some(asked) = self.asked.remove(&id) else { return };
        match (asked, text) {
            (Asked::Password(name), Some(pw)) => {
                self.passwords.insert(name.clone(), pw);
                self.typed.insert(name.clone());
                self.start(&name);
            }
            (Asked::Password(name), None) => {
                for job in self.held.remove(&name).unwrap_or_default() {
                    if let Purpose::Run(run) = job.purpose {
                        self.runs.remove(&run);
                    }
                }
            }
            (Asked::Export { text }, Some(path)) => {
                let rel = path.trim().trim_start_matches(['/', '\\']).trim_start_matches("project/");
                if rel.is_empty() || rel.split(['/', '\\']).any(|p| p == "..") {
                    return fenix_extension_api::notify("A path in the project: results.csv, out/rows.json");
                }
                let full = format!("/project/{rel}");
                if let Some(dir) = std::path::Path::new(&full).parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                match std::fs::write(&full, text) {
                    Ok(()) => fenix_extension_api::notify(&format!("Wrote {rel}")),
                    Err(e) => fenix_extension_api::notify(&format!("{rel}: {e}")),
                }
            }
            _ => {}
        }
        self.after();
    }

    /// A SQLite file opened: a connection of its own, its schema in the
    /// browser.
    fn opened(&mut self, full: &str) {
        let project = in_project(full);
        let (path, read_only) = match &project {
            Some(p) => (p.clone(), false),
            None => {
                // Outside the project: read, and opened as a copy.
                let bytes = match fenix_extension_api::read_file(full) {
                    Ok(b) => b,
                    Err(e) => return fenix_extension_api::notify(&format!("{full}: {e}")),
                };
                if !bytes.starts_with(b"SQLite format 3\0") {
                    return host::open_file(full, None);
                }
                let _ = std::fs::create_dir_all(COPIES);
                let stem: String = full.rsplit(['/', '\\']).next().unwrap_or("db").chars().filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
                let hash = full.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
                let copy = format!("{COPIES}/{hash:016x}-{stem}");
                if let Err(e) = std::fs::write(&copy, &bytes) {
                    return fenix_extension_api::notify(&format!("{copy}: {e}"));
                }
                (copy, true)
            }
        };
        if project.is_some() {
            let header = std::fs::read(&path).map(|b| b.starts_with(b"SQLite format 3\0")).unwrap_or(false);
            if !header {
                return host::open_file(full, None);
            }
        }
        let mut c = Conn::file(&path, read_only);
        c.name = full.replace('\\', "/").rsplit('/').next().unwrap_or(full).to_string();
        while self.saved.iter().any(|s| s.name == c.name) || self.files.iter().any(|f| f.name == c.name && f.database != path) {
            c.name.push('\'');
        }
        if read_only {
            c.name.push_str(" (copy)");
        }
        if let Some(i) = self.files.iter().position(|f| f.name == c.name) {
            self.files[i] = c.clone();
            if let Some(mut s) = self.sessions.remove(&c.name) {
                s.close(&mut Sockets);
            }
        } else {
            self.files.push(c.clone());
        }
        let nid = schema::id::connection(&c.name);
        self.browser.tree.expand(&nid);
        self.start(&c.name);
        self.open_browser();
        let roots = self.roots();
        self.browser.tree.select_id(&roots, &nid);
        if let Some(p) = self.browser_page {
            let _ = pages::file(p, Some(full));
        }
        self.after();
    }
}

/// Completion from the schema of the buffer's connection.
struct Completer<'a> {
    app: &'a App,
}

impl LanguageServer for Completer<'_> {
    fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities { completion_provider: Some(CompletionOptions { trigger_characters: Some(vec![".".into()]), ..Default::default() }), ..Default::default() }
    }

    fn completion(&mut self, _server: u64, docs: &Documents, params: CompletionParams) -> Option<CompletionResponse> {
        let at = &params.text_document_position;
        let text = docs.text(&at.text_document.uri)?;
        let path = uri_path(at.text_document.uri.as_str());
        let b = Buffer::File(path);
        let conn = self.app.connection_for(&b, text).or_else(|| self.app.last_conn.clone());
        let session = conn.as_ref().and_then(|n| self.app.sessions.get(n));
        let dialect = session.map(|s| s.conn.engine.dialect()).unwrap_or(sql::Dialect::Postgres);
        let line = text.lines().nth(at.position.line as usize).unwrap_or("");
        let column = lsp::utf16_to_char(line, at.position.character) as u32;
        let offset = sql::offset(text, at.position.line, column);
        let (_, items) = complete::complete(session.and_then(|s| s.schema.as_ref()), text, offset, dialect);
        let items = items
            .into_iter()
            .map(|i| CompletionItem {
                label: i.label,
                kind: Some(match i.what {
                    complete::What::Schema => CompletionItemKind::MODULE,
                    complete::What::Table => CompletionItemKind::CLASS,
                    complete::What::View => CompletionItemKind::INTERFACE,
                    complete::What::Column => CompletionItemKind::FIELD,
                    complete::What::Keyword => CompletionItemKind::KEYWORD,
                }),
                detail: (!i.detail.is_empty()).then_some(i.detail),
                ..Default::default()
            })
            .collect();
        Some(CompletionResponse::Array(items))
    }
}

impl Extension for App {
    fn activate(&mut self) -> Result<(), String> {
        self.history = History::load(HISTORY);
        self.load_bindings();
        self.reload();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "database.open" => self.open_browser(),
            "database.run" => self.run(false)?,
            "database.run_all" => self.run(true)?,
            "database.connection" => {
                let b = self.here_buffer().ok_or("Pick it from a .sql buffer")?;
                self.pick_connection(b, None);
            }
            "database.history" => {
                let into = self.here_buffer();
                self.show_history(into);
            }
            "database.forget_password" => {
                let items: Vec<PickerItem> = self.saved.iter().filter(|c| c.engine != Engine::Sqlite).map(|c| PickerItem::new(&c.name, &c.name).detail(format!("{} · {}", c.engine.name(), c.place()))).collect();
                if items.is_empty() {
                    return Err("No connections to a server: add them in database.connections".into());
                }
                let id = fenix_extension_api::pick("Forget the password kept for", &items);
                self.asked.insert(id, Asked::Forget);
            }
            "database.new_query" => match self.last_conn.clone().or_else(|| self.all_conns().first().map(|c| c.name.clone())) {
                Some(n) => self.intent(Intent::NewQuery(n)),
                None => return Err("No connections: add them in database.connections".into()),
            },
            _ => return Err(format!("no command {id}")),
        }
        self.after();
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::SocketConnected((s, r)) => {
                if let Some(sess) = self.sessions.values_mut().find(|x| x.socket() == Some(s)) {
                    sess.connected(&mut Sockets, r);
                }
            }
            Event::SocketData((s, bytes)) => {
                if let Some(sess) = self.sessions.values_mut().find(|x| x.socket() == Some(s)) {
                    sess.data(&mut Sockets, &bytes);
                }
            }
            Event::SocketClosed((s, why)) => {
                if let Some(sess) = self.sessions.values_mut().find(|x| x.socket() == Some(s)) {
                    sess.closed(why);
                }
            }
            Event::Timer(t) if self.lite_timer == Some(t) => {
                self.lite_timer = None;
                let n = max_rows();
                for s in self.sessions.values_mut().filter(|s| s.is_lite()) {
                    s.run_lite(n);
                }
            }
            Event::Picked((id, value)) => return self.picked(id, value),
            Event::Prompted((id, text)) => return self.prompted(id, text),
            Event::SettingChanged(key) if key == "database.connections" || key == "database.timeout" => self.reload(),
            Event::Opened(path) => return self.opened(&path),
            _ => return,
        }
        self.after();
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        let canvas = if Some(page) == self.browser_page {
            self.browser_w = width as usize;
            self.browser.render(&self.roots(), width as usize, height as usize)
        } else if let Some(t) = self.tables.get(&page) {
            t.render(width as usize, height as usize)
        } else {
            return Page::new().line([pages::muted("Nothing here")]);
        };
        canvas.to_page()
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let key: fenix_ui_kit::Key = key.into();
        if Some(page) == self.browser_page {
            let roots = self.roots();
            if let Some(i) = self.browser.key(key, &roots) {
                self.intent(i);
            }
        } else if let Some(t) = self.tables.get_mut(&page) {
            let ask = t.key(key);
            self.table_ask(page, ask);
        }
    }

    fn page_key_press(&mut self, page: u64, key: KeyPress) {
        let Some(press) = fenix_ui_kit::KeyPress::from_api(&key) else { return };
        if Some(page) == self.browser_page {
            let k = match press.key {
                fenix_ui_kit::Key::PageDown => fenix_ui_kit::Key::Char('J'),
                fenix_ui_kit::Key::PageUp => fenix_ui_kit::Key::Char('K'),
                _ => return,
            };
            let roots = self.roots();
            if let Some(i) = self.browser.key(k, &roots) {
                self.intent(i);
            }
        } else if let Some(t) = self.tables.get_mut(&page) {
            let ask = t.key_press(press);
            self.table_ask(page, ask);
        }
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        let m = fenix_ui_kit::Mouse::from(&event);
        if Some(page) == self.browser_page {
            let roots = self.roots();
            let split = Browser::side_width(self.browser_w.max(40));
            if let Some(i) = self.browser.mouse(&m, &roots, split) {
                self.intent(i);
            }
        } else if let Some(t) = self.tables.get_mut(&page) {
            let ask = t.mouse(&m);
            self.table_ask(page, ask);
        }
    }

    fn page_paste(&mut self, page: u64, text: &str) {
        if let Some(t) = self.tables.get_mut(&page) {
            t.paste(text);
        }
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        if Some(page) == self.browser_page {
            return Some("{}".into());
        }
        serde_json::to_string(&self.tables.get(&page)?.saved()).ok()
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        match kind {
            BROWSER => {
                self.browser_page = Some(page);
                true
            }
            TABLE => {
                let Ok(s) = serde_json::from_str::<tableview::Saved>(state) else { return false };
                let Some(c) = self.conn(&s.conn) else { return false };
                let target = Target { dialect: c.engine.dialect(), schema: s.schema.clone(), table: s.table.clone(), key: Vec::new(), rowid: false };
                let mut v = TableView::new(&s.conn, target, page_rows(), c.read_only);
                v.filter = s.filter;
                v.page = s.page;
                self.tables.insert(page, v);
                self.awaiting.insert(page);
                self.start(&s.conn);
                true
            }
            _ => false,
        }
    }

    fn page_closed(&mut self, page: u64) {
        if Some(page) == self.browser_page {
            self.browser_page = None;
        }
        self.tables.remove(&page);
        self.awaiting.remove(&page);
    }

    fn focus_moved(&mut self, focus: Focus) {
        self.focus = match focus {
            Focus::File(Some(p)) if p.to_ascii_lowercase().ends_with(".sql") => Some(Buffer::File(p)),
            Focus::Text(Some(id)) if self.scratches.contains(&id) => Some(Buffer::Scratch(id)),
            // A page (the browser's, say): the buffer you were in is kept.
            Focus::Page(_) => return,
            _ => None,
        };
        self.update_status();
    }

    fn buffer_changed(&mut self, _what: Changed) {
        self.update_status();
    }

    fn scratch_closed(&mut self, id: u64) {
        self.scratches.remove(&id);
        self.bindings.remove(&Buffer::Scratch(id).key());
        if self.focus == Some(Buffer::Scratch(id)) {
            self.focus = None;
            self.update_status();
        }
    }

    fn lsp_message(&mut self, server: u64, message: &str) {
        let mut servers = std::mem::take(&mut self.servers);
        servers.handle(server, message, &mut Completer { app: self });
        self.servers = servers;
    }
}

impl App {
    fn table_ask(&mut self, page: u64, ask: Option<Ask>) {
        match ask {
            Some(Ask::Fetch) => self.fetch(page),
            Some(Ask::Update(sql)) => {
                let Some(t) = self.tables.get(&page) else { return };
                let name = t.conn.clone();
                if self.conn(&name).is_some_and(|c| c.read_only) {
                    return;
                }
                let job = self.job(sql, true, Purpose::Update(page));
                self.submit(&name, job);
            }
            Some(Ask::Copy(text)) => {
                fenix_extension_api::copy(&text);
                if let Some(t) = self.tables.get_mut(&page) {
                    t.note = Some(("Copied".into(), Role::Muted));
                }
            }
            None => {}
        }
        pages::refresh(page);
    }
}

export!(App);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_uris() {
        assert_eq!(uri_path("file:///home/me/q%20uery.sql"), "/home/me/q uery.sql");
        assert_eq!(uri_path("file:///C:/data/q.sql"), "C:/data/q.sql");
        assert_eq!(norm("C:\\data\\q.sql"), "c:/data/q.sql");
        assert_eq!(norm(&uri_path("file:///C:/data/q.sql")), norm("C:\\data\\q.sql"), "the server's and the editor's paths meet");
    }

    #[test]
    fn a_buffer_names_its_connection() {
        assert_eq!(named_in("-- connection: prod\nselect 1").as_deref(), Some("prod"));
        assert_eq!(named_in("\n-- Database:  local dev \n").as_deref(), Some("local dev"));
        assert_eq!(named_in("select 1 -- connection: x"), None);
        assert_eq!(named_in("-- connection:\n"), None);
        assert_eq!(without_naming("-- connection: prod\n-- why\nselect 1"), "-- why\nselect 1");
    }
}
