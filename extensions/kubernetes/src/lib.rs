//! Kubernetes in Fenix, driving `kubectl`: the context and namespace you
//! choose for this project, a page of workloads, pods, services,
//! ingresses and Secrets' names, live logs, a shell in a pod, port-
//! forwards as background tasks, an object's YAML and `describe` in a
//! buffer, and changes -- delete a pod, restart, scale, apply an edited
//! YAML -- each asked about first. Coding agents get read-only tools.
//!
//! The parts that decide things are plain code, tested natively:
//! `kubectl` (every command line), `model` (what kubectl answered),
//! `secret` (a Secret without its values), `agent` (the agents' tools),
//! `page`, `logs` and `apply` (the pages and their keys). This file meets
//! Fenix: processes, pickers, prompts, timers, tasks, terminals.

pub mod agent;
pub mod apply;
pub mod kubectl;
pub mod logs;
pub mod model;
pub mod page;
pub mod secret;

#[cfg(test)]
mod driven;

use std::collections::HashMap;

#[cfg(not(test))]
pub(crate) use fenix_extension_api as api;
#[cfg(test)]
pub(crate) use fenix_test_kit::api;

use api::chrono::Utc;
use api::{page as pages, Event, Extension, Focus, Key, Page, PickerItem};

use agent::{Ask, Tail};
use apply::{ApplyAction, ApplyView, Stage};
use kubectl::{Kind, Ns, Selection, Target};
use logs::{LogAction, LogView, Stream};
use model::{Kubeconfig, More};
use page::{Action, Cluster, Forward, Header, Listing, MainView, Mutation};

const MAIN: &str = "main";
const LOGS: &str = "logs";
const APPLY: &str = "apply";

/// The most a list may be raised to with `+`, of one kind.
pub const MOST_ITEMS: usize = 5000;

/// What a process the extension started is for.
#[derive(Debug, Clone, PartialEq)]
enum Job {
    ClientVersion,
    Config,
    ServerVersion,
    List { kind: Kind, generation: u64 },
    Namespaces { generation: u64 },
    Yaml { target: Target, editable: bool, context: String },
    Describe { target: Target, context: String },
    Logs { page: u64 },
    Change { what: String },
    DryRun { page: u64 },
    Diff { page: u64 },
    Apply { page: u64 },
    UseContext(String),
    /// A coding agent's call: one process of those it needs.
    Agent { call: u64, part: Option<Kind> },
}

#[derive(Debug, Default)]
struct Running {
    job: Option<Job>,
    out: Vec<String>,
    err: Vec<String>,
    /// Output cut by hand (a table past its cap): there's more.
    cut: bool,
}

/// What a prompt was for.
#[derive(Debug, Clone, PartialEq)]
enum Asking {
    Filter,
    LogFilter(u64),
    Since(u64),
    Replicas(Target, Option<u32>),
    /// A change in a protected context: the name to type, and the
    /// context it was asked in.
    Typed(Mutation, String),
    Ports(Target),
    Namespace,
}

