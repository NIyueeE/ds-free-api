import pytest

pytestmark = [pytest.mark.requires_server]


def test_list_models(client):
    models = client.models.list()
    ids = [m.id for m in models.data]
    assert "deepseek-default" in ids


def test_get_model(client):
    model = client.models.retrieve("deepseek-default")
    assert model.id == "deepseek-default"
    assert model.type == "model"
