"""Unit tests for session_pool.py - stateless session pool."""

import asyncio
import time
from unittest.mock import AsyncMock, patch

import pytest


from deepseek_web_api.api.openai.chat_completions.session_pool import (
    StatelessSession,
    StatelessSessionPool,
    SessionPoolError,
    SessionPoolFullError,
    get_pool,
)


class TestStatelessSession:
    """Tests for StatelessSession dataclass."""

    def test_creation_defaults(self):
        session = StatelessSession(chat_session_id="test-123")
        assert session.chat_session_id == "test-123"
        assert session.is_initialized is False
        assert isinstance(session.last_access_time, float)
        assert isinstance(session.lock, asyncio.Lock)

    def test_creation_with_values(self):
        session = StatelessSession(
            chat_session_id="test-456",
            is_initialized=True,
        )
        assert session.is_initialized is True


class TestStatelessSessionPool:
    """Tests for StatelessSessionPool class."""

    @pytest.fixture
    def pool(self):
        return StatelessSessionPool(max_idle_seconds=60, pool_size=5, acquire_timeout=2.0)

    @pytest.mark.asyncio
    async def test_acquire_returns_session(self, pool):
        with patch.object(pool, '_create_session', new_callable=AsyncMock) as mock_create:
            mock_create.return_value = StatelessSession(chat_session_id="new-session")
            session = await pool.acquire()
            assert session is not None
            assert session.chat_session_id == "new-session"

    @pytest.mark.asyncio
    async def test_acquire_reuses_idle_session(self, pool):
        # Pre-add a session
        session = StatelessSession(chat_session_id="idle-session")
        pool._sessions["idle-session"] = session

        acquired = await pool.acquire()
        assert acquired.chat_session_id == "idle-session"
        assert session.lock.locked()

    @pytest.mark.asyncio
    async def test_acquire_creates_new_when_all_busy(self, pool):
        # Add a locked session
        busy_session = StatelessSession(chat_session_id="busy-session")
        await busy_session.lock.acquire()
        pool._sessions["busy-session"] = busy_session

        with patch.object(pool, '_create_session', new_callable=AsyncMock) as mock_create:
            mock_create.return_value = StatelessSession(chat_session_id="new-session")
            acquired = await pool.acquire()
            assert acquired.chat_session_id == "new-session"

    @pytest.mark.asyncio
    async def test_release_unlocks_session(self, pool):
        session = StatelessSession(chat_session_id="test-session")
        await session.lock.acquire()
        pool._sessions["test-session"] = session

        await pool.release(session)
        assert not session.lock.locked()

    @pytest.mark.asyncio
    async def test_release_with_error_resets_initialized(self, pool):
        session = StatelessSession(chat_session_id="test-session", is_initialized=True)
        await session.lock.acquire()

        await pool.release(session, error=True)
        assert session.is_initialized is False
        assert not session.lock.locked()

    @pytest.mark.asyncio
    async def test_release_updates_last_access_time(self, pool):
        session = StatelessSession(chat_session_id="test-session")
        original_time = session.last_access_time
        await session.lock.acquire()

        await asyncio.sleep(0.01)
        await pool.release(session)
        assert session.last_access_time >= original_time

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.delete_session", new_callable=AsyncMock)
    async def test_cleanup_idle_removes_old_sessions(self, mock_delete_session, pool):
        mock_delete_session.return_value.status_code = 200

        old_session = StatelessSession(chat_session_id="old-session")
        old_session.last_access_time = time.time() - 100
        pool._sessions["old-session"] = old_session

        new_session = StatelessSession(chat_session_id="new-session")
        pool._sessions["new-session"] = new_session

        removed = await pool.cleanup_idle()
        assert removed == 1
        assert "old-session" not in pool._sessions
        assert "new-session" in pool._sessions
        mock_delete_session.assert_awaited_once_with("old-session")

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.delete_session", new_callable=AsyncMock)
    async def test_cleanup_does_not_remove_locked_sessions(self, mock_delete_session, pool):
        session = StatelessSession(chat_session_id="locked-session")
        await session.lock.acquire()
        session.last_access_time = time.time() - 100
        pool._sessions["locked-session"] = session

        removed = await pool.cleanup_idle()
        assert removed == 0
        assert "locked-session" in pool._sessions
        mock_delete_session.assert_not_awaited()

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.delete_session", new_callable=AsyncMock)
    async def test_cleanup_idle_empty_pool(self, mock_delete_session, pool):
        removed = await pool.cleanup_idle()
        assert removed == 0
        mock_delete_session.assert_not_awaited()

    def test_size_property(self, pool):
        pool._sessions["a"] = StatelessSession(chat_session_id="a")
        pool._sessions["b"] = StatelessSession(chat_session_id="b")
        assert pool.size == 2

    @pytest.mark.asyncio
    async def test_available_count_property(self, pool):
        s1 = StatelessSession(chat_session_id="s1")
        s2 = StatelessSession(chat_session_id="s2")
        pool._sessions["s1"] = s1
        pool._sessions["s2"] = s2

        assert pool.available_count == 2

        await s1.lock.acquire()
        assert pool.available_count == 1

    @pytest.mark.asyncio
    async def test_create_session_raises_on_failure(self, pool):
        with patch('deepseek_web_api.api.v0_service.create_session', new_callable=AsyncMock) as mock_create:
            mock_create.return_value = (None, None)
            with pytest.raises(SessionPoolError):
                await pool._create_session()

    @pytest.mark.asyncio
    async def test_start_cleanup_creates_task(self, pool):
        """Test that start_cleanup creates a background task."""
        assert pool._cleanup_task is None
        pool.start_cleanup()
        # Task should be created (may still be pending)
        assert pool._cleanup_task is not None
        await pool.stop_cleanup()

    @pytest.mark.asyncio
    async def test_start_cleanup_idempotent(self, pool):
        """Test that calling start_cleanup twice doesn't create multiple tasks."""
        pool.start_cleanup()
        first_task = pool._cleanup_task
        pool.start_cleanup()  # Should not create new task
        assert pool._cleanup_task is first_task
        await pool.stop_cleanup()

    @pytest.mark.asyncio
    async def test_stop_cleanup_cancels_task(self, pool):
        """Test that stop_cleanup cancels the background task."""
        pool.start_cleanup()
        await asyncio.sleep(0.01)  # Let task start
        await pool.stop_cleanup()
        assert pool._cleanup_task is None

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.delete_session", new_callable=AsyncMock)
    async def test_cleanup_loop_removes_idle_sessions(self, mock_delete_session, pool):
        """Test that the cleanup loop actually removes idle sessions."""
        mock_delete_session.return_value.status_code = 200

        # Add a session that will be considered idle
        old_session = StatelessSession(chat_session_id="old-session")
        old_session.last_access_time = time.time() - 100
        pool._sessions["old-session"] = old_session

        # Start cleanup with short timeout
        pool._max_idle_seconds = 1
        pool.start_cleanup()

        # Wait for cleanup to run (should run after max_idle_seconds / 2 = 0.5s)
        await asyncio.sleep(0.6)

        await pool.stop_cleanup()

        # Session should be removed
        assert "old-session" not in pool._sessions
        mock_delete_session.assert_awaited_once_with("old-session")

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.delete_session", new_callable=AsyncMock)
    async def test_cleanup_loop_keeps_active_sessions(self, mock_delete_session, pool):
        """Test that the cleanup loop does not remove active sessions."""
        # Add an active session (recently accessed)
        active_session = StatelessSession(chat_session_id="active-session")
        pool._sessions["active-session"] = active_session

        pool._max_idle_seconds = 1
        pool.start_cleanup()

        # Wait for cleanup to run
        await asyncio.sleep(0.6)

        await pool.stop_cleanup()

        # Active session should still be there
        assert "active-session" in pool._sessions
        mock_delete_session.assert_not_awaited()

    @pytest.mark.asyncio
    @patch("deepseek_web_api.api.v0_service.delete_session", new_callable=AsyncMock)
    async def test_cleanup_idle_logs_remote_delete_failures_but_still_removes(self, mock_delete_session, pool):
        mock_delete_session.side_effect = RuntimeError("cleanup failed")

        old_session = StatelessSession(chat_session_id="old-session")
        old_session.last_access_time = time.time() - 100
        pool._sessions["old-session"] = old_session

        removed = await pool.cleanup_idle()

        assert removed == 1
        assert "old-session" not in pool._sessions
        mock_delete_session.assert_awaited_once_with("old-session")

    @pytest.mark.asyncio
    async def test_acquire_raises_when_pool_full_and_timeout(self, pool):
        """Pool at capacity with all sessions locked — acquire should time out."""
        pool._acquire_timeout = 0.1  # Short timeout for test speed

        # Fill pool to capacity
        for i in range(pool._pool_size):
            s = StatelessSession(chat_session_id=f"busy-{i}")
            await s.lock.acquire()
            pool._sessions[f"busy-{i}"] = s

        with pytest.raises(SessionPoolFullError):
            await pool.acquire()

    @pytest.mark.asyncio
    async def test_acquire_unblocks_when_session_released(self, pool):
        """Waiter in acquire() should receive the session once release() is called."""
        pool._acquire_timeout = 5.0

        # Fill pool to capacity with locked sessions
        sessions = []
        for i in range(pool._pool_size):
            s = StatelessSession(chat_session_id=f"busy-{i}")
            await s.lock.acquire()
            pool._sessions[f"busy-{i}"] = s
            sessions.append(s)

        # Start acquire in background — will block until a session is released
        acquire_task = asyncio.create_task(pool.acquire())
        await asyncio.sleep(0.05)  # Let acquire() reach the wait state

        # Release one session
        await pool.release(sessions[0])

        acquired = await asyncio.wait_for(acquire_task, timeout=2.0)
        assert acquired.chat_session_id == "busy-0"
        assert acquired.lock.locked()

    @pytest.mark.asyncio
    async def test_acquire_respects_pending_creates_as_reserved_slots(self, pool):
        """_pending_creates should count toward effective pool size to prevent over-creation."""
        # Pool size 5, 0 sessions, 4 pending creates
        pool._pending_creates = 4

        created_count = 0

        async def slow_create():
            nonlocal created_count
            await asyncio.sleep(0.05)
            created_count += 1
            return StatelessSession(chat_session_id=f"created-{created_count}")

        with patch.object(pool, '_create_session', new=slow_create):
            # Only 1 slot left — only one concurrent create should proceed immediately
            task1 = asyncio.create_task(pool.acquire())
            await asyncio.sleep(0.01)
            # Effective size is now 5 (0 + 4 + 1 pending), pool is full
            # Manually verify _pending_creates got incremented for task1
            assert pool._pending_creates == 5

            # Cancel task1 to avoid hanging test (it's blocked on slow_create)
            task1.cancel()
            try:
                await task1
            except (asyncio.CancelledError, Exception):
                pass


