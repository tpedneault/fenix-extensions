# Rust

Rust in Fenix: rust-analyzer as the language server, debugging with
lldb-dap or CodeLLDB, clippy on save, rustfmt as the formatter, tasks from
Cargo.toml's targets, the project doctor's checks with their fixes,
outdated and vulnerable crates, snippets, and an Axum service in the
new-project wizard. Built on lang-kit (`crates/lang-kit`), as each
language pack is. It depends on `fenix.rust-snippets`, which stays
installable on its own.

What it knows of the project's packages and targets is `cargo metadata
--no-deps`'s, asked when it starts and when `Cargo.toml` is saved.

## The language server

rust-analyzer, when it's found (on `PATH`, or in `~/.cargo/bin`, where
rustup puts it). With none, Fenix's own default stays. rustup's
`rust-analyzer` is only a proxy until its component is added: the
doctor's fix row runs `rustup component add rust-analyzer` and starts
the server again.

## Linting and formatting

When a Rust file is saved, `cargo clippy --message-format json
--all-targets` (`--workspace` in a workspace) runs on the project, and
clippy's lints are shown with rust-analyzer's problems, in every file
clippy found some in, and taken off a file clippy no longer speaks of.
rust-analyzer already shows rustc's own errors and warnings, so only
`clippy::` lints are taken. What clippy suggests is a fix on `SPC c a`:
one it's sure of as it is, one it may have wrong with "(check it)"; one
with placeholders isn't offered. A save while it runs starts it again. `rust.clippy` turns it off (for a project too, in its
`.fenix/settings.toml`); `SPC m c` runs it now.

`SPC c F` -- and saving, with `editor.format_on_save` -- runs `rustfmt
--edition <the project's>` on the buffer. rustfmt doesn't read
Cargo.toml's edition, so it's given, unless `rustfmt.toml` says one.

## Debugging

The adapter is LLVM's lldb-dap, over stdio, by any of its names
(`lldb-dap`, Debian's `lldb-dap-18`, the older `lldb-vscode`); or
CodeLLDB, which Fenix starts on a free port and connects to
(`codelldb --port ${port}`), when it's installed and `rust.debugger` =
`codelldb` or there's no lldb-dap. CodeLLDB is found on `PATH`, or where
`rust.codelldb` says: its release's `extension/adapter/codelldb` (the
`.vsix` is a zip), or VS Code's copy. Neither is rustup's: the doctor's
row says how to install them.

`SPC u u` offers a launch for each binary and example cargo has (`Rust:
debug orbit`, `Rust: debug example demo`): its task, `build <name>`, runs
first, then its program is started from cargo's target folder, with
rustc's lldb formatters loaded (`rustc --print sysroot`'s), so a
`String` or a `Vec` shows its contents.

In a project with no launch configuration of its own it also offers:

| | |
|---|---|
| Rust: binary (cargo build) | the file's binary (`src/main.rs`, `src/bin/*.rs`), else its crate's |
| Rust: tests in this file (cargo test) | a `tests/*.rs` file's tests, or the crate's tests in the file's module (`src/parser.rs`: `parser::`) |
| Rust: this example (cargo build --example) | the example you're in |

A launch file can't name the program cargo makes, so lldb-dap builds it
first: each launch file gives it a little Python (its `initCommands`,
run by lldb before it makes its target; the source is
`src/debug/fenix_cargo.py`) that runs `cargo metadata`, then `cargo
build` or `cargo test --no-run` for the file you're in, and links what
cargo made at `target/fenix-debug/debuggee`, the launch file's program.
A build that fails says why in the debug output, and nothing is
started. The tests in a module are picked by their path, as `cargo test
parser::` would. The script also loads Rust's formatters for lldb, as
`rust-lldb` does, so a `String` or a `Vec` shows its contents.

It needs lldb's Python, which LLVM's packages include. The program runs
in the project's folder; a test that reads files from its package's
folder in a workspace may want a launch of its own.

## Tasks

`SPC p t`: build, run (or `run <bin>` for each binary), `example
<name>`, `build <bin>` and `build example <name>` (what a launch builds
first), test, bench when there are benches, clippy, fmt, and audit with
cargo-audit. A workspace's say `--workspace` and each target its
package (`-p`); an example that needs features gets them. A task of the
same name in `.fenix/tools.json` wins.

## The project doctor

- **Toolchain**: the one `rust-toolchain.toml` (or `rust-toolchain`)
  pins, installed (`rustup run <channel> rustc --version`). Fix: `rustup
  toolchain install <channel>` with its components and targets.
- **Rust**: rustc, held to the packages' `rust-version`. Fix: `rustup
  update`, when the project doesn't pin one.
- **cargo**.
- **rust-analyzer**, **clippy**, **rustfmt**: rustup's components. Fix:
  `rustup component add`.
- **lldb-dap** (or **CodeLLDB**, the one used, else the one asked
  for): how to install it, for each system.
- **cargo-audit**, **cargo-outdated**: for the package page. Fix:
  `cargo binstall` when there's cargo-binstall, else `cargo install
  --locked`.

A fix that installs something is checked again, and what it installed
is used at once.

## Crates

`SPC m p` (or *Rust: crates*): lang-kit's page -- the vulnerable first,
then the outdated. Each crate of the lock is listed (`cargo metadata`),
the workspace's own left out; those a package asks for are marked with
it in a workspace, and `dev` or `build` when that's all they are.

| | |
|---|---|
| outdated | `cargo outdated --format json --workspace` with cargo-outdated; else `cargo update --dry-run --verbose` (what the requirements take) and the newest of each crate the project asks for from crates.io's sparse index |
| vulnerable | `cargo audit --json`, with cargo-audit; the risk from the advisory's CVSS vector (lang-kit's `Risk::parse`) |
| `u` | a major step: `cargo add -p <package> name@latest` (`--dev`, `--build`); else `cargo update -p name@current` |
| `U` | `cargo update` |
| `a` | `cargo search`, then `cargo add` with what's typed (`serde -F derive`, `-p orbit rand`); a workspace with no package of its own adds to its first |
| `d` | `cargo remove -p <package> name` |

## Snippets

rust-snippets' (`fn`, `test`, `impl`, `match`, ...), and these:

- Errors: `thiserr` (a thiserror enum and its `Result`), `errv` (a
  variant), `errs` (a struct with a source), `errimpl` (by hand).
- Async: `amain` (`#[tokio::main]`), `atest`, `afn`, `spawn`, `select`,
  `chan` (an mpsc channel).
- Builders: `builder` (a builder with its `build()`), `setter`.
- Trait impls: `from`, `tryfrom`, `fromstr`, `default`, `iter`, `deref`,
  `drop`, `ord`; and `derive`.

## Project templates

Fenix's wizard already has a cargo binary or library (with clap, for a
command line) and a workspace; this adds:

- **Axum service**: a router with a page and a JSON route, tests that
  call it without a server (tower's `oneshot`), tracing, and a clean
  shutdown on Ctrl+C.

## Keys

After `SPC m` in a Rust project: `p` crates, `d` check again, `c`
clippy now.
