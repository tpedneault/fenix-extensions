//! Protocol Buffers and gRPC in Fenix, on lang-kit: the grammar (its
//! language file and queries), buf's language server (`buf lsp serve`,
//! `buf beta lsp` on an older buf, protols when it's the one asked for or
//! the only one there), `buf format` as the formatter, `buf lint` on save,
//! `buf breaking` against the default branch as a task and a check of the
//! doctor's, `buf generate` for each template and `buf build` as tasks, and
//! the doctor's checks of buf, protoc and grpcurl, each with a fix row that
//! installs the release for this machine.
//!
//! The gRPC client (`protobuf.client`) is a page on grpcurl: the servers
//! are `protobuf.targets`, their services come from their reflection or
//! from the project's `.proto` files (built by buf into a descriptor set
//! when there's a workspace), a method's body is JSON in a scratch buffer,
//! made first from its input message, and a call's messages show as they
//! come, with its status, headers, trailers and timing, and go into the
//! method's history.

pub mod doctor;
pub mod grpc;
pub mod lint;
pub mod project;
pub mod tasks;
pub mod tools;
pub mod ui;

#[cfg(test)]
mod data;

use std::collections::{HashMap, HashSet};

use fenix_extension_api::{self as api, export, Changed, Command, Event, Extension, Key, MouseEvent, Page, PickerItem};
use fenix_lang_kit::exec::Runs;
use fenix_lang_kit::{find_version, Fenix, Host, Kit, Version};

use doctor::{BufCan, Breaking};
use grpc::call::{Outcome as Ended, Reader, Received, Reply};
use grpc::describe::{parse_describe, parse_list, Described, Service};
use grpc::history::{Entry, History};
use grpc::{Grpcurl, Source, Target};
use project::Project;
use tools::{Platform, Tools};
use ui::client::{Action, ClientPage, Data};

/// The project, as the extension sees it.
pub const PROJECT: &str = "/project";

/// Fenix's name for the language, the grammar's.
const PROTO: &[&str] = &["proto"];

/// The client's page.
pub const CLIENT: &str = "client";

/// The history, in the extension's own folder.
pub const HISTORY: &str = "/data/history.json";

/// The descriptor set `buf build` writes for the client, as a program's
/// argument names it.
pub const PROTOSET: &str = "${data}/schema.binpb";

/// Seconds grpcurl has to connect.
const CONNECT_SECS: u32 = 10;

/// Which language server the pack sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Server {
    Buf,
    Protols,
    None,
}

/// What the pack's settings say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `buf lint` on a saved file.
    pub lint: bool,
    pub server: Server,
    /// The branch `buf breaking` compares with; empty, the default branch.
    pub against: String,
    /// `buf breaking` run when a `.proto` file is saved, for the doctor.
    pub breaking_check: bool,
    pub targets: Vec<Target>,
    /// TLS without checking the server's certificate (`-insecure`).
    pub insecure: bool,
    /// Seconds a call may take (`-max-time`), 0 no limit.
    pub timeout: u32,
    /// Calls kept per method.
    pub history: usize,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { lint: true, server: Server::Buf, against: String::new(), breaking_check: true, targets: Vec::new(), insecure: false, timeout: 30, history: 50 }
    }
}

impl Settings {
    /// From Fenix: a bool that isn't `false` is on.
    pub fn read() -> Settings {
        let on = |key: &str| !api::setting(key).is_some_and(|v| v.trim().eq_ignore_ascii_case("false"));
        let int = |key: &str, default: u64| api::setting(key).and_then(|v| v.trim().parse().ok()).unwrap_or(default);
        let server = match api::setting("protobuf.server").as_deref().map(str::trim) {
            Some("protols") => Server::Protols,
            Some("none") => Server::None,
            _ => Server::Buf,
        };
        Settings {
            lint: on("protobuf.lint"),
            server,
            against: api::setting("protobuf.breaking_against").unwrap_or_default().trim().to_string(),
            breaking_check: on("protobuf.breaking_check"),
            targets: grpc::parse_targets(&api::setting("protobuf.targets").unwrap_or_default()),
            insecure: api::setting("protobuf.insecure").is_some_and(|v| v.trim() == "true"),
            timeout: int("protobuf.timeout", 30) as u32,
            history: int("protobuf.history", 50) as usize,
        }
    }
}

/// What the pack asks of Fenix beyond lang-kit's [`Host`]: the body's
/// scratch buffer, its own settings changed, the clipboard, text in a
/// buffer, a page brought forward, the clock. A test gives its own.
pub trait Editor {
    /// A scratch buffer beside, highlighted as JSON: its id.
    fn scratch_open(&mut self, title: &str, text: &str) -> u64;
    fn scratch_text(&mut self, id: u64) -> Option<String>;
    fn scratch_set(&mut self, id: u64, text: &str);
    fn set_setting(&mut self, key: &str, value: &str) -> Result<(), String>;
    fn copy(&mut self, text: &str);
    fn open_text(&mut self, title: &str, text: &str);
    fn focus_page(&mut self, page: u64);
    /// Milliseconds since 1970.
    fn now_ms(&mut self) -> u64;
}

/// The real thing.
pub struct FenixEditor;

