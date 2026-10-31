# Python

Python in Fenix: basedpyright as the language server with ruff's beside
it, debugging with debugpy, ruff as the formatter, tasks, the project doctor's
checks with their fixes, outdated and vulnerable packages, snippets, and
Django and Flask projects in the new-project wizard. Built on lang-kit
(`crates/lang-kit`), as each language pack is.

It works for a project uv manages (a `uv.lock`, or `[tool.uv]` in
`pyproject.toml`) and for one it doesn't: then the venv's packages are
uv's `uv pip` when there's uv, else the `python3` on `PATH` (the venv's,
when it's active) and its pip.

## The language server

basedpyright's, `basedpyright-langserver --stdio`, when it's installed;
else pyright's. `python.server` says which to prefer when both are.
With neither, Fenix's own `pyright-langserver` is started as before, and
the doctor's fix row installs basedpyright (`uv tool install
basedpyright`, else `pipx install`, else `pip install --user`).

`python.interpreter` names a Python of your own: it's checked, runs the
tasks and debugpy where there's no uv, and is the one the server is
given (`python.pythonPath`), over the one Fenix finds.

## Linting and formatting

ruff's server, `ruff server`, runs beside basedpyright: its problems
as you type, and its fixes and code actions on `SPC c a`, with the
server's. It needs ruff 0.5.3 or later (the doctor says).

With `python.ruff` = `save`, `ruff check --output-format json` runs on
a file when it's saved instead, and its problems are shown with the
server's: syntax errors and undefined names as errors, the rest as
warnings, or what ruff says if milder. Its fixes are offered on `SPC c
a` (one ruff calls unsafe says so); it never applies them (`--no-fix`),
and a file the project excludes is left alone. `off` is neither.

`SPC c F` -- and saving, with `editor.format_on_save` -- runs `ruff
format` on the buffer. The project's ruff (`uv run ruff`) when it asks
for it, else the one installed; with none, nothing is set and the doctor
offers to install it.

## Debugging

The adapter is debugpy over stdio. In a project uv manages it's `uv run
--with debugpy python -m debugpy.adapter`: uv brings debugpy into the
project's environment for the session, so it needn't be a dependency.
Without uv it's the Python's own, which needs `pip install debugpy` (a
fix row).

`SPC u u`, in a project with no launch configuration of its own, offers:

| | |
|---|---|
| Python: current file | the file you're in |
| Python: current file as a module | `python -m pkg.mod` for the file you're in, so relative imports work |
| pytest: current file, pytest: all tests | `python -m pytest`, with `justMyCode` off |
| Django: runserver | `manage.py runserver --noreload`, templates debugged too |
| Flask: current file | `flask --app <file> run`, without Flask's debugger and reloader |
| FastAPI: current file | `fastapi dev <file> --no-reload` |
| Python: attach (localhost:5678) | to `python -m debugpy --listen 5678 --wait-for-client app.py` |
| Python: attach to a process | to a Python running here, picked as it starts (`${pickProcess}`): debugpy is put into it by gdb (lldb on macOS) |

pytest, Flask and FastAPI are debugpy's `module`, so no program is sent.
The module one works the module out itself from the file, `src/` left
off, which Fenix's `${module}` keeps.

Beside these, each of the project's scripts (`[project.scripts]`) is a
launch of its own, *Python: orbit (script)*, calling what it names
(`orbit.cli:main`) as the script would; and a package with a
`__main__.py`, *Python: python -m orbit*.

## Tasks

`SPC p t`: run, test, lint, format, typecheck, and sync for a project uv
manages. Each runs the project's own tool when it asks for it (`uv run
pytest`), else one uv brings for the run (`uv run --with pytest
pytest`), else what's installed. `run` is the project's first script, a
Django project's `runserver`, a package's `__main__.py`, or `main.py` /
`app.py`; `test` is `manage.py test` in a Django project without
pytest-django; `typecheck` is mypy when the project has it, else
basedpyright (or pyright). The project's `.env` goes to each. A task of
the same name in `.fenix/tools.json` wins. Saving `pyproject.toml`,
`uv.lock` or a requirements file makes them again.

## The project doctor

- **Python**: the one uv would use (`uv python find`), else `python3`,
  held to `requires-python` (3.9 at least when it says none). Fix: `uv
  python install`.
- **uv**: needed by a project it manages. Fix: `pipx install uv`, else
  `pip install --user uv`.
- **Environment**: `.venv` in step with `uv.lock` (`uv sync --locked
  --dry-run --offline`): what differs, a lock that's out of date with
  `pyproject.toml`, or no `.venv` yet. Fix: `uv sync`.
- **Language server**, **ruff** (unless the project has its own),
  **debugpy** (without uv), each with its install.

A fix that installs something is checked again, and what it installed
is used at once.

## Packages

`SPC m p` (or *Python: packages*): lang-kit's page -- the vulnerable
first, then the outdated, the project's own marked with their group
(`dev`, `docs`, an extra as `[web]`).

| | uv's project | otherwise |
|---|---|---|
| list | `uv pip list --format json` | `uv pip list`, else `python3 -m pip list` |
| outdated | `uv pip list --outdated --format json` | the same |
| vulnerable | `uv tool run pip-audit --path .venv/.../site-packages` | the same with uv; else `python3 -m pip_audit` |
| `u` | `uv add 'name>=latest'` in its group; one it doesn't ask for, `uv sync --upgrade-package name` | `pip install --upgrade name` |
| `U` | `uv sync --upgrade` | `pip install --upgrade` the outdated |
| `a` | `uv add name` (`name@1.2` is `name==1.2`) | `pip install name` |
| `d` | `uv remove name` in its group | `pip uninstall name` |

pip-audit says no severity, so each advisory's risk is unknown; its
title is its CVE and the first sentence of what it says.

## Snippets

`cls`, `dc` (dataclass), `main` (`if __name__ == "__main__"`), `def`,
`adef`, `amain` (`asyncio.run`), `tg` (a TaskGroup), `with`, `cm` and
`acm` (contextlib's), `cmc` (`__enter__`/`__exit__`), `test`, `atest`,
`fixture`, `param`, `raises`, `prop`, `enum`, `proto`, `td`
(TypedDict), `try`, `ann` (`from __future__ import annotations`).

## Project templates

Fenix's wizard already has uv's Python, a FastAPI service, a CLI (Typer,
Click or argparse), a data project with notebooks and a desktop app;
this adds:

- **Django site**: settings from the environment (`.env` for
  development), one app with a page and its test, migrated; tests by
  pytest-django or `manage.py test`.
- **Flask app**: an app factory, a blueprint with a page and a JSON
  route, tests with Flask's client.

## Files

`*.pyw`, `*.py3`, `*.wsgi`, `*.gyp`, `*.gypi`, `SConstruct`,
`SConscript`, `wscript` and `.pythonrc` are read as Python.

## Keys

After `SPC m` in a Python project: `p` packages, `d` check again, `s`
sync.
