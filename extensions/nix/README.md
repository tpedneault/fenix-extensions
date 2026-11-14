# Nix

Nix in Fenix: its grammar (Fenix has none of its own), nixd or nil as the
language server, nixfmt, alejandra or nixpkgs-fmt as the formatter, statix and
deadnix on save, the tasks of a flake, of a plain `default.nix` or
`shell.nix`, and of a NixOS or home-manager configuration, pickers of a
flake's outputs, the project doctor's checks, snippets, and two projects for
the new-project wizard. Built on lang-kit (`crates/lang-kit`), as each
language pack is.

It starts with Fenix. The server and the formatter are set whenever the
programs are found; the tasks are for a project with a `flake.nix`,
`default.nix` or `shell.nix` (or a NixOS or home-manager configuration). It
needs Fenix 1.5 or later (extension API 1.5), and **installs nothing**:
Nix's tools come from Nix -- the official installer
([nixos.org/download](https://nixos.org/download/)), Determinate Systems', or
your package manager's -- and from your profile or the project's dev shell
after that. **Without Nix** the pack still highlights, folds, indents and
completes snippets, and uses the formatter, linters and language server you
have; the doctor says how to install Nix, and that. What needs Nix itself is
the tasks, the pickers and a dev shell.

## The grammar

`*.nix` files are read with tree-sitter-nix 0.3.0's grammar (`grammars/`,
MIT): highlighted (attribute names, function arguments, calls, builtins,
paths, URIs, strings and their `${ }`, escapes, operators), folded (sets,
lists, `let`, multi-line strings, and a binding that holds one:
`systemd.services.orbit = { ... };`), with Enter indenting inside a set, a
list or a `let`, and the breadcrumbs and `SPC c s` listing the bindings.
`#` comments a line. The grammar comes compiled to WebAssembly by the Lumis
project, from the grammar's own commit (`grammars/README.md` has the
hashes); the grammar's queries are the model for `queries/nix/`, written
for Fenix's capture names. Not done: highlighting the shell inside a
`buildPhase = ''...''` (Fenix has no injections).

## The server

**nixd** (nix-community's) or **nil** (oxalica's). Both are maintained and
both are installed through Nix, not as release binaries -- neither project
publishes one, so there's no download for the pack to check and no fix that
installs one: `nix profile install nixpkgs#nixd`, or add it to the dev shell.

- **nixd** evaluates with Nix's own libraries: completion of NixOS,
  home-manager and flake-parts options, packages from nixpkgs, and go to
  definition into nixpkgs. It's the default.
- **nil** analyses the text alone: lighter and quicker to start, with less to
  say about options. It's the fallback when only nil is found.

`nix.server` (`auto`, `nixd`, `nil`) picks. The server is found on `PATH` and
where Nix puts things (`~/.nix-profile/bin`, `nix profile`'s folder under
`~/.local/state`, `/nix/profile`, `/etc/profiles/per-user/$USER`,
`/run/current-system/sw`, `/nix/var/nix/profiles/default`) and in
`~/.local/bin`, `~/.cargo/bin` and Homebrew's.

**Through the project's dev shell.** When the server isn't on `PATH` but Nix is,
the doctor's row for it has a fix, *Try it in the project's dev shell*
(`SPC p h`, `f`): a task that runs `nix develop --no-write-lock-file -c nixd
--version` in the task pane, so you see the dev shell being built. When it
works, `nix.server_runner` becomes `dev-shell` for this project and the server
is started as `nix develop --no-write-lock-file -c nixd` (`nix-shell --run
nixd` with a `shell.nix`). A second row, *Fetch it with nix shell*, does the
same with `nix shell nixpkgs#nixd -c nixd --version`, which **downloads** nixd
the first time, and sets `nixpkgs`. Neither is ever started when a file is
opened: until you run the fix (or set `nix.server_runner` yourself), a
project with no server on `PATH` starts none.

Two cautions. A dev shell's `shellHook` that prints to stdout would reach the
server's channel: print to stderr (`echo ... >&2`). And a server started
through `nix develop` is slower to start than one on `PATH`.

