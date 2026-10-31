# Samples

What the tests read:

- `orbit.zig`: Zig the grammar's queries run on.
- `build.zig.zon`: a package's manifest as `zig init` writes it (Zig
  0.14 and later: `.name` an enum literal, a `.fingerprint`), with a
  dependency whose `.version` is a level down.
- `zig-build-l.txt`: `zig build -l` (`--list-steps`) for a project with
  two steps of its own beside `zig init`'s.
- `ast-check.txt`: `zig ast-check`'s stderr for a file with a `var`
  never changed and a missing `;`, and a line about another file.

The last two are written in the formats Zig 0.17 prints, not captured
from a run.
