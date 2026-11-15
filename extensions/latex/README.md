# LaTeX

LaTeX and BibTeX in Fenix: their grammars (Fenix has none of its own), texlab
as the language server, tex-fmt (or latexindent) as the formatter, chktex on
save, a build with whatever you have -- latexmk, Tectonic, or an engine -- whose
log becomes problems, the PDF it makes opened in the PDF reader, a forward
search to the PDF's page, the project doctor's checks with fixes that install
texlab, tex-fmt and Tectonic, snippets, and an article, a Beamer presentation
and a thesis for the new-project wizard. Built on lang-kit
(`crates/lang-kit`), as each language pack is.

It starts with Fenix. The server and the formatter are set whenever the
programs are found; the tasks and the checks are for a project with `.tex`
files. It needs Fenix 1.5 or later (extension API 1.5). It installs no TeX
distribution (TeX Live, MiKTeX and MacTeX are several gigabytes, and yours to
choose); with none, the doctor's fix installs Tectonic. The PDF is shown by
the PDF reader (`fenix.pdf`), a separate extension: without it the pack still
builds, and says what to install.

## The grammars

`*.tex`, `*.sty` and `*.cls` files are read with tree-sitter-latex 0.6.0,
`*.bib` files with tree-sitter-bibtex (`grammars/`, MIT, the commits the
README there names). Highlighted (commands, sectioning, environments, labels,
references and citations, packages and files, math, verbatim), folded
(sections down to the paragraph, environments), with Enter indenting inside
an environment or a group, and the breadcrumbs and `SPC c s` listing the
sections and a database's entries. `%` comments a line. tree-sitter-latex
ships no queries and the pack's own are in `queries/latex/`.

## The server

texlab, latex-lsp's: completion for commands, labels, citations and files,
hover, go to definition and references, rename, the outline, and its own
diagnostics. It's started as `texlab` for `.tex`, `.sty`, `.cls` and `.bib`
files. The pack builds, not texlab (texlab's own build is left at its default,
off); chktex is the pack's, not texlab's.

## Building

`SPC m b` (`latex.build`) builds the document with the first of these that
applies:

1. a `Tectonic.toml` at the top, and Tectonic: `tectonic -X build --keep-logs`;
2. a `.latexmkrc` (or `latexmkrc`), and latexmk: it decides the engine;
3. a magic comment in the document, `% !TEX program = xelatex` (or
   `pdflatex`, `lualatex`; TeXstudio's `% !TeX TS-program =` too): latexmk with
   that engine, else the engine by itself, when it's found;
4. latexmk, with pdflatex: `latexmk -pdf -interaction=nonstopmode
   -file-line-error -synctex=1 main.tex`;
5. Tectonic: `tectonic -X compile --keep-logs --synctex main.tex`;
6. an engine by itself, pdflatex then xelatex then lualatex. It runs **once**:
   it doesn't run BibTeX or again for the references, so the first build says
   "undefined references" until you build again. latexmk or Tectonic do that
   for you.

Tectonic runs BibTeX itself but not biber (BibLaTeX with `backend=biber`
needs a TeX distribution and latexmk). It downloads the packages a document
uses the first time it builds, from the internet, and keeps them.

**The document** is, in this order: the root the file you're in names
(`% !TEX root = ../main.tex`), or that file if it has a `\documentclass`; the
`latex.main` setting; `main.tex` at the top; the one `.tex` file at the top
with a `\documentclass`. The PDF is beside it. A document in a folder is
built there (`latexmk -cd`, `-output-directory=`).

**The log is read, not what the builder printed.** After a build the pack reads
the `.log` the engine wrote (`main.log`; Tectonic keeps one with
`--keep-logs`), and shows in the problems:

- **errors**, in the file and line TeX says (`l.12`, or the file and line
  `-file-line-error` prints); an undefined control sequence says which;
- **warnings**: undefined references and citations, every other `LaTeX
  Warning` and package or class warning, a font warning, a missing character,
  and **overfull boxes** with the lines they cover;
- **underfull boxes** as info.

Which file a message is about comes from the parentheses TeX opens a file
with in the log, the way every tool that reads it does -- a heuristic, and the
README's last section says what it can't do. An error in a package or the class
is put on the project file that read it, at its top, with the file's name in the
message. The summary "There were undefined references" isn't repeated. Without
a log, what the builder printed is read the same way.

- `SPC m w` (`latex.watch`): builds once, then runs `latexmk -pvc -view=none`
  as a background task, which builds again whenever a file changes, and opens
  the PDF. Again stops it. Only latexmk watches; with anything else the
  command says so. While it runs, `latex.build_on_save` does nothing.
- `latex.build_on_save`: saving a `.tex`, `.sty`, `.cls` or `.bib` file builds
  the document.
- `SPC m c` (`latex.clean`) is `latexmk -c`, the auxiliary files; `latex.clean_all`
  is `-C`, the PDF too. Only latexmk knows what it left: with another builder
  the command says so (Tectonic leaves none beside the document).
- `SPC m n` (`latex.word_count`): `texcount -brief -inc -utf8` on the document,
  when texcount is there (it comes with TeX Live and MiKTeX).

## The preview

The first successful build opens the PDF in the PDF reader, through the
reader's `pdf.open` service, in the pane you're in; the reader reloads a PDF
when it's written again, keeping its place, so every build after that shows
in it and the pack doesn't ask again. `SPC m p` (`latex.preview`) opens it
again, building first if there's no PDF; `latex.preview` off keeps the
automatic opening off.

Without the reader (not installed, or one with no `pdf.open`) the pack says so
in a notification -- where the PDF is, and that `fenix.pdf` from the Extensions
page shows it -- and each build tries again. Fenix has no call for the system's
viewer (`open_url` takes http(s) only), so the pack doesn't offer one.

**Forward search**: `SPC m s` (`latex.forward_search`) asks `synctex view` which
page the cursor's line is on in the PDF and opens the reader on that page. It
needs the `synctex` program (it comes with TeX Live, MiKTeX and MacTeX;
Tectonic doesn't ship it) and a PDF built with SyncTeX data (every build the
pack makes asks for it). The reader's `pdf.open` takes a page and no place on
it, so it's the page, not the line: the PDF doesn't scroll to or mark the
text. A reader that took a position (the `x` and `y` SyncTeX prints, which the
pack reads) would get it. There's no reverse search (a click in the PDF to the
source).

