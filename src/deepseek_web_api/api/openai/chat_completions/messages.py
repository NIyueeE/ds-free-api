"""Message conversion utilities for OpenAI-style messages to DeepSeek prompt."""

import json
from typing import List, Optional, Union

from .tools import TOOL_START_MARKER, TOOL_END_MARKER


def extract_text_content(content: Union[str, List, None]) -> str:
    """Extract plain text from OpenAI message content field.

    Handles both string content and list content blocks (e.g., [{"type": "text", "text": "..."}]).
    """
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
    """Convert OpenAI-style messages array to DeepSeek prompt format.

    Args:
        messages: List of OpenAI-style messages with role and content
        tools: Optional OpenAI tools specification

    Returns:
        Formatted prompt string for DeepSeek API
    """
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
                # Assistant called tools - use unified [TOOL🛠️] format
                tool_calls_json = []
                for tc in tool_calls:
                    func = tc.get("function", {})
                    name = func.get("name", "")
                    args = func.get("arguments", "")
                    # args may be a JSON string or dict
                    if isinstance(args, str):
                        try:
                            args = json.loads(args)
                        except Exception:
                            pass
                    tool_calls_json.append({"name": name, "arguments": args})
                tool_calls_str = json.dumps(tool_calls_json, ensure_ascii=False)
                prompt_parts.append(f"Assistant: {TOOL_START_MARKER}{tool_calls_str}{TOOL_END_MARKER}")
            else:
                prompt_parts.append(f"Assistant: {text}")
        elif role == "tool":
            # Tool result
            tool_id = msg.get("tool_call_id", "")
            prompt_parts.append(f"\nTool: id={tool_id}\n```\n{text}\n```")

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

## Response Format
- **User**: human input (you receive this)
- **Assistant**: YOUR response (you output this)
- **Tool**: tool execution result (you receive this after calling tools)

## Tool Usage
You can explain your reasoning before using tools. When you need to call tools, respond with:
[TOOL🛠️][{"name": "function_name", "arguments": {"param": "value"}}, {"name": "another_function", "arguments": {"param": "value"}}][/TOOL🛠️]

**IMPORTANT**:
1. Only use [TOOL🛠️]...[/TOOL🛠️] tags for tool calls.
2. If you need to call multiple tools, put them all in a single [TOOL🛠️]...[/TOOL🛠️] array.
"""
        system_parts.append(tools_prompt)

    # Build system instruction block
    if system_parts:
        prompt_parts.insert(0, "[System Instruction]\n" + "\n---\n".join(system_parts) + "\n---")

    # Add separator and REMINDER before Assistant output if tools are available
    if tools:
        prompt_parts.append("\n---\nAbove is our conversation history.\n\n[REMINDER] When you need to call tools, you MUST use the [TOOL🛠️]...[/TOOL🛠️] tags. For multiple tool calls, wrap them in a JSON array: [TOOL🛠️][{\"name\": \"func1\", \"arguments\": {...}}, {\"name\": \"func2\", \"arguments\": {...}}][/TOOL🛠️].")

    return "\n\n".join(prompt_parts)
