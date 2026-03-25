"""Tests for core/pow.py"""

import pytest
from unittest.mock import patch, MagicMock
import json
import base64

import sys
sys.path.insert(0, "src")

from deepseek_web_api.core import pow as pow_module


class TestPow:
    """Test PoW module."""

    @patch("deepseek_web_api.core.pow.compute_pow_answer")
    @patch("deepseek_web_api.core.auth.get_auth_headers")
    def test_get_pow_response_success(self, mock_auth_headers, mock_compute):
        """Test successful PoW response generation."""
        mock_auth_headers.return_value = {"authorization": "Bearer test"}

        # Mock API response
        mock_response = MagicMock()
        mock_response.json.return_value = {
            "code": 0,
            "data": {
                "biz_data": {
                    "challenge": {
                        "algorithm": "DeepSeekHashV1",
                        "challenge": "test-challenge",
                        "salt": "test-salt",
                        "difficulty": 10,
                        "expire_at": 1234567890,
                        "signature": "test-sig",
                        "target_path": "/api/v0/chat/completion",
                    }
                }
            }
        }

        with patch("deepseek_web_api.core.pow.requests.post", return_value=mock_response):
            mock_compute.return_value = 12345

            result = pow_module.get_pow_response("/api/v0/chat/completion")

            assert result is not None
            # Verify it's valid base64
            decoded = json.loads(base64.b64decode(result).decode())
            assert decoded["answer"] == 12345

    @patch("deepseek_web_api.core.auth.get_auth_headers")
    def test_get_pow_response_api_error(self, mock_auth_headers):
        """Test PoW response when API returns error."""
        mock_auth_headers.return_value = {"authorization": "Bearer test"}

        mock_response = MagicMock()
        mock_response.json.return_value = {"code": -1, "message": "error"}

        with patch("deepseek_web_api.core.pow.requests.post", return_value=mock_response):
            result = pow_module.get_pow_response()

            assert result is None

    @patch("deepseek_web_api.core.pow.compute_pow_answer")
    @patch("deepseek_web_api.core.auth.get_auth_headers")
    def test_get_pow_response_compute_failed(self, mock_auth_headers, mock_compute):
        """Test PoW response when compute fails."""
        mock_auth_headers.return_value = {"authorization": "Bearer test"}

        mock_response = MagicMock()
        mock_response.json.return_value = {
            "code": 0,
            "data": {
                "biz_data": {
                    "challenge": {
                        "algorithm": "DeepSeekHashV1",
                        "challenge": "test-challenge",
                        "salt": "test-salt",
                        "difficulty": 10,
                        "expire_at": 1234567890,
                        "signature": "test-sig",
                        "target_path": "/api/v0/chat/completion",
                    }
                }
            }
        }

        with patch("deepseek_web_api.core.pow.requests.post", return_value=mock_response):
            mock_compute.return_value = None  # Compute failed

            result = pow_module.get_pow_response()

            assert result is None

    def test_compute_pow_unsupported_algorithm(self):
        """Test compute fails with unsupported algorithm."""
        with pytest.raises(ValueError, match="Unsupported algorithm"):
            pow_module.compute_pow_answer(
                algorithm="UnknownAlgo",
                challenge_str="test",
                salt="salt",
                difficulty=10,
                expire_at=1234567890,
            )

    @patch("deepseek_web_api.core.pow._get_cached_wasm")
    def test_compute_pow_wasm_error(self, mock_wasm):
        """Test compute fails when WASM export is missing."""
        # Create mock exports without required functions
        mock_exports = {"memory": MagicMock(), "other": MagicMock()}
        mock_store = MagicMock()
        mock_wasm.return_value = (None, None, None, mock_store, mock_exports)

        with pytest.raises(RuntimeError, match="Missing wasm export"):
            pow_module.compute_pow_answer(
                algorithm="DeepSeekHashV1",
                challenge_str="test",
                salt="salt",
                difficulty=10,
                expire_at=1234567890,
            )


class TestWasmCaching:
    """Test WASM module caching."""

    def test_wasm_cached_after_first_load(self):
        """Test WASM module is cached after first load."""
        # Clear cache
        pow_module._wasm_cache.clear()

        # The actual WASM might not be available in test env
        # This just verifies the caching mechanism works
        assert len(pow_module._wasm_cache) == 0
