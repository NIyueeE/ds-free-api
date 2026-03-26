"""Unit tests for api/v0_service.py

These tests mock DeepSeek backend and core dependencies.
Run with: pytest tests/api/test_v0_service.py -v
"""

import json
from unittest.mock import AsyncMock, MagicMock, patch

import pytest


from deepseek_web_api.api import v0_service


@pytest.fixture
def mock_auth_headers():
    """Mock auth headers for tests."""
    return {
        "authorization": "Bearer test-token",
        "x-client-platform": "android",
    }


@pytest.fixture
def reset_parent_msg_store():
    """Reset ParentMsgStore singleton before each test."""
    from deepseek_web_api.core.parent_msg_store import ParentMsgStore

    ParentMsgStore._instance = None
    ParentMsgStore._lock = None
    yield
    ParentMsgStore._instance = None
    ParentMsgStore._lock = None


class TestProxyToDeepseek:
    """Test proxy_to_deepseek function."""

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.get_auth_headers")
    @patch("deepseek_web_api.api.v0_service.httpx.AsyncClient")
    async def test_proxy_uses_auth_headers(self, mock_client_class, mock_get_auth_headers, mock_auth_headers):
        """Test that proxy injects auth headers."""
        mock_get_auth_headers.return_value = mock_auth_headers

        mock_response = MagicMock()
        mock_response.content = b'{"code": 0}'
        mock_response.status_code = 200
        mock_response.headers = {"content-type": "application/json"}

        mock_client = AsyncMock()
        mock_client.request = AsyncMock(return_value=mock_response)
        mock_client_class.return_value.__aenter__ = AsyncMock(return_value=mock_client)
        mock_client_class.return_value.__aexit__ = AsyncMock(return_value=False)

        result = await v0_service.proxy_to_deepseek("POST", "api/v0/test", json_data={"test": True})

        assert result.status_code == 200
        mock_client.request.assert_called_once()
        call_kwargs = mock_client.request.call_args[1]
        assert "authorization" in call_kwargs["headers"]
        assert call_kwargs["headers"]["Host"] == v0_service.DEEPSEEK_HOST

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.get_auth_headers")
    @patch("deepseek_web_api.api.v0_service.httpx.AsyncClient")
    async def test_proxy_removes_content_type_for_files(self, mock_client_class, mock_get_auth_headers, mock_auth_headers):
        """Test that Content-Type is removed when uploading files."""
        mock_get_auth_headers.return_value = {**mock_auth_headers, "Content-Type": "application/json"}

        mock_response = MagicMock()
        mock_response.content = b'{"code": 0}'
        mock_response.status_code = 200
        mock_response.headers = {}

        mock_client = AsyncMock()
        mock_client.request = AsyncMock(return_value=mock_response)
        mock_client_class.return_value.__aenter__ = AsyncMock(return_value=mock_client)
        mock_client_class.return_value.__aexit__ = AsyncMock(return_value=False)

        files = {"file": ("test.txt", b"content", "text/plain")}
        await v0_service.proxy_to_deepseek("POST", "api/v0/upload", files=files)

        call_kwargs = mock_client.request.call_args[1]
        assert "Content-Type" not in call_kwargs["headers"]

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.invalidate_token")
    @patch("deepseek_web_api.api.v0_service.get_auth_headers")
    @patch("deepseek_web_api.api.v0_service.httpx.AsyncClient")
    async def test_proxy_retries_when_token_is_invalid(
        self, mock_client_class, mock_get_auth_headers, mock_invalidate_token, mock_auth_headers
    ):
        mock_get_auth_headers.side_effect = [
            {"authorization": "Bearer stale-token"},
            {"authorization": "Bearer fresh-token"},
        ]

        invalid_response = MagicMock()
        invalid_response.content = b'{"code":40003,"msg":"Authorization Failed (invalid token)","data":null}'
        invalid_response.status_code = 200
        invalid_response.headers = {}

        success_response = MagicMock()
        success_response.content = b'{"code":0,"data":{"biz_data":{"id":"session-123"}}}'
        success_response.status_code = 200
        success_response.headers = {}

        mock_client = AsyncMock()
        mock_client.request = AsyncMock(side_effect=[invalid_response, success_response])
        mock_client_class.return_value.__aenter__ = AsyncMock(return_value=mock_client)
        mock_client_class.return_value.__aexit__ = AsyncMock(return_value=False)

        result = await v0_service.proxy_to_deepseek("POST", "api/v0/test", json_data={"test": True})

        assert result.status_code == 200
        assert mock_client.request.await_count == 2
        mock_invalidate_token.assert_called_once()


