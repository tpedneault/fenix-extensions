# Changelog

## 1.1.0

For Fenix 1.5 (extension API 1.5): what it says names the keys of the
key scheme you use, and yours where you rebound them. On Fenix 1.0 to
1.4, 1.0.0 stays the one you have.

- A lesson in Fenix's tutorial (`SPC h T`, its *Jupyter notebooks*
  section): a notebook opened on its page, a cell edited beside it, and
  cells run. Its practice text is a small notebook, to save as
  `tour.ipynb` when you have none.
- A kernel that ended badly says where to read why -- the extensions'
  log, `C-k h l` with standard keys, `SPC h l` with Vim keys -- and no
  Python found names the settings' keys the same way; a page says them
  again when you switch.
- Installing ipykernel (`P`) says how it ended, once its task does: that
  `00` starts a notebook's kernel again with it, or that it failed and
  the task pane has why. It said only that it had started.
- Fixed: a magic that needs IPython said `i` installs ipykernel, where
  the key is `P`.

## 1.0.0

- The first release, for Fenix 1.0: Jupyter notebooks (`.ipynb`) on a page -- cells and their outputs, plots as pictures, tracebacks -- edited in a pane beside and through the file's buffer, written as Jupyter writes them, and run on the notebook's Python: ipykernel when it's there, Python itself when it isn't, `input()` asked in Fenix.
