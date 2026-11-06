# Haskell's grammar

`tree-sitter-haskell.wasm` is tree-sitter-haskell 0.23.1's grammar
compiled to WebAssembly, as its npm package ships it: the file of that
name in
https://registry.npmjs.org/tree-sitter-haskell/-/tree-sitter-haskell-0.23.1.tgz
(https://github.com/tree-sitter/tree-sitter-haskell, tag v0.23.1),
unchanged (SHA-256
`37a6b07b1a838d02ffb4f4c2a06863637a8efe48432d60a275f50f1d08f1092c`,
3.8 MB; the package's,
`9e4d54fd3c0a781b712feab91d7ecf1c335c90777f29b991df83f9464ba11d73`,
whose SHA-1 `ccf0fc6f2f55c193745947fe429a9681d1d618bb` and SHA-512 are
the ones npm gives for it). Its external scanner, the layout rule, is
in it. Its language ABI is 14, which Fenix's tree-sitter 0.26 loads (13
to 15); `src/data.rs` loads it as Fenix does and runs the queries.
tree-sitter-haskell is MIT-licensed, Copyright (c) 2014 Max Brunsfeld:
`LICENSE-tree-sitter-haskell`.

`queries/haskell/highlights.scm` is that grammar's own (MIT), with
Fenix's capture names, the grammar's helper captures (`@_name`, `@_op`)
given names Fenix colours -- it colours every capture, and one it has no
colour for would win its text and show it plain -- and its types first:
Fenix colours a text by the latest pattern, so the grammar's last
`(variable) @type` would make every name a type; a type variable is a
`type/variable`. `folds.scm` and `indents.scm` are the pack's: the
grammar has neither.

0.23.1 is the latest release, and the grammar Helix, Zed and
nvim-treesitter use.

To build one yourself, in the grammar's folder at its tag: `tree-sitter
build --wasm` (tree-sitter's CLI 0.25 or later fetches the WASI SDK it
compiles with, or uses Docker or emscripten).
