cmake_minimum_required(VERSION 3.25)
project({{name_snake}} VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD {{cxx_standard}})
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# vcpkg's toolchain (the presets name it) installs what vcpkg.json asks
# for when CMake configures.
find_package(fmt CONFIG REQUIRED)

add_executable({{name_snake}} src/main.cpp)
target_link_libraries({{name_snake}} PRIVATE fmt::fmt)
if(MSVC)
    target_compile_options({{name_snake}} PRIVATE /W4)
else()
    target_compile_options({{name_snake}} PRIVATE -Wall -Wextra -Wpedantic)
endif()
{{#if tests == Catch2}}

enable_testing()
add_subdirectory(tests)
{{/if}}
