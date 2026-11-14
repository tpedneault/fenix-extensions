# Kubernetes

Your clusters through `kubectl`, in a pane: the workloads and their pods,
services and ingresses, Secrets by name, live logs, a shell in a pod,
port-forwards that keep running, an object's YAML and `describe` in a
buffer -- and the changes you make, each one asked about first.

`SPC o k k` opens it (with standard keys, `Ctrl+K o k k`). It drives
`kubectl`, which has to be installed: the extension never talks to the
API server itself and never reads kubeconfig's credentials -- kubectl
does both, as it does in your terminal, with your kubeconfig, your
`KUBECONFIG` and your login plugins.

The tutorial (`SPC h T`) has a lesson on it, under **Kubernetes**: the
page, choosing a context, a pod's log, and how a change is asked. Without
a cluster, `s` on the tutorial page skips the steps that need one.

## Contexts and namespaces

The page's top line says where you are: the context, the namespace, and
whether that context is protected (below).

**Choosing a context here changes nothing anywhere else.** `c` (or
`Kubernetes: choose the context`) picks one of kubeconfig's contexts for
this project: from then on every kubectl the extension runs says
`--context=<it>` and `--namespace=<it>`. kubeconfig isn't written, so
your terminal, your other projects and every other tool stay where they
were. It's kept with the page's state for the project's session, and
`kubernetes.context` and `kubernetes.namespace` -- settings a project may
set in its `.fenix/settings.toml` -- pin them for a project for good.
Until you choose, it's kubeconfig's current context, said as
`(kubeconfig's)`, and named explicitly on every call all the same, so a
change asked about in one context can't land in another if kubeconfig
changes under it.

**Switching kubeconfig itself** -- `kubectl config use-context`, which
every tool that reads kubeconfig follows -- is a command of its own:
`Kubernetes: switch kubeconfig's current context, for every tool`
(`SPC o k u`). It says so when it's done.

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
| `e` | Its YAML in a buffer to edit and apply (below) |
| `D` | `kubectl describe`, in a buffer |
| `y` | Copy its name |
| `x` | Delete the pod -- asked first |
| `R` | Restart a deployment, statefulset or daemonset (`rollout restart`) -- asked first |
| `S` | Scale a deployment or statefulset -- how many, then asked |
| `c`, `n` | The context, the namespace |
| `/` | Filter by name |
| `+` | List more of a kind cut at its cap |
| `r` | Ask again (and whether the cluster answers) |

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
retry storm, no login plugin started every ten seconds. Every kubectl
call that reads or changes something gives up after 20 seconds (a log,
a shell and a port-forward run until stopped).

## Logs

`l` opens a log page, following the pod's log as it's written (the last
200 lines, then what comes; the last 5,000 kept). On it:

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

Closing the page stops kubectl. A stream that ends by itself (the pod
went) says why and isn't started again.

## Changes

Nothing that changes the cluster is one key, and nothing happens without
a question that names **the context and the namespace**: `x` on a pod
shows *Delete pod web-1? context kind-dev, namespace shop*, and only
`y` does it -- any other key is no. `S` asks how many replicas, then asks;
the scale goes with `--current-replicas`, so if someone changed it in
between, it fails rather than overwrites.

**Protected contexts.** `kubernetes.protected` names contexts where a
change needs more: the object's name **typed**, in place of `y`
(`kubernetes.protected_mode = "type-name"`), or no change at all
(`"refuse"`; kubectl in a terminal still can). Its default is `*prod*`
-- any context with `prod` in its name, `PROD` or `production` too. It
isn't empty because the costs aren't even: a context protected by
mistake costs typing a name; a production context left unprotected
costs an outage from a stray `y`. Empty it to protect none, or name your
own (`eks-*-live, *@main`). A protected context is red on the page and
says `protected`.

### Applying an edited YAML

`e` opens the object's YAML (as `kubectl get -o yaml` gives it) in a
buffer to edit. `Kubernetes: apply the YAML buffer` (`SPC o k a`) then:

1. reads the buffer once -- that text is what's checked and what's
   applied; an edit after needs `r` on the apply page;
2. runs `kubectl apply --dry-run=server`: the API server checks it and
   says what it would make. It must be the one object the buffer was
   opened from, its kind, name and namespace -- not another, not several;
3. runs `kubectl diff` and shows it;
4. `a` asks (naming the context and the namespace, or wanting the name
   typed in a protected context), and `y` applies.

The YAML keeps the `resourceVersion` it was read with, so if the object
changed since, the API server refuses the apply instead of losing the
other change. A buffer opened in one context isn't applied in another.
`kubectl diff` runs `diff` itself (or `KUBECTL_EXTERNAL_DIFF`): on a
machine without one, it says so and nothing is applied.

## Secrets

The Secrets tab lists them from kubectl's own table -- name, type, how
many keys, age -- so their data is never fetched to be listed. `i` shows
one's YAML with **its keys and not their values**: each value is replaced
by its length, and the `last-applied-configuration` annotation, which
copies them, is hidden too. There's no reveal key: `kubectl get secret`
in a terminal is where you see a value, deliberately. A Secret's YAML is
never offered for editing, no agent tool reads Secrets, and nothing of
theirs is written to the extension's log.

## Large clusters

