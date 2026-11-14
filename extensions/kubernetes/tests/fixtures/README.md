# Fixtures

Hand-written, not recorded: there was no cluster (and no kubectl) where
these tests were written. Each is shaped as kubectl prints it, from the
Kubernetes API's documented objects (apps/v1, batch/v1, core/v1,
networking.k8s.io/v1) and kubectl's documented output:

| File | As printed by |
|---|---|
| `config.txt` | `kubectl config view -o jsonpath=...` (`kubectl::config_view`) |
| `version-client.json` | `kubectl version --client -o json` |
| `version.json` | `kubectl version -o json` |
| `deployments.json`, `statefulsets.json`, `daemonsets.json`, `jobs.json`, `cronjobs.json`, `pods.json`, `services.json`, `ingresses.json`, `namespaces.json`, `events.json` | `kubectl get --raw <the list's path>?limit=N`: the API's List, with `metadata.continue` and `remainingItemCount` when there's more |
| `pod-web.json` | `kubectl get --raw /api/v1/namespaces/shop/pods/<pod>` |
| `secrets.txt` | `kubectl get secrets --no-headers` |
| `secret-db.json` | `kubectl get secret db-credentials -o json` |
| `deployment-web.yaml` | `kubectl get deployment web -o yaml` |
| `dry-run.json` | `kubectl apply --dry-run=server -o json -f -` |
| `diff.txt` | `kubectl diff -f -` (exit 1: it differs) |

Recorded output from a real cluster would replace them one for one.
