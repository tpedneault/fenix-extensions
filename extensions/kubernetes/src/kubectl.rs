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
//! - **The selection is the extension's**: `--context` and `--namespace`
//!   on every call, from what you chose for this project. kubeconfig's
//!   current context changes only through `use_context`, a command of its
//!   own.

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

/// `--context=<it>`, when there's one.
fn on(context: Option<&str>) -> Vec<String> {
    context.map(|c| vec![format!("--context={c}")]).unwrap_or_default()
}

/// `--context` and `--namespace`.
fn scope(context: Option<&str>, namespace: &str) -> Vec<String> {
    let mut out = on(context);
    out.push(format!("--namespace={namespace}"));
    out
}

/// kubectl's own version, asking nothing of a cluster.
pub fn client_version() -> Vec<String> {
    words(&["version", "--client", "-o", "json"])
}

/// What kubeconfig says, without its credentials: the current context on
/// the first line, then each context's name, cluster and namespace, a tab
/// between. kubectl merges `KUBECONFIG` itself.
pub fn config_view() -> Vec<String> {
    words(&["config", "view", "-o", r#"jsonpath={.current-context}{"\n"}{range .contexts[*]}{.name}{"\t"}{.context.cluster}{"\t"}{.context.namespace}{"\n"}{end}"#])
}

/// The cluster's version: whether it's reachable, as `context`.
pub fn server_version(context: Option<&str>) -> Vec<String> {
    let mut out = words(&["version", "-o", "json", "--request-timeout=10s"]);
    out.extend(on(context));
    out
}

/// The API path that lists `kind` in `namespace` (every one: `None`), at
/// most `limit`, from `from` (a `continue` the last answer gave).
pub fn list_path(kind: Kind, namespace: Option<&str>, limit: usize, from: Option<&str>) -> String {
    let mut path = match namespace {
        Some(ns) => format!("{}/namespaces/{ns}/{}", kind.api(), kind.resource()),
        None => format!("{}/{}", kind.api(), kind.resource()),
    };
    path.push_str(&format!("?limit={limit}"));
    if let Some(token) = from {
        path.push_str("&continue=");
        path.push_str(&encode(token));
    }
    path
}

/// `kind`, at most `limit`, from the API server itself: `get --raw` with
/// `limit` stops it there, and says how many more there are
/// (`remainingItemCount`) -- not every pod fetched and parsed to show
/// 500. Not for Secrets: `secrets` lists those.
pub fn list(kind: Kind, context: Option<&str>, namespace: Option<&str>, limit: usize) -> Vec<String> {
    debug_assert!(kind != Kind::Secret, "a Secret's data is never fetched to be listed");
    let mut out = vec!["get".to_string(), "--raw".to_string(), list_path(kind, namespace, limit, None)];
    out.extend(on(context));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// The Secrets: kubectl's own table (name, type, how many keys, age),
/// never their data, which stays in kubectl.
pub fn secrets(context: Option<&str>, namespace: Option<&str>) -> Vec<String> {
    let mut out = words(&["get", "secrets", "--no-headers"]);
    out.extend(on(context));
    match namespace {
        Some(ns) => out.push(format!("--namespace={ns}")),
        None => out.push("--all-namespaces".into()),
    }
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// The namespaces, for the picker.
pub fn namespaces(context: Option<&str>) -> Vec<String> {
    let mut out = words(&["get", "--raw", "/api/v1/namespaces?limit=1000"]);
    out.extend(on(context));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// Events in `namespace`, about one object when `about` says, at most
/// `limit`.
pub fn events(context: Option<&str>, namespace: &str, about: Option<(&str, &str)>, limit: usize) -> Vec<String> {
    let mut path = format!("/api/v1/namespaces/{namespace}/events?limit={limit}");
    if let Some((kind, name)) = about {
        path.push_str(&format!("&fieldSelector=involvedObject.kind%3D{kind},involvedObject.name%3D{name}"));
    }
    let mut out = vec!["get".to_string(), "--raw".to_string(), path];
    out.extend(on(context));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// One pod, as JSON: its containers and their state.
pub fn pod(context: Option<&str>, namespace: &str, pod: &str) -> Vec<String> {
    let mut out = vec!["get".to_string(), "--raw".to_string(), format!("/api/v1/namespaces/{namespace}/pods/{pod}")];
    out.extend(on(context));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// An object as YAML -- a Secret as JSON, so its values can be taken out
/// before anything shows it.
pub fn get(target: &Target, context: Option<&str>) -> Vec<String> {
    let output = if target.kind == Kind::Secret { "json" } else { "yaml" };
    let mut out = vec!["get".to_string(), target.kind.singular().to_string(), target.name.clone(), "-o".to_string(), output.to_string()];
    out.extend(scope(context, &target.namespace));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// `kubectl describe`.
pub fn describe(target: &Target, context: Option<&str>) -> Vec<String> {
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
pub fn logs(target: &Target, context: Option<&str>, o: &LogOptions) -> Vec<String> {
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
pub fn exec(target: &Target, context: Option<&str>, container: &str) -> Vec<String> {
    let mut out = words(&["exec", "-i", "-t"]);
    out.extend(scope(context, &target.namespace));
    out.push(format!("--container={container}"));
    out.push(target.name.clone());
    out.extend(words(&["--", "sh", "-c", SHELL_SCRIPT]));
    out
}

/// A port-forward, on this machine's loopback only.
pub fn port_forward(target: &Target, context: Option<&str>, local: u16, remote: u16) -> Vec<String> {
    let mut out = vec!["port-forward".to_string()];
    out.extend(scope(context, &target.namespace));
    out.push("--address=127.0.0.1".into());
    out.push(target.reference());
    out.push(format!("{local}:{remote}"));
    out
}

/// A pod deleted, without waiting for it to go.
pub fn delete(target: &Target, context: Option<&str>) -> Vec<String> {
    let mut out = vec!["delete".to_string(), target.kind.singular().to_string(), target.name.clone(), "--wait=false".to_string()];
    out.extend(scope(context, &target.namespace));
    out.push(REQUEST_TIMEOUT.into());
    out
}

pub fn rollout_restart(target: &Target, context: Option<&str>) -> Vec<String> {
    let mut out = vec!["rollout".to_string(), "restart".to_string(), target.reference()];
    out.extend(scope(context, &target.namespace));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// Scaled to `to`, only if it's still at `from` (`--current-replicas`):
/// someone else's change in between makes it fail, not lose.
pub fn scale(target: &Target, context: Option<&str>, from: Option<u32>, to: u32) -> Vec<String> {
    let mut out = vec!["scale".to_string(), target.reference(), format!("--replicas={to}")];
    if let Some(from) = from {
        out.push(format!("--current-replicas={from}"));
    }
    out.extend(scope(context, &target.namespace));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// What applying the YAML on stdin would do, done by the API server
/// without keeping it: the object it would be, as JSON.
pub fn apply_dry_run(namespace: &str, context: Option<&str>) -> Vec<String> {
    let mut out = words(&["apply", "--dry-run=server", "-o", "json", "-f", "-"]);
    out.extend(scope(context, namespace));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// What applying the YAML on stdin would change, as a diff (the server's
/// dry run). kubectl runs `diff` itself, or `KUBECTL_EXTERNAL_DIFF`.
pub fn diff(namespace: &str, context: Option<&str>) -> Vec<String> {
    let mut out = words(&["diff", "-f", "-"]);
    out.extend(scope(context, namespace));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// The YAML on stdin applied.
pub fn apply(namespace: &str, context: Option<&str>) -> Vec<String> {
    let mut out = words(&["apply", "-f", "-"]);
    out.extend(scope(context, namespace));
    out.push(REQUEST_TIMEOUT.into());
    out
}

/// kubeconfig's current context changed -- for every tool that reads it,
/// not only this extension. The name after `--`, so one that starts with
/// `-` is still a name.
pub fn use_context(name: &str) -> Vec<String> {
    vec!["config".into(), "use-context".into(), "--".into(), name.into()]
}

/// `text` percent-encoded for a URL's query: what isn't a letter, a digit
/// or `-._~` becomes `%XX`.
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

/// Whether `context` matches one of the `patterns` (`*prod*`, `eks-*`):
/// `*` any characters, `?` one, letters' case ignored.
pub fn protected(context: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| glob(&p.to_lowercase(), &context.to_lowercase()))
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
        for bad in ["a.b", "-n", "--namespace=x", "Shop", "", "a/b", "a?b"] {
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
        assert!(context("a\nb").is_err() && context("").is_err());
        let args = list(Kind::Pod, Some(odd), Some("shop"), 500);
        assert_eq!(args, ["get", "--raw", "/api/v1/namespaces/shop/pods?limit=500", &format!("--context={odd}"), REQUEST_TIMEOUT]);
        // A context named like a flag is still the context's value.
        assert_eq!(on(Some("--as=system:admin")), ["--context=--as=system:admin"]);
    }

    #[test]
    fn lists_are_capped_by_the_api_server() {
        assert_eq!(list_path(Kind::Deployment, Some("shop"), 500, None), "/apis/apps/v1/namespaces/shop/deployments?limit=500");
        assert_eq!(list_path(Kind::Pod, None, 50, None), "/api/v1/pods?limit=50");
        assert_eq!(list_path(Kind::Ingress, Some("web"), 10, Some("eyJ2Ijoi+/=")), "/apis/networking.k8s.io/v1/namespaces/web/ingresses?limit=10&continue=eyJ2Ijoi%2B%2F%3D");
        assert_eq!(list_path(Kind::CronJob, Some("ops"), 5, None), "/apis/batch/v1/namespaces/ops/cronjobs?limit=5");
        assert_eq!(secrets(Some("dev"), None), ["get", "secrets", "--no-headers", "--context=dev", "--all-namespaces", REQUEST_TIMEOUT]);
        assert_eq!(secrets(None, Some("shop")), ["get", "secrets", "--no-headers", "--namespace=shop", REQUEST_TIMEOUT]);
        assert_eq!(events(Some("dev"), "shop", Some(("Pod", "web-1")), 500), ["get", "--raw", "/api/v1/namespaces/shop/events?limit=500&fieldSelector=involvedObject.kind%3DPod,involvedObject.name%3Dweb-1", "--context=dev", REQUEST_TIMEOUT]);
    }

    #[test]
    fn each_command_line_is_an_argv_with_values_in_their_flags() {
        let web = t(Kind::Pod, "shop", "web-1");
        assert_eq!(
            logs(&web, Some("kind-dev"), &LogOptions { container: Some("nginx".into()), tail: 200, follow: true, ..LogOptions::default() }),
            ["logs", "--context=kind-dev", "--namespace=shop", "--container=nginx", "--timestamps=true", "--tail=200", "--follow=true", "web-1"]
        );
        let deploy = t(Kind::Deployment, "shop", "web");
        assert_eq!(
            logs(&deploy, None, &LogOptions { previous: true, since: Some("10m".into()), follow: true, ..LogOptions::default() }),
            ["logs", "--namespace=shop", "--all-containers=true", "--prefix=true", "--timestamps=true", "--previous=true", "--since=10m", "deployment/web"],
            "previous is read once, never followed"
        );
        assert_eq!(exec(&web, Some("kind-dev"), "nginx"), ["exec", "-i", "-t", "--context=kind-dev", "--namespace=shop", "--container=nginx", "web-1", "--", "sh", "-c", SHELL_SCRIPT]);
        assert_eq!(port_forward(&t(Kind::Service, "shop", "web"), Some("kind-dev"), 8080, 80), ["port-forward", "--context=kind-dev", "--namespace=shop", "--address=127.0.0.1", "service/web", "8080:80"]);
        assert_eq!(delete(&web, Some("kind-dev")), ["delete", "pod", "web-1", "--wait=false", "--context=kind-dev", "--namespace=shop", REQUEST_TIMEOUT]);
        assert_eq!(rollout_restart(&deploy, Some("kind-dev")), ["rollout", "restart", "deployment/web", "--context=kind-dev", "--namespace=shop", REQUEST_TIMEOUT]);
        assert_eq!(scale(&deploy, Some("kind-dev"), Some(3), 5), ["scale", "deployment/web", "--replicas=5", "--current-replicas=3", "--context=kind-dev", "--namespace=shop", REQUEST_TIMEOUT]);
        assert_eq!(apply_dry_run("shop", Some("kind-dev")), ["apply", "--dry-run=server", "-o", "json", "-f", "-", "--context=kind-dev", "--namespace=shop", REQUEST_TIMEOUT]);
        assert_eq!(use_context("-weird"), ["config", "use-context", "--", "-weird"]);
        assert_eq!(get(&t(Kind::Secret, "shop", "db"), None)[3..5], ["-o", "json"], "a Secret comes as JSON, to be redacted");
        assert_eq!(get(&deploy, None)[3..5], ["-o", "yaml"]);
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
    fn protected_contexts_match_their_patterns() {
        let p = vec!["*prod*".to_string()];
        assert!(protected("prod", &p) && protected("eks-PROD-eu", &p) && protected("gke_acme_europe-west1_production", &p));
        assert!(!protected("kind-dev", &p) && !protected("staging", &p));
        assert!(!protected("prod", &[]));
        let p = vec!["eks-?-live".to_string(), "*@main".to_string()];
        assert!(protected("eks-a-live", &p) && protected("me@main", &p) && !protected("eks-ab-live", &p));
    }
}
