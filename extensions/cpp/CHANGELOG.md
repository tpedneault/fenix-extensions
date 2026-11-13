# Changelog

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
