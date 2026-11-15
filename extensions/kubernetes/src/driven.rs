//! The extension driven as Fenix drives it, on the test kit's fake host.
//!
//! kubectl's answers: `config view -o json`, the versions, the one-call
//! workload columns and the error texts are kubectl 1.37.2's own,
//! recorded against a fake API server serving the fixture lists; the
//! lists, the pod, the Secret and the YAML are hand-written from the
//! API's documented objects (`tests/fixtures/README.md` says which is
//! which).
//!
//! Most fakes here answer at once. Those that test what waits on a
//! process don't: their processes are left running (`Ran::running`), and
//! the test says their lines and their ends itself, late and out of
//! order, to show that an answer to an earlier request is never taken
//! for a newer one's.
//!
//! None of it touches the filesystem: nothing here makes or shares a
//! folder.

use fenix_test_kit::{assert_snapshot, Fake, Harness, Ran};
use serde_json::Value;

use crate::api::chrono::DateTime;
use crate::api::{Event, Extension, Key};
use crate::kubectl::{self, Kind, Target};
use crate::Kube;

/// kubectl 1.37.2's `config view -o json` of a kubeconfig whose current
/// context is `"prod "` (with its space).
const CONFIG_REAL: &str = include_str!("../tests/fixtures/config-view.json");
const CLIENT: &str = include_str!("../tests/fixtures/version-client.json");
const VERSION: &str = include_str!("../tests/fixtures/version.json");
const WORKLOADS: &str = include_str!("../tests/fixtures/workloads.tsv");
const UNAUTHORIZED: &str = include_str!("../tests/fixtures/unauthorized.stderr");
const DEPLOYMENTS: &str = include_str!("../tests/fixtures/deployments.json");
const STATEFULSETS: &str = include_str!("../tests/fixtures/statefulsets.json");
const DAEMONSETS: &str = include_str!("../tests/fixtures/daemonsets.json");
const JOBS: &str = include_str!("../tests/fixtures/jobs.json");
const CRONJOBS: &str = include_str!("../tests/fixtures/cronjobs.json");
const PODS: &str = include_str!("../tests/fixtures/pods.json");
const SERVICES: &str = include_str!("../tests/fixtures/services.json");
const INGRESSES: &str = include_str!("../tests/fixtures/ingresses.json");
const NAMESPACES: &str = include_str!("../tests/fixtures/namespaces.json");
const EVENTS: &str = include_str!("../tests/fixtures/events.json");
const POD: &str = include_str!("../tests/fixtures/pod-web.json");
const SECRETS: &str = include_str!("../tests/fixtures/secrets.txt");
const SECRET: &str = include_str!("../tests/fixtures/secret-db.json");
const YAML: &str = include_str!("../tests/fixtures/deployment-web.yaml");

/// The recorded kubeconfig with `current` as its current context.
fn config(current: &str) -> String {
    CONFIG_REAL.replace(r#""current-context": "prod ""#, &format!(r#""current-context": {}"#, serde_json::to_string(current).unwrap()))
}

/// What a list's API path answers, whatever its namespace.
fn list_for(path: &str) -> Option<&'static str> {
    let resource = path.split('?').next()?.rsplit('/').next()?;
    Some(match resource {
        "deployments" => DEPLOYMENTS,
        "statefulsets" => STATEFULSETS,
        "daemonsets" => DAEMONSETS,
        "jobs" => JOBS,
        "cronjobs" => CRONJOBS,
        "pods" => PODS,
        "services" => SERVICES,
        "ingresses" => INGRESSES,
        "namespaces" => NAMESPACES,
        "events" => EVENTS,
        _ => return None,
    })
}

/// kubectl, kubeconfig and a cluster as Fenix's defaults and the
/// fixtures have them: `kind-dev` current (named as kind names its clusters:
/// local, so unprotected), namespace `shop`.
fn cluster() -> Fake {
    cluster_on("kind-dev")
}

fn cluster_on(current: &str) -> Fake {
    let fake = Fake::new();
    fake.clock(DateTime::parse_from_rfc3339("2026-11-14T12:00:00Z").unwrap())
        // The manifest's defaults, as Fenix gives them.
        .setting("kubernetes.protected_mode", "type-name")
        .setting("kubernetes.refresh_seconds", "10")
        .setting("kubernetes.max_items", "500")
        .setting("kubernetes.agent_logs", "false")
        .on_spawn("kubectl version --client", Ran::ok(CLIENT))
        .on_spawn("kubectl config view", Ran::ok(config(current)))
        .on_spawn("kubectl version -o json", Ran::ok(VERSION))
        .on_spawn("kubectl get secrets", Ran::ok(SECRETS))
        .on_spawn("kubectl get deployments,statefulsets,daemonsets,jobs,cronjobs", Ran::ok(WORKLOADS));
    fake.on_spawn_with(|cmd| {
        let a = &cmd.args;
        (a.len() > 2 && a[0] == "get" && a[1] == "--raw").then(|| list_for(&a[2]).map(|t| Ok(Ran::ok(t)))).flatten()
    });
    fake
}

fn opened(fake: &Fake) -> (Harness<Kube>, u64) {
    let mut h: Harness<Kube> = Harness::with_fake(fake.clone());
    h.activate().unwrap();
    h.command("kubernetes.open").unwrap();
    h.size = (120, 24);
    let page = h.page("main");
    (h, page)
}

/// The command lines spawned since the fake was last asked, a string each.
fn spawned(fake: &Fake) -> Vec<String> {
    fake.take().into_iter().filter(|c| c.starts_with("spawn ")).map(|c| c.splitn(3, ' ').nth(2).unwrap_or_default().to_string()).collect()
}

fn keys(h: &mut Harness<Kube>, page: u64, keys: &str) {
    for c in keys.chars() {
        h.key(page, Key::Char(c));
    }
}

/// The newest process `pattern` names: its id.
fn pid(fake: &Fake, pattern: &str) -> u64 {
    fake.process(pattern).unwrap_or_else(|| panic!("no process {pattern}")).id
}

/// Process `p` prints `text` (each line) and ends with `code`.
fn finish(h: &mut Harness<Kube>, p: u64, text: &str, code: i32) {
    for l in text.lines() {
        h.said(p, l, code != 0);
    }
    h.exited(p, Some(code));
}

