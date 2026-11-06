# OCaml's grammars

`tree-sitter-ocaml.wasm` (implementations, `.ml`) and
`tree-sitter-ocaml_interface.wasm` (interfaces, `.mli`) are
tree-sitter-ocaml 0.24.2's grammars compiled to WebAssembly, as its npm
package ships them: the files of those names in
https://registry.npmjs.org/tree-sitter-ocaml/-/tree-sitter-ocaml-0.24.2.tgz
(https://github.com/tree-sitter/tree-sitter-ocaml, tag v0.24.2),
unchanged:

| File | SHA-256 | |
|---|---|---|
| `tree-sitter-ocaml.wasm` | `761a78a804931cfac1fa0c6238989b4b0e86cc70db461b1315d743de923f8246` | 5.6 MB, exports `tree_sitter_ocaml` |
| `tree-sitter-ocaml_interface.wasm` | `c065e8da9052899592bd68c820ffda076e23394129de14c0bd2c46b189415ea9` | 4.9 MB, exports `tree_sitter_ocaml_interface` |

The package's SHA-256 is
`81d70d0ad83227ebc06e08e6b73f49296b807f4a2a7606a6d8b058a8ac654a63`; its
SHA-1 `e6efe6df0956b61729acd1620ed2676e69c89336` and its SHA-512 are the
ones npm gives for it. Its third grammar, for type expressions alone
(`tree-sitter-ocaml_type.wasm`), isn't a file's, and is left out. Each
grammar's external scanner (comments, quoted strings) is in it. Their
language ABI is 14, which Fenix's tree-sitter 0.26 loads (13 to 15);
`src/data.rs` loads them as Fenix does and runs the queries.
tree-sitter-ocaml is MIT-licensed, Copyright (c) 2020 Max Brunsfeld and
Pieter Goetschalckx: `LICENSE-tree-sitter-ocaml`.

`queries/ocaml/highlights.scm` is that grammar's own (MIT), with Fenix's
capture names (an attribute's name is an `@attribute`, `true` and
`false` are `@boolean`), its keywords told apart, and floats and doc
comments told from the rest by `#match?`. `queries/ocaml_interface/` has
the same queries for interfaces, the highlights without `(shebang)`,
which the interface grammar has no node for. `folds.scm` and
`indents.scm` are the pack's: the grammar has neither.

0.24.2 is the latest release, and the grammar Helix, Zed and
nvim-treesitter use.

To build them yourself, in the grammar's folder at its tag: `tree-sitter
build --wasm grammars/ocaml` and `grammars/interface` (tree-sitter's CLI
0.25 or later fetches the WASI SDK it compiles with, or uses Docker or
emscripten).
