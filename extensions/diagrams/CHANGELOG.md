# Changelog

## 1.3.0

For Fenix 1.5 (extension API 1.5):

- A project's own diagrams: a new diagram (`SPC m n`, the notebook's
  `SPC n d`, `diagram.new`) goes into the project when one is open --
  under `diagrams.folder` (`docs/diagrams` by default), in folders you
  type (`auth/login-flow`) -- or into the notebook, as you choose.
- `SPC m l` (`diagram.list`) lists the project's diagrams by folder, with
  what each is, and opens the one picked; built and hidden folders
  (`node_modules`, `target`, `.git`) are left out.
- `SPC m M` (`diagram.move`) moves or renames a diagram within the
  project, its unsaved edits along, its picture following it.

## 1.2.0

For Fenix 1.5 (extension API 1.5):

- Exporting (`SPC m e`, `e` on a diagram's page) asks how: copy the
  picture to the clipboard, or save it as an SVG or a PNG file beside the
  diagram, as before.
- `SPC m y` (`y` on a diagram's page, `diagram.copy_picture`) copies the
  diagram to the clipboard as a picture straight away, drawn as an export
  is.

## 1.1.0

- For Fenix 1.5 (extension API 1.5): what it says names the keys of the
  key scheme in use, and yours where you rebound them -- run outside a
  diagram, its commands say `C-k m p` (preview), `v`, `t` and `e` with
  standard keys, and `SPC m p` with Vim keys.

## 1.0.0

- The first release, for Fenix 1.0: mermaid diagrams drawn as you type, their errors on their lines, Fenix's themes and yours, and exports.