## Formatting

`SPC c F`, and saving with `editor.format_on_save`, run the formatter on the
buffer's text, directly (nothing is evaluated):

| Formatter | Runs |
|---|---|
| nixfmt (the official one, once `nixfmt-rfc-style`) | `nixfmt` |
| alejandra | `alejandra --quiet -` |
| nixpkgs-fmt | `nixpkgs-fmt` |

**Which one**, in order: `nix.formatter` when it names one (`off` turns it
off); else the one the project's flake names in its `formatter` output
(`formatter.${system} = pkgs.alejandra;`, read from `flake.nix`'s text, a
few words after `formatter` up to the `;`) -- even when it isn't installed,
since a different one would reformat the project in another style: the
doctor shows it missing and nothing is set; else the first of nixfmt,
alejandra and nixpkgs-fmt that's installed. A project that formats with
**treefmt** (a `formatter` output naming it, a `treefmt.toml`) gets no
formatter on save, for the same reason: a direct call would use another
style; the doctor says so, and `nix.formatter` formats with one tool
anyway.

**`nix fmt` isn't used on save.** It evaluates the flake before it runs the
formatter, so it takes seconds where Fenix gives a formatter three, and it
isn't a stdin filter for a buffer. It's the `fmt` task, for the whole project,
when the flake has a `formatter` output. The formatters' output is recorded
in `src/samples/` for alejandra and nixpkgs-fmt.

## Checks on save

- **statix** (`statix check -o json`): lints with their codes (`W01`),
  shown with the server's problems, and **its fixes offered by `SPC c a`**
  (`inherit y;` for `y = y;`). It needs a statix built with its `json`
  feature, as nixpkgs builds it; a statix without it says so in Fenix's log
  and shows nothing. It respects `statix.toml` and `.gitignore`.
- **deadnix** (`deadnix -o json`): unused `let` bindings, function
  arguments and pattern names, as warnings. It has no fixes in its JSON (its
  `--edit` rewrites the file itself), so none are offered. A function's unused
  arguments can be left out (`nix.deadnix_arguments` off passes `-L`: the
  `{ pkgs, lib, ... }:` style of `callPackage`). A file deadnix can't parse
  is the language server's to say.

Each has a setting (`nix.statix`, `nix.deadnix`) and runs only when found.
They go beside each other: Fenix keeps one list of problems per file for an
extension, so the pack sets both linters' findings together. Neither is
required.

## Tasks

`SPC p t`, in this order, in the project's folder. For a **flake**:

| Task | Runs |
|---|---|
| `check` | `nix flake check` |
| `build` | `nix build` |
| `run` | `nix run` |
| `fmt` | `nix fmt`, when the flake has a `formatter` output |
| `flake show` | `nix flake show` |
| `flake update (rewrites flake.lock)` | `nix flake update`: last, and named for what it does; no key or default is called `update` |