impl Editor for FenixEditor {
    fn scratch_open(&mut self, title: &str, text: &str) -> u64 {
        api::scratch::open_beside(title, text, Some("json"), 0.4)
    }
    fn scratch_text(&mut self, id: u64) -> Option<String> {
        api::scratch::text(id)
    }
    fn scratch_set(&mut self, id: u64, text: &str) {
        let _ = api::scratch::set(id, text);
    }
    fn set_setting(&mut self, key: &str, value: &str) -> Result<(), String> {
        api::set_setting(key, Some(value))
    }
    fn copy(&mut self, text: &str) {
        api::copy(text)
    }
    fn open_text(&mut self, title: &str, text: &str) {
        api::open_text(title, text, Some("json"))
    }
    fn focus_page(&mut self, page: u64) {
        api::page::focus(page)
    }
    fn now_ms(&mut self) -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }
}

/// What a grpcurl the client started is for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ask {
    /// `buf build` writing the descriptor set.
    Protoset,
    List,
    Service(String),
    /// A message or an enum, for a body's skeleton.
    Type(String),
}

/// What a prompt or a picker the client opened is for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asked {
    AddTarget,
    PickTarget,
    /// A metadata line: a new one, or the one at this row.
    Metadata(String, Option<usize>),
}

/// A call under way.
struct Call {
    process: u64,
    method: String,
    started: u64,
    reader: Reader,
    body: String,
    metadata: Vec<(String, String)>,
    target: String,
}

/// The gRPC client's state.
#[derive(Default)]
pub struct Client {
    pub page: Option<u64>,
    pub ui: ClientPage,
    /// The target chosen, of `protobuf.targets`.
    pub target: usize,
    /// The project's protos, not the server's reflection.
    pub protos: bool,
    pub described: Described,
    /// The services listed, in their order, as described.
    pub names: Vec<String>,
    pub loading: Option<String>,
    pub error: Option<String>,
    asks: Runs<Ask>,
    /// The types being described.
    pending: HashSet<String>,
    /// The method chosen: service, method.
    pub selected: Option<(String, String)>,
    /// Bodies by method path, as last written.
    pub bodies: HashMap<String, String>,
    pub metadata: HashMap<String, Vec<(String, String)>>,
    /// The body's scratch buffer, and the method it's for.
    pub scratch: Option<(u64, String)>,
    call: Option<Call>,
    /// Each method's last answer.
    pub replies: HashMap<String, Reply>,
    pub history: History,
    asked: HashMap<u64, Asked>,
    /// The descriptor set has been built since the project changed.
    protoset: bool,
    /// The body is being made again: the scratch buffer takes it.
    reset: bool,
}

impl Client {
    /// The selected method's path: `svc/Method`.
    pub fn path(&self) -> Option<String> {
        self.selected.as_ref().map(|(s, m)| format!("{s}/{m}"))
    }

    /// The services, as listed.
    pub fn services(&self) -> Vec<Service> {
        self.names.iter().filter_map(|n| self.described.service(n).cloned()).collect()
    }
}

pub struct Protobuf {
    pub kit: Kit,
    pub editor: Box<dyn Editor>,
    root: String,
    project: Project,
    pub tools: Tools,
    platform: Platform,
    settings: Settings,
    read_settings: fn() -> Settings,
    /// buf's version, as the doctor last read it.
    buf_version: Option<Version>,
    /// What's set in Fenix, so it's set again only when it changes.
    set_server: Option<Command>,
    set_formatter: Option<Command>,
    /// `buf breaking` running for the doctor, and what it found last.
    breaking: Runs<String>,
    pub found: Breaking,
    pub client: Client,
    pub history_path: String,
}

impl Default for Protobuf {
    fn default() -> Protobuf {
        let platform = api::platform();
        Protobuf::new(Box::new(Fenix), Box::new(FenixEditor), PROJECT, Settings::read, Platform::new(&platform.os, &platform.arch))
    }
}

/// Whether `body` reads as JSON messages: one or more, one after another
/// (a client stream's), and how many.
pub fn messages_in(body: &str) -> Result<usize, String> {
    let mut n = 0;
    for v in serde_json::Deserializer::from_str(body).into_iter::<serde_json::Value>() {
        v.map_err(|e| format!("the body isn't JSON: {e}"))?;
        n += 1;
    }
    if n == 0 {
        return Err("the body is empty: {} sends an empty message".into());
    }
    Ok(n)
}

impl Protobuf {
    /// The pack for the project in `root`, talking to `host` and `editor`,
    /// on `platform`.
    pub fn new(mut host: Box<dyn Host>, editor: Box<dyn Editor>, root: &str, read_settings: fn() -> Settings, platform: Platform) -> Protobuf {
        let tools = Tools::find(host.as_mut());
        let mut kit = Kit::with_host("protobuf", host);
        for install in platform.installs() {
            kit.installer.add(install);
        }
        let mut p = Protobuf {
            kit,
            editor,
            root: root.into(),
            project: Project::read(root),
            tools,
            platform,
            settings: read_settings(),
            read_settings,
            buf_version: None,
            set_server: None,
            set_formatter: None,
            breaking: Runs::new(),
            found: Breaking::NotRun,
            client: Client::default(),
            history_path: HISTORY.into(),
        };
        p.client.protos = p.project.buf_dir.is_some() && p.settings.targets.is_empty();
        p.configure();
        p
    }