#[test]
fn the_page_lists_what_kubectl_answered() {
    let fake = cluster();
    let (mut h, page) = opened(&fake);
    let calls = fake.take();
    assert!(calls.iter().any(|c| c.ends_with("kubectl version --client -o json")), "{calls:#?}");
    assert!(calls.iter().any(|c| c.ends_with("kubectl config view -o json")));
    assert!(calls.iter().any(|c| c.contains("kubectl get --raw /apis/apps/v1/namespaces/shop/deployments?limit=500 --context=kind-dev --request-timeout=20s")), "{calls:#?}");
    assert!(calls.iter().any(|c| c.contains("kubectl version -o json --request-timeout=10s --context=kind-dev")));
    assert_snapshot!("workloads", h.screen(page));
    keys(&mut h, page, "j");
    h.key(page, Key::Enter);
    assert_snapshot!("workloads-web-open", h.screen(page));
    keys(&mut h, page, "2");
    assert_snapshot!("pods", h.screen(page));
    keys(&mut h, page, "3");
    assert_snapshot!("network", h.screen(page));
    keys(&mut h, page, "4");
    assert_snapshot!("secrets", h.screen(page));
    let check = fake.checks().pop().unwrap();
    assert_eq!(check.label, "kubectl");
    assert_eq!(check.detail, "v1.37.2; kind-dev answers (server v1.30.4)");
    // Every process was kubectl, and every one that reached the cluster
    // named its context.
    for (id, _) in fake.spawned() {
        let p = fake.state().processes[&id].clone();
        assert_eq!(p.cmd.program, "kubectl");
        let local = p.cmd.args[..2] == ["config", "view"] || p.cmd.args.contains(&"--client".to_string());
        assert!(local || p.cmd.args.contains(&"--context=kind-dev".to_string()), "{:?}", p.cmd.args);
    }
}

#[test]
fn the_timer_asks_only_while_the_page_is_in_view() {
    let fake = cluster();
    let (mut h, page) = opened(&fake);
    fake.take();
    h.fire_timers();
    assert_eq!(spawned(&fake).len(), 6, "the tab in view's five kinds and pods, again");
    h.event(Event::PageShown((page, false)));
    assert!(fake.take().iter().any(|c| c.starts_with("cancel-timer")));
    h.fire_timers();
    assert!(spawned(&fake).is_empty(), "nothing asked while it's hidden");
    h.event(Event::PageShown((page, true)));
    assert!(fake.calls().iter().any(|c| c.ends_with("10000 repeat")));
}

