//! The Kubernetes page: the context and namespace at its top, then five
//! tabs -- workloads as a tree (each one's pods under it), pods as a
//! table, services and ingresses, Secrets (their names, never their
//! values) and the port-forwards running -- drawn with ui-kit. A key is
//! answered with an `Action` for `lib.rs` to carry out, so the page itself
//! runs nothing and is tested as it is.

use std::borrow::Cow;
use std::collections::BTreeMap;

use fenix_ui_kit::text::{fit, width};
use fenix_ui_kit::{Canvas, Column, Key, Outcome, Popup, Rect, Role, Style, Tab, TableSource, TableState, TabsState, TreeNode, TreeState, Width};

use crate::api::chrono::{DateTime, Utc};
use crate::kubectl::{Kind, Target};
use crate::model::{self, Health, Ingress, More, Pod, Secret, Service, Workload};

/// One kind's list, as the page has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing<T> {
    pub items: Vec<T>,
    pub more: More,
    pub cont: Option<String>,
    /// Why the last listing failed, in kubectl's words.
    pub error: Option<String>,
    pub loading: bool,
    /// Listed at least once since the selection changed.
    pub loaded: bool,
    /// How many to ask for: the cap, raised by `+`.
    pub limit: usize,
}

impl<T> Default for Listing<T> {
    fn default() -> Listing<T> {
        Listing { items: Vec::new(), more: More::None, cont: None, error: None, loading: false, loaded: false, limit: 0 }
    }
}

/// What the cluster said, kind by kind.
#[derive(Debug, Clone, Default)]
pub struct Cluster {
    pub workloads: BTreeMap<Kind, Listing<Workload>>,
    pub pods: Listing<Pod>,
    pub services: Listing<Service>,
    pub ingresses: Listing<Ingress>,
    pub secrets: Listing<Secret>,
}

impl Cluster {
    pub fn pod(&self, namespace: &str, name: &str) -> Option<&Pod> {
        self.pods.items.iter().find(|p| p.namespace == namespace && p.name == name)
    }

    pub fn workload(&self, t: &Target) -> Option<&Workload> {
        self.workloads.get(&t.kind)?.items.iter().find(|w| w.namespace == t.namespace && w.name == t.name)
    }

    pub fn service(&self, t: &Target) -> Option<&Service> {
        self.services.items.iter().find(|s| s.namespace == t.namespace && s.name == t.name)
    }

    /// A kind's list's state: its error, what's past it, how many are
    /// listed, whether it's being asked for.
    pub fn state(&self, kind: Kind) -> (Option<&str>, &More, usize, bool) {
        fn of<T>(l: &Listing<T>) -> (Option<&str>, &More, usize, bool) {
            (l.error.as_deref(), &l.more, l.items.len(), l.loading)
        }
        const NONE: &More = &More::None;
        match kind {
            Kind::Pod => of(&self.pods),
            Kind::Service => of(&self.services),
            Kind::Ingress => of(&self.ingresses),
            Kind::Secret => of(&self.secrets),
            k => self.workloads.get(&k).map(of).unwrap_or((None, NONE, 0, false)),
        }
    }
}

/// A port-forward running, as a background task.
#[derive(Debug, Clone, PartialEq)]
pub struct Forward {
    pub task: u64,
    pub target: Target,
    pub context: String,
    pub local: u16,
    pub remote: u16,
}

impl Forward {
    pub fn label(&self) -> String {
        format!("{} {}:{}", self.target.reference(), self.local, self.remote)
    }
}

/// What the page's top line says.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Header {
    /// The context in use, by name; none when kubeconfig has none.
    pub context: Option<String>,
    /// Where the context comes from: chosen here, or kubeconfig's current
    /// one as read at a time.
    pub origin: String,
    /// A context and namespace a setting suggests (yours, or the
    /// project's), not used until you take it (`u`).
    pub suggestion: Option<String>,
    pub namespace: String,
    pub protected: bool,
    /// kubectl's version, and the server's.
    pub versions: String,
    /// What stops the page: no kubectl, no kubeconfig, an unreachable
    /// cluster -- kubectl's words, and a hint.
    pub problem: Option<(String, Option<&'static str>)>,
    pub all_namespaces: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabId {
    Workloads,
    Pods,
    Network,
    Secrets,
    Forwards,
}

impl TabId {
    pub const ALL: [TabId; 5] = [TabId::Workloads, TabId::Pods, TabId::Network, TabId::Secrets, TabId::Forwards];

    /// The kinds it lists, and so asks for again.
    pub fn kinds(self) -> &'static [Kind] {
        match self {
            TabId::Workloads => &[Kind::Deployment, Kind::StatefulSet, Kind::DaemonSet, Kind::Job, Kind::CronJob, Kind::Pod],
            TabId::Pods => &[Kind::Pod],
            TabId::Network => &[Kind::Service, Kind::Ingress],
            TabId::Secrets => &[Kind::Secret],
            TabId::Forwards => &[],
        }
    }

    fn title(self) -> &'static str {
        match self {
            TabId::Workloads => "Workloads",
            TabId::Pods => "Pods",
            TabId::Network => "Network",
            TabId::Secrets => "Secrets",
            TabId::Forwards => "Forwards",
        }
    }
}