    /// The branch `buf breaking` compares with: the setting's, else the
    /// default branch.
    fn against(&self) -> Option<String> {
        if !self.settings.against.is_empty() {
            return Some(self.settings.against.clone());
        }
        self.project.default_branch.clone()
    }

    /// The kit's tasks, checks and linter, for the project, tools and
    /// settings as they are.
    fn configure(&mut self) {
        self.kit.tasks = tasks::tasks(&self.project, &self.tools, self.against().as_deref(), &self.root);
        self.kit.doctor.programs.clear();
        for program in doctor::programs(&self.project, self.settings.server == Server::Protols, &self.platform) {
            self.kit.doctor.program(program);
        }
        self.kit.doctor.extra = doctor::breaking(&self.found).into_iter().collect();
        self.kit.lint.linters.clear();
        if let (true, Some(buf), Some(dir)) = (self.settings.lint, &self.tools.buf, &self.project.buf_dir) {
            self.kit.lint.add(lint::buf_lint(&Command::new(buf), &self.project.cwd(), dir));
        }
    }

    /// The language server buf's version (or protols) gives, as asked.
    fn server(&self) -> Option<Command> {
        let can = BufCan::of(self.buf_version.as_ref());
        let buf = self.tools.buf.as_deref().filter(|_| can.lsp).map(|b| if can.lsp_serve { Command::new(b).args(["lsp", "serve"]) } else { Command::new(b).args(["beta", "lsp"]) });
        let protols = self.tools.protols.as_deref().map(Command::new);
        match self.settings.server {
            Server::Buf => buf.or(protols),
            Server::Protols => protols.or(buf),
            Server::None => None,
        }
    }

    /// The server and the formatter set in Fenix, where they changed.
    fn apply(&mut self) {
        let server = self.server();
        let can = BufCan::of(self.buf_version.as_ref());
        let formatter = self.tools.buf.as_deref().filter(|_| can.stdin_format).map(|b| Command::new(b).args(["format", "--stdin-filepath", "${file}"]));
        let languages: Vec<String> = PROTO.iter().map(|l| l.to_string()).collect();
        let host = self.kit.host.as_mut();
        if server != self.set_server {
            if let Err(why) = host.language_server(&languages, server.as_ref()) {
                host.log(&format!("language server: {why}"));
            }
            self.set_server = server;
        }
        if formatter != self.set_formatter {
            if let Err(why) = host.formatter(&languages, formatter.as_ref()) {
                host.log(&format!("formatter: {why}"));
            }
            self.set_formatter = formatter;
        }
    }

    fn set_tasks(&mut self) {
        if let Err(why) = self.kit.tasks.apply(self.kit.host.as_mut()) {
            self.kit.host.log(&format!("tasks: {why}"));
        }
    }

    /// `buf breaking` against the branch, in the background, for the
    /// doctor: when there's buf, a workspace and a repository.
    fn check_breaking(&mut self) {
        self.breaking.kill_all(self.kit.host.as_mut());
        let (Some(buf), Some(_), Some(branch), true) = (self.tools.buf.clone(), &self.project.buf_dir, self.against(), self.settings.breaking_check) else {
            self.found = Breaking::NotRun;
            return;
        };
        let cmd = Command::new(&buf).args(["breaking".to_string(), "--against".to_string(), self.project.against(&branch), "--error-format".to_string(), "json".to_string()]).cwd(&self.project.cwd());
        match self.breaking.start(self.kit.host.as_mut(), &cmd, branch.clone()) {
            Ok(_) => self.found = Breaking::Running(branch),
            Err(why) => self.found = Breaking::Failed(why),
        }
        self.publish_checks();
    }

    /// The doctor's checks set again, with the breaking check as it is.
    fn publish_checks(&mut self) {
        self.kit.doctor.extra = doctor::breaking(&self.found).into_iter().collect();
        let checks = self.kit.doctor.checks();
        self.kit.host.doctor(&checks);
    }

    /// The tools as they're found now; set again, and checked again,
    /// where that changed.
    fn find_again(&mut self) {
        let tools = Tools::find(self.kit.host.as_mut());
        if tools != self.tools {
            self.take(tools);
            self.kit.doctor.run(self.kit.host.as_mut());
        }
    }

    /// What the doctor found since: a tool a fix installed is used, and
    /// buf's version says which server and formatter.
    fn follow(&mut self) {
        let mut tools = self.tools.clone();
        for program in &self.kit.doctor.programs {
            if let Some(slot) = tools.slot(&program.name) {
                *slot = self.kit.doctor.found.get(&program.name).cloned();
            }
        }
        self.take(tools);
        let said = self.kit.doctor.checks().into_iter().find(|c| c.label == "buf").map(|c| find_version(&c.detail));
        if let Some(version) = said {
            if version != self.buf_version {
                self.buf_version = version;
                self.apply();
            }
        }
    }

    fn take(&mut self, tools: Tools) {
        if tools != self.tools {
            let buf_changed = tools.buf != self.tools.buf;
            self.tools = tools;
            self.configure();
            self.set_tasks();
            self.apply();
            if buf_changed {
                self.check_breaking();
            }
            self.refresh_client();
        }
    }

    /// The project's files changed: read again, and checked again.
    fn reload(&mut self) {
        self.project = Project::read(&self.root);
        self.client.protoset = false;
        self.configure();
        self.set_tasks();
        self.kit.doctor.run(self.kit.host.as_mut());
        self.apply();
        self.check_breaking();
    }

