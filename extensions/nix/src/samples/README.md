# What the tests read

**Recorded from the real tools**, run in a scratch folder and copied here
unchanged:

| File | What it is |
|---|---|
| `statix-input.nix`, `statix-lint.json` | `statix check -o json statix-input.nix` (as `lint.nix`): statix 0.5.8, built from https://github.com/oppiliappan/statix at `a5292554eddc5aebae8b66a0ebe37a4aa5720442` with `cargo install --features json` (the crates.io `statix` is another program). Three lints, each with a fix |
| `statix-syntax.json` | the same on `{ a = ; }`: four "Syntax error" lints (code 0), no fix |
| `deadnix-input.nix`, `deadnix.json` | `deadnix -o json deadnix-input.nix` (as `ok.nix`): deadnix 1.3.1 from crates.io. Thirteen unused bindings and arguments |
| `deadnix-syntax.json` | the same on `{ a = ; }`: results that are only a `message` |
| `ugly.nix`, `ugly.alejandra.nix`, `ugly.nixpkgs-fmt.nix` | `alejandra --quiet -` (4.0.0, https://github.com/kamadorueda/alejandra at tag `4.0.0`, package `alejandra_cli`) and `nixpkgs-fmt` (1.3.0, crates.io) given `ugly.nix` on stdin, as Fenix's formatter calls them. On a file that doesn't parse, alejandra exits 1 (Fenix takes a formatter's output only when it ends well) and nixpkgs-fmt exits 0 and writes the text back as it was |

A clean file prints nothing from statix and from deadnix, exit code 0: the
tests give `""`.

**Written from the tools' documentation, not recorded** -- Nix itself isn't
installed where this was written, and neither are nixd, nil or nixfmt (a
Haskell program):

| What | Where |
|---|---|
| `flake-show.json` | the shape `nix flake show --json` prints (Nix 2.2x): output types, by system for `packages`, `apps`, `checks`, `devShells`, `formatter`, by name for the configurations and modules |
| the version lines of `nix`, `nixd`, `nil`, `nixfmt`, `nixos-rebuild`, `home-manager`, and `nix config show experimental-features` | in `src/lib.rs` and `src/doctor.rs` |
| `nixfmt` reading stdin when no file is given | nixfmt's `--help` |

`flake.nix` and `module.nix` are samples written for the grammar's tests:
a flake with a package, a dev shell and a formatter, and a NixOS module with
options, a service, paths, URIs, escapes and operators.