/// Something changing the cluster, waiting for you to say yes.
#[derive(Debug, Clone, PartialEq)]
pub enum Mutation {
    Delete(Target),
    Restart(Target),
    Scale { target: Target, from: Option<u32>, to: u32 },
}

impl Mutation {
    pub fn target(&self) -> &Target {
        match self {
            Mutation::Delete(t) | Mutation::Restart(t) => t,
            Mutation::Scale { target, .. } => target,
        }
    }

    /// What it does, in a few words: `delete pod web-1`.
    pub fn what(&self) -> String {
        match self {
            Mutation::Delete(t) => format!("delete {t}"),
            Mutation::Restart(t) => format!("restart {t} (rollout restart)"),
            Mutation::Scale { target, from: Some(f), to } => format!("scale {target} from {f} to {to}"),
            Mutation::Scale { target, from: None, to } => format!("scale {target} to {to}"),
        }
    }

    /// The question, naming the context and the namespace.
    pub fn question(&self, context: &str) -> String {
        format!("{}? context {context}, namespace {}", capital(&self.what()), self.target().namespace)
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// What a key on the page asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Nothing,
    Redraw,
    Refresh,
    PickContext,
    PickNamespace,
    Filter,
    /// More of a kind (`+`): the cap raised.
    More(Kind),
    Logs(Target),
    Shell(Target),
    Forward(Target),
    Yaml(Target),
    /// The suggested context and namespace taken.
    TakeSuggestion,
    Describe(Target),
    Copy(String),
    /// A change asked for: `lib.rs` confirms it, or refuses it.
    Mutate(Mutation),
    /// Asks how many replicas.
    Scale(Target),
    /// You said yes.
    Confirmed(Mutation),
    /// You didn't.
    Declined,
    StopForward(u64),
    ShowForward(u64),
    OpenUrl(String),
    /// Something it can't do there, said.
    Say(String),
}

/// The main page's state.
#[derive(Debug, Clone)]
pub struct MainView {
    pub tabs: TabsState,
    pub workloads: TreeState,
    pub pods: TableState,
    pub network: TreeState,
    pub secrets: TreeState,
    pub forwards: TreeState,
    pub filter: String,
    /// The menu, open, its row picked.
    pub menu: Option<usize>,
    /// A change waiting for `y`.
    pub confirm: Option<(Mutation, String)>,
}

impl Default for MainView {
    fn default() -> MainView {
        let mut workloads = TreeState::new();
        for k in Kind::WORKLOADS {
            workloads.expand(&group_id(k));
        }
        let mut network = TreeState::new();
        network.expand(&group_id(Kind::Service));
        network.expand(&group_id(Kind::Ingress));
        let mut pods = TableState::new();
        pods.empty = "No pods".into();
        workloads.empty = "No workloads".into();
        network.empty = "No services or ingresses".into();
        let mut secrets = TreeState::new();
        secrets.empty = "No Secrets".into();
        let mut forwards = TreeState::new();
        forwards.empty = "No port-forwards: f on a pod or a service starts one".into();
        MainView { tabs: TabsState::new(), workloads, pods, network, secrets, forwards, filter: String::new(), menu: None, confirm: None }
    }
}

fn group_id(k: Kind) -> String {
    format!("g|{}", k.singular())
}

fn object_id(k: Kind, ns: &str, name: &str) -> String {
    format!("o|{}|{ns}|{name}", k.singular())
}

/// The object a row's id names: the last part of a child's path.
fn target_of(id: &str) -> Option<Target> {
    let last = id.rsplit('>').next()?;
    let mut parts = last.split('|');
    if parts.next()? != "o" {
        return None;
    }
    let kind = Kind::by_name(parts.next()?)?;
    Target::new(kind, parts.next()?, parts.next()?).ok()
}

fn health_style(h: Option<Health>) -> Style {
    match h {
        Some(Health::Good) => Style::new(Role::Good),
        Some(Health::Warn) => Style::new(Role::Warn),
        Some(Health::Bad) => Style::new(Role::Bad),
        Some(Health::Muted) => Style::new(Role::Muted),
        None => Style::default(),
    }
}

fn matches(filter: &str, ns: &str, name: &str) -> bool {
    filter.is_empty() || name.contains(filter) || ns.contains(filter)
}

/// `text` cut or padded to `w` display cells.
fn pad(text: &str, w: usize) -> String {
    let cut = fit(text, w);
    let room = w.saturating_sub(width(&cut));
    format!("{cut}{}", " ".repeat(room))
}

/// `name` padded to `w`, then `rest`.
fn columns(name: &str, w: usize, rest: &str) -> String {
    format!("{}  {rest}", pad(name, w))
}

fn shown_name(all: bool, ns: &str, name: &str) -> String {
    if all {
        format!("{ns}/{name}")
    } else {
        name.to_string()
    }
}

fn more_line(more: &More, listed: usize, kind: Kind) -> Option<String> {
    match more {
        More::Count(n) => Some(format!("… {n} more {} not listed (the first {listed}): + lists more", kind.resource())),
        More::Some => Some(format!("… more {} not listed (the first {listed}): + lists more", kind.resource())),
        More::None => None,
    }
}

/// The Workloads tab's tree: a branch per kind, its objects, and each
/// one's pods (a cronjob's jobs) under it.
pub fn workload_nodes(c: &Cluster, filter: &str, all: bool, now: DateTime<Utc>) -> Vec<TreeNode> {
    let mut roots = Vec::new();
    for kind in Kind::WORKLOADS {
        let Some(list) = c.workloads.get(&kind) else { continue };
        let items: Vec<&Workload> = list.items.iter().filter(|w| matches(filter, &w.namespace, &w.name)).collect();
        let w = items.iter().map(|w| width(&shown_name(all, &w.namespace, &w.name))).max().unwrap_or(0).min(40);
        let mut children: Vec<TreeNode> = items
            .iter()
            .map(|wl| {
                let id = object_id(kind, &wl.namespace, &wl.name);
                let kids: Vec<TreeNode> = if kind == Kind::CronJob {
                    c.workloads.get(&Kind::Job).map(|jobs| jobs.items.iter().filter(|j| j.namespace == wl.namespace && j.owners.iter().any(|o| o.kind == "CronJob" && o.name == wl.name)).map(|j| TreeNode::leaf(format!("{id}>{}", object_id(Kind::Job, &j.namespace, &j.name)), columns(&j.name, 36, &j.status)).style(health_style(j.health)).detail(model::age(j.created.as_deref(), now))).collect()).unwrap_or_default()
                } else {
                    c.pods.items.iter().filter(|p| wl.owns(p)).map(|p| pod_node(&id, p, now)).collect()
                };
                let label = columns(&shown_name(all, &wl.namespace, &wl.name), w, &wl.status);
                let node = if kids.is_empty() { TreeNode::leaf(id, label) } else { TreeNode::branch(id, label, kids) };
                node.style(health_style(wl.health)).detail(model::age(wl.created.as_deref(), now))
            })
            .collect();
        if let Some(line) = more_line(&list.more, list.items.len(), kind) {
            children.push(TreeNode::leaf(format!("more|{}", kind.singular()), line).style(Role::Muted));
        }
        let detail = match (&list.error, list.loading && !list.loaded) {
            (Some(_), _) => "failed".to_string(),
            (None, true) => "…".to_string(),
            _ => items.len().to_string(),
        };
        roots.push(TreeNode::branch(group_id(kind), kind.title(), children).style(Role::Title).detail(detail));
    }
    roots
}

fn pod_node(parent: &str, p: &Pod, now: DateTime<Utc>) -> TreeNode {
    let text = format!("{}  {} {}/{}  {} restart{}", pad(&p.name, 36), pad(&p.status, 17), p.ready.0, p.ready.1, p.restarts, if p.restarts == 1 { "" } else { "s" });
    TreeNode::leaf(format!("{parent}>{}", object_id(Kind::Pod, &p.namespace, &p.name)), text).style(health_style(Some(p.health()))).detail(model::age(p.created.as_deref(), now))
}

/// The Network tab's tree.
pub fn network_nodes(c: &Cluster, filter: &str, all: bool, now: DateTime<Utc>) -> Vec<TreeNode> {
    let services: Vec<&Service> = c.services.items.iter().filter(|s| matches(filter, &s.namespace, &s.name)).collect();
    let w = services.iter().map(|s| width(&shown_name(all, &s.namespace, &s.name))).max().unwrap_or(0).min(40);
    let mut svc: Vec<TreeNode> = services
        .iter()
        .map(|s| {
            let ext = if s.external.is_empty() { String::new() } else { format!("  → {}", s.external) };
            TreeNode::leaf(object_id(Kind::Service, &s.namespace, &s.name), columns(&shown_name(all, &s.namespace, &s.name), w, &format!("{:<12} {:<15} {}{ext}", s.kind, s.cluster_ip, s.ports))).detail(model::age(s.created.as_deref(), now))
        })
        .collect();
    if let Some(line) = more_line(&c.services.more, c.services.items.len(), Kind::Service) {
        svc.push(TreeNode::leaf("more|service", line).style(Role::Muted));
    }
    let ingresses: Vec<&Ingress> = c.ingresses.items.iter().filter(|i| matches(filter, &i.namespace, &i.name)).collect();
    let w = ingresses.iter().map(|i| width(&shown_name(all, &i.namespace, &i.name))).max().unwrap_or(0).min(40);
    let mut ing: Vec<TreeNode> = ingresses
        .iter()
        .map(|i| {
            let text = format!("{}{}{}", i.hosts, if i.tls { "  tls" } else { "" }, if i.address.is_empty() { String::new() } else { format!("  → {}", i.address) });
            TreeNode::leaf(object_id(Kind::Ingress, &i.namespace, &i.name), columns(&shown_name(all, &i.namespace, &i.name), w, &text)).detail(model::age(i.created.as_deref(), now))
        })
        .collect();
    if let Some(line) = more_line(&c.ingresses.more, c.ingresses.items.len(), Kind::Ingress) {
        ing.push(TreeNode::leaf("more|ingress", line).style(Role::Muted));
    }
    vec![TreeNode::branch(group_id(Kind::Service), "Services", svc).style(Role::Title).detail(services.len().to_string()), TreeNode::branch(group_id(Kind::Ingress), "Ingresses", ing).style(Role::Title).detail(ingresses.len().to_string())]
}

/// The Secrets tab: names, types and how many keys -- no values.
pub fn secret_nodes(c: &Cluster, filter: &str, all: bool) -> Vec<TreeNode> {
    let items: Vec<&Secret> = c.secrets.items.iter().filter(|s| matches(filter, &s.namespace, &s.name)).collect();
    let w = items.iter().map(|s| width(&shown_name(all, &s.namespace, &s.name))).max().unwrap_or(0).min(48);
    let mut out: Vec<TreeNode> = items
        .iter()
        .map(|s| TreeNode::leaf(object_id(Kind::Secret, &s.namespace, &s.name), columns(&shown_name(all, &s.namespace, &s.name), w, &format!("{:<36} {} key{}", s.kind, s.keys, if s.keys == 1 { "" } else { "s" }))).detail(s.age.clone()))
        .collect();
    if let Some(line) = more_line(&c.secrets.more, c.secrets.items.len(), Kind::Secret) {
        out.push(TreeNode::leaf("more|secret", line).style(Role::Muted));
    }
    out
}

pub fn forward_nodes(forwards: &[Forward]) -> Vec<TreeNode> {
    forwards.iter().map(|f| TreeNode::leaf(format!("f|{}", f.task), format!("{}   localhost:{} → {}  in {}/{}", f.label(), f.local, f.remote, f.context, f.target.namespace)).style(Role::Good)).collect()
}

/// The Pods tab's rows.
struct PodRows<'a> {
    pods: Vec<&'a Pod>,
    all: bool,
    now: DateTime<Utc>,
}