    // The client.

    fn grpcurl(&self) -> Option<Grpcurl> {
        let program = self.tools.grpcurl.clone()?;
        let source = if !self.client.protos {
            Source::Reflection
        } else if self.project.buf_dir.is_some() && self.tools.buf.is_some() {
            Source::Protoset(PROTOSET.into())
        } else {
            Source::Protos(self.project.protos_for_grpcurl())
        };
        Some(Grpcurl { program, target: self.settings.targets.get(self.client.target).cloned(), source, insecure: self.settings.insecure, connect_timeout: CONNECT_SECS, max_time: self.settings.timeout })
    }

    fn refresh_client(&mut self) {
        if let Some(page) = self.client.page {
            self.kit.host.refresh(page);
        }
    }

    fn open_client(&mut self) {
        match self.client.page {
            Some(page) => self.editor.focus_page(page),
            None => {
                self.client.history = History::load(&self.history_path);
                let page = self.kit.host.open_page(CLIENT, "gRPC");
                self.client.page = Some(page);
                self.list();
            }
        }
    }

    /// The services listed again: the descriptor set built first, when
    /// the project's protos are buf's.
    fn list(&mut self) {
        let host = self.kit.host.as_mut();
        self.client.asks.kill_all(host);
        self.client.pending.clear();
        self.client.described = Described::default();
        self.client.names.clear();
        self.client.error = None;
        self.client.loading = None;
        let Some(g) = self.grpcurl() else {
            self.client.error = Some("grpcurl isn't installed: I installs it, or the doctor's fix row (SPC p d)".into());
            self.refresh_client();
            return;
        };
        let started = match &g.source {
            Source::Reflection if g.target.is_none() => {
                self.client.error = Some("No server to ask: a adds one (host:port), or s uses the project's protos".into());
                None
            }
            Source::Protos(files) if files.is_empty() => {
                self.client.error = Some("The project has no .proto files: s asks the server's reflection".into());
                None
            }
            Source::Protoset(path) if !self.client.protoset => {
                let buf = self.tools.buf.clone().unwrap_or_else(|| "buf".into());
                self.client.loading = Some("building the project's protos (buf build)…".into());
                Some((grpc::build_protoset(&buf, &self.project.cwd(), path), Ask::Protoset))
            }
            _ => {
                self.client.loading = Some(format!("listing the services ({})…", g.source.word()));
                Some((g.list(), Ask::List))
            }
        };
        if let Some((cmd, ask)) = started {
            if let Err(why) = self.client.asks.start(self.kit.host.as_mut(), &cmd, ask) {
                self.client.loading = None;
                self.client.error = Some(why);
            }
        }
        self.refresh_client();
    }

    fn ask(&mut self, cmd: &Command, ask: Ask) {
        if let Err(why) = self.client.asks.start(self.kit.host.as_mut(), cmd, ask) {
            self.client.error = Some(why);
        }
    }

    /// What one of the client's grpcurls said.
    fn answered(&mut self, ask: Ask, out: fenix_lang_kit::Output) {
        match ask {
            Ask::Protoset => {
                if out.ok() {
                    self.client.protoset = true;
                    self.list();
                    return;
                }
                self.client.loading = None;
                self.client.error = Some(format!("buf build: {}", out.why()));
            }
            Ask::List => {
                let names = parse_list(&out.stdout);
                if !out.ok() || names.is_empty() {
                    self.client.loading = None;
                    let why = if out.ok() { "the server has no services".to_string() } else { out.why() };
                    let hint = if self.client.protos { "" } else { " (s uses the project's protos)" };
                    self.client.error = Some(format!("{why}{hint}"));
                } else if let Some(g) = self.grpcurl() {
                    self.client.loading = Some(format!("describing {} service{}…", names.len(), if names.len() == 1 { "" } else { "s" }));
                    for n in &names {
                        self.ask(&g.describe(n), Ask::Service(n.clone()));
                    }
                    self.client.names = names;
                }
            }
            Ask::Service(name) => {
                if out.ok() {
                    self.client.described.merge(parse_describe(&out.stdout));
                } else {
                    self.kit.host.log(&format!("describe {name}: {}", out.why()));
                }
                if !self.client.asks.jobs().any(|a| matches!(a, Ask::Service(_))) {
                    self.client.loading = None;
                    let services = self.client.services();
                    self.client.ui.open_new(&services);
                    if let Some(path) = self.client.path() {
                        self.client.ui.select(&services, &path);
                        self.ensure_body();
                    }
                }
            }
            Ask::Type(name) => {
                self.client.pending.remove(&name);
                let mut d = parse_describe(&out.stdout);
                if !out.ok() || (d.messages.is_empty() && d.enums.is_empty()) {
                    d.missing.push(name);
                }
                self.client.described.merge(d);
                self.ensure_body();
            }
        }
        self.refresh_client();
    }

    /// The selected method's input message, described far enough to make
    /// its body: what's missing asked for, else the body made and shown.
    fn ensure_body(&mut self) {
        let Some(path) = self.client.path() else { return };
        if !self.client.bodies.contains_key(&path) {
            let Some(input) = self.method().map(|(_, m)| m.input.clone()) else { return };
            let wanted: Vec<String> = grpc::skeleton::wanted(&self.client.described, &input).into_iter().filter(|w| !self.client.pending.contains(w)).collect();
            if let Some(g) = self.grpcurl().filter(|_| !wanted.is_empty()) {
                for w in wanted {
                    self.client.pending.insert(w.clone());
                    self.ask(&g.describe(&w), Ask::Type(w));
                }
                return;
            }
            if !self.client.pending.is_empty() {
                return;
            }
            let body = grpc::skeleton::skeleton(&self.client.described, &input);
            self.client.bodies.insert(path.clone(), body);
        }
        self.show_body();
    }

