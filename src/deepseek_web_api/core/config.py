"""Configuration and constants for DeepSeek API."""

import json
import logging
import os
import pathlib

try:
    import tomllib as toml
except ImportError:
    import tomli as toml

logger = logging.getLogger(__name__)

# ----------------------------------------------------------------------
# (1) Configuration file path and load/save functions
# ----------------------------------------------------------------------
CONFIG_PATH = os.getenv("CONFIG_PATH", "config.toml")


def load_config():
    """Load configuration from config.toml, return empty dict on error."""
    try:
        with open(CONFIG_PATH, "rb") as f:
            return toml.load(f)
    except Exception as e:
        logger.warning(f"[load_config] Cannot read config file: {e}")
        return {}


def save_config(cfg):
    """Write configuration back to config.toml.

    Uses tomli-w if available (Python 3.11+), otherwise falls back to json.
    """
    try:
        try:
            import tomli_w

            with open(CONFIG_PATH, "wb") as f:
                tomli_w.dump(cfg, f)
        except ImportError:
            # Fallback: write as JSON with TOML extension warning
            json_path = CONFIG_PATH.replace(".toml", ".json")
            logger.warning(
                f"[save_config] tomli-w not available, saving as JSON to {json_path}"
            )
            with open(json_path, "w", encoding="utf-8") as f:
                json.dump(cfg, f, ensure_ascii=False, indent=2)
    except Exception as e:
        logger.error(f"[save_config] Failed to write config file: {e}")


CONFIG = load_config()

# ----------------------------------------------------------------------
# (2) DeepSeek API constants
# ----------------------------------------------------------------------
DEEPSEEK_HOST = "chat.deepseek.com"
DEEPSEEK_LOGIN_URL = f"https://{DEEPSEEK_HOST}/api/v0/users/login"
DEEPSEEK_CREATE_POW_URL = f"https://{DEEPSEEK_HOST}/api/v0/chat/create_pow_challenge"

# BASE_HEADERS must be configured in config.toml under [headers]
# See config.toml.example for required fields
BASE_HEADERS = CONFIG.get("headers", {})

# HTTP request impersonation (browser signature for anti-bot)
# Can be in [browser.impersonate] or root level impersonate
DEFAULT_IMPERSONATE = CONFIG.get("browser", {}).get("impersonate") or CONFIG.get("impersonate", "")

# WASM module file path (relative to core module, or absolute)
_default_wasm = pathlib.Path(__file__).parent / "sha3_wasm_bg.7b9ca65ddd.wasm"
WASM_PATH = os.getenv("WASM_PATH", str(_default_wasm))

# Log level from config (default WARNING if not set)
_log_level_str = CONFIG.get("log_level", "WARNING").upper()
LOG_LEVEL = getattr(logging, _log_level_str, logging.WARNING)
