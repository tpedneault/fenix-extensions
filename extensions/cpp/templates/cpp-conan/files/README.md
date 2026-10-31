# {{name}}

A CMake program whose libraries Conan brings (`conanfile.txt`).

```
conan install . --build=missing -s build_type=Debug   # the libraries, and CMake presets for them
cmake --preset conan-debug                             # configure (into build/Debug)
cmake --build --preset conan-debug                     # build
ctest --preset conan-debug                             # test
```

Another library: add it under `[requires]` in `conanfile.txt`
(`conan search "spdlog*" -r conancenter` says its versions), run
`conan install` again, and `find_package` it in `CMakeLists.txt`.