    /// The selected method's body in the scratch buffer: opened beside when
    /// it isn't there, else its text replaced when it was another
    /// method's (that one's kept first).
    fn show_body(&mut self) {
        let Some(path) = self.client.path() else { return };
        let Some(body) = self.client.bodies.get(&path).cloned() else { return };
        match self.client.scratch.clone() {
            // This method's already; made again (`B`), its text replaced.
            Some((id, for_path)) if for_path == path => {
                if std::mem::take(&mut self.client.reset) {
                    self.editor.scratch_set(id, &body);
                }
            }
            Some((id, for_path)) => {
                if let Some(text) = self.editor.scratch_text(id) {
                    self.client.bodies.insert(for_path, text);
                }
                self.editor.scratch_set(id, &body);
                self.client.scratch = Some((id, path));
            }
            None => {
                let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                let id = self.editor.scratch_open(&format!("gRPC: {name}"), &body);
                self.client.scratch = Some((id, path));
            }
        }
    }

    /// The body as it'll go: the scratch buffer's, when it's this
    /// method's.
    fn body(&mut self, path: &str) -> Option<String> {
        if let Some((id, for_path)) = self.client.scratch.clone() {
            if for_path == path {
                if let Some(text) = self.editor.scratch_text(id) {
                    self.client.bodies.insert(path.to_string(), text);
                }
            }
        }
        self.client.bodies.get(path).cloned()
    }

    /// The selected method, and its service's name.
    fn method(&self) -> Option<(String, grpc::describe::Method)> {
        let (s, m) = self.client.selected.as_ref()?;
        let svc = self.client.described.service(s)?;
        Some((s.clone(), svc.methods.iter().find(|x| &x.name == m)?.clone()))
    }

    fn select(&mut self, service: &str, method: &str) {
        self.client.selected = Some((service.to_string(), method.to_string()));
        let services = self.client.services();
        self.client.ui.select(&services, &format!("{service}/{method}"));
        self.ensure_body();
        self.refresh_client();
    }

    /// The selected method called with its body and metadata.
    fn invoke(&mut self) -> Result<(), String> {
        let path = self.client.path().ok_or("choose a method first: SPC m g opens the client")?;
        let (_, method) = self.method().ok_or("that method isn't described yet")?;
        let g = self.grpcurl().ok_or("grpcurl isn't installed: the doctor's fix row installs it")?;
        let target = g.target.clone().ok_or("no server to call: T picks one, or adds one")?;
        let body = self.body(&path).ok_or("the body isn't made yet: wait for its message to be described")?;
        let n = messages_in(&body)?;
        if n > 1 && matches!(method.streaming, grpc::describe::Streaming::Unary | grpc::describe::Streaming::Server) {
            return Err(format!("{} takes one message; the body has {n}", method.name));
        }
        self.cancel();
        let metadata = self.client.metadata.get(&path).cloned().unwrap_or_default();
        let cmd = g.invoke(&path, &metadata, &body);
        let host = self.kit.host.as_mut();
        let process = host.spawn(&cmd)?;
        host.close_stdin(process);
        let started = self.editor.now_ms();
        self.client.call = Some(Call { process, method: path.clone(), started, reader: Reader::new(), body, metadata, target: target.address });
        self.client.replies.insert(path, Reply::default());
        self.client.ui.scroll.scroll_to(0, 0);
        if self.client.page.is_none() {
            self.open_client();
        }
        self.refresh_client();
        Ok(())
    }

    /// The call under way stopped: cancelled, as gRPC says it.
    fn cancel(&mut self) {
        let Some(call) = self.client.call.take() else { return };
        self.kit.host.kill(call.process);
        let at = self.editor.now_ms().saturating_sub(call.started);
        if let Some(r) = self.client.replies.get_mut(&call.method) {
            r.outcome = Ended::Status { code: "Canceled".into(), message: "cancelled here".into(), details: Vec::new() };
            r.ended_at = Some(at);
        }
        self.refresh_client();
    }

    /// A line or the end of the call under way.
    fn call_event(&mut self, event: &Event) -> bool {
        let Some(call) = self.client.call.as_mut() else { return false };
        let now = self.editor.now_ms();
        let at = now.saturating_sub(call.started);
        match event {
            Event::ProcessOutput(o) if o.process == call.process => {
                let reply = self.client.replies.entry(call.method.clone()).or_default();
                if call.reader.line(reply, &o.line, o.stderr, at) {
                    if let Some(page) = self.client.page {
                        self.kit.host.refresh(page);
                    }
                }
                true
            }
            Event::ProcessExit((id, code)) if *id == call.process => {
                let mut call = self.client.call.take().expect("there");
                let reply = self.client.replies.entry(call.method.clone()).or_default();
                call.reader.end(reply, *code, at);
                let message = match &reply.outcome {
                    Ended::Status { message, .. } => message.clone(),
                    Ended::Failed(why) => why.clone(),
                    Ended::Running => String::new(),
                };
                let entry = Entry {
                    at: (now / 1000) as i64,
                    target: call.target,
                    body: call.body,
                    metadata: call.metadata,
                    code: reply.code().to_string(),
                    message,
                    ms: at,
                    messages: reply.messages.len(),
                    first: reply.messages.first().map(|m| m.json.clone()).unwrap_or_default(),
                };
                self.client.history.push(&call.method, entry, self.settings.history);
                if let Err(why) = self.client.history.save(&self.history_path) {
                    self.kit.host.log(&format!("history: {why}"));
                }
                self.refresh_client();
                true
            }
            _ => false,
        }
    }

