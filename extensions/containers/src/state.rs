//! The extension's state and its pages, as plain logic: what the engines
//! said, what each page shows, what a key does. It asks for work as
//! `Effect`s (run this, lay that page out again, open this URL), which
//! `lib.rs` carries out through the host -- so all of it is tested here,
//! natively, without an engine.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use fenix_extension_api::{page, Key, Line, Page, Role, Span};

use crate::engine::{Engine, Up};
use crate::model::{self, Container, Details, DiskUse, Image, Layer, Load, Network, Stack, Volume};

/// What a process the extension started is for.
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    Probe(Engine),
    Containers,
    Images,
    Volumes,
    Networks,
    Stacks,
    /// The project's compose file, by its name, as `compose config`.
    ProjectConfig(String),
    Stats,
    DiskUse,
    Inspect(String),
    History(String),
    /// JSON to open as a buffer called this.
    Json(String),
    /// A log stream for page `page`, from container `id`, shown as `name`.
    Logs { page: u64, id: String, name: String },
    /// Something done, said as `what`; its output kept for the Compose
    /// tab when it's a stack's.
    Action { what: String, stack: bool },
    /// Starting an engine.
    Start(Engine),
}

/// Work the extension asks the host for.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Spawn { job: Job, engine: Engine, args: Vec<String>, cwd: Option<String> },
    Kill(u64),
    OpenPage { kind: String, title: String },
    Refresh(u64),
    Close(u64),
    Prompt { what: Asking, label: String, initial: String },
    Notify(String),
    OpenUrl(String),
    Copy(String),
    Terminal { engine: Engine, args: Vec<String>, title: String, cwd: Option<String> },
    OpenText { title: String, text: String, language: String },
    Status(Option<String>),
}

/// What a prompt was for.
#[derive(Debug, Clone, PartialEq)]
pub enum Asking {
    Filter(u64),
    /// The tag for an image built from the project's Dockerfile.
    BuildTag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Containers,
    Compose,
    Images,
    Volumes,
    Networks,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::Containers, Tab::Compose, Tab::Images, Tab::Volumes, Tab::Networks];

    fn name(self) -> &'static str {
        match self {
            Tab::Containers => "Containers",
            Tab::Compose => "Compose",
            Tab::Images => "Images",
            Tab::Volumes => "Volumes",
            Tab::Networks => "Networks",
        }
    }
}

/// A line of a log page.
#[derive(Debug, Clone, PartialEq)]
pub struct LogLine {
    /// Which of the page's streams it came from.
    pub source: usize,
    pub time: String,
    pub text: String,
}

/// What a page shows.
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Main { tab: Tab, focus: usize, filter: String, menu: bool, armed: Option<String> },
    Container { id: String, focus: usize, menu: bool, reveal: bool, armed: bool },
    Image { id: String },
    Logs { title: String, sources: Vec<String>, lines: VecDeque<LogLine>, follow: bool, timestamps: bool, filter: String, hidden: HashSet<usize>, focus: usize },
    Performance { by_memory: bool, focus: usize },
}

/// A line of the Main page's list, and what it stands for.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    Heading(String),
    Container(String),
    Stack(String),
    Service { stack: String, service: String },
    Image(String),
    Volume(String),
    Network(String),
    /// A line to read, not to act on.
    Note(String, Role),
}

/// The project's own compose file, read with `compose config`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProjectStack {
    pub name: String,
    pub file: String,
    /// `(service, image or "build ./dir")`.
    pub services: Vec<(String, String)>,
}

/// How many lines a log page keeps.
const KEPT_LOG_LINES: usize = 5000;
/// How many samples a container's load keeps (a minute, at one a second
/// or two).
const SAMPLES: usize = 60;

#[derive(Debug, Default)]
pub struct State {
    pub engines: BTreeMap<u8, (Engine, Up)>,
    pub current: Option<Engine>,
    /// `containers.engine`: one pinned, else the first that's up.
    pub pinned: Option<Engine>,
    pub containers: Vec<Container>,
    pub images: Vec<Image>,
    pub volumes: Vec<Volume>,
    pub networks: Vec<Network>,
    pub stacks: Vec<Stack>,
    pub project: Option<ProjectStack>,
    pub loads: HashMap<String, VecDeque<Load>>,
    pub disk: Vec<DiskUse>,
    pub details: HashMap<String, Details>,
    pub history: HashMap<String, Vec<Layer>>,
    /// The last stack action's output, and what it was.
    pub last_run: Option<(String, Vec<String>)>,
    pub views: HashMap<u64, View>,
    pub shown: HashSet<u64>,
    /// Processes running, and what each is for, with what it wrote.
    pub jobs: HashMap<u64, (Job, Vec<String>, Vec<String>)>,
    /// The stats stream, while one runs.
    pub stats_process: Option<u64>,
    /// Each log page's containers, `(id, name)`, to stream again when it's
    /// seen again.
    pub log_targets: HashMap<u64, Vec<(String, String)>>,
}

fn order(e: Engine) -> u8 {
    match e {
        Engine::Docker => 0,
        Engine::Podman => 1,
        Engine::Wslc => 2,
    }
}

fn s(text: impl Into<String>, role: Role) -> Span {
    Span { text: text.into(), role }
}

fn line(spans: Vec<Span>) -> Line {
    Line { spans, rule: false }
}

/// `left` with `right` set against the page's right edge.
fn spread(mut left: Vec<Span>, right: Vec<Span>, width: u32) -> Line {
    let used: usize = left.iter().chain(&right).map(|s| s.text.chars().count()).sum();
    let gap = (width as usize).saturating_sub(used).max(2);
    left.push(s(" ".repeat(gap), Role::Text));
    left.extend(right);
    line(left)
}