impl TableSource for PodRows<'_> {
    fn len(&self) -> usize {
        self.pods.len()
    }

    fn row(&self, i: usize) -> Vec<Cow<'_, str>> {
        let p = self.pods[i];
        let mut row: Vec<Cow<str>> = Vec::new();
        if self.all {
            row.push(Cow::Borrowed(&p.namespace));
        }
        row.extend([Cow::Borrowed(p.name.as_str()), format!("{}/{}", p.ready.0, p.ready.1).into(), Cow::Borrowed(p.status.as_str()), p.restarts.to_string().into(), model::age(p.created.as_deref(), self.now).into(), Cow::Borrowed(p.node.as_str())]);
        row
    }

    fn style(&self, i: usize, col: usize) -> Style {
        if col == usize::from(self.all) + 2 {
            health_style(Some(self.pods[i].health()))
        } else if col == usize::from(self.all) + 3 && self.pods[i].restarts > 0 {
            Style::new(Role::Warn)
        } else {
            Style::default()
        }
    }
}

fn pod_columns(all: bool) -> Vec<Column> {
    let mut cols = Vec::new();
    if all {
        cols.push(Column::fit("Namespace", 24));
    }
    cols.extend([Column::fit("Name", 52), Column::new("Ready", Width::Fixed(5)), Column::fit("Status", 22), Column::new("Restarts", Width::Fixed(8)).right(), Column::new("Age", Width::Fixed(5)).right(), Column::new("Node", Width::Flex(1))]);
    cols
}

