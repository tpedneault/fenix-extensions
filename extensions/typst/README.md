# Typst

Typst in Fenix: its grammar (Fenix has none of its own), tinymist as the
language server, typstyle as the formatter, `typst compile` and `typst
watch` with their errors as problems, the PDF it makes opened in the PDF
reader, the project doctor's checks with fixes that install each tool,
snippets, and an article and a package for the new-project wizard. Built on
lang-kit (`crates/lang-kit`), as each language pack is.

It starts with Fenix. The server and the formatter are set whenever the
programs are found; the tasks and the checks are for a project with a
`typst.toml` or `.typ` files. It needs Fenix 1.5 or later (extension API
1.5). The PDF is shown by the PDF reader (`fenix.pdf`), a separate extension:
without it the pack still compiles, and says what to install.

## The grammar

`*.typ` files are read with uben0's tree-sitter-typst (`grammars/`, MIT, at
the commit the README there names). Highlighted (headings, `*strong*`,
`_emphasis_`, raw blocks, links, labels and references, `#let`, `#set`,
`#show`, `#import`, calls, strings, numbers, math), folded (a heading's
section, blocks, content, raw blocks), with Enter indenting inside a block
or a call, and the breadcrumbs and `SPC c s` listing the sections. `//`
comments a line. The grammar's repository ships queries in another editor's
names; the pack's own are in `queries/typst/`.

## The server

tinymist, Myriad-Dreamin's: completion, hover, go to definition and
references, rename, outline, diagnostics, and Typst's own packages. It's
started with `tinymist lsp` for `.typ` files. It carries its own Typst, so
it needs no `typst` on the machine; its diagnostics come from it as you
type.

## Formatting

`SPC c F`, and saving with `editor.format_on_save`: `typstyle`, the standalone
formatter, reading the file on its input. tinymist can format too (it
bundles typstyle), but only through the language server, and Fenix's save
runs the formatter a pack gives as a command: so the standalone typstyle is
the one path, and its doctor row has a fix.

## Compile and preview

