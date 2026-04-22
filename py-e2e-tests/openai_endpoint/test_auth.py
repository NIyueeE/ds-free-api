import pytest
from openai import APIError

pytestmark = [pytest.mark.requires_server]


def test_invalid_token(client):
    from openai import OpenAI

    bad_client = OpenAI(base_url=client.base_url, api_key="sk-wrong")
    with pytest.raises(APIError) as exc_info:
        bad_client.chat.completions.create(
            model="deepseek-default",
            messages=[{"role": "user", "content": "你好"}],
        )

    assert exc_info.value.status_code == 401