fn shown_pods<'a>(c: &'a Cluster, filter: &str) -> Vec<&'a Pod> {
    c.pods.items.iter().filter(|p| matches(filter, &p.namespace, &p.name)).collect()
}

/// What each tab's rows are, for drawing and for keys.
pub struct Rows {
    pub workloads: Vec<TreeNode>,
    pub network: Vec<TreeNode>,
    pub secrets: Vec<TreeNode>,
    pub forwards: Vec<TreeNode>,
}

impl Rows {
    /// Tab `tab`'s rows (the others' are left empty: only one is shown).
    pub fn of(tab: TabId, c: &Cluster, forwards: &[Forward], filter: &str, all: bool, now: DateTime<Utc>) -> Rows {
        let mut rows = Rows { workloads: Vec::new(), network: Vec::new(), secrets: Vec::new(), forwards: Vec::new() };
        match tab {
            TabId::Workloads => rows.workloads = workload_nodes(c, filter, all, now),
            TabId::Network => rows.network = network_nodes(c, filter, all, now),
            TabId::Secrets => rows.secrets = secret_nodes(c, filter, all),
            TabId::Forwards => rows.forwards = forward_nodes(forwards),
            TabId::Pods => {}
        }
        rows
    }
}

impl MainView {
    pub fn tab(&self) -> TabId {
        TabId::ALL[self.tabs.active.min(4)]
    }

