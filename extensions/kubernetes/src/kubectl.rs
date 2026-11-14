//! kubectl's command lines: every one the extension runs, as an argv --
//! the program, then each argument a string of its own, given to Fenix's
//! `spawn`, `run_in_terminal` or `run_in_background`, which start the
//! program with them as they are (no shell reads them). Nothing here talks
//! to the API server or reads kubeconfig's credentials: kubectl does both.
//!
//! - **Values go in `--flag=value`**: a context, a namespace, a container
//!   is one argument whatever it holds, never two, never read as another
//!   flag (`--context=--as=admin` is a context called `--as=admin`).
//! - **Names are checked** before they're used (`name`, `label`): a pod,
//!   a deployment, a namespace is what Kubernetes allows (lower-case
//!   letters, digits, `-` and `.`), so it can't start with `-` and be read
//!   as an option, or hold `/`, `?` or `&` and change a path.
//! - **Every call names its context**: `--context=<it>` is never left out
//!   (there's no builder here that takes none), so kubectl never falls back
//!   on whatever kubeconfig's current context has become since. The
//!   extension never changes kubeconfig.
//! - **Path segments are percent-encoded** in `get --raw` paths, whatever
//!   was checked before.

use std::fmt;

/// How long kubectl waits on the API server for a request that reads or
/// changes something (not a stream: logs, exec, port-forward).
pub const REQUEST_TIMEOUT: &str = "--request-timeout=20s";

/// What a shell in a pod runs: bash when it has it, else sh. A constant:
/// nothing of yours or the cluster's is in it.
pub const SHELL_SCRIPT: &str = "command -v bash >/dev/null 2>&1 && exec bash || exec sh";

/// The namespace the extension shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Ns {
    /// The context's own (kubeconfig's `namespace`), else `default`.
    #[default]
    Default,
    Named(String),
    /// Every namespace.
    All,
}

/// The context and namespace the extension works in, for this project.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    /// None: kubeconfig's current context.
    pub context: Option<String>,
    pub namespace: Ns,
}

/// The kinds the page lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Deployment,
    StatefulSet,
    DaemonSet,
    Job,
    CronJob,
    Pod,
    Service,
    Ingress,
    Secret,
}

impl Kind {
    pub const ALL: [Kind; 9] = [Kind::Deployment, Kind::StatefulSet, Kind::DaemonSet, Kind::Job, Kind::CronJob, Kind::Pod, Kind::Service, Kind::Ingress, Kind::Secret];
    pub const WORKLOADS: [Kind; 5] = [Kind::Deployment, Kind::StatefulSet, Kind::DaemonSet, Kind::Job, Kind::CronJob];