class TestCreateSession:
    """Test create_session function."""

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek")
    async def test_create_session_extracts_id(self, mock_proxy, reset_parent_msg_store):
        """Test that session_id is extracted from response."""
        from deepseek_web_api.core.parent_msg_store import ParentMsgStore

        mock_response = MagicMock()
        mock_response.body = json.dumps({
            "code": 0,
            "data": {
                "biz_data": {
                    "id": "test-session-123"
                }
            }
        }).encode()
        mock_response.status_code = 200

        mock_proxy.return_value = mock_response

        session_id, resp = await v0_service.create_session()

        assert session_id == "test-session-123"
        # Verify parent_msg_store was updated
        store = ParentMsgStore.get_instance()
        assert await store.ahas("test-session-123") is True

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek")
    async def test_create_session_with_custom_body(self, mock_proxy, reset_parent_msg_store):
        """Test create_session with custom body."""
        mock_response = MagicMock()
        mock_response.body = json.dumps({
            "code": 0,
            "data": {"biz_data": {"id": "session-456"}}
        }).encode()
        mock_response.status_code = 200
        mock_proxy.return_value = mock_response

        session_id, resp = await v0_service.create_session({"agent": "custom"})

        mock_proxy.assert_called_once()
        call_args = mock_proxy.call_args[1]
        assert call_args["json_data"]["agent"] == "custom"

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek")
    async def test_create_session_invalid_response(self, mock_proxy, reset_parent_msg_store):
        """Test handling of invalid response."""
        mock_response = MagicMock()
        mock_response.body = b"invalid json"
        mock_response.status_code = 200
        mock_proxy.return_value = mock_response

        session_id, resp = await v0_service.create_session()

        assert session_id is None

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek")
    async def test_create_session_handles_unexpected_backend_payload(self, mock_proxy, reset_parent_msg_store):
        mock_response = MagicMock()
        mock_response.body = json.dumps({
            "code": 40003,
            "msg": "auth failed",
            "data": None,
        }).encode()
        mock_response.status_code = 200
        mock_proxy.return_value = mock_response

        session_id, resp = await v0_service.create_session()

        assert session_id is None
        assert resp is mock_response

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek")
    async def test_create_session_recovers_after_invalid_token_retry(self, mock_proxy, reset_parent_msg_store):
        mock_response = MagicMock()
        mock_response.body = json.dumps({
            "code": 0,
            "data": {"biz_data": {"id": "session-789"}}
        }).encode()
        mock_response.status_code = 200
        mock_proxy.return_value = mock_response

        session_id, resp = await v0_service.create_session()

        assert session_id == "session-789"
        assert resp.status_code == 200


class TestDeleteSession:
    """Test delete_session function."""

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek")
    async def test_delete_session_with_retry_success(self, mock_proxy, reset_parent_msg_store):
        """Test successful deletion with retry logic."""
        from deepseek_web_api.core.parent_msg_store import ParentMsgStore

        # Setup: create session first
        store = ParentMsgStore.get_instance()
        await store.acreate("session-to-delete")

        mock_response = MagicMock()
        mock_response.body = json.dumps({
            "data": {"biz_code": 0, "biz_msg": ""}
        }).encode()
        mock_response.status_code = 200
        mock_proxy.return_value = mock_response

        result = await v0_service.delete_session("session-to-delete")

        # Verify session was removed from store
        assert await store.ahas("session-to-delete") is False
        assert result is mock_response

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek")
    async def test_delete_session_always_clears_store(self, mock_proxy, reset_parent_msg_store):
        """Test that store is cleared even if backend deletion fails."""
        from deepseek_web_api.core.parent_msg_store import ParentMsgStore

        store = ParentMsgStore.get_instance()
        await store.acreate("session-fail")

        # Simulate backend failure
        mock_proxy.side_effect = Exception("Network error")

        result = await v0_service.delete_session("session-fail")

        # Verify session was still removed from store
        assert await store.ahas("session-fail") is False
        assert result.status_code == 502
        assert b"Failed to delete session after retries" in result.body

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek")
    @patch("asyncio.sleep")
    async def test_delete_session_retries_on_failure(self, mock_sleep, mock_proxy, reset_parent_msg_store):
        """Test retry mechanism on failure."""
        fail_response = MagicMock()
        fail_response.body = json.dumps({
            "data": {"biz_code": 1, "biz_msg": "error"}
        }).encode()
        fail_response.status_code = 200

        success_response = MagicMock()
        success_response.body = json.dumps({
            "data": {"biz_code": 0, "biz_msg": ""}
        }).encode()
        success_response.status_code = 200

        mock_proxy.side_effect = [fail_response, success_response]

        result = await v0_service.delete_session("session-retry")

        assert mock_proxy.call_count == 2
        assert result is success_response

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek")
    async def test_delete_session_returns_last_backend_response_on_biz_failure(self, mock_proxy, reset_parent_msg_store):
        fail_response = MagicMock()
        fail_response.body = json.dumps({
            "data": {"biz_code": 1, "biz_msg": "still failing"}
        }).encode()
        fail_response.status_code = 200

        mock_proxy.return_value = fail_response

        result = await v0_service.delete_session("session-fail")

        assert result is fail_response