    fn tree(&self, tab: TabId) -> Option<&TreeState> {
        match tab {
            TabId::Workloads => Some(&self.workloads),
            TabId::Network => Some(&self.network),
            TabId::Secrets => Some(&self.secrets),
            TabId::Forwards => Some(&self.forwards),
            TabId::Pods => None,
        }
    }

    fn tree_mut(&mut self, tab: TabId) -> Option<&mut TreeState> {
        match tab {
            TabId::Workloads => Some(&mut self.workloads),
            TabId::Network => Some(&mut self.network),
            TabId::Secrets => Some(&mut self.secrets),
            TabId::Forwards => Some(&mut self.forwards),
            TabId::Pods => None,
        }
    }

    fn nodes(tab: TabId, rows: &Rows) -> &[TreeNode] {
        match tab {
            TabId::Workloads => &rows.workloads,
            TabId::Network => &rows.network,
            TabId::Secrets => &rows.secrets,
            TabId::Forwards | TabId::Pods => &rows.forwards,
        }
    }

    /// The row the cursor is on: its id.
    pub fn selected_id(&self, c: &Cluster, rows: &Rows) -> Option<String> {
        match self.tab() {
            TabId::Pods => shown_pods(c, &self.filter).get(self.pods.row).map(|p| object_id(Kind::Pod, &p.namespace, &p.name)),
            tab => self.tree(tab)?.selected_node(Self::nodes(tab, rows)).map(|n| n.id.clone()),
        }
    }

    /// The object the cursor is on.
    pub fn selected(&self, c: &Cluster, rows: &Rows) -> Option<Target> {
        self.selected_id(c, rows).as_deref().and_then(target_of)
    }

    /// The menu for `target`: what each row says, and its key.
    pub fn menu_rows(target: &Target) -> Vec<(&'static str, char)> {
        let k = target.kind;
        let mut rows = Vec::new();
        if k.has_logs() {
            rows.push(("Logs", 'l'));
        }
        if k == Kind::Pod {
            rows.push(("Shell in a container", '!'));
        }
        if k.forwards() {
            rows.push(("Port-forward…", 'f'));
        }
        rows.push(("YAML (read-only)", 'i'));
        rows.push(("Describe", 'D'));
        rows.push(("Copy the name", 'y'));
        if k.restarts() {
            rows.push(("Restart (rollout restart)…", 'R'));
        }
        if k.scales() {
            rows.push(("Scale…", 'S'));
        }
        if k == Kind::Pod {
            rows.push(("Delete…", 'x'));
        }
        rows
    }

