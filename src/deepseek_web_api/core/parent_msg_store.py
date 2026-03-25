"""Parent message ID store for mapping chat_session_id to parent_message_id.

This is a technical requirement from DeepSeek API - to continue a conversation,
the correct parent_message_id must be sent with each request.
"""

import asyncio
import logging
from threading import Lock as ThreadLock

logger = logging.getLogger(__name__)


class ParentMsgStore:
    """Store for mapping chat_session_id to parent_message_id.

    DeepSeek API requires parent_message_id to continue a conversation.
    This store maintains that mapping in memory.
    """

    _instance = None
    _init_lock = ThreadLock()  # Sync lock for singleton init
    _lock: asyncio.Lock | None = None  # Async lock for operations

    @classmethod
    def get_instance(cls):
        """Get singleton instance (sync, for module import)."""
        if cls._instance is None:
            with cls._init_lock:
                if cls._instance is None:
                    cls._instance = cls()
                    cls._lock = asyncio.Lock()
                    logger.debug("[ParentMsgStore] Singleton instance created")
        return cls._instance

    @classmethod
    async def aget_instance(cls):
        """Get singleton instance (async, for use in async contexts)."""
        return cls.get_instance()

    def __init__(self):
        self._store: dict[str, int | None] = {}  # chat_session_id -> parent_message_id

    async def acreate(self, chat_session_id: str) -> None:
        """Create new entry with null parent_message_id."""
        async with self._lock:
            self._store[chat_session_id] = None
            logger.debug(f"[ParentMsgStore] Created session: {chat_session_id}")

    async def aget_parent_message_id(self, chat_session_id: str) -> int | None:
        """Get parent_message_id for session."""
        async with self._lock:
            msg_id = self._store.get(chat_session_id)
            logger.debug(f"[ParentMsgStore] Get parent_message_id: {chat_session_id} -> {msg_id}")
            return msg_id

    async def aupdate_parent_message_id(self, chat_session_id: str, message_id: int) -> None:
        """Update parent_message_id after receiving response."""
        async with self._lock:
            self._store[chat_session_id] = message_id
            logger.debug(f"[ParentMsgStore] Updated parent_message_id: {chat_session_id} -> {message_id}")

    async def adelete(self, chat_session_id: str) -> bool:
        """Delete session, return True if existed."""
        async with self._lock:
            existed = chat_session_id in self._store
            if existed:
                self._store.pop(chat_session_id, None)
            logger.debug(f"[ParentMsgStore] Deleted session: {chat_session_id}, existed={existed}")
            return existed

    async def ahas(self, chat_session_id: str) -> bool:
        """Check if session exists."""
        async with self._lock:
            exists = chat_session_id in self._store
            logger.debug(f"[ParentMsgStore] Has session: {chat_session_id} -> {exists}")
            return exists

    async def aget_all(self) -> list[str]:
        """Get all session IDs."""
        async with self._lock:
            return list(self._store.keys())