    fn pick_target(&mut self) {
        let mut items: Vec<PickerItem> = self.settings.targets.iter().enumerate().map(|(i, t)| PickerItem::new(t.label(), i.to_string())).collect();
        items.push(PickerItem::new("Add a server…", "add").detail("host:port, tls:// before one that's TLS"));
        let id = self.kit.host.pick("gRPC server", &items);
        self.client.asked.insert(id, Asked::PickTarget);
    }

    fn add_target(&mut self) {
        let id = self.kit.host.prompt("gRPC server (host:port; tls:// for TLS)", "localhost:50051");
        self.client.asked.insert(id, Asked::AddTarget);
    }

    /// The targets saved in `protobuf.targets`.
    fn save_targets(&mut self) {
        if let Err(why) = self.editor.set_setting("protobuf.targets", &grpc::targets_setting(&self.settings.targets)) {
            self.kit.host.notify(&format!("couldn't keep the servers: {why}"));
        }
    }

    /// A prompt's or a picker's answer, for what it was.
    fn answer(&mut self, asked: Asked, value: Option<String>) {
        let Some(value) = value else { return };
        match asked {
            Asked::PickTarget if value == "add" => self.add_target(),
            Asked::PickTarget => {
                if let Ok(i) = value.parse::<usize>() {
                    self.client.target = i.min(self.settings.targets.len().saturating_sub(1));
                    if !self.client.protos {
                        self.list();
                    }
                    self.refresh_client();
                }
            }
            Asked::AddTarget => {
                let Some(t) = grpc::parse_targets(&value).into_iter().next() else {
                    self.kit.host.notify(&format!("{value} isn't host:port"));
                    return;
                };
                let at = match self.settings.targets.iter().position(|x| x.address == t.address) {
                    Some(i) => {
                        self.settings.targets[i] = t;
                        i
                    }
                    None => {
                        self.settings.targets.push(t);
                        self.settings.targets.len() - 1
                    }
                };
                self.client.target = at;
                self.save_targets();
                if !self.client.protos {
                    self.list();
                }
                self.refresh_client();
            }
            Asked::Metadata(path, row) => {
                let list = self.client.metadata.entry(path).or_default();
                match (grpc::parse_metadata(&value), row) {
                    (Some(kv), Some(i)) if i < list.len() => list[i] = kv,
                    (Some(kv), _) => list.push(kv),
                    (None, Some(i)) if value.trim().is_empty() && i < list.len() => {
                        list.remove(i);
                    }
                    (None, _) => self.kit.host.notify("metadata is name: value"),
                }
                self.refresh_client();
            }
        }
    }

    /// What the selected method's last answer said, as text.
    fn response_text(&self) -> Option<String> {
        let r = self.client.replies.get(&self.client.path()?)?;
        Some(r.messages.iter().map(|m| m.json.as_str()).collect::<Vec<_>>().join("\n"))
    }

    fn action(&mut self, action: Action) -> Result<(), String> {
        let path = self.client.path();
        match action {
            Action::Nothing => return Ok(()),
            Action::Redraw => {}
            Action::Select(s, m) => self.select(&s, &m),
            Action::Invoke => self.invoke()?,
            Action::Cancel => self.cancel(),
            Action::PickTarget => self.pick_target(),
            Action::AddTarget => self.add_target(),
            Action::ToggleTls => {
                let i = self.client.target;
                let Some(t) = self.settings.targets.get_mut(i) else { return Err("no server: T adds one".into()) };
                t.tls = !t.tls;
                self.save_targets();
                if !self.client.protos {
                    self.list();
                }
            }
            Action::ToggleSource => {
                self.client.protos = !self.client.protos;
                self.list();
            }
            Action::Reload => {
                self.client.protoset = false;
                self.list();
            }
            Action::EditBody => {
                if path.is_none() {
                    return Err("choose a method first".into());
                }
                self.show_body();
            }
            Action::ResetBody => {
                let path = path.ok_or("choose a method first")?;
                self.client.bodies.remove(&path);
                self.client.reset = true;
                self.ensure_body();
            }
            Action::AddMetadata | Action::EditMetadata(_) => {
                let path = path.ok_or("choose a method first")?;
                let row = if let Action::EditMetadata(i) = action { Some(i) } else { None };
                let initial = row.and_then(|i| self.client.metadata.get(&path)?.get(i).map(|(k, v)| format!("{k}: {v}"))).unwrap_or_default();
                let id = self.kit.host.prompt("Metadata (name: value)", &initial);
                self.client.asked.insert(id, Asked::Metadata(path, row));
            }
            Action::DeleteMetadata(i) => {
                if let Some(list) = path.and_then(|p| self.client.metadata.get_mut(&p)) {
                    if i < list.len() {
                        list.remove(i);
                    }
                }
            }
            Action::FromHistory(i) => {
                let path = path.ok_or("choose a method first")?;
                let e = self.client.history.of(&path).get(i).cloned().ok_or("no such call")?;
                self.client.bodies.insert(path.clone(), e.body.clone());
                self.client.metadata.insert(path.clone(), e.metadata.clone());
                if let Some((id, p)) = self.client.scratch.clone() {
                    if p == path {
                        self.editor.scratch_set(id, &e.body);
                    }
                }
                self.show_body();
                let outcome = if e.code == "ERROR" { Ended::Failed(e.message.clone()) } else { Ended::Status { code: e.code.clone(), message: e.message.clone(), details: Vec::new() } };
                let messages = if e.first.is_empty() { Vec::new() } else { vec![Received { at: e.ms, json: e.first.clone() }] };
                self.client.replies.insert(path, Reply { messages, outcome, ended_at: Some(e.ms), ..Reply::default() });
            }
            Action::CopyResponse => {
                let text = self.response_text().ok_or("no answer to copy")?;
                self.editor.copy(&text);
                self.kit.host.notify("the response's messages copied");
            }
            Action::OpenResponse => {
                let text = self.response_text().ok_or("no answer to open")?;
                let name = path.unwrap_or_default();
                self.editor.open_text(&format!("gRPC response: {name}"), &text);
            }
            Action::InstallGrpcurl => return self.run_command("protobuf.install.grpcurl"),
        }
        self.refresh_client();
        Ok(())
    }