    /// Its resource, as the API's paths and kubectl name it.
    pub fn resource(self) -> &'static str {
        match self {
            Kind::Deployment => "deployments",
            Kind::StatefulSet => "statefulsets",
            Kind::DaemonSet => "daemonsets",
            Kind::Job => "jobs",
            Kind::CronJob => "cronjobs",
            Kind::Pod => "pods",
            Kind::Service => "services",
            Kind::Ingress => "ingresses",
            Kind::Secret => "secrets",
        }
    }

    /// As `kind/name` takes it (`deployment/web`).
    pub fn singular(self) -> &'static str {
        match self {
            Kind::Deployment => "deployment",
            Kind::StatefulSet => "statefulset",
            Kind::DaemonSet => "daemonset",
            Kind::Job => "job",
            Kind::CronJob => "cronjob",
            Kind::Pod => "pod",
            Kind::Service => "service",
            Kind::Ingress => "ingress",
            Kind::Secret => "secret",
        }
    }

    /// The API group's path.
    fn api(self) -> &'static str {
        match self {
            Kind::Deployment | Kind::StatefulSet | Kind::DaemonSet => "/apis/apps/v1",
            Kind::Job | Kind::CronJob => "/apis/batch/v1",
            Kind::Ingress => "/apis/networking.k8s.io/v1",
            Kind::Pod | Kind::Service | Kind::Secret => "/api/v1",
        }
    }

    /// As the page heads its list.
    pub fn title(self) -> &'static str {
        match self {
            Kind::Deployment => "Deployments",
            Kind::StatefulSet => "StatefulSets",
            Kind::DaemonSet => "DaemonSets",
            Kind::Job => "Jobs",
            Kind::CronJob => "CronJobs",
            Kind::Pod => "Pods",
            Kind::Service => "Services",
            Kind::Ingress => "Ingresses",
            Kind::Secret => "Secrets",
        }
    }

    /// The API's own name for it (`Deployment`), as an object's `kind`
    /// and an event's `involvedObject.kind` say it.
    pub fn api_kind(self) -> &'static str {
        match self {
            Kind::Deployment => "Deployment",
            Kind::StatefulSet => "StatefulSet",
            Kind::DaemonSet => "DaemonSet",
            Kind::Job => "Job",
            Kind::CronJob => "CronJob",
            Kind::Pod => "Pod",
            Kind::Service => "Service",
            Kind::Ingress => "Ingress",
            Kind::Secret => "Secret",
        }
    }

    /// A kind by any name kubectl takes for it: `deploy`, `deployments`,
    /// `Deployment`.
    pub fn by_name(name: &str) -> Option<Kind> {
        let n = name.trim().to_ascii_lowercase();
        Kind::ALL.into_iter().find(|k| {
            let short = match k {
                Kind::Deployment => "deploy",
                Kind::StatefulSet => "sts",
                Kind::DaemonSet => "ds",
                Kind::Job => "job",
                Kind::CronJob => "cj",
                Kind::Pod => "po",
                Kind::Service => "svc",
                Kind::Ingress => "ing",
                Kind::Secret => "secret",
            };
            n == k.singular() || n == k.resource() || n == short
        })
    }

    /// Whether a rollout restart applies.
    pub fn restarts(self) -> bool {
        matches!(self, Kind::Deployment | Kind::StatefulSet | Kind::DaemonSet)
    }

    /// Whether it scales.
    pub fn scales(self) -> bool {
        matches!(self, Kind::Deployment | Kind::StatefulSet)
    }

    /// Whether kubectl reads logs from it (a pod, or what runs pods).
    pub fn has_logs(self) -> bool {
        matches!(self, Kind::Pod | Kind::Deployment | Kind::StatefulSet | Kind::DaemonSet | Kind::Job)
    }

    /// Whether a port-forward goes to it.
    pub fn forwards(self) -> bool {
        matches!(self, Kind::Pod | Kind::Service | Kind::Deployment | Kind::StatefulSet)
    }
}

/// One object: its kind, namespace and name, each checked.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    pub kind: Kind,
    pub namespace: String,
    pub name: String,
}

impl Target {
    /// `kind` `namespace/name`, refused unless both are names Kubernetes
    /// allows.
    pub fn new(kind: Kind, namespace: &str, name: &str) -> Result<Target, String> {
        Ok(Target { kind, namespace: label(namespace)?, name: self::name(name)? })
    }

    /// `deployment/web`.
    pub fn reference(&self) -> String {
        format!("{}/{}", self.kind.singular(), self.name)
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.kind.singular(), self.name)
    }
}

/// An object's name, if it can only be one: a DNS subdomain, as
/// Kubernetes names most things -- lower-case letters, digits, `-` and
/// `.`, starting and ending with a letter or digit, at most 253.
pub fn name(given: &str) -> Result<String, String> {
    let ok = !given.is_empty()
        && given.len() <= 253
        && given.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        && given.starts_with(|c: char| c.is_ascii_alphanumeric())
        && given.ends_with(|c: char| c.is_ascii_alphanumeric());
    if ok {
        Ok(given.to_string())
    } else {
        Err(format!("{given:?} isn't a Kubernetes name: lower-case letters, digits, - and ., starting and ending with a letter or digit"))
    }
}

