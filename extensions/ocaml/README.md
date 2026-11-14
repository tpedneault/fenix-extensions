# OCaml

OCaml in Fenix: its grammars (Fenix has none of its own), one for
implementations and one for interfaces, ocaml-lsp-server, ocamlformat
where the project has an `.ocamlformat`, dune's tasks with a `dune exec`
for each executable, debugging bytecode with earlybird, the project
doctor's checks with fixes that run `opam install`, snippets, and a dune
project. Built on lang-kit (`crates/lang-kit`), as each language pack is.
It needs Fenix 1.5 or later (extension API 1.5); on an older Fenix, OCaml
1.0.0 is the one you get.

It works for a dune project (`dune-project`, or `dune` files) and for a
folder of scripts.

## opam's switch

OCaml's tools live in an opam switch -- the project's own (`_opam`), or
the one opam has chosen -- which an editor started from the desktop
rarely has on its `PATH`. Each tool is looked for on `PATH`, in the
project's `_opam/bin`, opam's `default` switch, Homebrew's and Nix's
folders; one that isn't there, while opam is, runs through `opam exec
--`, in the switch opam picks for the project, once the doctor has seen
it answer there. The tasks run dune that way too.

## The grammars

`*.ml` files are read with tree-sitter-ocaml 0.24.2's grammar for
implementations, `*.mli` with its grammar for interfaces (`grammars/`,
MIT), a language of its own (`ocaml_interface`) with the same queries:
highlighted (doc comments, `(** ... *)`, apart), folded (each `let`, at
the top or in an expression, a type, a module and its signature, a
class, a `match`), with Enter indenting after a line that ends in a
binding's `=`, a case's or a `fun`'s `->`, `struct`, `sig`, `object`,
`begin`, `do`, `then`, `else` or a bracket, and the breadcrumbs and `SPC
c s` listing the definitions. OCaml has no line comment, so `gcc` does
nothing in it.

## The server

ocaml-lsp-server (`ocamllsp`), for both: completion, types on hover, go
to definition, references, rename, and dune's and the compiler's
problems as you type. It asks dune how the project builds (`dune
ocaml-merlin`), so its libraries' types are there once `dune build` has
run.

## Formatting

`SPC c F`, and saving with `editor.format_on_save`: `ocamlformat --name
<file> -`, only in a project with an `.ocamlformat` at its top -- as
dune's `dune fmt` does, and as dune's users expect: one without isn't
formatted (`ocaml.ocamlformat` off never is). An `.ocamlformat` that pins
a `version` is held to it by the doctor, whose fix installs that version:
ocamlformat of another refuses to format.

## Tasks

`SPC p t`:

| Task | Runs |
|---|---|
| build | `dune build` |
| test | `dune test`; `test <dir>` for each folder with tests, when there are several |
| exec *name* | `dune exec <public name>`, or `dune exec ./<dir>/<name>.exe`, for each executable the `dune` files name |
| build *dir/name*.bc | the bytecode a launch debugs, with earlybird |
| fmt, clean | `dune fmt`, `dune clean` |
| opam: install dependencies | `opam install . --deps-only --with-test`, with `.opam` files |
| run *file* | `ocaml <file>`, for each OCaml file at the top of a folder without dune |

The executables are the `executable` and `executables` stanzas of the
project's `dune` files (five folders down, never `_build` or `_opam`),
read by the pack's own S-expression reader. The project's `.env` goes to
each task; a task of the same name in `.fenix/tools.json` wins. Saving a
`dune` file, `dune-project`, an `.opam` file or `.ocamlformat` reads them
again (`SPC m r` too).

## Debugging

earlybird (`ocamlearlybird debug`), OCaml's debug adapter, debugs
bytecode: an executable whose `dune` file says `(modes byte exe)` has a
launch of its own on `SPC u u` (*OCaml: debug <name>*), its `.bc` built
first and run from `_build/default`. A native-only executable has none:
earlybird can't debug one. `ocaml.debug` off sets neither the adapter
nor the launches.

## The project doctor

| Check | | Fix row |
|---|---|---|
| opam | found | opam's page (https://opam.ocaml.org/doc/Install.html, `SPC m g`) |
| OCaml | `ocaml -vnum`; needed | -- `opam switch create` makes one |
| dune | held to `dune-project`'s `(lang dune ...)`; in a dune project | `opam install dune` |
| ocaml-lsp-server | `ocamllsp --version` | `opam install ocaml-lsp-server` |
| ocamlformat | held to `.ocamlformat`'s `version`; where it formats | `opam install ocamlformat.<version>` |
| earlybird | with executables, while it debugs | `opam install earlybird` |

Without opam, each fix row opens opam's page. A fix runs as a task; the
doctor checks again after it, and what it installed is used at once.

## Snippets

| Key | |
|---|---|
| `let`, `letin`, `fn`, `rec`, `fun` | a binding, `let ... in`, a function, a recursive one over a list, `fun` |
| `match`, `if`, `try`, `for` | the expressions |
| `mod`, `modtype`, `functor` | a module, a module type, a functor |
| `type`, `record`, `exn` | a variant, a record, an exception |
| `main`, `printf` | `let () =`; `Printf.printf` |
| `alcotest`, `expect` | an Alcotest suite; a ppx_expect test |

In an interface: `val` (with its doc comment), `type` (abstract), `mod`
(a module's signature).

## The project template

*OCaml · dune project* (`SPC p n`): as `dune init project` makes one --
a library (unwrapped: its module is `Greeting`), an executable over it
built as bytecode too, for earlybird, a test on the standard library
alone, an `.ocamlformat`, and the `.opam` file dune writes -- with `dune
exec` as its run task.

## Settings

| Setting | Default | |
|---|---|---|
| `ocaml.ocamlformat` | on | ocamlformat, in a project with an `.ocamlformat` |
| `ocaml.debug` | on | earlybird, and the launches |

## Project kind

Fenix 1.6 and later know an OCaml project (`ML`): its tag in front of its name in the project hub, on Home, in the modeline and in the doctor, found by a `dune-project`, `dune-workspace` or `<folder>.opam` at its top, and opened on `bin/main.ml` when you've had none of its files open. `kind = "ocaml"` in `.fenix/settings.toml` says it by hand. Fenix's own kinds come first: a project Fenix already takes for Rust, Python, Go, JavaScript, C/C++, Tcl or Arduino stays that one until `kind = "ocaml"` says otherwise, so this kind names projects Fenix would otherwise call plain folders. Earlier Fenix ignores this and the pack works there as before. The kind is declared in `extension.toml` (`[[contributes.project_kinds]]`); the pack's context markers say when the pack applies, and are not the same list.

## Keys

After `SPC m` in an OCaml project or file (`C-k m` with standard keys):
`d` checks the project again, `r` reads it again, `g` opens opam's page.

## Not yet

- **dune's own files**: Fenix has no grammar for `dune`, `dune-project`
  or `.opam` files; ocamllex's `.mll` and Menhir's `.mly` aren't read as
  OCaml either.
- **Native debugging**: a native executable debugs with GDB or LLDB, as
  C's does; the pack sets no launches for it.
- **Reason and `.mlx`**: not read.
