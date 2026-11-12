# {{name}}

<!-- TODO(you): say which language this is for, and what it does for it,
in the second person: "Zig in Fenix: ..." (extensions/zig/README.md is
the model). Built on lang-kit (`crates/lang-kit`), as each language pack
is. -->

A language pack, started from the template: a language file, a check on
the project doctor's page that finds the language's tool, tasks, a
snippet, and tests. It's a skeleton: it names a placeholder tool,
`example-tool`, and installs nothing.

It starts with Fenix, and does its work in a project that holds
`{{folder}}.toml` -- TODO(you): the file that marks yours.

## Files

`*.example` files, read as Lua. TODO(you): your language's files, in
`languages/{{folder}}.toml`.

## Tasks

`SPC p t`:

| Task | Runs |
|---|---|
| `build` | `example-tool build` |
| `test` | `example-tool test` |

## The doctor

`SPC p h` shows the project's checks:

| Check | | Fix row |
|---|---|---|
| Example tool | `example-tool --version`; a warning when it isn't found | |

`SPC m d` checks again.

## Snippets

| Key | |
|---|---|
| `hello` | `print("hello")` |

## Keys

After `SPC m` in a project of the pack's context: `d` checks the project
again.

## What it may do

- **Run `example-tool`** (`permissions.process`): to ask its version for
  the doctor, and to build and test, as the tasks you start. No other
  program.

It asks for nothing else: no network, no files of the project, no
editor.

## Building

```text
cargo xtask pack extensions/{{folder}} dist
cargo xtask install extensions/{{folder}}
cargo test --manifest-path extensions/{{folder}}/Cargo.toml
```

`cargo test` runs the pack on lang-kit's fake host: its tasks are set,
the doctor warns about a missing tool and reads the version of one it
finds, and its commands are answered (`src/lib.rs`); the tool's check
and the tasks' table on their own; and the data as Fenix reads it -- the
language file, the snippets, and that the manifest lists every command
the kit and the keymap use (`src/data.rs`).