    /// A key on the page.
    pub fn key(&mut self, key: Key, c: &Cluster, forwards: &[Forward], all: bool, now: DateTime<Utc>) -> Action {
        // A change waiting for yes: y says it, anything else doesn't.
        if let Some((m, _)) = self.confirm.take() {
            return if key == Key::Char('y') { Action::Confirmed(m) } else { Action::Declined };
        }
        let rows = Rows::of(self.tab(), c, forwards, &self.filter, all, now);
        let target = self.selected(c, &rows);
        if let Some(at) = self.menu {
            let items = target.as_ref().map(Self::menu_rows).unwrap_or_default();
            match key {
                Key::Char('j') | Key::Down => self.menu = Some((at + 1).min(items.len().saturating_sub(1))),
                Key::Char('k') | Key::Up => self.menu = Some(at.saturating_sub(1)),
                Key::Enter => {
                    self.menu = None;
                    return match items.get(at) {
                        Some((_, k)) => self.key(Key::Char(*k), c, forwards, all, now),
                        None => Action::Redraw,
                    };
                }
                Key::Char(ch) if items.iter().any(|(_, k)| *k == ch) => {
                    self.menu = None;
                    return self.key(key, c, forwards, all, now);
                }
                _ => self.menu = None,
            }
            return Action::Redraw;
        }
        let tab = self.tab();
        let need = |kinds: fn(Kind) -> bool, what: &str| -> Action {
            match &target {
                Some(t) if kinds(t.kind) => Action::Nothing,
                Some(t) => {
                    let kind = t.kind.singular();
                    let a = if kind.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
                    Action::Say(format!("{what} isn't for {a} {kind}"))
                }
                None => Action::Say("nothing here: move to a row first".into()),
            }
        };
        let on = |make: fn(Target) -> Action, kinds: fn(Kind) -> bool, what: &str| -> Action {
            match need(kinds, what) {
                Action::Nothing => make(target.clone().expect("checked")),
                said => said,
            }
        };
        match key {
            Key::Char(']') | Key::Char('[') | Key::Tab | Key::BackTab | Key::Char('1'..='5') => {
                let k = match key {
                    Key::Char(']') => Key::Tab,
                    Key::Char('[') => Key::BackTab,
                    k => k,
                };
                return match self.tabs.handle_key(k, TabId::ALL.len()) {
                    Outcome::Ignored => Action::Nothing,
                    _ => Action::Refresh,
                };
            }
            Key::Char('r') => return Action::Refresh,
            Key::Char('c') => return Action::PickContext,
            Key::Char('n') => return Action::PickNamespace,
            Key::Char('/') => return Action::Filter,
            Key::Char('+') => {
                let kind = match (tab, self.selected_id(c, &rows)) {
                    (_, Some(id)) if id.starts_with("more|") || id.starts_with("g|") => Kind::by_name(id.split('|').nth(1).unwrap_or_default()),
                    (TabId::Pods, _) => Some(Kind::Pod),
                    (TabId::Secrets, _) => Some(Kind::Secret),
                    (_, _) => target.as_ref().map(|t| t.kind),
                };
                return kind.map_or(Action::Nothing, Action::More);
            }
            Key::Char('m') if target.is_some() => {
                self.menu = Some(0);
                return Action::Redraw;
            }
            Key::Char('l') if tab != TabId::Forwards => return on(Action::Logs, Kind::has_logs, "A log"),
            Key::Char('!') => return on(Action::Shell, |k| k == Kind::Pod, "A shell"),
            Key::Char('f') => return on(Action::Forward, Kind::forwards, "A port-forward"),
            Key::Char('i') => return on(Action::Yaml, |_| true, "YAML"),
            Key::Char('u') => return Action::TakeSuggestion,
            Key::Char('D') => return on(Action::Describe, |_| true, "Describe"),
            Key::Char('y') if target.is_some() => return Action::Copy(target.map(|t| t.name).unwrap_or_default()),
            Key::Char('x') => return on(|t| Action::Mutate(Mutation::Delete(t)), |k| k == Kind::Pod, "Deleting"),
            Key::Char('R') => return on(|t| Action::Mutate(Mutation::Restart(t)), Kind::restarts, "A rollout restart"),
            Key::Char('S') => return on(Action::Scale, Kind::scales, "Scaling"),
            _ => {}
        }
        if tab == TabId::Forwards {
            let task = self.forwards.selected_node(&rows.forwards).and_then(|n| n.id.strip_prefix("f|")).and_then(|t| t.parse::<u64>().ok());
            let forward = task.and_then(|t| forwards.iter().find(|f| f.task == t));
            match (key, forward) {
                (Key::Char('s') | Key::Char('x'), Some(f)) => return Action::StopForward(f.task),
                (Key::Enter, Some(f)) => return Action::ShowForward(f.task),
                (Key::Char('o'), Some(f)) => return Action::OpenUrl(format!("http://localhost:{}/", f.local)),
                _ => {}
            }
        }
        match tab {
            TabId::Pods => {
                let n = shown_pods(c, &self.filter).len();
                match self.pods.handle_key(key, n, pod_columns(all).len()) {
                    Outcome::Submit => {
                        self.menu = target.is_some().then_some(0);
                        Action::Redraw
                    }
                    Outcome::Ignored => Action::Nothing,
                    _ => Action::Redraw,
                }
            }
            tab => {
                let nodes = Self::nodes(tab, &rows);
                let Some(tree) = self.tree_mut(tab) else { return Action::Nothing };
                // `l` is the log, here as on Containers' page: a branch
                // opens with Enter or the arrow.
                match tree.handle_key(key, nodes) {
                    Outcome::Submit => {
                        let node = tree.selected_node(nodes).cloned();
                        match node {
                            Some(n) if n.expandable() => {
                                if tree.is_expanded(&n.id) {
                                    tree.collapse(&n.id);
                                } else {
                                    tree.expand(&n.id);
                                }
                                Action::Redraw
                            }
                            Some(n) if n.id.starts_with("more|") => Kind::by_name(n.id.split('|').nth(1).unwrap_or_default()).map_or(Action::Nothing, Action::More),
                            Some(_) if target.is_some() && tab != TabId::Forwards => {
                                self.menu = Some(0);
                                Action::Redraw
                            }
                            _ => Action::Nothing,
                        }
                    }
                    Outcome::Ignored => Action::Nothing,
                    _ => Action::Redraw,
                }
            }
        }
    }