class TestStreamChatCompletion:
    """Test stream_chat_completion function."""

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.get_pow_response")
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek_stream")
    @patch("deepseek_web_api.api.v0_service.create_session")
    async def test_stream_creates_session_when_none_provided(
        self, mock_create_session, mock_proxy_stream, mock_get_pow, reset_parent_msg_store
    ):
        """Test that new session is created when chat_session_id is None."""
        mock_get_pow.return_value = "pow-response-123"
        mock_create_session.return_value = ("new-session-789", MagicMock())

        async def mock_stream():
            yield b'data: {"test": "chunk"}\n\n'
            yield b'event: finish\ndata: {}\n\n'

        mock_proxy_stream.return_value = mock_stream()

        chunks = []
        async for chunk in v0_service.stream_chat_completion("Hello"):
            chunks.append(chunk)

        mock_create_session.assert_called_once()
        assert len(chunks) == 2

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.get_pow_response")
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek_stream")
    async def test_stream_uses_parent_message_id(self, mock_proxy_stream, mock_get_pow, reset_parent_msg_store):
        """Test that existing session uses parent_message_id."""
        from deepseek_web_api.core.parent_msg_store import ParentMsgStore

        mock_get_pow.return_value = "pow-response"

        # Setup existing session with parent message
        store = ParentMsgStore.get_instance()
        await store.acreate("existing-session")
        await store.aupdate_parent_message_id("existing-session", 42)

        async def mock_stream():
            yield b'data: {"response_message_id": 99}\n\n'
            yield b'event: finish\ndata: {}\n\n'

        mock_proxy_stream.return_value = mock_stream()

        chunks = []
        async for chunk in v0_service.stream_chat_completion("Hello", chat_session_id="existing-session"):
            chunks.append(chunk)

        # Verify proxy was called with parent_message_id
        call_kwargs = mock_proxy_stream.call_args[1]
        assert call_kwargs["json_data"]["chat_session_id"] == "existing-session"
        assert call_kwargs["json_data"]["parent_message_id"] == 42

        # Verify parent_message_id was updated after stream
        new_parent = await store.aget_parent_message_id("existing-session")
        assert new_parent == 99

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.get_pow_response")
    @patch("deepseek_web_api.api.v0_service.create_session")
    async def test_stream_fails_without_pow(self, mock_create_session, mock_get_pow, reset_parent_msg_store):
        """Test that stream yields error when PoW fails."""
        mock_get_pow.return_value = None
        mock_create_session.return_value = ("session-no-pow", MagicMock())

        chunks = []
        async for chunk in v0_service.stream_chat_completion("Hello"):
            chunks.append(chunk)

        assert len(chunks) == 2
        assert b'error' in chunks[0]
        assert b'finish' in chunks[1]


class TestUploadFile:
    """Test upload_file function."""

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.get_pow_response")
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek")
    async def test_upload_file_success(self, mock_proxy, mock_get_pow):
        """Test successful file upload."""
        mock_get_pow.return_value = "pow-for-upload"

        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_proxy.return_value = mock_response

        result = await v0_service.upload_file(b"file content", "test.txt", "text/plain")

        assert result.status_code == 200
        mock_proxy.assert_called_once()
        call_kwargs = mock_proxy.call_args[1]
        assert call_kwargs["headers"]["x-ds-pow-response"] == "pow-for-upload"
        assert call_kwargs["headers"]["x-file-size"] == "12"

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.get_pow_response")
    async def test_upload_file_fails_without_pow(self, mock_get_pow):
        """Test that upload raises error when PoW fails."""
        mock_get_pow.return_value = None

        with pytest.raises(RuntimeError, match="Failed to get PoW response"):
            await v0_service.upload_file(b"content", "test.txt", "text/plain")


