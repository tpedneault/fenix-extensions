# Changelog

## 1.1.0

- For Fenix 1.5 (extension API 1.5): built against API 1.5, as lang-kit,
  the kit it's on, now is. Nothing you see changed: the fixes that
  install OCaml's tools run `opam install` as a task, in Fenix's task
  pane, not as a download of the pack's own, so there is no install
  notification for it, and the grammars, the language server, dune's
  tasks and the checks work as they did. The README gives standard keys
  beside Vim's. On Fenix 1.0 to 1.4, 1.0.0 stays the one you have.

## 1.0.0

- The first release, for Fenix 1.0: ocaml: its grammars for .ml and .mli, ocaml-lsp-server, ocamlformat where the project has an .ocamlformat, dune's tasks with a dune exec for each executable, debugging bytecode with earlybird, the project's checks with fixes that run opam install, snippets, and a dune project.
