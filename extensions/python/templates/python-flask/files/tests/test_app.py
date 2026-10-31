import pytest

from {{name_snake}} import create_app


@pytest.fixture
def client():
    return create_app({"TESTING": True}).test_client()


def test_home_says_its_name(client):
    response = client.get("/")
    assert response.status_code == 200
    assert b"{{name}}" in response.data


def test_health(client):
    assert client.get("/health").get_json() == {"status": "ok"}