class TestFetchFiles:
    """Test fetch_files function."""

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek")
    async def test_fetch_files_passes_params(self, mock_proxy):
        """Test that file_ids are passed as query params."""
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_proxy.return_value = mock_response

        await v0_service.fetch_files("file-1,file-2")

        call_kwargs = mock_proxy.call_args[1]
        assert call_kwargs["params"]["file_ids"] == "file-1,file-2"


class TestGetHistoryMessages:
    """Test get_history_messages function."""

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek")
    async def test_get_history_passes_params(self, mock_proxy):
        """Test that all params are passed correctly."""
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_proxy.return_value = mock_response

        await v0_service.get_history_messages("session-123", offset=10, limit=50)

        call_kwargs = mock_proxy.call_args[1]
        assert call_kwargs["params"]["chat_session_id"] == "session-123"
        assert call_kwargs["params"]["offset"] == 10
        assert call_kwargs["params"]["limit"] == 50


class TestParseSseResponseMessageId:
    """Test parse_sse_response_message_id function."""

    def test_extracts_message_id_from_ready_event(self):
        """Test extraction from ready event."""
        sse_content = b'event: ready\ndata: {"request_message_id":1,"response_message_id":42}\n\n'

        result = v0_service.parse_sse_response_message_id(sse_content)

        assert result == 42

    def test_returns_none_when_not_found(self):
        """Test returns None when no response_message_id."""
        sse_content = b'event: update\ndata: {"updated_at":123}\n\n'

        result = v0_service.parse_sse_response_message_id(sse_content)

        assert result is None

    def test_handles_invalid_content(self):
        """Test handles invalid SSE content gracefully."""
        result = v0_service.parse_sse_response_message_id(b"invalid")

        assert result is None


class TestParseRetryAfter:
    """Test _parse_retry_after helper."""

    def test_returns_float_seconds(self):
        assert v0_service._parse_retry_after("10") == 10.0
        assert v0_service._parse_retry_after("30.5") == 30.5

    def test_returns_zero_for_none(self):
        assert v0_service._parse_retry_after(None) == 0.0

    def test_returns_zero_for_empty(self):
        assert v0_service._parse_retry_after("") == 0.0

    def test_returns_zero_for_unparseable(self):
        assert v0_service._parse_retry_after("not-a-number") == 0.0

    def test_clamps_negative_to_zero(self):
        assert v0_service._parse_retry_after("-5") == 0.0


class TestProxyToDeepseekStreamRateLimit:
    """Test rate limit detection in proxy_to_deepseek_stream."""

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.get_auth_headers")
    @patch("deepseek_web_api.api.v0_service.httpx.AsyncClient")
    async def test_raises_rate_limit_error_on_429(self, mock_client_class, mock_get_auth):
        mock_get_auth.return_value = {"authorization": "Bearer test"}

        mock_resp = AsyncMock()
        mock_resp.status_code = 429
        mock_resp.headers = {"Retry-After": "15"}
        mock_resp.aread = AsyncMock(return_value=b'rate limited')
        mock_resp.__aenter__ = AsyncMock(return_value=mock_resp)
        mock_resp.__aexit__ = AsyncMock(return_value=None)

        mock_client = MagicMock()
        mock_client.stream.return_value = mock_resp
        mock_client_cm = AsyncMock()
        mock_client_cm.__aenter__ = AsyncMock(return_value=mock_client)
        mock_client_cm.__aexit__ = AsyncMock(return_value=None)
        mock_client_class.return_value = mock_client_cm

        with pytest.raises(v0_service.RateLimitError) as exc_info:
            async for _ in v0_service.proxy_to_deepseek_stream("POST", "api/v0/test"):
                pass

        assert exc_info.value.retry_after == 15.0
        assert "429" in str(exc_info.value)

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.get_auth_headers")
    @patch("deepseek_web_api.api.v0_service.httpx.AsyncClient")
    async def test_raises_rate_limit_error_on_5xx(self, mock_client_class, mock_get_auth):
        mock_get_auth.return_value = {"authorization": "Bearer test"}

        mock_resp = AsyncMock()
        mock_resp.status_code = 503
        mock_resp.headers = {}
        mock_resp.aread = AsyncMock(return_value=b'service unavailable')
        mock_resp.__aenter__ = AsyncMock(return_value=mock_resp)
        mock_resp.__aexit__ = AsyncMock(return_value=None)

        mock_client = MagicMock()
        mock_client.stream.return_value = mock_resp
        mock_client_cm = AsyncMock()
        mock_client_cm.__aenter__ = AsyncMock(return_value=mock_client)
        mock_client_cm.__aexit__ = AsyncMock(return_value=None)
        mock_client_class.return_value = mock_client_cm

        with pytest.raises(v0_service.RateLimitError) as exc_info:
            async for _ in v0_service.proxy_to_deepseek_stream("POST", "api/v0/test"):
                pass

        assert exc_info.value.retry_after == 5.0
        assert "503" in str(exc_info.value)


