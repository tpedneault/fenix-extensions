# Changelog

## 1.1.1

- A board or port written into `sketch.yaml` outside its pickers -- by
  hand, by the Arduino IDE, or by another tool -- is built and uploaded
  for: `sketch.yaml` is read again when it's saved, and when you move
  within the sketch. Before, the build and upload tasks kept the board
  they had when you came into the sketch, `arduino:avr:uno` if it had
  none, and overrode the board `sketch.yaml` named.

## 1.1.0

- For Fenix 1.5 (extension API 1.5): what it says names the keys of the
  key scheme in use, and yours where you rebound them -- the board, the
  port, the monitor's speed and the board's options as `C-k m s`,
  `C-k m p`, `C-k m B` and `C-k m o` with standard keys and `SPC m s` and
  the rest with Vim keys, and a new sketch as `C-k f n` or `SPC f n`.

## 1.0.0

- The first release, for Fenix 1.0: arduino sketches: build, upload, the serial monitor, debugging, boards, ports, libraries, and the right language server.
