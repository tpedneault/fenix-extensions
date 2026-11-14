# Kubernetes

Your clusters through `kubectl`, in a pane: the workloads and their pods,
services and ingresses, Secrets by name, live logs, a shell in a pod,
port-forwards that keep running, an object's YAML and `describe` in a
buffer -- and the changes you make, each one asked about first.

`SPC o k k` opens it (with standard keys, `Ctrl+K o k k`). It drives
`kubectl`, which has to be installed: the extension never talks to the
API server itself, never reads kubeconfig's credentials, and **never
changes kubeconfig** -- kubectl does the talking, as it does in your
terminal, with your kubeconfig, your `KUBECONFIG` and your login plugins.

The tutorial (`SPC h T`) has a lesson on it, under **Kubernetes**: the
page, choosing a context, a pod's log, and how a change is asked. Without
a cluster, `s` on the tutorial page skips the steps that need one.

## Contexts and namespaces

The page's top line says where you are: the context, where it comes from,
whether it's protected (below), and the namespace.

**Every kubectl call names its context** (`--context=<it>`) and
namespace. Until you choose, the context is kubeconfig's current one as
read when the page last asked (`kubeconfig's current, read 12:04`; `r`
reads kubeconfig again). With no current context -- or a kubeconfig
kubectl can't read -- **nothing runs**: no list, log, shell, forward,
describe or agent tool; the page says so and `c` chooses one. If
kubeconfig, or `kubernetes.kubectl`, changes which cluster that is,
everything listed goes and any question with it, so nothing of the old
cluster is shown under the new name.

**Choosing a context here changes nothing anywhere else.** `c` (or
`Kubernetes: choose the context`) picks one of kubeconfig's contexts for
this project; kubeconfig isn't written, so your terminal, your other
projects and every other tool stay where they were. The choice is kept
with the page's state for the project's session. Names are read from
`kubectl config view -o json` and used exactly as written: `"prod "` and
`"prod"` are two contexts. One whose name holds a tab or a newline is left
out, and said.

There's no command to switch kubeconfig's own current context: `kubectl
config use-context` in a terminal does that.

**Suggested contexts.** `kubernetes.context` and `kubernetes.namespace`
suggest a context and namespace for a project, and a project may set them
in its `.fenix/settings.toml`. Fenix doesn't tell an extension whether a
value is yours or the project's, nor whether the project is trusted, so a
suggestion is only shown -- *u uses the suggested context …* -- and
nothing uses it, agents included, until you press `u`. A repository you
clone can't point the page, or your agents, at one of your clusters.

`n` picks a namespace, or **All namespaces**; if your account may not
list namespaces, it asks you to type one.

## The page

| Key | What it does |
| --- | --- |
| `1`-`5`, `]`, `[` | Workloads, Pods, Network, Secrets, Forwards |
| `j` `k` `g` `G`, `J` `K` | Move a row, to the top or the end, a screen |
| `Enter` | Open or close a group or a workload (its pods under it); on an object, its menu |
| `m` | The object's menu: everything below that applies to it |
| `l` | Its log (a pod's, or what kubectl picks of a workload's pods) |
| `!` | A shell in the pod: `kubectl exec -it`, in a terminal; which container, when it has several |
| `f` | A port-forward to the pod or the service (`local:remote`) |
| `i` | Its YAML, read-only, in a buffer |
| `D` | `kubectl describe`, in a buffer |
| `y` | Copy its name |
| `x` | Delete the pod -- asked first |
| `R` | Restart a deployment, statefulset or daemonset (`rollout restart`) -- asked first |
| `S` | Scale a deployment or statefulset -- how many, then asked |
| `c`, `n`, `u` | The context, the namespace; take the suggested ones |
| `/` | Filter by name |
| `+` | List more of a kind cut at its cap |
| `r` | Ask again: kubeconfig, whether the cluster answers, the tab |

On the Forwards tab: `s` stops the one under the cursor, `o` opens
`http://localhost:<port>/`, `Enter` shows its output.

The keys are Containers' where they do the same thing: `l` a log, `!` a
shell, `m` a menu, `R` restart, `x` remove (here `x`, then `y` to the
question, where Containers takes `x x`), `o` open a port, `y` copy,
`1`-`5` the tabs, `r` reload, `/` filter. `q` and `Esc` close the page,
as on any page.

**Refreshing**: while the page is in view, the tab in view is asked for
again every `kubernetes.refresh_seconds` (10); when it's hidden or
closed, nothing is asked. A failure -- an expired login, a cluster that
doesn't answer -- is shown with kubectl's own words (and a hint when they
make one plain), and **nothing is asked again until you press `r`**: no
retry storm, no login plugin started every ten seconds.

**A watchdog** stops any kubectl call that isn't a stream after 20 to 30
seconds without an answer -- most often a login plugin waiting for you
in a browser or on a device code, which kubectl's own
`--request-timeout` doesn't bound -- and says what was waiting. `r`
always works: it gives up whatever is still being waited for and asks
again.

## Logs

`l` opens a log page, following the pod's log as it's written (the last
200 lines, then what comes; the last 5,000 kept, each cut at 16 KiB). On
it:

