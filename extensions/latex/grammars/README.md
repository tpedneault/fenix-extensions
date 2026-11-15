# LaTeX's and BibTeX's grammars

`tree-sitter-latex.wasm` and `tree-sitter-bibtex.wasm` are the Fenix-facing
builds of two of Patrick Förster's grammars, the ones texlab's author
maintains, compiled to WebAssembly by the pack:

| File | Source | SHA-256 | Size |
|---|---|---|---|
| `tree-sitter-latex.wasm` | https://github.com/latex-lsp/tree-sitter-latex at `7e0ecdc02926c7b9b2e0c76003d4fe7b0944f957` (tag `v0.6.0`, 19 September 2025) | `a926e0e97a885a5a95f93271fba29a4437555e38aaa08c194986386b1f49784f` | 3718715 bytes |
| `tree-sitter-bibtex.wasm` | https://github.com/latex-lsp/tree-sitter-bibtex at `8d04ed27b3bc7929f14b7df9236797dab9f3fa66` (19 April 2025; it has no release tags) | `d9727f17eaf6b8ea7625a667b46bc1baab5ea38cb99ad6232ed3564be3b0223a` | 14786 bytes |

tree-sitter-bibtex commits `src/parser.c`; tree-sitter-latex doesn't (it
commits `grammar.js` and `src/scanner.c` only), so its parser is generated
first. Both can be made again from the commits:

```sh
git clone https://github.com/latex-lsp/tree-sitter-bibtex && cd tree-sitter-bibtex
git checkout 8d04ed27b3bc7929f14b7df9236797dab9f3fa66
npx -y tree-sitter-cli@0.26.13 build --wasm -o tree-sitter-bibtex.wasm .

git clone https://github.com/latex-lsp/tree-sitter-latex && cd tree-sitter-latex
git checkout 7e0ecdc02926c7b9b2e0c76003d4fe7b0944f957
npx -y tree-sitter-cli@0.26.13 generate
npx -y tree-sitter-cli@0.26.13 build --wasm -o tree-sitter-latex.wasm .
```

With tree-sitter's CLI 0.26.13, which fetched wasi-sdk itself. Their language
ABI is 14, which Fenix's tree-sitter 0.26 loads (13 to 15); `src/data.rs`
loads them as Fenix does and runs the queries on a sample of each.
tree-sitter-latex is the grammar texlab's own parser was ported from; it
reads LaTeX as far as a grammar can: Turing-complete macros are beyond any
(its README says so), so a document that redefines catcodes or builds
commands out of others may be coloured plainly. `.tex`, `.sty` and `.cls`
files are read with it; `.bib` with the other.

**Licence.** Both are MIT-licensed: `LICENSE-tree-sitter-latex` and
`LICENSE-tree-sitter-bibtex` are the repositories' `LICENSE` files word for
word, "Copyright (c) 2021 Patrick Förster" and "Copyright (c) 2020 Patrick
Förster".

tree-sitter-latex ships no queries, so `queries/latex/` (`highlights.scm`,
`folds.scm`, `indents.scm`) is this pack's own, written for Fenix's capture
names. `queries/bibtex/highlights.scm` is tree-sitter-bibtex's own (MIT) with
Fenix's capture names; its `folds.scm` and `indents.scm` are the pack's.
