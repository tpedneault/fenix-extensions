# Tcl

Tcl scripts in Fenix, as an extension and a language server for Tcl
files. Its answers join any other Tcl language
server's. Tcl's highlighting stays in Fenix.

- `K` on a built-in shows Tcl's own usage line for it -- on one of its
  arguments too (`K` on the `$s` in `string compare $s $t` names
  `string compare`) -- and an ensemble's subcommands.
- Completion: a command's `-options` once you type a `-` (`lsort -u`),
  an ensemble's subcommands after its name (`string co`, `binary encode
  `), and otherwise Tcl's built-ins and the project's own procs and
  namespaces by their fully-qualified names (`myns::greet`). Nothing in
  a comment or a string.
- `gd` on a proc goes to where it's defined: in the file, else in the
  project.
- A call to one of the project's procs is colored as a call, wherever
  it's defined: Fenix asks its language server for them.

The project's definitions come from [Universal
Ctags](https://ctags.io) (`ctags` on `PATH`), which scans the project in
the background when a Tcl file opens; until it lands, completion offers
the built-ins.

## Keys

In a Tcl file, under `SPC m` (beside the MIB's, when it's on):

| Key | Command | What it does |
| --- | --- | --- |
| `SPC m s` | `code.symbols` | Fuzzy-find a definition by its fully-qualified name and jump to it |
| `SPC m T` | `completion.refresh_tags` | Scan the project with ctags again, and say how many definitions it found |

## The project doctor

In a Tcl project (a `pkgIndex.tcl` or a `.tcl` file at its top, or
`kind = "tcl"` in `.fenix/project.toml`), the doctor checks for `tclsh`
(warns when Tcl isn't installed) and `ctags`.

## What it may do

- Run `ctags` and look for `tclsh`.
- Read the project (to tell whether it's a Tcl one), and open its files.
- Know the file you're in when its commands run, for its project.

## Building

```text
cargo xtask pack extensions/tcl dist
```

`src/signatures.rs` is generated from a real Tcl interpreter:
`tclsh tools/gen_tcl_signatures.tcl src/signatures.rs`.
