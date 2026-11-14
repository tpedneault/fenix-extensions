# Elixir's grammars

`tree-sitter-elixir.wasm` is tree-sitter-elixir 0.3.5's grammar compiled
to WebAssembly, as its npm package ships it: the file of that name in
https://registry.npmjs.org/tree-sitter-elixir/-/tree-sitter-elixir-0.3.5.tgz
(https://github.com/elixir-lang/tree-sitter-elixir, tag v0.3.5),
unchanged (SHA-256
`ed99093c548c12d43f7e337fd3440e9e2daa2ec671a5e29aadb6c6dcb2232a62`; the
package's, `f89db8ca50495f21b550ce3f65c9f8c00495bd0e3b9f86644f2d06d04d3188f2`).
Apache-2.0: `LICENSE-tree-sitter-elixir`.

`tree-sitter-heex.wasm` is tree-sitter-heex 0.8.1's grammar
(https://github.com/phoenixframework/tree-sitter-heex, revision
`008626a3fad379d17c81d5ed576edd9bd7a4fbf7`, its latest tag v0.9.0 being
newer: 0.8.1 is the one this build is of), compiled to WebAssembly by
the Lumis project (`tree-sitter build --wasm` 0.26.11) and published as
`@lumis-sh/wasm-heex` 0.26.5 on npm
(https://registry.npmjs.org/@lumis-sh/wasm-heex/-/wasm-heex-0.26.5.tgz),
unchanged (SHA-256
`fd9cf8602295f4984793087744851fd2023b243a096f81755a315c5655a118df`,
which is what the package's own `lumis.json` records for it). The
grammar's own repository ships no `.wasm`. tree-sitter-heex is
MIT-licensed, Copyright (c) 2022 Connor Lay: `LICENSE-tree-sitter-heex`
(the file at that revision; the Lumis packaging is MIT too, and adds
nothing to the grammar).

The Elixir grammar has language ABI 14 and HEEx's 15, which Fenix's tree-sitter 0.26 loads (13
to 15); the tests load them as Fenix does.

`queries/elixir/highlights.scm` is tree-sitter-elixir's own (Apache-2.0),
with Fenix's capture names, its placeholder captures given names of their
own, and atoms read as constants; `queries/heex/highlights.scm` is
tree-sitter-heex's, as the Lumis package ships it with the grammar
(MIT). The `folds.scm` and `indents.scm` of each are the pack's.

**EEx** has a grammar of its own (connorlay/tree-sitter-eex, MIT
according to its `package.json`) that this pack doesn't bring: that
repository has no licence file to put beside it. `.eex` and `.leex` files
are read with HEEx's grammar, which has EEx's `<% %>` in it.

The Elixir grammar is the one Helix, Zed and nvim-treesitter use; HEEx's
is Phoenix's own.

To build one yourself, in the grammar's folder at its tag: `tree-sitter
build --wasm` (tree-sitter's CLI 0.25 or later fetches the WASI SDK it
compiles with, or uses Docker or emscripten).
