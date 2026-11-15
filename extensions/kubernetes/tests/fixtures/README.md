# Fixtures

Two kinds, said plainly.

**Recorded from kubectl 1.37.2** (the release binary, its sha256 checked),
run against a scratch kubeconfig and a small fake API server on
127.0.0.1 that served the hand-written lists below. No real cluster was
used.

| File | As kubectl printed it |
|---|---|
| `config-view.json` | `kubectl config view -o json`, of a kubeconfig with the names a text format breaks: `"prod "` (trailing space) current, `"prod"`, `"tab\there"`, `"new\nline"`. Credentials as kubectl redacts them (`REDACTED`, `DATA+OMITTED`) |
| `version-client.json` | `kubectl version --client -o json` |
| `version.json` | `kubectl version -o json`, the fake server answering `/version` |
| `workloads.tsv` | `kubectl get deployments,statefulsets,daemonsets,jobs,cronjobs -o jsonpath=<kubectl::WORKLOAD_COLUMNS>` over the lists below |
| `unauthorized.stderr` | kubectl's stderr when the server answers 401 |
| `no-context.stderr` | kubectl's stderr for `--context` naming none |

Seen there too, and relied on: `get --raw` prints the body as it came,
on one line with no newline after it.

**Hand-written** from the API's documented objects (apps/v1, batch/v1,
core/v1, networking.k8s.io/v1): the lists `deployments.json`,
`statefulsets.json`, `daemonsets.json`, `jobs.json`, `cronjobs.json`,
`pods.json`, `services.json`, `ingresses.json`, `namespaces.json`,
`events.json` (what `get --raw <list>?limit=N` returns); `pod-web.json`;
`secret-db.json` (as `get secret -o json`); `deployment-web.yaml` (as
`get -o yaml`); and `secrets.txt` (kubectl's `get secrets --no-headers`
table, NAME TYPE DATA AGE: the fake server didn't serve the Table format
kubectl asks for, so this one wasn't recorded).
