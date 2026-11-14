//! What kubectl answered, read: kubeconfig's contexts, versions, and the
//! API's lists (`get --raw`) into what the page shows of each object --
//! a pod's status as kubectl's own STATUS column says it, its restarts,
//! age and node; a deployment's readiness; a service's ports. Only the
//! fields used are read (serde skips the rest), so a long list parses
//! fast and keeps little.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::api::chrono::{DateTime, Utc};
use crate::kubectl::Kind;

// -- kubeconfig and versions ----------------------------------------------

/// A context kubeconfig has.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Context {
    pub name: String,
    pub cluster: String,
    /// Its own namespace, if it sets one.
    pub namespace: Option<String>,
}

/// kubeconfig, as `kubectl::config_view` prints it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Kubeconfig {
    pub current: Option<String>,
    pub contexts: Vec<Context>,
}

impl Kubeconfig {
    pub fn read(out: &str) -> Kubeconfig {
        let mut lines = out.lines();
        let current = lines.next().map(str::trim).filter(|c| !c.is_empty()).map(String::from);
        let contexts = lines
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let mut parts = l.split('\t');
                let name = parts.next().unwrap_or_default().to_string();
                let cluster = parts.next().unwrap_or_default().to_string();
                let namespace = parts.next().map(str::trim).filter(|n| !n.is_empty()).map(String::from);
                Context { name, cluster, namespace }
            })
            .collect();
        Kubeconfig { current, contexts }
    }

    pub fn context(&self, name: &str) -> Option<&Context> {
        self.contexts.iter().find(|c| c.name == name)
    }
}

/// `kubectl version -o json`'s versions: the client's, and the server's
/// when it answered.
pub fn versions(out: &str) -> (Option<String>, Option<String>) {
    let v: serde_json::Value = serde_json::from_str(out).unwrap_or_default();
    let at = |k: &str| v[k]["gitVersion"].as_str().map(String::from);
    (at("clientVersion"), at("serverVersion"))
}

/// Why kubectl failed, in its words: its last lines that say something,
/// `error: ` off, at most three, at most 400 characters -- an expired
/// login, an unreachable server, a missing kubeconfig.
pub fn failure(err: &[String], out: &[String]) -> String {
    let lines: Vec<&str> = err.iter().chain(out).map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with("E1") && !l.starts_with("W1") && !l.starts_with("I1")).collect();
    // klog's `E1114 10:00:00.000 ...` lines repeat the error; kubectl's own
    // last line says it best. Fall back to them when that's all there is.
    let lines = if lines.is_empty() { err.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect() } else { lines };
    let tail: Vec<&str> = lines.iter().rev().take(3).rev().copied().collect();
    let text = tail.join(" ").replace("error: ", "").replace("Error from server ", "");
    if text.is_empty() {
        return "kubectl failed and said nothing".into();
    }
    let text = crate::agent::plain(&text);
    if text.chars().count() > 400 {
        format!("{}…", text.chars().take(399).collect::<String>())
    } else {
        text
    }
}

/// A hint for a failure kubectl's words make plain.
pub fn hint(why: &str) -> Option<&'static str> {
    let w = why.to_ascii_lowercase();
    if w.contains("unauthorized") || w.contains("must be logged in") || w.contains("token has expired") || w.contains("expired") && w.contains("token") || w.contains("getting credentials") {
        Some("log in again as your cluster's provider has you do (aws sso login, gcloud auth login, kubelogin), then r")
    } else if w.contains("forbidden") {
        Some("your account may not list this here: choose another namespace (n)")
    } else if w.contains("connection refused") || w.contains("no such host") || w.contains("i/o timeout") || w.contains("unable to connect") || w.contains("deadline exceeded") || w.contains("timeout") {
        Some("the cluster isn't answering: is it running, is the VPN up? r tries again")
    } else if w.contains("no configuration") || w.contains("context was not found") || w.contains("does not exist") && w.contains("context") {
        Some("kubeconfig has no such context: c chooses one, KUBECONFIG says where kubeconfig is")
    } else {
        None
    }
}

