# The proto grammar

`tree-sitter-proto.wasm` is tree-sitter-proto 0.6.0's grammar (Mohammad
Ashar Khan's, https://github.com/coder3101/tree-sitter-proto, proto2,
proto3 and editions), compiled to WebAssembly by the pack: the grammar
isn't on npm and its 0.6.0 release ships no `.wasm`, only its generated
`src/parser.c` (language ABI 15, which Fenix's tree-sitter 0.26 loads,
13 to 15; no external scanner), which is what was built, unchanged. It
came from crates.io's `tree-sitter-proto` 0.6.0
(`https://static.crates.io/crates/tree-sitter-proto/tree-sitter-proto-0.6.0.crate`),
whose `.crate` (a tar.gz) has the SHA-256
`9c93b6f1ed20de442e900eb636f2176af6063953dfaa9f76bf168dd3b490a3a1`, the
checksum crates.io's index gives for it; its `src/parser.c` has the
SHA-256 `ac481450c32e8fa52976075581701ecc63790fbab4c9c4f10d0901c9a39b61eb`.

It was built with the system's clang (18.1.3) and wasm-ld rather than
`tree-sitter build --wasm`, as the HTTP pack's grammar was: the parser
calls nothing of the C library's, and its header only declares
`malloc`, `calloc`, `realloc` and `free`, which tree-sitter's store gives
it, so a header declaring those was enough (`stub/stdlib.h`):

```text
clang --target=wasm32-unknown-wasi -ffreestanding -nostdinc \
  -isystem stub -isystem "$(clang -print-resource-dir)/include" \
  -fPIC -shared -Os -fno-exceptions -fvisibility=hidden -nostdlib \
  -Wl,--export=tree_sitter_proto -Wl,--allow-undefined -Wl,--no-entry \
  -I src src/parser.c -o tree-sitter-proto.wasm
```

The result has the SHA-256
`c81bd4202dc49d8c856cc3a46d5560a6395331a50c6a58307c5b9d7137d53980`.
`tree-sitter build --wasm` in the crate's folder (tree-sitter's CLI 0.25
or later) makes one that loads the same. tree-sitter-proto is
MIT-licensed, Copyright (c) 2024-2025 Mohammad Ashar Khan:
`LICENSE-tree-sitter-proto`.

`queries/proto/highlights.scm` is that grammar's own (MIT), which already
uses capture names Fenix colours (`keyword.type`, `type.builtin`,
`string.special.path`, `variable.member`), with its `@spell` taken out --
it pairs with `@comment`, which says it; `folds.scm` is the grammar's,
without its run of imports, which Fenix would list as scopes, and with
`extend`; `indents.scm` is the grammar's with Fenix's `@indent` (it
doesn't read `@indent.end`, `@indent.branch` or `@indent.auto`).

Its main branch has changed since 0.6.0 (names that are keywords, as in
`option (map).x`, and option values that are lists); a later release's
`parser.c` can be built the same way.
