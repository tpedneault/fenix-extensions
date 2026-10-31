# {{name}}

A CMake program whose libraries vcpkg brings (`vcpkg.json`). The
presets use vcpkg's toolchain, from `VCPKG_ROOT`.

```
cmake --preset debug          # configure (vcpkg installs the libraries into build/debug)
cmake --build --preset debug  # build
ctest --preset debug          # test
vcpkg add port spdlog         # another library; find_package it in CMakeLists.txt
```
