# Changelog

## 1.3.0

- For Fenix 1.6 (extension API 1.6): your tests on Fenix's tests
  page (`SPC T T`), in the gutter and after each test's line, run
  from any of them and debugged with `SPC T d`. A test is found as
  you type (`#[test]`, `#[tokio::test]` and any `#[<crate>::test]`,
  `#[ignore]`d ones as skipped, `#[rstest]` and `#[test_case]` cases
  once run); what a macro makes and doc tests come from `cargo test
  -- --list`, which builds the tests first. Runs are `cargo test
  --no-fail-fast`, by exact names, a step per package; with
  cargo-nextest 0.9.103 or later installed, `cargo nextest run`,
  which gives each test's time (cargo test doesn't). A failure is
  shown at its panic, with its message and output; a build that
  fails says so and fails no test; a binary that crashes errors its
  own tests only. Debugging a test builds its binary, then runs it
  under lldb-dap or CodeLLDB with `<name> --exact --nocapture`. The
  project doctor has a row for cargo-nextest, optional, with a fix
  that installs it. On Fenix 1.5, 1.2.0 stays the one you have.

## 1.2.0

- For Fenix 1.5 (extension API 1.5): built against API 1.5, as lang-kit,
  the kit it's on, now is. The text the lldb-dap fix row opens says the
  keys that check the project again as the key scheme you use has them
  (`SPC m d`, `C-k m d` with standard keys, or the ones you gave it).
  Nothing else you see changed: the fixes that install Rust's tools run
  rustup and cargo as tasks, in Fenix's task pane, not as a download of
  the pack's own, so there is no install notification for it. The README
  gives standard keys beside Vim's. On Fenix 1.0 to 1.4, 1.1.0 stays the
  one you have.
- A lesson in Fenix's tutorial (`SPC h T`, its *Rust* section), in a Rust
  project of yours: the project doctor and a fix from it, the crates
  page, a task, a debug session stopped at a breakpoint, and rustfmt and
  clippy on save.

## 1.1.0

- `rust.packages` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.1

- Breakpoints work on Windows in the launches that build first (the tests in a file, a binary, an example): the program's `.pdb` now goes beside the copy that's debugged, without which lldb found no code for any breakpoint.
- Those launches work with CodeLLDB too: their first command used a prefix only `lldb-dap` knows, and CodeLLDB refused it.

## 1.0.0

- The first release, for Fenix 1.0: rust: rust-analyzer, debugging with lldb-dap or CodeLLDB (each binary and example, the tests in your file, built by cargo first), clippy on save with its suggestions as fixes, rustfmt, tasks from Cargo.toml's targets, the toolchain checked with its fixes, outdated and vulnerable crates, snippets, and an Axum service.
