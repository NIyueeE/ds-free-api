"""Proof-of-Work (PoW) challenge handling using WASM."""

import base64
import ctypes
import json
import logging
import struct
import threading

from curl_cffi import requests
from wasmtime import Engine, Linker, Module, Store

from .config import DEEPSEEK_CREATE_POW_URL, DEFAULT_IMPERSONATE, WASM_PATH

logger = logging.getLogger(__name__)

# Cached WASM module and runtime - compiled once at first use
_wasm_cache: dict[str, tuple[Engine, Linker, Module, Store]] = {}
_cache_lock = threading.Lock()


def _get_cached_wasm(wasm_path: str) -> tuple[Engine, Linker, Module, Store]:
    """Get or create cached WASM module and Store."""
    if wasm_path not in _wasm_cache:
        with _cache_lock:
            if wasm_path not in _wasm_cache:
                logger.debug(f"[WASM] Loading module from {wasm_path}")
                try:
                    with open(wasm_path, "rb") as f:
                        wasm_bytes = f.read()
                except Exception as e:
                    logger.error(f"[WASM] Failed to load wasm file: {wasm_path}, error: {e}")
                    raise RuntimeError(f"Failed to load wasm file: {wasm_path}, error: {e}")
                engine = Engine()
                module = Module(engine, wasm_bytes)
                linker = Linker(engine)
                store = Store(engine)
                instance = linker.instantiate(store, module)
                # Pre-cache exports for fast access
                exports = instance.exports(store)
                _wasm_cache[wasm_path] = (engine, linker, module, store, exports)
                logger.debug("[WASM] Module loaded and cached successfully")
    return _wasm_cache[wasm_path]


def compute_pow_answer(
    algorithm: str,
    challenge_str: str,
    salt: str,
    difficulty: int,
    expire_at: int,
    wasm_path: str = WASM_PATH,
) -> int | None:
    """
    Compute DeepSeekHash answer using WASM module.

    Per JS logic:
      - Concatenate prefix: "{salt}_{expire_at}_"
      - Write challenge and prefix to wasm memory, call wasm_solve
      - Read status and result from wasm memory
      - If status is 0, return None; otherwise return integer answer
    """
    if algorithm != "DeepSeekHashV1":
        raise ValueError(f"Unsupported algorithm: {algorithm}")

    prefix = f"{salt}_{expire_at}_"

    # --- Get cached WASM module and exports ---
    _, _, _, store, exports = _get_cached_wasm(wasm_path)

    try:
        memory = exports["memory"]
        add_to_stack = exports["__wbindgen_add_to_stack_pointer"]
        alloc = exports["__wbindgen_export_0"]
        wasm_solve = exports["wasm_solve"]
    except KeyError as e:
        raise RuntimeError(f"Missing wasm export function: {e}")

    def write_memory(offset: int, data: bytes):
        size = len(data)
        base_addr = ctypes.cast(memory.data_ptr(store), ctypes.c_void_p).value
        ctypes.memmove(base_addr + offset, data, size)

    def read_memory(offset: int, size: int) -> bytes:
        base_addr = ctypes.cast(memory.data_ptr(store), ctypes.c_void_p).value
        return ctypes.string_at(base_addr + offset, size)

    def encode_string(text: str):
        data = text.encode("utf-8")
        length = len(data)
        ptr_val = alloc(store, length, 1)
        ptr = int(ptr_val.value) if hasattr(ptr_val, "value") else int(ptr_val)
        write_memory(ptr, data)
        return ptr, length

    # 1. Allocate 16 bytes stack space
    retptr = add_to_stack(store, -16)

    # 2. Encode challenge and prefix to wasm memory
    ptr_challenge, len_challenge = encode_string(challenge_str)
    ptr_prefix, len_prefix = encode_string(prefix)

    # 3. Call wasm_solve (difficulty passed as float)
    wasm_solve(
        store,
        retptr,
        ptr_challenge,
        len_challenge,
        ptr_prefix,
        len_prefix,
        float(difficulty),
    )

    # 4. Read 4-byte status and 8-byte result from retptr
    status_bytes = read_memory(retptr, 4)
    if len(status_bytes) != 4:
        add_to_stack(store, 16)
        raise RuntimeError("Failed to read status bytes")

    status = struct.unpack("<i", status_bytes)[0]

    value_bytes = read_memory(retptr + 8, 8)
    if len(value_bytes) != 8:
        add_to_stack(store, 16)
        raise RuntimeError("Failed to read result bytes")

    value = struct.unpack("<d", value_bytes)[0]

    # 5. Restore stack pointer
    add_to_stack(store, 16)

    if status == 0:
        return None

    return int(value)


def get_pow_response(target_path: str = "/api/v0/chat/completion") -> str | None:
    """Get PoW response for the specified endpoint.

    If token is invalid (40003), automatically refreshes token and retries.
    """
    from .auth import get_auth_headers, invalidate_token

    max_retries = 2

    for attempt in range(max_retries):
        logger.debug(f"[PoW] Requesting challenge for {target_path} (attempt {attempt + 1})")
        headers = get_auth_headers()
        resp = requests.post(
            DEEPSEEK_CREATE_POW_URL,
            headers=headers,
            json={"target_path": target_path},
            impersonate=DEFAULT_IMPERSONATE,
        )
        data = resp.json()
        resp.close()

        code = data.get("code")

        # Handle authentication error - invalidate token and retry
        if code == 40003 and attempt < max_retries - 1:
            logger.warning(f"[PoW] Token invalid (code={code}), refreshing token and retrying...")
            invalidate_token()
            continue

        if code != 0:
            logger.error(f"[PoW] Failed to get challenge, code={code}")
            return None

        challenge = data["data"]["biz_data"]["challenge"]
        logger.debug(f"[PoW] Got challenge, computing answer (difficulty={challenge['difficulty']})")

        answer = compute_pow_answer(
            challenge["algorithm"],
            challenge["challenge"],
            challenge["salt"],
            challenge["difficulty"],
            challenge["expire_at"],
        )

        if answer is None:
            logger.error("[PoW] Failed to compute answer")
            return None

        pow_dict = {
            "algorithm": challenge["algorithm"],
            "challenge": challenge["challenge"],
            "salt": challenge["salt"],
            "answer": answer,
            "signature": challenge["signature"],
            "target_path": challenge["target_path"],
        }
        logger.debug("[PoW] PoW response generated successfully")
        return base64.b64encode(json.dumps(pow_dict, separators=(",", ":")).encode()).decode()

    return None
