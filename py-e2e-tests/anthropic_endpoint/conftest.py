import os

import httpx
import pytest
from anthropic import Anthropic, APIConnectionError, AuthenticationError

BASE_URL = os.getenv("TEST_BASE_URL", "http://127.0.0.1:5317/anthropic")
API_KEY = os.getenv("TEST_API_KEY", "sk-test")


def _make_client():
    return Anthropic(
        base_url=BASE_URL,
        api_key=API_KEY,
        default_headers={"Authorization": f"Bearer {API_KEY}"},
        http_client=httpx.Client(timeout=30),
    )


@pytest.fixture(scope="session")
def client():
    return _make_client()


def pytest_runtest_setup(item):
    if "requires_server" in item.keywords:
        try:
            c = _make_client()
            c.models.list(timeout=5)
        except (APIConnectionError, AuthenticationError) as exc:
            pytest.skip(f"本地服务未启动或无法连接: {exc}")


@pytest.fixture(autouse=True)
def delay_between_tests():
    """每个测试结束后休眠 2.5 秒，避免对单一账号/后端造成过高频率。"""
    yield
    import time

    time.sleep(2.5)
