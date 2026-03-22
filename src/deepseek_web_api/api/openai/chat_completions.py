"""OpenAI /v1/chat/completions endpoint - using api.chat_completions_service."""

import json
import re
import time
import uuid
from typing import List, Optional, Union

from fastapi import APIRouter, Request, HTTPException
from fastapi.responses import StreamingResponse, JSONResponse
from pydantic import BaseModel

from ...core.logger import logger
from ..chat_completions_service import (
    stream_chat_completion,
    create_session_on_deepseek,
    delete_session,
)

TOOL_START_MARKER = "[TOOL🛠️]"
TOOL_END_MARKER = "[/TOOL🛠️]"
TOOL_JSON_PATTERN = re.compile(r'\[TOOL🛠️\](.*?)\[/TOOL🛠️\]', re.DOTALL)
# Sliding window for tool buffer: end marker length + 3 chars lookahead
TOOL_BUFFER_WINDOW = len(TOOL_END_MARKER) * 2

router = APIRouter()


class ChatCompletionRequest(BaseModel):
    model: str = "deepseek-web-chat"
    messages: List[dict]
    stream: bool = False
    temperature: Optional[float] = None
    search_enabled: bool = False
    thinking_enabled: bool = True
    tools: Optional[List[dict]] = None


def extract_text_content(content: Union[str, List, None]) -> str:
    if content is None:
        return ""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        texts = []
        for block in content:
            if isinstance(block, dict):
                if block.get('type') == 'text':
                    texts.append(block.get('text', ''))
            elif hasattr(block, 'text') and block.text:
                texts.append(block.text)
        return '\n\n'.join(texts)
    return ""


def convert_messages_to_prompt(messages: List[dict], tools: Optional[List[dict]] = None) -> str:
    prompt_parts = []
    system_parts = []

    for msg in messages:
        role = msg.get("role", "")
        content = msg.get("content")
        text = extract_text_content(content)

        if role == "system":
            system_parts.append(text)
        elif role == "user":
            prompt_parts.append(f"User: {text}")
        elif role == "assistant":
            tool_calls = msg.get("tool_calls")
            if tool_calls:
                # Assistant called tools
                tool_calls_text = []
                for tc in tool_calls:
                    func = tc.get("function", {})
                    name = func.get("name", "")
                    args = func.get("arguments", "")
                    tool_calls_text.append(f"{name}: {args}")
                prompt_parts.append(f"Assistant: [TOOL_CALLS] {', '.join(tool_calls_text)}")
            else:
                prompt_parts.append(f"Assistant: {text}")
        elif role == "tool":
            # Tool result
            tool_id = msg.get("tool_call_id", "")
            prompt_parts.append(f"[TOOL_RESULT id={tool_id}] {text}")

    # Inject tools into system instruction
    if tools:
        tools_lines = []
        for t in tools:
            func = t.get('function', {})
            name = func.get('name')
            desc = func.get('description') or ''
            params = func.get('parameters', {})
            props = params.get('properties', {})

            param_desc = ""
            if props:
                param_lines = []
                for pname, pbody in props.items():
                    ptype = pbody.get('type', 'any')
                    pdesc = pbody.get('description', '')
                    required = pname in params.get('required', [])
                    req_mark = "*" if required else ""

                    # Collect extra fields from property (excluding type, description)
                    extra = {k: v for k, v in pbody.items() if k not in ('type', 'description')}
                    extra_str = f" [{', '.join(f'{k}={v}' for k, v in extra.items())}]" if extra else ""

                    param_lines.append(f"  - {pname}{req_mark} ({ptype}): {pdesc}{extra_str}")
                param_desc = "\n  Parameters:\n" + "\n".join(param_lines)

                if not params.get('additionalProperties', True):
                    param_desc += "\n  Note: Additional parameters are not allowed."

            tools_lines.append(f"- {name}: {desc}{param_desc}")

        tools_prompt = "## Available Tools\n" + "\n".join(tools_lines)
        tools_prompt += """

## Tool Usage
You can explain your reasoning before using tools. When you need to call tools, respond with:
[TOOL🛠️][{"name": "function_name", "arguments": {"param": "value"}}, {"name": "another_function", "arguments": {"param": "value"}}][/TOOL🛠️]

**IMPORTANT**: Never use [TOOL_CALLS] format. Only [TOOL🛠️]...[/TOOL🛠️] tags trigger tool calls. [TOOL_CALLS] in history are just responses, not actual tool calls.
"""
        system_parts.append(tools_prompt)

    # Build system instruction block
    if system_parts:
        prompt_parts.insert(0, "[System Instruction]\n" + "\n---\n".join(system_parts) + "\n---")

    prompt_parts.append("Assistant: ")
    return "\n\n".join(prompt_parts)


