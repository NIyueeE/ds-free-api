import os
import urllib.parse

import pytest
from openai import OpenAI, APIConnectionError

BASE_URL = os.getenv("TEST_BASE_URL", "http://127.0.0.1:5317/v1")
API_KEY = os.getenv("TEST_API_KEY", "sk-test")


@pytest.fixture(scope="session")
def client():
    return OpenAI(base_url=BASE_URL, api_key=API_KEY)


def pytest_runtest_setup(item):
    if "requires_server" in item.keywords:
        try:
            c = OpenAI(base_url=BASE_URL, api_key=API_KEY)
            c.models.list(timeout=5)
        except APIConnectionError as exc:
            pytest.skip(f"本地服务未启动或无法连接: {exc}")


@pytest.fixture(autouse=True)
def delay_between_tests():
    """每个测试结束后休眠 2.5 秒，避免对单一账号/后端造成过高频率。"""
    yield
    import time
    time.sleep(2.5)
