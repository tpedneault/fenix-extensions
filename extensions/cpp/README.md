# C/C++

C and C++ in Fenix: clangd as the language server, with the build's
`compile_commands.json`; debugging with lldb-dap or GDB; CMake's presets
as tasks; clang-format as the formatter and clang-tidy on save; the
project doctor's checks with their fixes; vcpkg's and Conan's packages;
snippets; and vcpkg and Conan projects in the new-project wizard. Built
on lang-kit (`crates/lang-kit`), as each language pack is.

It's for a folder with a `CMakeLists.txt`, CMake presets, a
`compile_commands.json`, a `.clangd`, a `vcpkg.json` or a conanfile, or
C or C++ files at the top or in `src/`. An Arduino sketch (a `.ino`) is
left to the Arduino extension.

## The language server

`clangd`, found on `PATH` or in LLVM's folders (`/usr/lib/llvm-N/bin`,
Homebrew's, `C:\Program Files\LLVM\bin`). clangd finds a
`compile_commands.json` in the project or its `build/` itself; one in a
preset's folder (`build/debug`) is given to it with
`--compile-commands-dir`, unless `.clangd` says where. Until the build
writes one, the pack looks for it every few seconds, and clangd is
started again when it's there. Without clangd, Fenix's own `clangd` is
started as before, and the doctor's fix row installs it.

## The build

