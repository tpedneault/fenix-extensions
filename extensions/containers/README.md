# Containers

Docker, Podman and wslc in Fenix: containers grouped by Compose stack,
the project's own `compose.yaml`, live logs merged across a stack,
performance, images and their layers, volumes and networks.

`SPC d d` opens it. `SPC d p` opens performance, `SPC d b` builds the
project's Dockerfile. With standard keys the leader is `Ctrl+K`:
`Ctrl+K d d`, `Ctrl+K d p`, `Ctrl+K d b`. The page's own keys below are
the same with either.

The tutorial (`SPC h T`) has a lesson on it, under **Containers**: the
page and its tabs, the performance page, a container's log, and what
starts and stops one. Without Docker, Podman or wslc you can still go
through it: the pages open and say what's missing, and `s` on the
tutorial page skips the two steps that need a running container.

## The page

| Key | What it does |
| --- | --- |
| `1`-`5`, `]`, `[` | Containers, Compose, Images, Volumes, Networks |
| `j` `k` `g` `G` | Move |
| `Enter` | Open the container or image |
| `m` | The container's menu |
| `s` `R` `p` `K` | Start or stop, restart, pause, kill |
| `l` | Its logs; `L` on a stack, the whole stack's |
| `!` | A shell in it |
| `o` | Open its published port |
| `i` | Its `inspect`, as JSON |
| `y` | Copy its id |
| `x x` | Remove it |
| `P P` | Prune unused images, volumes or networks; `P` on Containers opens performance |
| `u` `d` `b` | Compose up, down, build |
| `/` | Filter |
| `e` | Another engine |
| `r` | Reload |

A log page follows (`F`), shows times (`t`), filters (`/`), hides a
stream (`1`-`9`) and opens as a buffer (`o`).

## Engines

The first engine that runs is used: Docker, then Podman, then wslc.
`containers.engine` pins one. An engine that's installed but not running says
so, and `Enter` starts it (Docker Desktop, or Podman's machine).

wslc (WSL 2.9.3 or later, a preview) has containers and images, but no
Compose, volumes or networks yet; those tabs say so.

## Builds, pulls, and being told

`SPC d b` asks for a tag (`app:dev`) and builds the project's
`Dockerfile` (or `Containerfile`) as a task beside the task pane, its
output in front of you. `Containers: the last build's output`
(`containers.build_output`) brings that output back.

Something long -- a build, an image pulled (`p` on Images), a Compose
stack's `up`, `pull` or `build` -- that ends while none of Containers'
pages is in view is a notification: a toast, kept in the history
(`SPC o n`), the system's own when Fenix isn't in front, as your
settings say. Enter on it opens Containers, or a failed build's output.
With a Containers page in view, or for something quick (start, stop,
remove), it's a line in the message line, as before.

## For coding agents

Three tools for your coding agents, in the `containers` toolset. Each
only reads: it runs one engine command that reads, and nothing that
starts, stops, removes, execs or builds.

| Tool | Class | What it answers |
| --- | --- | --- |
| `containers_list` | read | Each container's id, name, image, state, status, health, exit code, published ports, and Compose project and service; stopped ones too unless `all` is false (`ps -a`) |
| `containers_logs` | read | The last `lines` (200, up to 2000) of one container's log (`logs --tail`, with times), never more than 64 KiB, a line longer than 2 KiB cut; it says when it left lines out |
| `containers_images` | read | Each image's repository, tag, id, size, age and whether it's dangling |

- **Logs can hold secrets**: whatever the program printed -- a token in a
  URL, a password in a stack trace. An agent that reads a log has it,
  and Fenix allows reading by default. To ask first, give
  `agents.mcp.allow` a rule: `containers_logs = "ask"`, or
  `"toolset:containers" = "ask"`.
- **Never a container's environment or `inspect`**, which hold its
  secrets, nor its command line (`ps` has it; the answer doesn't).
- **The container's name or id an agent gives** goes on the engine's
  command line, so it's refused unless it's only letters, digits, `_`,
  `.` and `-`, starting with a letter or digit, at most 128 -- never an
  option (`-f`, `--since`), a second argument or a path. For Docker and
  Podman it's also put after `--`; wslc isn't known to take `--`, so
  there the check is what keeps it a name.
- **An engine that isn't running** is said so, and isn't started; one
  still being looked for (Containers just loaded) is waited for.

## What it may do

- **Run `docker`, `podman` and `wslc`** (`process`): to list, start,
  stop and remove what you ask on its pages, stream logs and load,
  build and pull; for agents, only the three reads above.
- **Read the project** (`filesystem = "project-read"`): its
  `compose.yaml` for the Compose tab, and whether it has a `Dockerfile`
  to build.

It sends nothing anywhere itself, and writes nothing in the project:
what a build or `compose up` does is the engine's.

## Building it

```text
cargo xtask pack extensions/containers dist
cargo test --manifest-path extensions/containers/Cargo.toml
```

It needs Rust's `wasm32-wasip2` target (`rustup target add wasm32-wasip2`).
The tests read each engine's real output (Docker's and Podman's JSON,
wslc's columns), drive the pages with keys, and cover the agents' tools
-- their arguments, the names refused, a log's tail cut by lines and by
bytes -- and when an ending is a notification. None needs an engine.
