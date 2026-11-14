# Changelog

## 1.2.0

- Fenix 1.6 and later show a project with a `cabal.project`, `stack.yaml`, `package.yaml` or `<folder>.cabal` at its top as a Haskell project (`HS`), in the project hub, on Home, in the modeline and in the doctor, and open it on `app/Main.hs` or `src/Main.hs` when you've had none of its files open; `kind = "haskell"` in a project's `.fenix/settings.toml` says it by hand. Fenix's own kinds come first: a project Fenix already takes for Rust, Python, Go, JavaScript, C/C++, Tcl or Arduino stays that one until `kind = "haskell"` says otherwise, so this kind names projects Fenix would otherwise call plain folders. Fenix 1.5 ignores all this and the pack works there as before: it's only data in the manifest, so it needs no newer Fenix and no newer API.

## 1.1.0

- For Fenix 1.5 (extension API 1.5): built against API 1.5, as the
  shared kit it's built on now is. Nothing you see changed: this pack
  installs GHC, Cabal, Stack and the language server with ghcup, as fixes
  the doctor offers and which run as tasks, so it has no download of its
  own to say the end of in a notification. On Fenix 1.0 to 1.4, 1.0.0
  stays the one you have. The README gives standard keys beside Vim's.

## 1.0.0

- The first release, for Fenix 1.0: haskell: its grammar, the Haskell Language Server, ormolu or fourmolu, hlint on save with its rewrites as fixes, Cabal's and Stack's tasks for each component, the project's checks with fixes that run ghcup, snippets, and a Cabal project.
