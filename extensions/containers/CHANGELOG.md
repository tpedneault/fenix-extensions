# Changelog

## 1.2.0

For Fenix 1.2 and later (extension API 1.3): an older Fenix keeps 1.1.0.

- A build, a pull or a Compose stack's `up`, `pull` or `build` that ends
  while you're elsewhere -- no Containers page in view -- is a
  notification, which the system shows when Fenix isn't in front; Enter
  on it opens Containers, or a failed build's output. In view, or for
  something quick, it's the message line, as before.
- `SPC d b` builds as a task beside the task pane, its output in front
  of you as before, and the Images tab lists the new image when it's
  done. `Containers: the last build's output` brings the output back.
- Coding agents can read your containers: `containers_list` (each one's
  state, health, ports and Compose stack), `containers_logs` (the last
  lines of one's log, at most 2000 and 64 KiB, cut where it says) and
  `containers_images`. They only read: nothing is started, stopped or
  removed, and a container's environment and `inspect` are never given.
  A log can hold secrets; the README says how to be asked first.
- A lesson in the tutorial (`SPC h T`, under Containers): the page and
  its tabs, performance, a container's log, starting and stopping. It
  can be done without an engine.

## 1.1.0

- `containers.open`, `containers.performance` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): their pages opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.0: docker, Podman and wslc: containers, Compose, live logs, performance, images, volumes and networks.
