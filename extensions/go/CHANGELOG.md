# Changelog

## 1.2.0

- A lesson in Fenix's tutorial (`SPC h T`, its *Go* section), in a Go
  module of yours: the project doctor and a fix from it, the modules
  page, a task, a debug session stopped at a breakpoint, and goimports
  and the linter on save.
- For Fenix 1.5 (extension API 1.5): built against API 1.5, as the
  shared kit it's built on now is. Nothing else you see changed: this
  pack installs Go and its tools (gopls, Delve, govulncheck,
  golangci-lint) with the fixes the doctor offers, which run as tasks, so
  it has no download of its own to say the end of in a notification. On
  Fenix 1.0 to 1.4, 1.1.0 stays the one you have. The README gives
  standard keys beside Vim's.

## 1.1.0

- `go.packages` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.0: go: gopls, debugging with Delve, go vet or golangci-lint on save, gofmt or goimports as the formatter, tasks, the project's checks with their fixes, outdated and vulnerable modules, snippets, and CLI, HTTP service and library projects.