#[test]
fn the_context_and_namespace_are_this_projects_and_kubeconfig_is_never_changed() {
    let fake = cluster();
    fake.on_pick("Context, for this project", "gke_acme_europe-west1_prod").on_pick("Namespace", "All namespaces");
    let (mut h, page) = opened(&fake);
    assert!(h.screen(page).contains("context kind-dev (kubeconfig's current, read 12:00)"), "{}", h.screen(page));
    fake.take();
    keys(&mut h, page, "c");
    let picker = fake.pickers().pop().unwrap();
    let names: Vec<&str> = picker.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(names, ["fake", "gke_acme_europe-west1_prod", "kind-dev", "prod", "prod "], "exactly as kubeconfig has them; a tab or a newline left out");
    assert!(picker.items[0].detail.ends_with("protected"), "fake's server is on 127.0.0.1, which says nothing: {}", picker.items[0].detail);
    assert!(picker.items[1].detail.ends_with("protected") && picker.items[2].detail.ends_with("unprotected: a local cluster's name"));
    let lines = spawned(&fake);
    assert!(!lines.is_empty() && lines.iter().all(|l| l.contains("--context=gke_acme_europe-west1_prod")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("/namespaces/payments/deployments")), "the context's own namespace");
    assert!(h.screen(page).contains("context gke_acme_europe-west1_prod protected (chosen here)   namespace payments"), "{}", h.screen(page));
    keys(&mut h, page, "n");
    let lines = spawned(&fake);
    assert!(lines[0].starts_with("kubectl get --raw /api/v1/namespaces?limit=1000 --context=gke_acme_europe-west1_prod"), "{lines:#?}");
    assert!(lines.iter().any(|l| l.starts_with("kubectl get --raw /apis/apps/v1/deployments?limit=500 --context=gke_acme_europe-west1_prod")), "{lines:#?}");
    keys(&mut h, page, "4");
    assert!(spawned(&fake).iter().any(|l| l.contains("get secrets --no-headers --context=gke_acme_europe-west1_prod --all-namespaces")));
    assert_eq!(h.ext.page_state(page).unwrap(), r#"{"context":"gke_acme_europe-west1_prod","namespace":"*","tab":3}"#);
    // Nothing anywhere changed kubeconfig: no command that could.
    assert!(!fake.calls().iter().any(|l| l.contains("use-context") || l.contains("set-context")));
    assert!(h.command("kubernetes.use_context").is_err(), "the global switch is gone");
}

/// `"prod "` and `"prod"` are two contexts: the one kubeconfig names is the
/// one used, its trailing space and all.
#[test]
fn a_context_name_is_used_exactly_as_kubeconfig_has_it() {
    let fake = cluster_on("prod ");
    let (mut h, page) = opened(&fake);
    let lines = spawned(&fake);
    assert!(lines.iter().any(|l| l.contains("--context=prod  --request-timeout")), "{lines:#?}");
    let p = fake.process("kubectl get --raw /api/v1/namespaces/default/pods?limit=500").unwrap();
    assert!(p.cmd.args.contains(&"--context=prod ".to_string()) && !p.cmd.args.contains(&"--context=prod".to_string()));
    assert!(h.screen(page).contains("context prod  protected"), "prod-cluster isn't local: {}", h.screen(page));
    assert!(fake.notifications().iter().any(|n| n.contains("2 context(s) whose names hold a tab or a newline: left out")));
}

/// A setting's context (yours or the project's: Fenix doesn't say which)
/// is only a suggestion: nothing, agents included, uses it until `u`.
#[test]
fn a_suggested_context_is_used_only_once_taken() {
    let fake = cluster();
    fake.setting("kubernetes.context", "gke_acme_europe-west1_prod").setting("kubernetes.namespace", "billing");
    let (mut h, page) = opened(&fake);
    let screen = h.screen(page);
    assert!(screen.contains("context kind-dev (kubeconfig's current"), "{screen}");
    assert!(screen.contains("u uses the suggested context gke_acme_europe-west1_prod, namespace billing (from kubernetes.context and .namespace"), "{screen}");
    assert!(!fake.calls().iter().any(|c| c.contains("gke_acme")), "not used before it's taken");
    h.serve("kubernetes.pods", "{}");
    assert!(fake.process("kubectl get --raw /api/v1/namespaces/shop/pods?limit=500").unwrap().cmd.args.contains(&"--context=kind-dev".to_string()), "nor by agents");
    fake.take();
    keys(&mut h, page, "u");
    assert!(spawned(&fake).iter().all(|l| l.contains("--context=gke_acme_europe-west1_prod")));
    let screen = h.screen(page);
    assert!(screen.contains("(chosen here)   namespace billing") && !screen.contains("suggested"), "{screen}");
}

#[test]
fn a_long_list_stops_at_the_cap_and_says_how_many_more() {
    let fake = cluster();
    let pod = r#"{"metadata":{"name":"p","namespace":"shop","creationTimestamp":"2026-11-14T11:00:00Z"},"spec":{"containers":[{"name":"c"}]},"status":{"phase":"Running","containerStatuses":[{"name":"c","ready":true,"state":{"running":{}}}]}}"#;
    let items: Vec<String> = (0..500).map(|n| pod.replace("\"p\"", &format!("\"p-{n}\""))).collect();
    let big = format!(r#"{{"kind":"PodList","metadata":{{"continue":"eyJ2IjoibWV0YS5rOHMuaW8vdjEiLCJydiI6NDgyMTF9","remainingItemCount":4500}},"items":[{}]}}"#, items.join(","));
    fake.on_spawn_with(move |cmd| cmd.args.iter().any(|a| a.starts_with("/api/v1/namespaces/shop/pods?limit=500")).then(|| Ok(Ran::ok(big.clone()))));
    let (mut h, page) = opened(&fake);
    keys(&mut h, page, "2");
    let screen = h.screen(page);
    assert!(screen.contains("… 4500 more pods not listed (the first 500): + lists more"), "{screen}");
    fake.take();
    keys(&mut h, page, "+");
    assert!(spawned(&fake).iter().any(|l| l.starts_with("kubectl get --raw /api/v1/namespaces/shop/pods?limit=1000")));
    for _ in 0..12 {
        keys(&mut h, page, "+");
    }
    assert!(fake.notifications().last().unwrap().contains("5000 pods is as many as the page lists"));
}

/// Parsing 5,000 pods' JSON, as `get --raw` gives a list: the numbers in
/// the README. `cargo test --release -- --ignored --nocapture measure`.
#[test]
#[ignore]
fn measure_parsing_5000_pods() {
    let one = r#"{"metadata":{"name":"web-7d9c8b6f4-xNNNN","namespace":"shop","uid":"0b1c2d3e","resourceVersion":"48000","creationTimestamp":"2026-11-12T09:01:00Z","labels":{"app":"web","pod-template-hash":"7d9c8b6f4"},"ownerReferences":[{"apiVersion":"apps/v1","kind":"ReplicaSet","name":"web-7d9c8b6f4","uid":"r1","controller":true}],"managedFields":[{"manager":"kube-controller-manager","operation":"Update","fieldsType":"FieldsV1","fieldsV1":{"f:metadata":{"f:labels":{".":{},"f:app":{}}},"f:spec":{"f:containers":{"k:{\"name\":\"nginx\"}":{".":{},"f:image":{},"f:name":{}}}}}}]},"spec":{"volumes":[{"name":"kube-api-access-abcde","projected":{"sources":[{"serviceAccountToken":{"expirationSeconds":3607,"path":"token"}}]}}],"containers":[{"name":"nginx","image":"nginx:1.27","ports":[{"containerPort":8080,"protocol":"TCP"}],"env":[{"name":"MODE","value":"production"}],"resources":{"limits":{"cpu":"500m","memory":"256Mi"}},"livenessProbe":{"httpGet":{"path":"/healthz","port":8080}}},{"name":"log-shipper","image":"fluent/fluent-bit:3.1"}],"nodeName":"kind-worker","tolerations":[{"key":"node.kubernetes.io/not-ready","operator":"Exists","effect":"NoExecute","tolerationSeconds":300}]},"status":{"phase":"Running","conditions":[{"type":"Ready","status":"True","lastTransitionTime":"2026-11-12T09:01:05Z"}],"hostIP":"172.18.0.3","podIP":"10.244.1.7","startTime":"2026-11-12T09:01:00Z","containerStatuses":[{"name":"nginx","state":{"running":{"startedAt":"2026-11-12T09:01:04Z"}},"lastState":{},"ready":true,"restartCount":0,"image":"docker.io/library/nginx:1.27","imageID":"docker.io/library/nginx@sha256:0000","containerID":"containerd://0000","started":true},{"name":"log-shipper","state":{"running":{}},"lastState":{},"ready":true,"restartCount":0,"image":"fluent/fluent-bit:3.1"}],"qosClass":"Burstable"}}"#;
    for n in [500usize, 2000, 5000] {
        let items: Vec<String> = (0..n).map(|i| one.replace("xNNNN", &format!("x{i:05}"))).collect();
        let text = format!(r#"{{"kind":"PodList","apiVersion":"v1","metadata":{{"resourceVersion":"1"}},"items":[{}]}}"#, items.join(","));
        let start = std::time::Instant::now();
        let listed = crate::model::pods(&text).unwrap();
        let took = start.elapsed();
        assert_eq!(listed.items.len(), n);
        println!("{n} pods: {:.1} MB of JSON parsed in {:.1} ms", text.len() as f64 / 1e6, took.as_secs_f64() * 1000.0);
    }
}

#[test]
fn five_thousand_pods_parse() {
    let pod = r#"{"metadata":{"name":"p","namespace":"shop"},"spec":{"containers":[{"name":"c"}]},"status":{"phase":"Running"}}"#;
    let items: Vec<String> = (0..crate::MOST_ITEMS).map(|n| pod.replace("\"p\"", &format!("\"p-{n}\""))).collect();
    let listed = crate::model::pods(&format!(r#"{{"items":[{}]}}"#, items.join(","))).unwrap();
    assert_eq!(listed.items.len(), crate::MOST_ITEMS);
}

#[test]
fn a_log_streams_stops_and_an_old_streams_lines_go_nowhere() {
    let fake = cluster();
    fake.on_pick("Logs of which container", "nginx").on_spawn("kubectl logs", Ran::running(""));
    let (mut h, page) = opened(&fake);
    keys(&mut h, page, "2");
    keys(&mut h, page, "l");
    let logs = h.page("logs");
    let first = pid(&fake, "kubectl logs");
    assert_eq!(fake.process("kubectl logs").unwrap().cmd.args, ["logs", "--context=kind-dev", "--namespace=shop", "--container=nginx", "--timestamps=true", "--tail=200", "--follow=true", "web-7d9c8b6f4-x2k4q"]);
    h.said(first, "2026-11-14T11:59:00.100000000Z GET / 200", false);
    h.said(first, "2026-11-14T11:59:01.200000000Z ERROR upstream timed out", false);
    h.size = (90, 10);
    assert_snapshot!("logs", h.screen(logs));
    // p: the previous run, in a new process; the first is stopped, and
    // what it still says isn't the new log's.
    keys(&mut h, logs, "p");
    let second = pid(&fake, "kubectl logs");
    assert_ne!(first, second);
    assert!(fake.state().processes[&first].killed);
    h.said(first, "2026-11-14T11:59:02Z LATE line from the first stream", false);
    h.exited(first, Some(0));
    h.said(second, "2026-11-14T10:00:00Z previous run's line", false);
    let screen = h.screen(logs);
    assert!(!screen.contains("LATE") && screen.contains("previous run's line"), "{screen}");
    assert!(!screen.contains("ended"), "the first stream's end isn't the second's: {screen}");
    // s stops it; s again starts from where it stopped.
    keys(&mut h, logs, "p");
    let third = pid(&fake, "kubectl logs");
    h.said(third, "2026-11-14T11:59:05.000000001Z after", false);
    keys(&mut h, logs, "s");
    assert!(fake.state().processes[&third].killed);
    assert!(h.screen(logs).contains("stopped: s starts it again"));
    keys(&mut h, logs, "s");
    assert!(fake.process("kubectl logs").unwrap().cmd.args.contains(&"--since-time=2026-11-14T11:59:05.000000001Z".to_string()));
    // Closing the page stops its stream.
    let last = pid(&fake, "kubectl logs");
    h.close(logs);
    assert!(fake.state().processes[&last].killed);
    // A stream that ends by itself says why, and isn't started again.
    fake.on_spawn("kubectl logs", Ran::fails(1, "Error from server (NotFound): pods \"web-7d9c8b6f4-x2k4q\" not found"));
    keys(&mut h, page, "l");
    let logs = h.page("logs");
    h.size = (160, 10);
    assert!(h.screen(logs).contains("ended: (NotFound): pods \"web-7d9c8b6f4-x2k4q\" not found"), "{}", h.screen(logs));
    fake.take();
    h.fire_timers();
    assert!(!spawned(&fake).iter().any(|l| l.contains("logs")), "no retry");
}

#[test]
fn a_port_forward_runs_as_a_task_until_stopped() {
    let fake = cluster();
    fake.on_prompt("Port-forward service/web", "8080:80");
    let (mut h, page) = opened(&fake);
    keys(&mut h, page, "3j");
    fake.take();
    keys(&mut h, page, "f");
    let prompt = fake.prompts().pop().unwrap();
    assert_eq!((prompt.label.as_str(), prompt.initial.as_str()), ("Port-forward service/web (kind-dev/shop): local:remote", "8080:80"));
    let calls = fake.take();
    let task = calls.iter().find(|c| c.starts_with("background ")).expect("a background task");
    assert!(task.ends_with("port-forward service/web 8080:80: kubectl port-forward --context=kind-dev --namespace=shop --address=127.0.0.1 service/web 8080:80"), "{task}");
    let id: u64 = task.split(' ').nth(1).unwrap().parse().unwrap();
    keys(&mut h, page, "5");
    assert_snapshot!("forwards", h.screen(page));
    keys(&mut h, page, "3f");
    assert!(fake.notifications().last().unwrap().contains("localhost:8080 is already forwarded"));
    keys(&mut h, page, "5s");
    assert!(fake.take().contains(&format!("cancel {id}")));
    h.event(Event::TaskFinished((id, false)));
    assert!(fake.notifications().last().unwrap().starts_with("service/web 8080:80 ended"));
    assert!(h.screen(page).contains("No port-forwards"));
    fake.on_prompt("Port-forward", "80:http");
    keys(&mut h, page, "3f");
    assert!(!fake.take().iter().any(|c| c.starts_with("background ")));
}

/// Every value goes into kubectl as an argument of its own: a context
/// named like a shell line, or like a flag, stays one argument, and
/// nothing is ever run but kubectl.
#[test]
fn every_command_line_is_an_argv_and_nothing_runs_but_kubectl() {
    let odd = "ctx; rm -rf ~ $(id) `id` --as=system:admin";
    let fake = cluster();
    fake.setting("kubernetes.context", odd).on_pick("Shell in which container", "log-shipper");
    let (mut h, page) = opened(&fake);
    keys(&mut h, page, "u");
    for (id, _) in fake.spawned() {
        let p = fake.state().processes[&id].clone();
        assert_eq!(p.cmd.program, "kubectl", "only kubectl, never a shell");
        if p.cmd.args[0] == "get" && p.cmd.args[1] == "--raw" {
            let ctx: Vec<&String> = p.cmd.args.iter().filter(|a| a.starts_with("--context=")).collect();
            if ctx[0] != "--context=kind-dev" {
                assert_eq!(ctx, [&format!("--context={odd}")], "{:?}", p.cmd.args);
            }
        }
    }
    // The odd context lists nothing here; the shell's command line is
    // what matters: kubectl exec in a terminal, the pod alone before --.
    h.ext.cluster.pods = h.ext.cluster.pods.clone();
    let t = Target::new(Kind::Pod, "shop", "web-7d9c8b6f4-x2k4q").unwrap();
    h.ext.cluster.pods.items = crate::model::pods(PODS).unwrap().items;
    fake.take();
    h.ext.shell(t.clone());
    h.settle();
    let calls = fake.take();
    let term = calls.iter().find(|c| c.starts_with("terminal ")).unwrap();
    let want = kubectl::exec(&t, odd, "log-shipper");
    assert!(term.ends_with(&format!(": kubectl {}", want.join(" "))), "{term}");
    assert_eq!(want.iter().filter(|a| a.starts_with("--context=")).count(), 1);
    assert_eq!(want[want.iter().position(|a| a == "--").unwrap() + 1..], ["sh", "-c", kubectl::SHELL_SCRIPT]);
}

#[test]
fn every_change_is_asked_naming_the_context_and_the_namespace() {
    let fake = cluster();
    fake.on_spawn("kubectl delete", Ran::ok("pod \"api-5f6d7c8b9-abcde\" deleted\u{202E}"))
        .on_spawn("kubectl rollout restart", Ran::ok("deployment.apps/web restarted"))
        .on_spawn("kubectl scale", Ran::ok("deployment.apps/web scaled"))
        .on_prompt("Scale deployment web", "5");
    let (mut h, page) = opened(&fake);
    keys(&mut h, page, "2jjj");
    fake.take();
    keys(&mut h, page, "x");
    assert_snapshot!("delete-asked", h.screen(page));
    keys(&mut h, page, "n");
    assert!(!fake.calls().iter().any(|c| c.contains("kubectl delete")), "a single key changes nothing");
    assert!(h.ext.asked.is_none(), "no is no: nothing left waiting");
    keys(&mut h, page, "xy");
    let lines = spawned(&fake);
    assert!(lines.iter().any(|l| l == "kubectl delete pod api-5f6d7c8b9-abcde --wait=false --context=kind-dev --namespace=shop --request-timeout=20s"), "{lines:#?}");
    assert!(fake.notifications().iter().any(|n| n == "delete pod api-5f6d7c8b9-abcde: pod \"api-5f6d7c8b9-abcde\" deleted"), "kubectl's words, their bidi control out: {:?}", fake.notifications());
    keys(&mut h, page, "1j");
    keys(&mut h, page, "R");
    assert!(h.screen(page).contains("Restart deployment web (rollout restart)? context kind-dev, namespace shop"));
    keys(&mut h, page, "y");
    assert!(spawned(&fake).iter().any(|l| l.starts_with("kubectl rollout restart deployment/web --context=kind-dev --namespace=shop")));
    keys(&mut h, page, "S");
    assert!(h.screen(page).contains("Scale deployment web from 3 to 5? context kind-dev, namespace shop"));
    keys(&mut h, page, "y");
    assert!(spawned(&fake).iter().any(|l| l.starts_with("kubectl scale deployment/web --replicas=5 --current-replicas=3 --context=kind-dev --namespace=shop")));
    keys(&mut h, page, "x");
    assert!(fake.notifications().last().unwrap().contains("Deleting isn't for a deployment"));
    // A question asked in one context isn't answered in another.
    fake.on_pick("Context, for this project", "fake");
    keys(&mut h, page, "gjR");
    let asked = h.ext.asked.clone().unwrap();
    h.command("kubernetes.context").unwrap();
    assert!(h.ext.asked.is_none() && h.ext.main.as_ref().unwrap().1.confirm.is_none(), "the question goes with the context");
    h.ext.confirmed(asked.0);
    assert!(fake.notifications().last().unwrap().contains("context changed"));
    // A pattern added while a plain question is open: y no longer does it.
    // (fake's server is on 127.0.0.1, but that doesn't make it local: it's
    // exempted by name first.)
    h.set_setting("kubernetes.unprotected", Some("fake"));
    keys(&mut h, page, "gjR");
    h.set_setting("kubernetes.protected", Some("fake"));
    keys(&mut h, page, "y");
    assert!(fake.notifications().last().unwrap().contains("fake is protected now: ask again"));
}

/// Every context is protected but the clearly local ones: a change there
/// needs the object's name typed, or is refused.
#[test]
fn a_protected_context_needs_the_name_typed_or_refuses() {
    let fake = cluster_on("gke_acme_europe-west1_prod");
    fake.on_spawn("kubectl delete", Ran::ok("pod deleted"));
    let (mut h, page) = opened(&fake);
    assert!(h.screen(page).contains("context gke_acme_europe-west1_prod protected (kubeconfig's current, read 12:00)   namespace payments"), "{}", h.screen(page));
    keys(&mut h, page, "2jjj");
    fake.on_prompt("PROTECTED", "api");
    fake.take();
    keys(&mut h, page, "x");
    let prompt = fake.prompts().pop().unwrap();
    assert_eq!(prompt.label, "PROTECTED context gke_acme_europe-west1_prod, namespace shop: type api-5f6d7c8b9-abcde to delete pod api-5f6d7c8b9-abcde");
    assert!(!fake.calls().iter().any(|c| c.contains("kubectl delete")));
    assert!(fake.notifications().last().unwrap().contains("That isn't api-5f6d7c8b9-abcde: nothing changed"));
    fake.on_prompt("PROTECTED", "api-5f6d7c8b9-abcde");
    keys(&mut h, page, "x");
    assert!(spawned(&fake).iter().any(|l| l.starts_with("kubectl delete pod api-5f6d7c8b9-abcde --wait=false --context=gke_acme_europe-west1_prod --namespace=shop")));
    h.set_setting("kubernetes.protected_mode", Some("refuse"));
    fake.take();
    keys(&mut h, page, "x");
    assert!(fake.notifications().last().unwrap().contains("gke_acme_europe-west1_prod is protected: delete pod api-5f6d7c8b9-abcde refused here"));
    assert!(spawned(&fake).is_empty());
    // Exempted by name: a plain question.
    h.set_setting("kubernetes.unprotected", Some("gke_*_prod"));
    keys(&mut h, page, "x");
    assert!(h.screen(page).contains("Delete pod api-5f6d7c8b9-abcde? context gke_acme_europe-west1_prod, namespace shop"));
}

/// A context whose API server is on 127.0.0.1 -- a tunnel or a
/// port-forward to production looks exactly so -- is protected with no
/// setting: only a local cluster's name, or kubernetes.unprotected, says
/// otherwise.
#[test]
fn a_server_on_localhost_alone_doesnt_make_a_context_local() {
    let fake = cluster_on("fake");
    let (mut h, page) = opened(&fake);
    assert!(h.screen(page).contains("context fake protected (kubeconfig's current"), "{}", h.screen(page));
    keys(&mut h, page, "2jjj");
    fake.take();
    keys(&mut h, page, "x");
    assert!(fake.prompts().pop().unwrap().label.starts_with("PROTECTED context fake, namespace shop: type api-5f6d7c8b9-abcde"));
    assert!(!h.screen(page).contains("y yes, do it"));
    h.set_setting("kubernetes.unprotected", Some("fake"));
    keys(&mut h, page, "x");
    assert!(h.screen(page).contains("Delete pod api-5f6d7c8b9-abcde? context fake, namespace shop"));
}

#[test]
fn a_secret_is_shown_without_its_values() {
    let fake = cluster();
    fake.on_spawn("kubectl get secret db-credentials", Ran::ok(SECRET));
    let (mut h, page) = opened(&fake);
    keys(&mut h, page, "4");
    keys(&mut h, page, "i");
    let p = fake.process("kubectl get secret").unwrap();
    assert_eq!(p.cmd.args[..5], ["get", "secret", "db-credentials", "-o", "json"]);
    assert!(fake.calls().iter().any(|c| c == "text secret-db-credentials.yaml (kind-dev/shop) (in a tab)"));
    let text = crate::secret_text(&Target::new(Kind::Secret, "shop", "db-credentials").unwrap(), SECRET).unwrap();
    assert_snapshot!("secret", text);
    for value in ["aHVudGVyMg", "hunter2", "YWRtaW4"] {
        assert!(!text.contains(value));
        assert!(!fake.calls().iter().any(|c| c.contains(value)), "{value} in the calls");
        assert!(!fake.logs().iter().any(|c| c.contains(value)));
    }
    assert!(!fake.spawned().iter().any(|(_, l)| l.contains("/secrets")), "listing never fetches their data");
}

fn answer(fake: &Fake) -> Result<Value, String> {
    let (_, a) = fake.state().answers.iter().next_back().map(|(k, v)| (*k, v.clone())).expect("an answer");
    a.map(|t| serde_json::from_str(&t).unwrap())
}

#[test]
fn agents_read_capped_data_in_the_chosen_namespace_only() {
    let fake = cluster();
    let mut h: Harness<Kube> = Harness::with_fake(fake.clone());
    h.activate().unwrap();
    assert_eq!(h.serve("kubernetes.pods", "{}"), None);
    let a = answer(&fake).unwrap();
    assert_eq!((a["context"].as_str(), a["namespace"].as_str()), (Some("kind-dev"), Some("shop")));
    assert_eq!(a["pods"][3]["status"], "CrashLoopBackOff");
    assert!(!a.to_string().contains("hunter2"), "a pod's environment never");
    // Workloads: one kubectl call (kubectl 1.37.2's columns), one answer.
    fake.take();
    h.serve("kubernetes.workloads", "{}");
    let lines = spawned(&fake);
    assert_eq!(lines.len(), 1, "{lines:#?}");
    let a = answer(&fake).unwrap();
    assert_eq!(a["deployments"][0]["status"], "2/3 ready");
    assert_eq!(a["cronjobs"][0]["status"], "0 * * * *");
    assert_eq!(a["jobs"][0]["status"], "Complete 1/1");
    // One pod: never its command or environment.
    fake.on_spawn("kubectl get --raw /api/v1/namespaces/shop/pods/api-5f6d7c8b9-abcde", Ran::ok(POD));
    h.serve("kubernetes.pod", r#"{"pod":"api-5f6d7c8b9-abcde"}"#);
    let a = answer(&fake).unwrap();
    assert_eq!(a["containers"][0]["last_state"]["exit_code"], 2);
    assert!(!a.to_string().contains("hunter2"), "{a}");
    // Events: the cluster's text as data, its escapes out.
    h.serve("kubernetes.events", r#"{"kind":"pod","name":"api-5f6d7c8b9-abcde"}"#);
    let a = answer(&fake).unwrap();
    assert_eq!(a["events"][0]["reason"], "BackOff");
    let msg = a["events"][1]["message"].as_str().unwrap();
    assert!(msg.contains("IGNORE PREVIOUS INSTRUCTIONS") && !msg.contains('\u{1b}'));
    // No other namespace, no context.
    assert!(h.serve("kubernetes.pods", r#"{"namespace":"kube-system"}"#).unwrap().unwrap_err().contains("leave namespace and context out"));
    // Logs and describe: off unless the user turns them on, and nothing runs.
    fake.take();
    assert!(h.serve("kubernetes.logs", r#"{"pod":"web-7d9c8b6f4-x2k4q"}"#).unwrap().unwrap_err().contains("kubernetes.agent_logs"));
    assert!(h.serve("kubernetes.describe", r#"{"kind":"pod","name":"api-5f6d7c8b9-abcde"}"#).unwrap().unwrap_err().contains("turned off"));
    assert!(spawned(&fake).is_empty());
    // Every process an agent caused only read.
    for (_, line) in fake.spawned() {
        let verb = line.split(' ').nth(1).unwrap();
        assert!(["version", "config", "get"].contains(&verb), "{line}");
    }
}

#[test]
fn with_logs_allowed_describe_is_json_without_env_commands_or_annotations() {
    let fake = cluster();
    fake.setting("kubernetes.agent_logs", "true");
    let long: String = (0..3000).map(|n| format!("2026-11-14T11:00:00Z line {n}\u{200B}\n")).collect();
    fake.on_spawn("kubectl logs", Ran::ok(long)).on_spawn("kubectl get --raw /api/v1/namespaces/shop/pods/api-5f6d7c8b9-abcde", Ran::ok(POD));
    let mut h: Harness<Kube> = Harness::with_fake(fake.clone());
    h.activate().unwrap();
    h.serve("kubernetes.logs", r#"{"pod":"web-7d9c8b6f4-x2k4q","lines":5000}"#);
    let a = answer(&fake).unwrap();
    assert_eq!(a["lines"], 2000);
    assert!(!a["log"].as_str().unwrap().contains('\u{200B}'));
    let logs = fake.process("kubectl logs").unwrap();
    assert!(logs.cmd.args.contains(&"--context=kind-dev".to_string()) && logs.cmd.args.contains(&"--namespace=shop".to_string()) && !logs.cmd.args.iter().any(|a| a.contains("follow")));
    h.serve("kubernetes.describe", r#"{"kind":"pod","name":"api-5f6d7c8b9-abcde"}"#);
    let p = fake.process("kubectl get --raw /api/v1/namespaces/shop/pods/api-5f6d7c8b9-abcde").unwrap();
    assert_eq!(p.cmd.args[..3], ["get", "--raw", "/api/v1/namespaces/shop/pods/api-5f6d7c8b9-abcde"], "the object's JSON, never describe's text");
    let a = answer(&fake).unwrap();
    let text = a.to_string();
    assert!(!text.contains("hunter2"), "{text}");
    assert!(text.contains("DB_PASSWORD") && a["describe"]["spec"]["containers"][0]["command"] == "<hidden>");
}

#[test]
fn agents_are_rate_limited_per_tool_and_overall() {
    let fake = cluster();
    fake.on_spawn_with(|cmd| (cmd.args.first().map(String::as_str) == Some("get")).then(|| Ok(Ran::running(""))));
    let mut h: Harness<Kube> = Harness::with_fake(fake.clone());
    h.activate().unwrap();
    assert_eq!(h.serve("kubernetes.pods", "{}"), None);
    assert!(h.serve("kubernetes.pods", "{}").unwrap().unwrap_err().starts_with("busy: a call to this tool is still running"));
    assert_eq!(h.serve("kubernetes.workloads", "{}"), None);
    assert_eq!(h.serve("kubernetes.events", "{}"), None);
    assert!(h.serve("kubernetes.pod", r#"{"pod":"x"}"#).unwrap().unwrap_err().starts_with("busy: 3 calls are running"));
    // As they end, more are taken -- up to the minute's 30.
    let running: Vec<u64> = fake.spawned().iter().filter(|(_, l)| l.contains("kubectl get")).map(|(p, _)| *p).collect();
    for p in running {
        h.exited(p, Some(1));
    }
    let mut taken = 3;
    for _ in 0..40 {
        match h.serve("kubernetes.pods", "{}") {
            None => {
                taken += 1;
                let p = fake.spawned().last().unwrap().0;
                h.exited(p, Some(1));
            }
            Some(Err(why)) => {
                assert!(why.starts_with("too many calls"), "{why}");
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(taken, crate::AGENT_PER_MINUTE);
}

/// No call without `--context`: with no current context in kubeconfig,
/// or kubeconfig unreadable, nothing reaches a cluster.
#[test]
fn without_a_context_nothing_runs() {
    for (config, said) in [(config(""), "No context: kubeconfig has no current context"), ("".to_string(), "kubectl couldn't read kubeconfig")] {
        let fake = cluster();
        if config.is_empty() {
            fake.on_spawn("kubectl config view", Ran::fails(1, "error: error loading config file \"/home/me/.kube/config\": yaml: line 3: mapping values are not allowed in this context"));
        } else {
            fake.on_spawn("kubectl config view", Ran::ok(config));
        }
        let (mut h, page) = opened(&fake);
        let screen = h.screen(page);
        assert!(screen.contains(said), "{screen}");
        assert!(screen.contains("context none"), "{screen}");
        // Every action refuses; only kubectl's version and kubeconfig ran.
        h.ext.cluster.pods.items = crate::model::pods(PODS).unwrap().items;
        let t = Target::new(Kind::Pod, "shop", "db-0").unwrap();
        for action in [crate::page::Action::Describe(t.clone()), crate::page::Action::Yaml(t.clone()), crate::page::Action::Logs(t.clone()), crate::page::Action::Shell(t.clone()), crate::page::Action::Forward(t.clone()), crate::page::Action::PickNamespace, crate::page::Action::Mutate(crate::page::Mutation::Delete(t.clone()))] {
            h.ext.act(page, action);
        }
        h.settle();
        assert!(h.serve("kubernetes.pods", "{}").unwrap().unwrap_err().starts_with("no context"));
        keys(&mut h, page, "r");
        for (_, line) in fake.spawned() {
            assert!(line.ends_with("version --client -o json") || line.ends_with("config view -o json"), "ran without a context: {line}");
        }
        assert!(fake.notifications().iter().filter(|n| n.starts_with("No context to")).count() >= 5, "{:?}", fake.notifications());
    }
}

/// kubeconfig read again (r) with another current context: what was
/// listed goes, the question with it, and a late answer for the old
/// cluster is dropped.
#[test]
fn kubeconfig_changing_under_the_page_resets_it() {
    let fake = cluster();
    let (mut h, page) = opened(&fake);
    keys(&mut h, page, "2jjjx");
    assert!(h.ext.asked.is_some());
    // The next reads of kubeconfig are slow; the lists after are too.
    fake.on_spawn("kubectl config view", Ran::running(""));
    fake.on_spawn_with(|cmd| (cmd.args.first().map(String::as_str) == Some("get")).then(|| Ok(Ran::running(""))));
    keys(&mut h, page, "r");
    keys(&mut h, page, "r");
    let reads: Vec<u64> = fake.spawned().iter().filter(|(_, l)| l.ends_with("config view -o json")).map(|(p, _)| *p).collect();
    let (older, newer) = (reads[reads.len() - 2], reads[reads.len() - 1]);
    let old_list = pid(&fake, "kubectl get --raw /api/v1/namespaces/shop/pods?limit=500");
    // The newer read says the current context is now "fake"; the older
    // one, ending after it, still says kind-dev: it's dropped.
    finish(&mut h, newer, &config("fake"), 0);
    finish(&mut h, older, &config("kind-dev"), 0);
    assert_eq!(h.ext.context().as_deref(), Some("fake"));
    assert!(h.ext.asked.is_none(), "the question about kind-dev's pod went");
    // kind-dev's late list isn't shown under fake's header.
    finish(&mut h, old_list, PODS, 0);
    keys(&mut h, page, "2");
    let screen = h.screen(page);
    assert!(screen.contains("context fake") && !screen.contains("web-7d9c8b6f4-x2k4q"), "{screen}");
    // Another kubectl: the same.
    h.ext.cluster.pods.items = crate::model::pods(PODS).unwrap().items;
    h.set_setting("kubernetes.kubectl", Some("/opt/k8s/kubectl"));
    let screen = h.screen(page);
    assert!(screen.contains("Reading kubeconfig") && !screen.contains("web-7d9c8b6f4-x2k4q"), "{screen}");
    assert_eq!(fake.spawned().last().unwrap().1, "/opt/k8s/kubectl config view -o json");
}

/// Each flow that waits on kubectl takes only the answer to what it's
/// waiting for now.
#[test]
fn late_answers_are_never_taken_for_newer_ones() {
    let fake = cluster();
    fake.on_spawn("kubectl version -o json", Ran::running("")).on_spawn("kubectl get deployment web", Ran::running("")).on_pick("Context, for this project", "fake").on_pick("Namespace", "payments");
    fake.on_spawn_with(|cmd| (cmd.args.get(2).map(String::as_str) == Some("/api/v1/namespaces?limit=1000")).then(|| Ok(Ran::running(""))));
    let (mut h, page) = opened(&fake);
    let old_version = pid(&fake, "kubectl version -o json");
    keys(&mut h, page, "j");
    keys(&mut h, page, "i");
    let old_yaml = pid(&fake, "kubectl get deployment web");
    keys(&mut h, page, "n");
    let old_namespaces = pid(&fake, "kubectl get --raw /api/v1/namespaces?limit=1000");
    // The context changes while all three are out.
    keys(&mut h, page, "c");
    // kind-dev's server answers late: not fake's.
    finish(&mut h, old_version, VERSION, 0);
    assert!(!h.screen(page).contains("server v1.30.4"), "{}", h.screen(page));
    // kind-dev's YAML: not opened as the page's now.
    finish(&mut h, old_yaml, YAML, 0);
    assert!(!fake.calls().iter().any(|c| c.starts_with("text deployment-web.yaml")));
    assert!(fake.notifications().last().unwrap().contains("came after the context changed"));
    // kind-dev's namespaces: no picker for them.
    let pickers = fake.pickers().len();
    finish(&mut h, old_namespaces, NAMESPACES, 0);
    assert_eq!(fake.pickers().len(), pickers);
}

/// A call kubectl never ends -- a login plugin waiting on a browser -- is
/// stopped by the watchdog, said, and `r` asks again.
#[test]
fn a_hung_call_is_stopped_and_r_always_works() {
    let fake = cluster();
    fake.on_spawn_with(|cmd| (cmd.args.first().map(String::as_str) == Some("get")).then(|| Ok(Ran::running(""))));
    fake.on_spawn("kubectl version -o json", Ran::running(""));
    let (mut h, page) = opened(&fake);
    let hung = pid(&fake, "kubectl get --raw /api/v1/namespaces/shop/pods?limit=500");
    // r while they hang: given up, asked again.
    fake.take();
    keys(&mut h, page, "r");
    assert!(fake.state().processes[&hung].killed);
    assert!(spawned(&fake).iter().any(|l| l.starts_with("kubectl get --raw /api/v1/namespaces/shop/pods")));
    // The watchdog: three beats, then stopped and said.
    let watchdog = fake.timers().iter().find(|t| t.millis == crate::WATCH_MS).unwrap().id;
    for _ in 0..crate::WATCH_BEATS {
        h.fire(watchdog);
    }
    let screen = h.screen(page);
    assert!(screen.contains("kubectl gave the cluster's version no answer in 30 s -- a login plugin may be waiting for you"), "{screen}");
    assert!(fake.spawned().iter().filter(|(_, l)| l.contains("kubectl get --raw")).all(|(p, _)| fake.state().processes[p].killed));
    assert!(fake.calls().iter().any(|c| c == &format!("cancel-timer {watchdog}")), "no calls left: the watchdog stops");
    fake.take();
    keys(&mut h, page, "r");
    assert_eq!(spawned(&fake).len(), 8, "kubeconfig, the server's version and the tab's six lists");
}

#[test]
fn an_expired_login_is_shown_and_not_retried() {
    let fake = cluster();
    fake.on_spawn("kubectl version -o json", Ran::fails(1, UNAUTHORIZED));
    fake.on_spawn_with(move |cmd| (cmd.args.first().map(String::as_str) == Some("get")).then(|| Ok(Ran::fails(1, UNAUTHORIZED))));
    let (mut h, page) = opened(&fake);
    assert_snapshot!("expired-login", h.screen(page));
    assert!(fake.checks().pop().unwrap().detail.contains("You must be logged in to the server (Unauthorized)"));
    fake.take();
    for _ in 0..5 {
        h.fire_timers();
    }
    assert!(spawned(&fake).is_empty(), "no retry storm");
    keys(&mut h, page, "r");
    assert_eq!(spawned(&fake).len(), 8, "kubeconfig, the server's version and the tab's six lists, once");
}

#[test]
fn without_kubectl_it_says_so() {
    let fake = cluster();
    fake.on_spawn_error("kubectl", "kubectl: No such file or directory (os error 2)");
    let (mut h, page) = opened(&fake);
    assert!(h.screen(page).contains("kubectl can't run: kubectl: No such file or directory (os error 2)"), "{}", h.screen(page));
    assert!(fake.checks().pop().unwrap().detail.contains("can't run"));
    assert_eq!(h.serve("kubernetes.pods", "{}").unwrap().unwrap_err(), "kubectl can't run here: kubectl: No such file or directory (os error 2)");
}

/// A call given up on: its process stopped, and its late answer goes
/// nowhere -- not to the next call of the same tool.
#[test]
fn an_agent_call_given_up_on_answers_nothing_later() {
    let fake = cluster();
    fake.on_spawn_with(|cmd| (cmd.args.first().map(String::as_str) == Some("get")).then(|| Ok(Ran::running(""))));
    let mut h: Harness<Kube> = Harness::with_fake(fake.clone());
    h.activate().unwrap();
    assert_eq!(h.serve("kubernetes.pods", "{}"), None);
    let first = fake.spawned().last().unwrap().0;
    let call = *h.ext.agent_calls.keys().next().unwrap();
    h.cancelled(call);
    assert!(fake.state().processes[&first].killed);
    assert_eq!(h.serve("kubernetes.pods", "{}"), None, "no longer busy");
    let second = fake.spawned().last().unwrap().0;
    // The first's answer, late: dropped.
    finish(&mut h, first, PODS, 0);
    assert!(fake.state().answers.is_empty(), "{:?}", fake.state().answers);
    finish(&mut h, second, PODS, 0);
    assert_eq!(fake.state().answers.len(), 1);
    assert!(!fake.state().answers.contains_key(&call));
}
