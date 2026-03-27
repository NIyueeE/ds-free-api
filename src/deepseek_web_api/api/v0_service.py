"""Chat completions service - extracted business logic from routes.py."""

import asyncio
import json
import logging
from collections.abc import AsyncGenerator
from typing import Any

import httpx
from fastapi import Response

from ..core.auth import get_auth_headers, invalidate_token
from ..core.pow import get_pow_response
from ..core.parent_msg_store import ParentMsgStore
from ..core.config import DEEPSEEK_HOST

logger = logging.getLogger("deepseek_web_api")

# API path constants
_PATH_CREATE_SESSION = "api/v0/chat_session/create"
_PATH_DELETE_SESSION = "api/v0/chat_session/delete"
_PATH_COMPLETION = "api/v0/chat/completion"
_PATH_EDIT_MESSAGE = "api/v0/chat/edit_message"
_PATH_UPLOAD_FILE = "api/v0/file/upload_file"
_PATH_FETCH_FILES = "api/v0/file/fetch_files"
_PATH_HISTORY_MESSAGES = "api/v0/chat/history_messages"

DEEPSEEK_BASE_URL = f"https://{DEEPSEEK_HOST}"

_MAX_RATE_LIMIT_RETRIES = 3
_RATE_LIMIT_BASE_DELAY = 5.0  # seconds; doubles each retry


class RateLimitError(Exception):
    """Raised when DeepSeek returns a rate-limit response (HTTP 429/5xx) before any bytes are yielded."""

    def __init__(self, message: str, retry_after: float = 0.0):
        super().__init__(message)
        self.retry_after = retry_after  # Seconds to wait, from Retry-After header if present


def _parse_retry_after(value: str | None) -> float:
    """Parse Retry-After header value into seconds. Returns 0.0 if absent or unparseable."""
    if not value:
        return 0.0
    try:
        return max(0.0, float(value))
    except (ValueError, TypeError):
        return 0.0


def _response_indicates_invalid_token(content: bytes) -> bool:
    """Return True if DeepSeek response payload indicates token invalidation."""
    if not content:
        return False
    try:
        data = json.loads(content)
    except Exception:
        return False
    return data.get("code") == 40003


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


async def proxy_to_deepseek(
    method: str,
    path: str,
    headers: dict[str, str] | None = None,
    json_data: dict[str, Any] | None = None,
    params: dict[str, Any] | None = None,
    content: bytes | None = None,
    files: dict[str, tuple[str, bytes, str]] | None = None,
) -> Response:
    """Proxy request to DeepSeek backend, return FastAPI Response."""
    url = f"{DEEPSEEK_BASE_URL}/{path}"
    logger.debug(f"[proxy] {method} {path}")
    max_retries = 2

    async with httpx.AsyncClient(timeout=120.0) as client:
        for attempt in range(max_retries):
            auth_headers = get_auth_headers()
            merged_headers = {**headers, **auth_headers} if headers else auth_headers
            merged_headers["Host"] = DEEPSEEK_HOST

            if files is not None and "Content-Type" in merged_headers:
                del merged_headers["Content-Type"]

            resp = await client.request(
                method=method,
                url=url,
                headers=merged_headers,
                json=json_data,
                params=params,
                content=content,
                files=files,
            )

            if _response_indicates_invalid_token(resp.content) and attempt < max_retries - 1:
                logger.warning(f"[proxy] token invalid on {path}, refreshing token and retrying...")
                invalidate_token()
                continue

            return Response(
                content=resp.content,
                status_code=resp.status_code,
                headers=dict(resp.headers),
            )

        return Response(content=b"", status_code=502)


async def proxy_to_deepseek_stream(
    method: str,
    path: str,
    headers: dict[str, str] | None = None,
    json_data: dict[str, Any] | None = None,
    params: dict[str, Any] | None = None,
) -> AsyncGenerator[bytes, None]:
    """Proxy request to DeepSeek backend as a streaming response, yield bytes.

    Raises:
        RateLimitError: If DeepSeek responds with HTTP 429 or 5xx before any bytes are yielded.
    """
    url = f"{DEEPSEEK_BASE_URL}/{path}"
    logger.debug(f"[proxy:stream] {method} {path}")
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
            if resp.status_code == 429:
                body = await resp.aread()
                retry_after = _parse_retry_after(resp.headers.get("Retry-After"))
                raise RateLimitError(
                    f"DeepSeek rate limited (HTTP 429): {body[:200]!r}",
                    retry_after=retry_after,
                )
            if resp.status_code >= 500:
                body = await resp.aread()
                raise RateLimitError(
                    f"DeepSeek server error (HTTP {resp.status_code}): {body[:200]!r}",
                    retry_after=5.0,
                )
            async for chunk in resp.aiter_bytes():
                yield chunk


