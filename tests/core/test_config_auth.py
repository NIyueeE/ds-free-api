"""Tests for auth-related config normalization helpers."""

import sys

sys.path.insert(0, "src")

from deepseek_web_api.core import config


class TestAuthConfig:
    def test_empty_config_returns_empty_tokens(self, monkeypatch):
        monkeypatch.setattr(config, "CONFIG", {})
        assert config.get_auth_tokens() == []

    def test_empty_auth_section_returns_empty_tokens(self, monkeypatch):
        monkeypatch.setattr(config, "CONFIG", {"auth": {}})
        assert config.get_auth_tokens() == []

    def test_empty_tokens_list_returns_empty_list(self, monkeypatch):
        monkeypatch.setattr(config, "CONFIG", {"auth": {"tokens": []}})
        assert config.get_auth_tokens() == []

    def test_valid_tokens_returned(self, monkeypatch):
        monkeypatch.setattr(
            config, "CONFIG", {"auth": {"tokens": ["sk-xxx", "sk-yyy"]}}
        )
        assert config.get_auth_tokens() == ["sk-xxx", "sk-yyy"]

    def test_tokens_are_stripped(self, monkeypatch):
        monkeypatch.setattr(
            config, "CONFIG", {"auth": {"tokens": ["  sk-xxx  ", "  sk-yyy  "]}}
        )
        assert config.get_auth_tokens() == ["sk-xxx", "sk-yyy"]

    def test_empty_strings_filtered_out(self, monkeypatch):
        monkeypatch.setattr(
            config, "CONFIG", {"auth": {"tokens": ["sk-xxx", "", "   ", "sk-yyy"]}}
        )
        assert config.get_auth_tokens() == ["sk-xxx", "sk-yyy"]

    def test_non_list_auth_section_returns_empty_list(self, monkeypatch):
        monkeypatch.setattr(config, "CONFIG", {"auth": {"tokens": "not-a-list"}})
        assert config.get_auth_tokens() == []

    def test_non_dict_auth_section_returns_empty_list(self, monkeypatch):
        monkeypatch.setattr(config, "CONFIG", {"auth": "not-a-dict"})
        assert config.get_auth_tokens() == []

    def test_mixed_types_converted_to_strings(self, monkeypatch):
        monkeypatch.setattr(
            config, "CONFIG", {"auth": {"tokens": ["sk-xxx", 123, True, "sk-yyy"]}}
        )
        assert config.get_auth_tokens() == ["sk-xxx", "123", "True", "sk-yyy"]
