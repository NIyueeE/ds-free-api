import pytest

pytestmark = [pytest.mark.requires_server]

MODEL = "deepseek-default"


# =============================================================================
# 基础功能
# =============================================================================

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


# =============================================================================
# 能力开关
# =============================================================================

def test_reasoning_effort_high(client):
    """reasoning_effort=high 显式开启深度思考（默认行为）"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "1+1="}],
        reasoning_effort="high",
        stream=False,
    )
    assert resp.choices[0].message.content


def test_reasoning_effort_none(client):
    """reasoning_effort=none 关闭深度思考"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "1+1="}],
        reasoning_effort="none",
        stream=False,
    )
    assert resp.choices[0].message.content


def test_web_search_enabled(client):
    """web_search_options 开启智能搜索"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "今天有什么新闻"}],
        web_search_options={"search_context_size": "high"},
        stream=False,
    )
    assert resp.choices[0].message.content


# =============================================================================
# 消息格式
# =============================================================================

def test_system_message(client):
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[
            {"role": "system", "content": "你是一个数学助手，只回答数字。"},
            {"role": "user", "content": "2+3="},
        ],
        stream=False,
    )
    assert resp.choices[0].message.content


def test_developer_message(client):
    """developer 角色作为 system 的替代，适配器应兼容解析"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[
            {"role": "developer", "content": "用中文回答。"},
            {"role": "user", "content": "hello"},
        ],
        stream=False,
    )
    assert resp.choices[0].message.content


def test_multimodal_user(client):
    """多模态消息（image_url / input_audio / file）应能正常解析不报错"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "描述一下图片内容"},
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "data:image/png;base64,iVBORw0KGgo=",
                            "detail": "high",
                        },
                    },
                    {"type": "input_audio", "input_audio": {"data": "base64...", "format": "mp3"}},
                    {"type": "file", "file": {"filename": "report.pdf"}},
                ],
            }
        ],
        stream=False,
    )
    assert resp.choices[0].message.content


def test_assistant_with_tool_calls_history(client):
    """assistant 消息携带 tool_calls 历史应能正常解析"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[
            {"role": "user", "content": "查北京天气"},
            {
                "role": "assistant",
                "content": None,
                "tool_calls": [
                    {
                        "id": "call_abc",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": '{"city":"北京"}'},
                    }
                ],
            },
            {"role": "tool", "tool_call_id": "call_abc", "content": "晴，25°C"},
            {"role": "user", "content": "谢谢"},
        ],
        stream=False,
    )
    assert resp.choices[0].message.content


def test_function_message_legacy(client):
    """已弃用的 function 角色应兼容解析"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[
            {"role": "user", "content": "计算"},
            {"role": "function", "name": "calc", "content": "42"},
        ],
        stream=False,
    )
    assert resp.choices[0].message.content


# =============================================================================
# Stop 序列
# =============================================================================

def test_stop_single_string(client):
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "请输出 ABCDEFG"}],
        stop="D",
        stream=False,
    )
    assert resp.choices[0].finish_reason == "stop"
    assert "D" not in resp.choices[0].message.content


def test_stop_multiple_strings(client):
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "请输出 ABCDEFG"}],
        stop=["D", "E"],
        stream=False,
    )
    assert resp.choices[0].finish_reason == "stop"


# =============================================================================
# Stream 选项
# =============================================================================

def test_stream_include_usage(client):
    stream = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "你好"}],
        stream=True,
        stream_options={"include_usage": True},
    )

    chunks = list(stream)
    assert chunks

    # 至少有一个 chunk 包含 usage 信息
    usage_chunks = [c for c in chunks if c.usage]
    assert len(usage_chunks) >= 1

    # 所有含 choices 的 chunk 中，最后一个有 finish_reason
    finish_chunks = [c for c in chunks if c.choices and c.choices[0].finish_reason]
    assert finish_chunks
    assert finish_chunks[-1].choices[0].finish_reason == "stop"


# =============================================================================
# Tool Choice 模式
# =============================================================================

def test_tool_choice_required(client):
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "查北京天气"}],
        tools=[
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "获取天气",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
                },
            }
        ],
        tool_choice="required",
        stream=False,
    )
    # required 模式下应当触发 tool_calls
    assert resp.choices[0].finish_reason == "tool_calls"
    assert resp.choices[0].message.tool_calls


def test_tool_choice_named_function(client):
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "查北京天气"}],
        tools=[
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
                },
            },
            {
                "type": "function",
                "function": {
                    "name": "get_time",
                    "parameters": {},
                },
            },
        ],
        tool_choice={"type": "function", "function": {"name": "get_weather"}},
        stream=False,
    )
    assert resp.choices[0].finish_reason == "tool_calls"
    assert resp.choices[0].message.tool_calls[0].function.name == "get_weather"


def test_tool_choice_none_ignores_tools(client):
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "你好"}],
        tools=[
            {
                "type": "function",
                "function": {"name": "get_weather", "parameters": {}},
            }
        ],
        tool_choice="none",
        stream=False,
    )
    # none 模式下不应触发 tool_calls
    assert resp.choices[0].message.tool_calls is None
    assert resp.choices[0].message.content


def test_parallel_tool_calls_false(client):
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "同时查北京和上海天气"}],
        tools=[
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
                },
            }
        ],
        parallel_tool_calls=False,
        stream=False,
    )
    # 只要求请求成功即可，具体是否只调用一个由模型决定
    assert resp.choices[0].finish_reason in ("stop", "tool_calls")


# =============================================================================
# 已弃用 functions / function_call 兼容
# =============================================================================

def test_functions_legacy_auto(client):
    """functions + function_call='auto' 应映射为 tools + tool_choice='auto'"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "查北京天气"}],
        functions=[
            {
                "name": "get_weather",
                "description": "获取天气",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
            }
        ],
        function_call="auto",
        stream=False,
    )
    # 映射后应等同于 tools + tool_choice=auto
    assert resp.choices[0].finish_reason in ("stop", "tool_calls")
    if resp.choices[0].message.tool_calls:
        assert resp.choices[0].message.tool_calls[0].function.name == "get_weather"