// -- The API's lists -------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct List<T> {
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
    #[serde(default)]
    pub metadata: ListMeta,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMeta {
    #[serde(rename = "continue", default)]
    pub cont: Option<String>,
    #[serde(default)]
    pub remaining_item_count: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub uid: String,
    #[serde(default)]
    pub creation_timestamp: Option<String>,
    #[serde(default)]
    pub deletion_timestamp: Option<String>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub owner_references: Vec<Owner>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Owner {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub uid: String,
}

/// How many more there are past what was listed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum More {
    #[default]
    None,
    /// The server counted them.
    Count(u64),
    /// More, uncounted (a filtered list; or a table cut by hand).
    Some,
}

impl More {
    fn of(meta: &ListMeta) -> More {
        match (meta.cont.as_deref().filter(|c| !c.is_empty()), meta.remaining_item_count) {
            (_, Some(n)) if n > 0 => More::Count(n),
            (Some(_), _) => More::Some,
            _ => More::None,
        }
    }
}

// Pods.

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawPod {
    #[serde(default)]
    pub metadata: Meta,
    #[serde(default)]
    pub spec: PodSpec,
    #[serde(default)]
    pub status: PodStatus,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodSpec {
    #[serde(default)]
    pub node_name: Option<String>,
    #[serde(default)]
    pub containers: Vec<ContainerSpec>,
    #[serde(default)]
    pub init_containers: Vec<ContainerSpec>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerSpec {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub ports: Vec<ContainerPort>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerPort {
    #[serde(default)]
    pub container_port: u16,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodStatus {
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub container_statuses: Vec<ContainerStatus>,
    #[serde(default)]
    pub init_container_statuses: Vec<ContainerStatus>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatus {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub restart_count: u32,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub state: State,
    #[serde(default)]
    pub last_state: State,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct State {
    #[serde(default)]
    pub waiting: Option<Waiting>,
    #[serde(default)]
    pub running: Option<serde::de::IgnoredAny>,
    #[serde(default)]
    pub terminated: Option<Terminated>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Waiting {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Terminated {
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub exit_code: i32,
    #[serde(default)]
    pub signal: Option<i32>,
    #[serde(default)]
    pub finished_at: Option<String>,
}

/// A pod, as the page shows it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pod {
    pub name: String,
    pub namespace: String,
    /// As kubectl's STATUS column: Running, CrashLoopBackOff, Init:0/2,
    /// Terminating, Completed...
    pub status: String,
    pub ready: (usize, usize),
    pub restarts: u32,
    pub created: Option<String>,
    pub node: String,
    pub labels: BTreeMap<String, String>,
    pub owners: Vec<Owner>,
    /// Its containers (not its init containers), in order.
    pub containers: Vec<String>,
    /// The ports its containers say they listen on.
    pub ports: Vec<u16>,
}

impl Pod {
    pub fn health(&self) -> Health {
        match self.status.as_str() {
            "Running" if self.ready.0 == self.ready.1 => Health::Good,
            "Completed" | "Succeeded" => Health::Muted,
            "Running" | "Pending" | "ContainerCreating" | "PodInitializing" | "Terminating" => Health::Warn,
            s if s.starts_with("Init:") && !s.contains("Err") && !s.contains("BackOff") => Health::Warn,
            _ => Health::Bad,
        }
    }
}

/// How a row reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Good,
    Warn,
    Bad,
    Muted,
}

/// kubectl's STATUS for a pod (its `printPod`, the common cases).
pub fn pod_status(p: &RawPod) -> String {
    let st = &p.status;
    let mut reason = st.reason.clone().or_else(|| st.phase.clone()).unwrap_or_else(|| "Unknown".into());
    let mut initializing = false;
    let inits = p.spec.init_containers.len().max(st.init_container_statuses.len());
    for (i, c) in st.init_container_statuses.iter().enumerate() {
        if let Some(t) = &c.state.terminated {
            if t.exit_code == 0 {
                continue;
            }
            reason = match (&t.reason, t.signal) {
                (Some(r), _) if !r.is_empty() => format!("Init:{r}"),
                (_, Some(s)) if s != 0 => format!("Init:Signal:{s}"),
                _ => format!("Init:ExitCode:{}", t.exit_code),
            };
        } else if let Some(r) = c.state.waiting.as_ref().and_then(|w| w.reason.as_deref()).filter(|r| !r.is_empty() && *r != "PodInitializing") {
            reason = format!("Init:{r}");
        } else {
            reason = format!("Init:{i}/{inits}");
        }
        initializing = true;
        break;
    }
    if !initializing {
        let mut running = false;
        for c in st.container_statuses.iter().rev() {
            if let Some(r) = c.state.waiting.as_ref().and_then(|w| w.reason.as_deref()).filter(|r| !r.is_empty()) {
                reason = r.to_string();
            } else if let Some(t) = &c.state.terminated {
                reason = match (&t.reason, t.signal) {
                    (Some(r), _) if !r.is_empty() => r.clone(),
                    (_, Some(s)) if s != 0 => format!("Signal:{s}"),
                    _ => format!("ExitCode:{}", t.exit_code),
                };
            } else if c.ready && c.state.running.is_some() {
                running = true;
            }
        }
        if reason == "Completed" && running {
            let ready = st.conditions.iter().any(|c| c.kind == "Ready" && c.status == "True");
            reason = if ready { "Running".into() } else { "NotReady".into() };
        }
    }
    if p.metadata.deletion_timestamp.is_some() {
        reason = if st.reason.as_deref() == Some("NodeLost") { "Unknown".into() } else { "Terminating".into() };
    }
    reason
}

pub fn pod(p: RawPod) -> Pod {
    let status = pod_status(&p);
    let ready = (p.status.container_statuses.iter().filter(|c| c.ready).count(), p.spec.containers.len());
    let restarts = p.status.container_statuses.iter().map(|c| c.restart_count).sum();
    let ports = p.spec.containers.iter().flat_map(|c| c.ports.iter().map(|p| p.container_port)).filter(|p| *p > 0).collect();
    Pod {
        name: p.metadata.name,
        namespace: p.metadata.namespace,
        status,
        ready,
        restarts,
        created: p.metadata.creation_timestamp,
        node: p.spec.node_name.unwrap_or_default(),
        labels: p.metadata.labels,
        owners: p.metadata.owner_references,
        containers: p.spec.containers.into_iter().map(|c| c.name).collect(),
        ports,
    }
}

// Workloads.

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawWorkload {
    #[serde(default)]
    pub metadata: Meta,
    #[serde(default)]
    pub spec: WorkloadSpec,
    #[serde(default)]
    pub status: WorkloadStatus,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSpec {
    #[serde(default)]
    pub replicas: Option<u32>,
    #[serde(default)]
    pub selector: Option<Selector>,
    #[serde(default)]
    pub completions: Option<u32>,
    #[serde(default)]
    pub schedule: Option<String>,
    #[serde(default)]
    pub suspend: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Selector {
    #[serde(default)]
    pub match_labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadStatus {
    #[serde(default)]
    pub replicas: Option<u32>,
    #[serde(default)]
    pub ready_replicas: Option<u32>,
    #[serde(default)]
    pub updated_replicas: Option<u32>,
    #[serde(default)]
    pub available_replicas: Option<u32>,
    #[serde(default)]
    pub desired_number_scheduled: Option<u32>,
    #[serde(default)]
    pub number_ready: Option<u32>,
    #[serde(default)]
    pub succeeded: Option<u32>,
    #[serde(default)]
    pub failed: Option<u32>,
    #[serde(default)]
    pub active: Option<serde_json::Value>,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub last_schedule_time: Option<String>,
}

/// A deployment, statefulset, daemonset, job or cronjob, as the page
/// shows it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Workload {
    pub kind: Option<Kind>,
    pub name: String,
    pub namespace: String,
    pub uid: String,
    /// `3/3 ready`, `Complete 1/1`, `*/5 * * * *`.
    pub status: String,
    pub health: Option<Health>,
    /// What it asks for, when it scales.
    pub replicas: Option<u32>,
    pub created: Option<String>,
    pub selector: BTreeMap<String, String>,
    pub owners: Vec<Owner>,
}

pub fn workload(kind: Kind, w: RawWorkload) -> Workload {
    let s = &w.status;
    let n = |v: Option<u32>| v.unwrap_or(0);
    let (status, health) = match kind {
        Kind::Deployment | Kind::StatefulSet => {
            let want = w.spec.replicas.unwrap_or(1);
            let ready = n(s.ready_replicas);
            let mut text = format!("{ready}/{want} ready");
            if kind == Kind::Deployment && n(s.updated_replicas) < want {
                text.push_str(&format!(", {} up to date", n(s.updated_replicas)));
            }
            (text, if want == 0 { Health::Muted } else if ready >= want { Health::Good } else if ready == 0 { Health::Bad } else { Health::Warn })
        }
        Kind::DaemonSet => {
            let (want, ready) = (n(s.desired_number_scheduled), n(s.number_ready));
            (format!("{ready}/{want} ready"), if ready >= want { Health::Good } else { Health::Warn })
        }
        Kind::Job => {
            let done = |t: &str| s.conditions.iter().any(|c| c.kind == t && c.status == "True");
            let completions = w.spec.completions.unwrap_or(1);
            let state = if done("Complete") {
                "Complete"
            } else if done("Failed") {
                "Failed"
            } else if done("Suspended") {
                "Suspended"
            } else {
                "Running"
            };
            let health = match state {
                "Complete" => Health::Muted,
                "Failed" => Health::Bad,
                _ => Health::Warn,
            };
            (format!("{state} {}/{completions}", n(s.succeeded)), health)
        }
        Kind::CronJob => {
            let active = s.active.as_ref().and_then(|a| a.as_array()).map_or(0, Vec::len);
            let mut text = w.spec.schedule.clone().unwrap_or_default();
            if w.spec.suspend == Some(true) {
                text.push_str(", suspended");
            }
            if active > 0 {
                text.push_str(&format!(", {active} running"));
            }
            (text, if w.spec.suspend == Some(true) { Health::Muted } else { Health::Good })
        }
        _ => (String::new(), Health::Muted),
    };
    Workload {
        kind: Some(kind),
        name: w.metadata.name,
        namespace: w.metadata.namespace,
        uid: w.metadata.uid,
        status,
        health: Some(health),
        replicas: if kind.scales() { Some(w.spec.replicas.unwrap_or(1)) } else { None },
        created: w.metadata.creation_timestamp,
        selector: w.spec.selector.map(|s| s.match_labels).unwrap_or_default(),
        owners: w.metadata.owner_references,
    }
}

impl Workload {
    /// Whether `pod` is one of its: a job's by its owner; the rest by
    /// their selector's labels, in the same namespace.
    pub fn owns(&self, pod: &Pod) -> bool {
        if pod.namespace != self.namespace {
            return false;
        }
        match self.kind {
            Some(Kind::Job) => pod.owners.iter().any(|o| o.kind == "Job" && (o.uid == self.uid || o.name == self.name)),
            Some(Kind::CronJob) => false,
            _ => !self.selector.is_empty() && self.selector.iter().all(|(k, v)| pod.labels.get(k) == Some(v)),
        }
    }
}

// Services and ingresses.

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawService {
    #[serde(default)]
    pub metadata: Meta,
    #[serde(default)]
    pub spec: ServiceSpec,
    #[serde(default)]
    pub status: ServiceStatus,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSpec {
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(rename = "clusterIP", default)]
    pub cluster_ip: Option<String>,
    #[serde(rename = "externalIPs", default)]
    pub external_ips: Vec<String>,
    #[serde(default)]
    pub ports: Vec<ServicePort>,
    #[serde(default)]
    pub external_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePort {
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub node_port: Option<u16>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    #[serde(default)]
    pub load_balancer: LoadBalancer,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LoadBalancer {
    #[serde(default)]
    pub ingress: Vec<LbIngress>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LbIngress {
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Service {
    pub name: String,
    pub namespace: String,
    pub kind: String,
    pub cluster_ip: String,
    pub external: String,
    /// `80/TCP`, `443:30443/TCP`.
    pub ports: String,
    pub first_port: Option<u16>,
    pub created: Option<String>,
}

pub fn service(s: RawService) -> Service {
    let ports = s
        .spec
        .ports
        .iter()
        .map(|p| {
            let proto = p.protocol.as_deref().unwrap_or("TCP");
            match p.node_port {
                Some(n) if n > 0 => format!("{}:{n}/{proto}", p.port),
                _ => format!("{}/{proto}", p.port),
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    let mut external: Vec<String> = s.status.load_balancer.ingress.iter().filter_map(|i| i.ip.clone().or_else(|| i.hostname.clone())).collect();
    external.extend(s.spec.external_ips.iter().cloned());
    if let Some(n) = &s.spec.external_name {
        external.push(n.clone());
    }
    Service {
        name: s.metadata.name,
        namespace: s.metadata.namespace,
        kind: s.spec.kind.unwrap_or_else(|| "ClusterIP".into()),
        cluster_ip: s.spec.cluster_ip.unwrap_or_default(),
        external: external.join(","),
        first_port: s.spec.ports.first().map(|p| p.port),
        ports,
        created: s.metadata.creation_timestamp,
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawIngress {
    #[serde(default)]
    pub metadata: Meta,
    #[serde(default)]
    pub spec: IngressSpec,
    #[serde(default)]
    pub status: ServiceStatus,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngressSpec {
    #[serde(default)]
    pub ingress_class_name: Option<String>,
    #[serde(default)]
    pub rules: Vec<IngressRule>,
    #[serde(default)]
    pub tls: Vec<serde::de::IgnoredAny>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct IngressRule {
    #[serde(default)]
    pub host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ingress {
    pub name: String,
    pub namespace: String,
    pub class: String,
    pub hosts: String,
    pub address: String,
    pub tls: bool,
    pub created: Option<String>,
}

pub fn ingress(i: RawIngress) -> Ingress {
    let hosts: Vec<String> = i.spec.rules.iter().map(|r| r.host.clone().unwrap_or_else(|| "*".into())).collect();
    let address: Vec<String> = i.status.load_balancer.ingress.iter().filter_map(|a| a.ip.clone().or_else(|| a.hostname.clone())).collect();
    Ingress {
        name: i.metadata.name,
        namespace: i.metadata.namespace,
        class: i.spec.ingress_class_name.unwrap_or_default(),
        hosts: if hosts.is_empty() { "*".into() } else { hosts.join(",") },
        address: address.join(","),
        tls: !i.spec.tls.is_empty(),
        created: i.metadata.creation_timestamp,
    }
}

// Secrets: kubectl's table, never their data.

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Secret {
    pub name: String,
    pub namespace: String,
    pub kind: String,
    /// How many keys it holds (kubectl's DATA column).
    pub keys: u32,
    /// kubectl's AGE, as it wrote it.
    pub age: String,
}

/// A line of `get secrets --no-headers`: `[NAMESPACE] NAME TYPE DATA AGE`.
pub fn secret_line(line: &str, all: bool, namespace: &str) -> Option<Secret> {
    let cols: Vec<&str> = line.split_whitespace().collect();
    let cols = if all { cols } else { std::iter::once(namespace).chain(cols).collect() };
    if cols.len() < 5 {
        return None;
    }
    Some(Secret { namespace: cols[0].into(), name: cols[1].into(), kind: cols[2].into(), keys: cols[3].parse().unwrap_or(0), age: cols[4].into() })
}

// Events.

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawEvent {
    #[serde(default)]
    pub metadata: Meta,
    #[serde(default)]
    pub involved_object: Involved,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub count: Option<u32>,
    #[serde(default)]
    pub last_timestamp: Option<String>,
    #[serde(default)]
    pub event_time: Option<String>,
    #[serde(default)]
    pub first_timestamp: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Involved {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
}

impl RawEvent {
    /// When it last happened, as its fields say it.
    pub fn when(&self) -> Option<&str> {
        self.last_timestamp.as_deref().or(self.event_time.as_deref()).or(self.first_timestamp.as_deref()).or(self.metadata.creation_timestamp.as_deref())
    }
}

// Lists read.

/// One kind's list: what came, and how many more there are.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Listed<T> {
    pub items: Vec<T>,
    pub more: More,
    /// The token that lists on from here.
    pub cont: Option<String>,
}

pub fn read_list<T: for<'de> Deserialize<'de>>(text: &str) -> Result<Listed<T>, String> {
    let list: List<T> = serde_json::from_str(text).map_err(|e| format!("kubectl's answer isn't the list it should be: {e}"))?;
    Ok(Listed { more: More::of(&list.metadata), cont: list.metadata.cont.filter(|c| !c.is_empty()), items: list.items })
}

pub fn pods(text: &str) -> Result<Listed<Pod>, String> {
    let l = read_list::<RawPod>(text)?;
    Ok(Listed { items: l.items.into_iter().map(pod).collect(), more: l.more, cont: l.cont })
}

pub fn workloads(kind: Kind, text: &str) -> Result<Listed<Workload>, String> {
    let l = read_list::<RawWorkload>(text)?;
    Ok(Listed { items: l.items.into_iter().map(|w| workload(kind, w)).collect(), more: l.more, cont: l.cont })
}

pub fn services(text: &str) -> Result<Listed<Service>, String> {
    let l = read_list::<RawService>(text)?;
    Ok(Listed { items: l.items.into_iter().map(service).collect(), more: l.more, cont: l.cont })
}

pub fn ingresses(text: &str) -> Result<Listed<Ingress>, String> {
    let l = read_list::<RawIngress>(text)?;
    Ok(Listed { items: l.items.into_iter().map(ingress).collect(), more: l.more, cont: l.cont })
}

/// Namespaces' names, from their list.
pub fn namespace_names(text: &str) -> Result<Vec<String>, String> {
    #[derive(Deserialize)]
    struct Ns {
        #[serde(default)]
        metadata: Meta,
    }
    let l = read_list::<Ns>(text)?;
    Ok(l.items.into_iter().map(|n| n.metadata.name).collect())
}

// Time.

pub fn parse_time(t: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(t).ok().map(|d| d.with_timezone(&Utc))
}

/// How long ago, as kubectl's AGE says it: `45s`, `12m`, `5h`, `3d`.
pub fn age(since: Option<&str>, now: DateTime<Utc>) -> String {
    let Some(t) = since.and_then(parse_time) else { return "?".into() };
    let s = (now - t).num_seconds().max(0);
    match s {
        0..=119 => format!("{s}s"),
        120..=7199 => format!("{}m", s / 60),
        7200..=172_799 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-11-14T12:00:00Z";

    fn now() -> DateTime<Utc> {
        parse_time(NOW).unwrap()
    }

    #[test]
    fn kubeconfig_is_its_names_and_namespaces_only() {
        let k = Kubeconfig::read("kind-dev\nkind-dev\tkind-dev\tshop\nprod-eu\tgke_acme_europe-west1_prod\t\narn:aws:eks:us-east-1:1:cluster/x\tarn:aws:eks:us-east-1:1:cluster/x\tdefault\n");
        assert_eq!(k.current.as_deref(), Some("kind-dev"));
        assert_eq!(k.contexts.len(), 3);
        assert_eq!(k.context("kind-dev").unwrap().namespace.as_deref(), Some("shop"));
        assert_eq!(k.context("prod-eu").unwrap().namespace, None);
        assert_eq!(k.contexts[2].name, "arn:aws:eks:us-east-1:1:cluster/x");
        // No kubeconfig at all: kubectl prints nothing.
        assert_eq!(Kubeconfig::read(""), Kubeconfig::default());
        assert_eq!(Kubeconfig::read("\n"), Kubeconfig::default());
    }

    #[test]
    fn versions_and_failures_are_read() {
        let v = r#"{"clientVersion":{"major":"1","minor":"31","gitVersion":"v1.31.2"},"kustomizeVersion":"v5.4.2","serverVersion":{"gitVersion":"v1.30.0"}}"#;
        assert_eq!(versions(v), (Some("v1.31.2".into()), Some("v1.30.0".into())));
        assert_eq!(versions(r#"{"clientVersion":{"gitVersion":"v1.31.2"}}"#), (Some("v1.31.2".into()), None));
        let err = vec![
            "E1114 12:00:00.123   4242 memcache.go:265] couldn't get current server API group list: the server has asked for the client to provide credentials".to_string(),
            "error: You must be logged in to the server (the server has asked for the client to provide credentials)".to_string(),
        ];
        let why = failure(&err, &[]);
        assert_eq!(why, "You must be logged in to the server (the server has asked for the client to provide credentials)");
        assert!(hint(&why).unwrap().contains("log in again"));
        assert!(hint("dial tcp 127.0.0.1:6443: connect: connection refused").unwrap().contains("isn't answering"));
        assert!(hint("(Forbidden): pods is forbidden: User \"dev\" cannot list resource").unwrap().contains("namespace"));
        assert_eq!(failure(&[], &[]), "kubectl failed and said nothing");
        assert!(failure(&["x".repeat(1000)], &[]).chars().count() <= 400);
        assert_eq!(failure(&["\u{1b}[31merror: boom\u{7}".into()], &[]), "boom", "colours and bells out");
    }

    #[test]
    fn ages_read_as_kubectl_writes_them() {
        assert_eq!(age(Some("2026-11-14T11:59:15Z"), now()), "45s");
        assert_eq!(age(Some("2026-11-14T11:48:00Z"), now()), "12m");
        assert_eq!(age(Some("2026-11-14T07:00:00Z"), now()), "5h");
        assert_eq!(age(Some("2026-11-11T12:00:00Z"), now()), "3d");
        assert_eq!(age(None, now()), "?");
        assert_eq!(age(Some("2026-11-14T12:00:05Z"), now()), "0s", "a clock a little ahead");
    }

    fn raw_pod(json: &str) -> RawPod {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_pods_status_is_kubectls() {
        let running = raw_pod(r#"{"metadata":{"name":"a"},"spec":{"containers":[{"name":"c"}]},"status":{"phase":"Running","containerStatuses":[{"name":"c","ready":true,"restartCount":2,"state":{"running":{"startedAt":"x"}}}]}}"#);
        assert_eq!(pod_status(&running), "Running");
        let crash = raw_pod(r#"{"spec":{"containers":[{"name":"c"}]},"status":{"phase":"Running","containerStatuses":[{"name":"c","ready":false,"restartCount":9,"state":{"waiting":{"reason":"CrashLoopBackOff"}}}]}}"#);
        assert_eq!(pod_status(&crash), "CrashLoopBackOff");
        let oom = raw_pod(r#"{"status":{"phase":"Running","containerStatuses":[{"name":"c","state":{"terminated":{"reason":"OOMKilled","exitCode":137}}}]}}"#);
        assert_eq!(pod_status(&oom), "OOMKilled");
        let init = raw_pod(r#"{"spec":{"initContainers":[{"name":"a"},{"name":"b"}]},"status":{"phase":"Pending","initContainerStatuses":[{"name":"a","state":{"terminated":{"exitCode":0}}},{"name":"b","state":{"running":{}}}]}}"#);
        assert_eq!(pod_status(&init), "Init:1/2");
        let init_err = raw_pod(r#"{"status":{"phase":"Pending","initContainerStatuses":[{"name":"a","state":{"waiting":{"reason":"ImagePullBackOff"}}}]}}"#);
        assert_eq!(pod_status(&init_err), "Init:ImagePullBackOff");
        let gone = raw_pod(r#"{"metadata":{"deletionTimestamp":"2026-11-14T11:00:00Z"},"status":{"phase":"Running"}}"#);
        assert_eq!(pod_status(&gone), "Terminating");
        let evicted = raw_pod(r#"{"status":{"phase":"Failed","reason":"Evicted"}}"#);
        assert_eq!(pod_status(&evicted), "Evicted");
        let done = raw_pod(r#"{"status":{"phase":"Succeeded","containerStatuses":[{"name":"c","state":{"terminated":{"reason":"Completed","exitCode":0}}}]}}"#);
        assert_eq!(pod_status(&done), "Completed");
        assert_eq!(pod_status(&RawPod::default()), "Unknown");
    }
}