- `SPC m c` (`typst.compile`): `typst compile --diagnostic-format short
  --root . main.typ main.pdf`, in the project. What it prints is read into
  problems, in whichever file they are (an error in a chapter the document
  imports is the chapter's), and a compile that works clears them. A line with
  no place (a document that isn't there) is the document's, at its top. The
  message line says how it went.
- **The document** is, in this order: the `typst.main` setting; `main.typ`;
  `src/main.typ`; a `typst.toml`'s `entrypoint`; the one `.typ` file at the
  top when there's only one. With none, the `.typ` file you're in is compiled.
  The PDF is beside it, `main.pdf`.
- **The preview**: the first compile that works opens the PDF in the PDF
  reader, through the reader's `pdf.open` service, in the pane you're in; the
  reader reloads a PDF when it's written again, keeping its place, so every
  compile after that shows in it and the pack doesn't ask again. `SPC m p`
  (`typst.preview`) opens it again (it compiles first if there's no PDF), and
  `typst.preview` off keeps the automatic opening off.
- Without the reader (not installed, or one with no `pdf.open`) the pack says
  so in a notification -- where the PDF is, and that `fenix.pdf` from the
  Extensions page shows it -- and each compile tries again, in case you
  installed it. Fenix has no call for the system's viewer (`open_url` takes
  http(s) only), so the pack doesn't offer one.
- `SPC m w` (`typst.watch`): compiles once, then runs `typst watch` as a
  background task, which compiles again whenever a file it read changes, and
  opens the PDF. Pressing it again stops it. Its output is in the task's
  buffer; its errors aren't read into problems (tinymist's are there as you
  type, and `SPC m c` reads a compile's). While it runs, `typst.compile_on_save`
  does nothing: it's the one compiling.
- `typst.compile_on_save`: saving a `.typ` file compiles the document, with
  its problems and the PDF reloading.

Typst's short format counts a line from 1 and a column **from 0**, in
characters; the pack's parser knows (`src/compile.rs`, tested on recorded
output). Hints aren't in that format, so the problems have the message only.

## Tasks

`SPC p t`, each in the project:

| Task | Runs |
|---|---|
| `compile` | `typst compile --diagnostic-format short --root . main.typ main.pdf` |
| `compile (png)` | the same, to `main-{p}.png`: a picture a page |
| `fonts` | `typst fonts`: the fonts Typst finds |
| `format -check` | `typstyle --check .`: the files that aren't formatted |

Every task is started by you, from the picker or `SPC p T` (rerun the last).
A task of the same name in `.fenix/tools.json` wins. `watch` is a command, not
a task: a task holds the task pane while it runs.

## The doctor

| Check | | Fix row |
|---|---|---|
| Typst | `typst --version`, 0.11 or later; needed in a project with Typst files, a warning in any other | Typst 0.15.1 |
| tinymist | `tinymist --version`: the Typst it bundles (it prints no version of its own); optional | tinymist 0.15.12 |
| typstyle | `typstyle --version`; optional | typstyle 0.15.2 |

Each fix row (Linux, macOS and Windows; x86_64 and arm64) downloads the
release from GitHub for this machine (`github.com`, and where GitHub sends
the file), checks it against the SHA-256 the pack knows, unpacks it into the
pack's tools folder, and uses it at once. Starting is said in the message
line; how it ended is a notification: "Installed tinymist 0.15.12", or
"Couldn't install tinymist 0.15.12" with which step failed and why, and
*Try again*. One you installed yourself is used instead: the pack looks on
`PATH`, and in `~/.local/bin`, `~/.cargo/bin`, Homebrew's, Snap's, Scoop's
and winget's.

**Where the checksums come from.** None of the three projects publishes a
checksum file with a release (`.sha256`, `SHA256SUMS` and the like are not
among the assets). Each SHA-256 in `src/tools.rs` was computed from the asset
downloaded from the release's own URL, for the tags `v0.15.12` (tinymist),
`v0.15.1` (Typst) and `v0.15.2` (typstyle); it pins those exact bytes, so Fenix
refuses a file that isn't them. It doesn't prove the release is what the
author meant: that is as far as the upstream goes.

## Snippets

| Key | |
|---|---|
| `doc` | `#set` rules for a document: title, author, page, text, paragraphs, heading numbers |
| `h1`, `h2`, `h3` | a heading |
| `fig`, `tab` | a figure with an image; a table in a figure, each with a label to reference |
| `eq` | an equation with a label |
| `let`, `fn`, `show`, `set` | a binding, a function, a show rule, a set rule |
| `tmpl` | a template function, applied with `#show: ...with(...)` |
| `imp`, `pkg`, `incl` | an import; a package from `@preview`; an include |
| `grid`, `cols`, `raw` | a grid; columns; a code block |
| `bib`, `cite`, `ref` | the bibliography; a citation; a reference |
| `for`, `if`, `ctx` | a loop; a conditional; `context` |
| `link`, `toc`, `lorem` | a link; the outline; placeholder text |

## New projects

*Typst · article* in the new-project wizard: a title block, an abstract,
numbered headings, a figure, an equation and a table with labels you can
reference, and a bibliography (`main.typ`, `refs.bib`, a `.gitignore` that
keeps the PDF out of Git). *Typst · package*: a `typst.toml` (name, version,
entrypoint, authors, licence), a `lib.typ` with a documented function, a
`main.typ` that imports it as an example, and a README. Both compile as they
are (checked with Typst 0.15.1). Neither runs anything when it's made.

## Settings

| Setting | Default | |
|---|---|---|
| `typst.main` | empty (found) | The document, in the project, when it isn't `main.typ`; a project's own (`.fenix/settings.toml`) |
| `typst.preview` | on | The first compile that works opens the PDF in the reader |
| `typst.compile_on_save` | off | Saving a `.typ` file compiles the document |

## Project kind

Fenix 1.6 and later know a Typst project (`TYP`): its tag in front of its name in the project hub, on Home, in the modeline and in the doctor, found by a `typst.toml` at its top, and opened on `main.typ` when you've had none of its files open. `kind = "typst"` in `.fenix/settings.toml` says it by hand. Fenix's own kinds come first: a project Fenix already takes for Rust, Python, Go, JavaScript, C/C++, Tcl or Arduino stays that one until `kind = "typst"` says otherwise, so this kind names projects Fenix would otherwise call plain folders. Your home folder is never a project, whatever dotfiles it holds. A folder of `.typ` files and no `typst.toml` stays a plain folder (wildcards aren't allowed in a kind's markers): the pack still loads for its files, and the compile commands work. Earlier Fenix ignores this and the pack works there as before. The kind is declared in `extension.toml` (`[[contributes.project_kinds]]`); the pack's context markers say when the pack applies, and are not the same list.

## Keys

After `SPC m` in a Typst project or file (`C-k m` with standard keys):

| Key | Command | |
|---|---|---|
| `c` | `typst.compile` | compile the document and show its PDF |
| `w` | `typst.watch` | watch the document (again to stop) |
| `p` | `typst.preview` | open the document's PDF |
| `d` | `typst.doctor` | check the project again |

## What it may do

- **Run `typst`, `tinymist` and `typstyle`** (`permissions.process`): to ask
  their versions for the doctor; `typst compile` and `typst watch`, as
  commands and tasks you start; typstyle as the formatter and `format -check`;
  tinymist as the server. `/tools/*` is where the pack installs them. No other
  program.
- **Reach `github.com`, `objects.githubusercontent.com` and
  `release-assets.githubusercontent.com`** (`permissions.network`): to
  download a release when you run a fix row. Typst itself reaches the package
  registry for an `@preview` import, as it does in a terminal; that's Typst's,
  not the pack's.
- **Read the project** (`filesystem = "project-read"`): to find the document
  (`main.typ`, `typst.toml`, the `.typ` files at the top) and to see whether the
  PDF is there. It writes nothing: the PDF is Typst's.
- **Know where you are** (`editor = "read"`): the project's folder as the
  machine has it, which the PDF reader needs for a full path, and the `.typ`
  file you're in when the project names no document.
- It calls the PDF reader's `pdf.open` service. That is not a permission:
  an extension may ask another's service, and the pack depends on none.

## Not yet

- **A position in the PDF.** The reader's `pdf.open` takes a page or a
  heading, not a place on the page, so the PDF can't scroll to where the
  cursor is (tinymist's own preview does, in a browser). That's the reader's to
  add.
- **`typst watch`'s errors as problems**, and a *typst query*, *typst init*
  from templates in the universe (`typst init @preview/...` runs fine in a
  terminal).
- **tinymist's browser preview** (`tinymist preview`), exports other than PDF
  and PNG, and Typst's debug adapter.

## Building

```text
cargo xtask pack extensions/typst dist
cargo xtask install extensions/typst
cargo test --locked --manifest-path extensions/typst/Cargo.toml
```

`cargo test` runs the pack on the test kit's fake host: its server, formatter,
tasks and checks are set; a compile's problems are set where they are and
cleared when fixed; the PDF reader's `pdf.open` is called once with a full path
(the fake service answers) and not again, and is told about when the service is
there and when nothing offers it; `watch` starts and stops a background task;
and the fix rows' downloads name the release and its SHA-256 (`src/lib.rs`).
Each piece on its own: the install table for each platform (`src/tools.rs`),
the doctor (`doctor.rs`), the document (`project.rs`), the diagnostics parser
(`compile.rs`), the preview (`preview.rs`), the tasks. And the data as Fenix
reads it (`src/data.rs`): the language file, the grammar loaded and its
queries run on a sample, the snippets, the two templates planned, and the
manifest against the code. What `typst compile` and the `--version`s printed in
`src/samples/` was recorded from the real binaries (see
`src/samples/README.md`).
