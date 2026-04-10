"""Configuration and constants for DeepSeek API."""

import json
import logging
import os
import pathlib
from typing import Any

try:
    import tomllib as toml
except ImportError:
    import tomli as toml

logger = logging.getLogger(__name__)

# ----------------------------------------------------------------------
# (1) Configuration file path and load/save functions
# ----------------------------------------------------------------------
CONFIG_PATH = os.getenv("CONFIG_PATH", "config.toml")
ACCOUNT_TOKEN_PATH = os.getenv("DEEPSEEK_ACCOUNT_TOKEN_PATH", "").strip()


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
            json_path = CONFIG_PATH.replace(".toml", ".json")
            logger.warning(
                f"[save_config] tomli-w not available, saving as JSON to {json_path}"
            )
            with open(json_path, "w", encoding="utf-8") as f:
                json.dump(cfg, f, ensure_ascii=False, indent=2)
    except Exception as e:
        logger.error(f"[save_config] Failed to write config file: {e}")


CONFIG = load_config()


def _get_server_config() -> dict:
    return CONFIG.get("server", {})


def _get_config_section(name: str) -> dict:
    section = CONFIG.get(name, {})
    return section if isinstance(section, dict) else {}


def _get_auth_config() -> dict:
    return _get_config_section("auth")


def _get_env_or_config(env_name: str, config_key: str, default):
    env_value = os.getenv(env_name)
    if env_value is not None:
        return env_value
    return _get_server_config().get(config_key, default)


def _parse_bool(value, default: bool) -> bool:
    if value is None:
        return default
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        normalized = value.strip().lower()
        if normalized in {"1", "true", "yes", "on"}:
            return True
        if normalized in {"0", "false", "no", "off"}:
            return False
    return bool(value)


def _parse_csv_or_list(value, default: list[str]) -> list[str]:
    if value is None:
        return list(default)
    if isinstance(value, list):
        return [str(item).strip() for item in value if str(item).strip()]
    if isinstance(value, str):
        items = [item.strip() for item in value.split(",")]
        return [item for item in items if item]
    return list(default)


def _load_json_env(env_name: str):
    raw = os.getenv(env_name, "").strip()
    if not raw:
        return None
    try:
        return json.loads(raw)
    except json.JSONDecodeError as exc:
        logger.warning(f"[config] Invalid JSON in {env_name}: {exc}")
        return None


def _stringify_mapping(mapping: Any) -> dict[str, str]:
    if not isinstance(mapping, dict):
        return {}
    normalized = {}
    for key, value in mapping.items():
        normalized_key = str(key).strip()
        if not normalized_key:
            continue
        normalized[normalized_key] = str(value).strip()
    return normalized


def _normalize_auth_token_entry(
    raw_entry: Any,
    *,
    fallback_name: str,
    default_enabled: bool = True,
) -> dict[str, Any] | None:
    if isinstance(raw_entry, dict):
        token = str(raw_entry.get("token", "")).strip()
        name = str(raw_entry.get("name", "")).strip() or fallback_name
        enabled = _parse_bool(raw_entry.get("enabled"), default_enabled)
    else:
        token = str(raw_entry).strip()
        name = fallback_name
        enabled = default_enabled

    if not token:
        return None

    return {
        "name": name,
        "token": token,
        "enabled": enabled,
    }


def _get_env_auth_token_entries() -> list[dict[str, Any]]:
    raw_env = _load_json_env("DEEPSEEK_WEB_AUTH_TOKENS_JSON")
    raw_entries: list[Any] = []

    if isinstance(raw_env, list):
        raw_entries.extend(raw_env)
    elif isinstance(raw_env, dict):
        raw_entries.append(raw_env)

    single_token = os.getenv("DEEPSEEK_WEB_AUTH_TOKEN", "").strip()
    if single_token:
        raw_entries.append(
            {
                "name": os.getenv("DEEPSEEK_WEB_AUTH_TOKEN_NAME", "").strip()
                or "env-auth-token",
                "token": single_token,
                "enabled": True,
            }
        )

    tokens = []
    for index, raw_entry in enumerate(raw_entries, start=1):
        normalized = _normalize_auth_token_entry(
            raw_entry,
            fallback_name=f"env-auth-token-{index}",
        )
        if normalized:
            tokens.append(normalized)
    return tokens


def _get_config_auth_token_entries() -> list[dict[str, Any]]:
    auth_cfg = _get_auth_config()
    raw_tokens = auth_cfg.get("tokens", [])
    if not isinstance(raw_tokens, list):
        return []

    tokens = []
    for index, raw_entry in enumerate(raw_tokens, start=1):
        normalized = _normalize_auth_token_entry(
            raw_entry,
            fallback_name=f"auth-token-{index}",
        )
        if normalized:
            tokens.append(normalized)
    return tokens


def get_local_api_key() -> str:
    """Get the legacy compatibility API key for protecting this proxy service."""
    env_key = os.getenv("DEEPSEEK_WEB_API_KEY", "").strip()
    if env_key:
        return env_key
    return str(_get_server_config().get("api_key", "")).strip()