async def delete_session(chat_session_id: str) -> Response:
    """Delete session from DeepSeek backend and clean up local store."""
    max_retries = 5
    retry_delay = 0.5
    last_exc = None
    last_resp = None

    for attempt in range(max_retries):
        try:
            resp = await proxy_to_deepseek(
                "POST",
                _PATH_DELETE_SESSION,
                json_data={"chat_session_id": chat_session_id},
            )
            last_resp = resp
            logger.info(f"[delete_session] attempt {attempt+1}: status={resp.status_code}")

            # Check biz_code from delete response
            if resp.body:
                try:
                    data = json.loads(resp.body)
                    biz_code = data.get("data", {}).get("biz_code")
                    if biz_code == 0:
                        # Delete succeeded
                        logger.info(f"[delete_session] session={chat_session_id} deleted")
                        break
                    else:
                        biz_msg = data.get("data", {}).get("biz_msg", "unknown error")
                        logger.warning(f"[delete_session] biz_code={biz_code}, msg={biz_msg}, retrying...")
                except json.JSONDecodeError:
                    logger.warning("[delete_session] failed to parse response, retrying...")

            # Retry on failure
            if attempt < max_retries - 1:
                await asyncio.sleep(retry_delay)
                retry_delay *= 2
            continue
        except Exception as e:
            last_exc = e
            logger.warning(f"[delete_session] attempt {attempt+1} failed: {type(e).__name__}: {e}")
            if attempt < max_retries - 1:
                await asyncio.sleep(retry_delay)
                retry_delay *= 2
            continue

    if last_exc:
        logger.warning(f"[delete_session] all {max_retries} attempts failed, last error: {last_exc}")

    # Always clean up local store, regardless of backend result
    await ParentMsgStore.get_instance().adelete(chat_session_id)

    if last_resp is not None:
        return last_resp

    return Response(
        content=json.dumps(
            {
                "code": -1,
                "msg": "Failed to delete session after retries",
                "data": {
                    "biz_code": -1,
                    "biz_msg": str(last_exc) if last_exc else "unknown error",
                    "biz_data": None,
                },
            }
        ),
        status_code=502,
        media_type="application/json",
    )


async def create_session(body: dict = None) -> tuple[str | None, Response]:
    """Create new session and return (session_id, response).

    Args:
        body: Request body, defaults to {"agent": "chat"}

    Returns:
        Tuple of (session_id, FastAPI Response with chat_session_id added to body)
    """
    if body is None:
        body = {"agent": "chat"}

    resp = await proxy_to_deepseek(
        "POST",
        _PATH_CREATE_SESSION,
        json_data=body,
    )

    chat_session_id = None
    if resp.body:
        try:
            data = json.loads(resp.body)
            payload = data.get("data")
            if not isinstance(payload, dict):
                logger.warning(
                    f"[create_session] backend returned unexpected payload: status={resp.status_code}, body={resp.body[:300]!r}"
                )
                return None, resp

            biz_data = payload.get("biz_data")
            if not isinstance(biz_data, dict):
                logger.warning(
                    f"[create_session] backend returned unexpected biz_data: status={resp.status_code}, body={resp.body[:300]!r}"
                )
                return None, resp

            chat_session_id = biz_data.get("id")
            if chat_session_id:
                await ParentMsgStore.get_instance().acreate(chat_session_id)
                data["chat_session_id"] = chat_session_id
                logger.info(f"[create_session] session={chat_session_id} created")
                resp = Response(
                    content=json.dumps(data),
                    status_code=resp.status_code,
                    headers={"Content-Type": "application/json"},
                )
        except Exception as e:
            logger.warning(f"Failed to process session response: {e}")

    return chat_session_id, resp


