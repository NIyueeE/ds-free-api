import pytest

pytestmark = [pytest.mark.requires_server]

MODEL = "deepseek-default"


def test_tool_call(client):
    """验证带 tools 的非流式请求能成功返回，且如果触发 tool_calls 则格式正确。"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[
            {"role": "user", "content": "请使用 get_weather 工具查询北京的天气。"}
        ],
        tools=[
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "获取指定城市的天气",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "city": {"type": "string", "description": "城市名称"}
                        },
                        "required": ["city"],
                    },
                },
            }
        ],
        stream=False,
    )

    assert resp.object == "chat.completion"
    assert resp.model == MODEL
    assert len(resp.choices) == 1

    msg = resp.choices[0].message
    # 如果模型返回了 tool_calls，则校验结构
    if msg.tool_calls:
        assert len(msg.tool_calls) > 0
        tc = msg.tool_calls[0]
        assert tc.type == "function"
        assert tc.function.name == "get_weather"
        assert "北京" in tc.function.arguments or "Beijing" in tc.function.arguments
    else:
        # 未触发工具时至少保证正常 content 存在，说明请求解析没坏
        assert msg.content


def test_tool_call_stream(client):
    """验证带 tools 的流式请求能正常结束，且能解析出 tool_calls。"""
    stream = client.chat.completions.create(
        model=MODEL,
        messages=[
            {"role": "user", "content": "请使用 get_weather 工具查询北京的天气。"}
        ],
        tools=[
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "获取指定城市的天气",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "city": {"type": "string"}
                        },
                        "required": ["city"],
                    },
                },
            }
        ],
        stream=True,
    )

    chunks = list(stream)
    assert chunks

    last = chunks[-1]
    assert last.choices[0].finish_reason in ("stop", "tool_calls")

    # 收集所有 delta 中的 tool_calls（流式可能分片返回）
    tool_calls = []
    for c in chunks:
        if c.choices and c.choices[0].delta.tool_calls:
            tool_calls.extend(c.choices[0].delta.tool_calls)

    if tool_calls:
        names = [tc.function.name for tc in tool_calls if tc.function and tc.function.name]
        assert "get_weather" in names
    else:
        # 未触发工具时保证有正常 content
        content = "".join(
            (c.choices[0].delta.content or "")
            for c in chunks if c.choices
        )
        assert content


def test_tool_call_with_leading_text(client):
    """验证模型输出前导文本后接 tool_calls 的场景。"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[
            {"role": "user", "content": "帮我查一下北京的天气，用 get_weather 工具。"}
        ],
        tools=[
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "parameters": {
                        "type": "object",
                        "properties": {"city": {"type": "string"}},
                    },
                },
            }
        ],
        tool_choice="required",
        stream=False,
    )

    assert resp.choices[0].finish_reason == "tool_calls"
    assert resp.choices[0].message.tool_calls
    assert resp.choices[0].message.tool_calls[0].function.name == "get_weather"


def test_custom_tool_grammar(client):
    """验证自定义工具（grammar 格式）的请求能正常解析不报错。"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "生成一个符合语法规则的字符串"}],
        tools=[
            {
                "type": "custom",
                "custom": {
                    "name": "grammar_tool",
                    "description": "基于语法的工具",
                    "format": {
                        "type": "grammar",
                        "grammar": {
                            "definition": "start: word+",
                            "syntax": "lark",
                        },
                    },
                },
            }
        ],
        stream=False,
    )
    # 自定义工具目前只是 prompt 注入，响应应为正常 content 或 tool_calls
    assert resp.choices[0].message.content or resp.choices[0].message.tool_calls