    /// The page drawn from the client's state.
    pub fn client_page(&mut self, width: usize, height: usize) -> fenix_ui_kit::Canvas {
        let services = self.client.services();
        let path = self.client.path();
        let method = self.method();
        let body = path.as_ref().and_then(|p| self.client.bodies.get(p).cloned()).unwrap_or_default();
        let none = Vec::new();
        let metadata = path.as_ref().and_then(|p| self.client.metadata.get(p)).unwrap_or(&none);
        let target = self.settings.targets.get(self.client.target);
        let source = if self.client.protos { "the project's protos" } else { "reflection" };
        let d = Data {
            target,
            targets: self.settings.targets.len(),
            source,
            services: &services,
            loading: self.client.loading.as_deref(),
            error: self.client.error.as_deref(),
            method: method.as_ref().map(|(s, m)| (s.as_str(), m)),
            metadata,
            body: &body,
            reply: path.as_ref().and_then(|p| self.client.replies.get(p)),
            history: path.as_ref().map(|p| self.client.history.of(p)).unwrap_or(&[]),
            grpcurl: self.tools.grpcurl.is_some(),
        };
        self.client.ui.render(&d, width, height)
    }

    fn client_key(&mut self, key: fenix_ui_kit::Key) {
        let services = self.client.services();
        let path = self.client.path();
        let method = self.method();
        let body = String::new();
        let none = Vec::new();
        let metadata = path.as_ref().and_then(|p| self.client.metadata.get(p)).cloned().unwrap_or(none);
        let history: Vec<Entry> = path.as_ref().map(|p| self.client.history.of(p).to_vec()).unwrap_or_default();
        let target = self.settings.targets.get(self.client.target).cloned();
        let action = {
            let d = Data {
                target: target.as_ref(),
                targets: self.settings.targets.len(),
                source: "",
                services: &services,
                loading: None,
                error: None,
                method: method.as_ref().map(|(s, m)| (s.as_str(), m)),
                metadata: &metadata,
                body: &body,
                reply: None,
                history: &history,
                grpcurl: self.tools.grpcurl.is_some(),
            };
            self.client.ui.key(key, &d)
        };
        if let Err(why) = self.action(action) {
            self.kit.host.notify(&why);
        }
    }
}

impl Extension for Protobuf {
    fn activate(&mut self) -> Result<(), String> {
        self.kit.activate()?;
        self.apply();
        self.check_breaking();
        self.follow();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        let done = match self.kit.run_command(id) {
            Some(done) => done,
            None => match id {
                "protobuf.client" => {
                    self.open_client();
                    Ok(())
                }
                "protobuf.invoke" => self.invoke(),
                "protobuf.cancel" => {
                    self.cancel();
                    Ok(())
                }
                "protobuf.target" => {
                    self.pick_target();
                    Ok(())
                }
                "protobuf.breaking" => {
                    let branch = self.against().ok_or("the project isn't a Git repository: there's nothing to compare with")?;
                    if self.project.buf_dir.is_none() {
                        return Err("there's no buf.yaml in the project".into());
                    }
                    let name = format!("breaking against {branch}");
                    let task = self.kit.tasks.tasks.iter().find(|t| t.name == name).map(|t| t.cmd.clone()).ok_or("buf isn't set up for the project yet")?;
                    self.check_breaking();
                    self.kit.host.run_task(&name, &task).map(|_| ())
                }
                _ => Err(format!("no command {id} here")),
            },
        };
        self.follow();
        done
    }

