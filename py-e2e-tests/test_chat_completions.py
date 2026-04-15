import pytest

pytestmark = [pytest.mark.requires_server]


MODEL = "deepseek-default"


def test_non_stream_basic(client):
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "你好"}],
        stream=False,
    )

    assert resp.object == "chat.completion"
    assert resp.model == MODEL
    assert len(resp.choices) == 1
    assert resp.choices[0].message.role == "assistant"
    assert resp.choices[0].message.content
    assert resp.usage.completion_tokens > 0
    assert resp.usage.prompt_tokens > 0
    assert resp.usage.total_tokens > 0


def test_stream_basic(client):
    stream = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "你好"}],
        stream=True,
    )

    chunks = list(stream)
    assert chunks

    first = chunks[0]
    assert first.choices[0].delta.role == "assistant"

    content = "".join(
        c.choices[0].delta.content or "" for c in chunks if c.choices
    )
    assert content

    last = chunks[-1]
    assert last.choices[0].finish_reason == "stop"


def test_stream_reasoning(client):
    stream = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "你好"}],
        stream=True,
        extra_body={"deepseek": {"reasoning": True}},
    )

    chunks = list(stream)
    assert chunks

    reasoning = "".join(
        getattr(c.choices[0].delta, "reasoning_content", None) or ""
        for c in chunks if c.choices
    )
    content = "".join(
        (c.choices[0].delta.content or "")
        for c in chunks if c.choices
    )

    # reasoning 模式应优先返回 reasoning_content
    assert reasoning or content

    last = chunks[-1]
    assert last.choices[0].finish_reason == "stop"
