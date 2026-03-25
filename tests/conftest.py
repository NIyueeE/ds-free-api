"""Pytest configuration and shared fixtures."""

import logging
import os
import sys

import pytest

# Disable auto-init for all tests to avoid side effects
os.environ["DISABLE_AUTO_INIT"] = "1"

# Configure logging for tests
logging.basicConfig(
    level=logging.DEBUG,
    format="%(levelname)s | %(name)s | %(message)s",
    stream=sys.stdout,
)


@pytest.fixture(autouse=True)
def reset_core_singletons():
    """Reset core module singletons before each test.

    This ensures tests don't interfere with each other.
    """
    # Import here to avoid circular imports
    from src.deepseek_web_api.core import ParentMsgStore

    # Reset ParentMsgStore
    ParentMsgStore._instance = None
    ParentMsgStore._lock = None

    yield
