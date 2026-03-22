"""Chat completions service - extracted business logic from routes.py."""

import asyncio
import json
import logging

import httpx
from fastapi import Response

from ..core.auth import get_auth_headers
from ..core.pow_service import get_pow_response
from ..core.session_store import SessionStore
from ..core.config import DEEPSEEK_HOST

logger = logging.getLogger("deepseek_web_api")

# API path constants
_PATH_CREATE_SESSION = "api/v0/chat_session/create"
_PATH_DELETE_SESSION = "api/v0/chat_session/delete"
_PATH_COMPLETION = "api/v0/chat/completion"
_PATH_UPLOAD_FILE = "api/v0/file/upload_file"
_PATH_FETCH_FILES = "api/v0/file/fetch_files"
_PATH_HISTORY_MESSAGES = "api/v0/chat/history_messages"

DEEPSEEK_BASE_URL = f"https://{DEEPSEEK_HOST}"
session_store = SessionStore.get_instance()


def parse_sse_response_message_id(content: bytes) -> int | None:
    """Parse SSE stream to extract response_message_id from ready event."""
    try:
        text = content.decode("utf-8")
        for line in text.split("\n"):
            line = line.strip()
            if line.startswith("data:") and "response_message_id" in line:
                data_str = line[5:].strip()
                data = json.loads(data_str)
                return data.get("response_message_id")
    except Exception as e:
        logger.warning(f"Failed to parse SSE response_message_id: {type(e).__name__}")
    return None


def extract_chat_session_id(resp_body: bytes) -> str | None:
    """Extract chat_session_id from DeepSeek API response."""
    try:
        data = json.loads(resp_body)
        return data.get("data", {}).get("biz_data", {}).get("id")
    except Exception as e:
        logger.warning(f"Failed to extract chat_session_id: {type(e).__name__}")
        return None


async def proxy_to_deepseek(
    method,
    path,
    headers=None,
    json_data=None,
    params=None,
    content=None,
    files=None,
):
    """Proxy request to DeepSeek backend, return FastAPI Response."""
    url = f"{DEEPSEEK_BASE_URL}/{path}"
    auth_headers = get_auth_headers()
    if headers:
        headers = {**headers, **auth_headers}
    else:
        headers = auth_headers
    headers["Host"] = DEEPSEEK_HOST

    if files is not None and "Content-Type" in headers:
        del headers["Content-Type"]

    async with httpx.AsyncClient(timeout=120.0) as client:
        resp = await client.request(
            method=method,
            url=url,
            headers=headers,
            json=json_data,
            params=params,
            content=content,
            files=files,
        )
        return Response(
            content=resp.content,
            status_code=resp.status_code,
            headers=dict(resp.headers),
        )


async def proxy_to_deepseek_stream(
    method,
    path,
    headers=None,
    json_data=None,
    params=None,
):
    """Proxy request to DeepSeek backend as a streaming response, yield bytes."""
    url = f"{DEEPSEEK_BASE_URL}/{path}"
    auth_headers = get_auth_headers()
    if headers:
        headers = {**headers, **auth_headers}
    else:
        headers = auth_headers
    headers["Host"] = DEEPSEEK_HOST

    async with httpx.AsyncClient(timeout=120.0) as client:
        async with client.stream(
            method=method,
            url=url,
            headers=headers,
            json=json_data,
            params=params,
        ) as resp:
            async for chunk in resp.aiter_bytes():
                yield chunk


async def create_session_on_deepseek() -> str | None:
    """Create session on DeepSeek backend and return the chat_session_id."""
    resp = await proxy_to_deepseek(
        "POST",
        _PATH_CREATE_SESSION,
        json_data={"agent": "chat"},
    )
    # resp is FastAPI Response, access body via .body
    if resp.body:
        try:
            csid = extract_chat_session_id(resp.body)
            if csid:
                await session_store.acreate_session(csid)
                return csid
        except Exception as e:
            logger.warning(f"Failed to create session: {e}")
    return None


async def delete_session(chat_session_id: str) -> None:
    """Delete session from local store and DeepSeek backend."""
    await session_store.adelete_session(chat_session_id)
    await proxy_to_deepseek(
        "POST",
        _PATH_DELETE_SESSION,
        json_data={"chat_session_id": chat_session_id},
    )


async def create_session(body: dict = None) -> Response:
    """Create new session and return response with chat_session_id added.

    Args:
        body: Request body, defaults to {"agent": "chat"}

    Returns:
        FastAPI Response with chat_session_id added to body
    """
    if body is None:
        body = {"agent": "chat"}

    resp = await proxy_to_deepseek(
        "POST",
        _PATH_CREATE_SESSION,
        json_data=body,
    )

    if resp.body:
        try:
            data = json.loads(resp.body)
            chat_session_id = data.get("data", {}).get("biz_data", {}).get("id")
            if chat_session_id:
                await session_store.acreate_session(chat_session_id)
                data["chat_session_id"] = chat_session_id
                return Response(
                    content=json.dumps(data),
                    status_code=resp.status_code,
                    headers={"Content-Type": "application/json"},
                )
        except Exception as e:
            logger.warning(f"Failed to process session response: {e}")

    return Response(
        content=resp.body,
        status_code=resp.status_code,
        headers={"Content-Type": "application/json"},
    )


