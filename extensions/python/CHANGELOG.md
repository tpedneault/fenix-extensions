# Changelog

## 1.3.0

- For Fenix 1.6 (extension API 1.6): your pytest tests on Fenix's tests
  page (`SPC T T`), in the gutter and after each test's line, run from
  any of them and debugged with `SPC T d`. A test is found as you type
  (`test*` functions, `Test*` and `unittest.TestCase` classes,
  `@pytest.mark.skip` as skipped, `@pytest.mark.parametrize` as a test
  whose cases appear once listed or run); `pytest --collect-only` lists
  what the scan can't see, and imports your test modules to do it, so
  Fenix lists only in a project you trust. Runs go through a small
  plugin of the pack's (loaded with `-p`, nothing installed in your
  project): `uv run python` in a uv project (`--with pytest` when the
  project doesn't ask for it), else your Python; by pytest's own ids, in
  a file when there are many, a step for each folder with its own
  pytest configuration, with your `.env`. A failure is shown at its
  assertion, with its message, traceback and output; a fixture that
  raises is errored, not failed; a file that doesn't load says so and
  fails no test; a process that dies errors the tests that hadn't
  ended and names the one running; pytest-xdist works. Debugging a test
  runs pytest under debugpy on its node id. On Fenix 1.5, 1.2.0 stays
  the one you have.

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
