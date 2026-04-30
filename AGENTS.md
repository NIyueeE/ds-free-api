# CLAUDE.md

> **Note**: This file serves dual duty as both `AGENTS.md` (the real file) and `CLAUDE.md` (symlink → `AGENTS.md`).
> Edit `AGENTS.md` directly; `CLAUDE.md` stays in sync automatically.

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Rust API proxy exposing free DeepSeek model endpoints. Translates standard OpenAI-compatible and Anthropic-compatible requests to DeepSeek's internal protocol with account pool rotation, PoW challenge handling, and streaming response support.

Requires Rust **1.95.0** (pinned in `rust-toolchain.toml`) with **edition 2024**.

Key dependencies and why they matter:
- `wasmtime` — executes DeepSeek's PoW WASM solver; the entire PoW system depends on this
- `tiktoken-rs` — client-side prompt token counting (DeepSeek returns 0 for `prompt_tokens`)
- `pin-project-lite` — underpins every streaming response wrapper (`SseStream`, `StateStream`, etc.)
- `axum` / `reqwest` — HTTP server and client respectively
- `tokio` with `signal` feature — async runtime with graceful shutdown on SIGTERM/SIGINT

## Principles

### 1. Single Responsibility
- `config.rs`: Configuration loading only, no client creation or business logic
- `client.rs`: Raw HTTP calls only, no token caching, retry, or SSE parsing
- `accounts.rs`: Account pool management only, no network requests
- `pow.rs`: WASM computation only, no account management or request sending
- `server/handlers.rs`: Route handling only, delegates to OpenAIAdapter / AnthropicCompat
- `server/stream.rs`: SSE response body only, no business logic
- `server/error.rs`: Error mapping only, no business logic
- `anthropic_compat.rs`: Protocol translation only, no direct ds_core access

### 2. Minimal Viable
- No premature abstractions: Extract traits/structs when needed, not before
- No redundant code: Remove unused imports, avoid over-documenting, no pre-written tests
- Delay dependency introduction: only add deps when actually needed

### 3. Control Complexity
- Explicit over implicit: Dependencies injected via parameters, no global state
- Composition over inheritance: Small modules composed via functions, no deep inheritance
- Clear boundaries: Modules interact via explicit interfaces, no internal logic leakage

## Architecture

### Module Structure

```
src/
├── main.rs                      # Thin binary wrapper: init logger, load config, run server
├── lib.rs                       # Public API boundary: exports Config, DeepSeekCore, ChatResponse, OpenAIAdapter, ChatResult, AnthropicCompat
├── config.rs                    # Config loader: -c flag, config.toml default
├── ds_core.rs                   # DeepSeek facade: DeepSeekCore, CoreError; declares accounts/ client/ completions/ pow
├── ds_core/
│   ├── accounts.rs              # Account pool: init validation, idle-aware (most-idle-first) selection
│   ├── pow.rs                   # PoW solver: WASM loading, DeepSeekHashV1 computation
│   ├── completions.rs           # Chat orchestration: SSE streaming, account guard
│   └── client.rs                # Raw HTTP client: API endpoints, zero business logic
├── openai_adapter.rs            # OpenAI adapter facade: OpenAIAdapter, OpenAIAdapterError, StreamResponse
├── openai_adapter/
│   ├── types.rs                 # OpenAI protocol types (request + response structs)
│   ├── models.rs                # Model list/get endpoints
│   ├── request.rs               # Request parsing facade; declares normalize/ prompt/ resolver/ tools/ files
│   ├── request/
│   │   ├── normalize.rs         # Request normalization/validation
│   │   ├── prompt.rs            # DeepSeek native tag prompt construction (<｜User｜>/<｜Assistant｜>)
│   │   ├── resolver.rs          # Model name to internal type resolution
│   │   ├── tools.rs             # Tool definition extraction and injection
│   │   └── files.rs             # File/Image content part to FilePayload extraction
│   ├── response.rs              # Response conversion facade: stream(), aggregate(); declares sse_parser/ state/ converter/ tool_parser
│   └── response/
│       ├── sse_parser.rs        # SSE byte stream to DsFrame event stream
│       ├── state.rs             # DeepSeek patch state machine
│       ├── converter.rs         # DsFrame to OpenAI chunk conversion
│       └── tool_parser.rs       # XML <tool_calls> detection/parse + TagConfig
├── anthropic_compat.rs          # Anthropic compat facade: AnthropicCompat, AnthropicCompatError, StreamResponse
├── anthropic_compat/
│   ├── models.rs                # Anthropic model list/get (translates from OpenAI format)
│   ├── request.rs               # Anthropic → OpenAI request mapping
│   ├── response.rs              # Response mapping facade; declares aggregate/ stream
│   └── response/
│       ├── aggregate.rs         # Non-streaming OpenAI → Anthropic response conversion
│       └── stream.rs            # Streaming OpenAI SSE → Anthropic SSE conversion
├── server.rs                    # HTTP server facade: axum router, auth middleware, shutdown; declares handlers/ stream/ error
└── server/
    ├── handlers.rs              # Route handlers: OpenAI + Anthropic endpoints
    ├── stream.rs                # SseBody: StreamResponse → axum Body
    └── error.rs                 # ServerError: OpenAI-compatible error JSON responses
```

