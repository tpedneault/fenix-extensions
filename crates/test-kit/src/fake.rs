//! The fake host: what an extension asked of it, written down, and the
//! answers a test gave it, sent back as the events Fenix would send.
//!
//! One [`Fake`] is this thread's at a time (`cargo test` runs each test
//! on a thread of its own): [`api`](crate::api)'s calls go to it, and so
//! do lang-kit's when it's given as the kit's `Host` (feature
//! `lang-kit`). [`Fake::new`] makes a fresh one and makes it this
//! thread's.

use std::cell::{Ref, RefCell, RefMut};
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use fenix_extension_api::chrono::{DateTime, FixedOffset};
use fenix_extension_api::{
    Check, Command, CursorInfo, Decoration, EditorTheme, Event, FolderEntry, GitSummary, Here, HttpRequest, HttpResponse, KeyScheme, LaunchConfig, Level, Notification, PickerItem, PlatformInfo, ProcessOutput, Problem,
    ProjectInfo, Rgb, Task, TaskResult, TextBuffer, TextEdit, Urgency,
};

/// A command as a line: its program, then its arguments, a space between.
pub fn line(cmd: &Command) -> String {
    std::iter::once(cmd.program.as_str()).chain(cmd.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
}

/// Whether `pattern` names the command `line`: the whole line, or its
/// start up to a space -- by the program as it was given, or by its file
/// name (`zig build -l` is `/t/zig build -l`).
fn names(pattern: &str, line: &str) -> bool {
    let starts = |l: &str| l == pattern || l.strip_prefix(pattern).is_some_and(|rest| rest.starts_with(' '));
    if starts(line) {
        return true;
    }
    let (program, rest) = line.split_once(' ').map_or((line, ""), |(p, r)| (p, r));
    let base = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let base = base.strip_suffix(".exe").unwrap_or(base);
    let short = if rest.is_empty() { base.to_string() } else { format!("{base} {rest}") };
    starts(&short)
}

/// Whether `pattern` names the request: `URL` or `METHOD URL`, a `*` at
/// its end any URL that starts so.
fn names_request(pattern: &str, req: &HttpRequest) -> bool {
    let (method, url) = match pattern.split_once(' ') {
        Some((m, u)) if m.chars().all(|c| c.is_ascii_uppercase()) => (Some(m), u),
        _ => (None, pattern),
    };
    if method.is_some_and(|m| !m.eq_ignore_ascii_case(&req.method)) {
        return false;
    }
    match url.strip_suffix('*') {
        Some(prefix) => req.url.starts_with(prefix),
        None => req.url == url,
    }
}

/// How a process the extension started goes, as a test says: what it
/// prints, a line at a time, and how it ends.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ran {
    pub stdout: String,
    pub stderr: String,
    /// Its exit code; `None` once it's said its lines keeps it running.
    pub code: Option<i32>,
    /// What `spawn-raw` gives: its stdout as bytes, not lines.
    pub bytes: Option<Vec<u8>>,
    ends: bool,
}

impl Ran {
    /// Exits 0, having printed `stdout`.
    pub fn ok(stdout: impl Into<String>) -> Ran {
        Ran { stdout: stdout.into(), code: Some(0), ends: true, ..Ran::default() }
    }

    /// Exits `code`, having printed `stderr` on its stderr.
    pub fn fails(code: i32, stderr: impl Into<String>) -> Ran {
        Ran { stderr: stderr.into(), code: Some(code), ends: true, ..Ran::default() }
    }

    /// Exits `code`, having printed nothing (yet: [`Ran::stdout`]).
    pub fn exits(code: i32) -> Ran {
        Ran { code: Some(code), ends: true, ..Ran::default() }
    }

    /// Prints `stdout` and goes on running: a server, a watcher. The test
    /// ends it with [`crate::Harness::exited`] if it likes.
    pub fn running(stdout: impl Into<String>) -> Ran {
        Ran { stdout: stdout.into(), ..Ran::default() }
    }

    /// Ended by a signal: no exit code.
    pub fn killed() -> Ran {
        Ran { ends: true, ..Ran::default() }
    }

    pub fn stdout(mut self, stdout: impl Into<String>) -> Ran {
        self.stdout = stdout.into();
        self
    }

    pub fn stderr(mut self, stderr: impl Into<String>) -> Ran {
        self.stderr = stderr.into();
        self
    }

    /// For `spawn-raw`: these bytes, as `process-bytes`.
    pub fn raw(mut self, bytes: impl Into<Vec<u8>>) -> Ran {
        self.bytes = Some(bytes.into());
        self
    }

    /// The events Fenix sends for process `id`: its stdout's lines, then
    /// its stderr's, then its exit -- or its bytes, for one started raw.
    pub fn events(&self, id: u64, raw: bool) -> Vec<Event> {
        let mut out = Vec::new();
        match (&self.bytes, raw) {
            (Some(bytes), true) => out.push(Event::ProcessBytes((id, bytes.clone()))),
            (None, true) if !self.stdout.is_empty() => out.push(Event::ProcessBytes((id, self.stdout.clone().into_bytes()))),
            _ => out.extend(self.stdout.lines().map(|l| Event::ProcessOutput(ProcessOutput { process: id, stderr: false, line: l.into() }))),
        }
        out.extend(self.stderr.lines().map(|l| Event::ProcessOutput(ProcessOutput { process: id, stderr: true, line: l.into() })));
        if self.ends {
            out.push(Event::ProcessExit((id, self.code)));
        }
        out
    }
}

/// What a picker is answered with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// The row with this label, or else this value.
    Row(String),
    /// Several rows (a picker that marks several), by label or value.
    Rows(Vec<String>),
    /// Closed without a choice (Esc).
    Nothing,
}

type SpawnFn = Box<dyn Fn(&Command) -> Option<Result<Ran, String>>>;
/// A task's name, whether it succeeds, and its tests passed and failed.
type TaskEnd = (String, bool, Option<(u32, u32)>);
type ServiceFn = Box<dyn Fn(&str) -> Result<String, String>>;

/// A page it opened.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenPage {
    pub id: u64,
    pub kind: String,
    pub title: String,
    /// Opened beside the focused pane, this much of it.
    pub beside: Option<f32>,
    /// Still open: not closed by it (`page::close`) or by you
    /// ([`crate::Harness::close`]).
    pub open: bool,
}

/// A process it started.
#[derive(Debug, Clone, PartialEq)]
pub struct Process {
    pub id: u64,
    pub cmd: Command,
    /// Started with `spawn-raw`: its stdout as bytes.
    pub raw: bool,
    /// What it wrote to its stdin.
    pub stdin: Vec<u8>,
    pub stdin_closed: bool,
    pub killed: bool,
}

/// A picker it showed.
#[derive(Debug, Clone, PartialEq)]
pub struct Picker {
    pub id: u64,
    pub title: String,
    pub items: Vec<PickerItem>,
    /// One that marks several.
    pub many: bool,
}

/// A prompt, a secret's prompt or a compose pane it opened.
#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    pub id: u64,
    pub label: String,
    pub initial: String,
    pub secret: bool,
    /// A compose pane (`compose`), not a one-line prompt.
    pub compose: bool,
    /// A compose pane the agent can write (`compose-with-ask`, 1.4): the
    /// `agents.ask` payload `A` sends, and whether it writes over
    /// `initial` too.
    pub ask: Option<(String, bool)>,
}