def test_functions_legacy_named(client):
    """function_call={'name': 'x'} 应映射为对应的 tool_choice"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "查天气"}],
        functions=[
            {
                "name": "get_weather",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}},
            }
        ],
        function_call={"name": "get_weather"},
        stream=False,
    )
    assert resp.choices[0].finish_reason == "tool_calls"
    assert resp.choices[0].message.tool_calls[0].function.name == "get_weather"


def test_functions_and_tools_priority(client):
    """tools 和 functions 同时存在时优先使用 tools"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "查时间"}],
        tools=[
            {
                "type": "function",
                "function": {
                    "name": "get_time",
                    "description": "获取时间",
                    "parameters": {},
                },
            }
        ],
        functions=[
            {
                "name": "get_weather",
                "description": "获取天气",
                "parameters": {},
            }
        ],
        tool_choice="auto",
        function_call="auto",
        stream=False,
    )
    # 应优先使用 tools 中的 get_time，不应被 functions 覆盖
    if resp.choices[0].message.tool_calls:
        names = [tc.function.name for tc in resp.choices[0].message.tool_calls]
        assert "get_weather" not in names  # functions 的 tool 不应出现


# =============================================================================
# response_format 降级兼容
# =============================================================================

def test_response_format_json_object(client):
    """response_format={'type': 'json_object'} 应在 prompt 中注入 JSON 约束"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "输出用户信息，包括姓名和年龄"}],
        response_format={"type": "json_object"},
        stream=False,
    )
    assert resp.choices[0].message.content


def test_response_format_json_schema(client):
    """response_format={'type': 'json_schema'} 应在 prompt 中注入 schema 约束"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "输出用户信息"}],
        response_format={
            "type": "json_schema",
            "json_schema": {
                "name": "user_info",
                "schema": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "age": {"type": "integer"},
                    },
                },
            },
        },
        stream=False,
    )
    assert resp.choices[0].message.content


def test_response_format_text_no_injection(client):
    """response_format={'type': 'text'} 不应注入额外约束"""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "你好"}],
        response_format={"type": "text"},
        stream=False,
    )
    assert resp.choices[0].message.content


# =============================================================================
# 解析但忽略的字段（不应报错）
# =============================================================================

def test_ignored_params(client):
    """
    传入大量适配器解析但不消费的字段，验证请求能正常完成不报错。
    这些字段包括：temperature, top_p, max_tokens, max_completion_tokens,
    frequency_penalty, presence_penalty, seed, n, metadata, store,
    user, safety_identifier, prompt_cache_key, modalities, prediction。
    """
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": "你好"}],
        temperature=0.5,
        top_p=0.9,
        max_tokens=100,
        max_completion_tokens=100,
        frequency_penalty=0.5,
        presence_penalty=0.5,
        seed=42,
        n=1,
        metadata={"key": "value"},
        store=True,
        user="test-user",
        safety_identifier="safe-id",
        prompt_cache_key="cache-key",
        modalities=["text"],
        prediction={"type": "content", "content": "预测内容"},
        stream=False,
    )
    # 关键断言：请求不报错且返回正常响应
    assert resp.object == "chat.completion"
    assert resp.choices[0].message.role == "assistant"
    assert resp.choices[0].message.content