For a **`default.nix` or `shell.nix`** (no flake): `build` (`nix-build`, with
a `default.nix`) and `shell (build only)` (`nix-shell --run true`: builds the
environment and leaves). For a **NixOS configuration** (`nixosConfigurations`
in the flake, or a `configuration.nix`) with `nixos-rebuild` installed:
`nixos build` and `nixos dry-build`, each `--flake .` (the flake's
configuration for this machine's hostname) or `-I
nixos-config=./configuration.nix`. For a **home-manager** one with
`home-manager` installed: `home-manager build`, with `--flake .` or `-f
./home.nix`.

**What isn't there, on purpose.** There is no `switch` task -- not of NixOS,
not of home-manager -- and no `test`, `boot`, `dry-activate`, garbage
collection or profile install: nothing the pack runs changes the running
system, a profile or the store beyond building. `build` and `dry-build` write
nothing but `./result`. `nixos-rebuild switch` is a command you type in a
terminal. `dry-activate` isn't a task either: it needs root, and a task's
stdin is closed, so there's nowhere to answer sudo; `dry-build` says what
would change without it. `nix flake update` rewrites `flake.lock`: it's last,
named for it, and has no key. A task's stdin is closed: a dev shell isn't a
task, it's `SPC m s`.

## A flake's outputs

`SPC m b`, `SPC m r` and `SPC m S` run `nix flake show --json
--no-write-lock-file` (it may fetch the flake's inputs, and can take a
while; the lock file isn't written by looking) and list what it prints in
a picker, for this machine's system:

| Key | Lists | Picking runs |
|---|---|---|
| `SPC m b` | `packages`, `checks`, `devShells`, the NixOS configurations' system and the home-manager configurations' activation package | `nix build <that>`, as a task |
| `SPC m r` | `apps`, then `packages` | `nix run <that>`, as a task |
| `SPC m S` | `devShells` | `nix develop <that>`, in a terminal |

`SPC m s` opens the default dev shell in a terminal (`nix develop`, or
`nix-shell` with a `shell.nix`), without a picker. Esc in a picker runs
nothing. `build` of a NixOS configuration builds its `toplevel` and writes
`./result`: it doesn't touch the machine.

## The doctor

| Check | | Fix row |
|---|---|---|
| Nix | `nix --version`; 2.4 or later for a flake; a warning when it isn't found, never a failure | *How to install Nix*: text beside the editor, with the installers' addresses; nothing is downloaded |
| Flakes | `nix config show experimental-features` (Nix 2.20 and later) holds `flakes`; a warning in a flake project, a note elsewhere | none: it says what to add to `nix.conf` |
| Language server | the server in use and its version; or the runner it's started through | *Try it in the project's dev shell*, *Fetch it with nix shell* (tasks you start) |
| Formatter | the one in use (the flake's, or the first found), and why there's none | none |
| statix, deadnix | their versions; optional | none: nixpkgs, or the dev shell |
| nixos-rebuild, home-manager | in a project that has such a configuration; optional | none |
| Without Nix | when Nix is missing: what still works | |

`SPC m d` checks again, and looks for the programs again: a server you
installed since is found and set.

## Snippets

| Key | |
|---|---|
| `flake`, `devshell`, `mkshell` | a flake skeleton; a `devShells` output; `pkgs.mkShell` |
| `drv`, `callpkg`, `override`, `fetch` | `stdenv.mkDerivation`; `callPackage`; `overrideAttrs`; `fetchFromGitHub` |
| `module`, `opt`, `enable`, `mkif`, `imports` | a NixOS module; `mkOption`; `mkEnableOption`; `mkIf`; `imports` |
| `svc`, `hm` | a systemd service; a home-manager module |
| `overlay` | `final: prev: { ... }` |
| `let`, `with`, `inh`, `inhf`, `rec`, `fn`, `if`, `assert` | the language's own |

## New projects

*Nix · flake with a dev shell* in the new-project wizard: a flake with a
package and an app built from a shell script, a dev shell holding nixd, statix,
deadnix and the formatter you pick, that formatter as the flake's `formatter`
output (so `nix fmt` and Fenix agree), a check, `.gitignore` and `.envrc`.
*Nix · NixOS module*: a module with `enable`, `port` and `package` options and the
service they turn on, `nixosModules.default`, and an example configuration
that only evaluates the module (build it from `SPC m b`; it changes nothing on
the machine). Neither runs anything when it's made. Flakes only see what Git
tracks: `git add` the files first.

## Settings

| Setting | Default | |
|---|---|---|
| `nix.server` | auto | `auto` (nixd, else nil), `nixd`, `nil` |
| `nix.server_runner` | path | `path`, `dev-shell`, `nixpkgs`: where a server not on `PATH` comes from; set by the doctor's fix rows when they work; per project |
| `nix.formatter` | auto | `auto`, `nixfmt`, `alejandra`, `nixpkgs-fmt`, `off`; per project |
| `nix.statix` | on | statix on a saved file |
| `nix.deadnix` | on | deadnix on a saved file |
| `nix.deadnix_arguments` | on | report unused function arguments |

## Project kind

Fenix 1.6 and later know a Nix project (`NIX`): its tag in front of its name in the project hub, on Home, in the modeline and in the doctor, found by a `flake.nix` at its top, and opened on `flake.nix` (else `default.nix`) when you've had none of its files open. `kind = "nix"` in `.fenix/settings.toml` says it by hand. Fenix's own kinds come first: a project Fenix already takes for Rust, Python, Go, JavaScript, C/C++, Tcl or Arduino stays that one until `kind = "nix"` says otherwise, so this kind names projects Fenix would otherwise call plain folders. Earlier Fenix ignores this and the pack works there as before. The kind is declared in `extension.toml` (`[[contributes.project_kinds]]`); the pack's context markers say when the pack applies, and are not the same list: `shell.nix` and `default.nix` load the pack but don't make the folder a Nix project, since other languages' projects carry them for a dev shell or a package -- a kind's marker wins over other packs' kinds, which are ordered by extension id, so it would take a Ruby, PHP or Terraform project that has a `shell.nix`. A project with a `flake.nix` and another pack's kind (Ruby, say) is the same case: `kind = "ruby"` in its settings says which it is.

## Keys

After `SPC m` in a Nix project or file (`C-k m` with standard keys): `d`
checks the project again, `b` builds a flake output and `r` runs one (from a
picker), `s` opens the dev shell in a terminal and `S` one of the flake's.

## What it may do

- **Run Nix and its tools** (`permissions.process`): `nix` -- the tasks, `nix
  flake show` for the pickers, a dev shell in a terminal, and the language
  server through the dev shell or nixpkgs when you ask --, `nix-build`,
  `nix-shell`, `nixos-rebuild` and `home-manager` (build and dry-build only),
  `nixd` and `nil` (the server, and their versions), `nixfmt`, `alejandra`
  and `nixpkgs-fmt` (the formatter), `statix` and `deadnix` (on save). No other
  program, and no `/tools`: the pack installs nothing.
- **Read the project** (`filesystem = "project-read"`): `flake.nix`,
  `default.nix`, `shell.nix`, `configuration.nix`, `home.nix` and
  `treefmt.toml` at its top, to see what the project is and which formatter
  the flake names. It writes nothing. (`nix flake show` and the dev shell
  may write what Nix writes: the store, and `flake.lock` for the tasks that
  update it.)
- **Read the file you save** (`editor = "read"`): to know which file to lint.

It asks for no network: Nix reaches what a flake's inputs and a download
need, as it does in a terminal; that's Nix's, not the pack's.

## Not yet

- **Debugging**, and tests: Nix has no debugger the pack wires, and `nix
  flake check` is a task.
- **`nixfmt`'s and `nixd`'s own configuration** (`.nixd.json`'s option
  expressions): nixd reads its own settings.
- **Injected languages** in strings; **`.nix` files that aren't Nix** (`flake.lock`
  is JSON).
- **Remote builders, `nix-darwin`'s `darwin-rebuild`, and `switch`.**

## Building

```text
cargo xtask pack extensions/nix dist
cargo xtask install extensions/nix
cargo test --locked --manifest-path extensions/nix/Cargo.toml
```

`cargo test` runs the pack on lang-kit's fake host and the test kit's: its
tasks, server and formatter are set per project shape, the doctor reads what
it finds and offers its fixes (as tasks, never started by themselves), the
flake pickers list `nix flake show`'s outputs, statix's and deadnix's JSON
is read into problems with their fixes (`src/lint.rs`); each piece on its own
(`src/tools.rs`, `project.rs`, `choose.rs`, `doctor.rs`, `tasks.rs`,
`flake.rs`); and the data as Fenix reads it (`src/data.rs`) -- the language
file, the grammar loaded and the queries run on a flake and a NixOS module,
the snippets (each is Nix once filled in), the project templates, and the
manifest against the code. What statix, deadnix, alejandra and nixpkgs-fmt
printed in `src/samples/` was recorded from the real programs; what Nix,
nixd, nil and nixfmt print is written from their documentation (see
`src/samples/README.md`).