/// The keys a command has, as a test set them ([`Fake::key`] and the
/// rest): what `key-for` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keys {
    /// Bound under the leader or as a chord: written with `SPC` for the
    /// leader, as Vim keys write it; with standard keys `SPC` is said as
    /// the leader chord ([`Fake::leader_chord`], `C-k`). `standard`, when
    /// given, is what standard keys have instead (`gd`, `F12`).
    Bound { vim: Option<String>, standard: Option<String> },
    /// Bound only in a buffer's `SPC m` menu, as this key there: Fenix's
    /// `key-for` doesn't know those, so it answers none.
    Menu(String),
}

/// A progress it said for a service call (`service-progress`, 1.2).
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub call: u64,
    pub message: String,
    pub fraction: Option<f32>,
}

/// An urgency as the log's lines say it.
pub fn urgency(u: Urgency) -> &'static str {
    match u {
        Urgency::Low => "low",
        Urgency::Normal => "normal",
        Urgency::Critical => "critical",
    }
}

/// A notification as a line of the log: `notification low #tag: Summary
/// -- its body [Label -> command]`.
pub fn notification_line(n: &Notification) -> String {
    let mut out = format!("notification {}", urgency(n.urgency));
    if let Some(tag) = &n.tag {
        out.push_str(&format!(" #{tag}"));
    }
    out.push_str(&format!(": {}", n.summary));
    if !n.body.is_empty() {
        out.push_str(&format!(" -- {}", n.body));
    }
    for a in &n.actions {
        out.push_str(&format!(" [{} -> {}]", a.label, a.command));
    }
    if let Some(p) = n.progress {
        out.push_str(&format!(" ({:.0}%)", p * 100.0));
    }
    out
}

/// A timer it set, still set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timer {
    pub id: u64,
    pub millis: u32,
    pub repeat: bool,
}

/// A service of another extension's it called.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceCall {
    pub id: u64,
    pub name: String,
    pub payload: String,
}

/// Everything the fake knows: what a test set up, and what the extension
/// did. A test reads it through [`Fake`]'s methods, or whole with
/// [`Fake::state`].
pub struct State {
    next: u64,
    calls: Vec<String>,
    taken: usize,
    events: VecDeque<Event>,

    // What it reads, as a test sets it.
    pub settings: BTreeMap<String, String>,
    pub secrets: BTreeMap<String, String>,
    pub env: BTreeMap<String, String>,
    pub programs: BTreeMap<String, String>,
    pub here: Option<Here>,
    pub cursor: Option<CursorInfo>,
    pub explorer: Option<String>,
    /// Buffers' text by path: `buffer::text`, changed by `buffer::edit`.
    pub buffers: BTreeMap<String, String>,
    /// What `read-file` reads, by path.
    pub files: BTreeMap<String, Vec<u8>>,
    pub folders: BTreeMap<String, Vec<FolderEntry>>,
    pub platform: PlatformInfo,
    pub theme: EditorTheme,
    pub palette: Vec<Rgb>,
    pub clipboard: Option<String>,
    pub clipboard_image: Option<Vec<u8>>,
    /// lang-kit's `Host::now`, seconds since 1970.
    pub now: u64,
    /// What `clock::now` says, when a test sets it.
    pub clock: Option<DateTime<FixedOffset>>,
    pub utc_offset: i32,
    /// Every `spawn` fails, as on a machine without the program.
    pub spawn_fails: bool,
    /// How you edit (`editor-keys`, 1.5): Vim keys unless a test says.
    pub key_scheme: KeyScheme,
    /// The leader as standard keys write it: `C-k`.
    pub leader_chord: String,
    /// The keys of each command, by id: what `key-for` answers (1.5).
    pub keys: BTreeMap<String, Keys>,
    /// The open project's repository (`git-state`, 1.1).
    pub git: Option<GitSummary>,
    /// The task that ended last (`last-task`, 1.1).
    pub last_task: Option<TaskResult>,

    // What it did.
    pub pages: BTreeMap<u64, OpenPage>,
    pub status: Option<String>,
    pub notifications: Vec<(Level, String)>,
    /// Its notifications (`notify-with`, 1.3), in order.
    pub toasts: Vec<Notification>,
    /// The tags it dismissed (`notify-dismiss`), in order.
    pub dismissed: Vec<String>,
    /// Its notifications showing: one with a tag in place of the one
    /// before it with that tag, none whose tag it dismissed since.
    pub showing: Vec<Notification>,
    /// What it said of its service calls' progress (`service-progress`).
    pub progress: Vec<Progress>,
    /// The commands it asked the keys of (`key-for`), in order.
    pub keys_asked: Vec<String>,
    pub logs: Vec<(Level, String)>,
    pub processes: BTreeMap<u64, Process>,
    pub tasks_run: BTreeMap<u64, (String, Command)>,
    pub http: BTreeMap<u64, HttpRequest>,
    pub timers: BTreeMap<u64, Timer>,
    pub pickers: Vec<Picker>,
    pub prompts: Vec<Prompt>,
    pub services: Vec<ServiceCall>,
    /// Its answers to calls of its own services made later (`answer`).
    pub answers: BTreeMap<u64, Result<String, String>>,
    pub settings_read: Vec<String>,
    pub diagnostics: BTreeMap<String, Vec<Problem>>,
    /// Decorations by buffer: a file's path, or `scratch:<id>`.
    pub decorations: BTreeMap<String, Vec<Decoration>>,
    pub edits: Vec<(String, Vec<TextEdit>)>,
    pub scratches: BTreeMap<u64, (String, String)>,
    pub checks: Vec<Check>,
    pub tasks: Vec<Task>,
    pub launches: Vec<LaunchConfig>,
    /// What its test providers said they run (`test-provider-set`, API
    /// 1.6, through lang-kit's `Host`): provider, runner, problem, in order.
    pub test_providers: Vec<(String, String, Option<String>)>,
    pub launch_variables: BTreeMap<String, Vec<PickerItem>>,
    pub language_servers: BTreeMap<String, Option<Command>>,
    pub server_settings: BTreeMap<String, String>,
    pub formatters: BTreeMap<String, Option<Command>>,
    pub debug_adapters: BTreeMap<String, Option<Command>>,
    pub copied: Vec<String>,
    pub urls: Vec<String>,

    // Its answers, as a test gives them; the last given wins.
    spawns: Vec<(String, Result<Ran, String>)>,
    spawn_fns: Vec<SpawnFn>,
    responses: Vec<(String, Result<HttpResponse, String>)>,
    downloads: Vec<(String, Vec<u8>)>,
    replies: Vec<(String, ServiceFn)>,
    picks: Vec<(String, Choice)>,
    typed: Vec<(String, Option<String>)>,
    task_ends: Vec<TaskEnd>,
}

