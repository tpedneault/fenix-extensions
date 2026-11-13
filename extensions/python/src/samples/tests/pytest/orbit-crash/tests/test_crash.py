import os


def test_before():
    assert True


def test_exits():
    print("about to exit")
    os._exit(7)


def test_after():
    assert True
