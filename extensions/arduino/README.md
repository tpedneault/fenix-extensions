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

The board and port live in `sketch.yaml`, where `arduino-cli` and the
Arduino IDE read them too; the monitor's speed in `.fenix/settings.toml`.

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