## Formatting and checks

- `SPC c F`, and saving with `editor.format_on_save`: **tex-fmt**, a single
  program with no Perl, installed by the doctor, reading the file on its input
  (`tex-fmt --stdin`); it indents environments and wraps long lines at 80 (a
  `tex-fmt.toml` in the folder it runs in changes that, as in a terminal:
  checked with 0.5.7; Fenix's folder for a formatter isn't something this
  pack could check). That's why it's
  the default. **latexindent** (Perl, and in practice a TeX distribution) is
  used when tex-fmt isn't found, or when `latex.formatter` says so
  (`latexindent -g /dev/null -`, `nul` on Windows, so it should leave no
  `indent.log`: not run here, there's no Perl latexindent on the machine this
  was written on).
  `.bib` files aren't formatted.
- On save: **chktex** on the saved `.tex`, `.sty` or `.cls` file when it's
  found (`latex.chktex`), its warnings shown with texlab's, with its message
  number as the code.

## Tasks

`SPC p t`, each in the project, for the document:

| Task | Runs |
|---|---|
| `build` | what builds it (above) |
| `clean` | `latexmk -c main.tex`: latexmk only |
| `clean (all)` | `latexmk -C main.tex`: the PDF too; latexmk only |
| `word count` | `texcount -brief -inc -utf8 main.tex`: when texcount is found |

Every task is started by you, from the picker or `SPC p T` (rerun the last).
A task of the same name in `.fenix/tools.json` wins. Building continuously is
a command, not a task: a task holds the task pane while it runs.

## The doctor

| Check | | Fix row |
|---|---|---|
| LaTeX build | what will build the document, or that nothing does: needed in a project with `.tex` files, a warning in any other | **Install Tectonic** (it downloads packages on first build) |
| texlab | `texlab --version`; optional | texlab 5.26.0 |
| tex-fmt | `tex-fmt --version`; optional | tex-fmt 0.5.7 |
| chktex, texcount, synctex | their versions; optional | none: a TeX distribution's |