/// What a picker was for.
#[derive(Debug, Clone, PartialEq)]
enum Picking {
    Context { global: bool },
    Namespace,
    LogContainer(Target),
    ShellContainer(Target),
    PageContainer(u64),
    YamlBuffer,
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

/// An agent's call, while its processes run.
#[derive(Debug)]
struct AgentCall {
    ask: Ask,
    context: String,
    namespace: String,
    waiting: usize,
    workloads: Vec<(Kind, Result<model::Listed<model::Workload>, String>)>,
    pods: Option<Result<model::Listed<model::Pod>, String>>,
}

#[derive(Default)]
pub struct Kube {
    program: String,
    kubeconfig: Option<Kubeconfig>,
    client: Option<String>,
    /// Why kubectl can't run at all.
    missing: Option<String>,
    server: Server,
    selection: Selection,
    /// Bumped when the selection changes: answers to the old one go.
    generation: u64,
    cluster: Cluster,
    main: Option<(u64, MainView)>,
    main_shown: bool,
    logs: HashMap<u64, LogView>,
    applies: HashMap<u64, ApplyView>,
    jobs: HashMap<u64, Running>,
    forwards: Vec<Forward>,
    /// YAML buffers open to edit, and what each was opened from.
    yaml: HashMap<u64, (Target, String)>,
    focused_scratch: Option<u64>,
    prompts: HashMap<u64, Asking>,
    pickers: HashMap<u64, Picking>,
    timer: Option<u64>,
    /// A listing failed: the timer asks nothing until you do (`r`).
    failed: bool,
    /// The page asked for before kubeconfig was read.
    refresh_waiting: bool,
    /// A change asked about: it, and the context it was asked in.
    asked: Option<(Mutation, String)>,
    agent_waiting: Vec<(u64, Ask)>,
    agent_calls: HashMap<u64, AgentCall>,
    agent_tails: HashMap<u64, Tail>,
    /// The page's height, for the log pages' keys.
    heights: HashMap<u64, usize>,
}

fn setting(key: &str) -> Option<String> {
    api::setting(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// `kubernetes.max_items`: how many of a kind are listed (50 to 5,000).
fn max_items() -> usize {
    setting("kubernetes.max_items").and_then(|v| v.parse().ok()).unwrap_or(500).clamp(50, MOST_ITEMS)
}

fn protected_patterns() -> Vec<String> {
    setting("kubernetes.protected").map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
}

fn now() -> api::chrono::DateTime<Utc> {
    api::clock::now().with_timezone(&Utc)
}

impl Kube {
    // -- Where it works ----------------------------------------------------

    /// The context in use, by name: the one chosen here, else kubeconfig's
    /// current one.
    fn context(&self) -> Option<String> {
        self.selection.context.clone().or_else(|| self.kubeconfig.as_ref().and_then(|k| k.current.clone()))
    }

    /// The namespace in use; none: all of them.
    fn namespace(&self) -> Option<String> {
        match &self.selection.namespace {
            Ns::Named(n) => Some(n.clone()),
            Ns::All => None,
            Ns::Default => Some(self.context().and_then(|c| self.kubeconfig.as_ref().and_then(|k| k.context(&c)).and_then(|c| c.namespace.clone())).unwrap_or_else(|| "default".into())),
        }
    }

    fn protected(&self, context: &str) -> bool {
        kubectl::protected(context, &protected_patterns())
    }

    fn header(&self) -> Header {
        let context = self.context();
        let versions = match (&self.client, &self.server) {
            (Some(c), Server::Up(s)) => format!("kubectl {c} · server {s}"),
            (Some(c), _) => format!("kubectl {c}"),
            (None, _) => String::new(),
        };
        let problem = if let Some(why) = &self.missing {
            Some((format!("kubectl can't run: {why}"), Some("install kubectl, or set kubernetes.kubectl to where it is")))
        } else if self.kubeconfig.as_ref().is_some_and(|k| k.contexts.is_empty()) && self.selection.context.is_none() {
            Some(("kubeconfig has no contexts: there's no ~/.kube/config, or KUBECONFIG names none.".to_string(), Some("set one up with your cluster's tools (kind create cluster, aws eks update-kubeconfig, gcloud container clusters get-credentials)")))
        } else if let Server::Down(why) = &self.server {
            Some((why.clone(), model::hint(why)))
        } else {
            None
        };
        Header {
            protected: context.as_deref().is_some_and(|c| self.protected(c)),
            chosen: self.selection.context.is_some(),
            context,
            namespace: self.namespace().unwrap_or_else(|| "all namespaces".into()),
            versions,
            problem,
            all_namespaces: self.selection.namespace == Ns::All,
        }
    }

    // -- Processes -----------------------------------------------------------

    fn spawn(&mut self, job: Job, args: Vec<String>, stdin: Option<&str>) -> Option<u64> {
        let cmd = api::Command::new(&self.program).args(args);
        match api::host::spawn(&cmd) {
            Ok(p) => {
                if let Some(text) = stdin {
                    if let Err(why) = api::process::write(p, text.as_bytes()) {
                        api::warn!("kubectl's input: {why}");
                    }
                }
                // Nothing reads from you: a credential plugin that asks is
                // told there's nobody there, rather than waiting forever.
                api::process::close_stdin(p);
                self.jobs.insert(p, Running { job: Some(job), ..Running::default() });
                Some(p)
            }
            Err(why) => {
                api::warn!("{}: {why}", self.program);
                self.missing = Some(why.clone());
                self.doctor();
                self.cannot_run(job, why);
                None
            }
        }
    }

    /// A job whose process couldn't start.
    fn cannot_run(&mut self, job: Job, why: String) {
        match job {
            Job::List { kind, .. } => self.listing_failed(kind, why),
            Job::Agent { call, .. } => {
                self.agent_calls.remove(&call);
                api::answer(call, Err(format!("kubectl can't run: {why}")));
            }
            Job::Logs { page } => {
                if let Some(v) = self.logs.get_mut(&page) {
                    v.stream = Stream::Ended(why);
                }
                pages::refresh(page);
            }
            _ => api::notify(&format!("kubectl can't run: {why}")),
        }
        self.refresh_main();
    }

    fn refresh_main(&self) {
        if let Some((p, _)) = &self.main {
            pages::refresh(*p);
        }
    }

    /// What kubectl's answer means, by what it was for.
    fn ended(&mut self, process: u64, code: Option<i32>) {
        if let Some(tail) = self.agent_tails.remove(&process) {
            if let Some(Running { job: Some(Job::Agent { call, .. }), err, out, .. }) = self.jobs.remove(&process) {
                if let Some(c) = self.agent_calls.remove(&call) {
                    if code == Some(0) {
                        api::answer(call, Ok(tail.answer(&c.context, &c.namespace, ask_pod(&c.ask)).to_string()));
                    } else {
                        api::answer(call, Err(model::failure(&err, &out)));
                    }
                }
            }
            return;
        }
        let Some(Running { job: Some(job), out, err, cut }) = self.jobs.remove(&process) else { return };
        let ok = code == Some(0);
        let why = || model::failure(&err, &out);
        match job {
            Job::ClientVersion => {
                self.client = if ok { model::versions(&out.join("\n")).0 } else { None };
                if !ok {
                    self.missing = Some(why());
                }
                self.doctor();
            }
            Job::Config => {
                // No kubeconfig at all is no contexts, not a failure.
                self.kubeconfig = Some(if ok { Kubeconfig::read(&out.join("\n")) } else { Kubeconfig::default() });
                if !ok {
                    api::warn!("kubectl config view: {}", why());
                }
                self.doctor();
                if std::mem::take(&mut self.refresh_waiting) {
                    self.refresh(true);
                }
                for (call, ask) in std::mem::take(&mut self.agent_waiting) {
                    if let Some(answer) = self.agent_start(call, ask) {
                        api::answer(call, answer);
                    }
                }
                self.refresh_main();
            }
            Job::ServerVersion => {
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
                        items.extend(names.into_iter().map(|n| {
                            let item = PickerItem::new(n.clone(), n.clone());
                            if current.as_deref() == Some(n.as_str()) {
                                item.detail("in use")
                            } else {
                                item
                            }
                        }));
                        let id = api::pick("Namespace", &items);
                        self.pickers.insert(id, Picking::Namespace);
                    }
                    _ => {
                        // Not allowed to list them (or no answer): type one.
                        api::notify(&format!("Couldn't list the namespaces ({}): type one", why()));
                        let id = api::prompt("Namespace", &self.namespace().unwrap_or_default());
                        self.prompts.insert(id, Asking::Namespace);
                    }
                }
            }
            Job::Yaml { target, editable, context } => {
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
                } else if editable {
                    let id = api::scratch::open(&title, &out.join("\n"), Some("yaml"));
                    self.yaml.insert(id, (target, context));
                    self.focused_scratch = Some(id);
                    api::notify(&format!("Edit it, then {} applies it, after a dry run, a diff and a question", key_for("kubernetes.apply", "SPC o k a")));
                } else {
                    api::open_text(&title, &out.join("\n"), Some("yaml"));
                }
            }
            Job::Describe { target, context } => {
                if ok {
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
                    let said = out.iter().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("done");
                    api::notify(&format!("{what}: {said}"));
                } else {
                    api::notify(&format!("Couldn't {what}: {}", why()));
                }
                self.failed = false;
                self.refresh(false);
            }
            Job::DryRun { page } => {
                if let Some(v) = self.applies.get_mut(&page) {
                    v.dry_run_ended(code, &out.join("\n"), why());
                }
                pages::refresh(page);
            }
            Job::Diff { page } => {
                if let Some(v) = self.applies.get_mut(&page) {
                    v.diff_ended(code, out.clone(), why());
                }
                pages::refresh(page);
            }
            Job::Apply { page } => {
                if let Some(v) = self.applies.get_mut(&page) {
                    v.stage = if ok { Stage::Applied(out.iter().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("done").to_string()) } else { Stage::Failed(why()) };
                }
                pages::refresh(page);
                self.refresh(false);
            }
            Job::UseContext(name) => {
                if ok {
                    api::notify(&format!("kubeconfig's current context is now {name}, for every tool that reads it"));
                    self.spawn(Job::Config, kubectl::config_view(), None);
                } else {
                    api::notify(&format!("Couldn't switch kubeconfig to {name}: {}", why()));
                }
            }
            Job::Agent { call, part } => self.agent_part(call, part, ok, &out, why()),
        }
    }