def _get_legacy_auth_token_entry() -> dict[str, Any] | None:
    token = get_local_api_key()
    if not token:
        return None
    return {
        "name": "legacy-api-key",
        "token": token,
        "enabled": True,
    }


# ----------------------------------------------------------------------
# (2) DeepSeek API constants
# ----------------------------------------------------------------------
DEEPSEEK_HOST = "chat.deepseek.com"

# Fallback headers applied when the user hasn't configured [headers].
# Keeps clients compatible when DeepSeek raises the minimum client version.
_FALLBACK_HEADERS: dict[str, str] = {
    "Host": "chat.deepseek.com",
    "User-Agent": "DeepSeek/2.0.0 Android/35",
    "Accept": "application/json",
    "Accept-Encoding": "gzip",
    "Content-Type": "application/json",
    "x-client-platform": "android",
    "x-client-version": "2.0.0",
    "x-client-locale": "zh_CN",
    "accept-charset": "UTF-8",
}
DEEPSEEK_LOGIN_URL = f"https://{DEEPSEEK_HOST}/api/v0/users/login"
DEEPSEEK_CREATE_POW_URL = f"https://{DEEPSEEK_HOST}/api/v0/chat/create_pow_challenge"

# WASM module file path (relative to core module, or absolute)
_default_wasm = pathlib.Path(__file__).parent / "sha3_wasm_bg.7b9ca65ddd.wasm"
WASM_PATH = os.getenv("WASM_PATH", str(_default_wasm))
_DEFAULT_WASM_URL = "https://fe-static.deepseek.com/chat/static/sha3_wasm_bg.7b9ca65ddd.wasm"
_DEFAULT_WASM_PATH = "core/deepseek.wasm"


def get_wasm_url() -> str:
    return CONFIG.get("wasm", {}).get("url") or _DEFAULT_WASM_URL


def get_wasm_path() -> str:
    return CONFIG.get("wasm", {}).get("path") or _DEFAULT_WASM_PATH

# Log level from config (default WARNING if not set)
_log_level_str = CONFIG.get("log_level", "WARNING").upper()
LOG_LEVEL = getattr(logging, _log_level_str, logging.WARNING)


def get_base_headers() -> dict[str, str]:
    headers = _stringify_mapping(_get_config_section("headers"))
    env_headers = _load_json_env("DEEPSEEK_BASE_HEADERS_JSON")
    if env_headers is not None:
        headers = _stringify_mapping(env_headers)
    # Fill in any missing keys from fallback defaults (e.g. when [headers] is empty)
    for k, v in _FALLBACK_HEADERS.items():
        if k not in headers:
            headers[k] = v
    return headers


def get_default_impersonate() -> str:
    env_value = os.getenv("DEEPSEEK_BROWSER_IMPERSONATE", "").strip()
    if env_value:
        return env_value

    browser_cfg = _get_config_section("browser")
    return str(browser_cfg.get("impersonate") or CONFIG.get("impersonate", "")).strip()


def _read_account_token_file() -> str:
    if not ACCOUNT_TOKEN_PATH:
        return ""
    try:
        return pathlib.Path(ACCOUNT_TOKEN_PATH).read_text(encoding="utf-8").strip()
    except FileNotFoundError:
        return ""
    except Exception as exc:
        logger.warning(f"[config] Failed to read account token file: {exc}")
        return ""


def get_persisted_account_token() -> str:
    env_token = os.getenv("DEEPSEEK_ACCOUNT_TOKEN", "").strip()
    if env_token:
        return env_token

    file_token = _read_account_token_file()
    if file_token:
        return file_token

    account_cfg = _get_config_section("account")
    return str(account_cfg.get("token", "")).strip()


def persist_account_token(token: str):
    if ACCOUNT_TOKEN_PATH:
        token_path = pathlib.Path(ACCOUNT_TOKEN_PATH)
        try:
            token_path.parent.mkdir(parents=True, exist_ok=True)
            token_path.write_text(token, encoding="utf-8")
            os.chmod(token_path, 0o600)
            logger.debug("[config] Token saved to token file")
            return
        except Exception as exc:
            logger.warning(f"[config] Failed to write account token file: {exc}")

    try:
        config = CONFIG.copy()
        if "account" not in config or not isinstance(config["account"], dict):
            config["account"] = {}
        config["account"]["token"] = token
        save_config(config)
        logger.debug("[config] Token saved to config file")
    except Exception as exc:
        logger.warning(f"[config] Failed to persist token: {exc}")


def clear_persisted_account_token():
    if ACCOUNT_TOKEN_PATH:
        token_path = pathlib.Path(ACCOUNT_TOKEN_PATH)
        try:
            if token_path.exists():
                token_path.unlink()
            logger.debug("[config] Token file cleared")
        except Exception as exc:
            logger.warning(f"[config] Failed to clear token file: {exc}")

    try:
        current_account = _get_config_section("account")
        if current_account:
            config = CONFIG.copy()
            config["account"] = current_account.copy()
            config["account"].pop("token", None)
            save_config(config)
            logger.debug("[config] Token cleared from config file")
    except Exception as exc:
        logger.warning(f"[config] Failed to clear token from config file: {exc}")


