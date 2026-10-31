# Containers

Docker, Podman and wslc in Fenix: containers grouped by Compose stack,
the project's own `compose.yaml`, live logs merged across a stack,
performance, images and their layers, volumes and networks.

`SPC d d` opens it. `SPC d p` opens performance, `SPC d b` builds the
project's Dockerfile.

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

## Permissions

It runs `docker`, `podman` and `wslc`, and reads the project for its
`compose.yaml`. It sends nothing anywhere.

## Building it

```text
cargo xtask pack extensions/containers dist
```

It needs Rust's `wasm32-wasip2` target (`rustup target add wasm32-wasip2`).
