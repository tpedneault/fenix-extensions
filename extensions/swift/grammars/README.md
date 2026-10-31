# Swift's grammar

`tree-sitter-swift.wasm` is tree-sitter-swift 0.7.1's grammar (Alex
Pinkus's, https://github.com/alex-pinkus/tree-sitter-swift), compiled to
WebAssembly by the pack: the release ships no `.wasm`, only its
generated `src/parser.c` and `src/scanner.c`, which is what was built,
unchanged. It came from npm's `tree-sitter-swift` 0.7.1
(`https://registry.npmjs.org/tree-sitter-swift/-/tree-sitter-swift-0.7.1.tgz`)
and was compiled with tree-sitter's CLI 0.26.13 (`tree-sitter-cli` on
npm), which used wasi-sdk 29:

```sh
tar xzf tree-sitter-swift-0.7.1.tgz && cd package
npx -y tree-sitter-cli@0.26.13 build --wasm -o tree-sitter-swift.wasm .
```

The result's SHA-256 is
`be600469cbb9932a1fc37dd4155b9100555d0949a9c067b4abe5d11fe846d32e`
(3.4 MB). Its language ABI is 14, which Fenix's tree-sitter 0.26 loads
(13 to 15); `src/data.rs` loads it as Fenix does and runs the queries.
The CLI 0.25 compiles only with Docker or emscripten; 0.26 fetches the
WASI SDK itself.

tree-sitter-swift is MIT-licensed, Copyright (c) 2021 alex-pinkus:
`LICENSE-tree-sitter-swift`.

`queries/swift/highlights.scm` is that grammar's own (MIT), with Fenix's
capture names; `folds.scm` and `indents.scm` are the pack's. It's the
grammar Helix, Zed and nvim-treesitter use for Swift. It doesn't read
typed throws (`throws(DecodingError)`, Swift 6) yet: the rest of the
function still highlights.
