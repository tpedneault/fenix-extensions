# Arduino

Arduino sketches in Fenix, through `arduino-cli`: build, upload, the
serial monitor, debugging where the board allows it, boards, ports,
libraries and board packages, and `arduino-language-server` for
completion, which runs clangd against the sketch as the CLI builds it.

A sketch is a folder holding `<folder>.ino` (or a `sketch.yaml`). Inside
one, `SPC m` is its menu:

| Key | What it does |
| --- | --- |
| `b` | Build (verify); errors go to the quickfix list |
| `u` | Build and upload; the serial monitor steps aside, then comes back |
| `m` | The serial monitor, in a pane under yours |
| `B` | The monitor's speed |
| `p` `s` `o` | The port, the board, the board's options |
| `l` `c` | Install a library, or board support |
| `d` | Debug, if the board can be |
| `i` | The sketch's board, port, speed and tools |
| `n` | A new sketch (also `SPC f n`) |

With standard keys, `SPC` is `Ctrl+K`: the menu is `Ctrl+K m`, a new
sketch `Ctrl+K f n`.

The board and port live in `sketch.yaml`, where `arduino-cli` and the
Arduino IDE read them too; the monitor's speed in `.fenix/settings.toml`.

A build or an upload is a task in Fenix's task pane, so Fenix notifies
you when one ends while you're elsewhere, as `notifications.tasks` says
(by default, one that took 10 seconds or more), with "Run it again".

It needs Fenix 1.5 or later (extension API 1.5). What it says names the
keys of the key scheme you use, and yours where you rebound them: "the
default -- `C-k m s` picks your board" with standard keys, `SPC m s`
with Vim keys.

## Tools

It finds `arduino-cli`, `clangd` and `arduino-language-server` in
Fenix's tools folder, on `PATH`, or where the Arduino CLI installer and
an Arduino IDE 2 put them. `arduino.cli`, `arduino.clangd` and
`arduino.language_server` point it elsewhere. The project doctor says
what's missing, whether the board's core is installed and whether the
board is plugged in, with a fix for each.

## Permissions

It runs those three programs, and reads and writes the sketch's
`sketch.yaml` and `.fenix/settings.toml`. It sends nothing anywhere.
