"""Session state store for mapping chat_session_id to parent_message_id."""

import asyncio
from threading import Lock as ThreadLock


class SessionStore:
    _instance = None
    _init_lock = ThreadLock()  # Sync lock for singleton init (thread-safe)
    _lock: asyncio.Lock | None = None  # Async lock for session operations

    @classmethod
    def get_instance(cls):
        """Get singleton instance (sync, for module import)."""
        if cls._instance is None:
            with cls._init_lock:
                if cls._instance is None:
                    cls._instance = cls()
                    cls._lock = asyncio.Lock()
        return cls._instance

    @classmethod
    async def aget_instance(cls):
        """Get singleton instance (async, for use in async contexts)."""
        return cls.get_instance()

    def __init__(self):
        self._sessions: dict[str, int | None] = {}  # chat_session_id -> parent_message_id

    async def acreate_session(self, chat_session_id: str) -> None:
        """Create new session with null parent_message_id."""
        async with self._lock:
            self._sessions[chat_session_id] = None

    async def aget_parent_message_id(self, chat_session_id: str) -> int | None:
        """Get parent_message_id for session."""
        async with self._lock:
            return self._sessions.get(chat_session_id)

    async def aupdate_parent_message_id(self, chat_session_id: str, message_id: int) -> None:
        """Update parent_message_id after receiving response."""
        async with self._lock:
            self._sessions[chat_session_id] = message_id

    async def adelete_session(self, chat_session_id: str) -> bool:
        """Delete session, return True if existed."""
        async with self._lock:
            if chat_session_id in self._sessions:
                del self._sessions[chat_session_id]
                return True
            return False

    async def ahas_session(self, chat_session_id: str) -> bool:
        """Check if session exists."""
        async with self._lock:
            return chat_session_id in self._sessions

    async def aget_all_sessions(self) -> list[str]:
        """Get all session IDs."""
        async with self._lock:
            return list(self._sessions.keys())

    # Sync versions for backward compatibility (used in non-async contexts)
    def create_session(self, chat_session_id: str) -> None:
        """Create new session with null parent_message_id."""
        self._sessions[chat_session_id] = None

    def get_parent_message_id(self, chat_session_id: str) -> int | None:
        """Get parent_message_id for session."""
        return self._sessions.get(chat_session_id)

    def update_parent_message_id(self, chat_session_id: str, message_id: int) -> None:
        """Update parent_message_id after receiving response."""
        self._sessions[chat_session_id] = message_id

    def delete_session(self, chat_session_id: str) -> bool:
        """Delete session, return True if existed."""
        if chat_session_id in self._sessions:
            del self._sessions[chat_session_id]
            return True
        return False

    def has_session(self, chat_session_id: str) -> bool:
        """Check if session exists."""
        return chat_session_id in self._sessions

    def get_all_sessions(self) -> list[str]:
        """Get all session IDs."""
        return list(self._sessions.keys())