A list is capped **by the API server**: the page asks `kubectl get --raw
/api/v1/namespaces/<ns>/pods?limit=500`, the server stops at 500 and says
how many more there are, and the page says *… 4500 more pods not listed
(the first 500): + lists more*. `+` raises that kind's cap by
`kubernetes.max_items`, up to 5,000; a namespace (`n`) or a filter is the
better way into a big cluster. Secrets, listed as kubectl's table, are
cut at the cap as they come.

Measured natively (`cargo test --release -- --ignored measure`, on the
machine this was written on), reading pods with their status, specs and
managed fields as the API returns them: 500 pods, 1.5 MB, in 3 ms; 2,000,
6 MB, in 13-17 ms; 5,000, 15 MB, in 32-33 ms. In Fenix's WebAssembly
it's slower (two to three times is usual) and an extension has 128 MiB
and should answer each event in a few milliseconds, so 500 is the
default and 5,000 the most.

## For coding agents

Six tools in the `kubernetes` toolset, all class `read`, in the context
and namespace you chose (an agent may name another namespace, never
another context). Each runs one kubectl read; none deletes, scales,
restarts, applies, execs or forwards, and none reads a Secret.

| Tool | What it answers |
| --- | --- |
| `kubernetes_workloads` | Deployments, statefulsets, daemonsets, jobs and cronjobs: readiness or state, replicas, age; at most 500 of each |
| `kubernetes_pods` | Pods: status as kubectl shows it, ready containers, restarts, age, node; at most 500 |
| `kubernetes_pod` | One pod: phase, conditions, owners, and each container's image, readiness, restarts, state and last state -- never its environment, command or arguments |
| `kubernetes_events` | A namespace's events, or one object's, newest first, at most 200, each message at most 1 KiB |
| `kubernetes_logs` | A pod's last lines (200, up to 2000; one container or all; or the previous run's), at most 64 KiB, a line at most 2 KiB; never followed |
| `kubernetes_describe` | `kubectl describe` of a pod, workload, service, ingress, node, namespace, PVC or HPA -- never a Secret or ConfigMap -- at most 64 KiB |

Each says when it was cut. **What comes back was written by whatever
runs in the cluster** -- a log, an event's message, `describe` -- so it's
returned as data in a field of the answer, with a note saying so, its
terminal escapes and control characters taken out. A log can hold
secrets a program printed: give `agents.mcp.allow` a rule
(`kubernetes_logs = "ask"`, or `"toolset:kubernetes" = "ask"`) to be
asked first. Names an agent gives are held to what Kubernetes allows a
name to be, so one can't become an option, a path or a second argument.

## Settings

| Setting | Default | |
| --- | --- | --- |
| `kubernetes.kubectl` | | Where kubectl is, when it isn't on the PATH |
| `kubernetes.context` | | The project's context (a project may set it); empty: kubeconfig's current |
| `kubernetes.namespace` | | The project's namespace, `*` for all; empty: the context's own, else `default` |
| `kubernetes.protected` | `*prod*` | Contexts where a change needs the name typed, or is refused |
| `kubernetes.protected_mode` | `type-name` | `type-name`, or `refuse` |
| `kubernetes.refresh_seconds` | 10 | Between refreshes while the page is in view; 0: only `r` |
| `kubernetes.max_items` | 500 | How many of each kind are listed at first (50 to 5,000) |

## Keys

| Key | Command | What it does |
| --- | --- | --- |
| `SPC o k k` | `kubernetes.open` | The page |
| `SPC o k c` | `kubernetes.context` | The context, for this project |
| `SPC o k n` | `kubernetes.namespace` | The namespace |
| `SPC o k a` | `kubernetes.apply` | Apply the YAML buffer (a dry run and a diff first) |
| `SPC o k f` | `kubernetes.forwards` | Stop a port-forward |
| `SPC o k u` | `kubernetes.use_context` | kubeconfig's current context, for every tool |

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
still just a context's name. Port-forwards listen on 127.0.0.1 only.
Fenix runs at most four background tasks of an extension's at once, so
at most four port-forwards.

It reads no files, writes none, and keeps nothing between sessions but
the page's context, namespace and tab.

## Not tried against a cluster yet

It was written where there was no cluster and no kubectl. Its tests run
on kubectl's output written by hand from the API's documented objects
(`tests/fixtures/`, which says so): the command lines it builds, what it
reads back, its pages and keys. What only a real kubectl and cluster
show -- kubectl's exact error texts, `--raw`'s `remainingItemCount` on
your server, how `kubectl exec` behaves in Fenix's terminal, a login
plugin that wants a browser -- hasn't been seen. There's no screenshot
yet for the same reason.

## Building

```text
cargo xtask pack extensions/kubernetes dist
cargo test --manifest-path extensions/kubernetes/Cargo.toml
```

It needs Rust's `wasm32-wasip2` target. The tests need no cluster: they
drive the extension on the test kit's fake host with kubectl's answers
from `tests/fixtures/`, and cover the contexts and namespaces, each list,
a list at its cap, logs streamed and stopped, a port-forward's life, the
shell's command line (an odd context stays one argument; nothing runs
but kubectl), every question and the protected contexts, applying, a
Secret without its values, the agents' tools and their caps, and an
expired login, no kubectl and no kubeconfig.