/// A namespace's or a container's name, if it can only be one: a DNS
/// label -- lower-case letters, digits and `-`, starting and ending with a
/// letter or digit, at most 63.
pub fn label(given: &str) -> Result<String, String> {
    let ok = !given.is_empty()
        && given.len() <= 63
        && given.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && given.starts_with(|c: char| c.is_ascii_alphanumeric())
        && given.ends_with(|c: char| c.is_ascii_alphanumeric());
    if ok {
        Ok(given.to_string())
    } else {
        Err(format!("{given:?} isn't a namespace or container name: lower-case letters, digits and -, starting and ending with a letter or digit"))
    }
}

/// A context's name, as kubeconfig may have it: anything printable. It's
/// only ever given as `--context=<it>`, one argument.
pub fn context(given: &str) -> Result<String, String> {
    if given.is_empty() || given.chars().any(char::is_control) {
        Err(format!("{given:?} isn't a context's name"))
    } else {
        Ok(given.to_string())
    }
}

/// A duration kubectl's `--since` takes: `30s`, `10m`, `2h30m`.
pub fn since(given: &str) -> Result<String, String> {
    let g = given.trim();
    let mut digits = false;
    let mut parts = 0;
    for c in g.chars() {
        match c {
            '0'..='9' => digits = true,
            's' | 'm' | 'h' if digits => {
                digits = false;
                parts += 1;
            }
            _ => return Err(format!("{given:?} isn't a duration: 30s, 10m, 2h, 1h30m")),
        }
    }
    if parts == 0 || digits || g.len() > 16 {
        return Err(format!("{given:?} isn't a duration: 30s, 10m, 2h, 1h30m"));
    }
    Ok(g.to_string())
}

/// A time kubectl wrote at the start of a log line (`--timestamps`), as
/// `--since-time` takes it back: RFC 3339's characters only.
pub fn since_time(given: &str) -> Option<String> {
    let ok = given.len() >= 20 && given.len() <= 40 && given.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | ':' | 'T' | 'Z' | '.' | '+'));
    ok.then(|| given.to_string())
}

/// `local:remote`, each a port (1-65535).
pub fn ports(given: &str) -> Result<(u16, u16), String> {
    let bad = || format!("{given:?} isn't local:remote, two ports (8080:80)");
    let (l, r) = given.trim().split_once(':').ok_or_else(bad)?;
    let port = |p: &str| p.trim().parse::<u16>().ok().filter(|p| *p > 0);
    Ok((port(l).ok_or_else(bad)?, port(r).ok_or_else(bad)?))
}

fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(|s| s.to_string()).collect()
}

/// `--context=<it>`: every call has one.
fn on(context: &str) -> String {
    format!("--context={context}")
}

/// `--context` and `--namespace`.
fn scope(context: &str, namespace: &str) -> Vec<String> {
    vec![on(context), format!("--namespace={namespace}")]
}

/// kubectl's own version, asking nothing of a cluster.
pub fn client_version() -> Vec<String> {
    words(&["version", "--client", "-o", "json"])
}

/// kubeconfig as kubectl merges it (`KUBECONFIG`), as JSON: without
/// `--raw`, kubectl prints its credentials as `REDACTED` and
/// `DATA+OMITTED`, and the extension reads only names, namespaces and
/// servers from it.
pub fn config_view() -> Vec<String> {
    words(&["config", "view", "-o", "json"])
}

/// The cluster's version: whether it's reachable, as `context`.
pub fn server_version(context: &str) -> Vec<String> {
    let mut out = words(&["version", "-o", "json", "--request-timeout=10s"]);
    out.push(on(context));
    out
}

/// The API path that lists `kind` in `namespace` (every one: `None`), at
/// most `limit`, from `from` (a `continue` the last answer gave).
pub fn list_path(kind: Kind, namespace: Option<&str>, limit: usize, from: Option<&str>) -> String {
    let mut path = match namespace {
        Some(ns) => format!("{}/namespaces/{}/{}", kind.api(), encode(ns), kind.resource()),
        None => format!("{}/{}", kind.api(), kind.resource()),
    };
    path.push_str(&format!("?limit={limit}"));
    if let Some(token) = from {
        path.push_str("&continue=");
        path.push_str(&encode(token));
    }
    path
}