The configure preset used is `cpp.preset`, else the one called `debug`,
else the first (one with no `condition` first). Presets are read from
`CMakePresets.json` and `CMakeUserPresets.json`, with their `include`s
(Conan's, which name its generated presets) and `inherits`, the earlier
parent winning as CMake has it.

`SPC p t`:

| | with presets | without |
|---|---|---|
| configure | `cmake --preset debug` | `cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug [-G Ninja]` |
| build | `cmake --build --preset debug` (its folder without a build preset) | `cmake --build build` |
| test | `ctest --preset debug` (`ctest --test-dir` without a test preset) | `ctest --test-dir build --output-on-failure` |
| clean | `--target clean` | the same |
| configure/build/test `<preset>` | each other preset's | |
| run `<target>` | each program the build makes, by `cmake -E env` | the same |
| conan install | `conan install . --build=missing -s build_type=Debug` | into `build` without a layout |

Configuring asks for `-DCMAKE_EXPORT_COMPILE_COMMANDS=ON` when the
preset doesn't. Without presets, vcpkg's toolchain is given in a vcpkg
project (`VCPKG_ROOT` is where vcpkg is), and Conan's in a Conan project
without a layout. A task of the same name in `.fenix/tools.json` wins.

The pack leaves CMake's file API query in each build folder
(`.cmake/api/v1/query/client-fenix/codemodel-v2`), so the next configure
writes its code model: each target's program, by its full path.

## Debugging

The adapter is `lldb-dap` (`lldb-vscode` before LLVM 18), else `gdb
--interpreter=dap` -- GDB 14 or later -- as `cpp.debugger` prefers.
Both speak DAP over stdio; both were run here with what Fenix sends
(a program, its arguments, folder and environment): breakpoints, the
stack, continuing to the end.

`SPC u u`, in a project with no launch configuration of its own, offers
what CMake's code model says the build makes, once it has configured:

| | |
|---|---|
| CMake: *target* | each program, `build <target>` first (`cmake --build ... --target <target>`) |
| C/C++: a CMake target, picked | the one you pick as it starts (`${cmakeTarget}`), the preset's `build` first |
| C/C++: attach to a process | a program running here, picked as it starts (`pid`) |

On Linux, attaching needs ptrace: `kernel.yama.ptrace_scope` at 0, or
the debugger run as root.

## Formatting and linting

`SPC c F` -- and saving, with `editor.format_on_save` -- runs
`clang-format --assume-filename=<file>` on the buffer: the project's
`.clang-format`, else LLVM's style.

When a source file is saved and the build has a `compile_commands.json`,
`clang-tidy -p <its folder> --quiet <file>` runs on it, and its warnings
and errors are shown with clangd's, each with its check, its notes as
hints. What clang-tidy can fix (`--export-fixes`, written beside the
database and read back) `SPC c a` offers on its line. The project's `.clang-tidy` says the checks; `cpp.tidy_on_save`
turns it off. Headers aren't linted on their own: the database has no
command for them.

## The project doctor

- **Compiler**: the first of `c++`, `cc`, `g++`, `gcc`, `clang++`,
  `clang`, `cl`. Fix: Apple's command line tools (`xcode-select
  --install`) on macOS, Visual Studio's build tools (winget) on Windows.
- **CMake**, held to `cmake_minimum_required` and to what the presets'
  schema version needs, and **Ninja**, needed when the preset uses it --
  and on Windows without a preset, where CMake would otherwise make a
  Visual Studio solution, which writes no `compile_commands.json`.
- **clangd**, **clang-format**, **clang-tidy**.
- **Debugger**: lldb-dap (only found), else GDB, 14 at least. Fix: LLVM
  from Homebrew or winget.
- **vcpkg** and **Conan** when the project uses them, and Conan's
  default profile (fix: `conan profile detect`).
- **compile_commands.json**, where the preset builds. Fix: configure.

CMake, Ninja, clangd, clang-format, clang-tidy and Conan are installed
from PyPI: `uv tool install`, else `pipx install`, else `pip install
--user`. A fix that installs something is checked again, and what it
installed is used at once.

## Packages

`SPC m p` (*C/C++: packages*): vcpkg's when there's a `vcpkg.json`,
else Conan's.

| | vcpkg | Conan |
|---|---|---|
| list | `vcpkg list --x-json` (the build's `vcpkg_installed`) | `conan graph info . --format=json` |
| outdated | `vcpkg search --x-json`: the newest in vcpkg's tree | `conan graph outdated . --format=json` |
| vulnerable | -- | `conan audit scan . --format=json` |
| `u` | -- | within the ranges: `conan lock upgrade --update-requires` with a `conan.lock`, else `conan install --update=name` |
| `U` | `vcpkg x-update-baseline` | the same, for all |
| `a` | `vcpkg add port name` | `conan install --requires=name/version`: into Conan's cache |
| `d` | -- | -- |

What vcpkg's and Conan's command lines can't do: vcpkg can't take a port
out of `vcpkg.json` or move one up alone (the baseline says every
version; an `overrides` entry, by hand, would), and has no audit. Conan
can't add a requirement to a conanfile or take one out, and a pinned
version (`fmt/10.2.1`) moves only when you change it; `conan audit`
needs a token from conan.io (`conan audit provider auth`), and says so
on the page until it has one.

## Snippets

C++: `guard`, `once`, `cls` (the rule of five), `st`, `raii` (a
move-only handle), `uptr`, `forr` (range-for), `forkv` (a map's keys and
values), `fori`, `main`, `mainv` (its arguments as a vector), `ns`,
`enumc`, `tmpl`, `lam`, `try`, `test` (GoogleTest), `tcase` (Catch2).
C: `guard`, `main`, `mainv`, `fori`, `st`, `sw`.

## Project templates

Fenix's wizard already has a CMake project (an executable, a static or
shared library, with GoogleTest, Catch2 or CTest); this adds:

- **C++ · CMake with vcpkg**: `vcpkg.json` with fmt (and Catch2),
  presets with vcpkg's toolchain, and the manifest's baseline set.
- **C++ · CMake with Conan**: `conanfile.txt` with fmt (and Catch2),
  `cmake_layout`, a profile made if there's none, and Conan's presets.

## Files

`*.cppm`, `*.ccm`, `*.cxxm`, `*.ixx`, `*.mpp` (modules), and `*.ipp`,
`*.tpp`, `*.txx`, `*.tcc` are read as C++, `//` its line comment.

## Keys

After `SPC m` in a C or C++ project: `p` packages, `d` check again, `c`
configure.
