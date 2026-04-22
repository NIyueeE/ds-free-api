import pytest

pytestmark = [pytest.mark.requires_server]

MODEL = "deepseek-default"


# =============================================================================
# 基础功能
# =============================================================================


def test_non_stream_basic(client):
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        messages=[{"role": "user", "content": "你好"}],
    )

    assert msg.type == "message"
    assert msg.role == "assistant"
    assert msg.model == MODEL
    assert msg.content
    assert msg.usage.input_tokens > 0
    assert msg.usage.output_tokens > 0


def test_stream_basic(client):
    with client.messages.stream(
        model=MODEL,
        max_tokens=1024,
        messages=[{"role": "user", "content": "你好"}],
    ) as stream:
        events = list(stream)

    assert events

    text_parts = []
    for event in events:
        if event.type == "content_block_delta":
            if hasattr(event.delta, "text"):
                text_parts.append(event.delta.text)

    assert "".join(text_parts)


# =============================================================================
# 能力开关
# =============================================================================


def test_thinking_enabled(client):
    """thinking=enabled 显式开启深度思考（默认行为）"""
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        messages=[{"role": "user", "content": "1+1="}],
        thinking={"type": "enabled", "budget_tokens": 2048},
    )
    assert msg.content


def test_thinking_disabled(client):
    """thinking=disabled 关闭深度思考"""
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        messages=[{"role": "user", "content": "1+1="}],
        thinking={"type": "disabled"},
    )
    assert msg.content


# =============================================================================
# 消息格式
# =============================================================================


def test_system_message(client):
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        system="你是一个数学助手，只回答数字。",
        messages=[{"role": "user", "content": "2+3="}],
    )
    assert msg.content


def test_system_as_blocks(client):
    """system 参数为文本块数组时应兼容解析"""
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        system=[{"type": "text", "text": "用中文回答。"}],
        messages=[{"role": "user", "content": "hello"}],
    )
    assert msg.content


def test_multimodal_user(client):
    """多模态消息（image / document 等）应能正常解析不报错"""
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        messages=[
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "描述一下图片内容"},
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": "iVBORw0KGgo=",
                        },
                    },
                ],
            }
        ],
    )
    assert msg.content


def test_assistant_with_tool_use_history(client):
    """assistant 消息携带 tool_use 历史应能正常解析"""
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        messages=[
            {"role": "user", "content": "查北京天气"},
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_abc",
                        "name": "get_weather",
                        "input": {"city": "北京"},
                    }
                ],
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_abc",
                        "content": "晴，25°C",
                    }
                ],
            },
            {"role": "user", "content": "谢谢"},
        ],
    )
    assert msg.content


# =============================================================================
# Stop 序列
# =============================================================================


def test_stop_sequences(client):
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        messages=[{"role": "user", "content": "请输出 ABCDEFG"}],
        stop_sequences=["D"],
    )
    assert msg.stop_reason == "end_turn" or msg.stop_reason == "stop_sequence"
    content_text = "".join(
        block.text for block in msg.content if hasattr(block, "text")
    )
    assert "D" not in content_text


def test_stop_multiple_sequences(client):
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        messages=[{"role": "user", "content": "请输出 ABCDEFG"}],
        stop_sequences=["D", "E"],
    )
    assert msg.stop_reason in ("end_turn", "stop_sequence")


# =============================================================================
# 解析但忽略的字段（不应报错）
# =============================================================================


def test_ignored_params(client):
    """
    传入大量适配器解析但不消费的字段，验证请求能正常完成不报错。
    这些字段包括：temperature, top_p, top_k, metadata。
    """
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        messages=[{"role": "user", "content": "你好"}],
        temperature=0.5,
        top_p=0.9,
        top_k=40,
        metadata={"user_id": "test-user"},
    )
    assert msg.type == "message"
    assert msg.role == "assistant"
    assert msg.content
