# Scala's grammar

`tree-sitter-scala.wasm` is tree-sitter-scala 0.24.0's grammar (Scala 2 and
Scala 3: `.scala`, `.sc`, `.sbt`, Mill's `.mill`) compiled to WebAssembly, as
its npm package ships it: the file of that name in
https://registry.npmjs.org/tree-sitter-scala/-/tree-sitter-scala-0.24.0.tgz
(https://github.com/tree-sitter/tree-sitter-scala, the commit
`2d55e74b0485fe05058ffe5e8155506c9710c767` its package records as
`gitHead`), unchanged:

| File | SHA-256 | |
|---|---|---|
| `tree-sitter-scala.wasm` | `b7ec2bb29c19827abcefd18ed5cb5a43596009f96a5d53c5b9d1f9676d7521c3` | exports `tree_sitter_scala` |

The package's SHA-256 is
`af83daf86c49a880bc931f1c6e1ae5b099fb48ee51d6d41f18edb5837e1ef61d`; its SHA-1
(`0f28c9441fb1b7bcbc0edc06f47af62855c82160`) and SHA-512 integrity are what
npm's registry lists for it, and both matched when it was fetched. Its
language ABI is 14, which Fenix's tree-sitter 0.26 loads (13 to 15);
`src/data.rs` loads it as Fenix does and runs the queries on a Scala 3 and a
Scala 2 sample. tree-sitter-scala is MIT-licensed (the package's `license`;
the licence's own text, Copyright (c) 2018 Max Brunsfeld and GitHub, is
`LICENSE-tree-sitter-scala`, the package's own `LICENSE`).

`queries/scala/highlights.scm` is adapted from the `queries/highlights.scm`
that package ships (MIT, credited there to @stumash): rewritten for Fenix's
capture names, with the Scala 3 forms and Scala CLI's `//> using` directives.
The `folds.scm` and `indents.scm` are the pack's own (the package's
`indents.scm` is for Neovim's rules, not Fenix's).

To build the grammar yourself, in the package's folder: `tree-sitter build
--wasm` (tree-sitter's CLI 0.25 or later fetches the WASI SDK it compiles
with, or uses Docker or emscripten).
