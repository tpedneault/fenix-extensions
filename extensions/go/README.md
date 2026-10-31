# Go

Go in Fenix: gopls as the language server, debugging with Delve, go vet
or golangci-lint on save, gofmt or goimports as the formatter, tasks,
the project doctor's checks with their fixes, outdated and vulnerable
modules, snippets, and CLI, HTTP service and library projects in the
new-project wizard. Built on lang-kit (`crates/lang-kit`), as each
language pack is.

What `go install` installs goes to `~/go/bin`, which an editor started
from the desktop often doesn't have on its `PATH`: the pack looks there
too, and in Go's own folders (`/usr/local/go/bin`, `C:\Program
Files\Go\bin`, Homebrew's).

## The language server

gopls, as found. Without it, Fenix starts `gopls` from `PATH` as
before, and the doctor's fix row installs it (`go install
golang.org/x/tools/gopls@latest`).

## Linting and formatting

When a Go file is saved, its package is linted and the problems in that
file are shown with gopls': golangci-lint's (`run --output.json.path
stdout`, or 1.x's `--out-format json`) when it's installed, else `go vet
-json`. A package that doesn't compile shows the compiler's errors.
`go.linter` picks go vet, or turns it off.

`SPC c F` -- and saving, with `editor.format_on_save` -- runs goimports
on the buffer when it's installed (`-srcdir` the file, so the package's
own imports are known), else gofmt. `go.formatter` can ask for gofmt.

## Debugging

The adapter is Delve's DAP server, `dlv dap --listen 127.0.0.1:${port}`:
Fenix starts it in the project for each session, on a port it picks,
and connects to it. Delve builds the package it's given in the project,
so `go.mod` is found.

`SPC u u`, in a project with no launch configuration of its own, offers:

| | |
|---|---|
| Go: the project's main package | the package at the top |
| Go: this package | the package of the file you're in: a `cmd/<name>` |
| Go: test this package | its tests (`mode: test`, `-test.v`), run in its folder |
| Go: attach to a process | a program running here, picked as the session starts (`mode: local`) |
| Go: attach to a headless Delve | a Delve already running, below |

*Go: attach to a process* lists this machine's processes
(`${pickProcess}`) and Delve attaches to the one picked. On Linux that
needs ptrace: `kernel.yama.ptrace_scope` at 0, or Delve run as root.
It's for the Delve Fenix starts, so not with `go.delve_server` set.

To debug through a Delve of your own -- on another machine's port, or
started before Fenix -- start it headless and set `go.delve_server` to
its address:

```sh
dlv attach <pid> --headless --listen 127.0.0.1:2345 --accept-multiclient
dlv exec ./orbit --headless --listen 127.0.0.1:2345 --accept-multiclient
```

Fenix then connects to that Delve in place of starting one, for every
session, until the setting is emptied again.

Delve needs a Go it supports: Delve 1.27 says Go 1.24 is too old when a
session starts. An older Delve (`go install
github.com/go-delve/delve/cmd/dlv@v1.25.2`) or a newer Go fixes it.

## Tasks

`SPC p t`: build (`go build ./...`), run (each main package: the top
one, and `run <name>` for each `cmd/<name>` beside it), test, vet, lint
(golangci-lint, when it's installed), format (goimports `-w`, else `go
fmt`), tidy, and vulncheck (govulncheck, when it's installed). The
project's `.env` goes to each. A task of the same name in
`.fenix/tools.json` wins. Saving `go.mod`, `go.work` or golangci-lint's
config makes them again.

## The project doctor

- **Go**: `go version` in the project, which is the toolchain Go
  switches to for it, held to `go.mod`'s `go` line (1.21 at least, the
  first that switches). Fix: `brew install go` on macOS, winget's
  `GoLang.Go` on Windows.
- **Toolchain**: when `go.mod`'s `toolchain` line is newer than its
  `go` line, the same held to it, as a warning: it's a suggestion, and
  `GOTOOLCHAIN=local` keeps the Go you have.
- **Modules**: `go mod tidy -diff` (Go 1.23): go.mod and go.sum tidy, or
  which isn't. Fix: `go mod tidy`.
- **gopls**, **Delve**, **govulncheck**, and **golangci-lint** where the
  project has a `.golangci.yml` (or it's installed), each with its `go
  install`.

A fix that installs something is checked again, and what it installed
is used at once.

## Packages

`SPC m p` (or *Go: modules*): lang-kit's page -- the vulnerable first,
then the outdated, the module's own requirements (go.mod's, not `//
indirect`) before the rest of the build list.

| | |
|---|---|
| list | `go list -m -json all` |
| outdated | `go list -m -u -json all` |
| vulnerable | `govulncheck -json ./...` |
| `u` | `go get path@<latest>` (the newest that fixes it, for one with no update); Go itself, `go get toolchain@go1.x.y` |
| `U` | `go get` each outdated one at its latest |
| `a` | `go get path`, or `path@version` |
| `d` | `go get path@none` |

A replaced module (`replace`) is shown at the version that replaces it,
in the group *replaced*, and isn't upgraded: go get doesn't move a
replace. Go's vulnerability database says no severity, so each
advisory's risk is unknown; its title says whether the code calls the
vulnerable function (and where), only imports its package, or only
requires its module. Go's own packages are `stdlib`.

go get doesn't tidy: when it leaves go.mod untidy, the doctor's
*Modules* row says so, and `SPC m t` tidies.

## Snippets

Errors: `iferr`, `errw` (wrap with `%w`), `errv` (a sentinel), `errt` (an
error type with `Unwrap`), `erras`. Tests: `tt` (table-driven), `test`,
`bench`. HTTP: `hf` (a handler), `hroute` (a route with a pattern),
`hjson`, `hsrv` (a server that shuts down on Ctrl-C). Concurrency: `gof`,
`wg`, `ch`, `sel` (with the context), `pool`, `ctx`. Types: `st`, `meth`,
`new`, `iface`, `str`. And `main`.

## Project templates

Fenix's wizard has no Go templates; this adds three, each asking for the
module path's start (`example.com`, `github.com/you`) and made with `go
mod init`:

- **Go · command-line tool**: flags and a subcommand parsed in a `run`
  that returns an error, and table-driven tests that run it.
- **Go · HTTP service**: `net/http`'s routing patterns, JSON handlers in
  `internal/server`, `log/slog`, a health check, a graceful shutdown,
  and tests with `httptest`.
- **Go · library module**: a documented package, table-driven tests, and
  an example `go test` checks.

## Keys

After `SPC m` in a Go project: `p` modules, `d` check again, `t` tidy.