    /// The page, `width` by `height`.
    pub fn render(&self, header: &Header, c: &Cluster, forwards: &[Forward], now: DateTime<Utc>, width: usize, height: usize) -> Canvas {
        let mut cv = Canvas::new();
        let w = width.max(40);
        let h = height.max(10);
        let all = header.all_namespaces;
        // The top line: where you are.
        let mut x = cv.put(0, 0, "Kubernetes", Role::Title);
        x = cv.put(x, 0, "   context ", Role::Muted);
        match &header.context {
            Some(ctx) => {
                x = cv.put(x, 0, ctx, if header.protected { Role::Bad } else { Role::Accent });
                if header.protected {
                    x = cv.put(x, 0, " protected", Role::Warn);
                }
                x = cv.put(x, 0, &format!(" ({})", header.origin), Role::Muted);
            }
            None => x = cv.put(x, 0, "none", Role::Warn),
        }
        x = cv.put(x, 0, "   namespace ", Role::Muted);
        x = cv.put(x, 0, &header.namespace, Role::Accent);
        if !self.filter.is_empty() {
            x = cv.put(x, 0, "   filter ", Role::Muted);
            x = cv.put(x, 0, &self.filter, Role::Accent);
        }
        let vw = width_of(&header.versions);
        if x + 3 + vw <= w {
            cv.put(w - vw, 0, &header.versions, Role::Muted);
        }
        let tabs: Vec<Tab> = TabId::ALL
            .iter()
            .map(|t| {
                let n = match t {
                    TabId::Workloads => Kind::WORKLOADS.iter().map(|k| c.workloads.get(k).map_or(0, |l| l.items.len())).sum(),
                    TabId::Pods => c.pods.items.len(),
                    TabId::Network => c.services.items.len() + c.ingresses.items.len(),
                    TabId::Secrets => c.secrets.items.len(),
                    TabId::Forwards => forwards.len(),
                };
                Tab::new(t.title()).badge(n.to_string())
            })
            .collect();
        self.tabs.render(&tabs, Rect::new(0, 1, w, 2), &mut cv);

        // The foot: what's wrong, what's cut, the keys.
        let foot_y = h - 1;
        let note_y = h - 2;
        // A setting's suggestion, not used yet, over the foot.
        let above = if let Some(s) = &header.suggestion {
            cv.put_fit(0, note_y - 1, w, &format!("u uses the suggested {s}"), Role::Warn);
            1
        } else {
            0
        };
        let body = Rect::new(0, 3, w, note_y.saturating_sub(3 + above));
        if let Some((problem, hint)) = &header.problem {
            let mut y = body.y;
            for line in fenix_ui_kit::text::wrap(problem, w.saturating_sub(2)).into_iter().take(4) {
                cv.put(0, y, &line, Role::Bad);
                y += 1;
            }
            if let Some(hint) = hint {
                cv.put_fit(0, y, w, hint, Role::Muted);
                y += 1;
            }
            cv.put_fit(0, y + 1, w, "r tries again;  c chooses a context;  n a namespace", Role::Muted);
        } else {
            let tab = self.tab();
            match tab {
                TabId::Pods => {
                    let rows = PodRows { pods: shown_pods(c, &self.filter), all, now };
                    self.pods.render(&pod_columns(all), &rows, body, &mut cv);
                }
                tab => {
                    let rows = Rows::of(tab, c, forwards, &self.filter, all, now);
                    if let Some(tree) = self.tree(tab) {
                        tree.render(Self::nodes(tab, &rows), body, &mut cv);
                    }
                }
            }
            // A kind's error, or what's cut.
            let mut notes: Vec<(String, Role)> = Vec::new();
            for kind in tab.kinds() {
                let (error, more, listed, loading) = c.state(*kind);
                if let Some(e) = error {
                    notes.push((format!("{}: {e}", kind.title()), Role::Bad));
                } else if let (TabId::Pods, Some(line)) = (tab, more_line(more, listed, *kind)) {
                    notes.push((line, Role::Muted));
                } else if loading && listed == 0 {
                    notes.push((format!("asking for {}…", kind.resource()), Role::Muted));
                }
            }
            if let Some((note, role)) = notes.first() {
                cv.put_fit(0, note_y, w, note, role.clone());
            }
        }
        let keys = match self.tab() {
            TabId::Forwards => "s stop  o open  Enter output  r reload  c context  n namespace  1-5 tabs",
            _ => "m menu  l logs  ! shell  f forward  i yaml  D describe  c context  n namespace  r reload  / filter  x delete  R restart  S scale",
        };
        cv.put_fit(0, foot_y, w, keys, Role::Muted);
        // The menu, or the question.
        if let Some((_, question)) = &self.confirm {
            let rows = vec![vec![(question.clone(), Style::new(Role::Warn))], vec![("y ".to_string(), Style::new(Role::Accent)), ("yes, do it".to_string(), Style::new(Role::Text)), ("    any other key: no".to_string(), Style::new(Role::Muted))]];
            let line = self.cursor_line(c, forwards, all, now).unwrap_or(body.y);
            cv.popup(Popup { line, col: 2, rows });
        } else if let (Some(at), Some(t)) = (self.menu, self.selected(c, &Rows::of(self.tab(), c, forwards, &self.filter, all, now))) {
            let line = self.cursor_line(c, forwards, all, now).unwrap_or(body.y);
            let rows = Self::menu_rows(&t)
                .iter()
                .enumerate()
                .map(|(i, (what, key))| {
                    let style = if i == at { Style::new(Role::Title) } else { Style::new(Role::Text) };
                    vec![(format!("{} {what:<28}", if i == at { "›" } else { " " }), style), (key.to_string(), Style::new(Role::Muted))]
                })
                .collect();
            cv.popup(Popup { line, col: 4, rows });
        }
        cv
    }