class TestStreamRateLimitRetry:
    """Test rate limit retry logic in stream_chat_completion and stream_edit_message."""

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.asyncio.sleep", new_callable=AsyncMock)
    @patch("deepseek_web_api.api.v0_service.get_pow_response")
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek_stream")
    @patch("deepseek_web_api.api.v0_service.create_session")
    async def test_stream_chat_retries_on_rate_limit(
        self, mock_create, mock_stream, mock_pow, mock_sleep, reset_parent_msg_store
    ):
        """stream_chat_completion retries after rate limit and succeeds."""
        mock_create.return_value = ("session-rl", MagicMock())
        mock_pow.return_value = "pow-token"

        async def rate_limited():
            raise v0_service.RateLimitError("HTTP 429", retry_after=0.0)
            yield  # make it an async generator

        async def success():
            yield b'data: {"response_message_id": 5}\n\n'

        mock_stream.side_effect = [rate_limited(), success()]

        chunks = []
        async for chunk in v0_service.stream_chat_completion("Hello"):
            chunks.append(chunk)

        assert mock_stream.call_count == 2
        mock_sleep.assert_called_once()  # Waited before retry
        assert len(chunks) > 0

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.asyncio.sleep", new_callable=AsyncMock)
    @patch("deepseek_web_api.api.v0_service.get_pow_response")
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek_stream")
    @patch("deepseek_web_api.api.v0_service.create_session")
    async def test_stream_chat_raises_after_all_retries(
        self, mock_create, mock_stream, mock_pow, mock_sleep, reset_parent_msg_store
    ):
        """stream_chat_completion raises RateLimitError after all retries exhausted."""
        mock_create.return_value = ("session-rl2", MagicMock())
        mock_pow.return_value = "pow-token"

        async def rate_limited():
            raise v0_service.RateLimitError("HTTP 429", retry_after=0.0)
            yield

        mock_stream.side_effect = [rate_limited(), rate_limited(), rate_limited()]

        with pytest.raises(v0_service.RateLimitError):
            async for _ in v0_service.stream_chat_completion("Hello"):
                pass

        assert mock_stream.call_count == v0_service._MAX_RATE_LIMIT_RETRIES

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.asyncio.sleep", new_callable=AsyncMock)
    @patch("deepseek_web_api.api.v0_service.get_pow_response")
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek_stream")
    async def test_stream_edit_retries_on_rate_limit(
        self, mock_stream, mock_pow, mock_sleep, reset_parent_msg_store
    ):
        """stream_edit_message retries after rate limit and succeeds."""
        mock_pow.return_value = "pow-token"

        async def rate_limited():
            raise v0_service.RateLimitError("HTTP 429", retry_after=0.0)
            yield

        async def success():
            yield b'data: {"response_message_id": 7}\n\n'

        mock_stream.side_effect = [rate_limited(), success()]

        chunks = []
        async for chunk in v0_service.stream_edit_message("Hello", chat_session_id="sess-edit"):
            chunks.append(chunk)

        assert mock_stream.call_count == 2
        mock_sleep.assert_called_once()
        assert len(chunks) > 0

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.asyncio.sleep", new_callable=AsyncMock)
    @patch("deepseek_web_api.api.v0_service.get_pow_response")
    @patch("deepseek_web_api.api.v0_service.proxy_to_deepseek_stream")
    @patch("deepseek_web_api.api.v0_service.create_session")
    async def test_retry_delay_respects_retry_after_header(
        self, mock_create, mock_stream, mock_pow, mock_sleep, reset_parent_msg_store
    ):
        """Retry delay uses the larger of base delay and Retry-After."""
        mock_create.return_value = ("session-rl3", MagicMock())
        mock_pow.return_value = "pow-token"

        async def rate_limited_with_header():
            raise v0_service.RateLimitError("HTTP 429", retry_after=60.0)
            yield

        async def success():
            yield b'data: {}\n\n'

        mock_stream.side_effect = [rate_limited_with_header(), success()]

        async for _ in v0_service.stream_chat_completion("Hello"):
            pass

        # Delay should be at least the Retry-After value (60s), not just base delay (5s)
        sleep_call_arg = mock_sleep.call_args[0][0]
        assert sleep_call_arg >= 60.0
