# {{name}}

A thesis. `main.tex` is the document, the chapters are in `chapters/`, the
bibliography is `refs.bib` (BibTeX).

```text
latexmk -pdf main.tex
```

In Fenix: `SPC m b` builds `main.tex` from any chapter and shows the PDF;
`SPC m s` goes to the page of the line you're on (with `synctex`).
