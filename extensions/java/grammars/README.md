# Kotlin's grammar

`tree-sitter-kotlin.wasm` is tree-sitter-kotlin 0.3.8's grammar compiled
to WebAssembly, as its release ships it: the asset of
https://github.com/fwcd/tree-sitter-kotlin/releases/tag/0.3.8, unchanged
(SHA-256 `c624e7443b371c28adc5d81674e73067564c12555ebe3ed96a6c8db814b7602d`).
Its language ABI is 14, which Fenix's tree-sitter 0.26 loads (13 to 15).
tree-sitter-kotlin is MIT-licensed, Copyright (c) 2019 fwcd:
`LICENSE-tree-sitter-kotlin`.

`queries/kotlin/highlights.scm` is that grammar's own (MIT; it says it's
from nvim-treesitter's, Apache-2.0), with Fenix's capture names;
`folds.scm` and `indents.scm` are the pack's.

fwcd's grammar is the one Helix, Zed and nvim-treesitter use.
tree-sitter-grammars' rewrite (1.1.0) was looked at: smaller, but it reads
`true`, `null`, `break` and `continue` as names, and has no queries.

To build one yourself, in the grammar's folder at its tag: `tree-sitter
build --wasm` (tree-sitter's CLI 0.25 or later fetches the WASI SDK it
compiles with, or uses Docker or emscripten).
