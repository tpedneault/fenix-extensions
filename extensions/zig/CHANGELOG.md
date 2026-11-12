# Changelog

## 1.1.0

- For Fenix 1.5 (extension API 1.5): how installing Zig or ZLS ended is a
  notification rather than a line in the message line, since the
  download can take a while and you may be elsewhere -- "Installed ZLS
  0.17.0", and what it's now used as; or "Couldn't install ZLS 0.17.0",
  saying whether its download, unpacking it or setting it up failed and
  why, with *Try again*. The newest for a tool takes the last one's
  place, and a failure's is taken away when you try again. Starting an
  install is still said in the message line. The README gives standard
  keys beside Vim's.

## 1.0.0

- The first release, for Fenix 1.0: zig: its grammar, ZLS, zig fmt, zig ast-check on save, zig build's steps as tasks, the project's checks with fixes that install Zig and ZLS, and snippets.
