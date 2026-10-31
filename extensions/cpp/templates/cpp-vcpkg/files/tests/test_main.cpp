#include <catch2/catch_test_macros.hpp>
#include <fmt/format.h>

TEST_CASE("fmt formats", "[fmt]") {
    REQUIRE(fmt::format("{}-{}", 1, 2) == "1-2");
}
