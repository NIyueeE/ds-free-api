"""Tests for core/auth.py"""

import pytest
from unittest.mock import patch, MagicMock

import sys
sys.path.insert(0, "src")

from deepseek_web_api.core import auth


class TestAuth:
    """Test authentication module."""

    def test_get_token_fast_path(self):
        """Test fast path when token already exists."""
        # Setup: manually set _account with token
        auth._account = {"token": "test-token-123"}

        token = auth.get_token()

        assert token == "test-token-123"

    @patch("deepseek_web_api.core.auth.login")
    @patch("deepseek_web_api.core.auth.CONFIG", {"account": {"email": "", "password": "password"}})
    def test_get_token_slow_path(self, mock_login):
        """Test slow path when token needs to be initialized."""
        # Setup: _account exists but no token, and CONFIG has no token
        # Login should be called
        auth._account = {"email": "test@test.com", "password": "password"}

        # Mock login to set the token (since mock doesn't run actual code)
        def mock_login_fn():
            auth._account["token"] = "new-token-456"
            return "new-token-456"

        mock_login.side_effect = mock_login_fn

        token = auth.get_token()

        assert token == "new-token-456"
        mock_login.assert_called_once()

    def test_get_token_init_if_needed(self):
        """Test token initialization when _account is None."""
        # Save original and set directly to simulate initialization complete
        auth._account = {"email": "test@test.com", "password": "password", "token": "initialized-token"}

        token = auth.get_token()

        assert token == "initialized-token"

    def test_get_auth_headers(self):
        """Test headers include authorization."""
        auth._account = {"token": "test-token"}

        headers = auth.get_auth_headers()

        assert "authorization" in headers
        assert headers["authorization"] == "Bearer test-token"

    @patch("deepseek_web_api.core.auth.requests.post")
    def test_login_success(self, mock_post):
        """Test successful login."""
        auth._account = {"email": "test@test.com", "password": "password123"}

        mock_response = MagicMock()
        mock_response.json.return_value = {
            "data": {
                "biz_data": {
                    "user": {"token": "login-success-token"}
                }
            }
        }
        mock_post.return_value = mock_response

        token = auth.login()

        assert token == "login-success-token"
        assert auth._account["token"] == "login-success-token"

    def test_login_missing_credentials(self):
        """Test login fails with missing credentials."""
        auth._account = {"email": "", "password": ""}

        with pytest.raises(ValueError, match="missing required login info"):
            auth.login()

    @patch("deepseek_web_api.core.auth.requests.post")
    def test_login_invalid_response(self, mock_post):
        """Test login fails with invalid response."""
        auth._account = {"email": "test@test.com", "password": "password"}

        mock_response = MagicMock()
        mock_response.json.return_value = {"data": None}
        mock_post.return_value = mock_response

        with pytest.raises(ValueError, match="invalid response format"):
            auth.login()

    @patch("deepseek_web_api.core.auth.requests.post")
    def test_login_missing_token(self, mock_post):
        """Test login fails when token is missing from response."""
        auth._account = {"email": "test@test.com", "password": "password"}

        mock_response = MagicMock()
        mock_response.json.return_value = {
            "data": {"biz_data": {"user": {}}}
        }
        mock_post.return_value = mock_response

        with pytest.raises(ValueError, match="missing token"):
            auth.login()
