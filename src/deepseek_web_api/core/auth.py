"""Authentication for single account mode."""

import logging
import threading

from curl_cffi import requests

from .config import BASE_HEADERS, CONFIG, DEEPSEEK_LOGIN_URL, DEFAULT_IMPERSONATE, save_config

logger = logging.getLogger(__name__)

# Global single account
_account = None
_token_lock = threading.Lock()  # Protect _account check-and-set


def init_single_account():
    """Initialize single account from config (lazy, no login)."""
    global _account
    if _account is None:
        _account = CONFIG.get("account")
    if not _account:
        raise ValueError("No account configured")
    # No auto-login - token will be obtained on first use


def login() -> str:
    """Login and get new token, then save to config."""
    global _account
    email = _account.get("email", "").strip()
    mobile = _account.get("mobile", "").strip()
    password = _account.get("password", "").strip()

    if not password or (not email and not mobile):
        logger.error("[login] Missing email/mobile or password")
        raise ValueError("Account missing required login info (email or mobile and password required)")

    payload = {
        "password": password,
        "device_id": "deepseek_to_api",
        "os": "android",
    }
    if email:
        payload["email"] = email
    else:
        payload["mobile"] = mobile
        payload["area_code"] = _account.get("area_code")

    logger.info("[login] Attempting login...")
    resp = requests.post(
        DEEPSEEK_LOGIN_URL,
        headers=BASE_HEADERS,
        json=payload,
        impersonate=DEFAULT_IMPERSONATE,
    )
    data = resp.json()
    resp.close()

    if data.get("data") is None or data["data"].get("biz_data") is None:
        logger.error("[login] Invalid response format from DeepSeek")
        raise ValueError("Login failed: invalid response format")

    new_token = data["data"]["biz_data"]["user"].get("token")
    if not new_token:
        logger.error("[login] Missing token in response")
        raise ValueError("Login failed: missing token")

    # Save token to config for persistence
    _account["token"] = new_token
    _save_token(new_token)

    logger.info("[login] Login successful, token obtained and saved")
    return new_token


def _save_token(token: str):
    """Save token to config file for persistence."""
    try:
        config = CONFIG.copy()
        if "account" not in config:
            config["account"] = {}
        config["account"]["token"] = token
        save_config(config)
        logger.debug("[login] Token saved to config")
    except Exception as e:
        logger.warning(f"[login] Failed to save token: {e}")


def invalidate_token():
    """Invalidate current token, forcing refresh on next get_token().

    Call this when API returns authentication errors (e.g., 40003).
    """
    global _account
    if _account:
        _account.pop("token", None)
        logger.debug("[invalidate_token] Token invalidated in memory")

    # Also clear from CONFIG to prevent reuse
    try:
        from .config import CONFIG as current_config
        if current_config.get("account"):
            new_config = current_config.copy()
            new_config["account"] = new_config["account"].copy()
            new_config["account"].pop("token", None)
            save_config(new_config)
            logger.debug("[invalidate_token] Token invalidated in config")
    except Exception as e:
        logger.warning(f"[invalidate_token] Failed to clear token from config: {e}")


def get_token() -> str:
    """Get current token, login if needed (lazy initialization)."""
    global _account

    # Fast path: already have valid token
    if _account and _account.get("token"):
        return _account["token"]

    # Slow path: initialize and get token
    with _token_lock:
        # Double-check after acquiring lock
        if _account and _account.get("token"):
            return _account["token"]

        # Initialize account from config
        if _account is None:
            _account = CONFIG.get("account")
            if not _account:
                raise ValueError("No account configured")

        # Check config for existing token
        config_token = CONFIG.get("account", {}).get("token")
        if config_token:
            _account["token"] = config_token
            logger.debug("[get_token] Loaded token from config")
            return config_token

        # No token, need to login
        return login()


def get_auth_headers() -> dict:
    """Get headers with authorization."""
    return {**BASE_HEADERS, "authorization": f"Bearer {get_token()}"}