async def upload_file(file_content: bytes, filename: str, content_type: str) -> Response:
    """Upload file to DeepSeek.

    Args:
        file_content: File binary content
        filename: File name
        content_type: MIME type

    Returns:
        FastAPI Response from DeepSeek

    Raises:
        RuntimeError: If PoW response cannot be obtained
    """
    files = {"file": (filename, file_content, content_type)}
    pow_response = get_pow_response(target_path="/api/v0/file/upload_file")
    if not pow_response:
        logger.error("[upload_file] Failed to get PoW response")
        raise RuntimeError("Failed to get PoW response for file upload")

    headers = {
        "x-ds-pow-response": pow_response,
        "x-file-size": str(len(file_content)),
    }

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
    logger.info(f"[stream_chat] session={chat_session_id}, prompt={prompt[:30]}...")

    # Determine chat_session_id and parent_message_id
    store = ParentMsgStore.get_instance()
    if chat_session_id:
        parent_message_id = await store.aget_parent_message_id(chat_session_id)
        if not await store.ahas(chat_session_id):
            await store.acreate(chat_session_id)
            parent_message_id = None
    else:
        # Pre-create session so we can return the session_id in header
        chat_session_id, _ = await create_session()
        if not chat_session_id:
            logger.error("[stream_chat_completion] Failed to create session")
            yield b"data: {\"error\": \"Failed to create session\"}\n\n"
            yield b"event: finish\ndata: {}\n\n"
            return
        parent_message_id = None

    # Get PoW and stream, with retry on rate limit
    last_rate_limit_error: RateLimitError | None = None
    for rate_limit_attempt in range(_MAX_RATE_LIMIT_RETRIES):
        if rate_limit_attempt > 0:
            delay = max(
                _RATE_LIMIT_BASE_DELAY * (2 ** (rate_limit_attempt - 1)),
                last_rate_limit_error.retry_after if last_rate_limit_error else 0.0,
            )
            logger.warning(
                f"[stream_chat] rate limited by DeepSeek, "
                f"retrying in {delay:.1f}s (attempt {rate_limit_attempt + 1}/{_MAX_RATE_LIMIT_RETRIES})"
            )
            await asyncio.sleep(delay)

        # Fresh PoW each attempt (challenges expire)
        pow_response = get_pow_response()
        if not pow_response:
            logger.error("[stream_chat_completion] Failed to get PoW response")
            yield b"data: {\"error\": \"Failed to get PoW response\"}\n\n"
            yield b"event: finish\ndata: {}\n\n"
            return

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
        headers = {"x-ds-pow-response": pow_response}

        try:
            # Stream and collect only if we need to extract message_id
            collected = b"" if chat_session_id else None
            async for chunk in proxy_to_deepseek_stream(
                "POST",
                _PATH_COMPLETION,
                headers=headers,
                json_data=payload,
            ):
                if collected is not None:
                    collected += chunk
                yield chunk

            # Update session with message_id after stream completes
            if collected is not None:
                msg_id = parse_sse_response_message_id(collected)
                if msg_id:
                    await ParentMsgStore.get_instance().aupdate_parent_message_id(chat_session_id, msg_id)
                    logger.info(f"[stream_chat] session={chat_session_id} updated parent_msg_id={msg_id}")
            return  # Success

        except RateLimitError as e:
            last_rate_limit_error = e
            if rate_limit_attempt == _MAX_RATE_LIMIT_RETRIES - 1:
                logger.error(f"[stream_chat] all rate-limit retries exhausted: {e}")
                raise


async def stream_edit_message(
    prompt: str,
    chat_session_id: str | None = None,
    search_enabled: bool = True,
    thinking_enabled: bool = True,
):
    """Stream edit message from DeepSeek and yield SSE bytes.

    Uses fixed message_id=1 to enable stateless multi-turn conversations
    within a single chat_session_id.

    Args:
        prompt: The prompt to send
        chat_session_id: Optional session ID, uses default if not provided
        search_enabled: Enable web search
        thinking_enabled: Enable thinking/reasoning

    Yields:
        bytes: Raw SSE response chunks from DeepSeek
    """
    logger.info(f"[edit_message] session={chat_session_id}, prompt={prompt[:30]}...")

    # Use provided session_id or create default one
    if not chat_session_id:
        chat_session_id, _ = await create_session()
        if not chat_session_id:
            logger.error("[edit_message] Failed to create session")
            yield b"data: {\"error\": \"Failed to create session\"}\n\n"
            yield b"event: finish\ndata: {}\n\n"
            return

    # Get PoW and stream, with retry on rate limit
    last_rate_limit_error: RateLimitError | None = None
    for rate_limit_attempt in range(_MAX_RATE_LIMIT_RETRIES):
        if rate_limit_attempt > 0:
            delay = max(
                _RATE_LIMIT_BASE_DELAY * (2 ** (rate_limit_attempt - 1)),
                last_rate_limit_error.retry_after if last_rate_limit_error else 0.0,
            )
            logger.warning(
                f"[edit_message] rate limited by DeepSeek, "
                f"retrying in {delay:.1f}s (attempt {rate_limit_attempt + 1}/{_MAX_RATE_LIMIT_RETRIES})"
            )
            await asyncio.sleep(delay)

        # Fresh PoW each attempt (challenges expire)
        pow_response = get_pow_response()
        if not pow_response:
            logger.error("[edit_message] Failed to get PoW response")
            yield b"data: {\"error\": \"Failed to get PoW response\"}\n\n"
            yield b"event: finish\ndata: {}\n\n"
            return

        # Build payload with fixed message_id=1
        payload = {
            "chat_session_id": chat_session_id,
            "message_id": 1,  # Fixed message_id for stateless conversation
            "prompt": prompt,
            "search_enabled": search_enabled,
            "thinking_enabled": thinking_enabled,
        }
        headers = {"x-ds-pow-response": pow_response}

        try:
            # Stream from DeepSeek
            collected = b""
            async for chunk in proxy_to_deepseek_stream(
                "POST",
                _PATH_EDIT_MESSAGE,
                headers=headers,
                json_data=payload,
            ):
                collected += chunk
                yield chunk

            # Update parent_message_id for future calls
            msg_id = parse_sse_response_message_id(collected)
            if msg_id:
                await ParentMsgStore.get_instance().aupdate_parent_message_id(chat_session_id, msg_id)
                logger.info(f"[edit_message] session={chat_session_id} updated parent_msg_id={msg_id}")
            return  # Success

        except RateLimitError as e:
            last_rate_limit_error = e
            if rate_limit_attempt == _MAX_RATE_LIMIT_RETRIES - 1:
                logger.error(f"[edit_message] all rate-limit retries exhausted: {e}")
                raise
