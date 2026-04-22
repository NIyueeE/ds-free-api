import pytest

pytestmark = [pytest.mark.requires_server]

MODEL = "deepseek-default"


def test_tool_call(client):
    """验证带 tools 的非流式请求能成功返回，且如果触发 tool_use 则格式正确。"""
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        messages=[
            {"role": "user", "content": "请使用 get_weather 工具查询北京的天气。"}
        ],
        tools=[
            {
                "type": "custom",
                "name": "get_weather",
                "description": "获取指定城市的天气",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "city": {"type": "string", "description": "城市名称"}
                    },
                    "required": ["city"],
                },
            }
        ],
    )

    assert msg.type == "message"
    assert msg.model == MODEL

    tool_use_blocks = [b for b in msg.content if b.type == "tool_use"]
    if tool_use_blocks:
        assert len(tool_use_blocks) > 0
        tb = tool_use_blocks[0]
        assert tb.name == "get_weather"
        assert "北京" in str(tb.input) or "Beijing" in str(tb.input)
    else:
        text = "".join(
            b.text for b in msg.content if b.type == "text"
        )
        assert text


def test_tool_call_stream(client):
    """验证带 tools 的流式请求能正常结束。"""
    with client.messages.stream(
        model=MODEL,
        max_tokens=1024,
        messages=[
            {"role": "user", "content": "请使用 get_weather 工具查询北京的天气。"}
        ],
        tools=[
            {
                "type": "custom",
                "name": "get_weather",
                "description": "获取指定城市的天气",
                "input_schema": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                },
            }
        ],
    ) as stream:
        events = list(stream)

    assert events

    # 收集所有 tool_use block
    tool_blocks = []
    for event in events:
        if event.type == "content_block_start" and event.content_block.type == "tool_use":
            tool_blocks.append(event.content_block)

    if tool_blocks:
        names = [tb.name for tb in tool_blocks]
        assert "get_weather" in names
    else:
        # 未触发工具时保证有正常文本
        text_parts = []
        for event in events:
            if event.type == "content_block_delta" and hasattr(event.delta, "text"):
                text_parts.append(event.delta.text)
        assert "".join(text_parts)


def test_tool_choice_required(client):
    """tool_choice=any 映射为 required，应触发 tool_use"""
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        messages=[{"role": "user", "content": "查北京天气"}],
        tools=[
            {
                "type": "custom",
                "name": "get_weather",
                "description": "获取天气",
                "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}},
            }
        ],
        tool_choice={"type": "any"},
    )
    tool_use_blocks = [b for b in msg.content if b.type == "tool_use"]
    assert tool_use_blocks
    assert msg.stop_reason == "tool_use"


def test_tool_choice_none_ignores_tools(client):
    """tool_choice=none 不应触发 tool_use"""
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        messages=[{"role": "user", "content": "你好"}],
        tools=[
            {
                "type": "custom",
                "name": "get_weather",
                "input_schema": {},
            }
        ],
        tool_choice={"type": "none"},
    )
    tool_use_blocks = [b for b in msg.content if b.type == "tool_use"]
    assert not tool_use_blocks
    text = "".join(b.text for b in msg.content if b.type == "text")
    assert text


def test_disable_parallel_tool_use(client):
    """disable_parallel_tool_use 映射为 parallel_tool_calls=false"""
    msg = client.messages.create(
        model=MODEL,
        max_tokens=1024,
        messages=[{"role": "user", "content": "同时查北京和上海天气"}],
        tools=[
            {
                "type": "custom",
                "name": "get_weather",
                "input_schema": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                },
            }
        ],
        tool_choice={"type": "auto", "disable_parallel_tool_use": True},
    )
    assert msg.content
    assert msg.stop_reason in ("end_turn", "tool_use")