def get_account_config() -> dict[str, Any]:
    account_cfg = _get_config_section("account").copy()

    env_map = {
        "DEEPSEEK_ACCOUNT_EMAIL": "email",
        "DEEPSEEK_ACCOUNT_MOBILE": "mobile",
        "DEEPSEEK_ACCOUNT_AREA_CODE": "area_code",
        "DEEPSEEK_ACCOUNT_PASSWORD": "password",
    }
    for env_name, config_key in env_map.items():
        env_value = os.getenv(env_name)
        if env_value is not None:
            account_cfg[config_key] = env_value

    token = get_persisted_account_token()
    if token:
        account_cfg["token"] = token
    else:
        account_cfg.pop("token", None)

    return account_cfg


def get_auth_token_entries() -> list[dict[str, Any]]:
    """Return normalized auth token entries from all supported config sources."""
    entries = []

    legacy_entry = _get_legacy_auth_token_entry()
    if legacy_entry:
        entries.append(legacy_entry)

    entries.extend(_get_env_auth_token_entries())
    entries.extend(_get_config_auth_token_entries())
    return entries


def get_auth_tokens() -> list[str]:
    """Return enabled auth token values from all supported config sources."""
    return [entry["token"] for entry in get_auth_token_entries() if entry["enabled"]]


def get_enabled_auth_tokens() -> list[str]:
    """Return enabled auth token values from all supported config sources."""
    return get_auth_tokens()


def has_effective_auth_tokens() -> bool:
    """Return True when at least one enabled auth token is configured."""
    return bool(get_enabled_auth_tokens())


def get_auth_required() -> bool:
    """Return whether auth is explicitly required for /v0 and /v1."""
    return _parse_bool(_get_auth_config().get("required"), False)


def get_auth_mode_name() -> str:
    """Describe which auth config source(s) are currently in effect."""
    has_legacy = _get_legacy_auth_token_entry() is not None
    has_explicit = bool(_get_env_auth_token_entries() or _get_config_auth_token_entries())

    if has_legacy and has_explicit:
        return "mixed compatibility mode"
    if has_explicit:
        return "formal auth.tokens mode"
    if has_legacy:
        return "legacy single-token compatibility mode"
    return "anonymous mode"


def get_auth_mode_summary() -> str:
    """Return a log-safe summary of the active auth mode."""
    enabled_count = len(get_enabled_auth_tokens())
    required = get_auth_required()
    return (
        f"Auth mode: {get_auth_mode_name()}; "
        f"{enabled_count} enabled token(s); required={required}."
    )


def get_pool_size() -> int:
    """Max concurrent DeepSeek sessions in the stateless session pool."""
    return int(_get_env_or_config("DEEPSEEK_WEB_POOL_SIZE", "pool_size", 10))


def get_pool_acquire_timeout() -> float:
    """Seconds to wait for an available session before returning 503."""
    return float(
        _get_env_or_config(
            "DEEPSEEK_WEB_POOL_ACQUIRE_TIMEOUT",
            "pool_acquire_timeout",
            30.0,
        )
    )


def get_max_idle_seconds() -> float:
    """Seconds of inactivity before an idle session is eligible for cleanup."""
    return float(
        _get_env_or_config(
            "DEEPSEEK_WEB_MAX_IDLE_SECONDS",
            "max_idle_seconds",
            300.0,
        )
    )


def get_server_host() -> str:
    return str(_get_env_or_config("DEEPSEEK_WEB_HOST", "host", "127.0.0.1")).strip()


def get_server_port() -> int:
    return int(_get_env_or_config("DEEPSEEK_WEB_PORT", "port", 5001))


def get_server_reload() -> bool:
    return _parse_bool(_get_env_or_config("DEEPSEEK_WEB_RELOAD", "reload", True), True)


def get_cors_origins() -> list[str]:
    return _parse_csv_or_list(
        _get_env_or_config("DEEPSEEK_WEB_CORS_ORIGINS", "cors_origins", ["*"]),
        ["*"],
    )


def get_cors_origin_regex() -> str | None:
    value = _get_env_or_config("DEEPSEEK_WEB_CORS_ORIGIN_REGEX", "cors_origin_regex", "")
    value = str(value).strip()
    return value or None


def get_cors_allow_credentials() -> bool:
    return _parse_bool(
        _get_env_or_config(
            "DEEPSEEK_WEB_CORS_ALLOW_CREDENTIALS",
            "cors_allow_credentials",
            False,
        ),
        False,
    )


def get_cors_allow_methods() -> list[str]:
    return _parse_csv_or_list(
        _get_env_or_config("DEEPSEEK_WEB_CORS_ALLOW_METHODS", "cors_allow_methods", ["*"]),
        ["*"],
    )


def get_cors_allow_headers() -> list[str]:
    return _parse_csv_or_list(
        _get_env_or_config("DEEPSEEK_WEB_CORS_ALLOW_HEADERS", "cors_allow_headers", ["*"]),
        ["*"],
    )


# Keep module-level constants for callers that import them directly.
BASE_HEADERS = get_base_headers()
DEFAULT_IMPERSONATE = get_default_impersonate()
