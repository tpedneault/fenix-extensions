# Changelog

## 1.3.0

- Fenix 1.6 and later also take a project with a `CMakePresets.json`, `compile_commands.json`, `vcpkg.json`, `conanfile.txt`, `conanfile.py` or `.clangd` at its top for a C/C++ project (`C++`), as one with a `CMakeLists.txt` already is, in the project hub, on Home, in the modeline and in the doctor. Nothing else about the kind changes. Fenix's own markers come first, so a folder with a `Cargo.toml`, `package.json` or `pyproject.toml` stays Rust, JS or Python. Your home folder is never a project, whatever dotfiles it holds (`.clangd` among them). Fenix 1.5 ignores this and the pack works there as before: it's only data in the manifest, so it needs no newer Fenix and no newer API.

## 1.2.0

- For Fenix 1.5 (extension API 1.5): built against API 1.5, as the
  shared kit it's built on now is. Nothing you see changed: this pack
  installs its tools (a compiler, CMake, Ninja, clangd and the rest) with
  the fixes the doctor offers, which run as tasks, so it has no download
  of its own to say the end of in a notification. On Fenix 1.0 to 1.4,
  1.1.0 stays the one you have. The README gives standard keys beside
  Vim's.

## 1.1.0

- `cpp.packages` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.0: c and C++: clangd with the build's compile_commands.json, debugging each CMake target with lldb-dap or GDB, CMake's presets as tasks, clang-format, clang-tidy on save with its fixes, the project's checks with their fixes, vcpkg and Conan packages, snippets, and vcpkg and Conan projects.
