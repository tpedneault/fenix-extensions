# Samples

What the tests read, and which of it is real.

**Recorded from the real programs** (linux x86_64, the release binaries the
pack installs):

- `texlab-version.txt`, `tex-fmt-version.txt`: `texlab --version` (5.26.0) and
  `tex-fmt --version` (0.5.7).

**Written by hand, from the shape of real TeX output.** No TeX distribution
is on the machine they were made on (TeX Live is several gigabytes), and
Tectonic, which installs and runs, couldn't fetch its bundle from the
network available there, so no build could be recorded. The logs follow
what pdfTeX and XeTeX write -- the banner, `(./main.tex` and `)` around each
file, the `! message.` and `l.12 \foo` of an error and `./main.tex:12:
message.` with `-file-line-error`, `LaTeX Warning:` and `Package x Warning:`
with their `(x)` continuation lines, boxes with their dumps, lines broken at
79 characters -- and the parser is held to what they say, not to a real build:

- `pdflatex-errors.log`: pdfTeX with `-file-line-error`: an undefined
  control sequence, a missing `$` and an unknown environment in a chapter, a
  package that isn't installed.
- `pdflatex-warnings.log`: undefined references and citations (one broken at
  79 characters), a package's warning on two lines, overfull and underfull
  boxes, a font warning, a float specifier, and the "rerun" notice.
- `plain-errors.log`: XeTeX without `-file-line-error`: the place is the `l.`
  line; a missing character; a package's own error, and `Missing \begin{document}`.
- `tectonic-console.txt`: Tectonic's console, `warning: main.tex:7: ...` and
  `error: ...`, as far as the author remembers its shape. This one is the
  least certain: the pack reads the `.log` Tectonic keeps (`--keep-logs`) and
  falls back to this only when it isn't there.
- `synctex-view.txt`: what `synctex view` prints, from SyncTeX's documented
  output. The chktex lines in `src/chktex.rs` are from the format string the
  pack gives chktex (`-f`), likewise.

When a recorded log differs from these, the parser is what changes, and the
fixture is replaced by the recording.
