from django.test import TestCase
from django.urls import reverse


class HomeTests(TestCase):
    def test_the_home_page_says_its_name(self) -> None:
        response = self.client.get(reverse("{{app}}:home"))
        self.assertEqual(response.status_code, 200)
        self.assertContains(response, "{{name}}")
