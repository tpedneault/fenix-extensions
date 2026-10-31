# Ada's grammar

`tree-sitter-ada.wasm` is tree-sitter-ada 0.9.2's grammar (Emmanuel
Briot's, https://github.com/briot/tree-sitter-ada), compiled to
WebAssembly by the pack: the release ships no `.wasm`, only its
generated `src/parser.c` (tree-sitter 0.25.8's; no external scanner),
which is what was built, unchanged. It came from crates.io's
`tree-sitter-ada` 0.9.2
(`https://crates.io/api/v1/crates/tree-sitter-ada/0.9.2/download`),
whose `.crate` (a tar.gz) has the SHA-256
`38a93324639ffa4932338c566baf6b3a74bdcf576ec56601e16975dc72a6564f`,
the checksum crates.io gives for it. It was compiled with tree-sitter's
CLI 0.26.13 (`tree-sitter-cli` on npm), which used wasi-sdk 29:

```sh
tar xzf tree-sitter-ada-0.9.2.crate && cd tree-sitter-ada-0.9.2
npx -y tree-sitter-cli@0.26.13 build --wasm -o tree-sitter-ada.wasm .
```

The result's SHA-256 is
`c155744f085b517560432379c130c03d633560a2cfd659e9f13e580f25ce56d5`
(330 KB). Its language ABI is 14, which Fenix's tree-sitter 0.26 loads
(13 to 15); `src/data.rs` loads it as Fenix does and runs the queries.

tree-sitter-ada is MIT-licensed, Copyright (c) 2023 Emmanuel Briot:
`LICENSE-tree-sitter-ada`.

`queries/ada/highlights.scm` is that grammar's own (MIT), with Fenix's
capture names, SPARK's aspects and pragmas as attributes, and a few
names more (types, parameters, components); `folds.scm` is the
grammar's, with tasks, protected objects, records, `case` and handlers
added; `indents.scm` is the pack's. It's the grammar Helix, Zed and
nvim-treesitter use for Ada.

GNAT's project files (`.gpr`) are read with it too (`languages/gpr.toml`),
though it isn't their grammar: `project Orbit is` and `for Main use
(...)` aren't Ada, so a project file parses with errors, and only its
keywords, strings and comments are coloured.
