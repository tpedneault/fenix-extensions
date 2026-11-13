# Haskell

Haskell in Fenix: its grammar (Fenix has none of its own), the Haskell
Language Server, ormolu or fourmolu as the formatter, hlint on save with
its rewrites as fixes, Cabal's or Stack's tasks -- a task each for the
components the packages name --, the project doctor's checks with fixes
that run ghcup, snippets, and a Cabal project. Built on lang-kit
(`crates/lang-kit`), as each language pack is.

It works for a Cabal project (a `.cabal` file at the top, or a
`cabal.project` listing packages), a Stack one (`stack.yaml`, or hpack's
`package.yaml`), and a folder of scripts. It needs Fenix 1.5 or later
(extension API 1.5); on an older Fenix, Haskell 1.0.0 is the one you get.

## The grammar

`*.hs` and `*.hs-boot` files are read with tree-sitter-haskell 0.23.1's
grammar (`grammars/`, MIT): highlighted, folded (each equation, a data
type, a class, an instance, the imports, a `where`'s bindings, a `do`, a
`case`), with Enter indenting after a line that ends in `where`, `=`,
`->`, `do`, `of`, `then`, `else`, `let` or a bracket, and the breadcrumbs
and `SPC c s` listing the declarations. `--` comments a line.

## The server

The Haskell Language Server, `haskell-language-server-wrapper --lsp`: the
wrapper starts the server built for the project's GHC, which ghcup
installs beside it. A `haskell-language-server` of its own (Nix's) is
used when there's no wrapper. It's told the formatter the pack uses
(`haskell.formattingProvider`), and, while the pack runs hlint, to leave
its own hlint off, so a hint isn't said twice.

## Formatting and hlint

- `SPC c F`, and saving with `editor.format_on_save`: `ormolu
  --stdin-input-file <file>`, or fourmolu in a project with a
  `fourmolu.yaml` (`haskell.formatter`), which reads it. Either reads the
  `.cabal` file's extensions. `off` leaves formatting to the server.
- On save (`haskell.hlint`): `hlint --json` on the file, its hints shown
  with the server's problems -- an error, a warning, a suggestion as a
  note -- each with its rewrite on `SPC c a`: hlint's `to` in place of
  the code it found. A rewrite over several lines isn't offered (hlint
  prints it as GHC would, not as it was laid out). The project's
  `.hlint.yaml` is hlint's own to read.

## Tasks

`SPC p t`, by Cabal or Stack (`haskell.build_tool`: Stack in a project
with a `stack.yaml`, or only `package.yaml`s; else Cabal):

| Task | Cabal | Stack |
|---|---|---|
| build | `cabal build all` | `stack build` |
| test | `cabal test all` | `stack test` |
| run *exe* | `cabal run exe:<exe>`, each executable | `stack run <exe>` |
| test *suite* | `cabal test test:<suite>`, each test suite | `stack test <package>:test:<suite>` |
| bench *name* | `cabal bench bench:<name>`, each benchmark | `stack bench <package>:bench:<name>` |
| update, clean | `cabal update`, `cabal clean` | `stack clean` |
| hlint | `hlint .`, while it lints | |

The components are the packages' `.cabal` files' -- the one at the top,
or those `cabal.project`'s or `stack.yaml`'s `packages:` list (a folder,
a `.cabal` file, or folders by `*`) -- read for their `name:` and their
sections (`executable orbit`); where a package has only `package.yaml`,
its `executables:`, `tests:` and `benchmarks:`. A folder of scripts gets
`run <file>` (`runghc`) for each file at its top with a `main`. The
project's `.env` goes to each task; a task of the same name in
`.fenix/tools.json` wins. Saving a `.cabal` file, `cabal.project`,
`stack.yaml` or `package.yaml` reads them again (`SPC m r` too).

## The project doctor

| Check | | Fix row |
|---|---|---|
| GHC | `ghc --numeric-version`; needed, but for Stack, which brings its own | `ghcup install ghc --set recommended` |
| Cabal, or Stack | the one that builds the project | `ghcup install cabal recommended` (`stack`) |
| Haskell Language Server | the wrapper, or a server of its own | `ghcup install hls recommended` |
| hlint | while it lints | `cabal install hlint` (or `stack install`) |
| ormolu, or fourmolu | the formatter in use | `cabal install ormolu` (or `fourmolu`) |

Without ghcup, the toolchain's fix rows open ghcup's page
(https://www.haskell.org/ghcup/install/, `SPC m g`), which says how to
get it for this machine. A fix runs as a task; what it installed is
found and used at once, looked for on `PATH` and in ghcup's, Cabal's,
Stack's, Nix's and Homebrew's folders.

## Snippets

| Key | |
|---|---|
| `mod`, `main` | a module header with its exports; `Main` with `main :: IO ()` |
| `fn`, `guard`, `lam` | a function with its signature; guards; a lambda |
| `data`, `rec`, `newtype` | a sum type, a record, a newtype, each deriving `Show` and `Eq` |
| `class`, `inst` | a type class, an instance |
| `case`, `if`, `let`, `where`, `do` | the expressions and blocks |
| `imp`, `lang` | `import qualified ... as`; a `LANGUAGE` pragma |
| `hspec`, `desc`, `it`, `prop` | an hspec spec module, `describe`, `it`, a QuickCheck `prop` |

## The project template

*Haskell · Cabal package* (`SPC p n`): a `.cabal` file as `cabal init`
writes one -- a library, an executable over it, and a test suite on base
alone, so it builds without Hackage -- with `cabal run` as its run task.

## Settings

| Setting | Default | |
|---|---|---|
| `haskell.formatter` | `auto` | `ormolu`, `fourmolu`, or `off` |
| `haskell.hlint` | on | hlint on a saved file |
| `haskell.build_tool` | `auto` | `cabal` or `stack` |

## Keys

After `SPC m` in a Haskell project or file (`C-k m` with standard keys):
`d` checks the project again, `r` reads it again, `g` opens ghcup's page.

## Not yet

- **Debugging**: Haskell's debuggers (`ghci`'s, haskell-debugger) aren't
  set as an adapter yet.
- **`.cabal` files and literate Haskell**: Fenix has no grammar for
  `.cabal` files or `cabal.project`, and `.lhs` isn't read as Haskell.
- **A lambda hlint rewrites** keeps the brackets it was in:
  `map (\x -> x * 2)` becomes `map ((* 2))`, which hlint then says are
  redundant. Its own `--refactor` takes them off.
