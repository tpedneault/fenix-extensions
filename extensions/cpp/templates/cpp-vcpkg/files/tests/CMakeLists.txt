find_package(Catch2 3 CONFIG REQUIRED)
include(Catch)

add_executable({{name_snake}}_tests test_main.cpp)
target_link_libraries({{name_snake}}_tests PRIVATE fmt::fmt Catch2::Catch2WithMain)
catch_discover_tests({{name_snake}}_tests)
