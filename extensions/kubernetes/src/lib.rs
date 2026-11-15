//! Kubernetes in Fenix, driving `kubectl`: the context and namespace you
//! choose for this project, a page of workloads, pods, services,
//! ingresses and Secrets' names, live logs, a shell in a pod, port-
//! forwards as background tasks, an object's YAML and `describe` in a
//! read-only buffer, and changes -- delete a pod, restart, scale -- each
//! asked about first. Coding agents get read-only tools. kubeconfig is
//! never changed.
//!
//! The parts that decide things are plain code, tested natively:
//! `kubectl` (every command line, and what's protected), `model` (what
//! kubectl answered), `secret` (a Secret without its values), `agent`
//! (the agents' tools), `page` and `logs` (the pages and their keys). This
//! file meets Fenix: processes, pickers, prompts, timers, tasks,
//! terminals.
//!
//! Every answer that comes back is checked against what's being waited
//! for now: lists, the server's version, namespaces, YAML and describe
//! carry the selection's generation, kubeconfig's reads a sequence
//! number, a log stream its process -- a result for an earlier request is
//! dropped, never shown as the newer one's.

pub mod agent;
pub mod kubectl;
pub mod logs;
pub mod model;
pub mod page;
pub mod secret;

#[cfg(test)]
mod driven;

use std::collections::{HashMap, VecDeque};

#[cfg(not(test))]
pub(crate) use fenix_extension_api as api;
#[cfg(test)]
pub(crate) use fenix_test_kit::api;

use api::chrono::Utc;
use api::{page as pages, Event, Extension, Key, Page, PickerItem};

use agent::{Ask, Tail};
use kubectl::{Kind, Ns, Protection, Selection, Target};
use logs::{LogAction, LogView, Stream};
use model::{Kubeconfig, More};
use page::{Action, Cluster, Forward, Header, Listing, MainView, Mutation};

const MAIN: &str = "main";
const LOGS: &str = "logs";

/// The most a list may be raised to with `+`, of one kind.
pub const MOST_ITEMS: usize = 5000;
/// The watchdog's beat, and how many beats a kubectl call that isn't a
/// stream may take before it's stopped: 20 to 30 seconds.
pub const WATCH_MS: u32 = 10_000;
pub const WATCH_BEATS: u32 = 3;
/// Agents' calls: at most this many at once (one per tool), and in a
/// minute.
pub const AGENT_AT_ONCE: usize = 3;
pub const AGENT_PER_MINUTE: usize = 30;

/// What a process the extension started is for.
#[derive(Debug, Clone, PartialEq)]
enum Job {
    ClientVersion,
    Config { seq: u64 },
    ServerVersion { generation: u64 },
    List { kind: Kind, generation: u64 },
    Namespaces { generation: u64 },
    Yaml { target: Target, context: String, generation: u64 },
    Describe { target: Target, context: String, generation: u64 },
    Logs { page: u64 },
    Change { what: String },
    /// A coding agent's call.
    Agent { call: u64 },
}

impl Job {
    /// What it was waiting on, for the watchdog's words.
    fn waiting(&self) -> String {
        match self {
            Job::ClientVersion => "kubectl's version".into(),
            Job::Config { .. } => "kubeconfig".into(),
            Job::ServerVersion { .. } => "the cluster's version".into(),
            Job::List { kind, .. } => format!("the {}", kind.resource()),
            Job::Namespaces { .. } => "the namespaces".into(),
            Job::Yaml { target, .. } => format!("{target}'s YAML"),
            Job::Describe { target, .. } => format!("describe {target}"),
            Job::Logs { .. } => "a log".into(),
            Job::Change { what } => what.clone(),
            Job::Agent { .. } => "an agent's call".into(),
        }
    }
}

#[derive(Debug)]
struct Running {
    job: Job,
    out: Vec<String>,
    err: Vec<String>,
    /// Output cut by hand past its cap: there's more.
    cut: bool,
    /// Watchdog beats seen.
    beats: u32,
}

/// What a prompt was for.
#[derive(Debug, Clone, PartialEq)]
enum Asking {
    Filter,
    LogFilter(u64),
    Since(u64),
    Replicas(Target, Option<u32>),
    /// A change in a protected context: the context it was asked in.
    Typed(Mutation, String),
    Ports(Target),
    Namespace,
}

/// What a picker was for.
#[derive(Debug, Clone, PartialEq)]
enum Picking {
    Context,
    /// The namespaces, listed in this generation.
    Namespace(u64),
    LogContainer(Target),
    ShellContainer(Target),
    PageContainer(u64),
    StopForward,
}

/// Whether the cluster answers.
#[derive(Debug, Clone, PartialEq, Default)]
enum Server {
    #[default]
    Unknown,
    Checking,
    Up(String),
    Down(String),
}

/// An agent's call, while its process runs.
#[derive(Debug)]
struct AgentCall {
    service: String,
    ask: Ask,
    context: String,
    namespace: String,
    /// A log's tail, as it comes.
    tail: Option<Tail>,
}

#[derive(Default)]
pub struct Kube {
    program: String,
    kubeconfig: Option<Kubeconfig>,
    /// Why kubeconfig couldn't be read.
    config_error: Option<String>,
    config_seq: u64,
    /// When kubeconfig was last read, as the clock says (12:04).
    read_at: Option<String>,
    client: Option<String>,
    /// Why kubectl can't run at all.
    missing: Option<String>,
    server: Server,
    selection: Selection,
    /// What `kubernetes.context` and `kubernetes.namespace` suggest --
    /// yours or the project's, which Fenix doesn't tell apart -- used only
    /// once you take it (`u`).
    suggestion: Option<Selection>,
    /// Bumped when what's listed changes: answers to the old one go.
    generation: u64,
    cluster: Cluster,
    main: Option<(u64, MainView)>,
    main_shown: bool,
    logs: HashMap<u64, LogView>,
    jobs: HashMap<u64, Running>,
    forwards: Vec<Forward>,
    prompts: HashMap<u64, Asking>,
    pickers: HashMap<u64, Picking>,
    timer: Option<u64>,
    watchdog: Option<u64>,
    /// A listing failed: the timer asks nothing until you do (`r`).
    failed: bool,
    /// The page asked for before kubeconfig was read.
    refresh_waiting: bool,
    /// A change asked about: it, and the context it was asked in.
    asked: Option<(Mutation, String)>,
    agent_waiting: Vec<(u64, String, Ask)>,
    agent_calls: HashMap<u64, AgentCall>,
    /// When agents' calls were taken, in seconds, the last minute's.
    agent_times: VecDeque<i64>,
    /// The page's height, for the log pages' keys.
    heights: HashMap<u64, usize>,
}

