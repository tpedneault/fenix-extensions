# Samples

What the tests read:

- `report.typ`: Typst the grammar's queries run on.
- `typst-version.txt`, `tinymist-version.txt`, `typstyle-version.txt`:
  `--version` of Typst 0.15.1, tinymist 0.15.12 (which prints the Typst it
  bundles and no version of its own) and typstyle 0.15.2, recorded from the
  linux x86_64 release binaries the pack installs.
- `compile-*.txt`: what `typst compile --diagnostic-format short` printed
  (to stderr), recorded from Typst 0.15.1 in small projects made for it:
  `error` (an unknown variable, 5:9), `import-error` (the same in a file the
  document imports), `warning` (an error and a warning in one run),
  `two-errors` (two unclosed delimiters), `columns` (a column after two
  two-byte characters: counted in characters, from 0), `file-not-found` (an
  `#import` of a file that isn't there), `no-input` (a document that isn't
  there: a line with no place), and `ok` (nothing at all).
  The one thing changed in them: the folder a file-not-found line names is
  written `/home/ada/book` instead of this machine's scratch folder.

The output is real Typst's, not written by hand.
