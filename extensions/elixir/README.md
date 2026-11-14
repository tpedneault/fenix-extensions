# Elixir

Elixir in Fenix: its grammar and HEEx's (Fenix has neither of its own),
Expert, `mix format`, the compiler's warnings and Credo on save, `mix`'s
tasks, the project doctor's checks with fixes that install Hex and
Expert, project templates, a lesson, and snippets. Built on lang-kit
(`crates/lang-kit`), as each language pack is.

It starts with Fenix. Expert is set for Elixir files whenever it's found;
the doctor checks Elixir, Hex and Expert in every project, and the
tasks, linters and `mix format` are there in a project with a `mix.exs`
at its top. It needs Fenix 1.5 or later (extension API 1.5).

## Files

| Files | Read as |
|---|---|
| `*.ex`, `*.exs`, `mix.lock` | Elixir |
| `*.heex`, `*.eex`, `*.leex` | HEEx |

`#` comments a line in Elixir.

## The grammars

Elixir is read with tree-sitter-elixir 0.3.5's grammar and HEEx with
tree-sitter-heex 0.8.1's (`grammars/`, Apache-2.0 and MIT; their
README says where each came from and its SHA-256): highlighted, folded
(modules, functions, tests, `do` blocks, collections, tags and
components), with Enter indenting inside a block and the breadcrumbs and
`SPC c s` listing the definitions.

- **EEx and LEEx** files are read as HEEx: its `<% %>` is theirs, and
  the HTML around it is what they hold. There's no grammar of their own
  here.
- **The Elixir inside a template** (`<%= ... %>`, `{...}`) isn't
  coloured: Fenix doesn't run one language inside another, so a HEEx file
  shows its tags, components, slots, attributes and delimiters.

## The server