fn setting(key: &str) -> Option<String> {
    api::setting(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn list_setting(key: &str) -> Vec<String> {
    setting(key).map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
}

/// `kubernetes.max_items`: how many of a kind are listed (50 to 5,000).
fn max_items() -> usize {
    setting("kubernetes.max_items").and_then(|v| v.parse().ok()).unwrap_or(500).clamp(50, MOST_ITEMS)
}

fn now() -> api::chrono::DateTime<Utc> {
    api::clock::now().with_timezone(&Utc)
}

/// What the settings suggest, if they suggest anything.
fn suggested() -> Option<Selection> {
    let context = setting("kubernetes.context").and_then(|c| kubectl::context(&c).ok());
    let namespace = match setting("kubernetes.namespace").as_deref() {
        None => Ns::Default,
        Some("*") | Some("all") => Ns::All,
        Some(n) => kubectl::label(n).map(Ns::Named).unwrap_or(Ns::Default),
    };
    (context.is_some() || namespace != Ns::Default).then_some(Selection { context, namespace })
}

impl Kube {
    // -- Where it works ----------------------------------------------------

    /// The context in use, by name: the one chosen here, else kubeconfig's
    /// current one as last read. None: nothing runs against a cluster.
    fn context(&self) -> Option<String> {
        self.selection.context.clone().or_else(|| self.kubeconfig.as_ref().and_then(|k| k.current.clone()))
    }

    /// The context's cluster and server, as kubeconfig says.
    fn cluster_of(&self, context: &str) -> (Option<String>, Option<String>) {
        match self.kubeconfig.as_ref().and_then(|k| k.context(context)) {
            Some(c) => (Some(c.cluster.clone()), c.server.clone()),
            None => (None, None),
        }
    }

    /// What decides which cluster calls go to: the context, its cluster and
    /// its server. A change of any is a new place.
    fn identity(&self) -> (Option<String>, Option<String>, Option<String>) {
        let context = self.context();
        let (cluster, server) = context.as_deref().map(|c| self.cluster_of(c)).unwrap_or_default();
        (context, cluster, server)
    }

    /// The namespace in use; none: all of them.
    fn namespace(&self) -> Option<String> {
        match &self.selection.namespace {
            Ns::Named(n) => Some(n.clone()),
            Ns::All => None,
            // kubeconfig's namespace was checked as it was read.
            Ns::Default => Some(self.context().and_then(|c| self.kubeconfig.as_ref().and_then(|k| k.context(&c)).and_then(|c| c.namespace.clone())).unwrap_or_else(|| "default".into())),
        }
    }

    fn protected(&self, context: &str) -> bool {
        self.verdict(context).protected()
    }

    /// Whether `context` is protected, and why.
    fn verdict(&self, context: &str) -> kubectl::Verdict {
        let (cluster, server) = self.cluster_of(context);
        Protection { extra: list_setting("kubernetes.protected"), exempt: list_setting("kubernetes.unprotected") }.verdict(context, cluster.as_deref(), server.as_deref())
    }

    /// The context, or a refusal said: nothing runs without one.
    fn need_context(&self, what: &str) -> Option<String> {
        let c = self.context();
        if c.is_none() {
            api::notify(&format!("No context to {what} in: kubeconfig has no current context{}. c chooses one; nothing ran.", if self.config_error.is_some() { " (it couldn't be read)" } else { "" }));
        }
        c
    }

    fn header(&self) -> Header {
        let context = self.context();
        let versions = match (&self.client, &self.server) {
            (Some(c), Server::Up(s)) => format!("kubectl {c} · server {s}"),
            (Some(c), _) => format!("kubectl {c}"),
            (None, _) => String::new(),
        };
        let no_contexts = self.kubeconfig.as_ref().is_some_and(|k| k.contexts.is_empty());
        let problem = if let Some(why) = &self.missing {
            Some((format!("kubectl can't run: {why}"), Some("install kubectl, or set kubernetes.kubectl to where it is")))
        } else if self.kubeconfig.is_none() {
            Some(("Reading kubeconfig…".to_string(), None))
        } else if let (Some(why), None) = (&self.config_error, &context) {
            Some((format!("kubectl couldn't read kubeconfig: {why}"), Some("fix kubeconfig (KUBECONFIG says where it is), then r")))
        } else if no_contexts && context.is_none() {
            Some(("kubeconfig has no contexts: there's no ~/.kube/config, or KUBECONFIG names none.".to_string(), Some("set one up with your cluster's tools (kind create cluster, aws eks update-kubeconfig, gcloud container clusters get-credentials)")))
        } else if context.is_none() {
            Some(("No context: kubeconfig has no current context, and nothing runs without one.".to_string(), Some("c chooses one, for this project")))
        } else if let Server::Down(why) = &self.server {
            Some((why.clone(), model::hint(why)))
        } else {
            None
        };
        let origin = if self.selection.context.is_some() {
            "chosen here".to_string()
        } else {
            format!("kubeconfig's current, read {}", self.read_at.as_deref().unwrap_or("?"))
        };
        let suggestion = self.suggestion.as_ref().filter(|s| {
            let taken = Selection { context: s.context.clone().or_else(|| self.selection.context.clone()), namespace: s.namespace.clone() };
            taken != self.selection
        });
        let suggestion = suggestion.map(|s| {
            let ns = match &s.namespace {
                Ns::Default => "its own namespace".to_string(),
                Ns::All => "all namespaces".to_string(),
                Ns::Named(n) => format!("namespace {n}"),
            };
            format!("{}{ns} (from kubernetes.context and .namespace: your settings or this project's)", s.context.as_ref().map(|c| format!("context {c}, ")).unwrap_or_default())
        });
        Header {
            protected: context.as_deref().is_some_and(|c| self.protected(c)),
            origin,
            suggestion,
            context,
            namespace: self.namespace().unwrap_or_else(|| "all namespaces".into()),
            versions,
            problem,
            all_namespaces: self.selection.namespace == Ns::All,
        }
    }

    // -- Processes -----------------------------------------------------------

    fn spawn(&mut self, job: Job, args: Vec<String>) -> Option<u64> {
        let cmd = api::Command::new(&self.program).args(args);
        match api::host::spawn(&cmd) {
            Ok(p) => {
                // Nothing reads from you: a credential plugin that asks is
                // told there's nobody there, rather than waiting forever.
                api::process::close_stdin(p);
                let stream = matches!(job, Job::Logs { .. });
                self.jobs.insert(p, Running { job, out: Vec::new(), err: Vec::new(), cut: false, beats: 0 });
                if !stream && self.watchdog.is_none() {
                    self.watchdog = Some(api::host::set_timer(WATCH_MS, true));
                }
                Some(p)
            }
            Err(why) => {
                api::warn!("{}: {why}", self.program);
                self.missing = Some(why.clone());
                self.doctor();
                self.finish(job, None, Vec::new(), vec![format!("error: {why}")], false);
                None
            }
        }
    }

    fn refresh_main(&self) {
        if let Some((p, _)) = &self.main {
            pages::refresh(*p);
        }
    }

    /// The watchdog's beat: a call that isn't a stream and has gone on too
    /// long is stopped, and said so -- most often a login plugin waiting
    /// on a browser or a device code, which `--request-timeout` doesn't
    /// bound.
    fn beat(&mut self) {
        let mut late = Vec::new();
        for (p, run) in self.jobs.iter_mut() {
            if matches!(run.job, Job::Logs { .. }) {
                continue;
            }
            run.beats += 1;
            if run.beats >= WATCH_BEATS {
                late.push(*p);
            }
        }
        for p in late {
            api::process::kill(p);
            if let Some(run) = self.jobs.remove(&p) {
                let why = format!("error: kubectl gave {} no answer in {} s -- a login plugin may be waiting for you (a browser, a device code). It was stopped; r asks again.", run.job.waiting(), WATCH_BEATS * WATCH_MS / 1000);
                self.finish(run.job, None, Vec::new(), vec![why], false);
            }
        }
        if !self.jobs.values().any(|r| !matches!(r.job, Job::Logs { .. })) {
            if let Some(t) = self.watchdog.take() {
                api::host::cancel_timer(t);
            }
        }
    }

    fn ended(&mut self, process: u64, code: Option<i32>) {
        let Some(run) = self.jobs.remove(&process) else { return };
        self.finish(run.job, code, run.out, run.err, run.cut);
    }

    /// What kubectl's answer means, by what it was for.
    fn finish(&mut self, job: Job, code: Option<i32>, out: Vec<String>, err: Vec<String>, cut: bool) {
        let ok = code == Some(0);
        let why = || model::failure(&err, &out);
        match job {
            Job::ClientVersion => {
                self.client = if ok { model::versions(&out.join("\n")).0 } else { None };
                if !ok && self.missing.is_none() {
                    self.missing = Some(why());
                }
                self.doctor();
                self.refresh_main();
            }
            Job::Config { seq } => {
                if seq != self.config_seq {
                    return; // An earlier read's: a newer one is coming.
                }
                let before = self.kubeconfig.is_some().then(|| self.identity());
                let read = if ok { Kubeconfig::read(&out.join("\n")) } else { Err(why()) };
                match read {
                    Ok(k) => {
                        if !k.refused.is_empty() {
                            api::notify(&format!("kubeconfig has {} context(s) whose names hold a tab or a newline: left out, since kubectl can't be given them as one name", k.refused.len()));
                        }
                        self.kubeconfig = Some(k);
                        self.config_error = None;
                    }
                    Err(e) => {
                        api::warn!("kubectl config view: {e}");
                        self.kubeconfig = Some(Kubeconfig::default());
                        self.config_error = Some(e);
                    }
                }
                self.read_at = Some(api::clock::now().format("%H:%M").to_string());
                // Another context, cluster or server than before: what's
                // listed is another cluster's.
                if before.is_some_and(|b| b != self.identity()) {
                    self.reset();
                    self.refresh_waiting = self.main.is_some();
                }
                self.doctor();
                if std::mem::take(&mut self.refresh_waiting) {
                    self.refresh(true);
                }
                for (call, service, ask) in std::mem::take(&mut self.agent_waiting) {
                    if let Some(answer) = self.agent_start(call, &service, ask) {
                        api::answer(call, answer);
                    }
                }
                self.refresh_main();
            }
            Job::ServerVersion { generation } => {
                if generation != self.generation {
                    return;
                }
                self.server = match (ok, model::versions(&out.join("\n")).1) {
                    (true, Some(v)) => Server::Up(v),
                    (true, None) => Server::Up("?".into()),
                    _ => Server::Down(why()),
                };
                if matches!(self.server, Server::Down(_)) {
                    self.failed = true;
                }
                self.doctor();
                self.refresh_main();
            }
            Job::List { kind, generation } => {
                if generation != self.generation {
                    return;
                }
                if ok || (kind == Kind::Secret && cut) {
                    self.listed(kind, &out, cut);
                } else {
                    self.listing_failed(kind, why());
                }
                self.refresh_main();
            }
            Job::Namespaces { generation } => {
                if generation != self.generation {
                    return;
                }
                match (ok, model::namespace_names(&out.join("\n"))) {
                    (true, Ok(names)) => {
                        let mut items = vec![PickerItem::new("All namespaces", "*").detail("every namespace you may list")];
                        let current = self.namespace();
                        items.extend(names.into_iter().filter(|n| kubectl::label(n).is_ok()).map(|n| {
                            let item = PickerItem::new(n.clone(), n.clone());
                            if current.as_deref() == Some(n.as_str()) {
                                item.detail("in use")
                            } else {
                                item
                            }
                        }));
                        let id = api::pick("Namespace", &items);
                        self.pickers.insert(id, Picking::Namespace(self.generation));
                    }
                    _ => {
                        // Not allowed to list them (or no answer): type one.
                        api::notify(&format!("Couldn't list the namespaces ({}): type one", why()));
                        let id = api::prompt("Namespace", &self.namespace().unwrap_or_default());
                        self.prompts.insert(id, Asking::Namespace);
                    }
                }
            }
            Job::Yaml { target, context, generation } => {
                if generation != self.generation {
                    api::notify(&format!("{target}'s YAML came after the context changed: not shown"));
                    return;
                }
                if !ok {
                    api::notify(&format!("Couldn't read {target}'s YAML: {}", why()));
                    return;
                }
                let title = format!("{}-{}.yaml ({context}/{})", target.kind.singular(), target.name, target.namespace);
                if target.kind == Kind::Secret {
                    // Its values out before anything shows it.
                    match secret_text(&target, &out.join("\n")) {
                        Ok(text) => api::open_text(&title, &text, Some("yaml")),
                        Err(why) => api::notify(&format!("{target}: {why}")),
                    }
                } else {
                    api::open_text(&title, &out.join("\n"), Some("yaml"));
                }
            }
            Job::Describe { target, context, generation } => {
                if generation != self.generation {
                    api::notify(&format!("describe {target} came after the context changed: not shown"));
                } else if ok {
                    api::open_text(&format!("describe {} ({context}/{})", target.reference(), target.namespace), &out.join("\n"), None);
                } else {
                    api::notify(&format!("Couldn't describe {target}: {}", why()));
                }
            }
            Job::Logs { page } => {
                if let Some(v) = self.logs.get_mut(&page) {
                    if v.stream != Stream::Stopped {
                        v.stream = Stream::Ended(if ok { if v.options.previous { "read to its end".into() } else { "kubectl stopped following".into() } } else { why() });
                    }
                }
                pages::refresh(page);
            }
            Job::Change { what } => {
                if ok {
                    let said = out.iter().map(|l| agent::shown(l.trim())).find(|l| !l.is_empty()).unwrap_or_else(|| "done".into());
                    api::notify(&format!("{what}: {said}"));
                } else {
                    api::notify(&format!("Couldn't {what}: {}", why()));
                }
                self.failed = false;
                self.refresh(false);
            }
            Job::Agent { call } => {
                let why = why();
                self.agent_done(call, ok, out, why, cut)
            }
        }
    }

    // -- Lists -----------------------------------------------------------------

    fn set_loading(&mut self, kind: Kind, loading: bool) -> usize {
        let base = max_items();
        let c = &mut self.cluster;
        let (l, limit) = match kind {
            Kind::Pod => (&mut c.pods.loading, &mut c.pods.limit),
            Kind::Service => (&mut c.services.loading, &mut c.services.limit),
            Kind::Ingress => (&mut c.ingresses.loading, &mut c.ingresses.limit),
            Kind::Secret => (&mut c.secrets.loading, &mut c.secrets.limit),
            k => {
                let w = c.workloads.entry(k).or_default();
                (&mut w.loading, &mut w.limit)
            }
        };
        *l = loading;
        *limit = (*limit).max(base);
        *limit
    }

    fn is_loading(&self, kind: Kind) -> bool {
        self.cluster.state(kind).3
    }

    /// `kind` asked for again.
    fn list(&mut self, kind: Kind, context: &str) {
        if self.is_loading(kind) || self.missing.is_some() {
            return;
        }
        let limit = self.set_loading(kind, true);
        let namespace = self.namespace();
        let args = match kind {
            Kind::Secret => kubectl::secrets(context, namespace.as_deref()),
            k => kubectl::list(k, context, namespace.as_deref(), limit),
        };
        self.spawn(Job::List { kind, generation: self.generation }, args);
    }

    fn listed(&mut self, kind: Kind, out: &[String], cut: bool) {
        // `get --raw` answers on one line: parsed where it is, not copied.
        let text: std::borrow::Cow<str> = if out.len() == 1 { std::borrow::Cow::Borrowed(&out[0]) } else { std::borrow::Cow::Owned(out.join("\n")) };
        let c = &mut self.cluster;
        let result = match kind {
            Kind::Pod => model::pods(&text).map(|l| fill(&mut c.pods, l)),
            Kind::Service => model::services(&text).map(|l| fill(&mut c.services, l)),
            Kind::Ingress => model::ingresses(&text).map(|l| fill(&mut c.ingresses, l)),
            Kind::Secret => {
                let all = self.selection.namespace == Ns::All;
                let ns = match &self.selection.namespace {
                    Ns::All => String::new(),
                    _ => self.namespace().unwrap_or_default(),
                };
                let items = out.iter().filter_map(|l| model::secret_line(l, all, &ns)).collect();
                fill(&mut self.cluster.secrets, model::Listed { items, more: if cut { More::Some } else { More::None }, cont: None });
                Ok(())
            }
            k => model::workloads(k, &text).map(|l| fill(c.workloads.entry(k).or_default(), l)),
        };
        if let Err(why) = result {
            self.listing_failed(kind, why);
        }
    }

    fn listing_failed(&mut self, kind: Kind, why: String) {
        api::warn!("listing {}: {why}", kind.resource());
        self.failed = true;
        let c = &mut self.cluster;
        let set = |e: &mut Option<String>, l: &mut bool| {
            *e = Some(why.clone());
            *l = false;
        };
        match kind {
            Kind::Pod => set(&mut c.pods.error, &mut c.pods.loading),
            Kind::Service => set(&mut c.services.error, &mut c.services.loading),
            Kind::Ingress => set(&mut c.ingresses.error, &mut c.ingresses.loading),
            Kind::Secret => set(&mut c.secrets.error, &mut c.secrets.loading),
            k => {
                let w = c.workloads.entry(k).or_default();
                set(&mut w.error, &mut w.loading)
            }
        }
    }

    /// The page's lists asked for again: the tab in view's (`whole`: and
    /// whether the cluster answers).
    fn refresh(&mut self, whole: bool) {
        let Some(tab) = self.main.as_ref().map(|(_, v)| v.tab()) else { return };
        if self.kubeconfig.is_none() {
            self.refresh_waiting = true;
            return;
        }
        if self.missing.is_some() {
            return;
        }
        let Some(context) = self.context() else {
            self.refresh_main();
            return;
        };
        if whole {
            self.failed = false;
            self.server = Server::Checking;
            self.spawn(Job::ServerVersion { generation: self.generation }, kubectl::server_version(&context));
        }
        for kind in tab.kinds() {
            self.list(*kind, &context);
        }
        self.refresh_main();
    }

    /// `r`: whatever is still being waited for is given up, kubeconfig is
    /// read again, and everything asked again.
    fn reload(&mut self) {
        let waiting: Vec<u64> = self.jobs.iter().filter(|(_, r)| matches!(r.job, Job::List { .. } | Job::ServerVersion { .. })).map(|(p, _)| *p).collect();
        for p in waiting {
            api::process::kill(p);
            if let Some(Running { job: Job::List { kind, .. }, .. }) = self.jobs.remove(&p) {
                self.set_loading(kind, false);
            }
        }
        self.failed = false;
        self.read_config();
        self.refresh(true);
    }

    fn read_config(&mut self) {
        self.config_seq += 1;
        self.spawn(Job::Config { seq: self.config_seq }, kubectl::config_view());
    }

    /// On the timer: the tab in view's lists, unless something failed
    /// (no retry storm: `r` asks again) or they're still coming.
    fn tick(&mut self) {
        let Some((_, view)) = &self.main else { return };
        if !self.main_shown || self.failed || matches!(self.server, Server::Down(_) | Server::Checking) {
            return;
        }
        let kinds = view.tab().kinds();
        if kinds.iter().any(|k| self.is_loading(*k)) {
            return;
        }
        self.refresh(false);
    }

    fn start_timer(&mut self) {
        let seconds: u32 = setting("kubernetes.refresh_seconds").and_then(|v| v.parse().ok()).unwrap_or(10);
        if self.timer.is_none() && seconds > 0 {
            self.timer = Some(api::host::set_timer(seconds.clamp(2, 3600) * 1000, true));
        }
    }

    fn stop_timer(&mut self) {
        if let Some(t) = self.timer.take() {
            api::host::cancel_timer(t);
        }
    }

    /// What's listed is no longer where calls go: it goes, with every
    /// answer still coming for it and any question asked about it.
    fn reset(&mut self) {
        self.generation += 1;
        let old: Vec<u64> = self.jobs.iter().filter(|(_, r)| matches!(r.job, Job::List { .. } | Job::ServerVersion { .. } | Job::Namespaces { .. })).map(|(p, _)| *p).collect();
        for p in old {
            api::process::kill(p);
            self.jobs.remove(&p);
        }
        self.cluster = Cluster::default();
        self.server = Server::Unknown;
        self.failed = false;
        self.asked = None;
        self.prompts.retain(|_, a| !matches!(a, Asking::Typed(..) | Asking::Replicas(..)));
        if let Some((_, v)) = &mut self.main {
            v.confirm = None;
            v.menu = None;
        }
        self.refresh_main();
    }

    /// Where it works changed by your choice.
    fn select(&mut self, selection: Selection) {
        if selection == self.selection {
            return;
        }
        self.selection = selection;
        self.reset();
        self.doctor();
        self.refresh(true);
    }

    // -- The doctor --------------------------------------------------------------

    fn doctor(&self) {
        use api::{Check, Health, Section};
        let (health, detail) = match (&self.missing, &self.client, &self.server) {
            (Some(why), _, _) => (Health::Bad, format!("can't run: {why}")),
            (None, None, _) => (Health::Info, "looking for it".to_string()),
            (None, Some(v), Server::Up(s)) => (Health::Ok, format!("{v}; {} answers (server {s})", self.context().unwrap_or_default())),
            (None, Some(v), Server::Down(why)) => (Health::Warn, format!("{v}; {}: {why}", self.context().unwrap_or_else(|| "the cluster".into()))),
            (None, Some(v), _) => match (&self.kubeconfig, self.context()) {
                (Some(k), None) if k.contexts.is_empty() => (Health::Warn, format!("{v}; kubeconfig has no contexts")),
                (Some(_), None) => (Health::Warn, format!("{v}; kubeconfig has no current context: the Kubernetes page's c chooses one")),
                (_, c) => (Health::Ok, format!("{v}; the cluster ({}) is asked when the Kubernetes page opens", c.unwrap_or_else(|| "kubeconfig's current".into()))),
            },
        };
        let mut check = Check::new(Section::Toolchain, "kubectl", health, detail);
        if self.missing.is_none() {
            check = check.fix("open the Kubernetes page", "kubernetes.open");
        }
        api::project::doctor(&[check]);
    }

    // -- The page's actions --------------------------------------------------------

    fn open_main(&mut self) {
        if let Some((p, _)) = &self.main {
            pages::focus(*p);
            return;
        }
        let p = pages::open(MAIN, "Kubernetes");
        self.main = Some((p, MainView::default()));
        self.main_shown = true;
        self.start_timer();
        self.refresh(true);
    }

    fn pick_context(&mut self) {
        if let Some(why) = &self.config_error {
            api::notify(&format!("kubectl couldn't read kubeconfig: {why}"));
            return;
        }
        let Some(k) = &self.kubeconfig else {
            api::notify("kubeconfig isn't read yet: a moment");
            return;
        };
        if k.contexts.is_empty() {
            api::notify("kubeconfig has no contexts");
            return;
        }
        let current = self.context();
        let items: Vec<PickerItem> = k
            .contexts
            .iter()
            .map(|c| {
                let mut detail = vec![format!("cluster {}", c.cluster)];
                if let Some(ns) = &c.namespace {
                    detail.push(format!("namespace {ns}"));
                }
                if k.current.as_deref() == Some(c.name.as_str()) {
                    detail.push("kubeconfig's current".into());
                }
                if current.as_deref() == Some(c.name.as_str()) {
                    detail.push("in use here".into());
                }
                detail.push(self.verdict(&c.name).label().into());
                PickerItem::new(c.name.clone(), c.name.clone()).detail(detail.join(" · "))
            })
            .collect();
        let id = api::pick("Context, for this project", &items);
        self.pickers.insert(id, Picking::Context);
    }

    fn pick_namespace(&mut self) {
        if self.missing.is_some() {
            return;
        }
        let Some(context) = self.need_context("list namespaces") else { return };
        self.spawn(Job::Namespaces { generation: self.generation }, kubectl::namespaces(&context));
    }

    fn act(&mut self, page: u64, action: Action) {
        match action {
            Action::Nothing => return,
            Action::Redraw => {}
            Action::Refresh => self.reload(),
            Action::PickContext => self.pick_context(),
            Action::PickNamespace => self.pick_namespace(),
            Action::TakeSuggestion => match self.suggestion.clone() {
                Some(s) => {
                    let taken = Selection { context: s.context.or_else(|| self.selection.context.clone()), namespace: s.namespace };
                    self.select(taken);
                }
                None => api::notify("No context or namespace is suggested (kubernetes.context, kubernetes.namespace)"),
            },
            Action::Filter => {
                let filter = self.main.as_ref().map(|(_, v)| v.filter.clone()).unwrap_or_default();
                let id = api::prompt("Filter by name", &filter);
                self.prompts.insert(id, Asking::Filter);
            }
            Action::More(kind) => {
                let add = max_items();
                let c = &mut self.cluster;
                let limit = match kind {
                    Kind::Pod => &mut c.pods.limit,
                    Kind::Service => &mut c.services.limit,
                    Kind::Ingress => &mut c.ingresses.limit,
                    Kind::Secret => &mut c.secrets.limit,
                    k => &mut c.workloads.entry(k).or_default().limit,
                };
                if *limit >= MOST_ITEMS {
                    api::notify(&format!("{MOST_ITEMS} {} is as many as the page lists: choose a namespace (n), or filter", kind.resource()));
                } else {
                    *limit = (*limit + add).min(MOST_ITEMS);
                    api::notify(&format!("listing up to {} {}", *limit, kind.resource()));
                    if let Some(context) = self.need_context("list") {
                        self.list(kind, &context);
                    }
                }
            }
            Action::Logs(t) => self.open_logs(t),
            Action::Shell(t) => self.shell(t),
            Action::Forward(t) => {
                let Some(context) = self.need_context("port-forward") else { return pages::refresh(page) };
                let port = match t.kind {
                    Kind::Service => self.cluster.service(&t).and_then(|s| s.first_port),
                    Kind::Pod => self.cluster.pod(&t.namespace, &t.name).and_then(|p| p.ports.first().copied()),
                    _ => None,
                };
                let initial = port.map(|p| format!("{}:{p}", if p < 1024 { 8000 + p } else { p })).unwrap_or_else(|| "8080:80".into());
                let id = api::prompt(&format!("Port-forward {} ({context}/{}): local:remote", t.reference(), t.namespace), &initial);
                self.prompts.insert(id, Asking::Ports(t));
            }
            Action::Yaml(t) => {
                if let Some(context) = self.need_context("read YAML") {
                    self.spawn(Job::Yaml { target: t.clone(), context: context.clone(), generation: self.generation }, kubectl::get(&t, &context));
                }
            }
            Action::Describe(t) => {
                if let Some(context) = self.need_context("describe") {
                    let args = kubectl::describe(&t, &context);
                    self.spawn(Job::Describe { target: t, context, generation: self.generation }, args);
                }
            }
            Action::Copy(name) => {
                api::copy(&name);
                api::notify(&format!("copied {name}"));
            }
            Action::Mutate(m) => self.ask_change(m),
            Action::Scale(t) => {
                if let Some(context) = self.need_context("scale") {
                    let from = self.cluster.workload(&t).and_then(|w| w.replicas);
                    let id = api::prompt(&format!("Scale {t} ({context}/{}) to how many replicas?", t.namespace), &from.map(|f| f.to_string()).unwrap_or_default());
                    self.prompts.insert(id, Asking::Replicas(t, from));
                }
            }
            Action::Confirmed(m) => self.confirmed(m),
            Action::Declined => self.asked = None,
            Action::StopForward(task) => self.stop_forward(task),
            Action::ShowForward(task) => {
                if let Err(why) = api::task::show(task) {
                    api::notify(&format!("Its output is gone: {why}"));
                }
            }
            Action::OpenUrl(url) => {
                if let Err(why) = api::open_url(&url) {
                    api::notify(&format!("Couldn't open {url}: {why}"));
                }
            }
            Action::Say(text) => api::notify(&text),
        }
        pages::refresh(page);
    }

    // -- Changes ---------------------------------------------------------------------

    /// A change: refused, asked with its name to type, or asked plainly --
    /// every time naming the context and the namespace.
    fn ask_change(&mut self, m: Mutation) {
        let Some(context) = self.context() else {
            api::notify("Which cluster? kubeconfig has no current context: c chooses one first. Nothing changed.");
            return;
        };
        if self.protected(&context) {
            if setting("kubernetes.protected_mode").as_deref() == Some("refuse") {
                api::notify(&format!("{context} is protected: {} refused here (kubernetes.protected_mode). Nothing changed.", m.what()));
                return;
            }
            let name = m.target().name.clone();
            let label = format!("PROTECTED context {context}, namespace {}: type {name} to {}", m.target().namespace, m.what());
            let id = api::prompt(&label, "");
            self.prompts.insert(id, Asking::Typed(m, context));
            return;
        }
        let question = m.question(&context);
        self.asked = Some((m.clone(), context));
        if let Some((p, v)) = &mut self.main {
            v.confirm = Some((m, question));
            pages::refresh(*p);
        }
    }

    /// `y` to a question: done, if it's still the one asked, in the same
    /// context, and that context isn't protected now.
    fn confirmed(&mut self, m: Mutation) {
        match self.asked.take() {
            Some((asked, context)) if asked == m && self.context().as_deref() == Some(context.as_str()) => {
                if self.protected(&context) {
                    api::notify(&format!("{context} is protected now: ask again. Nothing changed."));
                } else {
                    self.change(m, &context);
                }
            }
            _ => api::notify("The context changed since it was asked: nothing done"),
        }
    }

    fn change(&mut self, m: Mutation, context: &str) {
        let what = m.what();
        let args = match &m {
            Mutation::Delete(t) => kubectl::delete(t, context),
            Mutation::Restart(t) => kubectl::rollout_restart(t, context),
            Mutation::Scale { target, from, to } => kubectl::scale(target, context, *from, *to),
        };
        self.spawn(Job::Change { what }, args);
    }

    // -- Logs, shells, forwards -----------------------------------------------------

    fn open_logs(&mut self, t: Target) {
        if self.need_context("read a log").is_none() {
            return;
        }
        let containers = if t.kind == Kind::Pod { self.cluster.pod(&t.namespace, &t.name).map(|p| p.containers.clone()).unwrap_or_default() } else { Vec::new() };
        if containers.len() > 1 {
            let mut items = vec![PickerItem::new("All containers", "*")];
            items.extend(containers.iter().map(|c| PickerItem::new(c.clone(), c.clone())));
            let id = api::pick("Logs of which container", &items);
            self.pickers.insert(id, Picking::LogContainer(t));
        } else {
            let first = containers.first().cloned();
            self.log_page(t, containers, first);
        }
    }

    fn log_page(&mut self, t: Target, containers: Vec<String>, container: Option<String>) {
        let Some(context) = self.need_context("read a log") else { return };
        let view = LogView::new(t.clone(), context, containers, container);
        let page = pages::open(LOGS, &format!("logs {}", t.name));
        self.logs.insert(page, view);
        self.stream(page);
    }

    /// The log page's stream, (re)started with its options. The one before
    /// is stopped and forgotten: what it still says goes nowhere.
    fn stream(&mut self, page: u64) {
        self.stop_stream(page);
        let Some(v) = self.logs.get_mut(&page) else { return };
        v.stream = Stream::Running;
        let args = v.args();
        self.spawn(Job::Logs { page }, args);
        pages::refresh(page);
    }

    fn stop_stream(&mut self, page: u64) {
        let running: Vec<u64> = self.jobs.iter().filter(|(_, r)| r.job == Job::Logs { page }).map(|(p, _)| *p).collect();
        for p in running {
            api::process::kill(p);
            self.jobs.remove(&p);
        }
    }

    fn shell(&mut self, t: Target) {
        if self.need_context("open a shell").is_none() {
            return;
        }
        let containers = self.cluster.pod(&t.namespace, &t.name).map(|p| p.containers.clone()).unwrap_or_default();
        match containers.len() {
            0 => api::notify(&format!("{t}: its containers aren't known yet: r, then again")),
            1 => self.exec(&t, &containers[0]),
            _ => {
                let items: Vec<PickerItem> = containers.iter().map(|c| PickerItem::new(c.clone(), c.clone())).collect();
                let id = api::pick("Shell in which container", &items);
                self.pickers.insert(id, Picking::ShellContainer(t));
            }
        }
    }

    fn exec(&mut self, t: &Target, container: &str) {
        let Ok(container) = kubectl::label(container) else { return };
        let Some(context) = self.need_context("open a shell") else { return };
        let cmd = api::Command::new(&self.program).args(kubectl::exec(t, &context, &container));
        let title = format!("{}/{container} ({context}/{})", t.name, t.namespace);
        if let Err(why) = api::host::run_in_terminal(&cmd, &title, false) {
            api::notify(&format!("Couldn't open a terminal: {why}"));
        }
    }

    fn forward(&mut self, t: Target, local: u16, remote: u16) {
        if let Some(f) = self.forwards.iter().find(|f| f.local == local) {
            api::notify(&format!("localhost:{local} is already forwarded to {}: s on the Forwards tab stops it", f.target.reference()));
            return;
        }
        let Some(context) = self.need_context("port-forward") else { return };
        let cmd = api::Command::new(&self.program).args(kubectl::port_forward(&t, &context, local, remote));
        let name = format!("port-forward {} {local}:{remote}", t.reference());
        match api::host::run_task_background(&name, &cmd) {
            Ok(task) => {
                api::notify(&format!("localhost:{local} → {} {remote}, while the task runs (the Forwards tab, 5)", t.reference()));
                self.forwards.push(Forward { task, target: t, context, local, remote });
            }
            Err(why) => api::notify(&format!("Couldn't start {name}: {why} (Fenix runs 4 background tasks of an extension's at once)")),
        }
        self.refresh_main();
    }

    fn stop_forward(&mut self, task: u64) {
        if let Some(f) = self.forwards.iter().find(|f| f.task == task) {
            api::notify(&format!("stopping {}", f.label()));
            api::task::cancel(task);
        }
    }

    // -- Agents ------------------------------------------------------------------------

    /// Too many calls: at most one at a time per tool, `AGENT_AT_ONCE` in
    /// all, `AGENT_PER_MINUTE` a minute.
    fn agent_busy(&mut self, service: &str) -> Option<String> {
        let t = api::clock::now().timestamp();
        while self.agent_times.front().is_some_and(|s| t - s >= 60) {
            self.agent_times.pop_front();
        }
        let running = self.agent_calls.len() + self.agent_waiting.len();
        let this = self.agent_calls.values().filter(|c| c.service == service).count() + self.agent_waiting.iter().filter(|(_, s, _)| s == service).count();
        if this > 0 {
            return Some(format!("busy: a call to this tool is still running; ask again when it has answered ({AGENT_AT_ONCE} calls at once at most, one per tool)"));
        }
        if running >= AGENT_AT_ONCE {
            return Some(format!("busy: {running} calls are running; at most {AGENT_AT_ONCE} at once"));
        }
        if self.agent_times.len() >= AGENT_PER_MINUTE {
            return Some(format!("too many calls: {AGENT_PER_MINUTE} in the last minute already; wait a little"));
        }
        self.agent_times.push_back(t);
        None
    }

    fn agent_start(&mut self, call: u64, service: &str, ask: Ask) -> Option<Result<String, String>> {
        if let Some(why) = &self.missing {
            return Some(Err(format!("kubectl can't run here: {why}")));
        }
        let Some(context) = self.context() else {
            return Some(Err("no context: kubeconfig has no current context, and the user hasn't chosen one on the Kubernetes page. Nothing ran.".into()));
        };
        let Some(namespace) = self.namespace() else {
            return Some(Err("the user's Kubernetes page shows every namespace: agents read only one, which the user chooses there (n)".into()));
        };
        let tail = match &ask {
            Ask::Logs { lines, .. } => Some(Tail::new(*lines)),
            _ => None,
        };
        let args = agent::command(&ask, &context, &namespace);
        self.agent_calls.insert(call, AgentCall { service: service.to_string(), ask, context, namespace, tail });
        self.spawn(Job::Agent { call }, args);
        None
    }

    fn agent_done(&mut self, call: u64, ok: bool, out: Vec<String>, why: String, cut: bool) {
        let Some(c) = self.agent_calls.remove(&call) else { return };
        let text = out.join("\n");
        let answer = if !ok && !(cut && c.ask == Ask::Workloads) {
            // kubectl's words, as cleaned for an agent.
            Err(agent::plain(&why))
        } else {
            match &c.ask {
                Ask::Workloads => Ok(agent::workloads_answer(&c.context, &c.namespace, &model::workload_lines(&out), cut, now()).to_string()),
                Ask::Pods => model::pods(&text).map(|l| agent::pods_answer(&c.context, &c.namespace, &l.items, &l.more, now()).to_string()),
                Ask::Pod { .. } => serde_json::from_str::<model::RawPod>(&text).map(|p| agent::pod_answer(&c.context, &p, now()).to_string()).map_err(|e| format!("kubectl's answer wasn't a pod: {e}")),
                Ask::Events { .. } => model::read_list::<model::RawEvent>(&text).map(|l| agent::events_answer(&c.context, &c.namespace, l.items, &l.more).to_string()),
                Ask::Logs { pod, .. } => Ok(c.tail.as_ref().map(|t| t.answer(&c.context, &c.namespace, pod)).unwrap_or_default().to_string()),
                Ask::Describe { kind, name } => agent::describe_answer(&c.context, &c.namespace, kind, name, &text).map(|v| v.to_string()),
            }
        };
        api::answer(call, answer);
    }
}

/// What a Secret's buffer shows: its keys, never their values.
pub fn secret_text(target: &Target, json: &str) -> Result<String, String> {
    let v = secret::redact(json)?;
    Ok(format!("# {target}: its keys, not their values.\n{}", secret::yaml(&v)))
}

fn fill<T>(list: &mut Listing<T>, l: model::Listed<T>) {
    list.items = l.items;
    list.more = l.more;
    list.cont = l.cont;
    list.error = None;
    list.loading = false;
    list.loaded = true;
}

impl Extension for Kube {
    fn activate(&mut self) -> Result<(), String> {
        self.program = setting("kubernetes.kubectl").unwrap_or_else(|| "kubectl".into());
        self.suggestion = suggested();
        // Only what's on this machine: kubectl's version and kubeconfig's
        // contexts. The cluster is asked when the page opens.
        self.spawn(Job::ClientVersion, kubectl::client_version());
        self.read_config();
        self.doctor();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "kubernetes.open" => self.open_main(),
            "kubernetes.context" => self.pick_context(),
            "kubernetes.namespace" => self.pick_namespace(),
            "kubernetes.forwards" => {
                if self.forwards.is_empty() {
                    api::notify("No port-forwards running");
                } else {
                    let items: Vec<PickerItem> = self.forwards.iter().map(|f| PickerItem::new(f.label(), f.task.to_string()).detail(format!("localhost:{} · {}/{}", f.local, f.context, f.target.namespace))).collect();
                    let id = api::pick("Stop which port-forward", &items);
                    self.pickers.insert(id, Picking::StopForward);
                }
            }
            _ => return Err(format!("no command {id}")),
        }
        Ok(())
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Timer(t) if Some(t) == self.timer => self.tick(),
            Event::Timer(t) if Some(t) == self.watchdog => self.beat(),
            Event::ProcessOutput(o) => {
                let cap = self.cluster.secrets.limit.max(max_items());
                let Some(run) = self.jobs.get_mut(&o.process) else { return };
                if o.stderr {
                    if run.err.len() < 200 {
                        run.err.push(o.line);
                    }
                    return;
                }
                match &run.job {
                    Job::Logs { page } => {
                        let page = *page;
                        if let Some(v) = self.logs.get_mut(&page) {
                            v.push(&o.line);
                        }
                        pages::refresh(page);
                    }
                    Job::Agent { call } => {
                        let Some(c) = self.agent_calls.get_mut(call) else { return };
                        if let Some(tail) = c.tail.as_mut() {
                            tail.push(&o.line);
                        } else if c.ask == Ask::Workloads {
                            if run.out.len() >= agent::MAX_WORKLOAD_LINES {
                                if !run.cut {
                                    run.cut = true;
                                    api::process::kill(o.process);
                                }
                            } else {
                                run.out.push(o.line);
                            }
                        } else {
                            run.out.push(o.line);
                        }
                    }
                    Job::List { kind: Kind::Secret, .. } => {
                        if run.out.len() >= cap {
                            if !run.cut {
                                // Past the cap: the rest isn't read.
                                run.cut = true;
                                api::process::kill(o.process);
                            }
                        } else {
                            run.out.push(o.line);
                        }
                    }
                    _ => run.out.push(o.line),
                }
            }
            Event::ProcessExit((process, code)) => self.ended(process, code),
            Event::TaskFinished((task, ok)) => {
                if let Some(at) = self.forwards.iter().position(|f| f.task == task) {
                    let f = self.forwards.remove(at);
                    api::notify(&format!("{} ended{}", f.label(), if ok { "" } else { ": the Forwards tab's Enter, or the task's output, says why" }));
                    self.refresh_main();
                }
            }
            Event::Picked((id, picked)) => {
                let Some(what) = self.pickers.remove(&id) else { return };
                let Some(value) = picked else { return };
                self.picked(what, value);
            }
            Event::Prompted((id, text)) => {
                let Some(what) = self.prompts.remove(&id) else { return };
                let Some(text) = text else { return };
                self.prompted(what, text);
            }
            Event::PageShown((p, shown)) => {
                if self.main.as_ref().is_some_and(|(m, _)| *m == p) {
                    self.main_shown = shown;
                    if shown {
                        self.start_timer();
                        self.tick();
                    } else {
                        self.stop_timer();
                    }
                }
            }
            Event::SettingChanged(key) => match key.as_str() {
                "kubernetes.kubectl" => {
                    // Another kubectl may read another kubeconfig: nothing
                    // listed, asked or known stays.
                    self.program = setting("kubernetes.kubectl").unwrap_or_else(|| "kubectl".into());
                    self.missing = None;
                    self.client = None;
                    self.kubeconfig = None;
                    self.reset();
                    self.refresh_waiting = self.main.is_some();
                    self.spawn(Job::ClientVersion, kubectl::client_version());
                    self.read_config();
                }
                "kubernetes.refresh_seconds" => {
                    self.stop_timer();
                    if self.main_shown && self.main.is_some() {
                        self.start_timer();
                    }
                }
                "kubernetes.context" | "kubernetes.namespace" => {
                    self.suggestion = suggested();
                    self.refresh_main();
                }
                _ => self.refresh_main(),
            },
            _ => {}
        }
    }

    fn serve(&mut self, call: u64, name: &str, payload: &str) -> Option<Result<String, String>> {
        let ask = match agent::ask(name, payload) {
            Ok(a) => a,
            Err(why) => return Some(Err(why)),
        };
        if agent::NEED_LOGS_SETTING.contains(&name) && setting("kubernetes.agent_logs").as_deref() != Some("true") {
            return Some(Err("turned off: the user hasn't let agents read logs and descriptions (kubernetes.agent_logs), which can hold secrets and are written by whatever runs in the cluster".into()));
        }
        if let Some(why) = self.agent_busy(name) {
            return Some(Err(why));
        }
        if self.kubeconfig.is_none() && self.missing.is_none() {
            // kubeconfig's contexts aren't read yet: answered once they are.
            self.agent_waiting.push((call, name.to_string(), ask));
            return None;
        }
        self.agent_start(call, name, ask)
    }

    fn service_cancelled(&mut self, call: u64) {
        self.agent_waiting.retain(|(c, _, _)| *c != call);
        self.agent_calls.remove(&call);
        let running: Vec<u64> = self.jobs.iter().filter(|(_, r)| r.job == Job::Agent { call }).map(|(p, _)| *p).collect();
        for p in running {
            api::process::kill(p);
            self.jobs.remove(&p);
        }
    }

    fn page(&mut self, page: u64, width: u32, height: u32) -> Page {
        self.heights.insert(page, height as usize);
        if let Some((p, view)) = &self.main {
            if *p == page {
                return view.render(&self.header(), &self.cluster, &self.forwards, now(), width as usize, height as usize).to_page();
            }
        }
        if let Some(v) = self.logs.get(&page) {
            return v.render(width as usize, height as usize).to_page();
        }
        Page::new().line([pages::muted("Nothing here: the page is closed")])
    }

    fn page_key(&mut self, page: u64, key: Key) {
        let now = now();
        if let Some((p, view)) = &mut self.main {
            if *p == page {
                let all = self.selection.namespace == Ns::All;
                let action = view.key(key.into(), &self.cluster, &self.forwards, all, now);
                self.act(page, action);
                return;
            }
        }
        if let Some(v) = self.logs.get_mut(&page) {
            let height = self.heights.get(&page).copied().unwrap_or(24);
            match v.key(key.into(), height) {
                LogAction::Nothing => return,
                LogAction::Redraw => {}
                LogAction::Restart => self.stream(page),
                LogAction::Stop => {
                    self.stop_stream(page);
                    if let Some(v) = self.logs.get_mut(&page) {
                        v.stream = Stream::Stopped;
                    }
                }
                LogAction::AskSince => {
                    let id = api::prompt("Since (30s, 10m, 2h; empty: the last 200 lines)", v.options.since.as_deref().unwrap_or(""));
                    self.prompts.insert(id, Asking::Since(page));
                }
                LogAction::PickContainer => {
                    let mut items = vec![PickerItem::new("All containers", "*")];
                    items.extend(v.containers.iter().map(|c| PickerItem::new(c.clone(), c.clone())));
                    let id = api::pick("Log of which container", &items);
                    self.pickers.insert(id, Picking::PageContainer(page));
                }
                LogAction::OpenBuffer => api::open_text(&format!("{}.log", v.target.name), &v.text(), Some("log")),
                LogAction::Filter => {
                    let id = api::prompt("Filter the log", &v.filter);
                    self.prompts.insert(id, Asking::LogFilter(page));
                }
            }
            pages::refresh(page);
        }
    }

    fn page_closed(&mut self, page: u64) {
        if self.main.as_ref().is_some_and(|(p, _)| *p == page) {
            self.main = None;
            self.main_shown = false;
            self.asked = None;
            self.stop_timer();
        }
        if self.logs.remove(&page).is_some() {
            self.stop_stream(page);
        }
        self.heights.remove(&page);
    }

    /// The main page keeps where it was: the context and namespace you
    /// chose, the tab -- kept by Fenix with the project's session.
    fn page_state(&mut self, page: u64) -> Option<String> {
        let (p, view) = self.main.as_ref()?;
        if *p != page {
            return None;
        }
        let ns = match &self.selection.namespace {
            Ns::Default => serde_json::Value::Null,
            Ns::All => "*".into(),
            Ns::Named(n) => n.clone().into(),
        };
        Some(serde_json::json!({ "context": self.selection.context, "namespace": ns, "tab": view.tabs.active }).to_string())
    }

    fn page_restore(&mut self, page: u64, kind: &str, state: &str) -> bool {
        if kind != MAIN || self.main.is_some() {
            return false;
        }
        let v: serde_json::Value = serde_json::from_str(state).unwrap_or_default();
        let context = v["context"].as_str().and_then(|c| kubectl::context(c).ok());
        let namespace = match v["namespace"].as_str() {
            Some("*") => Ns::All,
            Some(n) => kubectl::label(n).map(Ns::Named).unwrap_or_default(),
            None => Ns::Default,
        };
        self.selection = Selection { context, namespace };
        let mut view = MainView::default();
        view.tabs.active = v["tab"].as_u64().unwrap_or(0).min(4) as usize;
        self.main = Some((page, view));
        self.main_shown = true;
        self.start_timer();
        self.refresh(true);
        true
    }
}

