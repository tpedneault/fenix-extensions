# Jupyter notebooks

Jupyter notebooks (`.ipynb`) in Fenix. A notebook opens in it however you
open it -- the explorer, the file picker, `:e`, a recent file -- as its
cells one under another: code highlighted on a panel with its `[n]`
beside it, markdown as it reads, and each cell's outputs below it, with
`Out[n]`, tracebacks in red and matplotlib's plots (and any other PNG,
JPEG, GIF or WebP) drawn as pictures.

```text
hw1.ipynb  Python 3 (ipykernel) · idle · .venv · 6 cells · not saved

▌         Homework 1
▌         Read the data, greet the grader, plot it.

     [1]: with open('data.txt') as f:
              values = [int(x) for x in f.read().split()]
          print('read', len(values), 'values')
          sum(values)
          read 8 values
  Out[1]: 31

     [2]: name = input('Your name: ')
          print(f'Hello, {name}!')
          Hello, Ada!
```

The keys are Jupyter's command mode's, with Vim's moves:

| Key | What it does |
| --- | --- |
| `j` `k`, the arrows | The next cell, the one before |
| `g` `G` | The first and last cell |
| `J` `K`, PageDown PageUp | A screen down, up; the wheel scrolls |
| `Enter`, `e` | Edit the cell, in a pane beside: Fenix's editor, with its keys and highlighting. What you type is the cell's source as you type it |
| `Shift+Enter` | Run the cell and go to the next (a new one after the last) |
| `Ctrl+Enter`, `r` | Run the cell |
| `Alt+Enter` | Run it, and make a cell below |
| `R` | Run every cell, from the top; an error stops the rest, as in Jupyter |
| `ii` `00` | Interrupt the kernel; restart it (what it held is gone) |
| `a` `b` | A new cell above, below |
| `dd`, `x` | Delete the cell (`x` keeps it to paste) |
| `c` `v` `V` | Copy the cell; paste it below, above |
| `m` `y` | Make it markdown, code |
| `[` `]` | Move it up, down |
| `o` | Hide or show its outputs |
| `C` | Clear its outputs |
| `Y` | Copy its source |
| `u` `U`, Ctrl+Z Ctrl+Shift+Z | Undo, redo: the buffer's |
| `w`, Ctrl+S | Save |
| `t` | The notebook's text, its JSON (`SPC f o` too, and back) |
| `P` | Install ipykernel into the Python the notebook runs on |
| `q` | Close it, and its kernel |

A click focuses a cell, a double click edits it.

The commands (`SPC :`) run every cell, interrupt, restart, clear every
output, and install ipykernel, for the notebook you were last in.

## Changes go through the notebook's buffer

A cell edited, made, moved, deleted or run is an edit of the notebook's
buffer, as if typed: the file changes when you save (`w`), and Fenix's
undo has each change. Only what changed is replaced, and the notebook is
written as Jupyter writes it -- keys in order, a space a level, sources
and text as lines -- so a notebook from Jupyter, VS Code or Colab saved
here differs only where you changed it, and a diff of it is small.
Edited in its text (`t`), the page reads it again as you go.

## Running cells

The first cell you run starts the notebook's kernel, in the notebook's
folder (so `open('data.csv')` finds the file beside it), on its Python:

1. the one `jupyter.python` names (`SPC ,`), when you set it;
2. else the project's `.venv` (or `venv`);
3. else, in a uv project (a `pyproject.toml`), `uv run --with ipykernel`;
4. else `python3` (`python` on Windows) on `PATH`.

When that Python has **ipykernel**, cells run on a real Jupyter kernel
(the notebook's own, by its kernelspec, or `python3`): IPython's magics,
`display()`, rich reprs, everything Jupyter does. When it doesn't, they
run on that Python itself, so a notebook runs with nothing installed:

- the last line's value is shown as `Out[n]`, `print()`'s output below the cell;
- `input()` asks you for a line in Fenix's modeline;
- matplotlib's figures are drawn as pictures, on `plt.show()` and at the end of the cell;
- `!command`, `%pip install`, `%cd`, `%time` and `%matplotlib` work; other magics say they need IPython;
- `ii` interrupts a cell (a `KeyboardInterrupt`), `00` starts afresh.

The footer says which it is; `P` (or **Jupyter: install ipykernel**)
installs ipykernel as a task -- `uv add --dev ipykernel` in a uv project,
uv's or the Python's own pip otherwise -- and `00` restarts the kernel
with it.

What a cell runs is yours: the kernel runs with your permissions, as
Jupyter's does. Fenix's sandbox holds the extension to running Python
(and uv) only, but not what Python then does.

## Settings

| Setting | |
| --- | --- |
| `jupyter.python` | The Python to run notebooks with; empty to find one as above. A project may set its own (`.fenix/settings.toml`) |

## Not yet

- Widgets (ipywidgets) show their text, not the widget; HTML-only and
  SVG outputs say what they are, and JavaScript doesn't run.
- A kernel for another language (R, Julia) runs when it's installed, as
  the notebook's kernelspec names it, but only Python is highlighted.
- Completion and hover in a cell's editor: it's a scratch buffer, not a
  file the language server knows.
