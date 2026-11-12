# Changelog

## 1.3.0

- A lesson in Fenix's tutorial (`SPC h T`, its *Notebook* section): the
  notebook page, a note from a template, a line captured into today's
  journal, a link to a note, tags and search.
- The hints that named a leader sequence (`SPC n n`, `SPC n j`, `SPC n t`,
  `SPC m a`, `SPC h x` and the rest) -- the empty notebook page, Home's
  section, a mermaid block's hover, and what it says when an action can't
  run -- say the keys the key scheme in use has: `C-k n n` with standard
  keys, your own leader chord if you set one, a key you bound yourself.
  Home's section and the hints change when you switch between Vim keys and
  standard keys.

## 1.2.0

- `notebook.open`, `notebook.new_note`, `notebook.find`, `notebook.recent`, `notebook.journal`, `notebook.search`, `notebook.tags` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): their pages opened by a script or another tool, as
  the screenshots' scripts do.

## 1.1.0

- For Fenix 1.5 (extension API 1.5): a link's hover says the keys of the
  key scheme in use for following it -- `F12` with standard keys, `gd`
  with Vim keys. The README gives standard keys beside Vim's.

## 1.0.0

- The first release, for Fenix 1.0: notes, a daily journal, links and quick captures, kept for you in one folder -- an Obsidian vault works as it is.
