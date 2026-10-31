# Ruby's grammar

`tree-sitter-ruby.wasm` is tree-sitter-ruby 0.23.1's grammar compiled to
WebAssembly, as its npm package ships it: the file of that name in
https://registry.npmjs.org/tree-sitter-ruby/-/tree-sitter-ruby-0.23.1.tgz
(https://github.com/tree-sitter/tree-sitter-ruby, tag v0.23.1), unchanged
(SHA-256 `09a96427d7c72f0613ed470cd9812223fc4a91d6a9c025c0235cc6bd59ff96f4`).
Its language ABI is 14, which Fenix's tree-sitter 0.26 loads (13 to 15).
tree-sitter-ruby is MIT-licensed, Copyright (c) 2016 Rob Rix:
`LICENSE-tree-sitter-ruby`.

`queries/ruby/highlights.scm` is that grammar's own (MIT), with Fenix's
capture names; `folds.scm` and `indents.scm` are the pack's.

0.23.1 is the latest release, and the grammar Helix, Zed and
nvim-treesitter use.

To build one yourself, in the grammar's folder at its tag: `tree-sitter
build --wasm` (tree-sitter's CLI 0.25 or later fetches the WASI SDK it
compiles with, or uses Docker or emscripten).
