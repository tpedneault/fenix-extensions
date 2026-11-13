# Changelog

## 1.2.0

- For Fenix 1.5 (extension API 1.5): built against API 1.5, as lang-kit,
  the kit it's on, now is. Nothing else you see changed: the fixes that
  install Python's tools run uv, pipx or pip as tasks, in Fenix's task pane,
  not as a download of the pack's own, so there is no install
  notification for it. The README gives standard keys beside Vim's. On
  Fenix 1.0 to 1.4, 1.1.0 stays the one you have.
- A lesson in Fenix's tutorial (`SPC h T`, its *Python* section), in a
  Python project of yours: the project doctor and a fix from it, the
  packages page, a task, a debug session stopped at a breakpoint, and
  ruff on save.

## 1.1.0

- `python.packages` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.0: python: basedpyright with ruff's server beside it, debugging with debugpy (the project's scripts, pytest, Django, Flask, FastAPI, attaching to a process), ruff as the formatter, tasks, the project's checks with their fixes, outdated and vulnerable packages for uv and pip, snippets, and Django and Flask projects.
