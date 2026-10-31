# Zig

Zig in Fenix: its grammar (Fenix has none of its own), ZLS, `zig fmt`,
`zig ast-check` on save, `zig build`'s steps as tasks, the project
doctor's checks with fixes that install Zig and ZLS, and snippets. Built
on lang-kit (`crates/lang-kit`), as each language pack is.

It starts with Fenix. ZLS is set for Zig files whenever it's found; the
doctor checks Zig and ZLS in every project.

## The grammar

`*.zig` and `*.zon` files are read with tree-sitter-zig 1.1.2's grammar
(`grammars/`, MIT): highlighted, folded (functions, tests, structs,
enums, unions, `switch`es), with Enter indenting inside a block and the
breadcrumbs and `SPC c s` listing the declarations. `//` comments a line.

## The server

ZLS (`zls`), the Zig language server: completion, go to definition,
hover, references, rename, inlay hints, and its own problems. It's told
the Zig the pack found (`zig_exe_path`), so a Zig in the pack's tools
folder, not on `PATH`, is the one it reads the standard library and
`build.zig` with.

A ZLS knows the Zig of its own version: ZLS 0.17 for Zig 0.17. The
doctor says when the two don't go together.

## Formatting and checks

- `SPC c F`, and saving with `editor.format_on_save`: `zig fmt --stdin`.
- On save: `zig ast-check` on the file (`zig.ast_check`), in a few
  milliseconds -- syntax errors, unused locals and parameters, a `var`
  never changed, shadowing -- shown with ZLS's problems. What needs a
  build (types, comptime) is ZLS's, or `zig build`'s.

## Tasks

`SPC p t`:

| | |
|---|---|
| `build` | `zig build` |
| `build <step>` | `zig build <step>`, for each step `zig build -l` lists (`test`, `run` and the project's own); `test` and `run` until it has answered |
| `run <file>`, `test <file>` | `zig run` and `zig test` on each Zig file at the top of a project without `build.zig` (`run` for one with a `pub fn main`) |
| `fmt` | `zig fmt .` |

`zig build -l` runs the project's `build.zig`, so it runs in the
background when the project opens and when `build.zig` or
`build.zig.zon` is saved (`SPC m s` runs it again). A `build.zig` that
doesn't build keeps the steps it had; the output page says why.
`zig.build_steps` off leaves `build`, `build test` and `build run`. The
project's `.env` goes to each task.

## The doctor

| Check | | Fix row |
|---|---|---|
| Zig | `zig version`, held to `build.zig.zon`'s `minimum_zig_version`; needed | Zig 0.17.0 (Linux, macOS 13 and later) |
| ZLS | `zls --version`, held to Zig's version | ZLS 0.17.0 (Linux, macOS, Windows; x86_64 and arm64) |

ZLS's release comes from GitHub; Zig's from the wheels Zig's own project
puts on PyPI (`pip install ziglang`, https://codeberg.org/ziglang/zig-pypi),
the same Zig as ziglang.org's. Each is checked against the SHA-256 the
pack knows, unpacked into the pack's tools folder, where only the pack
looks, and used at once. One you installed yourself is used instead: the
pack looks on `PATH`, and in zvm's, zigup's and anyzig's folders,
Homebrew's, Snap's and winget's. On Windows, Zig comes from
https://ziglang.org/download/ or `winget install zig.zig`.

## Snippets

| Key | |
|---|---|
| `main` | `std` imported, `pub fn main() !void` |
| `fn`, `pfn` | a function, a public one |
| `test`, `teq` | a test block; `std.testing.expectEqual` |
| `struct`, `enum`, `union`, `err` | a struct with `init`, an enum, a tagged `union(enum)`, an error set |
| `sw`, `for`, `while` | `switch`; `for` with its index (`0..`); `while` with its continue expression |
| `errdefer`, `catch` | `errdefer`; `catch \|err\|`, logged and returned |
| `gpa`, `list` | a `DebugAllocator` and its leak check; an `ArrayList` given its allocator |
| `print` | `std.debug.print` |
| `gen` | a generic type: a function of a `comptime T: type` |
| `imp` | `@import` |

## Settings

| Setting | Default | |
|---|---|---|
| `zig.ast_check` | on | `zig ast-check` on a saved file |
| `zig.build_steps` | on | `zig build -l`'s steps as tasks |

## Keys

After `SPC m` in a Zig project or file: `d` checks the project again, `s`
lists `zig build`'s steps again.

## Not yet

- **Debugging**: a Zig program debugs with lldb-dap or GDB, as C's
  does; the pack doesn't set an adapter or launches for `zig build`'s
  artifacts yet.
- **ZON**: the grammar has no ZON of its own; `build.zig.zon` reads as
  Zig, which colours it all.