| Key | What it does |
| --- | --- |
| `s` | Stop the stream; again, start it from where it stopped (`--since-time`) |
| `p` | The previous run's log, before the last restart (read once); again, back |
| `S` | Since a while ago: `10m`, `2h` (empty: the last 200 lines) |
| `c`, `a` | Another container; all of them, each line saying which |
| `t` | Times |
| `/` | Filter |
| `o` | Open what's shown in a buffer |
| `j` `k` `g` `G` | Scroll; `G` (or `F`) follows the end again |

Changing how it's read starts a new stream; the old one is stopped and
whatever it still says goes nowhere. Closing the page stops kubectl. A
stream that ends by itself (the pod went) says why and isn't started
again.

## Changes

Nothing that changes the cluster is one key, and nothing happens without
a question that names **the context and the namespace**: `x` on a pod
shows *Delete pod web-1? context kind-dev, namespace shop*, and only
`y` does it -- any other key is no. `S` asks how many replicas, then asks;
the scale goes with `--current-replicas`, so if someone changed it in
between, it fails rather than overwrites. A question goes when the
context changes, and `y` is refused if the context became protected
since it was asked.

**Every context is protected by default, except clearly local ones.** A
change in a protected context needs the object's name **typed** in place
of `y` (`kubernetes.protected_mode = "type-name"`), or isn't made at all
(`"refuse"`; kubectl in a terminal still can). A context is unprotected
only when its name, or its cluster's name in kubeconfig, is a local
cluster's -- `kind-*`, `minikube`, `docker-desktop`, `rancher-desktop`,
`k3d-*`, `colima`, `orbstack` -- or its cluster's server is this machine
(`localhost`, `127.x.x.x`, `::1`). Why: production contexts often say
nothing about it -- EKS names them by ARN, GKE as
`gke_<project>_<zone>_<cluster>` -- so a pattern like `*prod*` misses
them, and the costs aren't even: typing a name on a cluster that didn't
need it costs seconds, a stray `y` on production costs an outage.

- `kubernetes.unprotected` exempts your own dev clusters by name
  (context or cluster: `dev-*`).
- `kubernetes.protected` protects by name whatever else matches -- a
  tunnel to production on `localhost`, for one, which would otherwise
  look local.

A protected context is red on the page and says `protected`; the context
picker says which are.

## Secrets

The Secrets tab lists them from kubectl's own table -- name, type, how
many keys, age -- so their data is never fetched to be listed. `i` shows
one's YAML with **its keys and not their values**: each value under
`data` and `stringData`, and **every annotation's and label's value**
(any of them can copy the data, as `last-applied-configuration` does),
replaced by its length. There's no reveal key: `kubectl get secret` in a
terminal is where you see a value, deliberately. No agent tool reads
Secrets, and nothing of theirs is written to the extension's log.

## Large clusters

A list is capped **by the API server**: the page asks `kubectl get --raw
/api/v1/namespaces/<ns>/pods?limit=500`, the server stops at 500 and says
how many more there are, and the page says *… 4500 more pods not listed
(the first 500): + lists more*. `+` raises that kind's cap by
`kubernetes.max_items`, up to 5,000; a namespace (`n`) or a filter is the
better way into a big cluster. Secrets, listed as kubectl's table, are
cut at the cap as they come.

Parsing pods as the API returns them, natively (`cargo test --release --
--ignored measure`): 500 pods (0.9 MB) in about 2 ms, 5,000 (9 MB) in
about 25 ms; pods with fuller specs, 15 MB for 5,000, took about 33 ms.
An unoptimised build takes several times as long (one review measured
205 ms for 5,000). In Fenix's WebAssembly it's slower
again, and an extension has 128 MiB and should answer each event in a
few milliseconds, so 500 is the default and 5,000 the most.

## For coding agents

Six tools in the `kubernetes` toolset, all class `read`. **They work in
the context and the one namespace you chose on the page**: an agent can
name neither, and with **All namespaces** chosen, or no context, they
refuse. None deletes, scales, restarts, execs or forwards; none reads a
Secret; none ever returns an environment value, a command or its
arguments.

