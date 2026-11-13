# Changelog

## 1.3.0

- For Fenix 1.6 (extension API 1.6): your Go tests on Fenix's tests
  page (`SPC T T`), in the gutter and after each test's line, run from
  any of them and debugged with `SPC T d`. A test is found as you type
  (`TestXxx`, `FuzzXxx` and `ExampleXxx` with an `// Output:` comment; a
  test that calls `t.Run` is a template whose subtests appear once run;
  a file under a build tag shows its tests as skipped); `go list` names
  the test files you haven't opened. Runs are `go test -json -fullpath`
  by anchored `-run` regexes, a step for each package, or for each
  module where there's no `go.work`. A failure is shown at the last
  `file_test.go:37:` line the test printed (a panic at its first frame
  in your code), with its message and output; a package that doesn't
  build says so and fails no test; a test binary that exits, panics or
  times out errors its own package's tests only. Debugging a test runs
  Delve's test mode on it. It needs Go 1.21 or later (`-fullpath`); the
  tests page says so with an older Go. On Fenix 1.5, 1.2.0 stays the one
  you have.

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
