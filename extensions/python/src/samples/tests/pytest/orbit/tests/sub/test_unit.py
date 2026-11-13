import unittest


class TestUnit(unittest.TestCase):
    def test_ok(self):
        self.assertEqual(1, 1)

    def test_bad(self):
        self.assertEqual(1, 2)
