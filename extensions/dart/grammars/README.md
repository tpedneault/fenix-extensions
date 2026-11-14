# Dart's grammar

`tree-sitter-dart.wasm` is UserNobody14's tree-sitter-dart
(https://github.com/UserNobody14/tree-sitter-dart) at commit
`0038280b774b65c1b89ba7fec50624d8baed9fe1` (17 October 2026, "fix(parser):
require single-record type commas"), compiled to WebAssembly by the pack:
the repository commits a `.wasm` of its own, but this one was built from
the committed `src/parser.c` and `src/scanner.c`, unchanged, so what's
here can be made again from the commit:

```sh
git clone https://github.com/UserNobody14/tree-sitter-dart && cd tree-sitter-dart
git checkout 0038280b774b65c1b89ba7fec50624d8baed9fe1
npx -y tree-sitter-cli@0.26.13 build --wasm -o tree-sitter-dart.wasm .
```

It was built with tree-sitter's CLI 0.26.13, which fetched wasi-sdk 29
itself. The result's SHA-256 is
`554f8ac774be62b320decae51ae420d226b0efec0e8358845ccb11dc7f36883d`
(1.2 MB). Its language ABI is 15, which Fenix's tree-sitter 0.26 loads
(13 to 15); `src/data.rs` loads it as Fenix does, runs the queries on a
sample of Dart 3 -- sealed and base classes, enhanced enums, extension
types, records, patterns, switch expressions, `when` guards, cascades,
string interpolation -- and expands each snippet into it.

The grammar follows the language to Dart 3.13 (primary constructors, dot
shorthands). It's the one nvim-treesitter, Helix and Zed's Dart
extensions are built on.

**Licence.** `LICENSE-tree-sitter-dart` is the repository's `LICENSE`,
word for word: the MIT licence, "Copyright (c) 2020-2023 UserNobody14
and others". (Its `package.json` says ISC, which the licence file
doesn't; the file is what's kept here, and it's MIT.)

`queries/dart/highlights.scm` is the repository's own (MIT), with
Fenix's capture names (`@conditional`, `@repeat`, `@keyword.return`,
`@parameter`, ...) and the pack's additions; `folds.scm` and `indents.scm`
are the pack's.