impl Kube {
    fn picked(&mut self, what: Picking, value: String) {
        match what {
            Picking::Context => {
                let Ok(name) = kubectl::context(&value) else { return };
                self.select(Selection { context: Some(name), namespace: Ns::Default });
            }
            Picking::Namespace(generation) => {
                if generation != self.generation {
                    return; // Listed for a context no longer in use.
                }
                let namespace = if value == "*" { Ns::All } else { kubectl::label(&value).map(Ns::Named).unwrap_or_default() };
                let s = Selection { context: self.selection.context.clone().or_else(|| self.context()), namespace };
                self.select(s);
            }
            Picking::LogContainer(t) => {
                let containers = self.cluster.pod(&t.namespace, &t.name).map(|p| p.containers.clone()).unwrap_or_default();
                let container = if value == "*" { None } else { kubectl::label(&value).ok() };
                self.log_page(t, containers, container);
            }
            Picking::ShellContainer(t) => self.exec(&t, &value),
            Picking::PageContainer(page) => {
                if let Some(v) = self.logs.get_mut(&page) {
                    v.options.container = if value == "*" { None } else { kubectl::label(&value).ok() };
                    v.restart();
                    self.stream(page);
                }
            }
            Picking::StopForward => {
                if let Ok(task) = value.parse() {
                    self.stop_forward(task);
                }
            }
        }
    }