    fn handle(&mut self, event: Event) {
        if self.client.asks.owns(&event) {
            if let Some((ask, out)) = self.client.asks.event(&event) {
                self.answered(ask, out);
            }
            return;
        }
        if self.call_event(&event) {
            return;
        }
        if self.breaking.owns(&event) {
            if let Some((branch, out)) = self.breaking.event(&event) {
                self.found = match lint::violations(&out) {
                    Ok(found) => Breaking::Found { detail: lint::breaking_detail(&found, &branch), count: found.len(), branch },
                    Err(why) => Breaking::Failed(why),
                };
                self.publish_checks();
            }
            return;
        }
        match &event {
            Event::Prompted((id, value)) | Event::Picked((id, value)) => {
                if let Some(asked) = self.client.asked.remove(id) {
                    self.answer(asked, value.clone());
                    return;
                }
            }
            _ => {}
        }
        let name = match &event {
            Event::Saved(name) => Some(name.replace('\\', "/")),
            _ => None,
        };
        let file = name.as_deref().map(|n| n.rsplit('/').next().unwrap_or(n).to_string());
        let project_file = file.as_deref().is_some_and(|f| matches!(f, "buf.yaml" | "buf.work.yaml" | "buf.lock") || project::template_name(f).is_some());
        let proto_saved = file.as_deref().is_some_and(|f| f.ends_with(".proto"));
        let setting = matches!(&event, Event::SettingChanged(key) if key.starts_with("protobuf."));
        let installing = self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        let task_ended = matches!(&event, Event::TaskFinished(_));
        self.kit.handle(&event);
        if project_file {
            self.reload();
        } else if proto_saved {
            self.client.protoset = false;
            self.check_breaking();
        }
        if setting {
            self.settings = (self.read_settings)();
            self.client.target = self.client.target.min(self.settings.targets.len().saturating_sub(1));
            self.configure();
            self.set_tasks();
            self.apply();
            self.refresh_client();
        }
        let installed = installing && !self.kit.installer.installs.iter().any(|i| self.kit.installer.is_running(&i.name));
        if installed || task_ended {
            // What a fix put here, or a task (brew's, go install's) did.
            self.find_again();
        }
        self.follow();
    }

    fn file_saved(&mut self, path: &str) {
        self.kit.file_saved(path);
    }

    fn buffer_changed(&mut self, what: Changed) {
        if let Changed::Scratch(id) = what {
            if let Some((sid, path)) = self.client.scratch.clone() {
                if sid == id {
                    if let Some(text) = self.editor.scratch_text(id) {
                        self.client.bodies.insert(path, text);
                    }
                    self.refresh_client();
                }
            }
        }
    }

    fn scratch_closed(&mut self, scratch: u64) {
        if self.client.scratch.as_ref().is_some_and(|(id, _)| *id == scratch) {
            self.client.scratch = None;
        }
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        if self.client.page == Some(page) {
            return self.client_page(width as usize, height as usize).to_page();
        }
        self.kit.page(page, width, height).unwrap_or_default()
    }

    fn page_key(&mut self, page: u64, key: Key) {
        if self.client.page == Some(page) {
            self.client_key(key.into());
            return;
        }
        self.kit.page_key(page, key);
    }

    fn page_mouse(&mut self, page: u64, event: MouseEvent) {
        if self.client.page == Some(page) {
            let m = fenix_ui_kit::Mouse::from(&event);
            let services = self.client.services();
            let method = self.method();
            let target = self.settings.targets.get(self.client.target).cloned();
            let action = {
                let d = Data { target: target.as_ref(), targets: self.settings.targets.len(), source: "", services: &services, loading: None, error: None, method: method.as_ref().map(|(s, m)| (s.as_str(), m)), metadata: &[], body: "", reply: None, history: &[], grpcurl: true };
                self.client.ui.mouse(&m, &d)
            };
            if let Err(why) = self.action(action) {
                self.kit.host.notify(&why);
            }
            return;
        }
        self.kit.page_mouse(page, &event);
    }

    fn page_state(&mut self, page: u64) -> Option<String> {
        if self.client.page == Some(page) {
            let (side, tab) = self.client.ui.state();
            let target = self.settings.targets.get(self.client.target).map(|t| t.address.clone());
            return Some(serde_json::json!({"method": self.client.path(), "target": target, "protos": self.client.protos, "side": side, "tab": tab}).to_string());
        }
        self.kit.page_state(page)
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if kind == CLIENT {
            let v: serde_json::Value = serde_json::from_str(state).unwrap_or_default();
            self.client.page = Some(page);
            self.client.history = History::load(&self.history_path);
            self.client.protos = v["protos"].as_bool().unwrap_or(self.client.protos);
            if let Some(t) = v["target"].as_str() {
                self.client.target = self.settings.targets.iter().position(|x| x.address == t).unwrap_or(0);
            }
            if let Some((s, m)) = v["method"].as_str().and_then(|p| p.rsplit_once('/')) {
                self.client.selected = Some((s.to_string(), m.to_string()));
            }
            self.client.ui.side.active = v["side"].as_u64().unwrap_or(0).min(1) as usize;
            self.client.ui.tabs.active = v["tab"].as_u64().unwrap_or(0).min(3) as usize;
            self.list();
            return true;
        }
        self.kit.page_restore(page, kind, state)
    }

    fn page_closed(&mut self, page: u64) {
        if self.client.page == Some(page) {
            self.client.page = None;
            self.cancel();
            let host = self.kit.host.as_mut();
            self.client.asks.kill_all(host);
            return;
        }
        self.kit.page_closed(page);
    }
}

export!(Protobuf);

#[cfg(test)]
mod tests;