**Expert** (`expert`, [expert-lsp/expert](https://github.com/expert-lsp/expert)),
the Elixir project's official language server -- Lexical and Next LS
merged -- started as `expert --stdio`: completion, go to definition,
hover, references, rename, code actions and its own problems.

Why Expert and not ElixirLS, Next LS or Lexical: it's the one the
Elixir project calls official and keeps tagging (its tags run to
v0.2.3), and it ships as one file per system on GitHub, which installs
into the tools folder like any other release. Next LS and Lexical are
what it was made from; ElixirLS is the long-standing alternative, but
comes as a zip run through a launch script, which this pack would have
to unpack and wrap, and is no longer the project the Elixir team points
to.

I couldn't open GitHub's release pages or API from where this version
was made, so the asset names (`expert_linux_amd64`, `expert_darwin_arm64`,
`expert_windows_amd64.exe`, ...) are the ones Expert's README, its
`justfile` and its release workflow give, and v0.2.3 is the newest tag
`git ls-remote` shows; that a release is published under it, and what its
files hash to, is unchecked (see the doctor, below).

Expert builds what it needs for your project the first time it opens
it, which takes a while. It's **alpha**: it works for most projects, and
may not for an old Elixir or Erlang (the Expert project lists the
supported ones).

## Formatting and checks

- `SPC c F`, and saving with `editor.format_on_save`: `mix format
  --no-compile --stdin-filename <file> -`, in the project's folder, so
  its `.formatter.exs` (`import_deps`, `locals_without_parens`) applies.
  HEEx files are formatted too when `.formatter.exs` lists
  `Phoenix.LiveView.HTMLFormatter` in its `plugins`. Fenix gives a
  formatter three seconds, and `mix format` has to start the Erlang VM:
  on a big project with plugins it may not finish in time.
- On save, **the compiler**: `mix compile --all-warnings` -- Mix compiles
  what changed and prints the warnings of the rest again, so a warning
  stays until it's fixed -- read into problems with their file, line and
  column and the underline's width: an unused variable, an undefined
  function, a missing module, and a compile error (a syntax error, an
  undefined variable) where it is. Elixir before 1.16 prints no column:
  the problem is the whole line. A dependency's warnings are left out
  (`elixir.compiler_warnings`).
- On save, in a project that has **Credo** (`:credo` in `mix.exs`, or a
  `.credo.exs`): `mix credo <file> --format json` on the file, its issues
  as problems -- a warning for Credo's `warning` checks, information for
  the others, a hint for a low-priority one (`# TODO` tags) -- with the
  check's name as the code (`elixir.credo`).

Both need the project's dependencies fetched (`mix deps.get`); without
them they say so, once, and set nothing.

## Tasks

`SPC p t`:

| | |
|---|---|
| `deps.get` | `mix deps.get` |
| `compile` | `mix compile` |
| `test` | `mix test` |
| `test --failed` | `mix test --failed`: what failed last time |
| `format --check-formatted` | `mix format --check-formatted` |
| `format` | `mix format` |
| `credo` | `mix credo`, when the project has Credo |
| `phx.server` | `mix phx.server`, when Phoenix is a dependency |

`iex -S mix` isn't a task, since a task's output is read and iex is typed
into: `SPC m i` opens it in a terminal. The project's `.env` goes to each
task.

**Tests** run through the `mix test` tasks for now. The tests page (`SPC T`),
the gutter and running one test are Fenix's test explorer, which a
language pack feeds through a test provider; Elixir's, for ExUnit, is a
later release, once Fenix has them.

## The doctor

| Check | | Fix row |
|---|---|---|
| Elixir | `elixir --short-version`, held to the version `mix.exs`'s `elixir:` asks for; needed | |
| Hex | `mix hex.info`; `mix deps.get` needs it | `mix local.hex --force`, as a task |
| Expert | `expert --version`; Fenix works without it | Expert 0.2.3 (Linux, macOS, Windows on x86_64; Linux and macOS on arm64) |

Elixir is the one the pack finds on `PATH`, or in asdf's and mise's
shims, Homebrew's and the Windows installer's folders; installing it (and
Erlang) is yours: [elixir-lang.org/install](https://elixir-lang.org/install.html),
asdf, mise.

**Expert's download is checked.** Its release comes from
`github.com/expert-lsp/expert/releases/download/v0.2.3/expert_<os>_<arch>`,
unpacked into the pack's tools folder, where only the pack looks. Each
file's SHA-256 in `src/tools.rs` is the one the release's own
`expert_checksums.txt` gives, and Fenix refuses a download that doesn't
match it. One you installed yourself (`~/.local/bin/expert`, as Expert's own
`just install` makes) is used instead.

Starting an install is said in the message line; how it ended is a
notification, since a download can take a while: "Installed Expert
0.2.3", or "Couldn't install Expert 0.2.3" with which step failed and
why, and *Try again*.

## Project templates

Fenix's new-project wizard has no Elixir templates; this adds two, each
with tests and its `.formatter.exs`, made from files (no `mix new` is
run):

- **Elixir · library**: a Mix library, a documented module with doctests.
- **Elixir · OTP application**: a supervisor, a `GenServer` counter under
  it, and tests that start it with `start_supervised!`.

## Snippets

| Key | |
|---|---|
| `defmodule`, `moduledoc`, `doc`, `docex`, `spec`, `typ`, `callback`, `behaviour` | a module and its documentation, specs and types |
| `def`, `defp`, `defg`, `defone`, `defmacro`, `defs`, `defex`, `defimpl`, `defproto` | functions (with a guard, in one line), a macro, a struct, an exception, a protocol and its implementation |
| `genserver`, `init`, `hcall`, `hcast`, `hinfo`, `hcont`, `term`, `sup`, `agent`, `task` | a `GenServer` and each of its callbacks, a supervisor, an agent, `Task.async_stream` |
| `case`, `with`, `cond`, `if`, `ife`, `unless`, `try`, `receive` | control flow, with its `else` and `after` |
| `fn`, `fnm`, `for`, `forinto` | anonymous functions (one clause, several), comprehensions |
| `pipe`, `pmap`, `pfilter`, `preduce`, `pthen`, `ptap`, `insp` | a pipeline and the steps that go in one; `IO.inspect` with its label |
| `alias`, `import`, `importo`, `use` | |
| `exunit`, `test`, `testc`, `describe`, `setup`, `setupc`, `doctest`, `assert`, `refute`, `araise`, `areceive`, `tag` | an ExUnit module and what goes in one |

## Settings

| Setting | Default | |
|---|---|---|
| `elixir.compiler_warnings` | on | `mix compile --all-warnings` on a saved file |
| `elixir.credo` | on | `mix credo` on a saved file, in a project that has Credo |

## Project kind

Fenix 1.6 and later know an Elixir project (`EX`): its tag in front of its name in the project hub, on Home, in the modeline and in the doctor, found by a `mix.exs` at its top, and opened on `lib/<folder>.ex` when you've had none of its files open. `kind = "elixir"` in `.fenix/settings.toml` says it by hand. Fenix's own kinds come first: a project Fenix already takes for Rust, Python, Go, JavaScript, C/C++, Tcl or Arduino stays that one until `kind = "elixir"` says otherwise, so this kind names projects Fenix would otherwise call plain folders. Earlier Fenix ignores this and the pack works there as before. The kind is declared in `extension.toml` (`[[contributes.project_kinds]]`); the pack's context markers say when the pack applies, and are not the same list.

## Keys

After `SPC m` in an Elixir project or file (`C-k m` with standard keys):
`d` checks the project again, `i` opens `iex -S mix` in a terminal.

## Not yet

- **ExUnit as a test provider**: the tests page, gutter marks and
  running one test. Tests run through the `mix test` tasks until then.
- **Debugging**: there's no adapter or launch set; Elixir's debugger
  (`:debugger`, ElixirLS's adapter) isn't wired.
- **Hex's packages page**: dependencies are `mix.exs`'s and `mix deps.get`'s;
  there's no page for outdated or vulnerable ones (`mix hex.outdated`,
  `mix hex.audit`).
- **EEx's own grammar**, and the Elixir inside templates: see above.

## What it may do

- **Run `mix`, `elixir`, `iex` and `expert`** (`permissions.process`): to
  compile, test, format and run Credo (the tasks you start, and what runs
  when you save), to ask Elixir and Expert their versions for the doctor,
  to open `iex -S mix` in a terminal, and to start Expert as the language
  server. And programs in the pack's own tools folder (`/tools/*`), where
  Expert is installed. No other program.
- **Reach `github.com`** and the hosts GitHub sends its downloads to
  (`permissions.network`): to download Expert's release when you run its
  fix row. Nothing else is fetched; `mix deps.get` has its own network.
- **Read the project's files** (`filesystem = "project-read"`):
  `mix.exs` (its dependencies and the Elixir it asks for),
  `.formatter.exs` (whether HEEx has a formatter), `.credo.exs` and
  `.env`. It writes none.
- **Read the file you saved** (`editor = "read"`), to compile and check it.

## Building

```text
cargo xtask pack extensions/elixir dist
cargo xtask install extensions/elixir
cargo test --manifest-path extensions/elixir/Cargo.toml
```

`cargo test` runs the pack on the test kit's fake host (`src/lib.rs`):
the server, the formatter and the tasks set, the doctor's rows and their
fixes, Expert's install and a failed one's notification, the linters run
on a save and a file made clean, `iex` in a terminal. Beside it: the
compiler's output and Credo's JSON read as problems (`src/lint.rs`); the
project's `mix.exs` read (`src/project.rs`); and the data as Fenix reads
it -- the language files, the snippets rendered, both grammars loaded and
their queries run, the templates planned, and the manifest against the
code (`src/data.rs`).

`src/samples/` holds what the tests read. Recorded from real tools
(Elixir 1.20.5 on Erlang/OTP 29): the compiler's output for warnings,
an undefined variable and a syntax error. Written from the formats'
documentation and sources: Credo's JSON (from `Credo.CLI.Output.Formatter.JSON`),
`mix hex.info`, and the compiler's older output (Elixir 1.15); Credo and
Hex weren't installed. Expert's `--version` output is assumed to be its
number.

## Licence

The pack is MIT OR Apache-2.0, as the repository is. The grammars keep
theirs: tree-sitter-elixir's Apache-2.0 (`grammars/LICENSE-tree-sitter-elixir`)
and tree-sitter-heex's MIT (`grammars/LICENSE-tree-sitter-heex`).
