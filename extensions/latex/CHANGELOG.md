# Changelog

## 1.0.0

- The first release, for Fenix 1.5 (extension API 1.5): LaTeX and BibTeX. Their
  grammars (`.tex`, `.sty`, `.cls` and `.bib` files: highlighted, folded,
  indented, with their sections and entries as the scopes `SPC c s` and the
  breadcrumbs list), texlab as the language server, tex-fmt (or latexindent) as
  the formatter, chktex on save, and a build: `SPC m b` builds the document with
  what you have -- latexmk, Tectonic or an engine, a `.latexmkrc`, a
  `Tectonic.toml` or a `% !TEX program` comment deciding, a `% !TEX root`
  comment naming the document from a chapter -- and reads the log into
  problems: errors, undefined references and citations, overfull boxes. `SPC m w`
  builds continuously with latexmk; `SPC m c` cleans; `SPC m n` counts words
  (texcount). The PDF opens in the PDF reader (the `fenix.pdf` extension) the
  first time and the reader reloads it after each build, and without the reader
  the pack says what to install; `SPC m s` goes to the PDF page of the cursor's
  line with synctex. The project doctor checks what builds, texlab, tex-fmt,
  chktex, texcount and synctex, with fixes that install texlab, tex-fmt and
  Tectonic (which downloads packages on its first build) from their GitHub
  releases, checked against their SHA-256. Snippets, an article, a Beamer
  presentation and a thesis for the new-project wizard, and a lesson in the
  tutorial.
- Fenix 1.6 and later show a project with a `.latexmkrc`, `latexmkrc` or
  `Tectonic.toml` at its top as a LaTeX project (`TEX`), in the project hub, on
  Home, in the modeline and in the doctor, and open it on `main.tex` when you've
  had none of its files open; `kind = "latex"` in a project's
  `.fenix/settings.toml` says it by hand. A folder of `.tex` files with none of
  those stays a plain folder. Fenix's own kinds come first: a project Fenix
  already takes for Rust, Python, Go, JavaScript, C/C++, Tcl or Arduino stays that
  one until `kind = "latex"` says otherwise. Fenix 1.5 ignores this and the pack
  works there as before: it's only data in the manifest.
