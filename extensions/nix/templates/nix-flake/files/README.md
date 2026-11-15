# {{name}}

A flake, started from Fenix's *Nix · flake with a dev shell* template.

| Output | What it is |
|---|---|
| `packages.default` | a small program, built from a shell script: `nix build` leaves it in `./result` |
| `apps.default` | the same, for `nix run` |
| `devShells.default` | `nixd`, `statix`, `deadnix` and `{{formatter}}`: `nix develop` |
| `formatter` | `{{formatter}}`: `nix fmt`, and what Fenix formats a Nix file with |
| `checks.build` | `nix flake check` builds the package |

**Flakes only see what Git tracks.** Run `git init` and `git add .` before
the first `nix build`, or Nix says the files aren't there.

In Fenix, `SPC p t` offers `check`, `build`, `run`, `fmt`, `flake show` and,
last, `flake update (rewrites flake.lock)`; `SPC m b` and `SPC m r` pick one of
the flake's outputs from `nix flake show`, and `SPC m s` opens the dev shell
in a terminal. `SPC p h` is the project doctor: it says which of the dev
shell's tools Fenix finds, and its fix rows try nixd through the dev shell.
