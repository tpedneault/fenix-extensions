# Ada and SPARK

Ada and SPARK in Fenix: their grammar (Fenix has none of its own),
AdaCore's Ada Language Server for Ada and for GNAT's project files,
Alire's or gprbuild's tasks, GNATprove's findings as problems, the
project doctor's checks with fixes that install Alire, GNAT, gprbuild,
GNATprove and the server, and snippets. Built on lang-kit
(`crates/lang-kit`), as each language pack is.

It starts with Fenix. The server is set for Ada files whenever it's
found; the doctor checks, and the tasks run, in an Ada project: an Alire
crate (`alire.toml`), a GNAT project file (`.gpr`) at the top, or Ada
sources.

## The grammar

`*.ads`, `*.adb` and `*.ada` are read with tree-sitter-ada 0.9.2's
grammar (`grammars/`, MIT): highlighted, folded (packages, subprograms,
tasks, protected objects, records, `if`, `case`, loops, handlers), with
Enter indenting inside a unit or a statement, and the breadcrumbs and
`SPC c s` listing them. SPARK's aspects (`SPARK_Mode`, `Global`, `Pre`,
`Post`) and pragmas (`Loop_Invariant`, `Assert`) are coloured as
attributes, `'Old` and `'Range` as properties. `--` comments a line.

Project files (`*.gpr`) are a language of their own, `gpr`, read with
the same grammar, which isn't theirs: their keywords, strings and
comments are coloured, and they don't fold.

## The server

The Ada Language Server (`ada_language_server`): completion, go to
definition, references, rename, hover, its own problems, and formatting
(`SPC c F`, and saving with `editor.format_on_save`), which it does
itself with GNATformat. Project files get it too, in its GPR mode
(`--language-gpr`).

It's told the project file to load (`ada.projectFile`, its own
setting): the one you choose, else an Alire crate's (`<name>.gpr`), else
the only one at the project's top. With several and none chosen, the
doctor says so. In an Alire crate, with Alire found, it runs in the
crate's environment (`alr exec -- ada_language_server`), where Alire's
GNAT and the dependencies' project files are.

## Tasks

`SPC p t`:

| | In an Alire crate | With a project file |
|---|---|---|
| `build` | `alr build` | `gprbuild -P <gpr>`; and `build <main>` for each main when `for Main use` lists several |
| `run` | `alr run`; `run <exe>` for each of several `executables` | -- |
| `test` | `alr test`: the crate's test actions, else a release build | -- |
| `update` | `alr update` | -- |
| `clean` | `alr clean` | `gprclean -P <gpr>` |
| `prove` | `alr exec -P1 -- gnatprove --level=N -j0` | `gnatprove -P <gpr> --level=N -j0` |

`alr` always runs with `-n`, so it never waits on a question. `prove`
is there when the project uses SPARK: `SPARK_Mode` in a source, or
GNATprove among the crate's dependencies. `N` is `ada.proveLevel`. The
project's `.env` goes to each task.

## Proof

`SPC m p` (`ada.prove`) runs the proof as the `prove` task does, with
one message a line (`--output=oneline`) and past an error (`-k`), and
shows what GNATprove found as problems, on the files they're in: a check
not proved at `high` as an error, `medium` as a warning, `low` as
information; flow analysis's and the compiler's errors and warnings as
they are. A proof takes from seconds to many minutes, so it runs only
when asked, not on save; its problems stay until the next run.

## The doctor

| Check | | Fix row |
|---|---|---|
| Alire | `alr --version`; needed in a crate | Alire 2.1.1 (Linux and macOS, x86_64 and arm64; Windows x86_64) |
| GNAT (Alire's) | in a crate, `alr version`'s selected GNAT and gprbuild; needed | `alr toolchain --select gnat_native gprbuild` |
| GNAT | elsewhere, `gnat --version`; needed | `alr install gnat_native gprbuild`, with Alire |
| gprbuild | with a project file, `gprbuild --version`; needed | the same |
| Ada Language Server | `ada_language_server --version` | 2027.0.202610121 (Linux and macOS, x86_64 and arm64; Windows x86_64) |
| GNATprove | with SPARK: `gnatprove --version`, or a crate's through `alr exec` | `alr install gnatprove`, with Alire |
| Project file | several `.gpr` and none chosen, or the one chosen not there | -- |

Alire's and the server's releases come from GitHub, each checked against
the SHA-256 the pack knows, unpacked into the pack's tools folder, and
used at once. The server's is a 110 MB download that unpacks to about
460 MB, most of it the debug symbols AdaCore ships beside it.
`alr install` puts GNAT, gprbuild and GNATprove in
Alire's install prefix, `~/.alire/bin`, where the pack looks. The pack
never runs `alr with gnatprove`, which changes the crate's manifest: to
make GNATprove a dependency of the crate, run it yourself.

Programs you installed yourself are used instead: the pack looks on
`PATH`, in `~/.alire/bin`, and where GNAT's installers put GNAT
(`/usr/gnat/bin`, `/opt/gnat/bin`, GNAT Community 2021's
`/opt/GNAT/2021/bin` and `C:\GNAT\2021\bin`) and Homebrew does. Alire's
own toolchains (`~/.local/share/alire/toolchains`) are named by a hash,
so they're used through `alr`, not looked for.

## Snippets

| Key | |
|---|---|
| `pkg`, `pkgb`, `spark` | a package spec, its body; a spec `with SPARK_Mode` |
| `main` | a main procedure, with `Ada.Text_IO` |
| `proc`, `func`, `exprf` | a procedure, a function, an expression function |
| `rec`, `tagged` | a record type; a tagged type and a primitive |
| `gen` | a generic package |
| `task`, `prot` | a task type and its body; a protected object and its body |
| `case`, `for`, `exc` | `case` with `when others`; `for` over `'Range`; a block with a handler |
| `contract`, `inv` | a procedure with `Global`, `Pre` and `Post`; `pragma Loop_Invariant` |
| `aunit` | an AUnit test case: its spec and body |

Each is indented by three spaces, as GNAT's style check (`-gnaty3`)
wants.

## Settings

| Setting | Default | |
|---|---|---|
| `ada.projectFile` | empty | The project file the server, the tasks and GNATprove use, under the project; the project's own setting |
| `ada.proveLevel` | 2 | `gnatprove --level`, 0 (quickest) to 4 |

## Keys

After `SPC m` in an Ada project or file: `d` checks the project again,
`p` proves.

## Not yet

- **Running a GNAT project's mains**: without Alire, there's no task
  that runs a built executable; `alr run` does in a crate.
- **Debugging**: GNAT's programs debug with GDB; the pack doesn't set an
  adapter or launches yet.
- **Cross toolchains**: the doctor checks the native GNAT; a crate that
  depends on `gnat_arm_elf` or another builds with it through `alr`, but
  isn't checked for it.
