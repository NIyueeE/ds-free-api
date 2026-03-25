"""Tests for core/parent_msg_store.py"""

import pytest
import asyncio

import sys
sys.path.insert(0, "src")

from deepseek_web_api.core import ParentMsgStore


class TestParentMsgStore:
    """Test ParentMsgStore module."""

    @pytest.mark.asyncio
    async def test_singleton(self):
        """Test that get_instance returns the same instance."""
        store1 = ParentMsgStore.get_instance()
        store2 = ParentMsgStore.get_instance()

        assert store1 is store2

    @pytest.mark.asyncio
    async def test_singleton_async(self):
        """Test that aget_instance returns the same instance."""
        store1 = await ParentMsgStore.aget_instance()
        store2 = ParentMsgStore.get_instance()

        assert store1 is store2

    @pytest.mark.asyncio
    async def test_acreate(self):
        """Test creating a session."""
        store = ParentMsgStore.get_instance()

        await store.acreate("session-1")

        assert await store.ahas("session-1")
        assert await store.aget_parent_message_id("session-1") is None

    @pytest.mark.asyncio
    async def test_aupdate_parent_message_id(self):
        """Test updating parent_message_id."""
        store = ParentMsgStore.get_instance()

        await store.acreate("session-1")
        await store.aupdate_parent_message_id("session-1", 12345)

        msg_id = await store.aget_parent_message_id("session-1")
        assert msg_id == 12345

    @pytest.mark.asyncio
    async def test_adelete(self):
        """Test deleting a session."""
        store = ParentMsgStore.get_instance()

        await store.acreate("session-1")
        assert await store.ahas("session-1")

        deleted = await store.adelete("session-1")
        assert deleted is True
        assert not await store.ahas("session-1")

    @pytest.mark.asyncio
    async def test_adelete_nonexistent(self):
        """Test deleting nonexistent session returns False."""
        store = ParentMsgStore.get_instance()

        deleted = await store.adelete("nonexistent")
        assert deleted is False

    @pytest.mark.asyncio
    async def test_ahas(self):
        """Test checking session existence."""
        store = ParentMsgStore.get_instance()

        await store.acreate("session-1")
        assert await store.ahas("session-1")
        assert not await store.ahas("session-2")

    @pytest.mark.asyncio
    async def test_aget_all(self):
        """Test getting all session IDs."""
        store = ParentMsgStore.get_instance()

        await store.acreate("session-1")
        await store.acreate("session-2")

        all_sessions = await store.aget_all()

        assert "session-1" in all_sessions
        assert "session-2" in all_sessions

    @pytest.mark.asyncio
    async def test_multiple_sessions(self):
        """Test managing multiple sessions."""
        store = ParentMsgStore.get_instance()

        # Create multiple sessions with different message IDs
        await store.acreate("session-1")
        await store.aupdate_parent_message_id("session-1", 100)

        await store.acreate("session-2")
        await store.aupdate_parent_message_id("session-2", 200)

        await store.acreate("session-3")

        # Verify each session has correct message ID
        assert await store.aget_parent_message_id("session-1") == 100
        assert await store.aget_parent_message_id("session-2") == 200
        assert await store.aget_parent_message_id("session-3") is None

    @pytest.mark.asyncio
    async def test_concurrent_operations(self):
        """Test concurrent operations don't cause issues."""
        store = ParentMsgStore.get_instance()

        # Run multiple operations concurrently
        async def create_and_update(session_id: str, msg_id: int):
            await store.acreate(session_id)
            await store.aupdate_parent_message_id(session_id, msg_id)

        await asyncio.gather(
            create_and_update("concurrent-1", 1),
            create_and_update("concurrent-2", 2),
            create_and_update("concurrent-3", 3),
        )

        # Verify all sessions were created correctly
        assert await store.aget_parent_message_id("concurrent-1") == 1
        assert await store.aget_parent_message_id("concurrent-2") == 2
        assert await store.aget_parent_message_id("concurrent-3") == 3