impl Default for State {
    fn default() -> State {
        State {
            next: 0,
            calls: Vec::new(),
            taken: 0,
            events: VecDeque::new(),
            settings: BTreeMap::new(),
            secrets: BTreeMap::new(),
            env: BTreeMap::new(),
            programs: BTreeMap::new(),
            here: None,
            cursor: None,
            explorer: None,
            buffers: BTreeMap::new(),
            files: BTreeMap::new(),
            folders: BTreeMap::new(),
            platform: PlatformInfo { os: "linux".into(), arch: "x86_64".into(), env: "gnu".into() },
            theme: EditorTheme {
                name: "Test".into(),
                background: (16, 16, 16),
                text: (224, 224, 224),
                panel: (32, 32, 32),
                accent: (80, 160, 240),
                muted: (128, 128, 128),
                tint: (40, 40, 40),
                warn: (240, 180, 60),
                dark: true,
            },
            palette: vec![(240, 80, 80), (80, 200, 120), (80, 160, 240), (240, 180, 60), (200, 120, 240), (60, 200, 200)],
            clipboard: None,
            clipboard_image: None,
            now: 0,
            clock: None,
            utc_offset: 0,
            spawn_fails: false,
            key_scheme: KeyScheme::Vim,
            leader_chord: "C-k".into(),
            keys: BTreeMap::new(),
            git: None,
            last_task: None,
            pages: BTreeMap::new(),
            status: None,
            notifications: Vec::new(),
            toasts: Vec::new(),
            dismissed: Vec::new(),
            showing: Vec::new(),
            progress: Vec::new(),
            keys_asked: Vec::new(),
            logs: Vec::new(),
            processes: BTreeMap::new(),
            tasks_run: BTreeMap::new(),
            http: BTreeMap::new(),
            timers: BTreeMap::new(),
            pickers: Vec::new(),
            prompts: Vec::new(),
            services: Vec::new(),
            answers: BTreeMap::new(),
            settings_read: Vec::new(),
            diagnostics: BTreeMap::new(),
            decorations: BTreeMap::new(),
            edits: Vec::new(),
            scratches: BTreeMap::new(),
            checks: Vec::new(),
            tasks: Vec::new(),
            launches: Vec::new(),
            test_providers: Vec::new(),
            launch_variables: BTreeMap::new(),
            language_servers: BTreeMap::new(),
            server_settings: BTreeMap::new(),
            formatters: BTreeMap::new(),
            debug_adapters: BTreeMap::new(),
            copied: Vec::new(),
            urls: Vec::new(),
            spawns: Vec::new(),
            spawn_fns: Vec::new(),
            responses: Vec::new(),
            downloads: Vec::new(),
            replies: Vec::new(),
            picks: Vec::new(),
            typed: Vec::new(),
            task_ends: Vec::new(),
        }
    }
}

/// A level as the log's lines say it.
pub(crate) fn level(l: Level) -> &'static str {
    match l {
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
    }
}

/// A buffer as the fake keeps it: a file's path, or `scratch:<id>`.
pub(crate) fn buffer_key(b: &TextBuffer) -> String {
    match b {
        TextBuffer::File(path) => path.clone(),
        TextBuffer::Scratch(id) => format!("scratch:{id}"),
    }
}

/// `edits` made to `text`, each from where it was before any: lines and
/// columns from 0, in characters.
pub fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
    let mut starts = vec![0usize];
    let chars: Vec<char> = text.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c == '\n' {
            starts.push(i + 1);
        }
    }
    let at = |(line, col): (u32, u32)| -> usize {
        let Some(&start) = starts.get(line as usize) else { return chars.len() };
        let end = starts.get(line as usize + 1).map_or(chars.len(), |n| n - 1);
        (start + col as usize).min(end)
    };
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| std::cmp::Reverse((e.start, e.end)));
    let mut out = chars.clone();
    for e in sorted {
        let (a, b) = (at(e.start), at(e.end).max(at(e.start)));
        out.splice(a..b, e.text.chars());
    }
    out.into_iter().collect()
}

/// Vim keys' `SPC …` as standard keys' key menu writes it, under
/// `leader`: `C-k …`. Keys without the leader (`gd`) aren't standard
/// keys': none.
fn in_standard(vim: &str, leader: &str) -> Option<String> {
    match vim.strip_prefix("SPC") {
        Some("") => Some(leader.to_string()),
        Some(rest) if rest.starts_with(' ') => Some(format!("{leader}{rest}")),
        _ => None,
    }
}

fn words(languages: &[String]) -> String {
    languages.join(",")
}

impl State {
    /// A new id: processes, pages, timers, pickers and the rest count up
    /// together from 1.
    pub fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    /// A line in the log of calls.
    pub fn call(&mut self, line: impl Into<String>) {
        self.calls.push(line.into());
    }

    /// An event for the extension, as though Fenix sent it: given to it
    /// when the harness next settles.
    pub fn send(&mut self, event: Event) {
        self.events.push_back(event);
    }

    // -- What lang-kit's mock wrote, in its words ------------------------

    pub fn spawn(&mut self, cmd: &Command, raw: bool) -> Result<u64, String> {
        let l = line(cmd);
        let canned = self.spawn_fns.iter().rev().find_map(|f| f(cmd)).or_else(|| self.spawns.iter().rev().find(|(p, _)| names(p, &l)).map(|(_, r)| r.clone()));
        if self.spawn_fails || matches!(canned, Some(Err(_))) {
            let why = match canned {
                Some(Err(why)) => why,
                _ => format!("{} isn't there", cmd.program),
            };
            self.call(format!("spawn failed {l}"));
            return Err(why);
        }
        let id = self.id();
        self.call(if raw { format!("spawn-raw {id} {l}") } else { format!("spawn {id} {l}") });
        self.processes.insert(id, Process { id, cmd: cmd.clone(), raw, stdin: Vec::new(), stdin_closed: false, killed: false });
        if let Some(Ok(ran)) = canned {
            for e in ran.events(id, raw) {
                self.send(e);
            }
        }
        Ok(id)
    }

    pub fn close_stdin(&mut self, process: u64) {
        self.call(format!("close {process}"));
        if let Some(p) = self.processes.get_mut(&process) {
            p.stdin_closed = true;
        }
    }

    pub fn write_stdin(&mut self, process: u64, data: &[u8]) -> Result<(), String> {
        self.call(format!("write {process} {}", String::from_utf8_lossy(data).trim_end()));
        match self.processes.get_mut(&process) {
            Some(p) if !p.stdin_closed => {
                p.stdin.extend_from_slice(data);
                Ok(())
            }
            Some(_) => Err("its stdin is closed".into()),
            None => Err(format!("no process {process}")),
        }
    }

    pub fn kill(&mut self, process: u64) {
        self.call(format!("kill {process}"));
        if let Some(p) = self.processes.get_mut(&process) {
            p.killed = true;
        }
    }

    pub fn run_task(&mut self, name: &str, cmd: &Command, background: bool) -> Result<u64, String> {
        let id = self.id();
        let word = if background { "background" } else { "task" };
        self.call(format!("{word} {id} {name}: {}", line(cmd)));
        self.tasks_run.insert(id, (name.into(), cmd.clone()));
        let ended = self.task_ends.iter().rev().find(|(n, _, _)| n == name).map(|(_, ok, tests)| (*ok, *tests));
        if let Some((ok, tests)) = ended {
            if !background {
                self.last_task = Some(TaskResult { name: name.into(), ok, tests });
            }
            self.send(Event::TaskFinished((id, ok)));
        }
        Ok(id)
    }

    pub fn download(&mut self, req: &HttpRequest, path: &str, sha256: Option<&str>) -> Result<u64, String> {
        let id = self.id();
        self.call(format!("download {id} {} -> {path} {}", req.url, sha256.unwrap_or("-")));
        self.http.insert(id, req.clone());
        let bytes = self.downloads.iter().rev().find(|(u, _)| *u == req.url).map(|(_, b)| b.clone());
        let answer = self.responses.iter().rev().find(|(p, _)| names_request(p, req)).map(|(_, a)| a.clone());
        if let Some(bytes) = bytes {
            if let Some(dir) = std::path::Path::new(path).parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            std::fs::write(path, &bytes).map_err(|e| format!("{path}: {e}"))?;
            self.send(Event::HttpResponse((id, Ok(HttpResponse::new(200, Vec::new())))));
        } else if let Some(answer) = answer {
            self.send(Event::HttpResponse((id, answer)));
        }
        Ok(id)
    }

