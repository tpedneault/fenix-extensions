# Scripted screenshots

Every picture on an extension's store page can be retaken unattended,
after a theme or a layout changes:

```text
cargo xtask screenshots [extension] [--fenix <path>] [--theme <name>] [--size WxH] [--dry-run]
```

For each picture in an extension's `screenshots.toml`, xtask packs the
extension, makes a clean profile for it (`FENIX_HOME`, under
`target/screenshots/<extension>/`), installs the package there with
`fenix --install-extension`, starts Fenix at the window size and theme
given, drives it over [the control channel][control], captures the
window into `extensions/<extension>/screenshots/<file>`, and stops that
Fenix. With no extension named, it does every one that has a script.

`--dry-run` reads and checks every script and prints what it would do,
starting nothing. `cargo xtask validate` checks the scripts too.

- `--fenix`: the Fenix to run; else `$FENIX`, else `fenix` on the `PATH`.
- `--theme`: the house theme, `Orbit Dark`, unless given.
- `--size`: the window's inside, in pixels, `1600x1000` unless given.
  It's physical pixels: at 150 % display scaling the picture is still
  1600 by 1000.

[control]: https://github.com/tpedneault/fenix/blob/main/docs/CONTROL.md

## House style

- One theme and one window size for the whole store: the defaults above.
- PNG, under 2 MB each (`validate` refuses more).
- Up to three an extension. The first is the hero, the one the store
  shows first.
- A sample of something real-looking, small enough to read at a glance,
  and nothing from a real person or a real service.

## The script

`extensions/<name>/screenshots.toml`, beside `extension.toml`. It isn't
packed. A sample project is a folder under `screenshots/samples/`
(here), which isn't packed either, and which is copied before Fenix opens
it, so nothing it writes lands in the repository.

```toml
[[shot]]
file = "table.png"            # under the extension's screenshots/: lower case, .png
note = "What's in it"         # printed with the plan; say here what stands in for a service
project = "csv"               # screenshots/samples/csv, opened with fenix/openProject
open = ["data/people.csv"]    # files in the project, fenix/open, in order; "src/main.rs:12[:3]" for a position
commands = []                 # fenix/runCommand ids, in order (see below)
settle_ms = 1500              # after the wait, before the picture (default 1000, at most 10000)

[shot.wait]                   # what fenix/state should say first; every one given must hold
project = true                # the project is the sample
file = "data/people.csv"      # the file in focus is this one
no_file = false               # or: no file in focus (a page or tool has it)
line = 12                     # the cursor's line, from 1
diagnostics = 1               # at least this many problems in `file` (fenix/diagnostics)
idle = true                   # nothing but the caret is being drawn
timeout_s = 30                # default 15, at most 120
```

A sample project needs a `README.md`: `fenix/openProject` opens it, and
without one it opens a file finder, which would be in the picture.

`commands` runs only what `fenix/runCommand` will run: Fenix's own
commands that are external, and an extension's marked `external = true`
in its `[[contributes.commands]]`. `validate` refuses one of an
extension here that isn't marked so, and one that names an extension's
namespace but isn't in its manifest; anything else is taken for Fenix's,
and checked against `fenix/commands` before the shot runs.

## Known not to work, or not tried

**None of this has run against a real Fenix yet.** It was written where
there was no Fenix and no display: the script checks, the control
channel client, the order of a shot's steps, waiting with a timeout and
error replies are tested against an in-process fake, and the package is
tested to leave the script out. Starting Fenix, the endpoint it really
listens on, the window's size and the capture itself are untried.

What to try first, on Windows:

1. `cargo xtask screenshots csv --fenix "<path to fenix.exe>"`, and
   watch. `target\screenshots\csv\fenix-1.log` has what Fenix printed.
2. If it says there's no control channel: the pipe it looks for is
   `\\.\pipe\fenix-<SID>-<session>-<hash of target\screenshots\csv\home-1>`;
   `[System.IO.Directory]::GetFiles('\\.\pipe\') | Select-String fenix`
   lists the ones there are.
3. If the picture is black or empty: the capture asks the window to draw
   itself (`PrintWindow` with `PW_RENDERFULLCONTENT`), which may not
   reach a GPU swapchain on every driver. The fallback (the screen
   under the window) runs only when `PrintWindow` fails outright; try
   making it the only way (in `CAPTURE_PS1`, `xtask/src/screenshots.rs`),
   with nothing over the window.
4. That the window came out the size asked (it's restored from the
   profile's `state/windows.json`) and in the theme asked (its
   `settings.toml`).
5. That no `fenix.exe` is left running afterwards: each is killed, since
   quitting isn't a command the channel will run.

On Linux it's best effort: Fenix is started without `WAYLAND_DISPLAY`
so that its window is an X window, found with `xdotool` and taken with
ImageMagick's `import` or with `maim`; without them it says so.

What the control channel can't do, and so neither can a script:

- **Most extension pages can't be reached.** `fenix/runCommand` runs only
  external commands, and no extension in the first pass marks any: the
  Regex playground, a diagram's preview, the Notebook page, Rust's
  package page and an HTTP response are all behind commands it refuses.
  The first pass is what opening a file shows (CSV, Logs, Hex, HTTP,
  Colours, Data tools). The rest wait for those commands to be marked
  external where that makes sense, or for the channel to take keys.
- **"Ready" is a guess.** `fenix/state` says the project, the file and
  the line, not which page is up or whether an extension has finished
  drawing it. `idle` reads `stats.frames` a poll apart; the caret
  blinks for 30 seconds after a file opens, so idle means "nothing more
  than the caret", and the caret may be on or off in the picture.
  `settle_ms` covers the rest.
- **No quit.** Fenix is killed after each picture.
- **Dependencies aren't installed** with the extension: an extension
  whose pictures need another one installed can't be scripted yet.