fn raw(path: String, context: &str) -> Vec<String> {
    vec!["get".to_string(), "--raw".to_string(), path, on(context), REQUEST_TIMEOUT.to_string()]
}

/// `kind`, at most `limit`, from the API server itself: `get --raw` with
/// `limit` stops it there, and says how many more there are
/// (`remainingItemCount`) -- not every pod fetched and parsed to show
/// 500. Not for Secrets: `secrets` lists those.
pub fn list(kind: Kind, context: &str, namespace: Option<&str>, limit: usize) -> Vec<String> {
    debug_assert!(kind != Kind::Secret, "a Secret's data is never fetched to be listed");
    raw(list_path(kind, namespace, limit, None), context)
}

/// The Secrets: kubectl's own table (name, type, how many keys, age),
/// never their data, which stays in kubectl.
pub fn secrets(context: &str, namespace: Option<&str>) -> Vec<String> {
    let mut out = words(&["get", "secrets", "--no-headers"]);
    out.push(on(context));
    match namespace {
        Some(ns) => out.push(format!("--namespace={ns}")),
        None => out.push("--all-namespaces".into()),
    }
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// The namespaces, for the picker.
pub fn namespaces(context: &str) -> Vec<String> {
    raw("/api/v1/namespaces?limit=1000".into(), context)
}

/// Events in `namespace`, about one object when `about` says, at most
/// `limit`.
pub fn events(context: &str, namespace: &str, about: Option<(&str, &str)>, limit: usize) -> Vec<String> {
    let mut path = format!("/api/v1/namespaces/{}/events?limit={limit}", encode(namespace));
    if let Some((kind, name)) = about {
        path.push_str(&format!("&fieldSelector={}", encode(&format!("involvedObject.kind={kind},involvedObject.name={name}"))));
    }
    raw(path, context)
}

/// What `kubernetes_describe` may read, by kubectl's name for the kind:
/// its API path in a namespace. Namespaced kinds only: agents stay in the
/// namespace you chose.
pub fn object_api(kind: &str) -> Option<(&'static str, &'static str)> {
    Some(match kind {
        "pod" => ("/api/v1", "pods"),
        "service" => ("/api/v1", "services"),
        "persistentvolumeclaim" => ("/api/v1", "persistentvolumeclaims"),
        "deployment" => ("/apis/apps/v1", "deployments"),
        "statefulset" => ("/apis/apps/v1", "statefulsets"),
        "daemonset" => ("/apis/apps/v1", "daemonsets"),
        "replicaset" => ("/apis/apps/v1", "replicasets"),
        "job" => ("/apis/batch/v1", "jobs"),
        "cronjob" => ("/apis/batch/v1", "cronjobs"),
        "ingress" => ("/apis/networking.k8s.io/v1", "ingresses"),
        "horizontalpodautoscaler" => ("/apis/autoscaling/v2", "horizontalpodautoscalers"),
        _ => return None,
    })
}

/// One object, as the API's JSON.
pub fn object(context: &str, kind: &str, namespace: &str, name: &str) -> Option<Vec<String>> {
    let (api, resource) = object_api(kind)?;
    Some(raw(format!("{api}/namespaces/{}/{resource}/{}", encode(namespace), encode(name)), context))
}

/// One pod, as JSON: its containers and their state.
pub fn pod(context: &str, namespace: &str, pod: &str) -> Vec<String> {
    raw(format!("/api/v1/namespaces/{}/pods/{}", encode(namespace), encode(pod)), context)
}

/// The columns `workloads` prints, a line an object.
pub const WORKLOAD_COLUMNS: &str = r#"{range .items[*]}{.kind}{"\t"}{.metadata.namespace}{"\t"}{.metadata.name}{"\t"}{.metadata.creationTimestamp}{"\t"}{.spec.replicas}{"\t"}{.status.readyReplicas}{"\t"}{.status.updatedReplicas}{"\t"}{.status.desiredNumberScheduled}{"\t"}{.status.numberReady}{"\t"}{.spec.completions}{"\t"}{.status.succeeded}{"\t"}{.status.failed}{"\t"}{.spec.schedule}{"\t"}{.spec.suspend}{"\t"}{.status.active}{"\n"}{end}"#;

/// Every kind of workload in `namespace`, in one kubectl call: a line an
/// object, of the few fields an agent is given (`WORKLOAD_COLUMNS`). Read
/// as it comes and cut by the extension past its cap.
pub fn workloads(context: &str, namespace: &str) -> Vec<String> {
    let mut out = words(&["get", "deployments,statefulsets,daemonsets,jobs,cronjobs"]);
    out.extend(scope(context, namespace));
    out.extend(["--chunk-size=500".to_string(), REQUEST_TIMEOUT.to_string(), "-o".to_string(), format!("jsonpath={WORKLOAD_COLUMNS}")]);
    out
}

/// An object as YAML -- a Secret as JSON, so its values can be taken out
/// before anything shows it.
pub fn get(target: &Target, context: &str) -> Vec<String> {
    let output = if target.kind == Kind::Secret { "json" } else { "yaml" };
    let mut out = vec!["get".to_string(), target.kind.singular().to_string(), target.name.clone(), "-o".to_string(), output.to_string()];
    out.extend(scope(context, &target.namespace));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// `kubectl describe`, for your buffer (never an agent's).
pub fn describe(target: &Target, context: &str) -> Vec<String> {
    let mut out = vec!["describe".to_string(), target.kind.singular().to_string(), target.name.clone()];
    out.extend(scope(context, &target.namespace));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// How a log is read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LogOptions {
    /// One container; none: all of them (`--all-containers`, each line
    /// prefixed with where it came from).
    pub container: Option<String>,
    /// The container's last run, before it restarted: read once, not
    /// followed.
    pub previous: bool,
    /// Only what came in this long (`10m`); else the last `tail` lines.
    pub since: Option<String>,
    /// Only what came after this time (resuming a stopped stream).
    pub since_time: Option<String>,
    pub tail: u32,
    pub follow: bool,
}

/// A log: a pod's, or what kubectl picks of a workload's pods.
pub fn logs(target: &Target, context: &str, o: &LogOptions) -> Vec<String> {
    let mut out = vec!["logs".to_string()];
    out.extend(scope(context, &target.namespace));
    match &o.container {
        Some(c) => out.push(format!("--container={c}")),
        None => out.extend(words(&["--all-containers=true", "--prefix=true"])),
    }
    out.push("--timestamps=true".into());
    if o.previous {
        out.push("--previous=true".into());
    }
    match (&o.since_time, &o.since) {
        (Some(t), _) => out.push(format!("--since-time={t}")),
        (None, Some(s)) => out.push(format!("--since={s}")),
        (None, None) => out.push(format!("--tail={}", o.tail)),
    }
    if o.follow && !o.previous {
        out.push("--follow=true".into());
    }
    out.push(if target.kind == Kind::Pod { target.name.clone() } else { target.reference() });
    out
}

/// A shell in a pod's container, in a terminal: `exec -i -t`, the pod
/// alone before `--`, then the constant script.
pub fn exec(target: &Target, context: &str, container: &str) -> Vec<String> {
    let mut out = words(&["exec", "-i", "-t"]);
    out.extend(scope(context, &target.namespace));
    out.push(format!("--container={container}"));
    out.push(target.name.clone());
    out.extend(words(&["--", "sh", "-c", SHELL_SCRIPT]));
    out
}

/// A port-forward, on this machine's loopback only.
pub fn port_forward(target: &Target, context: &str, local: u16, remote: u16) -> Vec<String> {
    let mut out = vec!["port-forward".to_string()];
    out.extend(scope(context, &target.namespace));
    out.push("--address=127.0.0.1".into());
    out.push(target.reference());
    out.push(format!("{local}:{remote}"));
    out
}

/// A pod deleted, without waiting for it to go.
pub fn delete(target: &Target, context: &str) -> Vec<String> {
    let mut out = vec!["delete".to_string(), target.kind.singular().to_string(), target.name.clone(), "--wait=false".to_string()];
    out.extend(scope(context, &target.namespace));
    out.push(REQUEST_TIMEOUT.into());
    out
}

pub fn rollout_restart(target: &Target, context: &str) -> Vec<String> {
    let mut out = vec!["rollout".to_string(), "restart".to_string(), target.reference()];
    out.extend(scope(context, &target.namespace));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// Scaled to `to`, only if it's still at `from` (`--current-replicas`):
/// someone else's change in between makes it fail, not lose.
pub fn scale(target: &Target, context: &str, from: Option<u32>, to: u32) -> Vec<String> {
    let mut out = vec!["scale".to_string(), target.reference(), format!("--replicas={to}")];
    if let Some(from) = from {
        out.push(format!("--current-replicas={from}"));
    }
    out.extend(scope(context, &target.namespace));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// `text` percent-encoded for a URL's path or query: what isn't a letter,
/// a digit or `-._~` becomes `%XX`.
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Contexts and clusters that are clearly on this machine, by name.
pub const LOCAL_NAMES: [&str; 9] = ["kind-*", "minikube", "docker-desktop", "docker-for-desktop", "rancher-desktop", "k3d-*", "colima", "colima-*", "orbstack"];

/// Whether `server` (`https://127.0.0.1:6443`) is this machine's own:
/// localhost, 127.0.0.1 (any 127.x) or ::1.
pub fn local_server(server: &str) -> bool {
    let rest = server.split_once("://").map_or(server, |(_, r)| r);
    let host = if let Some(v6) = rest.strip_prefix('[') {
        v6.split(']').next().unwrap_or_default()
    } else {
        rest.split(['/', ':', '?', '#']).next().unwrap_or_default()
    };
    let host = host.to_ascii_lowercase();
    host == "localhost" || host == "::1" || host.starts_with("127.") && host.split('.').count() == 4 && host.split('.').all(|p| p.parse::<u8>().is_ok())
}

/// What decides whether a context is protected.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Protection {
    /// `kubernetes.protected`: protected whatever else matches.
    pub extra: Vec<String>,
    /// `kubernetes.unprotected`: exempt, as a local one is.
    pub exempt: Vec<String>,
}

impl Protection {
    /// Whether a change in `context` (on `cluster`, at `server`, when
    /// kubeconfig says) needs its object's name typed. Every context is,
    /// unless it's clearly local -- its name or its cluster's named as a
    /// local cluster is, or its server this machine -- or exempted by
    /// name. `kubernetes.protected` wins over both.
    pub fn protects(&self, context: &str, cluster: Option<&str>, server: Option<&str>) -> bool {
        let names: Vec<&str> = std::iter::once(context).chain(cluster).collect();
        let any = |patterns: &[String]| names.iter().any(|n| patterns.iter().any(|p| glob(&p.to_lowercase(), &n.to_lowercase())));
        if any(&self.extra) {
            return true;
        }
        let local = LOCAL_NAMES.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        !(any(&self.exempt) || any(&local) || server.is_some_and(local_server))
    }
}

fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(kind: Kind, ns: &str, name: &str) -> Target {
        Target::new(kind, ns, name).unwrap()
    }

    #[test]
    fn names_are_what_kubernetes_allows_and_nothing_else() {
        for ok in ["web", "web-7d9c8b-x2k4q", "a", "db.primary", "0abc"] {
            assert_eq!(name(ok).as_deref(), Ok(ok));
        }
        for bad in ["", "-f", "--all-namespaces", "Web", "web ", " web", "web;rm", "a/b", "../x", "web?limit=1", "web&x", "$(id)", "`id`", "web\n", "wéb", "web-", ".web", "${data}"] {
            assert!(name(bad).is_err(), "{bad:?} let through");
        }
        assert!(name(&"a".repeat(253)).is_ok() && name(&"a".repeat(254)).is_err());
        assert!(label("shop").is_ok() && label("kube-system").is_ok());
        for bad in ["a.b", "-n", "--namespace=x", "Shop", "", "a/b", "a?b", "x/secrets?"] {
            assert!(label(bad).is_err(), "{bad:?}");
        }
        assert!(label(&"a".repeat(63)).is_ok() && label(&"a".repeat(64)).is_err());
        assert!(Target::new(Kind::Pod, "--all-namespaces", "web").is_err());
        assert!(Target::new(Kind::Pod, "shop", "-it").is_err());
    }

    #[test]
    fn a_context_can_be_anything_printable_and_stays_one_argument() {
        let odd = "arn:aws:eks:eu-west-1:123:cluster/prod; rm -rf ~ $(id) --as=admin";
        assert_eq!(context(odd).as_deref(), Ok(odd));
        assert_eq!(context("prod ").as_deref(), Ok("prod "), "kept exactly: never trimmed into another context");
        assert!(context("a\nb").is_err() && context("tab\there").is_err() && context("").is_err());
        let args = list(Kind::Pod, odd, Some("shop"), 500);
        assert_eq!(args, ["get", "--raw", "/api/v1/namespaces/shop/pods?limit=500", &format!("--context={odd}"), REQUEST_TIMEOUT]);
        assert_eq!(on("--as=system:admin"), "--context=--as=system:admin");
    }

    #[test]
    fn lists_are_capped_by_the_api_server_and_paths_encoded() {
        assert_eq!(list_path(Kind::Deployment, Some("shop"), 500, None), "/apis/apps/v1/namespaces/shop/deployments?limit=500");
        assert_eq!(list_path(Kind::Pod, None, 50, None), "/api/v1/pods?limit=50");
        assert_eq!(list_path(Kind::Ingress, Some("web"), 10, Some("eyJ2Ijoi+/=")), "/apis/networking.k8s.io/v1/namespaces/web/ingresses?limit=10&continue=eyJ2Ijoi%2B%2F%3D");
        // Even a namespace no check let through can't leave its segment.
        assert_eq!(list_path(Kind::Pod, Some("x/secrets?a=b"), 5, None), "/api/v1/namespaces/x%2Fsecrets%3Fa%3Db/pods?limit=5");
        assert_eq!(secrets("dev", None), ["get", "secrets", "--no-headers", "--context=dev", "--all-namespaces", REQUEST_TIMEOUT]);
        assert_eq!(events("dev", "shop", Some(("Pod", "web-1")), 500), ["get", "--raw", "/api/v1/namespaces/shop/events?limit=500&fieldSelector=involvedObject.kind%3DPod%2CinvolvedObject.name%3Dweb-1", "--context=dev", REQUEST_TIMEOUT]);
        assert_eq!(object("dev", "horizontalpodautoscaler", "shop", "web").unwrap()[2], "/apis/autoscaling/v2/namespaces/shop/horizontalpodautoscalers/web");
        assert_eq!(object("dev", "secret", "shop", "db"), None, "never a Secret");
        assert_eq!(object("dev", "node", "shop", "n1"), None, "nothing outside the namespace");
    }

    #[test]
    fn each_command_line_is_an_argv_with_values_in_their_flags() {
        let web = t(Kind::Pod, "shop", "web-1");
        assert_eq!(
            logs(&web, "kind-dev", &LogOptions { container: Some("nginx".into()), tail: 200, follow: true, ..LogOptions::default() }),
            ["logs", "--context=kind-dev", "--namespace=shop", "--container=nginx", "--timestamps=true", "--tail=200", "--follow=true", "web-1"]
        );
        let deploy = t(Kind::Deployment, "shop", "web");
        assert_eq!(
            logs(&deploy, "c", &LogOptions { previous: true, since: Some("10m".into()), follow: true, ..LogOptions::default() }),
            ["logs", "--context=c", "--namespace=shop", "--all-containers=true", "--prefix=true", "--timestamps=true", "--previous=true", "--since=10m", "deployment/web"],
            "previous is read once, never followed"
        );
        assert_eq!(exec(&web, "kind-dev", "nginx"), ["exec", "-i", "-t", "--context=kind-dev", "--namespace=shop", "--container=nginx", "web-1", "--", "sh", "-c", SHELL_SCRIPT]);
        assert_eq!(port_forward(&t(Kind::Service, "shop", "web"), "kind-dev", 8080, 80), ["port-forward", "--context=kind-dev", "--namespace=shop", "--address=127.0.0.1", "service/web", "8080:80"]);
        assert_eq!(delete(&web, "kind-dev"), ["delete", "pod", "web-1", "--wait=false", "--context=kind-dev", "--namespace=shop", REQUEST_TIMEOUT]);
        assert_eq!(rollout_restart(&deploy, "kind-dev"), ["rollout", "restart", "deployment/web", "--context=kind-dev", "--namespace=shop", REQUEST_TIMEOUT]);
        assert_eq!(scale(&deploy, "kind-dev", Some(3), 5), ["scale", "deployment/web", "--replicas=5", "--current-replicas=3", "--context=kind-dev", "--namespace=shop", REQUEST_TIMEOUT]);
        assert_eq!(get(&t(Kind::Secret, "shop", "db"), "c")[3..5], ["-o", "json"], "a Secret comes as JSON, to be redacted");
        assert_eq!(get(&deploy, "c")[3..5], ["-o", "yaml"]);
        let w = workloads("kind-dev", "shop");
        assert_eq!(w[..6], ["get", "deployments,statefulsets,daemonsets,jobs,cronjobs", "--context=kind-dev", "--namespace=shop", "--chunk-size=500", REQUEST_TIMEOUT]);
        assert_eq!(w.last().unwrap(), &format!("jsonpath={WORKLOAD_COLUMNS}"));
    }

    #[test]
    fn durations_times_and_ports_are_checked() {
        for ok in ["30s", "10m", "2h", "1h30m", "90s"] {
            assert_eq!(since(ok).as_deref(), Ok(ok));
        }
        for bad in ["", "10", "m", "10d", "-1h", "1h;id", "10 m", "--tail=1"] {
            assert!(since(bad).is_err(), "{bad:?}");
        }
        assert_eq!(since_time("2026-11-14T10:00:00.123456789Z").as_deref(), Some("2026-11-14T10:00:00.123456789Z"));
        assert_eq!(since_time("[pod/web/nginx]"), None);
        assert_eq!(ports("8080:80"), Ok((8080, 80)));
        for bad in ["8080", "0:80", "70000:80", "a:b", "80:", ":80", "8080:80:1"] {
            assert!(ports(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn every_context_is_protected_but_the_clearly_local_ones() {
        let p = Protection::default();
        // Protected: production's names, and names that say nothing.
        assert!(p.protects("arn:aws:eks:eu-west-1:123:cluster/shop", Some("arn:aws:eks:eu-west-1:123:cluster/shop"), Some("https://ABC.gr7.eu-west-1.eks.amazonaws.com")));
        assert!(p.protects("gke_acme_europe-west1_main", Some("gke_acme_europe-west1_main"), Some("https://34.1.2.3")));
        assert!(p.protects("staging", None, None), "unknown to kubeconfig: protected");
        // Local: by context name, cluster name or server.
        assert!(!p.protects("kind-dev", Some("kind-dev"), Some("https://127.0.0.1:6443")));
        assert!(!p.protects("minikube", None, None));
        assert!(!p.protects("dev", Some("k3d-dev"), None));
        assert!(!p.protects("lab", Some("lab"), Some("https://localhost:6443")));
        assert!(!p.protects("lab", Some("lab"), Some("https://[::1]:6443")));
        assert!(p.protects("lab", Some("lab"), Some("https://127.example.com:6443")));
        assert!(p.protects("lab", Some("lab"), Some("https://localhost.example.com")));
        // Exempt by name; protected by name over everything.
        let p = Protection { extra: vec!["*prod*".into()], exempt: vec!["dev-*".into()] };
        assert!(!p.protects("dev-eu", Some("dev-eu"), Some("https://10.0.0.1")));
        assert!(p.protects("kind-prod", Some("kind-prod"), Some("https://127.0.0.1:6443")), "kubernetes.protected wins");
        assert!(p.protects("tunnel", Some("prod-eu"), Some("https://127.0.0.1:16443")), "the cluster's name counts too");
    }
}