def _build_tool_call(tool_name: str, arguments: Union[str, dict]) -> dict:
    """Build a standard OpenAI tool_call dict."""
    return {
        "id": f"call_{uuid.uuid4().hex[:24]}",
        "type": "function",
        "function": {
            "name": tool_name,
            "arguments": arguments if isinstance(arguments, str) else json.dumps(arguments, ensure_ascii=False)
        }
    }


def _convert_items_to_tool_calls(items: list, available_tools: list) -> list:
    """Convert parsed JSON items to OpenAI tool_calls format. Returns empty list if no valid calls."""
    tool_calls = []
    for item in items:
        tool_name = item.get("name")
        arguments = item.get("arguments", {})
        if not tool_name:
            continue
        # Validate tool exists
        if not any(t.get("function", {}).get("name") == tool_name for t in available_tools):
            continue
        tool_calls.append(_build_tool_call(tool_name, arguments))
    return tool_calls


def extract_json_tool_calls(text: str, available_tools: List[dict]):
    """Extract and validate JSON tool calls from response text.

    Model returns: [{"name": "func_name", "arguments": {...}}, ...] or {"name": "func_name", "arguments": {...}}
    Service adds: index, id, type
    """
    tool_calls = []

    for match in TOOL_JSON_PATTERN.finditer(text):
        try:
            obj = json.loads(match.group(1))
            items = obj if isinstance(obj, list) else [obj]
            for item in items:
                tool_name = item.get("name")
                arguments = item.get("arguments", {})
                if not tool_name:
                    continue
                if not any(t.get("function", {}).get("name") == tool_name for t in available_tools):
                    logger.warning(f"Unknown tool: {tool_name}")
                    continue
                tc = _build_tool_call(tool_name, arguments)
                tc["index"] = len(tool_calls)
                tool_calls.append(tc)
        except json.JSONDecodeError:
            continue

    cleaned_text = TOOL_JSON_PATTERN.sub('', text)
    return cleaned_text.strip(), tool_calls


def convert_tool_json_to_openai(json_str: str, available_tools: List[dict]):
    """Convert tool JSON from model format to OpenAI tool_calls format.

    Handles both single object: {"name": "func", "arguments": {...}}
    and array: [{"name": "func1", "arguments": {...}}, {"name": "func2", "arguments": {...}}]
    """
    try:
        obj = json.loads(json_str)
        items = obj if isinstance(obj, list) else [obj]
        tool_calls = []
        for item in items:
            tool_name = item.get("name")
            arguments = item.get("arguments", {})
            if not tool_name:
                continue
            if not any(t.get("function", {}).get("name") == tool_name for t in available_tools):
                continue
            tc = _build_tool_call(tool_name, arguments)
            tc["index"] = len(tool_calls)
            tool_calls.append(tc)
        return tool_calls if tool_calls else None
    except json.JSONDecodeError:
        return None