/// Text cut to `n` cells, with `…` when it was longer.
fn fit(text: &str, n: usize) -> String {
    if text.chars().count() <= n {
        format!("{text:<n$}")
    } else {
        let cut: String = text.chars().take(n.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// A bar `cells` wide, `fraction` full.
fn bar(fraction: f32, cells: usize) -> String {
    let full = ((fraction.clamp(0.0, 1.0) * cells as f32).round() as usize).min(cells);
    format!("{}{}", "█".repeat(full), "░".repeat(cells - full))
}

/// The last samples as a sparkline, from its lowest to its highest: for
/// memory, which moves little against its own size.
pub fn sparkline_between(values: &[f32]) -> String {
    let low = values.iter().cloned().fold(f32::INFINITY, f32::min);
    let high = values.iter().cloned().fold(0.0f32, f32::max);
    if values.is_empty() || high - low <= high * 0.01 {
        return "▄".repeat(values.len());
    }
    sparkline(&values.iter().map(|v| v - low).collect::<Vec<_>>())
}

/// The last samples as a sparkline, from nothing.
pub fn sparkline(values: &[f32]) -> String {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = values.iter().cloned().fold(0.0f32, f32::max).max(1.0);
    values.iter().map(|v| LEVELS[((v / max) * 7.0).round().clamp(0.0, 7.0) as usize]).collect()
}

/// How a log line reads: an error, a warning, or just a line.
fn log_role(text: &str) -> Role {
    let upper = text.to_ascii_uppercase();
    if ["ERROR", "FATAL", "PANIC", "EXCEPTION"].iter().any(|w| upper.contains(w)) {
        Role::Bad
    } else if ["WARN"].iter().any(|w| upper.contains(w)) {
        Role::Warn
    } else if upper.contains("DEBUG") || upper.contains("TRACE") {
        Role::Muted
    } else {
        Role::Text
    }
}

/// Colours for a stack's services, in turn.
const SERVICE_ROLES: [Role; 4] = [Role::Accent, Role::Good, Role::Warn, Role::Title];

impl State {
    pub fn engine(&self) -> Option<Engine> {
        self.current
    }

    fn up(&self, e: Engine) -> Option<&Up> {
        self.engines.get(&order(e)).map(|(_, up)| up)
    }

    fn run(&self, job: Job, args: Vec<String>) -> Option<Effect> {
        Some(Effect::Spawn { job, engine: self.current?, args, cwd: None })
    }

    /// At activation: every engine asked whether it's there.
    pub fn activate(&mut self, pinned: Option<Engine>) -> Vec<Effect> {
        self.pinned = pinned;
        Engine::ALL
            .into_iter()
            .map(|e| {
                self.engines.insert(order(e), (e, Up::Probing));
                Effect::Spawn { job: Job::Probe(e), engine: e, args: e.probe(), cwd: None }
            })
            .collect()
    }

    /// The engine to use, once the probes are in: the pinned one, else
    /// the first that runs, else the first that's installed.
    fn choose(&mut self) -> bool {
        let before = self.current;
        let running = |e: &Engine| matches!(self.up(*e), Some(Up::Running { .. }));
        let installed = |e: &Engine| matches!(self.up(*e), Some(Up::Running { .. } | Up::Down(_)));
        self.current = self.pinned.filter(installed).or_else(|| Engine::ALL.into_iter().find(running)).or_else(|| Engine::ALL.into_iter().find(installed)).or(self.current);
        before != self.current
    }

    /// Everything listed again, for the engine in use.
    pub fn reload(&mut self) -> Vec<Effect> {
        let Some(e) = self.current else { return Vec::new() };
        if !matches!(self.up(e), Some(Up::Running { .. })) {
            return Vec::new();
        }
        let caps = e.caps();
        let mut out: Vec<Effect> = [Some((Job::Containers, e.containers())), Some((Job::Images, e.images())), caps.volumes.then(|| (Job::Volumes, e.volumes())), caps.networks.then(|| (Job::Networks, e.networks())), caps.compose.then(|| (Job::Stacks, e.stacks()))]
            .into_iter()
            .flatten()
            .filter_map(|(job, args)| self.run(job, args))
            .collect();
        out.extend(self.project_config());
        // Pages opened before the engine was known (restored ones): what
        // they show, asked for now.
        let mut pages: Vec<(&u64, &View)> = self.views.iter().collect();
        pages.sort_by_key(|(p, _)| **p);
        for (page, view) in pages {
            match view {
                View::Container { id, .. } if !self.details.contains_key(id) && !self.jobs.values().any(|(j, _, _)| *j == Job::Inspect(id.clone())) => out.push(Effect::Spawn { job: Job::Inspect(id.clone()), engine: e, args: e.inspect(id), cwd: None }),
                View::Image { id } if caps.history && !self.history.contains_key(id) && !self.jobs.values().any(|(j, _, _)| *j == Job::History(id.clone())) => out.push(Effect::Spawn { job: Job::History(id.clone()), engine: e, args: e.history(id), cwd: None }),
                View::Logs { .. } if self.shown.contains(page) && self.log_streams(*page).is_empty() => out.extend(self.stream_logs(*page)),
                _ => {}
            }
        }
        out
    }

    /// The containers, again (on the timer).
    pub fn tick(&mut self) -> Vec<Effect> {
        let busy = self.jobs.values().any(|(j, _, _)| *j == Job::Containers);
        let main_shown = self.shown.iter().any(|p| matches!(self.views.get(p), Some(View::Main { .. } | View::Performance { .. })));
        let mut out = Vec::new();
        if main_shown && !busy {
            if let Some(e) = self.current.filter(|e| matches!(self.up(*e), Some(Up::Running { .. }))) {
                out.extend(self.run(Job::Containers, e.containers()));
                if !e.caps().stats_stream && !self.jobs.values().any(|(j, _, _)| *j == Job::Stats) {
                    out.extend(self.run(Job::Stats, e.stats()));
                }
            }
        }
        out.extend(self.stats_wanted());
        out
    }

    /// The project's compose file, when there is one under `/project`.
    fn project_config(&self) -> Option<Effect> {
        let e = self.current.filter(|e| e.caps().compose)?;
        let file = ["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml"].into_iter().find(|f| std::path::Path::new("/project").join(f).is_file())?;
        Some(Effect::Spawn { job: Job::ProjectConfig(file.to_string()), engine: e, args: vec!["compose".into(), "-f".into(), file.into(), "config".into(), "--format".into(), "json".into()], cwd: Some("/project".into()) })
    }

    /// Start or stop the stats stream as pages want it.
    pub fn stats_wanted(&mut self) -> Vec<Effect> {
        let want = self.current.is_some_and(|e| e.caps().stats_stream && matches!(self.up(e), Some(Up::Running { .. })))
            && self.shown.iter().any(|p| matches!(self.views.get(p), Some(View::Main { tab: Tab::Containers, .. } | View::Performance { .. } | View::Container { .. })));
        match (want, self.stats_process) {
            (true, None) if !self.jobs.values().any(|(j, _, _)| *j == Job::Stats) => self.current.map(|e| vec![Effect::Spawn { job: Job::Stats, engine: e, args: e.stats(), cwd: None }]).unwrap_or_default(),
            (false, Some(p)) => {
                self.stats_process = None;
                vec![Effect::Kill(p)]
            }
            _ => Vec::new(),
        }
    }

    /// A process started: what it's for, kept until it ends.
    pub fn started(&mut self, process: u64, job: Job) {
        if job == Job::Stats && self.current.is_some_and(|e| e.caps().stats_stream) {
            self.stats_process = Some(process);
        }
        self.jobs.insert(process, (job, Vec::new(), Vec::new()));
    }

    /// A line a process wrote. Streams (stats, logs) are read as they
    /// come; the rest kept until the end.
    pub fn output(&mut self, process: u64, stderr: bool, text: &str) -> Vec<Effect> {
        let Some((job, out, err)) = self.jobs.get_mut(&process) else { return Vec::new() };
        match job.clone() {
            Job::Stats if self.current.is_some_and(|e| e.caps().stats_stream) => {
                // Piped, `stats` still draws: a cursor move before each
                // line, an erase after it. The JSON is what's between.
                let text = match (text.find('{'), text.rfind('}')) {
                    (Some(start), Some(end)) if start < end => &text[start..=end],
                    _ => text,
                };
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
                    if let Some((id, load)) = model::load(&v) {
                        self.add_load(&id, load);
                    }
                }
                self.refresh_where(|v| matches!(v, View::Main { tab: Tab::Containers, .. } | View::Performance { .. } | View::Container { .. }))
            }
            Job::Logs { page, id: _, name } => {
                let Some(View::Logs { sources, lines, .. }) = self.views.get_mut(&page) else { return Vec::new() };
                let source = sources.iter().position(|n| *n == name).unwrap_or(0);
                let (time, rest) = match text.split_once(' ') {
                    Some((t, rest)) if t.len() >= 19 && t.as_bytes()[4] == b'-' => (t[11..19].to_string(), rest.to_string()),
                    _ => (String::new(), text.to_string()),
                };
                let at = lines.iter().rposition(|l| l.time <= time || time.is_empty()).map_or(0, |i| i + 1);
                lines.insert(at.min(lines.len()), LogLine { source, time, text: if stderr && log_role(&rest) == Role::Text { format!("stderr: {rest}") } else { rest } });
                while lines.len() > KEPT_LOG_LINES {
                    lines.pop_front();
                }
                vec![Effect::Refresh(page)]
            }
            Job::Action { stack: true, .. } => {
                (if stderr { err } else { out }).push(text.to_string());
                if let Some((_, lines)) = &mut self.last_run {
                    lines.push(text.trim().to_string());
                }
                self.refresh_where(|v| matches!(v, View::Main { tab: Tab::Compose, .. }))
            }
            _ => {
                (if stderr { err } else { out }).push(text.to_string());
                Vec::new()
            }
        }
    }

    fn add_load(&mut self, id: &str, load: Load) {
        let key = self.containers.iter().find(|c| c.id.starts_with(id) || c.name == id).map(|c| c.id.clone()).unwrap_or_else(|| id.to_string());
        let series = self.loads.entry(key).or_default();
        series.push_back(load);
        while series.len() > SAMPLES {
            series.pop_front();
        }
    }

    /// The latest load of container `c`.
    fn load_of(&self, c: &Container) -> Option<Load> {
        self.loads.get(&c.id).and_then(|s| s.back()).copied()
    }

    fn refresh_where(&self, which: impl Fn(&View) -> bool) -> Vec<Effect> {
        let mut pages: Vec<u64> = self.views.iter().filter(|(p, v)| self.shown.contains(p) && which(v)).map(|(p, _)| *p).collect();
        pages.sort();
        pages.into_iter().map(Effect::Refresh).collect()
    }

    fn refresh_all(&self) -> Vec<Effect> {
        let mut pages: Vec<u64> = self.views.keys().copied().collect();
        pages.sort();
        pages.into_iter().map(Effect::Refresh).collect()
    }

    /// A process ended: its output read for what it was for.
    pub fn exited(&mut self, process: u64, code: Option<i32>) -> Vec<Effect> {
        let Some((job, out, err)) = self.jobs.remove(&process) else { return Vec::new() };
        if self.stats_process == Some(process) {
            self.stats_process = None;
        }
        let text = out.join("\n");
        let ok = code == Some(0);
        let mut effects = Vec::new();
        match job {
            Job::Probe(e) => effects = self.probed(e, e.read_probe(code, &text, &err.join("\n"))),
            Job::Containers if ok => {
                self.containers = if self.current == Some(Engine::Wslc) { model::wslc_containers(&text) } else { model::containers(&text) };
                effects.extend(self.status());
                effects.extend(self.refresh_all());
            }
            Job::Images if ok => {
                self.images = if self.current == Some(Engine::Wslc) { model::wslc_images(&text) } else { model::images(&text) };
                effects.extend(self.refresh_all());
            }
            Job::Volumes if ok => {
                self.volumes = model::volumes(&text);
                effects.extend(self.refresh_all());
            }
            Job::Networks if ok => {
                self.networks = model::networks(&text);
                effects.extend(self.refresh_all());
            }
            Job::Stacks if ok => {
                self.stacks = model::stacks(&text);
                effects.extend(self.refresh_all());
            }
            Job::ProjectConfig(file) if ok => {
                self.project = serde_json::from_str::<serde_json::Value>(&text).ok().map(|v| ProjectStack {
                    name: v.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_string(),
                    file,
                    services: v
                        .get("services")
                        .and_then(|s| s.as_object())
                        .map(|m| {
                            m.iter()
                                .map(|(name, svc)| {
                                    let what = svc.get("image").and_then(|i| i.as_str()).map(str::to_string).unwrap_or_else(|| format!("build {}", svc.pointer("/build/context").and_then(|c| c.as_str()).unwrap_or(".")));
                                    (name.clone(), what)
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                });
                effects.extend(self.refresh_all());
            }
            Job::Stats => {
                // wslc's, asked once: columns of NAME/ID, CPU %, MEM USAGE.
                for row in model::table(&text) {
                    let get = |names: &[&str]| row.iter().find(|(h, _)| names.contains(&h.as_str())).map(|(_, v)| v.clone()).unwrap_or_default();
                    let id = get(&["CONTAINER ID", "CONTAINER", "NAME", "ID"]);
                    if !id.is_empty() {
                        let mem = get(&["MEM USAGE / LIMIT", "MEM USAGE"]);
                        let mut m = mem.split('/').map(|x| model::bytes(&x.replace(' ', "")));
                        self.add_load(&id, Load { cpu: model::percent(&get(&["CPU %", "CPU"])), mem: m.next().unwrap_or(0), mem_limit: m.next().unwrap_or(0), ..Load::default() });
                    }
                }
                effects.extend(self.refresh_where(|v| matches!(v, View::Main { .. } | View::Performance { .. })));
            }
            Job::DiskUse if ok => {
                self.disk = model::disk_use(&text);
                effects.extend(self.refresh_where(|v| matches!(v, View::Performance { .. })));
            }
            Job::Inspect(id) if ok => {
                if let Some(d) = model::details(&text) {
                    self.details.insert(id, d);
                }
                effects.extend(self.refresh_where(|v| matches!(v, View::Container { .. })));
            }
            Job::Json(title) if ok => effects.push(Effect::OpenText { title, text, language: "json".into() }),
            Job::Json(title) => effects.push(Effect::Notify(format!("{title}: {}", err.join(" ").trim()))),
            Job::History(id) if ok => {
                self.history.insert(id, model::layers(&text));
                effects.extend(self.refresh_where(|v| matches!(v, View::Image { .. })));
            }
            Job::Logs { page, name, .. } => {
                if let Some(View::Logs { sources, lines, .. }) = self.views.get_mut(&page) {
                    let source = sources.iter().position(|n| *n == name).unwrap_or(0);
                    lines.push_back(LogLine { source, time: String::new(), text: format!("-- {name} stopped writing{}", code.map(|c| format!(" (exit {c})")).unwrap_or_default()) });
                }
                effects.push(Effect::Refresh(page));
            }
            Job::Action { what, stack } => {
                let said = if ok { format!("{what}: done") } else { format!("{what}: failed -- {}", err.iter().chain(&out).rev().find(|l| !l.trim().is_empty()).map(|l| l.trim()).unwrap_or("no reason given")) };
                effects.push(Effect::Notify(said));
                if stack {
                    if let Some((title, _)) = &mut self.last_run {
                        *title = format!("{title} · {}", if ok { "done" } else { "failed" });
                    }
                }
                effects.extend(self.reload());
            }
            Job::Start(e) => {
                effects.push(Effect::Notify(if ok { format!("{} is starting", e.program()) } else { format!("{} couldn't start: {}", e.program(), err.join(" ").trim()) }));
                self.engines.insert(order(e), (e, Up::Probing));
                effects.push(Effect::Spawn { job: Job::Probe(e), engine: e, args: e.probe(), cwd: None });
            }
            Job::Containers | Job::Images | Job::Volumes | Job::Networks | Job::Stacks | Job::ProjectConfig(_) | Job::DiskUse | Job::Inspect(_) | Job::History(_) => {}
        }
        effects
    }

    /// What engine `e`'s probe found: once they're all in (or this one
    /// runs), the engine to use, and everything listed.
    fn probed(&mut self, e: Engine, up: Up) -> Vec<Effect> {
        self.engines.insert(order(e), (e, up));
        let mut effects = Vec::new();
        let probing = self.engines.values().any(|(_, up)| *up == Up::Probing);
        if !probing || matches!(self.up(e), Some(Up::Running { .. })) {
            let changed = self.choose();
            if changed || !probing {
                effects.extend(self.reload());
            }
        }
        effects.extend(self.status());
        effects.extend(self.refresh_all());
        effects
    }

    /// A job whose process couldn't start. For a probe, that's an engine
    /// that isn't installed.
    pub fn unspawnable(&mut self, job: Job) -> Vec<Effect> {
        match job {
            Job::Probe(e) => self.probed(e, Up::Missing),
            _ => Vec::new(),
        }
    }

    /// The modeline item: the engine in use, and how many run.
    fn status(&self) -> Vec<Effect> {
        let Some(e) = self.current else { return vec![Effect::Status(None)] };
        let text = match self.up(e) {
            Some(Up::Running { .. }) => format!("{} · {} running", e.program(), self.containers.iter().filter(|c| c.running()).count()),
            Some(Up::Down(_)) => format!("{} · down", e.program()),
            _ => return Vec::new(),
        };
        vec![Effect::Status(Some(text))]
    }

    /// A page opened as `page`: what it shows, if it's one asked for.
    pub fn opened(&mut self, page: u64, kind: &str, arg: Option<String>) -> Vec<Effect> {
        let view = match kind {
            "main" => View::Main { tab: Tab::Containers, focus: 0, filter: String::new(), menu: false, armed: None },
            "performance" => View::Performance { by_memory: true, focus: 0 },
            "container" => {
                let id = arg.unwrap_or_default();
                let mut effects = Vec::new();
                if let Some(e) = self.current {
                    effects.push(Effect::Spawn { job: Job::Inspect(id.clone()), engine: e, args: e.inspect(&id), cwd: None });
                }
                self.views.insert(page, View::Container { id, focus: 0, menu: false, reveal: false, armed: false });
                return effects;
            }
            "image" => {
                let id = arg.unwrap_or_default();
                let mut effects = Vec::new();
                if let Some(e) = self.current.filter(|e| e.caps().history) {
                    effects.push(Effect::Spawn { job: Job::History(id.clone()), engine: e, args: e.history(&id), cwd: None });
                }
                self.views.insert(page, View::Image { id });
                return effects;
            }
            _ => return Vec::new(),
        };
        self.views.insert(page, view);
        Vec::new()
    }

    /// A page opened as `page` by its kind, `main`, `performance`,
    /// `container:<id>`, `image:<id>` or `logs:<id>=<name>,...`.
    pub fn open(&mut self, page: u64, kind: &str, title: &str) -> Vec<Effect> {
        match kind.split_once(':') {
            Some(("logs", _)) => self.open_logs(page, title, &log_targets(kind).unwrap_or_default()),
            Some((base, arg)) => self.opened(page, base, Some(arg.to_string())),
            None => self.opened(page, kind, None),
        }
    }

    /// What page `page` needs to open again next time.
    pub fn save(&self, page: u64) -> Option<String> {
        match self.views.get(&page)? {
            View::Main { tab, .. } => Some(Tab::ALL.iter().position(|t| t == tab).unwrap_or(0).to_string()),
            View::Container { id, .. } | View::Image { id } => Some(id.clone()),
            View::Logs { .. } => Some(self.log_targets.get(&page)?.iter().map(|(id, n)| format!("{id}={n}")).collect::<Vec<_>>().join(",")),
            View::Performance { .. } => Some(String::new()),
        }
    }

    /// Page `page`, of `kind`, opened again from what `save` gave.
    pub fn restore(&mut self, page: u64, kind: &str, state: &str) -> Option<Vec<Effect>> {
        let effects = match kind {
            "main" => {
                let out = self.opened(page, "main", None);
                if let Some(View::Main { tab, .. }) = self.views.get_mut(&page) {
                    *tab = Tab::ALL[state.parse::<usize>().unwrap_or(0).min(4)];
                }
                out
            }
            "performance" => self.opened(page, kind, None),
            "container" | "image" if !state.is_empty() => self.opened(page, kind, Some(state.to_string())),
            "logs" => {
                let targets = log_targets(&format!("logs:{state}"))?;
                let title = format!("{} · logs", targets.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", "));
                self.open_logs(page, &title, &targets)
            }
            _ => return None,
        };
        Some(effects)
    }

    /// `containers.engine` changed: `None` picks the first that runs.
    pub fn pin(&mut self, engine: Option<Engine>) -> Vec<Effect> {
        self.pinned = engine;
        let before = self.current;
        self.choose();
        match (before, self.current) {
            (Some(b), Some(now)) if b != now => {
                self.current = Some(b);
                self.switch_to(now)
            }
            (None, Some(_)) => {
                let mut out = self.reload();
                out.extend(self.status());
                out
            }
            _ => Vec::new(),
        }
    }

    /// A log page for these containers (`(id, name)`), opened as `page`.
    pub fn open_logs(&mut self, page: u64, title: &str, targets: &[(String, String)]) -> Vec<Effect> {
        let sources = targets.iter().map(|(_, n)| n.clone()).collect();
        self.views.insert(page, View::Logs { title: title.into(), sources, lines: VecDeque::new(), follow: true, timestamps: false, filter: String::new(), hidden: HashSet::new(), focus: 0 });
        self.log_targets.insert(page, targets.to_vec());
        self.stream_logs(page)
    }

    /// Page `page`'s log streams started, each from its last lines.
    fn stream_logs(&self, page: u64) -> Vec<Effect> {
        let (Some(e), Some(targets)) = (self.current, self.log_targets.get(&page)) else { return Vec::new() };
        let tail = if targets.len() > 1 { 200 } else { 500 };
        targets.iter().map(|(id, name)| Effect::Spawn { job: Job::Logs { page, id: id.clone(), name: name.clone() }, engine: e, args: e.logs(id, tail), cwd: None }).collect()
    }

    /// Page `page`'s log streams, by process.
    fn log_streams(&self, page: u64) -> Vec<u64> {
        let mut out: Vec<u64> = self.jobs.iter().filter(|(_, (j, _, _))| matches!(j, Job::Logs { page: p, .. } if *p == page)).map(|(p, _)| *p).collect();
        out.sort();
        out
    }

    /// A page shown, or hidden: streams it needs started or stopped.
    pub fn shown(&mut self, page: u64, shown: bool) -> Vec<Effect> {
        if shown {
            self.shown.insert(page);
        } else {
            self.shown.remove(&page);
        }
        let mut out = self.stats_wanted();
        // A log page streams only while it's seen: hidden, its streams stop;
        // seen again, they start over from their last lines.
        if matches!(self.views.get(&page), Some(View::Logs { .. })) {
            let streams = self.log_streams(page);
            if !shown {
                for p in streams {
                    self.jobs.remove(&p);
                    out.push(Effect::Kill(p));
                }
            } else if streams.is_empty() {
                if let Some(View::Logs { lines, .. }) = self.views.get_mut(&page) {
                    lines.clear();
                }
                out.extend(self.stream_logs(page));
            }
        }
        if shown && matches!(self.views.get(&page), Some(View::Performance { .. })) {
            if let Some(e) = self.current.filter(|e| e.caps().json) {
                out.push(Effect::Spawn { job: Job::DiskUse, engine: e, args: e.disk_use(), cwd: None });
            }
        }
        out
    }

    /// A page closed: its log streams stopped.
    pub fn closed(&mut self, page: u64) -> Vec<Effect> {
        self.views.remove(&page);
        self.shown.remove(&page);
        self.log_targets.remove(&page);
        let mut out: Vec<Effect> = self.log_streams(page).into_iter().map(Effect::Kill).collect();
        out.extend(self.stats_wanted());
        out
    }

    // ---------- the Main page ----------

    fn matches(filter: &str, hay: &str) -> bool {
        let hay = hay.to_lowercase();
        filter.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
    }

    /// The Main page's list for `tab`, kept to `filter`.
    fn items(&self, tab: Tab, filter: &str) -> Vec<Item> {
        let mut out = Vec::new();
        let caps = self.current.map(|e| e.caps());
        match tab {
            Tab::Containers => {
                let shown: Vec<&Container> = self.containers.iter().filter(|c| Self::matches(filter, &format!("{} {} {} {}", c.name, c.image, c.status, c.project.as_deref().unwrap_or("")))).collect();
                let mut stacks: Vec<&str> = shown.iter().filter_map(|c| c.project.as_deref()).collect();
                stacks.sort();
                stacks.dedup();
                for stack in stacks {
                    out.push(Item::Heading(format!("{stack} · compose")));
                    out.extend(shown.iter().filter(|c| c.project.as_deref() == Some(stack)).map(|c| Item::Container(c.id.clone())));
                }
                let rest: Vec<&&Container> = shown.iter().filter(|c| c.project.is_none()).collect();
                if !rest.is_empty() {
                    if !out.is_empty() {
                        out.push(Item::Heading("other".into()));
                    }
                    out.extend(rest.into_iter().map(|c| Item::Container(c.id.clone())));
                }
                if out.is_empty() {
                    out.push(Item::Note(if filter.is_empty() { "No containers.".into() } else { "Nothing matches -- / changes the filter.".into() }, Role::Muted));
                }
            }
            Tab::Compose => {
                if !caps.is_some_and(|c| c.compose) {
                    out.push(Item::Note(format!("{} has no Compose yet.", self.current.map(|e| e.program()).unwrap_or("This engine")), Role::Muted));
                    return out;
                }
                if let Some(p) = &self.project {
                    out.push(Item::Heading(format!("this project · {} · {}", p.name, p.file)));
                    out.push(Item::Stack(p.name.clone()));
                    out.extend(p.services.iter().map(|(svc, _)| Item::Service { stack: p.name.clone(), service: svc.clone() }));
                }
                let others: Vec<&Stack> = self.stacks.iter().filter(|st| self.project.as_ref().is_none_or(|p| p.name != st.name)).filter(|st| Self::matches(filter, &st.name)).collect();
                if !others.is_empty() {
                    out.push(Item::Heading(if self.project.is_some() { "other stacks".into() } else { "stacks".into() }));
                    out.extend(others.into_iter().map(|st| Item::Stack(st.name.clone())));
                }
                if out.is_empty() {
                    out.push(Item::Note("No Compose stacks, and no compose.yaml in this project.".into(), Role::Muted));
                }
            }
            Tab::Images => {
                let used: HashSet<&str> = self.containers.iter().map(|c| c.image.as_str()).collect();
                let in_use = |i: &Image| used.contains(i.reference().as_str()) || used.contains(i.repository.as_str()) || self.containers.iter().any(|c| i.id.contains(&c.image));
                let shown: Vec<&Image> = self.images.iter().filter(|i| Self::matches(filter, &i.reference())).collect();
                let (busy, idle): (Vec<&&Image>, Vec<&&Image>) = shown.iter().partition(|i| in_use(i));
                if !busy.is_empty() {
                    out.push(Item::Heading("in use".into()));
                    out.extend(busy.into_iter().map(|i| Item::Image(i.id.clone())));
                }
                if !idle.is_empty() {
                    out.push(Item::Heading("unused".into()));
                    out.extend(idle.into_iter().map(|i| Item::Image(i.id.clone())));
                }
                if out.is_empty() {
                    out.push(Item::Note("No images.".into(), Role::Muted));
                }
            }
            Tab::Volumes | Tab::Networks if !caps.is_some_and(|c| if tab == Tab::Volumes { c.volumes } else { c.networks }) => {
                out.push(Item::Note(format!("{} has no {} of its own yet.", self.current.map(|e| e.program()).unwrap_or("This engine"), tab.name().to_lowercase()), Role::Muted));
            }
            Tab::Volumes => {
                let shown: Vec<&Volume> = self.volumes.iter().filter(|v| Self::matches(filter, &v.name)).collect();
                let (busy, idle): (Vec<&&Volume>, Vec<&&Volume>) = shown.iter().partition(|v| !self.volume_users(&v.name).is_empty());
                if !busy.is_empty() {
                    out.push(Item::Heading("in use".into()));
                    out.extend(busy.into_iter().map(|v| Item::Volume(v.name.clone())));
                }
                if !idle.is_empty() {
                    out.push(Item::Heading("not in use".into()));
                    out.extend(idle.into_iter().map(|v| Item::Volume(v.name.clone())));
                }
                if out.is_empty() {
                    out.push(Item::Note("No volumes.".into(), Role::Muted));
                }
            }
            Tab::Networks => {
                out.extend(self.networks.iter().filter(|n| Self::matches(filter, &n.name)).map(|n| Item::Network(n.name.clone())));
                if out.is_empty() {
                    out.push(Item::Note("No networks.".into(), Role::Muted));
                }
            }
        }
        out
    }

    /// The containers that use volume `name` (from what `inspect` said),
    /// or whose stack names it.
    fn volume_users(&self, name: &str) -> Vec<String> {
        let mut users: Vec<String> = self.details.iter().filter(|(_, d)| d.mounts.iter().any(|m| m.0 == name)).filter_map(|(id, _)| self.containers.iter().find(|c| &c.id == id).map(|c| c.name.clone())).collect();
        if users.is_empty() {
            users = self.containers.iter().filter(|c| c.project.as_deref().is_some_and(|p| name.starts_with(&format!("{p}_")))).map(|c| c.name.clone()).collect();
        }
        users
    }

    fn focusable(item: &Item) -> bool {
        !matches!(item, Item::Heading(_) | Item::Note(..))
    }

    fn focused_item(&self, tab: Tab, filter: &str, focus: usize) -> Option<Item> {
        self.items(tab, filter).into_iter().filter(Self::focusable).nth(focus)
    }

    fn container(&self, id: &str) -> Option<&Container> {
        self.containers.iter().find(|c| c.id == id)
    }

    /// A key on page `page`.
    pub fn key(&mut self, page: u64, key: Key) -> Vec<Effect> {
        let Some(view) = self.views.get(&page).cloned() else { return Vec::new() };
        let mut out = match view {
            View::Main { tab, focus, filter, menu, armed } => self.main_key(page, tab, focus, filter, menu, armed, key),
            View::Container { id, focus, menu, reveal, armed } => self.container_key(page, id, focus, menu, reveal, armed, key),
            View::Image { id } => self.image_key(&id, key),
            View::Logs { .. } => self.logs_key(page, key),
            View::Performance { .. } => self.performance_key(page, key),
        };
        out.push(Effect::Refresh(page));
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn main_key(&mut self, page: u64, tab: Tab, focus: usize, filter: String, menu: bool, armed: Option<String>, key: Key) -> Vec<Effect> {
        let count = self.items(tab, &filter).iter().filter(|i| Self::focusable(i)).count();
        let item = self.focused_item(tab, &filter, focus);
        let set = |state: &mut State, tab: Tab, focus: usize, menu: bool, armed: Option<String>| {
            if let Some(View::Main { tab: t, focus: f, menu: m, armed: a, .. }) = state.views.get_mut(&page) {
                *t = tab;
                *f = focus;
                *m = menu;
                *a = armed;
            }
        };
        let e = self.current;
        // The engine isn't up: Enter starts it, `e` picks another.
        if let Some(engine) = e {
            if let Some(Up::Down(_)) = self.up(engine) {
                return match key {
                    Key::Enter => engine.starter().map(|(what, args)| vec![Effect::Notify(format!("Asking {} to {what}…", engine.program())), Effect::Spawn { job: Job::Start(engine), engine, args, cwd: None }]).unwrap_or_default(),
                    Key::Char('e') => self.next_engine(),
                    _ => Vec::new(),
                };
            }
        }
        if menu {
            set(self, tab, focus, false, None);
            return match (key, item) {
                (Key::Escape | Key::Char('q') | Key::Char('m'), _) => Vec::new(),
                (key, Some(Item::Container(id))) => self.container_action(page, &id, key),
                _ => Vec::new(),
            };
        }
        let disarm = |state: &mut State| set(state, tab, focus, false, None);
        match key {
            Key::Char(c @ '1'..='5') => {
                let t = Tab::ALL[(c as usize) - ('1' as usize)];
                set(self, t, 0, false, None);
                Vec::new()
            }
            Key::Char(']') | Key::Tab => {
                let i = Tab::ALL.iter().position(|t| *t == tab).unwrap_or(0);
                set(self, Tab::ALL[(i + 1) % 5], 0, false, None);
                Vec::new()
            }
            Key::Char('[') | Key::BackTab => {
                let i = Tab::ALL.iter().position(|t| *t == tab).unwrap_or(0);
                set(self, Tab::ALL[(i + 4) % 5], 0, false, None);
                Vec::new()
            }
            Key::Char('j') | Key::Down => {
                set(self, tab, (focus + 1).min(count.saturating_sub(1)), false, None);
                Vec::new()
            }
            Key::Char('k') | Key::Up => {
                set(self, tab, focus.saturating_sub(1), false, None);
                Vec::new()
            }
            Key::Char('g') | Key::Home => {
                set(self, tab, 0, false, None);
                Vec::new()
            }
            Key::Char('G') | Key::End => {
                set(self, tab, count.saturating_sub(1), false, None);
                Vec::new()
            }
            Key::Char('e') => self.next_engine(),
            Key::Char('r') if tab != Tab::Compose => self.reload(),
            Key::Char('/') => vec![Effect::Prompt { what: Asking::Filter(page), label: "Filter".into(), initial: filter }],
            Key::Char('P') if tab == Tab::Containers => vec![Effect::OpenPage { kind: "performance".into(), title: "Performance".into() }],
            Key::Char('m') if matches!(item, Some(Item::Container(_))) => {
                set(self, tab, focus, true, None);
                Vec::new()
            }
            Key::Char('x') => {
                let target = match &item {
                    Some(Item::Container(id)) => self.container(id).map(|c| (c.name.clone(), "container", c.id.clone())),
                    Some(Item::Image(id)) => self.images.iter().find(|i| &i.id == id).map(|i| (i.reference(), "image", i.id.clone())),
                    Some(Item::Volume(n)) => Some((n.clone(), "volume", n.clone())),
                    Some(Item::Network(n)) if !self.networks.iter().any(|x| &x.name == n && x.built_in()) => Some((n.clone(), "network", n.clone())),
                    _ => None,
                };
                let Some((name, kind, id)) = target else { return Vec::new() };
                if armed.as_deref() == Some(id.as_str()) {
                    disarm(self);
                    let Some(e) = e else { return Vec::new() };
                    let args = if kind == "container" { e.act("rm", &id) } else { e.remove(kind, &id) };
                    self.run(Job::Action { what: format!("remove {name}"), stack: false }, args).into_iter().collect()
                } else {
                    set(self, tab, focus, false, Some(id));
                    vec![Effect::Notify(format!("x again removes {name} -- anything else keeps it"))]
                }
            }
            Key::Char('P') if matches!(tab, Tab::Images | Tab::Volumes | Tab::Networks) => {
                let kind = match tab {
                    Tab::Images => "image",
                    Tab::Volumes => "volume",
                    _ => "network",
                };
                if armed.as_deref() == Some("prune") {
                    disarm(self);
                    let Some(e) = e else { return Vec::new() };
                    self.run(Job::Action { what: format!("prune unused {kind}s"), stack: false }, e.prune(kind)).into_iter().collect()
                } else {
                    set(self, tab, focus, false, Some("prune".into()));
                    vec![Effect::Notify(format!("P again removes every {kind} nothing uses -- anything else keeps them"))]
                }
            }
            key => match item {
                Some(Item::Container(id)) => self.container_action(page, &id, key),
                Some(Item::Image(id)) => self.image_key(&id, key),
                Some(Item::Stack(name)) | Some(Item::Service { stack: name, .. }) => self.stack_key(&name, key),
                _ => Vec::new(),
            },
        }
    }

    /// `e`: the next engine that's installed.
    fn next_engine(&mut self) -> Vec<Effect> {
        let installed: Vec<Engine> = Engine::ALL.into_iter().filter(|e| matches!(self.up(*e), Some(Up::Running { .. } | Up::Down(_)))).collect();
        let Some(i) = self.current.and_then(|c| installed.iter().position(|e| *e == c)) else { return Vec::new() };
        let next = installed[(i + 1) % installed.len()];
        if Some(next) == self.current {
            return vec![Effect::Notify("No other engine is installed".into())];
        }
        self.switch_to(next)
    }

    /// Engine `next` in use, from nothing listed.
    fn switch_to(&mut self, next: Engine) -> Vec<Effect> {
        let mut out = vec![];
        if let Some(p) = self.stats_process.take() {
            self.jobs.remove(&p);
            out.push(Effect::Kill(p));
        }
        self.current = Some(next);
        self.project = None;
        self.details.clear();
        self.history.clear();
        self.disk.clear();
        self.containers.clear();
        self.images.clear();
        self.volumes.clear();
        self.networks.clear();
        self.stacks.clear();
        self.loads.clear();
        out.extend(self.reload());
        out.extend(self.status());
        out.extend(self.stats_wanted());
        out
    }

    /// What a key does to container `id`, wherever it's shown.
    fn container_action(&mut self, page: u64, id: &str, key: Key) -> Vec<Effect> {
        let Some(c) = self.container(id).cloned() else { return Vec::new() };
        let Some(e) = self.current else { return Vec::new() };
        let act = |state: &State, verb: &str, what: &str| state.run(Job::Action { what: format!("{what} {}", c.name), stack: false }, e.act(verb, &c.id)).into_iter().collect::<Vec<_>>();
        match key {
            Key::Enter => vec![Effect::OpenPage { kind: format!("container:{}", c.id), title: c.name.clone() }],
            Key::Char('S') | Key::Char('s') if !c.running() => act(self, "start", "start"),
            Key::Char('S') | Key::Char('s') => act(self, "stop", "stop"),
            Key::Char('R') => act(self, "restart", "restart"),
            Key::Char('p') => act(self, if c.state == "paused" { "unpause" } else { "pause" }, if c.state == "paused" { "unpause" } else { "pause" }),
            Key::Char('K') => act(self, "kill", "kill"),
            Key::Char('l') => {
                let _ = page;
                vec![Effect::OpenPage { kind: format!("logs:{}={}", c.id, c.name), title: format!("{} · logs", c.name) }]
            }
            Key::Char('!') => vec![Effect::Terminal { engine: e, args: e.shell(&c.id), title: c.name.clone(), cwd: None }],
            Key::Char('o') => match c.host_ports().first() {
                Some(port) => vec![Effect::OpenUrl(format!("http://localhost:{port}"))],
                None => vec![Effect::Notify(format!("{} publishes no port", c.name))],
            },
            Key::Char('y') => vec![Effect::Copy(c.id.clone()), Effect::Notify(format!("Copied {}'s id", c.name))],
            Key::Char('i') => self.run(Job::Json(format!("{} · inspect", c.name)), e.inspect(&c.id)).into_iter().collect(),
            _ => Vec::new(),
        }
    }

    fn image_key(&mut self, id: &str, key: Key) -> Vec<Effect> {
        let Some(img) = self.images.iter().find(|i| i.id == id).cloned() else { return Vec::new() };
        let Some(e) = self.current else { return Vec::new() };
        match key {
            Key::Enter => vec![Effect::OpenPage { kind: format!("image:{}", img.id), title: img.reference() }],
            Key::Char('p') if !img.dangling() => self.run(Job::Action { what: format!("pull {}", img.reference()), stack: false }, e.image_act("pull", &img.reference())).into_iter().collect(),
            Key::Char('R') if !img.dangling() => vec![Effect::Terminal { engine: e, args: vec!["run".into(), "--rm".into(), "-it".into(), img.reference()], title: img.reference(), cwd: None }],
            _ => Vec::new(),
        }
    }

    fn stack_key(&mut self, name: &str, key: Key) -> Vec<Effect> {
        let Some(e) = self.current.filter(|e| e.caps().compose) else { return Vec::new() };
        let project = self.project.as_ref().filter(|p| p.name == name);
        // Another stack by the file it came up from, where it says one;
        // `down` needs only its name.
        let theirs = self.stacks.iter().find(|st| st.name == name).and_then(|st| st.files.split(',').next()).map(str::trim).filter(|f| !f.is_empty() && !matches!(key, Key::Char('d')));
        let (file, cwd) = match project {
            Some(p) => (Some(p.file.clone()), Some("/project".to_string())),
            None => (theirs.map(str::to_string), None),
        };
        let verb = match key {
            Key::Char('u') => "up",
            Key::Char('d') => "down",
            Key::Char('R') => "restart",
            Key::Char('p') => "pull",
            Key::Char('b') => "build",
            Key::Char('L') => {
                let targets: Vec<(String, String)> = self.containers.iter().filter(|c| c.project.as_deref() == Some(name)).map(|c| (c.id.clone(), c.service.clone().unwrap_or_else(|| c.name.clone()))).collect();
                if targets.is_empty() {
                    return vec![Effect::Notify(format!("{name} has nothing running to show logs of"))];
                }
                let kind = format!("logs:{}", targets.iter().map(|(id, n)| format!("{id}={n}")).collect::<Vec<_>>().join(","));
                return vec![Effect::OpenPage { kind, title: format!("{name} · logs") }];
            }
            _ => return Vec::new(),
        };
        self.last_run = Some((format!("compose {verb} · {name}"), Vec::new()));
        vec![Effect::Spawn { job: Job::Action { what: format!("compose {verb} {name}"), stack: true }, engine: e, args: e.compose(verb, name, file.as_deref()), cwd }]
    }

    #[allow(clippy::too_many_arguments)]
    fn container_key(&mut self, page: u64, id: String, focus: usize, menu: bool, reveal: bool, armed: bool, key: Key) -> Vec<Effect> {
        let set = |state: &mut State, focus: usize, menu: bool, reveal: bool, armed: bool| {
            if let Some(View::Container { focus: f, menu: m, reveal: r, armed: a, .. }) = state.views.get_mut(&page) {
                *f = focus;
                *m = menu;
                *r = reveal;
                *a = armed;
            }
        };
        if menu {
            set(self, focus, false, reveal, false);
            if matches!(key, Key::Escape | Key::Char('q') | Key::Char('m')) {
                return Vec::new();
            }
        }
        match key {
            Key::Char('m') => {
                set(self, focus, true, reveal, false);
                Vec::new()
            }
            Key::Char('v') => {
                set(self, focus, false, !reveal, false);
                Vec::new()
            }
            Key::Char('x') => {
                let Some(c) = self.container(&id).cloned() else { return Vec::new() };
                if armed {
                    set(self, focus, false, reveal, false);
                    let Some(e) = self.current else { return Vec::new() };
                    let mut out: Vec<Effect> = self.run(Job::Action { what: format!("remove {}", c.name), stack: false }, e.act("rm", &c.id)).into_iter().collect();
                    out.push(Effect::Close(page));
                    out
                } else {
                    set(self, focus, false, reveal, true);
                    vec![Effect::Notify(format!("x again removes {}{}", c.name, if c.running() { " -- stop it first (s)" } else { "" }))]
                }
            }
            Key::Enter => match self.container(&id).and_then(|c| c.host_ports().into_iter().next()) {
                Some(port) => vec![Effect::OpenUrl(format!("http://localhost:{port}"))],
                None => Vec::new(),
            },
            Key::Char('j') | Key::Down => {
                set(self, focus + 1, false, reveal, false);
                Vec::new()
            }
            Key::Char('k') | Key::Up => {
                set(self, focus.saturating_sub(1), false, reveal, false);
                Vec::new()
            }
            key => self.container_action(page, &id, key),
        }
    }

    fn logs_key(&mut self, page: u64, key: Key) -> Vec<Effect> {
        let Some(View::Logs { follow, timestamps, hidden, focus, lines, filter, sources, .. }) = self.views.get_mut(&page) else { return Vec::new() };
        let count = lines.iter().filter(|l| !hidden.contains(&l.source) && State::matches(filter, &l.text)).count();
        match key {
            Key::Char('F') => {
                *follow = true;
                *focus = count.saturating_sub(1);
            }
            Key::Char('t') => *timestamps = !*timestamps,
            Key::Char('j') | Key::Down => {
                *focus = (*focus + 1).min(count.saturating_sub(1));
                *follow = *focus + 1 >= count;
            }
            Key::Char('k') | Key::Up => {
                *focus = focus.saturating_sub(1);
                *follow = false;
            }
            Key::Char('g') | Key::Home => {
                *focus = 0;
                *follow = false;
            }
            Key::Char('G') | Key::End => {
                *focus = count.saturating_sub(1);
                *follow = true;
            }
            Key::Char(c @ '1'..='9') => {
                let n = c as usize - '1' as usize;
                if n < sources.len() && !hidden.remove(&n) {
                    hidden.insert(n);
                }
            }
            Key::Char('/') => return vec![Effect::Prompt { what: Asking::Filter(page), label: "Filter the log".into(), initial: filter.clone() }],
            Key::Char('o') => {
                let text = lines.iter().map(|l| format!("{} {} {}", l.time, sources.get(l.source).map(String::as_str).unwrap_or(""), l.text)).collect::<Vec<_>>().join("\n");
                return vec![Effect::OpenText { title: "log".into(), text, language: "log".into() }];
            }
            _ => {}
        }
        Vec::new()
    }

    fn performance_key(&mut self, page: u64, key: Key) -> Vec<Effect> {
        let Some(View::Performance { by_memory, focus }) = self.views.get_mut(&page) else { return Vec::new() };
        match key {
            Key::Char('o') => *by_memory = !*by_memory,
            Key::Char('j') | Key::Down => *focus += 1,
            Key::Char('k') | Key::Up => *focus = focus.saturating_sub(1),
            _ => {}
        }
        Vec::new()
    }

    /// `containers.build`: the project's Dockerfile built, under a tag asked
    /// for.
    pub fn build(&self) -> Vec<Effect> {
        if self.current.is_none() {
            return vec![Effect::Notify("No container engine yet".into())];
        }
        if !std::path::Path::new("/project/Dockerfile").is_file() && !std::path::Path::new("/project/Containerfile").is_file() {
            return vec![Effect::Notify("This project has no Dockerfile".into())];
        }
        vec![Effect::Prompt { what: Asking::BuildTag, label: "Build the image as".into(), initial: "app:dev".into() }]
    }

    /// A prompt answered.
    pub fn prompted(&mut self, what: Asking, text: Option<String>) -> Vec<Effect> {
        let Some(text) = text else { return Vec::new() };
        match what {
            Asking::BuildTag => {
                let (Some(e), tag) = (self.current, text.trim()) else { return Vec::new() };
                if tag.is_empty() {
                    return Vec::new();
                }
                vec![Effect::Terminal { engine: e, args: vec!["build".into(), "-t".into(), tag.into(), ".".into()], title: format!("build {tag}"), cwd: Some("/project".into()) }]
            }
            Asking::Filter(page) => match self.views.get_mut(&page) {
                Some(View::Main { filter, focus, .. }) => {
                    *filter = text;
                    *focus = 0;
                    vec![Effect::Refresh(page)]
                }
                Some(View::Logs { filter, .. }) => {
                    *filter = text;
                    vec![Effect::Refresh(page)]
                }
                _ => Vec::new(),
            },
        }
    }

    // ---------- laying pages out ----------

    pub fn render(&self, page: u64, width: u32) -> Page {
        match self.views.get(&page) {
            Some(View::Main { tab, focus, filter, menu, armed }) => self.render_main(*tab, *focus, filter, *menu, armed.as_deref(), width),
            Some(View::Container { id, menu, reveal, armed, .. }) => self.render_container(id, *menu, *reveal, *armed, width),
            Some(View::Image { id }) => self.render_image(id, width),
            Some(View::Logs { title, sources, lines, follow, timestamps, filter, hidden, focus }) => Self::render_logs(title, sources, lines, *follow, *timestamps, filter, hidden, *focus, width),
            Some(View::Performance { by_memory, focus }) => self.render_performance(*by_memory, *focus, width),
            None => Page::new().line([page::muted("This page's extension doesn't know it any more.")]),
        }
    }

    /// The header the Main and Performance pages have, under the title
    /// Fenix gives them: the engines, and the load.
    fn header(&self, width: u32) -> Vec<Line> {
        let mut chips = Vec::new();
        for (e, up) in self.engines.values() {
            let (mark, role) = match up {
                Up::Running { .. } => ("●", Role::Good),
                Up::Down(_) => ("●", Role::Warn),
                Up::Probing => ("○", Role::Muted),
                Up::Missing => continue,
            };
            let name = match up {
                Up::Running { version } => format!("{} {version}", e.program()),
                _ => e.program().to_string(),
            };
            let on = self.current == Some(*e);
            chips.push(s(format!("{mark} "), role));
            chips.push(s(if on { format!("[{name}]") } else { name }, if on { Role::Accent } else { Role::Muted }));
            chips.push(s("  ", Role::Text));
        }
        let mut lines = vec![spread(chips, vec![s("e engine", Role::Muted)], width)];
        let running = self.containers.iter().filter(|c| c.running()).count();
        let stopped = self.containers.len() - running;
        let cpu: f32 = self.containers.iter().filter_map(|c| self.load_of(c)).map(|l| l.cpu).sum();
        let mem: u64 = self.containers.iter().filter_map(|c| self.load_of(c)).map(|l| l.mem).sum();
        let mut summary = vec![s(format!("{running} running · {stopped} stopped"), Role::Muted)];
        if mem > 0 {
            summary.push(s(format!("   CPU {cpu:.0}% · memory {}", model::size(mem)), Role::Muted));
        }
        lines.push(line(summary));
        lines
    }

    fn render_main(&self, tab: Tab, focus: usize, filter: &str, menu: bool, armed: Option<&str>, width: u32) -> Page {
        let mut p = Page::new();
        for l in self.header(width) {
            p.lines.push(l);
        }
        // An engine that isn't up says so, and how to start it.
        let e = self.current;
        match e.and_then(|e| self.up(e).map(|u| (e, u))) {
            None => {
                let probing = self.engines.values().any(|(_, u)| *u == Up::Probing);
                p = p.blank().line([page::muted(if probing { "Looking for Docker, Podman and wslc…" } else { "No container engine here: Docker, Podman or wslc (WSL 2.9.3 or later)." })]);
                return p;
            }
            Some((engine, Up::Down(why))) => {
                p = p.blank().line([page::warn(format!("{why}."))]);
                if let Some((what, _)) = engine.starter() {
                    p = p.line([page::accent("Enter"), page::text(format!("  {what}"))]);
                }
                p = p.line([page::accent("e"), page::text("  another engine")]);
                for (other, up) in self.engines.values().filter(|(o, _)| *o != engine) {
                    let said = match up {
                        Up::Running { version } => format!("{}: running · {version}", other.program()),
                        Up::Down(w) => format!("{}: {w}", other.program()),
                        Up::Missing => format!("{}: not installed", other.program()),
                        Up::Probing => format!("{}: looking…", other.program()),
                    };
                    p = p.line([page::muted(said)]);
                }
                return p.key("Enter", "start").key("e", "engine");
            }
            _ => {}
        }
        let caps = e.map(|e| e.caps());
        // The tabs.
        let mut tabs = Vec::new();
        for (n, t) in Tab::ALL.iter().enumerate() {
            let count = match t {
                Tab::Containers => self.containers.len(),
                Tab::Compose => self.stacks.len().max(self.project.is_some() as usize),
                Tab::Images => self.images.len(),
                Tab::Volumes => self.volumes.len(),
                Tab::Networks => self.networks.len(),
            };
            let can = match t {
                Tab::Compose => caps.is_some_and(|c| c.compose),
                Tab::Volumes => caps.is_some_and(|c| c.volumes),
                Tab::Networks => caps.is_some_and(|c| c.networks),
                _ => true,
            };
            let on = *t == tab;
            let label = format!("{} {}{}", n + 1, t.name(), if can && count > 0 { format!(" {count}") } else { String::new() });
            tabs.push(s(if on { format!("[{label}]") } else { label }, if on { Role::Accent } else if can { Role::Text } else { Role::Muted }));
            tabs.push(s("   ", Role::Text));
        }
        p = p.blank().line(tabs);
        if !filter.is_empty() {
            p = p.line([page::accent(format!("/ {filter}")), page::muted("  (/ changes it)")]);
        }
        p = p.rule();
        let items = self.items(tab, filter);
        let mut focusable = 0;
        let mut focus_line = None;
        let mut detail: Option<Item> = None;
        for item in &items {
            let is_focus = Self::focusable(item) && focusable == focus;
            if is_focus {
                focus_line = Some(p.len());
                detail = Some(item.clone());
            }
            if Self::focusable(item) {
                focusable += 1;
            }
            let l = self.item_line(item, armed, width);
            p.lines.push(l);
        }
        if let Some(i) = focus_line {
            p = p.focus(i);
        }
        // The focused row's details, under the list.
        if let Some(Item::Container(id)) = &detail {
            if let Some(c) = self.container(id) {
                p = p.blank().line([page::muted(format!("{} · {}", c.name, c.image))]).rule();
                let mut facts = vec![page::muted("status "), page::text(c.status.clone())];
                if !c.ports.is_empty() {
                    facts.push(page::muted("   ports "));
                    facts.push(page::accent(c.ports.clone()));
                }
                p = p.line(facts);
                if let Some(series) = self.loads.get(&c.id).filter(|s| s.len() > 1) {
                    let cpu: Vec<f32> = series.iter().map(|l| l.cpu).collect();
                    let mem: Vec<f32> = series.iter().map(|l| l.mem as f32).collect();
                    p = p.line([page::muted("cpu, the last minute     "), page::accent(sparkline(&cpu))]).line([page::muted("memory, the last minute  "), page::accent(sparkline_between(&mem))]);
                }
            }
        }
        if let (Some(Item::Container(_)), true) = (&detail, &menu) {
            let rows = MENU.iter().map(|(k, what)| vec![page::accent(format!("{k:<3}")), if *k == "x" { page::bad(*what) } else { page::text(*what) }]).collect::<Vec<_>>();
            p = p.popup(focus_line.unwrap_or(0), rows);
        }
        // What the Compose tab last ran.
        if tab == Tab::Compose {
            if let Some((title, out)) = &self.last_run {
                p = p.blank().line([page::muted(title.clone())]).rule();
                for l in out.iter().rev().take(8).collect::<Vec<_>>().into_iter().rev() {
                    p = p.line([Span { text: l.clone(), role: if l.to_lowercase().contains("error") { Role::Bad } else { Role::Muted } }]);
                }
            }
        }
        // The keys.
        p = match tab {
            Tab::Containers => p.key("Enter", "open").key("m", "menu").key("l", "logs").key("!", "shell").key("s", "start/stop").key("o", "open port").key("x x", "remove").key("P", "performance"),
            Tab::Compose => p.key("u", "up").key("d", "down").key("R", "restart").key("p", "pull").key("b", "build").key("L", "logs"),
            Tab::Images => p.key("Enter", "open").key("R", "run").key("p", "pull").key("x x", "remove").key("P P", "prune"),
            Tab::Volumes | Tab::Networks => p.key("x x", "remove").key("P P", "prune"),
        };
        p.key("1-5", "tab").key("/", "filter").key("e", "engine").key("r", "reload")
    }

    fn item_line(&self, item: &Item, armed: Option<&str>, width: u32) -> Line {
        match item {
            Item::Heading(h) => line(vec![s(h.to_uppercase(), Role::Muted)]),
            Item::Note(text, role) => line(vec![s(format!("  {text}"), role.clone())]),
            Item::Container(id) => {
                let Some(c) = self.container(id) else { return line(Vec::new()) };
                let (mark, role) = if c.running() { ("●", Role::Good) } else if c.state == "paused" { ("‖", Role::Warn) } else { ("○", Role::Muted) };
                let mut left = vec![s(format!("{mark} "), role), s(fit(&c.name, 24), if c.running() { Role::Text } else { Role::Muted }), s(fit(&c.image, 22), Role::Muted)];
                match (c.health(), c.exit_code()) {
                    (Some("unhealthy"), _) => left.push(s(" unhealthy", Role::Bad)),
                    (Some(h), _) => left.push(s(format!(" {h}"), Role::Good)),
                    (None, Some(code)) if code != 0 => left.push(s(format!(" exited {code}"), Role::Bad)),
                    _ => {}
                }
                if let Some(port) = c.host_ports().first() {
                    left.push(s(format!(" :{port}"), Role::Accent));
                }
                let right = if armed == Some(c.id.as_str()) {
                    vec![s("x again removes it", Role::Warn)]
                } else if let Some(l) = self.load_of(c).filter(|_| c.running()) {
                    let frac = if l.mem_limit > 0 { l.mem as f32 / l.mem_limit as f32 } else { 0.0 };
                    let mem_role = if frac > 0.95 { Role::Bad } else if frac > 0.8 { Role::Warn } else { Role::Good };
                    vec![s(bar(l.cpu / 100.0, 6), Role::Accent), s(format!(" {:>5.1}%  ", l.cpu), Role::Muted), s(bar(frac, 6), mem_role), s(format!(" {:>8}", model::size(l.mem)), Role::Muted)]
                } else {
                    vec![s(c.status.clone(), Role::Muted)]
                };
                spread(left, right, width)
            }
            Item::Stack(name) => {
                let st = self.stacks.iter().find(|st| &st.name == name);
                let up = self.containers.iter().filter(|c| c.project.as_deref() == Some(name) && c.running()).count();
                let of = self.project.as_ref().filter(|p| &p.name == name).map(|p| p.services.len()).unwrap_or_else(|| self.containers.iter().filter(|c| c.project.as_deref() == Some(name)).count());
                let role = if up > 0 { Role::Good } else { Role::Muted };
                spread(vec![s(if up > 0 { "● " } else { "○ " }, role), s(name.clone(), Role::Title), s(format!("  {up} of {of} running"), Role::Muted)], vec![s(st.map(|st| st.status.clone()).unwrap_or_else(|| "not up".into()), Role::Muted)], width)
            }
            Item::Service { stack, service } => {
                let c = self.containers.iter().find(|c| c.project.as_deref() == Some(stack) && c.service.as_deref() == Some(service));
                let what = self.project.as_ref().and_then(|p| p.services.iter().find(|(n, _)| n == service)).map(|(_, w)| w.clone()).unwrap_or_default();
                let n = self.project.as_ref().and_then(|p| p.services.iter().position(|(s, _)| s == service)).unwrap_or(0);
                let (mark, mrole, state) = match c {
                    Some(c) if c.running() => ("●", Role::Good, c.status.clone()),
                    Some(c) => ("○", Role::Muted, c.status.clone()),
                    None => ("○", Role::Muted, "not started".into()),
                };
                spread(vec![s(format!("    {mark} "), mrole), s(fit(service, 14), SERVICE_ROLES[n % SERVICE_ROLES.len()].clone()), s(fit(&what, 28), Role::Muted)], vec![s(state, Role::Muted)], width)
            }
            Item::Image(id) => {
                let Some(i) = self.images.iter().find(|i| &i.id == id) else { return line(Vec::new()) };
                let users = self.containers.iter().filter(|c| c.image == i.reference() || c.image == i.repository).count();
                let right = if armed == Some(i.id.as_str()) {
                    vec![s("x again removes it", Role::Warn)]
                } else {
                    vec![s(format!("{:>8}", model::size(i.size)), Role::Muted), s(format!("  {}  ", if users == 1 { "1 container".into() } else { format!("{users} containers") }), Role::Muted), s(i.created.clone(), Role::Muted)]
                };
                let name = if i.dangling() { vec![s("<none>", Role::Muted), s("  dangling", Role::Warn)] } else { vec![s(i.repository.clone(), Role::Text), s(format!(":{}", i.tag), Role::Accent)] };
                let mut left = vec![s("  ", Role::Text)];
                left.extend(name);
                spread(left, right, width)
            }
            Item::Volume(name) => {
                let v = self.volumes.iter().find(|v| &v.name == name);
                let users = self.volume_users(name);
                let right = if armed == Some(name.as_str()) { vec![s("x again removes it", Role::Warn)] } else { vec![s(if users.is_empty() { "not in use".into() } else { users.join(", ") }, Role::Muted)] };
                spread(vec![s(format!("  {}", fit(name, 32)), Role::Text), s(v.map(|v| v.driver.clone()).unwrap_or_default(), Role::Muted)], right, width)
            }
            Item::Network(name) => {
                let n = self.networks.iter().find(|n| &n.name == name);
                let users: Vec<String> = self.details.iter().filter(|(_, d)| d.networks.iter().any(|(net, _, _)| net == name)).filter_map(|(id, _)| self.container(id).map(|c| c.name.clone())).collect();
                let built_in = n.is_some_and(|n| n.built_in());
                let right = if armed == Some(name.as_str()) { vec![s("x again removes it", Role::Warn)] } else { vec![s(if built_in { "built in".into() } else if users.is_empty() { String::new() } else { users.join(", ") }, Role::Muted)] };
                spread(vec![s(format!("  {}", fit(name, 32)), if built_in { Role::Muted } else { Role::Text }), s(n.map(|n| n.driver.clone()).unwrap_or_default(), Role::Muted)], right, width)
            }
        }
    }

    fn render_container(&self, id: &str, menu: bool, reveal: bool, armed: bool, width: u32) -> Page {
        let Some(c) = self.container(id) else { return Page::new().line([page::muted("This container isn't there any more.")]) };
        let state_role = if c.running() { Role::Good } else { Role::Muted };
        let mut p = Page::new().line([page::title(c.name.clone()), page::text("  "), Span { text: c.state.clone(), role: state_role }, page::muted(format!("  {}", c.status))]);
        let d = self.details.get(id);
        let mut sub = vec![page::muted(c.image.clone())];
        if let Some(d) = d {
            sub.push(page::muted(format!("  ·  {}  ·  {} restarts", d.command, d.restarts)));
        }
        if let (Some(stack), Some(svc)) = (&c.project, &c.service) {
            sub.push(page::muted(format!("  ·  compose {stack} / {svc}")));
        }
        p = p.line(sub).rule();
        if armed {
            p = p.line([page::warn("x again removes it")]);
        }
        let ports = c.host_ports();
        let mut focus_line = None;
        if !ports.is_empty() {
            p = p.line([page::muted("PORTS")]);
            focus_line = Some(p.len());
            for port in &ports {
                p = p.line([page::accent(format!("  http://localhost:{port}")), page::muted("   Enter opens it")]);
            }
        }
        if let Some(l) = self.loads.get(id).and_then(|s| s.back()) {
            p = p.blank().line([page::muted("LOAD")]).line([page::text(format!("  cpu {:.1}%   memory {}{}   net {} / {}", l.cpu, model::size(l.mem), if l.mem_limit > 0 { format!(" of {}", model::size(l.mem_limit)) } else { String::new() }, model::size(l.net_in), model::size(l.net_out)))]);
        }
        match d {
            None => p = p.blank().line([page::muted("Reading its details…")]),
            Some(d) => {
                if !d.mounts.is_empty() {
                    p = p.blank().line([page::muted("MOUNTS")]);
                    for (src, dst, kind, rw) in &d.mounts {
                        p = p.line([page::text(format!("  {src}")), page::muted(format!(" → {dst} · {kind}{}", if *rw { "" } else { ", read-only" }))]);
                    }
                }
                if !d.env.is_empty() {
                    p = p.blank().line([page::muted("ENVIRONMENT")]);
                    for (k, v) in &d.env {
                        let shown = if model::secret_name(k) && !reveal { vec![page::muted("••••••••   v shows")] } else { vec![page::text(v.clone())] };
                        let mut spans = vec![page::text(format!("  {k}")), page::muted(" = ")];
                        spans.extend(shown);
                        p = p.line(spans);
                    }
                }
                if !d.networks.is_empty() {
                    p = p.blank().line([page::muted("NETWORKS")]);
                    for (net, ip, aliases) in &d.networks {
                        p = p.line([page::text(format!("  {net}")), page::muted(format!(" {ip}{}", if aliases.is_empty() { String::new() } else { format!(" · aliases {aliases}") }))]);
                    }
                }
                if !d.health_log.is_empty() {
                    p = p.blank().line([page::muted("HEALTH, LAST CHECKS")]);
                    for h in &d.health_log {
                        p = p.line([Span { text: format!("  {h}"), role: if h.starts_with("ok") { Role::Good } else { Role::Bad } }]);
                    }
                }
            }
        }
        let _ = width;
        if let Some(i) = focus_line {
            p = p.focus(i);
        }
        if menu {
            let rows = MENU.iter().map(|(k, what)| vec![page::accent(format!("{k:<3}")), if *k == "x" { page::bad(*what) } else { page::text(*what) }]).collect::<Vec<_>>();
            p = p.popup(1, rows);
        }
        p.key("m", "menu").key("Enter", "open port").key("!", "shell").key("l", "logs").key("i", "JSON").key("v", "secrets").key("x x", "remove")
    }

    fn render_image(&self, id: &str, width: u32) -> Page {
        let Some(i) = self.images.iter().find(|i| i.id == id) else { return Page::new().line([page::muted("This image isn't there any more.")]) };
        let mut p = Page::new().line([page::title(i.reference()), page::muted(format!("  {}  ·  {}", model::size(i.size), i.created))]).line([page::muted(i.id.clone())]).rule();
        match self.history.get(id) {
            Some(layers) => {
                p = p.line([page::muted("LAYERS")]);
                let biggest = layers.iter().map(|l| l.size).max().unwrap_or(0);
                for l in layers.iter().rev() {
                    let role = if l.size == biggest && biggest > 0 { Role::Warn } else { Role::Muted };
                    p = p.line([Span { text: format!("  {:>9}  ", model::size(l.size)), role }, page::text(fit(&l.created_by, (width as usize).saturating_sub(14)).trim_end().to_string())]);
                }
            }
            None if self.current.is_some_and(|e| !e.caps().history) => p = p.line([page::muted("This engine doesn't show an image's layers.")]),
            None => p = p.line([page::muted("Reading its layers…")]),
        }
        let users: Vec<&Container> = self.containers.iter().filter(|c| c.image == i.reference() || c.image == i.repository).collect();
        p = p.blank().line([page::muted("USED BY")]);
        if users.is_empty() {
            p = p.line([page::muted("  nothing")]);
        }
        for c in users {
            p = p.line([if c.running() { page::good("  ● ") } else { page::muted("  ○ ") }, page::text(c.name.clone()), page::muted(format!("  {}", c.status))]);
        }
        p.key("R", "run").key("p", "pull")
    }

    #[allow(clippy::too_many_arguments)]
    fn render_logs(title: &str, sources: &[String], lines: &VecDeque<LogLine>, follow: bool, timestamps: bool, filter: &str, hidden: &HashSet<usize>, focus: usize, width: u32) -> Page {
        let mut p = Page::new().line(vec![
            page::title(title.to_string()),
            page::muted(format!("  {} stream{}", sources.len(), if sources.len() == 1 { "" } else { "s" })),
            if follow { page::good("   ● following") } else { page::muted("   paused -- F follows") },
        ]);
        if sources.len() > 1 {
            let mut legend = Vec::new();
            for (n, name) in sources.iter().enumerate() {
                let role = if hidden.contains(&n) { Role::Muted } else { SERVICE_ROLES[n % SERVICE_ROLES.len()].clone() };
                legend.push(Span { text: format!("{} {name}{}   ", n + 1, if hidden.contains(&n) { " (hidden)" } else { "" }), role });
            }
            p = p.line(legend);
        }
        if !filter.is_empty() {
            p = p.line([page::accent(format!("/ {filter}"))]);
        }
        p = p.rule();
        let shown: Vec<&LogLine> = lines.iter().filter(|l| !hidden.contains(&l.source) && State::matches(filter, &l.text)).collect();
        // The last screenful when following; the window around the cursor
        // otherwise.
        let window = 400usize;
        let start = if follow { shown.len().saturating_sub(window) } else { focus.saturating_sub(window / 2).min(shown.len().saturating_sub(window)) };
        let top = p.len();
        for l in shown.iter().skip(start).take(window) {
            let mut spans = Vec::new();
            if timestamps && !l.time.is_empty() {
                spans.push(page::muted(format!("{} ", l.time)));
            }
            if sources.len() > 1 {
                spans.push(Span { text: format!("{:<10}", fit(sources.get(l.source).map(String::as_str).unwrap_or(""), 9)), role: SERVICE_ROLES[l.source % SERVICE_ROLES.len()].clone() });
            }
            let role = if l.text.starts_with("-- ") { Role::Muted } else { log_role(&l.text) };
            spans.push(Span { text: fit(&l.text, (width as usize).saturating_sub(22)).trim_end().to_string(), role });
            p.lines.push(line(spans));
        }
        if shown.is_empty() {
            p = p.line([page::muted(if lines.is_empty() { "Waiting for its first lines…" } else { "Nothing matches -- / changes the filter." })]);
        } else {
            let at = if follow { shown.len() - 1 } else { focus.min(shown.len() - 1) };
            p = p.focus(top as u32 + (at - start.min(at)) as u32);
        }
        p.key("/", "filter").key("F", "follow").key("t", "times").key("1-9", "streams").key("o", "as a buffer")
    }

    fn render_performance(&self, by_memory: bool, focus: usize, width: u32) -> Page {
        let mut p = Page::new();
        for l in self.header(width) {
            p.lines.push(l);
        }
        p = p.line([page::muted(format!("sorted by {}", if by_memory { "memory" } else { "CPU" }))]).rule();
        let mut running: Vec<(&Container, Load, Vec<f32>)> = self
            .containers
            .iter()
            .filter(|c| c.running())
            .map(|c| {
                let series = self.loads.get(&c.id);
                (c, series.and_then(|s| s.back()).copied().unwrap_or_default(), series.map(|s| s.iter().map(|l| l.cpu).collect()).unwrap_or_default())
            })
            .collect();
        running.sort_by(|a, b| if by_memory { b.1.mem.cmp(&a.1.mem) } else { b.1.cpu.total_cmp(&a.1.cpu) });
        // The sparkline last: its glyphs can be wider than a cell.
        p = p.line([page::muted(format!("{:<24}{:<9}{:<30}{:<22}{:<22}the last minute", "container", "cpu", "memory", "net in / out", "disk r / w"))]);
        let first = p.len();
        for (c, l, cpu) in &running {
            let frac = if l.mem_limit > 0 { l.mem as f32 / l.mem_limit as f32 } else { 0.0 };
            let mem_role = if frac > 0.95 { Role::Bad } else if frac > 0.8 { Role::Warn } else { Role::Good };
            let tail: Vec<f32> = cpu.iter().rev().take(16).rev().copied().collect();
            p = p.line([
                page::text(fit(&c.name, 23)),
                page::text(" "),
                page::text(format!("{:>6.1}%  ", l.cpu)),
                Span { text: bar(frac, 8), role: mem_role },
                page::muted(format!(" {:<21}", if l.mem_limit > 0 { format!("{} / {}", model::size(l.mem), model::size(l.mem_limit)) } else { format!("{} · no limit", model::size(l.mem)) })),
                page::muted(format!("{:<22}", format!("{} / {}", model::size(l.net_in), model::size(l.net_out)))),
                page::muted(format!("{:<22}", format!("{} / {}", model::size(l.disk_read), model::size(l.disk_write)))),
                page::accent(sparkline(&tail)),
            ]);
        }
        if running.is_empty() {
            p = p.line([page::muted("Nothing's running.")]);
        } else {
            p = p.focus(first + (focus.min(running.len() - 1)) as u32);
        }
        if !self.disk.is_empty() {
            let reclaim: u64 = self.disk.iter().map(|d| d.reclaimable).sum();
            p = p.blank().line([page::muted("DISK"), page::muted(format!("   {} reclaimable", model::size(reclaim)))]).rule();
            for d in &self.disk {
                p = p.line([page::text(format!("{:<16}", d.kind)), page::text(format!("{:>10}", model::size(d.size))), page::muted(format!("   {} · {} active · {} reclaimable", d.count, d.active, model::size(d.reclaimable)))]);
            }
        }
        p.key("o", "sort")
    }
}

/// The container menu: key, and what it does.
const MENU: [(&str, &str); 9] = [("s", "Start or stop"), ("R", "Restart"), ("p", "Pause"), ("!", "Shell"), ("l", "Logs"), ("o", "Open its port"), ("i", "Inspect (JSON)"), ("y", "Copy its id"), ("x", "Remove (x x)")];

/// Turns a page's kind back into what to open: `logs:<id>=<name>,...`.
pub fn log_targets(kind: &str) -> Option<Vec<(String, String)>> {
    let rest = kind.strip_prefix("logs:")?;
    Some(rest.split(',').filter_map(|t| t.split_once('=').map(|(id, n)| (id.to_string(), n.to_string()))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(p: &Page) -> String {
        p.lines.iter().map(|l| if l.rule { "----".to_string() } else { l.spans.iter().map(|s| s.text.as_str()).collect::<String>() }).collect::<Vec<_>>().join("\n")
    }

    const PS: &str = r#"{"ID":"8a1f3c2d9e","Image":"nginx:1.27","Labels":"com.docker.compose.project=shop,com.docker.compose.service=web","Names":"shop-web-1","Ports":"0.0.0.0:8080->80/tcp","State":"running","Status":"Up 2 hours (healthy)"}
{"ID":"77aa","Image":"shop-api:dev","Labels":"","Names":"migrate","Ports":"","State":"exited","Status":"Exited (1) 12 minutes ago"}"#;

    /// Docker up, its containers listed, a Main page open and shown.
    fn ready() -> (State, u64) {
        let mut st = State::default();
        let probes = st.activate(None);
        assert_eq!(probes.len(), 3, "every engine asked");
        st.started(1, Job::Probe(Engine::Docker));
        st.output(1, false, "29.7.2");
        let effects = st.exited(1, Some(0));
        assert!(effects.iter().any(|e| matches!(e, Effect::Spawn { job: Job::Containers, engine: Engine::Docker, .. })), "{effects:?}");
        st.unspawnable(Job::Probe(Engine::Podman));
        st.unspawnable(Job::Probe(Engine::Wslc));
        st.started(10, Job::Containers);
        for l in PS.lines() {
            st.output(10, false, l);
        }
        let effects = st.exited(10, Some(0));
        assert!(effects.contains(&Effect::Status(Some("docker · 1 running".into()))), "{effects:?}");
        st.opened(100, "main", None);
        st.shown(100, true);
        (st, 100)
    }

    #[test]
    fn the_main_page_groups_by_stack_and_says_state_inline() {
        let (st, page) = ready();
        let t = text(&st.render(page, 120));
        assert!(t.contains("[docker 29.7.2]"), "{t}");
        assert!(t.contains("1 running · 1 stopped"), "{t}");
        assert!(t.contains("[1 Containers 2]"), "{t}");
        assert!(t.contains("SHOP · COMPOSE") && t.contains("OTHER"), "{t}");
        assert!(t.contains("shop-web-1") && t.contains("healthy") && t.contains(":8080"), "{t}");
        assert!(t.contains("exited 1"), "{t}");
        assert_eq!(st.render(page, 120).focus, Some(st.render(page, 120).lines.iter().position(|l| l.spans.iter().any(|s| s.text.contains("shop-web-1"))).unwrap() as u32));
    }

    #[test]
    fn stats_stream_in_and_draw_bars_and_sparklines() {
        let (mut st, page) = ready();
        let effects = st.stats_wanted();
        let Some(Effect::Spawn { args, .. }) = effects.first() else { panic!("{effects:?}") };
        assert_eq!(args, &["stats", "--format", "json"]);
        st.started(20, Job::Stats);
        for cpu in ["1.0%", "4.0%", "8.1%"] {
            let refreshed = st.output(20, false, &format!("\u{1b}[2J\u{1b}[H{{\"ID\":\"8a1f3c2d9e\",\"CPUPerc\":\"{cpu}\",\"MemUsage\":\"31MiB / 1GiB\",\"NetIO\":\"1kB / 2kB\",\"BlockIO\":\"0B / 0B\",\"PIDs\":\"3\"}}\u{1b}[K"));
            assert_eq!(refreshed, [Effect::Refresh(page)]);
        }
        let t = text(&st.render(page, 120));
        assert!(t.contains("8.1%") && t.contains("31 MiB"), "{t}");
        assert!(t.contains("the last minute"), "the focused row's sparklines: {t}");
        // Hidden: the stream stops.
        assert_eq!(st.shown(page, false), [Effect::Kill(20)]);
    }

    #[test]
    fn keys_move_switch_tabs_and_act_on_containers() {
        let (mut st, page) = ready();
        let e = st.key(page, Key::Char('j'));
        assert_eq!(e, [Effect::Refresh(page)]);
        // migrate is focused now: s starts it.
        let e = st.key(page, Key::Char('s'));
        assert!(matches!(&e[0], Effect::Spawn { job: Job::Action { what, .. }, args, .. } if what == "start migrate" && args == &["start", "77aa"]), "{e:?}");
        // x twice removes.
        let e = st.key(page, Key::Char('x'));
        assert!(matches!(&e[0], Effect::Notify(t) if t.starts_with("x again removes migrate")));
        assert!(text(&st.render(page, 120)).contains("x again removes it"));
        let e = st.key(page, Key::Char('x'));
        assert!(matches!(&e[0], Effect::Spawn { args, .. } if args == &["rm", "77aa"]), "{e:?}");
        // Back up to the web container: its port opens, its logs open.
        st.key(page, Key::Char('k'));
        assert_eq!(st.key(page, Key::Char('o'))[0], Effect::OpenUrl("http://localhost:8080".into()));
        assert!(matches!(&st.key(page, Key::Char('l'))[0], Effect::OpenPage { kind, .. } if kind == "logs:8a1f3c2d9e=shop-web-1"));
        assert!(matches!(&st.key(page, Key::Char('!'))[0], Effect::Terminal { args, .. } if args[..3] == ["exec", "-it", "8a1f3c2d9e"]));
        // The menu: m opens it, a key from it acts, and it closes.
        st.key(page, Key::Char('m'));
        let p = st.render(page, 120);
        assert!(p.popup.as_ref().is_some_and(|m| m.rows.len() == MENU.len()), "{:?}", p.popup);
        let e = st.key(page, Key::Char('R'));
        assert!(matches!(&e[0], Effect::Spawn { args, .. } if args == &["restart", "8a1f3c2d9e"]), "{e:?}");
        assert!(st.render(page, 120).popup.is_none());
        // Tabs.
        st.key(page, Key::Char('3'));
        assert!(text(&st.render(page, 120)).contains("[3 Images]"));
        st.key(page, Key::Char(']'));
        assert!(text(&st.render(page, 120)).contains("[4 Volumes]"));
    }

    #[test]
    fn a_filter_keeps_to_what_matches() {
        let (mut st, page) = ready();
        let e = st.key(page, Key::Char('/'));
        assert!(matches!(&e[0], Effect::Prompt { what: Asking::Filter(p), .. } if *p == page));
        st.prompted(Asking::Filter(page), Some("migrate".into()));
        let t = text(&st.render(page, 120));
        assert!(t.contains("migrate") && !t.contains("shop-web-1") && t.contains("/ migrate"), "{t}");
        st.prompted(Asking::Filter(page), Some("nothing-like-it".into()));
        assert!(text(&st.render(page, 120)).contains("Nothing matches"));
    }

    /// Docker Desktop closed: the page says so; Enter starts it.
    #[test]
    fn an_engine_thats_down_says_so_and_starts() {
        let mut st = State::default();
        st.activate(None);
        st.started(1, Job::Probe(Engine::Docker));
        st.output(1, true, "failed to connect to the docker API at npipe:////./pipe/dockerDesktopLinuxEngine");
        st.exited(1, Some(1));
        st.unspawnable(Job::Probe(Engine::Podman));
        st.unspawnable(Job::Probe(Engine::Wslc));
        st.opened(100, "main", None);
        let t = text(&st.render(100, 120));
        assert!(t.contains("Docker Desktop isn't running.") && t.contains("start Docker Desktop"), "{t}");
        let e = st.key(100, Key::Enter);
        assert!(e.iter().any(|e| matches!(e, Effect::Spawn { job: Job::Start(Engine::Docker), args, .. } if args == &["desktop", "start"])), "{e:?}");
    }

    #[test]
    fn compose_has_the_project_first_and_runs_its_verbs() {
        let (mut st, page) = ready();
        st.started(30, Job::Stacks);
        st.output(30, false, r#"[{"Name":"shop","Status":"running(1)","ConfigFiles":"C:\\code\\shop\\compose.yaml"},{"Name":"monitoring","Status":"running(2)","ConfigFiles":"x"}]"#);
        st.exited(30, Some(0));
        st.started(31, Job::ProjectConfig("compose.yaml".into()));
        st.output(31, false, r#"{"name":"shop","services":{"web":{"image":"nginx:1.27"},"api":{"build":{"context":"./api"}}}}"#);
        st.exited(31, Some(0));
        st.key(page, Key::Char('2'));
        let t = text(&st.render(page, 120));
        assert!(t.contains("THIS PROJECT · SHOP") && t.contains("OTHER STACKS") && t.contains("monitoring"), "{t}");
        assert!(t.contains("build ./api") && t.contains("not started"), "{t}");
        let e = st.key(page, Key::Char('u'));
        assert!(matches!(&e[0], Effect::Spawn { args, cwd, job: Job::Action { stack: true, .. }, .. } if args == &["compose", "-f", "compose.yaml", "up", "-d"] && cwd.as_deref() == Some("/project")), "{e:?}");
        st.started(32, Job::Action { what: "compose up shop".into(), stack: true });
        st.output(32, true, " Container shop-web-1  Started");
        assert!(text(&st.render(page, 120)).contains("Container shop-web-1  Started"));
        // The other stack, by its name.
        st.key(page, Key::Char('G'));
        let e = st.key(page, Key::Char('d'));
        assert!(matches!(&e[0], Effect::Spawn { args, .. } if args == &["compose", "-p", "monitoring", "down"]), "{e:?}");
        let e = st.key(page, Key::Char('p'));
        assert!(matches!(&e[0], Effect::Spawn { args, cwd: None, .. } if args == &["compose", "-f", "x", "pull"]), "{e:?}");
    }

    #[test]
    fn logs_merge_streams_follow_and_filter() {
        let (mut st, _) = ready();
        st.started(20, Job::Stats);
        let effects = st.open_logs(200, "shop · logs", &[("a".into(), "web".into()), ("b".into(), "api".into())]);
        assert_eq!(effects.len(), 2);
        st.shown(200, true);
        st.started(40, Job::Logs { page: 200, id: "a".into(), name: "web".into() });
        st.started(41, Job::Logs { page: 200, id: "b".into(), name: "api".into() });
        st.output(41, false, "2026-10-24T14:32:04.000000000Z ERROR payment: connection refused");
        st.output(40, false, "2026-10-24T14:31:58.000000000Z GET / 200");
        let p = st.render(200, 120);
        let t = text(&p);
        let web = t.find("GET / 200").unwrap();
        let api = t.find("ERROR payment").unwrap();
        assert!(web < api, "merged by time: {t}");
        assert!(p.lines.iter().any(|l| l.spans.iter().any(|s| s.text.contains("ERROR payment") && s.role == Role::Bad)));
        st.key(200, Key::Char('2'));
        assert!(!text(&st.render(200, 120)).contains("ERROR payment"), "api hidden");
        st.key(200, Key::Char('2'));
        st.prompted(Asking::Filter(200), Some("payment".into()));
        let t = text(&st.render(200, 120));
        assert!(t.contains("ERROR payment") && !t.contains("GET / 200"), "{t}");
        // Hidden, its streams stop; seen again, they start over.
        assert_eq!(st.shown(200, false), [Effect::Kill(40), Effect::Kill(41)]);
        let again = st.shown(200, true);
        assert_eq!(again.iter().filter(|e| matches!(e, Effect::Spawn { job: Job::Logs { page: 200, .. }, .. })).count(), 2, "{again:?}");
        assert!(text(&st.render(200, 120)).contains("Waiting for its first lines"));
        st.started(40, Job::Logs { page: 200, id: "a".into(), name: "web".into() });
        st.started(41, Job::Logs { page: 200, id: "b".into(), name: "api".into() });
        // Closed: its streams stop.
        assert_eq!(st.closed(200)[..2], [Effect::Kill(40), Effect::Kill(41)]);
    }

    #[test]
    fn a_containers_page_reads_inspect_and_masks_secrets() {
        let (mut st, _) = ready();
        let effects = st.opened(300, "container", Some("8a1f3c2d9e".into()));
        assert!(matches!(&effects[0], Effect::Spawn { args, .. } if args == &["container", "inspect", "8a1f3c2d9e"]));
        st.started(50, Job::Inspect("8a1f3c2d9e".into()));
        st.output(50, false, r#"[{"Config":{"Cmd":["nginx"],"Env":["API_TOKEN=abc","MODE=dev"]},"RestartCount":0,"State":{},"Mounts":[{"Type":"bind","Source":"C:\\site","Destination":"/usr/share/nginx/html","RW":false}],"NetworkSettings":{"Networks":{"shop_default":{"IPAddress":"172.18.0.2","Aliases":[]}}}}]"#);
        st.exited(50, Some(0));
        let t = text(&st.render(300, 120));
        assert!(t.contains("http://localhost:8080") && t.contains("read-only") && t.contains("shop_default 172.18.0.2"), "{t}");
        assert!(t.contains("API_TOKEN = ••••••••") && t.contains("MODE = dev"), "{t}");
        st.key(300, Key::Char('v'));
        assert!(text(&st.render(300, 120)).contains("API_TOKEN = abc"));
        assert_eq!(st.key(300, Key::Enter)[0], Effect::OpenUrl("http://localhost:8080".into()));
    }

    #[test]
    fn wslc_has_no_compose_volumes_or_networks_and_says_so() {
        let mut st = State::default();
        st.activate(Some(Engine::Wslc));
        st.started(1, Job::Probe(Engine::Wslc));
        st.output(1, false, "wslc 2.9.4");
        st.exited(1, Some(0));
        st.unspawnable(Job::Probe(Engine::Docker));
        st.unspawnable(Job::Probe(Engine::Podman));
        assert_eq!(st.current, Some(Engine::Wslc));
        st.opened(100, "main", None);
        st.key(100, Key::Char('2'));
        assert!(text(&st.render(100, 120)).contains("wslc has no Compose yet."));
        st.key(100, Key::Char('4'));
        assert!(text(&st.render(100, 120)).contains("wslc has no volumes of its own yet."));
    }

    /// Pages come back with the session: restored before the engine is
    /// known, they ask for what they show once it is.
    #[test]
    fn pages_are_saved_and_restored() {
        let (mut st, page) = ready();
        st.key(page, Key::Char('3'));
        assert_eq!(st.save(page).as_deref(), Some("2"));
        st.open_logs(200, "web · logs", &[("a".into(), "web".into())]);
        assert_eq!(st.save(200).as_deref(), Some("a=web"));

        let mut st = State::default();
        st.activate(None);
        assert_eq!(st.restore(1, "main", "2"), Some(Vec::new()));
        assert!(matches!(st.views.get(&1), Some(View::Main { tab: Tab::Images, .. })));
        assert_eq!(st.restore(2, "container", "8a1f3c2d9e"), Some(Vec::new()), "no engine yet: nothing asked");
        assert_eq!(st.restore(3, "logs", "a=web"), Some(Vec::new()));
        st.shown(3, true);
        assert_eq!(st.restore(4, "nonsense", ""), None);
        st.started(9, Job::Probe(Engine::Docker));
        st.output(9, false, "29.7.2");
        st.unspawnable(Job::Probe(Engine::Podman));
        let effects = st.unspawnable(Job::Probe(Engine::Wslc));
        let effects = [effects, st.exited(9, Some(0))].concat();
        assert!(effects.iter().any(|e| matches!(e, Effect::Spawn { job: Job::Inspect(id), .. } if id == "8a1f3c2d9e")), "{effects:?}");
        assert!(effects.iter().any(|e| matches!(e, Effect::Spawn { job: Job::Logs { page: 3, .. }, .. })), "{effects:?}");
    }

    #[test]
    fn inspect_opens_as_json() {
        let (mut st, page) = ready();
        let e = st.key(page, Key::Char('i'));
        let Some(Effect::Spawn { job: job @ Job::Json(_), .. }) = e.first().cloned() else { panic!("{e:?}") };
        st.started(60, job);
        st.output(60, false, "[{}]");
        assert_eq!(st.exited(60, Some(0)), [Effect::OpenText { title: "shop-web-1 · inspect".into(), text: "[{}]".into(), language: "json".into() }]);
    }

    /// An engine pinned in the settings is used, and switched to when it
    /// changes.
    #[test]
    fn the_engine_setting_pins_one() {
        let (mut st, _) = ready();
        st.engines.insert(1, (Engine::Podman, Up::Running { version: "5.6".into() }));
        let effects = st.pin(Some(Engine::Podman));
        assert_eq!(st.current, Some(Engine::Podman));
        assert!(st.containers.is_empty(), "docker's list dropped");
        assert!(effects.iter().any(|e| matches!(e, Effect::Spawn { engine: Engine::Podman, job: Job::Containers, .. })), "{effects:?}");
        assert!(effects.contains(&Effect::Status(Some("podman · 0 running".into()))));
        assert!(st.pin(Some(Engine::Podman)).is_empty());
    }

    #[test]
    fn sparkline_and_log_targets() {
        assert_eq!(sparkline(&[0.0, 50.0, 100.0]), "▁▅█");
        assert_eq!(sparkline_between(&[100.0, 100.0, 100.0]), "▄▄▄", "flat");
        assert_eq!(sparkline_between(&[1000.0, 1050.0, 1100.0]), "▁▅█", "its own range");
        assert_eq!(log_targets("logs:a=web,b=api"), Some(vec![("a".into(), "web".into()), ("b".into(), "api".into())]));
    }
}
