# {{name}}

An article in LaTeX. `main.tex` is the document; the bibliography is
`refs.bib` (BibTeX).

```text
latexmk -pdf main.tex      # main.pdf, run as many times as it takes
tectonic main.tex          # the same with Tectonic
```

In Fenix: `SPC m b` builds and shows the PDF (its log is read into problems),
`SPC m w` builds continuously (latexmk).