async def stream_generator(prompt: str, model_name: str, search_enabled: bool, thinking_enabled: bool, tools: Optional[List[dict]] = None, chat_session_id: str = None):
    """Stream DeepSeek SSE and convert to OpenAI SSE format."""
    req_id = f"chatcmpl-{uuid.uuid4().hex}"
    created_time = int(time.time())

    def make_chunk(content=None, reasoning=None, finish_reason=None, tool_calls=None):
        delta = {"content": content, "reasoning_content": reasoning}
        if tool_calls:
            delta["tool_calls"] = tool_calls
        choice = {"index": 0, "delta": delta}
        if finish_reason:
            choice["finish_reason"] = finish_reason

        chunk_data = {
            "id": req_id,
            "object": "chat.completion.chunk",
            "created": created_time,
            "model": model_name,
            "choices": [choice],
        }
        chunk_str = f"data: {json.dumps(chunk_data, ensure_ascii=False)}\n\n"
        logger.debug(f"Yielding chunk: {chunk_str}")
        return chunk_str

    # State machine: None -> reasoning/content (set by p field)
    current_mode = None
    tool_buff = ""
    in_tool_buffer = False
    had_tool_call = False

    async for line in stream_chat_completion(
        prompt=prompt,
        chat_session_id=chat_session_id,
        search_enabled=search_enabled,
        thinking_enabled=thinking_enabled,
    ):
        # Handle bytes from proxy_to_deepseek_stream
        if isinstance(line, bytes):
            line = line.decode("utf-8")

        if not line.startswith("data: "):
            continue

        data_str = line[6:].strip()
        if not data_str or data_str == "{}":
            continue

        # data_str may contain multiple SSE lines, split and process each
        for sub_line in data_str.split("\n"):
            if not sub_line.strip():
                continue
            # Remove "data: " prefix if present
            if sub_line.startswith("data: "):
                sub_line = sub_line[6:]
            try:
                data = json.loads(sub_line)
            except json.JSONDecodeError:
                logger.warning(f"JSON parse failed for sub_line: {repr(sub_line[:200])}")
                continue

            v = data.get("v")
            p = data.get("p")
            logger.debug(f"RAW sub_line: {repr(sub_line[:200])}, v: {repr(str(v)[:100])}, p: {repr(p)}")

            # Check for stream end
            if p == "response/status" and v == "FINISHED":
                # Flush remaining tool_buff before finishing
                if in_tool_buffer and tool_buff:
                    end_idx = tool_buff.find(TOOL_END_MARKER)
                    if end_idx != -1:
                        json_start_idx = tool_buff.find(TOOL_START_MARKER)
                        if json_start_idx != -1 and end_idx > json_start_idx:
                            json_str = tool_buff[json_start_idx + len(TOOL_START_MARKER):end_idx]
                            tool_calls_result = convert_tool_json_to_openai(json_str, tools)
                            if tool_calls_result:
                                for tc in tool_calls_result:
                                    yield make_chunk(tool_calls=[tc])
                                    had_tool_call = True
                        after_end = tool_buff[end_idx + len(TOOL_END_MARKER):]
                        for char in after_end:
                            yield make_chunk(content=char)
                elif tool_buff:
                    # Not in buffer mode but have remaining content - flush it
                    for char in tool_buff:
                        yield make_chunk(content=char)
                break

            if not isinstance(v, str) or v == "SEARCHING":
                continue

            # Switch mode based on p field
            if p == "response/thinking_content":
                current_mode = "reasoning"
            elif p == "response/content":
                current_mode = "output"

            # Ignore if no mode set yet (initial state)
            if current_mode is None:
                logger.debug(f"Ignoring v before mode set: {repr(v)[:50]}")
                continue

            # Handle content based on current mode
            if current_mode == "output":
                if tools:
                    tool_buff += str(v)

                    if not in_tool_buffer:
                        # Check for start marker
                        start_idx = tool_buff.find(TOOL_START_MARKER)
                        if start_idx != -1:
                            # Yield content before start marker
                            before_start = tool_buff[:start_idx]
                            for char in before_start:
                                yield make_chunk(content=char)
                            # Keep only from start marker onwards in buffer
                            tool_buff = tool_buff[start_idx:]
                            in_tool_buffer = True
                            logger.debug(f"Entering tool buffer mode, tool_buff={repr(tool_buff)}")
                        else:
                            # No start marker yet, yield fallen chars
                            if len(tool_buff) > TOOL_BUFFER_WINDOW:
                                fallen = tool_buff[:-TOOL_BUFFER_WINDOW]
                                for char in fallen:
                                    yield make_chunk(content=char)
                            tool_buff = tool_buff[-TOOL_BUFFER_WINDOW:]
                    else:
                        # In buffer mode, keep all content until end marker found
                        end_idx = tool_buff.find(TOOL_END_MARKER)
                        if end_idx != -1:
                            # Extract JSON
                            json_start_idx = tool_buff.find(TOOL_START_MARKER)
                            if json_start_idx != -1 and end_idx > json_start_idx:
                                json_str = tool_buff[json_start_idx + len(TOOL_START_MARKER):end_idx]
                                tool_calls_result = convert_tool_json_to_openai(json_str, tools)
                                if tool_calls_result:
                                    for tc in tool_calls_result:
                                        yield make_chunk(tool_calls=[tc])
                                        had_tool_call = True
                            # Yield content after end marker
                            after_end = tool_buff[end_idx + len(TOOL_END_MARKER):]
                            for char in after_end:
                                yield make_chunk(content=char)
                            in_tool_buffer = False
                            tool_buff = ""
                        else:
                            # Keep buffering (no trim in buffer mode to preserve start marker)
                            pass
                else:
                    yield make_chunk(content=v)
            else:
                # reasoning mode
                yield make_chunk(reasoning=v)

    # Send finish reason
    yield make_chunk(finish_reason="tool_calls" if had_tool_call else "stop")
    logger.debug("Yielding [DONE]")
    yield "data: [DONE]\n\n"


