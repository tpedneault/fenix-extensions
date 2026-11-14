# Typst's grammar

`tree-sitter-typst.wasm` is uben0's tree-sitter-typst
(https://github.com/uben0/tree-sitter-typst) at commit
`46cf4ded12ee974a70bf8457263b67ad7ee0379d` (10 February 2025, "add static to
local function in scanner, README update"; the last release tag, v0.11.0, is
older), compiled to WebAssembly by the pack: the repository commits
`src/parser.c` and `src/scanner.c`, and they were built unchanged, so what's
here can be made again from the commit:

```sh
git clone https://github.com/uben0/tree-sitter-typst && cd tree-sitter-typst
git checkout 46cf4ded12ee974a70bf8457263b67ad7ee0379d
npx -y tree-sitter-cli@0.26.13 build --wasm -o tree-sitter-typst.wasm .
```

It was built with tree-sitter's CLI 0.26.13, which fetched wasi-sdk itself.
The result's SHA-256 is
`314ec85381cb87aa70f611b9e445fa01644cdc78218faee4476f509fb4be61b3`
(764418 bytes). Its language ABI is 14, which Fenix's tree-sitter 0.26 loads
(13 to 15); `src/data.rs` loads it as Fenix does and runs the queries on a
sample of Typst: headings, emphasis, raw blocks, links and references, labels,
code (`#let`, `#set`, `#show`, `#import`, `#for`, `#if`, `#context`),
calls, and math.

It's the grammar Helix and nvim-treesitter use for Typst. It was last
changed in February 2025, so syntax newer than that may be coloured as plain
text or flagged as an error in the editor, never worse: the compiler and
tinymist are the judges of what is valid. One thing it can't read, which
the pack found: an `#import` of a list and the wildcard together
(`#import "a.typ": b, *`, which Typst itself refuses too).

**Licence.** `LICENSE-tree-sitter-typst` is the repository's `LICENSE`, word
for word: the MIT licence, "Copyright (c) 2023 Gerbais-Nief Eddie".

`queries/typst/highlights.scm` starts from the repository's own
(`queries/typst/highlights.scm`, MIT), with Fenix's capture names
(`@text.title`, `@text.strong`, `@text.uri`, `@keyword`, ...) and the pack's
additions; `folds.scm` and `indents.scm` are the pack's.
