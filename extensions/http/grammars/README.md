# tree-sitter-http

`tree-sitter-http.wasm` is [rest-nvim/tree-sitter-http]'s grammar at
commit `db8b4398de90b6d0b6c780aba96aaa2cd8e9202c` (language ABI 14),
compiled to WebAssembly unchanged. tree-sitter-http is MIT-licensed,
Copyright (c) 2021 NTBBloodbath.

It was built with the system's clang and wasm-ld rather than
`tree-sitter build --wasm`: the parser has no scanner and uses only
`malloc`, `calloc`, `realloc`, `free` and `memcpy`-like calls, which
tree-sitter's store gives it, so headers declaring those were enough
(`stub/stdlib.h`, `stub/string.h`):

```text
clang --target=wasm32-unknown-wasi -ffreestanding -nostdinc \
  -isystem stub -isystem "$(clang -print-resource-dir)/include" \
  -fPIC -shared -Os -fno-exceptions -fvisibility=hidden -nostdlib \
  -Wl,--export=tree_sitter_http -Wl,--allow-undefined -Wl,--no-entry \
  -I src src/parser.c -o tree-sitter-http.wasm
```

`tree-sitter build --wasm` in the grammar's folder (tree-sitter's CLI,
0.25 or later) makes one that loads the same.

The queries in `queries/http/` are the extension's own, by the capture
names Fenix's grammars use.

[rest-nvim/tree-sitter-http]: https://github.com/rest-nvim/tree-sitter-http