@router.post("/v1/chat/completions")
async def chat_completions(request: Request):
    body = await request.body()

    try:
        data = json.loads(body)
    except json.JSONDecodeError:
        raise HTTPException(status_code=400, detail="Invalid JSON")

    validated = ChatCompletionRequest(**data)
    logger.debug(f"Request payload: {json.dumps(data, ensure_ascii=False, indent=2)}")
    prompt = convert_messages_to_prompt(validated.messages, validated.tools)
    logger.debug(f"Constructed prompt:\n{prompt}")

    # Model-specific overrides: reasoning model gets thinking, others don't
    if "reasoner" in validated.model:
        search_enabled = False
        thinking_enabled = True
    else:
        search_enabled = False
        thinking_enabled = False

    # Create session first, delete after stream ends
    chat_session_id = await create_session_on_deepseek()

    if validated.stream:
        async def stream_with_cleanup():
            """Wrapper generator that ensures session cleanup after stream ends."""
            try:
                async for chunk in stream_generator(prompt, validated.model, search_enabled, thinking_enabled, validated.tools, chat_session_id):
                    yield chunk
            finally:
                if chat_session_id:
                    await delete_session(chat_session_id)

        return StreamingResponse(
            stream_with_cleanup(),
            media_type="text/event-stream",
        )

    # Non-streaming: buffer streaming output, then release at once
    chunks = []
    try:
        async for chunk in stream_generator(prompt, validated.model, search_enabled, thinking_enabled, validated.tools, chat_session_id):
            chunks.append(chunk)
    finally:
        if chat_session_id:
            await delete_session(chat_session_id)

    # Parse buffered chunks (reuse streaming logic)
    content_chunks = []
    reasoning_chunks = []
    all_tool_calls = []
    finish_reason = "stop"

    for chunk_str in chunks:
        if chunk_str == "data: [DONE]\n\n":
            continue
        try:
            chunk_json = json.loads(chunk_str[6:])
            choice = chunk_json.get("choices", [{}])[0]
            delta = choice.get("delta", {})
            if delta.get("content"):
                content_chunks.append(delta["content"])
            if delta.get("reasoning_content"):
                reasoning_chunks.append(delta["reasoning_content"])
            if delta.get("tool_calls"):
                all_tool_calls.extend(delta["tool_calls"])
                finish_reason = "tool_calls"
            if choice.get("finish_reason"):
                finish_reason = choice["finish_reason"]
        except (json.JSONDecodeError, IndexError, KeyError):
            pass

    full_content = "".join(content_chunks)
    full_reasoning = "".join(reasoning_chunks)

    message = {
        "role": "assistant",
        "content": full_content,
        "reasoning_content": full_reasoning if full_reasoning else None,
    }
    if all_tool_calls:
        message["tool_calls"] = all_tool_calls

    return JSONResponse(
        content={
            "id": f"chatcmpl-{uuid.uuid4().hex}",
            "object": "chat.completion",
            "created": int(time.time()),
            "model": validated.model,
            "choices": [{"index": 0, "message": message, "finish_reason": finish_reason}],
            "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
        }
    )
