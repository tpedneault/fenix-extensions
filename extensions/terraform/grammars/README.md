# Terraform's and HCL's grammars

`tree-sitter-terraform.wasm` (the Terraform dialect: `.tf`, `.tfvars`) and
`tree-sitter-hcl.wasm` (HCL itself: `.hcl`) are tree-sitter-hcl 1.2.0's
grammars compiled to WebAssembly, as its npm package ships them: the files
of those names in
https://registry.npmjs.org/@tree-sitter-grammars/tree-sitter-hcl/-/tree-sitter-hcl-1.2.0.tgz
(https://github.com/tree-sitter-grammars/tree-sitter-hcl, the commit
`fad991865fee927dd1de5e172fb3f08ac674d914` its package records as
`gitHead`), unchanged:

| File | SHA-256 | |
|---|---|---|
| `tree-sitter-terraform.wasm` | `0d9ef3ae926acc0411bfecba9b34df4ea659061917b96455570a45a70824e890` | exports `tree_sitter_terraform` |
| `tree-sitter-hcl.wasm` | `86bb80cd151bd3ab1e44ed431a9c1874978db31519c62055efb50f028a1d0118` | exports `tree_sitter_hcl` |

The package's SHA-256 is
`6aa2c0b5e17788fdd238c00c7d18c755771c474c05b6073b17a3cac18881d417`. Their
language ABI is 14, which Fenix's tree-sitter 0.26 loads (13 to 15);
`src/data.rs` loads them as Fenix does and runs the queries on them.
tree-sitter-hcl is Apache-2.0-licensed (the package's `license`; the
licence's own text, which names no holder, is `LICENSE-tree-sitter-hcl`,
the same as the repository's at that commit). It's the grammar Helix and
nvim-treesitter use for Terraform.

`tree-sitter-terraform.wasm` is used for both `.tf` files (`terraform`)
and `.tfvars` (`terraform-vars`, a language of its own because
terraform-ls knows them by that name): one file, two language files
(`languages/`).

The grammar's repository and package ship no queries, so
`queries/terraform/` -- `highlights.scm`, `folds.scm` and `indents.scm`,
for both grammars, which share their node names -- is this pack's own,
written for Fenix's capture names.

To build the grammars yourself, in the package's folder: `tree-sitter build
--wasm` (tree-sitter's CLI 0.25 or later fetches the WASI SDK it compiles
with, or uses Docker or emscripten); the terraform dialect is built from
`dialects/terraform`.
