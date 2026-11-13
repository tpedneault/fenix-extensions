import sys
import pytest
from orbit import add


def test_adds():
    assert add(1, 2) == 3


def test_fails_on_purpose():
    print("some output before the failure")
    assert add(2, 2) == 5, "two and two"


@pytest.mark.skip(reason="slow")
def test_skipped():
    pass


@pytest.mark.xfail(reason="known bug")
def test_known_bug():
    assert False


@pytest.mark.parametrize("a,b,want", [(1, 1, 2), (-1, -1, -3), ("x", "y", "xy")], ids=["small", "negative", "strings [odd] id"])
def test_cases(a, b, want):
    assert add(a, b) == want


class TestOuter:
    def test_in_class(self):
        assert True

    class TestInner:
        def test_deep(self):
            assert add(0, 0) == 1

    @pytest.mark.parametrize("n", [1, 2])
    def test_param_in_class(self, n):
        assert n


@pytest.fixture
def broken():
    raise RuntimeError("fixture broke")


def test_uses_broken_fixture(broken):
    pass
