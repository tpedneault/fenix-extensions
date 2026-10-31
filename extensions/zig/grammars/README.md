# Zig's grammar

`tree-sitter-zig.wasm` is tree-sitter-zig 1.1.2's grammar compiled to
WebAssembly, as its npm package ships it: the file of
`@tree-sitter-grammars/tree-sitter-zig` 1.1.2 on npm
(https://github.com/tree-sitter-grammars/tree-sitter-zig), unchanged
(SHA-256 `54b3b83dd9c62da5815f06132bc3fc914d9dcc780370b32416446a0b7969e8c6`;
the package's, `2512a88611e400dbafb9cb79b247c7f2671ed034e1714b33df29712ee93e534c`).
Its language ABI is 14, which Fenix's tree-sitter 0.26 loads (13 to 15).
tree-sitter-zig is MIT-licensed, Copyright (c) 2024 Amaan Qureshi:
`LICENSE-tree-sitter-zig`.

`queries/zig/highlights.scm` is that grammar's own (MIT), with Fenix's
capture names and its `#lua-match?`s as `#match?`s, which tree-sitter
runs (it leaves a predicate it doesn't know alone, so the pattern would
match everything); `folds.scm` is the grammar's, with the declarations
Fenix lists as scopes first; `indents.scm` is the pack's.

It's the grammar Helix, Zed and nvim-treesitter use.

To build one yourself, in the grammar's folder at its tag: `tree-sitter
build --wasm` (tree-sitter's CLI 0.25 or later fetches the WASI SDK it
compiles with, or uses Docker or emscripten).