    /// The canvas line the cursor's row was last drawn on.
    fn cursor_line(&self, c: &Cluster, forwards: &[Forward], all: bool, now: DateTime<Utc>) -> Option<usize> {
        match self.tab() {
            TabId::Pods => Some(3 + 2 + self.pods.row.saturating_sub(self.pods.visible_rows().start)),
            tab => {
                let rows = Rows::of(tab, c, forwards, &self.filter, all, now);
                let tree = self.tree(tab)?;
                let n = tree.rows(Self::nodes(tab, &rows)).len();
                Some(3 + tree.selected.min(n.saturating_sub(1)))
            }
        }
    }
}

fn width_of(s: &str) -> usize {
    width(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_id_names_its_object() {
        assert_eq!(target_of("o|deployment|shop|web"), Target::new(Kind::Deployment, "shop", "web").ok());
        assert_eq!(target_of("o|deployment|shop|web>o|pod|shop|web-1"), Target::new(Kind::Pod, "shop", "web-1").ok());
        assert_eq!(target_of("g|deployment"), None);
        assert_eq!(target_of("more|pod"), None);
        assert_eq!(target_of("o|pod|Bad Ns|x"), None, "a name the cluster couldn't have isn't acted on");
    }

    #[test]
    fn each_kind_has_only_the_actions_it_takes() {
        let keys = |k| MainView::menu_rows(&Target::new(k, "shop", "x").unwrap()).iter().map(|(_, c)| *c).collect::<String>();
        assert_eq!(keys(Kind::Pod), "l!fiDyx");
        assert_eq!(keys(Kind::Deployment), "lfiDyRS");
        assert_eq!(keys(Kind::DaemonSet), "liDyR");
        assert_eq!(keys(Kind::Secret), "iDy");
        assert_eq!(keys(Kind::Service), "fiDy");
    }

    #[test]
    fn a_question_names_the_context_and_the_namespace() {
        let m = Mutation::Scale { target: Target::new(Kind::Deployment, "shop", "web").unwrap(), from: Some(3), to: 5 };
        assert_eq!(m.question("kind-dev"), "Scale deployment web from 3 to 5? context kind-dev, namespace shop");
        let d = Mutation::Delete(Target::new(Kind::Pod, "shop", "web-1").unwrap());
        assert_eq!(d.question("prod-eu"), "Delete pod web-1? context prod-eu, namespace shop");
    }
}
