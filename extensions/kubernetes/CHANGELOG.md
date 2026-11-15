# Changelog

## 1.0.0

- The first release, for Fenix 1.5 (extension API 1.5): Kubernetes
  through kubectl. A page of workloads with their pods, pods, services
  and ingresses, and Secrets by name, capped by the API server with how
  many more there are; the context and namespace chosen for the project,
  every call naming its context and nothing run without one -- kubeconfig
  is never changed; live logs (a container or all, the previous run,
  since a while ago, stopped and started again); a shell in a pod;
  port-forwards as background tasks, listed and stopped; YAML and
  `describe` in a read-only buffer, a Secret's without its values;
  deleting a pod, restarting and scaling, each asked about naming the
  context and the namespace, and every context but a local cluster's
  protected, a change there needing the object's name typed. Read-only
  tools for coding agents in the namespace you chose, logs and
  descriptions only when you allow them. A doctor row for kubectl, and a
  lesson in the tutorial.