**Additional files not in src/**:
- `config.example.toml` — authoritative configuration reference (all fields documented with examples)
- `examples/adapter_cli.rs` + `examples/adapter_cli-script.txt` — unified protocol debug CLI (modes: `chat`, `raw`, `compare`, `concurrent N`, `status`, `models`/`model <id>`)
- `examples/adapter_cli/` — JSON request samples (basic_chat, reasoning, reasoning_search, stop, stream, tool_call, tool_call_multi_turn, tool_call_parallel, tool_call_required, web_search)
- `py-e2e-tests/` — Python e2e test suite (pytest + uv, no extra deps needed):
  - `scenarios/{basic,repair}/openai|anthropic/*.json` — scenario-driven JSON test cases with request params + check rules
  - `runner.py` — single-run entry point; `stress_runner.py` — multi-iteration load testing
  - `config.toml` — e2e-specific server config (port 5317), `conftest.py` — shared fixtures

### Facade Module Pattern

`ds_core.rs`, `openai_adapter.rs`, `server.rs`, `request.rs`, `response.rs`, and `anthropic_compat.rs` are **facades**:
- They declare submodules with `mod` (keeping implementation private)
- They re-export only the minimal public interface via `pub use`
- They sometimes contain `#[cfg(test)]` test modules

This means the file tree does not directly map to the public API. To understand what a module exposes externally, read its facade file, not the directory listing.

### Binary / Library Split

- `main.rs` is a thin binary wrapper (~10 lines): init `env_logger`, parse CLI args, load config, call `server::run()`
- `lib.rs` defines the public API surface: `Config`, `DeepSeekCore`, `CoreError`, `ChatRequest`, `ChatResponse`, `AccountStatus`, `OpenAIAdapter`, `OpenAIAdapterError`, `ChatResult`, `StreamResponse`, `AnthropicCompat`
- The crate can be built as both a library (`cargo build --lib`) and a binary (`cargo build --bin ds-free-api`)

### StreamResponse Type

`StreamResponse` is the unifying bridge between adapter layers and the HTTP server:
- Every adapter's streaming method returns `StreamResponse` (a boxed `Stream<Item = Result<Bytes>> + Send`)
- `server/stream.rs::SseBody` wraps `StreamResponse` and converts it into an `axum::body::Body`
- This decouples the adapters from the HTTP framework — they produce bytes, the server handles SSE framing

## Key Architectural Patterns

### Account Pool Model
1 account = 1 session = 1 concurrency. Scale via more accounts in `config.toml`.

### Request Flow (per-chat)
`v0_chat()` → `get_account()` → `split_history()` → `create_session()` → `upload_files()` → `compute_pow()` → `completion()` → `parse_ready()` → `GuardedStream`

Each `v0_chat()` call in `completions.rs` creates a dedicated session, uploads multi-turn history as files, then streams the response. The session is destroyed when the stream ends.

### Single-Struct Pipeline

The adapter layer uses a **single struct** (`ChatCompletionsRequest`) that flows through the entire request
pipeline — no intermediate `AdapterRequest` or `prepare` step:

```
ChatCompletionsRequest
  → normalize::apply |
  → tools::extract   | 每个子函数直接读 ChatCompletionsRequest 的字段
  → files::extract   |
  → prompt::build    |
  → resolver::resolve|
  → tiktoken
  → try_chat (ds_core::ChatRequest)
  → if req.stream → ChatCompletionsResponseChunk | else → ChatCompletionsResponse
```

Response structs are serialized directly to JSON (via `#[derive(Serialize)]` + `serde_json::to_vec`),
with no intermediate representation. `ChatCompletionsResponse` for non-streaming and
`ChatCompletionsResponseChunk` for streaming are the only two chat completions output types.
`ModelList`/`Model` serve `/v1/models` independently.

### GuardedStream & Account Lifecycle
`AccountGuard` marks an account as `busy` and automatically releases it on `Drop`. `GuardedStream` wraps the SSE stream with an `AccountGuard`, so the account is held busy until the stream is fully consumed or dropped. This binds account concurrency to stream lifetime without explicit cleanup logic.

### Account Initialization Flow
`AccountPool::init()` spins up all accounts concurrently (capped at 13 via `tokio::sync::Semaphore`). Per-account initialization (`try_init_account`) follows:
1. `login` — obtain Bearer token
2. `create_session` — create chat session
3. `health_check` — send a test completion (with PoW) to verify the session is writable
4. `update_title` — rename session to "managed-by-ai-free-api"

Health check is required because an empty session will fail on `edit_message` with `invalid message id`.

### Temp Session Lifecycle
Each `v0_chat()` call creates a dedicated chat session and destroys it when the stream ends. `GuardedStream::drop` always calls `delete_session`, regardless of whether the stream completed naturally or the client disconnected. On abnormal disconnection, it also calls `stop_stream` before deletion. This avoids session leaks without explicit cleanup at call sites. Sessions are tracked in `active_sessions: Arc<Mutex<HashMap<String, ActiveSession>>>` to support `stop_stream` delivery by `message_id`.

### History Splitting & File Upload
Multi-turn conversations are handled by splitting the prompt into an inline portion (most recent messages) and a history portion (earlier messages), uploaded as a file. The split in `split_history_prompt()`:
- The last user+assistant message pair and the final user message go inline
- Everything earlier is wrapped in `[file content begin]` … `[file content end]` markers and uploaded as `EMPTY.txt`

External files from the request (`ChatRequest.files`) are uploaded individually before the history file. Upload uses a separate PoW computation targeting `/api/v0/file/upload_file`, then polls for completion with 30 retries at 2s intervals (60s total timeout).

### Request Pipeline (OpenAI)
```
JSON body → serde deserialize → normalize (validation/defaults) → tools extract → prompt build (DeepSeek native tags) → resolver (model mapping) → ChatRequest
```

### Response Pipeline (OpenAI)
```
ds_core SSE bytes → SseStream (sse_parser) → StateStream (state/patch machine) → ConverterStream (converter) → ToolCallStream (tool_parser) → StopStream (stop sequences) → SSE bytes
```

All stream wrappers use `pin_project_lite::pin_project!` macro and implement the `Stream` trait with `poll_next`.

### Capability Toggles
The adapter maps OpenAI request fields to DeepSeek internal flags in `request/resolver.rs`:
- **Reasoning**: `reasoning_effort` defaults to `"high"` if absent (reasoning is on by default). Explicitly set to `"none"` to disable.
- **Web search**: `web_search_options` enables search when present; omitted by default (search off).

### Prompt Injection Strategy
The adapter builds the prompt directly using DeepSeek native tags (`<｜User｜>` / `<｜Assistant｜>` / `<｜Tool｜>` etc.) and embeds system instructions (tool definitions, format requirements, reminder) inside a `<think>` block appended to the last assistant turn. This approach was validated through iterative testing against claude-3.5-sonnet system prompt patterns — see `docs/deepseek-prompt-injection.md` for the full research history and alternative approaches considered.

### Anthropic Compatibility Layer
The Anthropic compat layer (`anthropic_compat/`) is a **pure protocol translator** that sits on top of `openai_adapter`:
- Does NOT directly access `ds_core` — all data flows through `OpenAIAdapter`
- Request flow: `Anthropic JSON → to_openai_request() → OpenAIAdapter::chat_completions() / try_chat()`
- Response flow: `OpenAI SSE/JSON → from_chat_completion_stream() / from_chat_completion_bytes() → Anthropic SSE/JSON`
- Supports both streaming and non-streaming `/v1/messages`

**Streaming tool calls** use the `input_json_delta` event sequence:
1. `content_block_start` with empty `input: {}`
2. One or more `content_block_delta` with `input_json_delta` containing partial JSON
3. `content_block_stop`

**Tool use ID mapping** via `map_id()`: OpenAI `chatcmpl-{hex}` → Anthropic `msg_{hex}`; OpenAI `call_{suffix}` → Anthropic `toolu_{suffix}`.

**Tool `type` compatibility**: Claude Code may omit the `type` field in tool definitions. `ToolUnion` in `request.rs` implements a custom `Deserialize` that defaults to `Custom` when `type` is absent.

**Document content block**: `ContentBlock::Document` is supported — base64 documents are mapped to OpenAI `file` content parts (uploaded as data URLs), and URL documents trigger web search mode (same as `image_url` HTTP links).

### Error Translation Chain
Errors propagate upward with translation at module boundaries:
1. `client.rs`: `ClientError` (HTTP, business errors, JSON parse)
2. `accounts.rs`: `PoolError` (`ClientError` | `PowError` | validation errors)
3. `ds_core.rs`: `CoreError` (`Overloaded` | `ProofOfWorkFailed` | `ProviderError` | `Stream`)
4. `openai_adapter.rs`: `OpenAIAdapterError` (`BadRequest` | `Overloaded` | `ProviderError` | `Internal`)
5. `anthropic_compat.rs`: `AnthropicCompatError` (`BadRequest` | `Overloaded` | `Internal`)
6. `server/error.rs`: `ServerError` (`Adapter` | `Unauthorized` | `NotFound`)

`client.rs` parses DeepSeek's wrapper envelope `{code, msg, data: {biz_code, biz_msg, biz_data}}` via `Envelope::into_result()`.

### Prompt Token Calculation
DeepSeek's free API returns `0` for `prompt_tokens`. The adapter computes this server-side in `openai_adapter.rs` using `tiktoken-rs` with the `cl100k_base` tokenizer (same family as GPT-4). The count is stored in `StreamCfg.prompt_tokens` and injected into the final `Usage` object in `converter.rs` for both streaming and non-streaming responses.

### Tool Calls via XML
The adapter injects tool definitions as natural language into the prompt and parses `<tool_calls>` XML in the response back into structured `tool_calls` JSON. Custom (non-function) tools with grammar/text format definitions are also supported. When a tool call is triggered, `finish_reason` may be `"tool_calls"` instead of `"stop"`.

The `arguments` field in tool call responses is normalized to always be a JSON string (never an object), so downstream consumers receive a consistent type regardless of what DeepSeek returns.

### Configurable Tool Call Tag Fallback
The primary tag is `<tool_calls>` (with s). The system also supports configurable fallback tag arrays:
- **`TagConfig.extra_starts`**: hardcoded defaults `["<tool_call>", "<function>"]` + user config from `config.toml`
- **`TagConfig.extra_ends`**: hardcoded defaults `["</tool_call>", "</function>"]` + user config
- Default values live only in `config.rs` (single source of truth), loaded via `Arc<TagConfig>` and threaded through the stream pipeline via `StreamCfg`
- Matching uses sliding-window prefix/suffix search against the full tag array, no paired tuples required

### Obfuscation
Random base64 padding in SSE chunks to reach a target response size (~512 bytes), controlled by `stream_options.include_obfuscation` (defaults to true).

### Overloaded Retry
`OpenAIAdapter::try_chat()` retries up to **6 times** with **exponential backoff** (1s → 2s → 4s → 8s → 16s) on `CoreError::Overloaded`, which is triggered by DeepSeek's `rate_limit_reached` SSE hint or when all accounts are busy.

### request_id & x-ds-account
Each request gets a `req-{n}` ID generated at the handler, threaded down through adapter → ds_core. Key log points carry `req=` for `grep`-able cross-layer tracing. The `x-ds-account` HTTP response header (set via `SseBody::with_header()`) carries the account identifier upstream from ds_core through `ChatResponse` / `ChatResult<T>` wrappers.

### HTTP Routes
**OpenAI-compatible:**
- `GET /` — health check, returns "ai-free-api"
- `POST /v1/chat/completions` — OpenAI-compatible chat completions (streaming and non-streaming)
- `GET /v1/models` — list available models
- `GET /v1/models/{id}` — get a specific model

**Anthropic-compatible:**
- `POST /anthropic/v1/messages` — Anthropic Messages API (streaming and non-streaming)
- `GET /anthropic/v1/models` — list available models (Anthropic format)
- `GET /anthropic/v1/models/{id}` — get a specific model (Anthropic format)

Optional Bearer token auth via `[[server.api_tokens]]` in config; no auth when empty.

### Model ID Mapping
`model_types` in `[deepseek]` config (default: `["default", "expert"]`) maps each type to OpenAI model ID `deepseek-{type}` (e.g., `deepseek-default`, `deepseek-expert`). Anthropic compat uses the same model IDs.

### PoW Fragility
`pow.rs` loads a WASM module downloaded from DeepSeek's CDN. The solver hardcodes the wasm-bindgen-generated symbol `__wbindgen_export_0` for memory allocation. If DeepSeek recompiles the WASM and changes export ordering, instantiation will fail with `PowError::Execution`. The WASM URL is configurable in `config.toml` to allow quick updates.

## Where to Look

| Task | Location | Notes |
|------|----------|-------|
| Config loading | `src/config.rs` | Single unified entry, `-c` flag support |
| Config reference | `config.example.toml` | All fields documented with examples (authoritative) |
| DeepSeek chat flow | `src/ds_core/` | accounts → pow → completions → client |
| Chat orchestration + file upload | `src/ds_core/completions.rs` | `v0_chat()`, history splitting, upload retry, `GuardedStream` |
| OpenAI request parsing | `src/openai_adapter/request/` | normalize → tools → files → prompt → resolver |
| File upload extraction | `src/openai_adapter/request/files.rs` | data URL → FilePayload, HTTP URL → search mode |
| OpenAI response conversion | `src/openai_adapter/response/` | sse_parser → state → converter → tool_parser |
| Tool call tag config | `src/openai_adapter/response/tool_parser.rs` | `TagConfig` with extra_starts/extra_ends fallback arrays |
| Stream pipeline config | `src/openai_adapter/response.rs` | `StreamCfg` struct (consolidates 8 stream params) |
| Anthropic compat layer | `src/anthropic_compat/` | request mapping → openai_adapter → response mapping |
| Anthropic streaming response | `src/anthropic_compat/response/stream.rs` | OpenAI SSE → Anthropic SSE event stream |
| Anthropic aggregate response | `src/anthropic_compat/response/aggregate.rs` | OpenAI JSON → Anthropic JSON |
| OpenAI protocol types | `src/openai_adapter/types.rs` | Request/response structs, `#![allow(dead_code)]` |
| Model listing | `src/openai_adapter/models.rs` | Model registry and listing |
| HTTP server/routes | `src/server/` | handlers → stream → error |
| Unified debug CLI | `examples/adapter_cli.rs` + `examples/adapter_cli-script.txt` | Modes: chat/raw/compare/concurrent/status/models |
| Example request JSON | `examples/adapter_cli/` | Pre-built ChatCompletionsRequest samples (chat, stream, stop, reasoning, web_search, tool_call, etc.) |
| Scripted regression test | `just adapter-cli -- source examples/adapter_cli-script.txt` | Runs all JSON samples in sequence |
| Stress test scripts | `py-e2e-tests/stress_test_tools_openai.py`, `py-e2e-tests/stress_test_tools_anthropic.py` | Load testing for OpenAI and Anthropic endpoints |
| e2e scenario test framework | `py-e2e-tests/runner.py`, `py-e2e-tests/scenarios/` | JSON-driven scenarios with checks; `stress_runner.py` for load testing |
| CI pipeline | `.github/workflows/ci.yml` | `cargo check + clippy + fmt + audit + machete` and `cargo test` |
| Release workflow | `.github/workflows/release.yml` | Tag `v*` triggers multi-platform build (8 targets, 4 OS) + CHANGELOG release notes |
| Claude config | `AGENTS.md` | Agent delegation patterns for this repo |
| Code style / logging | `docs/code-style.md`, `docs/logging-spec.md` | Comments, naming, targets, levels |
| Prompt injection strategy | `docs/deepseek-prompt-injection.md` | DeepSeek native tags, claude-3.5-sonnet system prompt research |
| API reference | `docs/deepseek-api-reference.md` | DeepSeek endpoint details |

## Conventions

- **Config**: Uncommented values in `config.toml` = required; commented = optional with default
- **Module files**: `foo.rs` declares sub-modules, `foo/` contains implementation
- **Comments**: Chinese in source files (team preference)
- **Errors**: Chinese error messages for user-facing output
- **Logging**: `log` crate with explicit targets. Untargeted logs (e.g., bare `log::info!`) are prohibited. Targets used:
  - `ds_core::accounts`, `ds_core::client`
  - `adapter` (for `openai_adapter`)
  - `http::server`, `http::request`, `http::response` (for `server`)
  - `anthropic_compat`, `anthropic_compat::models`, `anthropic_compat::request`, `anthropic_compat::response::stream`, `anthropic_compat::response::aggregate`
  - See `docs/logging-spec.md` for full target/level mapping
- **Visibility**: `pub(crate)` for types not part of the public API; facade modules keep submodules private with `mod`
- **Tests**: All tests are inline (`#[cfg(test)]` within `src/` files). `request.rs` has sync unit tests for parsing logic; `response.rs` has `tokio::test` async tests for stream aggregation. No separate `tests/` directory.
- **Test output**: `println!` / `eprintln!` are allowed inside `#[cfg(test)]` blocks for debugging test failures; they remain prohibited in library code
- **Import grouping**: std → third-party → `crate::` → local (`super`, `self`), separated by blank lines
- **Comments**: Follow `docs/code-style.md`:
  - `//!` — module docs: first line = responsibility, then key design decisions
  - `///` — public API docs: verb-led, note side effects and panic conditions
  - `//` — inline: explain "why", not "what"
- **Naming**: `snake_case` for modules/functions, `PascalCase` for types/enum variants, `SCREAMING_SNAKE_CASE` for constants
- **Test code**: `println!` / `eprintln!` are allowed inside `#[cfg(test)]` for debugging failures; prohibited in library code

## Anti-Patterns

- Do NOT create separate config entry points — `src/config.rs` is the single source
- Do NOT implement provider logic outside its `*_core/` module
- Do NOT commit `config.toml` (only `config.example.toml`)
- Do NOT use `println!`/`eprintln!` in library code — use `log` crate with target
- Do NOT use untargeted log macros — always specify `target: "..."`
- Do NOT access `ds_core` directly from `anthropic_compat` — always go through `OpenAIAdapter`

## Commands

```bash
# Setup (do not commit config.toml)
cp config.example.toml config.toml

# One-pass check (check + clippy + fmt + audit + unused deps)
just check

# Pre-commit hook runs check + clippy + fmt + audit + machete + cargo test --lib
# (see .git/hooks/pre-commit — matches CI order)

# Trace through the entire SSE pipeline
RUST_LOG=adapter=trace,ds_core::accounts=debug,info just serve

# Run the HTTP server
just serve
RUST_LOG=debug just serve

# Module-level logging filters
RUST_LOG=ds_core::accounts=debug,ds_core::client=warn,info just serve
RUST_LOG=adapter=debug,anthropic_compat=debug just serve

# Run unified protocol debug CLI (modes: chat, raw, compare, concurrent N, status, models, model <id>)
just adapter-cli
RUST_LOG=debug just adapter-cli
# Script mode — runs all JSON samples in sequence (full regression)
just adapter-cli -- source examples/adapter_cli-script.txt
# Interactive mode with a specific config
cargo run --example adapter_cli -- -c /path/to/config.toml

# Run specific test modules (pass test name filter and args)
just test-adapter-request
just test-adapter-response
just test-adapter-request converter_emits_role_and_content -- --exact

# Run a single Rust test (use -- --exact for precise name matching)
cargo test converter_emits_role_and_content -- --exact

# Run all Rust tests
cargo test

# Run only library tests (skips example compilation, faster iteration)
cargo test --lib

# e2e tests (requires `uv`, server on port 5317)
just e2e-basic    # 基础功能（OpenAI + Anthropic 双端点）
just e2e-repair   # 工具调用损坏修复专项
just e2e-stress   # 全部场景 × 3 次迭代压测

# Start server with e2e config
just e2e-serve

# Individual checks
cargo check
cargo clippy -- -D warnings
cargo fmt --check
cargo audit        # requires: cargo install cargo-audit
cargo machete      # requires: cargo install cargo-machete

# Build
cargo build
cargo build --release

# Release (tag push triggers CI: 8 targets x 4 platforms via cross)
git tag v0.x.x
git push origin v0.x.x
# CI extracts changelog from CHANGELOG.md, creates GitHub release

```
