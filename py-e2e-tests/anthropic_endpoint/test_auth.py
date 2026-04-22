import pytest
from anthropic import Anthropic, AuthenticationError

pytestmark = [pytest.mark.requires_server]


def test_invalid_token(client):
    import httpx

    bad_client = Anthropic(
        base_url=client.base_url,
        api_key="sk-wrong",
        http_client=httpx.Client(headers={"Authorization": "Bearer sk-wrong"}),
    )
    with pytest.raises(AuthenticationError) as exc_info:
        bad_client.messages.create(
            model="deepseek-default",
            max_tokens=1024,
            messages=[{"role": "user", "content": "你好"}],
        )

    assert exc_info.value.status_code == 401
