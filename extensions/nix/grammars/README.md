# Nix's grammar

`tree-sitter-nix.wasm` is tree-sitter-nix 0.3.0's grammar
(https://github.com/nix-community/tree-sitter-nix, revision
`ea1d87f7996be1329ef6555dcacfa63a69bd55c6`) compiled to WebAssembly by the
Lumis project (`tree-sitter build --wasm` 0.26) and published as
`@lumis-sh/wasm-nix` 0.26.4 on npm
(https://registry.npmjs.org/@lumis-sh/wasm-nix/-/wasm-nix-0.26.4.tgz),
unchanged:

| File | SHA-256 | |
|---|---|---|
| `tree-sitter-nix.wasm` | `e8e23db3384a341ddab82307aaf9d2001df3c2d81843d4af29d6f08e857def7f` | exports `tree_sitter_nix`; the package's own `lumis.json` records the same hash |

The package's SHA-256 is
`58e5c88eae8c21a5390ed487104d2c0b0273bfe4cb1a0d3982c5183a239f2b7d`. The
grammar's own repository ships no `.wasm`, and the npm package of its own
name (`tree-sitter-nix` 0.0.2) is far older than 0.3.0 and carries none.
Its language ABI is checked by `src/data.rs`, which loads it as Fenix does
(tree-sitter 0.26 takes ABI 13 to 15) and runs the queries on a flake and a
NixOS module.

tree-sitter-nix is MIT-licensed, Copyright (c) 2019 Charles Strahan:
`LICENSE-tree-sitter-nix` is the licence file at that revision (the Lumis
packaging is MIT too, and adds nothing to the grammar). It's the grammar
Helix, Neovim's nvim-treesitter and Zed use for Nix.

`queries/nix/highlights.scm` is written from the grammar's own
`queries/highlights.scm` (MIT) at that revision, with Fenix's capture
names; its `folds.scm` and `indents.scm` are the pack's own. The grammar's
injections (shell in `buildPhase` and `writeShellScript` strings) aren't
used: Fenix has none.

To build the grammar yourself, in the repository at that revision:
`tree-sitter build --wasm` (tree-sitter's CLI 0.25 or later fetches the WASI
SDK it compiles with, or uses Docker or emscripten).