async def upload_file(file_content: bytes, filename: str, content_type: str) -> Response:
    """Upload file to DeepSeek.

    Args:
        file_content: File binary content
        filename: File name
        content_type: MIME type

    Returns:
        FastAPI Response from DeepSeek
    """
    files = {"file": (filename, file_content, content_type)}
    pow_response = get_pow_response(target_path="/api/v0/file/upload_file")
    headers = {
        "x-ds-pow-response": pow_response,
        "x-file-size": str(len(file_content)),
    } if pow_response else {}

    return await proxy_to_deepseek(
        "POST",
        _PATH_UPLOAD_FILE,
        headers=headers,
        files=files,
    )


async def fetch_files(file_ids: str) -> Response:
    """Fetch file status from DeepSeek.

    Args:
        file_ids: Comma-separated file IDs

    Returns:
        FastAPI Response from DeepSeek
    """
    return await proxy_to_deepseek(
        "GET",
        _PATH_FETCH_FILES,
        params={"file_ids": file_ids},
    )


async def get_history_messages(chat_session_id: str, offset: int = 0, limit: int = 20) -> Response:
    """Get chat history from DeepSeek.

    Args:
        chat_session_id: Session ID
        offset: Message offset
        limit: Message limit

    Returns:
        FastAPI Response from DeepSeek
    """
    return await proxy_to_deepseek(
        "GET",
        _PATH_HISTORY_MESSAGES,
        params={"chat_session_id": chat_session_id, "offset": offset, "limit": limit},
    )


async def stream_chat_completion(
    prompt: str,
    chat_session_id: str | None = None,
    search_enabled: bool = True,
    thinking_enabled: bool = True,
    ref_file_ids: list | None = None,
):
    """Stream chat completion from DeepSeek and yield SSE bytes.

    This is an async generator that yields raw SSE bytes chunks.
    Session management is handled internally:
    - If chat_session_id is provided, it will be used (multi-turn conversation)
    - If not provided, a new session is created and cleaned up after

    Args:
        prompt: The prompt to send
        chat_session_id: Optional existing session ID for multi-turn
        search_enabled: Enable web search
        thinking_enabled: Enable thinking/reasoning
        ref_file_ids: Optional list of file IDs to reference

    Yields:
        bytes: Raw SSE response chunks from DeepSeek
    """
    max_retries = 3
    retry_delay = 1.0
    collected = b""
    last_exception = None

    for attempt in range(max_retries):
        # Determine chat_session_id and parent_message_id (fresh for each attempt)
        if chat_session_id:
            parent_message_id = await session_store.aget_parent_message_id(chat_session_id)
            if not await session_store.ahas_session(chat_session_id):
                await session_store.acreate_session(chat_session_id)
                parent_message_id = None
        else:
            # Pre-create session so we can return the session_id in header
            chat_session_id = await create_session_on_deepseek()
            parent_message_id = None

        # Get PoW
        pow_response = get_pow_response()

        # Build payload for DeepSeek
        payload = {
            "chat_session_id": chat_session_id,
            "parent_message_id": parent_message_id,
            "preempt": False,
            "prompt": prompt,
            "ref_file_ids": ref_file_ids or [],
            "search_enabled": search_enabled,
            "thinking_enabled": thinking_enabled,
        }

        headers = {"x-ds-pow-response": pow_response} if pow_response else {}

        try:
            async for chunk in proxy_to_deepseek_stream(
                "POST",
                _PATH_COMPLETION,
                headers=headers,
                json_data=payload,
            ):
                collected += chunk
                yield chunk
            break  # Success, exit retry loop
        except (httpx.ConnectError, httpx.RemoteProtocolError, OSError) as e:
            last_exception = e
            if attempt < max_retries - 1:
                logger.warning(f"Request failed (attempt {attempt + 1}/{max_retries}): {e}, retrying in {retry_delay}s...")
                await asyncio.sleep(retry_delay)
                retry_delay *= 2  # Exponential backoff
                # Delete failed session and create fresh one for next attempt
                if chat_session_id:
                    await session_store.adelete_session(chat_session_id)
                    chat_session_id = None
            else:
                logger.error(f"Request failed after {max_retries} attempts: {e}")
                raise last_exception

    # Update session with message_id after stream completes
    if chat_session_id:
        msg_id = parse_sse_response_message_id(collected)
        if msg_id:
            await session_store.aupdate_parent_message_id(chat_session_id, msg_id)