    fn prompted(&mut self, what: Asking, text: String) {
        match what {
            Asking::Filter => {
                if let Some((p, v)) = &mut self.main {
                    v.filter = text.trim().to_string();
                    v.workloads.selected = 0;
                    v.pods.row = 0;
                    v.network.selected = 0;
                    v.secrets.selected = 0;
                    pages::refresh(*p);
                }
            }
            Asking::LogFilter(page) => {
                if let Some(v) = self.logs.get_mut(&page) {
                    v.filter = text.trim().to_string();
                    v.top = None;
                }
                pages::refresh(page);
            }
            Asking::Since(page) => {
                let since = if text.trim().is_empty() { Ok(None) } else { kubectl::since(&text).map(Some) };
                match since {
                    Ok(s) => {
                        if let Some(v) = self.logs.get_mut(&page) {
                            v.options.since = s;
                            v.options.previous = false;
                            v.restart();
                            self.stream(page);
                        }
                    }
                    Err(why) => api::notify(&why),
                }
            }
            Asking::Replicas(t, from) => match text.trim().parse::<u32>() {
                Ok(to) if to <= 10_000 => {
                    if from == Some(to) {
                        api::notify(&format!("{t} has {to} already: nothing to do"));
                    } else {
                        self.ask_change(Mutation::Scale { target: t, from, to });
                    }
                }
                _ => api::notify(&format!("{:?} isn't a number of replicas: nothing changed", text.trim())),
            },
            Asking::Typed(m, context) => {
                if text.trim() != m.target().name {
                    api::notify(&format!("That isn't {}: nothing changed", m.target().name));
                } else if self.context().as_deref() != Some(context.as_str()) {
                    api::notify("The context changed since it was asked: nothing done");
                } else {
                    self.change(m, &context);
                }
            }
            Asking::Ports(t) => match kubectl::ports(&text) {
                Ok((local, remote)) => self.forward(t, local, remote),
                Err(why) => api::notify(&why),
            },
            Asking::Namespace => {
                if text.trim() == "*" {
                    self.select(Selection { context: self.selection.context.clone(), namespace: Ns::All });
                } else {
                    match kubectl::label(text.trim()) {
                        Ok(n) => self.select(Selection { context: self.selection.context.clone(), namespace: Ns::Named(n) }),
                        Err(why) => api::notify(&why),
                    }
                }
            }
        }
    }
}

api::export!(Kube);