The fix rows (Linux, macOS and Windows; x86_64 and arm64, Tectonic not on
Windows ARM, which has no release) download the release from GitHub for this
machine (`github.com`, and where GitHub sends the file), check it against the
SHA-256 the pack knows, unpack it into the pack's tools folder, and use it at
once. Starting is said in the message line; how it ended is a notification:
"Installed Tectonic 0.17.0", or "Couldn't install Tectonic 0.17.0" with which
step failed and why, and *Try again*. The Tectonic fix's text says what the
first build does: it downloads the packages a document uses from the internet
(Tectonic's bundle), and keeps them. One you installed yourself is used
instead: the pack looks on `PATH`, and in `~/.local/bin`, `~/.cargo/bin`,
Homebrew's, Snap's, MacTeX's `/Library/TeX/texbin`, MiKTeX's, Scoop's and
winget's.

**Where the checksums come from.** None of the three projects publishes a
checksum file with a release (`.sha256`, `SHA256SUMS` and the like are not
among the assets). Each SHA-256 in `src/tools.rs` was computed from the asset
downloaded from the release's own URL, for the tags `v5.26.0` (texlab),
`tectonic@0.17.0` and `v0.5.7` (tex-fmt); it pins those exact bytes, so Fenix
refuses a file that isn't them. It doesn't prove the release is what the author
meant: that is as far as the upstream goes.

## Snippets

LaTeX (`.tex`, `.sty`, `.cls`):

| Key | |
|---|---|
| `doc` | a document: class, packages, title, `document` |
| `sec`, `ssec`, `sssec`, `chap` | a section, subsection, subsubsection, chapter, with a label |
| `env`, `ite`, `enu`, `desc` | an environment (its name mirrored on `\end`); itemize, enumerate, description |
| `fig`, `tab` | a figure with an image, caption and label; a table with `booktabs` rules |
| `eq`, `ali`, `mat`, `frac`, `im` | an equation with a label; `align`; a matrix; a fraction; inline math |
| `thm`, `pf`, `nthm`, `abs` | a theorem; a proof; `\newtheorem`; an abstract |
| `frame`, `fitem`, `cols` | a Beamer frame; one with a list; two columns |
| `pkg`, `pkgo`, `cmd` | `\usepackage`, with options; `\newcommand` |
| `lst`, `verb`, `tikz` | a listing; verbatim; a TikZ picture |
| `cite`, `ref`, `eqref`, `href`, `foot` | a citation; a reference; an equation reference; a link; a footnote |
| `bib`, `bbl`, `toc`, `inp`, `incl`, `root` | the BibTeX bibliography; BibLaTeX's; the contents; `\input`; `\include`; the `% !TEX root` comment |

BibTeX (`.bib`): `art`, `book`, `inp` (conference paper), `thesis`, `online`,
`misc` and `str` (`@string`).

## New projects

*LaTeX · article* in the new-project wizard: a title block, an abstract,
sections, a figure, a table and an equation with labels you can reference, a
BibTeX bibliography (`refs.bib`), a `.latexmkrc` (which names the engine, and
makes it a LaTeX project for Fenix 1.6) and a `.gitignore` for what a build
leaves. *LaTeX · Beamer presentation*: a title frame, an outline, frames, two
columns and a block. *LaTeX · thesis*: a title page, an abstract, contents and
lists of figures and tables, chapters in files of their own (each starts with
`% !TEX root = ../main.tex`, so `SPC m b` in a chapter builds the thesis), an
appendix, and the bibliography. None runs anything when it's made. They were
formatted with tex-fmt 0.5.7 and checked against its output; **none was
compiled**: there's no TeX distribution where they were written, and Tectonic
couldn't fetch its bundle. They use standard packages only (`amsmath`,
`graphicx`, `booktabs`, `hyperref`, `setspace`, and Beamer's own).

## Settings

| Setting | Default | |
|---|---|---|
| `latex.main` | empty (found) | The document, in the project, when it isn't `main.tex`; a project's own (`.fenix/settings.toml`) |
| `latex.preview` | on | The first build that works opens the PDF in the reader |
| `latex.build_on_save` | off | Saving a `.tex`, `.sty`, `.cls` or `.bib` file builds the document |
| `latex.chktex` | on | chktex on a saved file |
| `latex.formatter` | auto | `auto` (tex-fmt, else latexindent), `tex-fmt` or `latexindent` |

## Project kind

Fenix 1.6 and later know a LaTeX project (`TEX`): its tag in front of its name in the project hub, on Home, in the modeline and in the doctor, found by a `.latexmkrc`, `latexmkrc` or `Tectonic.toml` at its top, and opened on `main.tex` when you've had none of its files open. `kind = "latex"` in `.fenix/settings.toml` says it by hand. Fenix's own kinds come first: a project Fenix already takes for Rust, Python, Go, JavaScript, C/C++, Tcl or Arduino stays that one until `kind = "latex"` says otherwise, so this kind names projects Fenix would otherwise call plain folders. Your home folder is never a project, whatever dotfiles it holds. A folder with only `.tex` files and none of those markers stays a plain folder (wildcards aren't allowed in a kind's markers): the pack still loads for its files and its commands work; add a `.latexmkrc` (the templates have one) to make it a project. Earlier Fenix ignores this and the pack works there as before. The kind is declared in `extension.toml` (`[[contributes.project_kinds]]`); the pack's context markers say when the pack applies, and are not the same list.

## Keys

After `SPC m` in a LaTeX project or file (`C-k m` with standard keys):

| Key | Command | |
|---|---|---|
| `b` | `latex.build` | build the document and show its PDF |
| `w` | `latex.watch` | build continuously (again to stop) |
| `p` | `latex.preview` | open the document's PDF |
| `s` | `latex.forward_search` | this line's page in the PDF (synctex) |
| `c` | `latex.clean` | clean the auxiliary files |
| `n` | `latex.word_count` | count the words |
| `d` | `latex.doctor` | check the project again |

## What it may do

- **Run** `latexmk`, `tectonic`, `pdflatex`, `xelatex` and `lualatex`
  (`permissions.process`) to build, as commands and tasks you start; `texlab`
  as the server; `tex-fmt` or `latexindent` as the formatter; `chktex` on
  save; `texcount` and `synctex` for the word count and the forward search;
  and each tool's `--version` for the doctor. `/tools/*` is where the pack
  installs texlab, Tectonic and tex-fmt. No other program.
- **Reach `github.com`, `objects.githubusercontent.com` and
  `release-assets.githubusercontent.com`** (`permissions.network`): to
  download a release when you run a fix row. What a builder reaches for
  packages (Tectonic's bundle, a TeX distribution's `tlmgr`) is the builder's,
  as in a terminal, not the pack's.
- **Read the project** (`filesystem = "project-read"`): to find the document
  (`main.tex`, `.latexmkrc`, `Tectonic.toml`, the `.tex` files at the top and
  the first lines of the document for magic comments), and to read the log and
  see the PDF a build made. It writes nothing: the build's files are the
  builder's.
- **Know where you are** (`editor = "read"`): the project's folder as the
  machine has it, which the PDF reader needs for a full path; the file you're
  in, which names its document; and its line, for the forward search.
- It calls the PDF reader's `pdf.open` service. That is not a permission:
  an extension may ask another's service, and the pack depends on none.

## Not yet

- **A position in the PDF**, and a reverse search (a click in the PDF to the
  source): the reader's `pdf.open` takes a page or a heading, not a place on the
  page, and the pack has no call from the reader back. That's the reader's to add.
- **Latexmk's `-pvc` messages as problems**, the build's log when it's
  watching, and biber's and BibTeX's own logs (`.blg`).
- **A log it can't follow**: the file a message is about is found from the
  parentheses TeX opens files with, so a log whose lines are cut differently
  (an engine with another `max_print_line`), or a file name with spaces, is
  read less well; a message the pack can't place is on the document, at its top.
- **`.latexmkrc`'s own `$out_dir`/`$aux_dir`**: the pack reads the log beside
  the PDF; a latexmkrc that moves them leaves it reading what the builder
  printed.
- A thesis class's own template, `biber`, glossaries, and `texlab`'s own
  build and forward-search settings.

## Building

```text
cargo xtask pack extensions/latex dist
cargo xtask install extensions/latex
cargo test --locked --manifest-path extensions/latex/Cargo.toml
```

`cargo test` runs the pack on the test kit's fake host: its server, formatter,
tasks and checks are set; a build's log is read into problems where they are
and cleared when fixed; the builder is chosen from what's there, a magic
comment, a `.latexmkrc` and a `Tectonic.toml`; the PDF reader's `pdf.open` is
called once with a full path and not again, and is told about when the service
is there and when nothing offers it; the forward search asks synctex and opens
the page it says; the continuous build starts and stops; and the fix rows'
downloads name the release and its SHA-256 (`src/lib.rs`). Each piece on its
own: the install table for each platform (`src/tools.rs`), the builder's
command lines (`build.rs`), the log parser (`log.rs`), chktex's lines
(`chktex.rs`), synctex's output (`synctex.rs`), the doctor, the document
(`project.rs`), the preview, the tasks. And the data as Fenix reads it
(`src/data.rs`): the language files, the grammars loaded and their queries run
on a sample, the snippets, the three templates planned, and the manifest
against the code. What is real and what is written from the shape of real
output in `src/samples/` is in `src/samples/README.md`: the `--version`s are
recorded from texlab 5.26.0 and tex-fmt 0.5.7; **the build logs, chktex's lines and
synctex's output are not recordings**.