class TestGetPool:
    """Tests for get_pool function."""

    @pytest.mark.asyncio
    async def test_get_pool_returns_same_instance(self):
        # Reset global pool
        import deepseek_web_api.api.openai.chat_completions.session_pool as sp_module
        sp_module._pool = None

        pool1 = await get_pool()
        pool2 = await get_pool()
        assert pool1 is pool2

        # Cleanup
        await pool1.stop_cleanup()
        sp_module._pool = None

    @pytest.mark.asyncio
    async def test_get_pool_creates_with_defaults(self):
        import deepseek_web_api.api.openai.chat_completions.session_pool as sp_module
        sp_module._pool = None

        pool = await get_pool()
        assert isinstance(pool, StatelessSessionPool)
        assert pool._max_idle_seconds == 300
        assert pool._pool_size == 10
        assert pool._acquire_timeout == 30.0

        # Cleanup
        await pool.stop_cleanup()
        sp_module._pool = None

    @pytest.mark.asyncio
    async def test_get_pool_starts_cleanup(self):
        """Test that get_pool automatically starts the cleanup loop."""
        import deepseek_web_api.api.openai.chat_completions.session_pool as sp_module
        sp_module._pool = None

        pool = await get_pool()
        assert pool._cleanup_task is not None

        # Cleanup
        await pool.stop_cleanup()
        sp_module._pool = None