    pub fn set_executable(&mut self, path: &str) -> Result<(), String> {
        self.call(format!("executable {path}"));
        Ok(())
    }

    pub fn find_program(&mut self, name: &str) -> Option<String> {
        self.programs.get(name).cloned()
    }

    pub fn set_timer(&mut self, millis: u32, repeat: bool) -> u64 {
        let id = self.id();
        self.call(if repeat { format!("timer {id} {millis} repeat") } else { format!("timer {id} {millis}") });
        self.timers.insert(id, Timer { id, millis, repeat });
        id
    }

    pub fn cancel_timer(&mut self, timer: u64) {
        self.call(format!("cancel-timer {timer}"));
        self.timers.remove(&timer);
    }

    fn typed_for(&self, label: &str) -> Option<Option<String>> {
        self.typed.iter().rev().find(|(p, _)| label.contains(p.as_str())).map(|(_, t)| t.clone())
    }

    pub fn prompt(&mut self, label: &str, initial: &str, secret: bool) -> u64 {
        let id = self.id();
        self.call(if secret { format!("prompt-secret {id} {label}") } else { format!("prompt {id} {label} [{initial}]") });
        self.prompts.push(Prompt { id, label: label.into(), initial: initial.into(), secret, compose: false, ask: None });
        if let Some(text) = self.typed_for(label) {
            self.send(Event::Prompted((id, text)));
        }
        id
    }

    pub fn compose(&mut self, title: &str, verb: &str, initial: &str) -> u64 {
        self.compose_asking(title, verb, initial, None)
    }

    /// A compose pane; with `ask`, one the agent can write
    /// (`compose-with-ask`): its payload, and whether it writes over
    /// `initial`. What's in the pane comes back as for any compose pane.
    pub fn compose_asking(&mut self, title: &str, verb: &str, initial: &str, ask: Option<(&str, bool)>) -> u64 {
        let id = self.id();
        self.call(match ask {
            Some((payload, over)) => format!("compose {id} {title} ({verb}) [{initial}] ask{} {payload}", if over { " over" } else { "" }),
            None => format!("compose {id} {title} ({verb}) [{initial}]"),
        });
        self.prompts.push(Prompt { id, label: title.into(), initial: initial.into(), secret: false, compose: true, ask: ask.map(|(p, o)| (p.to_string(), o)) });
        if let Some(text) = self.typed_for(title) {
            self.send(Event::Composed((id, text)));
        }
        id
    }

    pub fn pick(&mut self, title: &str, items: &[PickerItem], many: bool) -> u64 {
        let id = self.id();
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        self.call(format!("{} {id} {title}: {}", if many { "pick-many" } else { "pick" }, labels.join(", ")));
        self.pickers.push(Picker { id, title: title.into(), items: items.to_vec(), many });
        let find = |want: &str| items.iter().find(|i| i.label == want).or_else(|| items.iter().find(|i| i.value == want)).map(|i| i.value.clone());
        let choice = self.picks.iter().rev().find(|(p, _)| title.contains(p.as_str())).map(|(_, c)| c.clone());
        if let Some(choice) = choice {
            let event = match (&choice, many) {
                (Choice::Nothing, false) => Event::Picked((id, None)),
                (Choice::Nothing, true) => Event::PickedMany((id, None)),
                (Choice::Row(r), false) => Event::Picked((id, Some(find(r).unwrap_or_else(|| panic!("the picker \"{title}\" has no row {r:?}: {labels:?}"))))),
                (Choice::Row(r), true) => Event::PickedMany((id, Some(vec![find(r).unwrap_or_else(|| panic!("the picker \"{title}\" has no row {r:?}: {labels:?}"))]))),
                (Choice::Rows(rs), _) => {
                    let values = rs.iter().map(|r| find(r).unwrap_or_else(|| panic!("the picker \"{title}\" has no row {r:?}: {labels:?}"))).collect::<Vec<_>>();
                    if many {
                        Event::PickedMany((id, Some(values)))
                    } else {
                        Event::Picked((id, values.into_iter().next()))
                    }
                }
            };
            self.send(event);
        }
        id
    }

    pub fn notify(&mut self, level: Level, message: &str) {
        self.call(match level {
            Level::Info => format!("notify {message}"),
            l => format!("notify {} {message}", self::level(l)),
        });
        self.notifications.push((level, message.into()));
    }

    /// A notification (`notify-with`): written down, kept, and shown in
    /// place of the one showing with its tag.
    pub fn notify_with(&mut self, n: &Notification) {
        self.call(notification_line(n));
        self.toasts.push(n.clone());
        if n.tag.is_some() {
            self.showing.retain(|o| o.tag != n.tag);
        }
        self.showing.push(n.clone());
    }

    /// Its toast tagged `tag` taken away (`notify-dismiss`).
    pub fn notify_dismiss(&mut self, tag: &str) {
        self.call(format!("dismiss {tag}"));
        self.dismissed.push(tag.into());
        self.showing.retain(|n| n.tag.as_deref() != Some(tag));
    }

    /// What `key-for` answers for `command` in the scheme in use.
    pub fn key_for(&mut self, command: &str) -> Option<String> {
        self.keys_asked.push(command.into());
        let Some(Keys::Bound { vim, standard }) = self.keys.get(command) else { return None };
        match self.key_scheme {
            KeyScheme::Vim => vim.clone(),
            KeyScheme::Standard => standard.clone().or_else(|| vim.as_deref().and_then(|v| in_standard(v, &self.leader_chord))),
        }
    }

    /// How far service call `call` has got (`service-progress`).
    pub fn progress(&mut self, call: u64, message: &str, fraction: Option<f32>) {
        self.call(match fraction {
            Some(f) => format!("progress {call} {message} ({:.0}%)", f * 100.0),
            None => format!("progress {call} {message}"),
        });
        self.progress.push(Progress { call, message: message.into(), fraction });
    }

    pub fn log(&mut self, level: Level, message: &str) {
        self.call(format!("{} {message}", self::level(level)));
        self.logs.push((level, message.into()));
    }

    pub fn open_page(&mut self, kind: &str, title: &str, beside: Option<f32>) -> u64 {
        let id = self.id();
        self.call(match beside {
            Some(w) => format!("page {id} {kind} {title} (beside {w})"),
            None => format!("page {id} {kind} {title}"),
        });
        self.pages.insert(id, OpenPage { id, kind: kind.into(), title: title.into(), beside, open: true });
        id
    }

    pub fn refresh(&mut self, page: u64) {
        self.call(format!("refresh {page}"));
    }

    pub fn open_text(&mut self, title: &str, _text: &str, _language: Option<&str>, beside: Option<f32>) {
        self.call(match beside {
            Some(_) => format!("text {title}"),
            None => format!("text {title} (in a tab)"),
        });
    }

    pub fn doctor(&mut self, checks: &[Check]) {
        self.checks = checks.to_vec();
        self.call(format!("doctor {}", checks.len()));
    }