    // -- Lists -----------------------------------------------------------------

    fn listing_mut<T>(list: &mut Listing<T>) -> &mut Listing<T> {
        list
    }

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
    fn list(&mut self, kind: Kind) {
        if self.is_loading(kind) || self.missing.is_some() {
            return;
        }
        let limit = self.set_loading(kind, true);
        let (context, namespace) = (self.context(), self.namespace());
        let args = match kind {
            Kind::Secret => kubectl::secrets(context.as_deref(), namespace.as_deref()),
            k => kubectl::list(k, context.as_deref(), namespace.as_deref(), limit),
        };
        self.spawn(Job::List { kind, generation: self.generation }, args, None);
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
                let w = Self::listing_mut(c.workloads.entry(k).or_default());
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
        if whole {
            self.failed = false;
            self.server = Server::Checking;
            let context = self.context();
            self.spawn(Job::ServerVersion, kubectl::server_version(context.as_deref()), None);
        }
        for kind in tab.kinds() {
            self.list(*kind);
        }
        self.refresh_main();
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

    /// Where it works changed: what was listed goes, and is asked for
    /// again.
    fn select(&mut self, selection: Selection) {
        if selection == self.selection {
            return;
        }
        self.selection = selection;
        self.generation += 1;
        // Lists still coming are for the old selection.
        let old: Vec<u64> = self.jobs.iter().filter(|(_, r)| matches!(r.job, Some(Job::List { .. }) | Some(Job::ServerVersion))).map(|(p, _)| *p).collect();
        for p in old {
            api::process::kill(p);
            self.jobs.remove(&p);
        }
        self.cluster = Cluster::default();
        self.server = Server::Unknown;
        self.failed = false;
        self.asked = None;
        if let Some((_, v)) = &mut self.main {
            v.confirm = None;
            v.menu = None;
        }
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
            (None, Some(v), _) => match &self.kubeconfig {
                Some(k) if k.contexts.is_empty() => (Health::Warn, format!("{v}; kubeconfig has no contexts")),
                _ => (Health::Ok, format!("{v}; the cluster ({}) is asked when the Kubernetes page opens", self.context().unwrap_or_else(|| "kubeconfig's current".into()))),
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

    fn pick_context(&mut self, global: bool) {
        let Some(k) = &self.kubeconfig else {
            api::notify("kubeconfig isn't read yet: a moment");
            return;
        };
        if k.contexts.is_empty() {
            api::notify("kubeconfig has no contexts");
            return;
        }
        let patterns = protected_patterns();
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
                if kubectl::protected(&c.name, &patterns) {
                    detail.push("protected".into());
                }
                PickerItem::new(c.name.clone(), c.name.clone()).detail(detail.join(" · "))
            })
            .collect();
        let title = if global { "kubectl config use-context: every tool's current context" } else { "Context, for this project" };
        let id = api::pick(title, &items);
        self.pickers.insert(id, Picking::Context { global });
    }

    fn pick_namespace(&mut self) {
        if self.missing.is_some() {
            return;
        }
        let context = self.context();
        self.spawn(Job::Namespaces { generation: self.generation }, kubectl::namespaces(context.as_deref()), None);
    }

    fn act(&mut self, page: u64, action: Action) {
        match action {
            Action::Nothing => return,
            Action::Redraw => {}
            Action::Refresh => {
                // Asked by you: a failure no longer holds the timer back.
                self.failed = false;
                let whole = !matches!(self.server, Server::Up(_));
                self.refresh(whole);
            }
            Action::PickContext => self.pick_context(false),
            Action::PickNamespace => self.pick_namespace(),
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
                    self.list(kind);
                }
            }
            Action::Logs(t) => self.open_logs(t),
            Action::Shell(t) => self.shell(t),
            Action::Forward(t) => {
                let port = match t.kind {
                    Kind::Service => self.cluster.service(&t).and_then(|s| s.first_port),
                    Kind::Pod => self.cluster.pod(&t.namespace, &t.name).and_then(|p| p.ports.first().copied()),
                    _ => None,
                };
                let initial = port.map(|p| format!("{}:{p}", if p < 1024 { 8000 + p } else { p })).unwrap_or_else(|| "8080:80".into());
                let id = api::prompt(&format!("Port-forward {} ({}/{}): local:remote", t.reference(), self.context().unwrap_or_default(), t.namespace), &initial);
                self.prompts.insert(id, Asking::Ports(t));
            }
            Action::Yaml(t) | Action::Edit(t) if self.context().is_none() => {
                let _ = t;
                api::notify("Which cluster? kubeconfig has no current context: c chooses one");
            }
            Action::Yaml(t) => {
                let context = self.context().unwrap_or_default();
                self.spawn(Job::Yaml { target: t.clone(), editable: false, context: context.clone() }, kubectl::get(&t, Some(&context)), None);
            }
            Action::Edit(t) => {
                let context = self.context().unwrap_or_default();
                self.spawn(Job::Yaml { target: t.clone(), editable: true, context: context.clone() }, kubectl::get(&t, Some(&context)), None);
            }
            Action::Describe(t) => {
                let context = self.context().unwrap_or_default();
                let args = kubectl::describe(&t, self.context().as_deref());
                self.spawn(Job::Describe { target: t, context }, args, None);
            }
            Action::Copy(name) => {
                api::copy(&name);
                api::notify(&format!("copied {name}"));
            }
            Action::Mutate(m) => self.ask_change(m),
            Action::Scale(t) => {
                let from = self.cluster.workload(&t).and_then(|w| w.replicas);
                let id = api::prompt(&format!("Scale {t} ({}/{}) to how many replicas?", self.context().unwrap_or_default(), t.namespace), &from.map(|f| f.to_string()).unwrap_or_default());
                self.prompts.insert(id, Asking::Replicas(t, from));
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
                api::notify(&format!("{context} is protected (kubernetes.protected): {} refused here. Nothing changed.", m.what()));
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
        match &m {
            Mutation::Apply { page, .. } => {
                if let Some(v) = self.applies.get_mut(page) {
                    v.confirm = Some(question);
                }
                pages::refresh(*page);
            }
            _ => {
                if let Some((p, v)) = &mut self.main {
                    v.confirm = Some((m, question));
                    pages::refresh(*p);
                }
            }
        }
    }

    /// `y` to a question: done, if it's still the one asked, in the same
    /// context.
    fn confirmed(&mut self, m: Mutation) {
        match self.asked.take() {
            Some((asked, context)) if asked == m && self.context().as_deref() == Some(context.as_str()) => self.change(m, &context),
            _ => api::notify("The context changed since it was asked: nothing done"),
        }
    }

    fn change(&mut self, m: Mutation, context: &str) {
        let what = m.what();
        match m {
            Mutation::Delete(t) => {
                self.spawn(Job::Change { what }, kubectl::delete(&t, Some(context)), None);
            }
            Mutation::Restart(t) => {
                self.spawn(Job::Change { what }, kubectl::rollout_restart(&t, Some(context)), None);
            }
            Mutation::Scale { target, from, to } => {
                self.spawn(Job::Change { what }, kubectl::scale(&target, Some(context), from, to), None);
            }
            Mutation::Apply { page, target } => {
                let Some(v) = self.applies.get_mut(&page) else { return };
                if v.context.as_deref() != Some(context) {
                    api::notify("The context changed since the dry run: r checks it again");
                    return;
                }
                v.stage = Stage::Applying;
                let text = v.text.clone();
                self.spawn(Job::Apply { page }, kubectl::apply(&target.namespace, Some(context)), Some(&text));
                pages::refresh(page);
            }
        }
    }

    // -- Logs, shells, forwards -----------------------------------------------------

    fn open_logs(&mut self, t: Target) {
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
        let view = LogView::new(t.clone(), self.context(), containers, container);
        let page = pages::open(LOGS, &format!("logs {}", t.name));
        self.logs.insert(page, view);
        self.stream(page);
    }

    /// The log page's stream, (re)started with its options.
    fn stream(&mut self, page: u64) {
        self.stop_stream(page);
        let Some(v) = self.logs.get_mut(&page) else { return };
        if v.stream == Stream::Starting || matches!(v.stream, Stream::Stopped | Stream::Ended(_)) {
            v.stream = Stream::Running;
        }
        let args = v.args();
        self.spawn(Job::Logs { page }, args, None);
        pages::refresh(page);
    }

    fn stop_stream(&mut self, page: u64) {
        let running: Vec<u64> = self.jobs.iter().filter(|(_, r)| r.job == Some(Job::Logs { page })).map(|(p, _)| *p).collect();
        for p in running {
            api::process::kill(p);
            self.jobs.remove(&p);
        }
    }

    fn shell(&mut self, t: Target) {
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
        let context = self.context();
        let cmd = api::Command::new(&self.program).args(kubectl::exec(t, context.as_deref(), &container));
        let title = format!("{}/{container} ({}/{})", t.name, context.unwrap_or_default(), t.namespace);
        if let Err(why) = api::host::run_in_terminal(&cmd, &title, false) {
            api::notify(&format!("Couldn't open a terminal: {why}"));
        }
    }

    fn forward(&mut self, t: Target, local: u16, remote: u16) {
        if let Some(f) = self.forwards.iter().find(|f| f.local == local) {
            api::notify(&format!("localhost:{local} is already forwarded to {}: s on the Forwards tab stops it", f.target.reference()));
            return;
        }
        let context = self.context();
        let cmd = api::Command::new(&self.program).args(kubectl::port_forward(&t, context.as_deref(), local, remote));
        let name = format!("port-forward {} {local}:{remote}", t.reference());
        match api::host::run_task_background(&name, &cmd) {
            Ok(task) => {
                api::notify(&format!("localhost:{local} → {} {remote}, while the task runs (the Forwards tab, 5)", t.reference()));
                self.forwards.push(Forward { task, target: t, context: context.unwrap_or_else(|| "kubeconfig's context".into()), local, remote });
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

    // -- Apply -----------------------------------------------------------------------

    fn apply_command(&mut self) {
        self.yaml.retain(|id, _| api::scratch::text(*id).is_some());
        let chosen = match self.focused_scratch.filter(|s| self.yaml.contains_key(s)) {
            Some(s) => Some(s),
            None if self.yaml.len() == 1 => self.yaml.keys().next().copied(),
            None if self.yaml.is_empty() => {
                api::notify(&format!("No YAML open to apply: e on an object (its menu: Edit the YAML) opens one, then {}", key_for("kubernetes.apply", "SPC o k a")));
                return;
            }
            None => {
                let items: Vec<PickerItem> = self.yaml.iter().map(|(id, (t, c))| PickerItem::new(format!("{} ({c}/{})", t.reference(), t.namespace), id.to_string())).collect();
                let id = api::pick("Apply which YAML", &items);
                self.pickers.insert(id, Picking::YamlBuffer);
                return;
            }
        };
        if let Some(s) = chosen {
            self.check_apply(s, None);
        }
    }

    /// The buffer's text read, and checked: a dry run and a diff, on a
    /// page of their own (`page`, or a new one).
    fn check_apply(&mut self, scratch: u64, page: Option<u64>) {
        let Some((target, context)) = self.yaml.get(&scratch).cloned() else { return };
        let Some(text) = api::scratch::text(scratch) else {
            self.yaml.remove(&scratch);
            api::notify("That YAML buffer is closed");
            return;
        };
        if self.context().as_deref() != Some(context.as_str()) {
            api::notify(&format!("This YAML is from {context}, and the page is on {} now: c chooses {context} again to apply it", self.context().unwrap_or_default()));
            return;
        }
        let page = page.unwrap_or_else(|| pages::open(APPLY, &format!("apply {}", target.reference())));
        self.applies.insert(page, ApplyView::new(scratch, target.clone(), Some(context.clone()), text.clone()));
        self.spawn(Job::DryRun { page }, kubectl::apply_dry_run(&target.namespace, Some(&context)), Some(&text));
        self.spawn(Job::Diff { page }, kubectl::diff(&target.namespace, Some(&context)), Some(&text));
        pages::refresh(page);
    }

    // -- Agents ------------------------------------------------------------------------

    fn agent_start(&mut self, call: u64, ask: Ask) -> Option<Result<String, String>> {
        if let Some(why) = &self.missing {
            return Some(Err(format!("kubectl can't run here: {why}")));
        }
        let context = self.context();
        let namespace = ask.namespace().map(String::from).or_else(|| self.namespace());
        let shown_context = context.clone().unwrap_or_else(|| "kubeconfig's current context".into());
        let ns_for_one = match (&namespace, &ask) {
            (None, Ask::Workloads { .. } | Ask::Pods { .. }) => None,
            (None, _) => return Some(Err("the extension shows every namespace: say which namespace the pod or object is in".into())),
            (Some(n), _) => Some(n.clone()),
        };
        let mut c = AgentCall { ask: ask.clone(), context: shown_context, namespace: namespace.clone().unwrap_or_else(|| "all".into()), waiting: 0, workloads: Vec::new(), pods: None };
        let ctx = context.as_deref();
        let ns = namespace.as_deref();
        match &ask {
            Ask::Workloads { .. } => {
                c.waiting = Kind::WORKLOADS.len();
                self.agent_calls.insert(call, c);
                for kind in Kind::WORKLOADS {
                    self.spawn(Job::Agent { call, part: Some(kind) }, kubectl::list(kind, ctx, ns, agent::MAX_LISTED), None);
                }
            }
            Ask::Pods { .. } => {
                c.waiting = 1;
                self.agent_calls.insert(call, c);
                self.spawn(Job::Agent { call, part: Some(Kind::Pod) }, kubectl::list(Kind::Pod, ctx, ns, agent::MAX_LISTED), None);
            }
            Ask::Logs { lines, .. } => {
                let n = ns_for_one.unwrap_or_default();
                c.waiting = 1;
                c.namespace = n.clone();
                self.agent_calls.insert(call, c);
                if let Some(p) = self.spawn(Job::Agent { call, part: None }, agent::command(&ask, ctx, &n), None) {
                    self.agent_tails.insert(p, Tail::new(*lines));
                }
            }
            Ask::Pod { .. } | Ask::Describe { .. } | Ask::Events { .. } => {
                let n = ns_for_one.unwrap_or_default();
                c.waiting = 1;
                c.namespace = n.clone();
                self.agent_calls.insert(call, c);
                self.spawn(Job::Agent { call, part: None }, agent::command(&ask, ctx, &n), None);
            }
        }
        None
    }

    fn agent_part(&mut self, call: u64, part: Option<Kind>, ok: bool, out: &[String], why: String) {
        let Some(c) = self.agent_calls.get_mut(&call) else { return };
        let text = out.join("\n");
        c.waiting = c.waiting.saturating_sub(1);
        match part {
            Some(Kind::Pod) => c.pods = Some(if ok { model::pods(&text) } else { Err(why) }),
            Some(kind) => c.workloads.push((kind, if ok { model::workloads(kind, &text) } else { Err(why) })),
            None => {
                let answer = if !ok {
                    Err(why)
                } else {
                    match &c.ask {
                        Ask::Pod { .. } => serde_json::from_str::<model::RawPod>(&text).map(|p| agent::pod_answer(&c.context, &p, now()).to_string()).map_err(|e| format!("kubectl's answer wasn't a pod: {e}")),
                        Ask::Events { .. } => model::read_list::<model::RawEvent>(&text).map(|l| agent::events_answer(&c.context, &c.namespace, l.items, &l.more).to_string()),
                        Ask::Describe { kind, name, .. } => Ok(agent::describe_answer(&c.context, &c.namespace, kind, name, out).to_string()),
                        _ => Err("unexpected".into()),
                    }
                };
                self.agent_calls.remove(&call);
                api::answer(call, answer);
                return;
            }
        }
        if c.waiting > 0 {
            return;
        }
        let Some(c) = self.agent_calls.remove(&call) else { return };
        let answer = match &c.ask {
            Ask::Pods { .. } => match c.pods {
                Some(Ok(l)) => Ok(agent::pods_answer(&c.context, &c.namespace, &l.items, &l.more, now()).to_string()),
                Some(Err(e)) => Err(e),
                None => Err("kubectl said nothing".into()),
            },
            _ => {
                if let Some((_, Err(e))) = c.workloads.iter().find(|(_, r)| r.is_err()) {
                    Err(e.clone())
                } else {
                    let mut lists: Vec<(Kind, model::Listed<model::Workload>)> = c.workloads.into_iter().filter_map(|(k, r)| r.ok().map(|l| (k, l))).collect();
                    lists.sort_by_key(|(k, _)| *k);
                    let refs: Vec<(Kind, &[model::Workload], &More)> = lists.iter().map(|(k, l)| (*k, l.items.as_slice(), &l.more)).collect();
                    Ok(agent::workloads_answer(&c.context, &c.namespace, &refs, now()).to_string())
                }
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

fn ask_pod(ask: &Ask) -> &str {
    match ask {
        Ask::Logs { pod, .. } => pod,
        _ => "",
    }
}

fn fill<T>(list: &mut Listing<T>, l: model::Listed<T>) {
    list.items = l.items;
    list.more = l.more;
    list.cont = l.cont;
    list.error = None;
    list.loading = false;
    list.loaded = true;
}

/// The keys that run `command` now, else `vim` with the leader of the
/// scheme in use.
fn key_for(command: &str, vim: &str) -> String {
    api::key_for(command).unwrap_or_else(|| match (api::key_scheme(), vim.strip_prefix("SPC ")) {
        (api::KeyScheme::Standard, Some(rest)) => format!("C-k {rest}"),
        _ => vim.to_string(),
    })
}

impl Extension for Kube {
    fn activate(&mut self) -> Result<(), String> {
        self.program = setting("kubernetes.kubectl").unwrap_or_else(|| "kubectl".into());
        self.selection = Selection {
            context: setting("kubernetes.context"),
            namespace: match setting("kubernetes.namespace").as_deref() {
                None => Ns::Default,
                Some("*") | Some("all") => Ns::All,
                Some(n) => kubectl::label(n).map(Ns::Named).unwrap_or(Ns::Default),
            },
        };
        // Only what's on this machine: kubectl's version and kubeconfig's
        // contexts. The cluster is asked when the page opens.
        self.spawn(Job::ClientVersion, kubectl::client_version(), None);
        self.spawn(Job::Config, kubectl::config_view(), None);
        self.doctor();
        Ok(())
    }

    fn run_command(&mut self, id: &str) -> Result<(), String> {
        match id {
            "kubernetes.open" => self.open_main(),
            "kubernetes.context" => self.pick_context(false),
            "kubernetes.use_context" => self.pick_context(true),
            "kubernetes.namespace" => self.pick_namespace(),
            "kubernetes.apply" => self.apply_command(),
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
            Event::ProcessOutput(o) => {
                if let Some(tail) = self.agent_tails.get_mut(&o.process) {
                    if !o.stderr {
                        tail.push(&o.line);
                        return;
                    }
                }
                let cap = self.cluster.secrets.limit.max(max_items());
                let Some(run) = self.jobs.get_mut(&o.process) else { return };
                match &run.job {
                    Some(Job::Logs { page }) => {
                        let page = *page;
                        if let Some(v) = self.logs.get_mut(&page) {
                            if o.stderr {
                                run.err.push(o.line);
                            } else {
                                v.push(&o.line);
                            }
                        }
                        pages::refresh(page);
                    }
                    Some(Job::List { kind: Kind::Secret, .. }) if !o.stderr => {
                        run.out.push(o.line);
                        if run.out.len() > cap && !run.cut {
                            // Past the cap: the rest isn't read.
                            run.out.truncate(cap);
                            run.cut = true;
                            api::process::kill(o.process);
                        }
                    }
                    _ if o.stderr => run.err.push(o.line),
                    _ if run.cut => {}
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
                    self.program = setting("kubernetes.kubectl").unwrap_or_else(|| "kubectl".into());
                    self.missing = None;
                    self.spawn(Job::ClientVersion, kubectl::client_version(), None);
                    self.spawn(Job::Config, kubectl::config_view(), None);
                }
                "kubernetes.refresh_seconds" => {
                    self.stop_timer();
                    if self.main_shown && self.main.is_some() {
                        self.start_timer();
                    }
                }
                "kubernetes.context" | "kubernetes.namespace" => {
                    let mut s = self.selection.clone();
                    if key == "kubernetes.context" {
                        s.context = setting("kubernetes.context");
                    } else {
                        s.namespace = match setting("kubernetes.namespace").as_deref() {
                            None => Ns::Default,
                            Some("*") | Some("all") => Ns::All,
                            Some(n) => kubectl::label(n).map(Ns::Named).unwrap_or(Ns::Default),
                        };
                    }
                    self.select(s);
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
        if self.kubeconfig.is_none() && self.missing.is_none() {
            // kubeconfig's contexts aren't read yet: answered once they are.
            self.agent_waiting.push((call, ask));
            return None;
        }
        self.agent_start(call, ask)
    }

    fn service_cancelled(&mut self, call: u64) {
        self.agent_waiting.retain(|(c, _)| *c != call);
        self.agent_calls.remove(&call);
        let running: Vec<u64> = self.jobs.iter().filter(|(_, r)| matches!(r.job, Some(Job::Agent { call: c, .. }) if c == call)).map(|(p, _)| *p).collect();
        for p in running {
            api::process::kill(p);
            self.jobs.remove(&p);
            self.agent_tails.remove(&p);
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
        if let Some(v) = self.applies.get(&page) {
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
            return;
        }
        if let Some(v) = self.applies.get_mut(&page) {
            match v.key(key.into()) {
                ApplyAction::Nothing => return,
                ApplyAction::Redraw => {}
                ApplyAction::Ask => {
                    let m = Mutation::Apply { page, target: v.target.clone() };
                    self.ask_change(m);
                }
                ApplyAction::Declined => self.asked = None,
                ApplyAction::Confirmed => {
                    let m = Mutation::Apply { page, target: v.target.clone() };
                    self.confirmed(m);
                }
                ApplyAction::Recheck => {
                    let scratch = v.scratch;
                    self.check_apply(scratch, Some(page));
                }
            }
            pages::refresh(page);
        }
    }

    fn page_closed(&mut self, page: u64) {
        if self.main.as_ref().is_some_and(|(p, _)| *p == page) {
            self.main = None;
            self.main_shown = false;
            self.stop_timer();
        }
        if self.logs.remove(&page).is_some() {
            self.stop_stream(page);
        }
        self.applies.remove(&page);
        self.heights.remove(&page);
    }

    fn scratch_closed(&mut self, scratch: u64) {
        self.yaml.remove(&scratch);
        if self.focused_scratch == Some(scratch) {
            self.focused_scratch = None;
        }
    }

    fn focus_moved(&mut self, focus: Focus) {
        if let Focus::Text(Some(id)) = focus {
            if self.yaml.contains_key(&id) {
                self.focused_scratch = Some(id);
            }
        }
    }

    /// The main page keeps where it was: the context, the namespace, the
    /// tab -- this project's choice, kept with its session.
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
        self.selection = Selection { context: context.or(self.selection.context.clone()), namespace };
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
            Picking::Context { global } => {
                let Ok(name) = kubectl::context(&value) else { return };
                if global {
                    self.spawn(Job::UseContext(name.clone()), kubectl::use_context(&name), None);
                }
                self.select(Selection { context: Some(name), namespace: Ns::Default });
            }
            Picking::Namespace => {
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
            Picking::YamlBuffer => {
                if let Ok(id) = value.parse() {
                    self.check_apply(id, None);
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