| Tool | What it answers |
| --- | --- |
| `kubernetes_workloads` | Deployments, statefulsets, daemonsets, jobs and cronjobs: readiness or state, replicas, age; at most 500 of each, from one kubectl call cut at 2,500 objects |
| `kubernetes_pods` | Pods: status as kubectl shows it, ready containers, restarts, age, node; at most 500 |
| `kubernetes_pod` | One pod: phase, conditions, owners, and each container's image, readiness, restarts, state and last state |
| `kubernetes_events` | The namespace's events, or one object's, newest first, at most 200, each message at most 1 KiB |
| `kubernetes_logs` | **Off unless `kubernetes.agent_logs` is on.** A pod's last lines (200, up to 2000; one container or all; or the previous run's), at most 64 KiB, a line at most 2 KiB; never followed |
| `kubernetes_describe` | **Off unless `kubernetes.agent_logs` is on.** One object in the namespace -- pod, deployment, statefulset, daemonset, replicaset, job, cronjob, service, ingress, PVC, HPA; never a Secret or ConfigMap -- as the API has it (not `kubectl describe`'s text), with every `env` value, every `command` and `args` (probes and hooks too), annotations' values and managed fields taken out; at most 64 KiB |

- **Logs and descriptions are off by default** because a log can hold
  whatever a program printed -- tokens, passwords -- and both are written
  by whatever runs in the cluster. Fenix offers an extension's tools from
  its manifest and can't hide one behind a setting, so with the setting
  off the two are listed but answer that they're turned off, and run
  nothing.
- **What comes back from the cluster** -- a log, an event's message, a
  reason -- is returned as data in a field of the answer, with a note
  saying so, its terminal escapes, control characters and bidi and
  zero-width characters taken out.
- **Rate limits**: one call at a time per tool, three at once in all,
  thirty a minute; past them an agent is told it's busy.
- Names an agent gives are held to what Kubernetes allows a name to be,
  so one can't become an option, a path or a second argument.
- Agents' tools can start kubectl -- and so a login plugin -- without the
  page ever opened. To be asked first, give `agents.mcp.allow` a rule
  (`"toolset:kubernetes" = "ask"`).

## Settings

| Setting | Default | |
| --- | --- | --- |
| `kubernetes.kubectl` | | Where kubectl is, when it isn't on the PATH |
| `kubernetes.context` | | A context to suggest (a project may set it); used once you press `u` |
| `kubernetes.namespace` | | A namespace to suggest, `*` for all |
| `kubernetes.protected` | | Contexts or clusters always protected, even if they look local |
| `kubernetes.unprotected` | | Contexts or clusters where a change needs only `y` |
| `kubernetes.protected_mode` | `type-name` | `type-name`, or `refuse` |
| `kubernetes.agent_logs` | `false` | Whether agents may read logs and descriptions |
| `kubernetes.refresh_seconds` | 10 | Between refreshes while the page is in view; 0: only `r` |
| `kubernetes.max_items` | 500 | How many of each kind are listed at first (50 to 5,000) |

## Keys

| Key | Command | What it does |
| --- | --- | --- |
| `SPC o k k` | `kubernetes.open` | The page |
| `SPC o k c` | `kubernetes.context` | The context, for this project |
| `SPC o k n` | `kubernetes.namespace` | The namespace |
| `SPC o k f` | `kubernetes.forwards` | Stop a port-forward |

With standard keys, `SPC` is `Ctrl+K`.

## What it may do

- **Run `kubectl`** (`process`), or the one `kubernetes.kubectl` names:
  to list, read logs and YAML, describe, open a shell and port-forward
  as you ask on its pages, and to make the changes you confirm; for
  agents, only the reads above. kubectl talks to your cluster with your
  kubeconfig; the extension sends nothing anywhere itself.

Every argument goes to kubectl as an argument of its own -- no shell
reads any of them -- and a context, namespace or container is given as
`--flag=value`, so a context called `x; rm -rf ~` or `--as=admin` is
still just a context's name; path segments of `get --raw` are
percent-encoded. Port-forwards listen on 127.0.0.1 only. Fenix runs at
most four background tasks of an extension's at once, so at most four
port-forwards.

It reads no files, writes none, and keeps nothing between sessions but
the page's context, namespace and tab.

## Limits

- **No editing and applying YAML.** YAML buffers are read-only views.
  Applying an edited buffer back was left out of 1.0.0: `kubectl apply`
  of `get -o yaml`'s text writes the whole live object into
  `last-applied-configuration`, which a later `apply` from git then prunes
  against. Bringing it back needs `kubectl replace` (a strict
  `resourceVersion` conflict, no annotation rewrite) or server-side apply
  with its own field manager and the server's fields stripped, one
  checked generation from the dry run to the apply, the API group checked
  as well as kind and name -- and a run against a real cluster first.
- **No reveal for a Secret's values**, by design.
- On Windows, `!` needs `sh` in the container; distroless images and
  Windows containers have none, and kubectl says so.

## Not tried against a cluster yet

Its tests run on the test kit's fake host. kubectl's own output -- `config
view -o json`, the versions, the one-call workload columns, its error
texts -- was recorded from kubectl 1.37.2 against a fake API server; the
lists, a pod and a Secret are hand-written from the API's documented
objects (`tests/fixtures/README.md` says which is which). What only a real
cluster shows -- `remainingItemCount` on your server, the Secrets table
as a server prints it, `kubectl exec` in Fenix's terminal, a login plugin
that wants a browser -- hasn't been seen. There's no screenshot yet for
the same reason.

## Building

```text
cargo xtask pack extensions/kubernetes dist
cargo test --manifest-path extensions/kubernetes/Cargo.toml
```

It needs Rust's `wasm32-wasip2` target. The tests need no cluster. Most
fakes answer at once; those for what waits on a process (kubeconfig, the
server's version, lists, namespaces, YAML, logs, agents' calls) answer
late and out of order, to show that an answer to an earlier request is
never taken for a newer one's.