    pub fn set_tasks(&mut self, tasks: &[Task]) -> Result<(), String> {
        self.tasks = tasks.to_vec();
        self.call(format!("tasks {}", tasks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")));
        Ok(())
    }

    pub fn language_server(&mut self, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        self.call(format!("server {}: {}", words(languages), cmd.map(line).unwrap_or_default()));
        self.language_servers.insert(words(languages), cmd.cloned());
        Ok(())
    }

    pub fn add_language_server(&mut self, name: &str, languages: &[String], cmd: Option<&Command>) -> Result<(), String> {
        self.call(format!("add {name} {}: {}", words(languages), cmd.map(line).unwrap_or_default()));
        self.language_servers.insert(name.into(), cmd.cloned());
        Ok(())
    }

    pub fn server_settings(&mut self, name: &str, settings: Option<&str>) -> Result<(), String> {
        self.call(format!("settings {name}: {}", settings.unwrap_or_default()));
        match settings {
            Some(s) => self.server_settings.insert(name.into(), s.into()),
            None => self.server_settings.remove(name),
        };
        Ok(())
    }

    pub fn debug_adapter(&mut self, languages: &[String], cmd: Option<&Command>, port: Option<u16>) -> Result<(), String> {
        self.call(match port {
            Some(port) => format!("connect {}: {} :{port}", words(languages), cmd.map(line).unwrap_or_default()),
            None => format!("adapter {}: {}", words(languages), cmd.map(line).unwrap_or_default()),
        });
        self.debug_adapters.insert(words(languages), cmd.cloned());
        Ok(())
    }

    pub fn formatter(&mut self, languages: &[String], cmd: Option<&Command>, keep: bool) -> Result<(), String> {
        let kept = if keep { " (kept)" } else { "" };
        self.call(format!("formatter {}: {}{kept}", words(languages), cmd.map(line).unwrap_or_default()));
        self.formatters.insert(words(languages), cmd.cloned());
        Ok(())
    }

    pub fn diagnostics(&mut self, path: &str, problems: &[Problem]) -> Result<(), String> {
        let fixes: usize = problems.iter().map(|p| p.fixes.len()).sum();
        self.call(match fixes {
            0 => format!("problems {path} {}", problems.len()),
            n => format!("problems {path} {} ({n} fixes)", problems.len()),
        });
        self.diagnostics.insert(path.into(), problems.to_vec());
        Ok(())
    }

    pub fn http(&mut self, req: &HttpRequest) -> Result<u64, String> {
        let id = self.id();
        self.call(format!("http {id} {} {}", req.method, req.url));
        self.http.insert(id, req.clone());
        let answer = self.responses.iter().rev().find(|(p, _)| names_request(p, req)).map(|(_, a)| a.clone());
        if let Some(answer) = answer {
            self.send(Event::HttpResponse((id, answer)));
        }
        Ok(id)
    }

    pub fn open_url(&mut self, url: &str) -> Result<(), String> {
        self.call(format!("url {url}"));
        self.urls.push(url.into());
        Ok(())
    }

    pub fn task_cancel(&mut self, task: u64) {
        self.call(format!("cancel {task}"));
    }

    pub fn task_show(&mut self, task: u64) -> Result<(), String> {
        self.call(format!("show {task}"));
        Ok(())
    }

    pub fn set_launches(&mut self, launches: &[LaunchConfig]) -> Result<(), String> {
        self.call(format!("launches {}", launches.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(", ")));
        self.launches = launches.to_vec();
        Ok(())
    }

    pub fn launch_variable(&mut self, name: &str, choices: &[PickerItem]) -> Result<(), String> {
        self.call(format!("variable {name}: {}", choices.iter().map(|c| c.value.as_str()).collect::<Vec<_>>().join(", ")));
        if choices.is_empty() {
            self.launch_variables.remove(name);
        } else {
            self.launch_variables.insert(name.into(), choices.to_vec());
        }
        Ok(())
    }

    pub fn here_path(&self) -> Option<(String, Option<String>)> {
        let here = self.here.as_ref()?;
        Some((here.path.clone()?, here.project.as_ref().map(|p| p.root.clone())))
    }

    // -- The rest of the API ----------------------------------------------

    pub fn setting(&mut self, key: &str) -> Option<String> {
        self.settings_read.push(key.into());
        self.settings.get(key).cloned()
    }

    pub fn set_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        self.call(format!("setting-set {key} {}", value.unwrap_or("-")));
        match value {
            Some(v) => self.settings.insert(key.into(), v.into()),
            None => self.settings.remove(key),
        };
        Ok(())
    }

    pub fn set_secret(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        self.call(format!("secret-set {key} {}", if value.is_some() { "(set)" } else { "-" }));
        match value {
            Some(v) => self.secrets.insert(key.into(), v.into()),
            None => self.secrets.remove(key),
        };
        Ok(())
    }

    pub fn status(&mut self, text: Option<&str>) {
        self.call(format!("status {}", text.unwrap_or("-")));
        self.status = text.map(str::to_string);
    }

    pub fn copy(&mut self, text: &str) {
        self.call(format!("copy {text}"));
        self.copied.push(text.into());
        self.clipboard = Some(text.into());
    }

    pub fn call_service(&mut self, name: &str, payload: &str) -> u64 {
        let id = self.id();
        self.call(format!("service {id} {name} {payload}"));
        self.services.push(ServiceCall { id, name: name.into(), payload: payload.into() });
        let reply = self.replies.iter().rev().find(|(n, _)| n == name).map(|(_, f)| f(payload));
        if let Some(reply) = reply {
            self.send(Event::ServiceReply((id, reply)));
        }
        id
    }

    pub fn answer(&mut self, call: u64, answer: Result<&str, &str>) {
        self.call(match answer {
            Ok(text) => format!("answer {call} {text}"),
            Err(why) => format!("answer {call} failed: {why}"),
        });
        self.answers.insert(call, answer.map(str::to_string).map_err(str::to_string));
    }

    pub fn edit(&mut self, path: &str, edits: &[TextEdit]) -> Result<(), String> {
        self.call(format!("edit {path} {}", edits.len()));
        self.edits.push((path.into(), edits.to_vec()));
        if let Some(text) = self.buffers.get(path) {
            let changed = apply_edits(text, edits);
            self.buffers.insert(path.into(), changed);
        }
        Ok(())
    }

    pub fn decorate(&mut self, buffer: &TextBuffer, decorations: &[Decoration]) -> Result<(), String> {
        let key = buffer_key(buffer);
        self.call(format!("decorations {key} {}", decorations.len()));
        self.decorations.insert(key, decorations.to_vec());
        Ok(())
    }

    pub fn scratch_open(&mut self, title: &str, text: &str, beside: Option<f32>) -> u64 {
        let id = self.id();
        self.call(match beside {
            Some(w) => format!("scratch {id} {title} (beside {w})"),
            None => format!("scratch {id} {title}"),
        });
        self.scratches.insert(id, (title.into(), text.into()));
        id
    }

    /// Something the fake doesn't keep, written down by its name.
    pub fn other(&mut self, what: impl Into<String>) {
        self.call(what);
    }
}

/// The fake host: a handle on this thread's [`State`]. Clones are the
/// same fake.
#[derive(Clone)]
pub struct Fake(Rc<RefCell<State>>);

thread_local! {
    static CURRENT: RefCell<Option<Fake>> = const { RefCell::new(None) };
}

impl Default for Fake {
    fn default() -> Fake {
        Fake::new()
    }
}

impl Fake {
    /// A fresh fake, which is now this thread's: what `api` calls from here
    /// on go to.
    pub fn new() -> Fake {
        let fake = Fake(Rc::new(RefCell::new(State::default())));
        fake.install();
        fake
    }

