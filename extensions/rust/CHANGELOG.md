# Changelog

## 1.1.0

- `rust.packages` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.1

- Breakpoints work on Windows in the launches that build first (the tests in a file, a binary, an example): the program's `.pdb` now goes beside the copy that's debugged, without which lldb found no code for any breakpoint.
- Those launches work with CodeLLDB too: their first command used a prefix only `lldb-dap` knows, and CodeLLDB refused it.

## 1.0.0

- The first release, for Fenix 1.0: rust: rust-analyzer, debugging with lldb-dap or CodeLLDB (each binary and example, the tests in your file, built by cargo first), clippy on save with its suggestions as fixes, rustfmt, tasks from Cargo.toml's targets, the toolchain checked with its fixes, outdated and vulnerable crates, snippets, and an Axum service.
