import sys
import pytest
from orbit import add


@pytest.fixture
def leaky():
    yield
    raise RuntimeError("teardown broke")


def test_teardown_fails(leaky):
    assert add(1, 1) == 2


@pytest.mark.xfail(strict=True, reason="fixed already")
def test_strict_xpass():
    assert add(1, 1) == 2


@pytest.mark.xfail(reason="flaky")
def test_xpass():
    assert add(1, 1) == 2


def test_名前():
    assert add(1, 1) == 2


@pytest.mark.skipif(sys.platform != "nowhere", reason="not on this platform")
def test_skipif():
    pass


def test_skips_inside():
    pytest.skip("not today")


@pytest.mark.parametrize("s", [pytest.param("long", id="y" * 1100), "short"])
def test_long_id(s):
    assert s


@pytest.mark.parametrize("a", [1, 2])
@pytest.mark.parametrize("b", [3, 4])
def test_stacked(a, b):
    assert a < b


@pytest.mark.parametrize("n", [pytest.param(1, marks=pytest.mark.skip(reason="later")), 2])
def test_marked_case(n):
    assert n


@pytest.fixture(params=[10, 20])
def num(request):
    return request.param


def test_fixture_param(num):
    assert num
