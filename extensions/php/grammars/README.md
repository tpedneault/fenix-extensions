# PHP's grammar

`tree-sitter-php.wasm` is tree-sitter-php 0.24.2's `php` grammar (PHP
inside HTML, what a `.php` file is) compiled to WebAssembly, as its npm
package ships it: `tree-sitter-php.wasm` in
https://registry.npmjs.org/tree-sitter-php/-/tree-sitter-php-0.24.2.tgz,
unchanged (SHA-256
`d4df6a6ff08c87c3ec4f9cbb785fe09998a0cb570e03f57d7b19b3acfb146aa7`; the
package's own is
`fdd22530d3845540ba74a1199d3f67204a608c99994bb423645d3f0cfdaead94`). It
exports `tree_sitter_php`, the name Fenix takes from the file's. Its
language ABI is 15, which Fenix's tree-sitter 0.26 loads (13 to 15).
tree-sitter-php is MIT-licensed, Copyright (c) 2017 Josh Vera, GitHub, and
(c) 2019 Max Brunsfeld, Amaan Qureshi, Christian Frøystad, Caleb White:
`LICENSE-tree-sitter-php`.

The package's other grammar, `php_only` (PHP with no `<?php` and no HTML
around it), is for PHP inside another language, which Fenix doesn't
highlight.

`queries/php/highlights.scm` is that grammar's own (MIT), with Fenix's
capture names and the names classes, enums and attributes are declared
with; `folds.scm` and `indents.scm` are the pack's.

To build one yourself, in the grammar's `php` folder at its tag
(https://github.com/tree-sitter/tree-sitter-php, v0.24.2): `tree-sitter
build --wasm` (tree-sitter's CLI 0.25 or later fetches the WASI SDK it
compiles with, or uses Docker or emscripten).
