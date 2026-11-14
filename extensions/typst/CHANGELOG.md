# Changelog

## 1.0.0

- The first release, for Fenix 1.5 (extension API 1.5): Typst. Its grammar
  (`.typ` files: highlighted, folded, indented, with its sections as the
  scopes `SPC c s` and the breadcrumbs list), tinymist as the language
  server, typstyle as the formatter, and compile and watch: `SPC m c` runs
  `typst compile` and shows its errors as problems, in the file they're in;
  `SPC m w` compiles and then watches in the background; the PDF opens in the
  PDF reader (the `fenix.pdf` extension) the first time and the reader
  reloads it after each compile, and without the reader the pack says what to
  install. Tasks (`compile`, `compile (png)`, `fonts`, `format -check`), and
  the project doctor checks Typst, tinymist and typstyle, each with a fix that
  installs it from its GitHub release, checked against its SHA-256. Snippets,
  a *Typst · article* and a *Typst · package* for the new-project wizard, and
  a lesson in the tutorial.
- Fenix 1.6 and later show a project with a `typst.toml` at its top as a Typst
  project (`TYP`), in the project hub, on Home, in the modeline and in the
  doctor, and open it on `main.typ` when you've had none of its files open;
  `kind = "typst"` in a project's `.fenix/settings.toml` says it by hand.
  Fenix's own kinds come first: a project Fenix already takes for Rust,
  Python, Go, JavaScript, C/C++, Tcl or Arduino stays that one until `kind =
  "typst"` says otherwise. Fenix 1.5 ignores this and the pack works there as
  before: it's only data in the manifest.