    /// This one is this thread's again.
    pub fn install(&self) {
        CURRENT.with(|c| *c.borrow_mut() = Some(self.clone()));
    }

    /// This thread's fake: a fresh one if there's none yet, so code that
    /// calls the host can be tested without setting one up.
    pub fn current() -> Fake {
        CURRENT.with(|c| c.borrow_mut().get_or_insert_with(|| Fake(Rc::new(RefCell::new(State::default())))).clone())
    }

    /// `f` on this thread's fake's state.
    pub fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
        let fake = Fake::current();
        let mut state = fake.0.borrow_mut();
        f(&mut state)
    }

    /// Everything it knows, to read.
    pub fn state(&self) -> Ref<'_, State> {
        self.0.borrow()
    }

    /// Everything it knows, to change.
    pub fn state_mut(&self) -> RefMut<'_, State> {
        self.0.borrow_mut()
    }

    // -- Set up: what the extension finds ----------------------------------

    pub fn setting(&self, key: &str, value: &str) -> &Fake {
        self.state_mut().settings.insert(key.into(), value.into());
        self
    }

    pub fn secret(&self, key: &str, value: &str) -> &Fake {
        self.state_mut().secrets.insert(key.into(), value.into());
        self
    }

    pub fn env(&self, name: &str, value: &str) -> &Fake {
        self.state_mut().env.insert(name.into(), value.into());
        self
    }

    /// `find-program` finds `name` at `path`.
    pub fn program(&self, name: &str, path: &str) -> &Fake {
        self.state_mut().programs.insert(name.into(), path.into());
        self
    }

    pub fn platform(&self, os: &str, arch: &str) -> &Fake {
        let mut s = self.state_mut();
        s.platform.os = os.into();
        s.platform.arch = arch.into();
        drop(s);
        self
    }

    pub fn theme(&self, theme: EditorTheme) -> &Fake {
        self.state_mut().theme = theme;
        self
    }

    pub fn palette(&self, palette: &[Rgb]) -> &Fake {
        self.state_mut().palette = palette.to_vec();
        self
    }

    /// You're in the file at `path` (a full path), whose extension is
    /// `language` (`rs`, as Fenix gives it), holding `text`, on its first
    /// line: `here` says so, and its buffer has the text.
    pub fn in_file(&self, path: &str, language: &str, text: &str) -> &Fake {
        let mut s = self.state_mut();
        let project = s.here.as_ref().and_then(|h| h.project.clone());
        s.here = Some(Here { path: Some(path.into()), line: 1, text: text.lines().next().unwrap_or("").into(), language: language.into(), project });
        s.buffers.insert(path.into(), text.into());
        s.cursor = Some(CursorInfo { at: (0, 0), selection: None, selected: false });
        drop(s);
        self
    }

    /// The cursor at `line` and `column` (from 0) of the file you're in:
    /// `here`'s text is that line's (and its `line`, from 1, that one).
    pub fn cursor(&self, line: u32, column: u32) -> &Fake {
        let mut s = self.state_mut();
        let text = s.here.as_ref().and_then(|h| h.path.clone()).and_then(|p| s.buffers.get(&p).cloned());
        if let (Some(here), Some(text)) = (s.here.as_mut(), text) {
            here.line = line + 1;
            here.text = text.lines().nth(line as usize).unwrap_or("").into();
        }
        s.cursor = Some(CursorInfo { at: (line, column), selection: None, selected: false });
        drop(s);
        self
    }

    /// `from` up to `to` selected, each (line, column) from 0: `here`'s
    /// text is what's selected, as Fenix gives it.
    pub fn selection(&self, from: (u32, u32), to: (u32, u32)) -> &Fake {
        let mut s = self.state_mut();
        let text = s.here.as_ref().and_then(|h| h.path.clone()).and_then(|p| s.buffers.get(&p).cloned()).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let mut picked = String::new();
        for l in from.0..=to.0 {
            let line: Vec<char> = lines.get(l as usize).copied().unwrap_or("").chars().collect();
            let a = if l == from.0 { from.1 as usize } else { 0 };
            let b = if l == to.0 { (to.1 as usize).min(line.len()) } else { line.len() };
            picked.extend(line[a.min(b)..b].iter());
            if l != to.0 {
                picked.push('\n');
            }
        }
        if let Some(here) = s.here.as_mut() {
            here.text = picked;
            here.line = from.0 + 1;
        }
        s.cursor = Some(CursorInfo { at: to, selection: Some((from, to)), selected: true });
        drop(s);
        self
    }

    /// The project you're in: its name and folder.
    pub fn project(&self, name: &str, root: &str) -> &Fake {
        let mut s = self.state_mut();
        let info = ProjectInfo { name: name.into(), root: root.into(), meta: Vec::new() };
        match s.here.as_mut() {
            Some(here) => here.project = Some(info),
            None => s.here = Some(Here { path: None, line: 0, text: String::new(), language: String::new(), project: Some(info) }),
        }
        drop(s);
        self
    }

    /// A buffer's text, by path, for `buffer::text`.
    pub fn buffer(&self, path: &str, text: &str) -> &Fake {
        self.state_mut().buffers.insert(path.into(), text.into());
        self
    }

    /// What `read-file` reads at `path`.
    pub fn file(&self, path: &str, bytes: impl Into<Vec<u8>>) -> &Fake {
        self.state_mut().files.insert(path.into(), bytes.into());
        self
    }

    pub fn clipboard(&self, text: &str) -> &Fake {
        self.state_mut().clipboard = Some(text.into());
        self
    }

    /// What `clock::now` says.
    pub fn clock(&self, now: DateTime<FixedOffset>) -> &Fake {
        self.state_mut().clock = Some(now);
        self
    }

    /// lang-kit's `Host::now`, in seconds since 1970.
    pub fn now(&self, secs: u64) -> &Fake {
        self.state_mut().now = secs;
        self
    }

    /// Every `spawn` fails, or none does.
    pub fn spawn_fails(&self, fails: bool) -> &Fake {
        self.state_mut().spawn_fails = fails;
        self
    }

    /// How you edit (`key_scheme`, `editor-keys`): Vim keys unless a test
    /// says. [`crate::Harness::key_scheme`] switches it and tells the
    /// extension, as Fenix does.
    pub fn key_scheme(&self, scheme: KeyScheme) -> &Fake {
        self.state_mut().key_scheme = scheme;
        self
    }

    /// The leader as standard keys write it (`keys.leader_chord`): `C-k`
    /// unless a test says.
    pub fn leader_chord(&self, chord: &str) -> &Fake {
        self.state_mut().leader_chord = chord.into();
        self
    }

    /// `command` runs on `keys`, written as Vim keys write them, `SPC`
    /// the leader (`SPC h g`): `key_for` answers that with Vim keys, and
    /// with standard keys the same under the leader chord (`C-k h g`). A
    /// key that isn't under the leader (`gd`) standard keys don't have
    /// unless [`Fake::key_in`] says.
    pub fn key(&self, command: &str, keys: &str) -> &Fake {
        self.state_mut().keys.insert(command.into(), Keys::Bound { vim: Some(keys.into()), standard: None });
        self
    }

    /// `command` runs on `keys` in `scheme` alone (`F12` with standard
    /// keys), over what [`Fake::key`] said for it.
    pub fn key_in(&self, scheme: KeyScheme, command: &str, keys: &str) -> &Fake {
        let mut s = self.state_mut();
        let entry = s.keys.entry(command.into()).or_insert(Keys::Bound { vim: None, standard: None });
        if let Keys::Menu(_) = entry {
            *entry = Keys::Bound { vim: None, standard: None };
        }
        if let Keys::Bound { vim, standard } = entry {
            match scheme {
                KeyScheme::Vim => *vim = Some(keys.into()),
                KeyScheme::Standard => *standard = Some(keys.into()),
            }
        }
        drop(s);
        self
    }

    /// No key runs `command`: `key_for` answers none (as for any command
    /// a test said nothing of), with either scheme.
    pub fn no_key(&self, command: &str) -> &Fake {
        self.state_mut().keys.remove(command);
        self
    }

    /// `command` is bound only in a buffer's `SPC m` menu, as `key`
    /// there: `key_for` answers none for it, as Fenix's does, so the
    /// extension has to write the menu's keys itself.
    pub fn menu_key(&self, command: &str, key: &str) -> &Fake {
        self.state_mut().keys.insert(command.into(), Keys::Menu(key.into()));
        self
    }

    /// The open project's repository, as `git_state` gives it; none
    /// outside one (the default).
    pub fn git(&self, git: Option<GitSummary>) -> &Fake {
        self.state_mut().git = git;
        self
    }

    /// The task that ended last, as `last_task` gives it. A task pane's
    /// task answered by [`Fake::on_task`] sets it too, as it ends.
    pub fn last_task(&self, task: Option<TaskResult>) -> &Fake {
        self.state_mut().last_task = task;
        self
    }

    // -- Its answers ---------------------------------------------------------

    /// A process `pattern` names -- its whole command line, or its start
    /// up to a space, by its program's path or its name (`zig build -l`)
    /// -- runs as `ran` says: its lines and its exit come as events.
    pub fn on_spawn(&self, pattern: &str, ran: Ran) -> &Fake {
        self.state_mut().spawns.push((pattern.into(), Ok(ran)));
        self
    }

    /// A process `pattern` names can't start: `spawn` says `why`.
    pub fn on_spawn_error(&self, pattern: &str, why: &str) -> &Fake {
        self.state_mut().spawns.push((pattern.into(), Err(why.into())));
        self
    }

    /// Each process started is given to `answer`; what it says, if
    /// anything, is how it runs (or why it couldn't start).
    pub fn on_spawn_with(&self, answer: impl Fn(&Command) -> Option<Result<Ran, String>> + 'static) -> &Fake {
        self.state_mut().spawn_fns.push(Box::new(answer));
        self
    }

    /// A request `pattern` names (`URL`, or `METHOD URL`; a `*` at its end
    /// any URL starting so) is answered with `response`.
    pub fn on_http(&self, pattern: &str, response: HttpResponse) -> &Fake {
        self.state_mut().responses.push((pattern.into(), Ok(response)));
        self
    }

    /// A request `pattern` names fails: no answer, only `why`.
    pub fn on_http_error(&self, pattern: &str, why: &str) -> &Fake {
        self.state_mut().responses.push((pattern.into(), Err(why.into())));
        self
    }

    /// A download of `url` writes `bytes` to the path it was asked for,
    /// then answers as Fenix does when it's checked and kept.
    pub fn on_download(&self, url: &str, bytes: impl Into<Vec<u8>>) -> &Fake {
        self.state_mut().downloads.push((url.into(), bytes.into()));
        self
    }

    /// Another extension's service `name` answers `reply`.
    pub fn on_service(&self, name: &str, reply: Result<&str, &str>) -> &Fake {
        let reply = reply.map(str::to_string).map_err(str::to_string);
        self.state_mut().replies.push((name.into(), Box::new(move |_| reply.clone())));
        self
    }

    /// Service `name` answers what `reply` makes of its payload.
    pub fn on_service_with(&self, name: &str, reply: impl Fn(&str) -> Result<String, String> + 'static) -> &Fake {
        self.state_mut().replies.push((name.into(), Box::new(reply)));
        self
    }

    /// A picker whose title has `title` in it is answered with the row
    /// labelled `row` (or, failing that, whose value is `row`).
    pub fn on_pick(&self, title: &str, row: &str) -> &Fake {
        self.state_mut().picks.push((title.into(), Choice::Row(row.into())));
        self
    }

    /// A picker that marks several, answered with these rows.
    pub fn on_pick_many(&self, title: &str, rows: &[&str]) -> &Fake {
        self.state_mut().picks.push((title.into(), Choice::Rows(rows.iter().map(|r| r.to_string()).collect())));
        self
    }

    /// A picker whose title has `title` in it is closed with nothing.
    pub fn on_pick_nothing(&self, title: &str) -> &Fake {
        self.state_mut().picks.push((title.into(), Choice::Nothing));
        self
    }

    /// A prompt, secret's prompt or compose pane whose label has `label`
    /// in it is answered with `text`.
    pub fn on_prompt(&self, label: &str, text: &str) -> &Fake {
        self.state_mut().typed.push((label.into(), Some(text.into())));
        self
    }

    /// The same, closed with nothing (Esc).
    pub fn on_prompt_nothing(&self, label: &str) -> &Fake {
        self.state_mut().typed.push((label.into(), None));
        self
    }

    /// A task called `name` ends, having succeeded or not.
    pub fn on_task(&self, name: &str, ok: bool) -> &Fake {
        self.state_mut().task_ends.push((name.into(), ok, None));
        self
    }

    /// A task called `name` ends with its tests counted, `passed` and
    /// `failed` (`last_task`'s `tests`): it succeeded when none failed.
    pub fn on_task_tests(&self, name: &str, passed: u32, failed: u32) -> &Fake {
        self.state_mut().task_ends.push((name.into(), failed == 0, Some((passed, failed))));
        self
    }

    // -- What it did ---------------------------------------------------------

    /// The calls it made since last asked, each as a line: `spawn 3 zig
    /// version`, `notify Copied`, `page 1 regex *regex* (beside 0.5)`.
    pub fn take(&self) -> Vec<String> {
        let mut s = self.state_mut();
        let from = s.taken;
        s.taken = s.calls.len();
        s.calls[from..].to_vec()
    }

    /// Every call it made, each as a line.
    pub fn calls(&self) -> Vec<String> {
        self.state().calls.clone()
    }

    /// Events waiting to be given to the extension.
    pub fn pending(&self) -> Vec<Event> {
        self.state().events.iter().cloned().collect()
    }

    /// The next event waiting, taken.
    pub fn next_event(&self) -> Option<Event> {
        self.state_mut().events.pop_front()
    }

    /// An event for the extension, as though Fenix sent it.
    pub fn send(&self, event: Event) {
        self.state_mut().send(event);
    }

    /// What it notified, in order (any level).
    pub fn notifications(&self) -> Vec<String> {
        self.state().notifications.iter().map(|(_, m)| m.clone()).collect()
    }

    /// Its notifications (`notify-with`), every one, in order: what each
    /// said, how urgent, its tag and actions.
    pub fn toasts(&self) -> Vec<Notification> {
        self.state().toasts.clone()
    }

    /// The notifications showing now, oldest first, as Fenix keeps them:
    /// one with a tag in place of the one before it with that tag, and
    /// none whose tag it dismissed since. (A toast's time running out
    /// isn't counted: what's shown is what it last said.)
    pub fn showing(&self) -> Vec<Notification> {
        self.state().showing.clone()
    }

    /// The tags it dismissed (`notify-dismiss`), in order.
    pub fn dismissed(&self) -> Vec<String> {
        self.state().dismissed.clone()
    }

    /// What it said of service call `call`'s progress, in order.
    pub fn progress_of(&self, call: u64) -> Vec<Progress> {
        self.state().progress.iter().filter(|p| p.call == call).cloned().collect()
    }

    /// The commands it asked the keys of (`key_for`), in order.
    pub fn keys_asked(&self) -> Vec<String> {
        self.state().keys_asked.clone()
    }

    /// What it logged, in order, as `level message`.
    pub fn logs(&self) -> Vec<String> {
        self.state().logs.iter().map(|(l, m)| format!("{} {m}", level(*l))).collect()
    }

    /// What it has on the modeline.
    pub fn status(&self) -> Option<String> {
        self.state().status.clone()
    }

    /// Its pages still open, oldest first.
    pub fn open_pages(&self) -> Vec<OpenPage> {
        self.state().pages.values().filter(|p| p.open).cloned().collect()
    }

    /// Its newest open page of `kind`.
    pub fn page(&self, kind: &str) -> Option<u64> {
        self.state().pages.values().rev().find(|p| p.open && p.kind == kind).map(|p| p.id)
    }

    /// The processes it started, by id, as lines.
    pub fn spawned(&self) -> Vec<(u64, String)> {
        self.state().processes.values().map(|p| (p.id, line(&p.cmd))).collect()
    }

    /// The process it started last whose line `pattern` names (as
    /// [`Fake::on_spawn`] matches).
    pub fn process(&self, pattern: &str) -> Option<Process> {
        self.state().processes.values().rev().find(|p| names(pattern, &line(&p.cmd))).cloned()
    }

    /// Its HTTP requests and downloads, by id.
    pub fn requests(&self) -> Vec<(u64, HttpRequest)> {
        self.state().http.iter().map(|(id, r)| (*id, r.clone())).collect()
    }

    /// Its timers still set.
    pub fn timers(&self) -> Vec<Timer> {
        self.state().timers.values().copied().collect()
    }

    /// The pickers it showed, in order.
    pub fn pickers(&self) -> Vec<Picker> {
        self.state().pickers.clone()
    }

    /// The prompts and compose panes it opened, in order.
    pub fn prompts(&self) -> Vec<Prompt> {
        self.state().prompts.clone()
    }

    /// The services it called, in order.
    pub fn services(&self) -> Vec<ServiceCall> {
        self.state().services.clone()
    }

    /// The problems it last set for `path`.
    pub fn problems(&self, path: &str) -> Vec<Problem> {
        self.state().diagnostics.get(path).cloned().unwrap_or_default()
    }

    /// The decorations it last set in a file (by path) or a scratch
    /// buffer (`scratch:<id>`).
    pub fn decorations(&self, buffer: &str) -> Vec<Decoration> {
        self.state().decorations.get(buffer).cloned().unwrap_or_default()
    }

    /// A buffer's text now, its edits made.
    pub fn text(&self, path: &str) -> Option<String> {
        self.state().buffers.get(path).cloned()
    }

    /// What it last put on the clipboard.
    pub fn copied(&self) -> Option<String> {
        self.state().copied.last().cloned()
    }

    /// The project doctor's checks it last set.
    pub fn checks(&self) -> Vec<Check> {
        self.state().checks.clone()
    }

    /// The tasks it last set.
    pub fn tasks(&self) -> Vec<Task> {
        self.state().tasks.clone()
    }

    /// The settings it read, by key, in order (again each time).
    pub fn settings_read(&self) -> Vec<String> {
        self.state().settings_read.clone()
    }

    /// A setting as it is now: what the test set, or the extension.
    pub fn setting_value(&self, key: &str) -> Option<String> {
        self.state().settings.get(key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_is_named_by_its_line_or_its_programs_name() {
        assert!(names("zig build -l", "/t/zig build -l"));
        assert!(names("zig build", "/t/zig build -l"));
        assert!(names("/t/zig", "/t/zig version"));
        assert!(names("zig", "C:\\t\\zig.exe version"));
        assert!(!names("zig bu", "/t/zig build -l"), "a word at a time");
        assert!(!names("zls", "/t/zig version"));
    }

    #[test]
    fn a_request_is_named_by_its_url_and_method() {
        let req = HttpRequest::get("https://api.example.com/v1/items?page=2");
        assert!(names_request("https://api.example.com/v1/items?page=2", &req));
        assert!(names_request("GET https://api.example.com/v1/*", &req));
        assert!(!names_request("POST https://api.example.com/v1/*", &req));
        assert!(!names_request("https://api.example.com/v1/items", &req), "the whole URL unless it says *");
    }

    #[test]
    fn edits_are_made_from_where_the_text_was() {
        let t = "one two\nthree\n";
        let edits = [TextEdit::new((0, 4), (0, 7), "2"), TextEdit::insert((1, 0), "> "), TextEdit::new((0, 0), (0, 3), "1")];
        assert_eq!(apply_edits(t, &edits), "1 2\n> three\n");
        assert_eq!(apply_edits("ab", &[TextEdit::insert((5, 0), "!")]), "ab!", "past the end is the end");
    }

    #[test]
    fn a_process_answered_sends_its_lines_then_its_exit() {
        let fake = Fake::new();
        fake.on_spawn("git status", Ran::ok("a\nb").stderr("warning"));
        let id = Fake::with(|s| s.spawn(&Command::new("/usr/bin/git").arg("status"), false)).unwrap();
        let events = fake.pending();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0], Event::ProcessOutput(ProcessOutput { process: id, stderr: false, line: "a".into() }));
        assert_eq!(events[2], Event::ProcessOutput(ProcessOutput { process: id, stderr: true, line: "warning".into() }));
        assert_eq!(events[3], Event::ProcessExit((id, Some(0))));
        assert_eq!(fake.take(), [format!("spawn {id} /usr/bin/git status")]);
        assert!(fake.take().is_empty(), "taken");
        assert_eq!(fake.calls().len(), 1, "but kept");

        fake.on_spawn_error("git", "git isn't installed");
        assert_eq!(Fake::with(|s| s.spawn(&Command::new("git").arg("log"), false)), Err("git isn't installed".into()));
        assert_eq!(fake.take(), ["spawn failed git log"]);
    }

    #[test]
    fn the_last_answer_given_wins() {
        let fake = Fake::new();
        fake.on_service("agenda.timer", Ok("null")).on_service("agenda.timer", Err("not there"));
        let id = Fake::with(|s| s.call_service("agenda.timer", "{}"));
        assert_eq!(fake.next_event(), Some(Event::ServiceReply((id, Err("not there".into())))));
        assert_eq!(fake.next_event(), None);
    }

    #[test]
    fn each_thread_has_its_own_and_a_new_one_replaces_it() {
        let a = Fake::new();
        a.setting("x.y", "1");
        assert_eq!(Fake::with(|s| s.setting("x.y")), Some("1".into()));
        let _b = Fake::new();
        assert_eq!(Fake::with(|s| s.setting("x.y")), None);
        std::thread::spawn(|| assert_eq!(Fake::with(|s| s.settings.len()), 0)).join().unwrap();
        a.install();
        assert_eq!(Fake::with(|s| s.setting("x.y")), Some("1".into()));
        assert_eq!(a.settings_read(), ["x.y", "x.y"]);
    }
}
