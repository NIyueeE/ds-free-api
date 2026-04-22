# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Rust API proxy exposing free DeepSeek model endpoints. Translates standard OpenAI-compatible and Anthropic-compatible requests to DeepSeek's internal protocol with account pool rotation, PoW challenge handling, and streaming response support.

Requires Rust **1.94.1** (pinned in `rust-toolchain.toml`) with **edition 2024**.

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
├── lib.rs                       # Public API boundary: exports Config, DeepSeekCore, OpenAIAdapter, AnthropicCompat
├── config.rs                    # Config loader: -c flag, config.toml default
├── ds_core.rs                   # DeepSeek facade: DeepSeekCore, CoreError; declares accounts/ client/ completions/ pow
├── ds_core/
│   ├── accounts.rs              # Account pool: init validation, round-robin selection
│   ├── pow.rs                   # PoW solver: WASM loading, DeepSeekHashV1 computation
│   ├── completions.rs           # Chat orchestration: SSE streaming, account guard
│   └── client.rs                # Raw HTTP client: API endpoints, zero business logic
├── openai_adapter.rs            # OpenAI adapter facade: OpenAIAdapter, OpenAIAdapterError, StreamResponse
├── openai_adapter/
│   ├── types.rs                 # OpenAI protocol types (request + response structs)
│   ├── models.rs                # Model list/get endpoints
│   ├── request.rs               # Request parsing facade: AdapterRequest, parse(); declares normalize/ prompt/ resolver/ tools
│   ├── request/
│   │   ├── normalize.rs         # Request normalization/validation
│   │   ├── prompt.rs            # ChatML prompt construction (<|im_start|>/<|im_end|>)
│   │   ├── resolver.rs          # Model name to internal type resolution
│   │   └── tools.rs             # Tool definition extraction and injection
│   ├── response.rs              # Response conversion facade: stream(), aggregate(); declares sse_parser/ state/ converter/ tool_parser
│   └── response/
│       ├── sse_parser.rs        # SSE byte stream to DsFrame event stream
│       ├── state.rs             # DeepSeek patch state machine
│       ├── converter.rs         # DsFrame to OpenAI chunk conversion
│       └── tool_parser.rs       # XML <tool_calls> detection/parse
├── anthropic_compat.rs          # Anthropic compat facade: AnthropicCompat, AnthropicCompatError, StreamResponse
├── anthropic_compat/
│   ├── models.rs                # Anthropic model list/get (translates from OpenAI format)
│   ├── request.rs               # Anthropic → OpenAI request mapping
│   └── response.rs              # OpenAI → Anthropic response mapping (stream + aggregate)
├── server.rs                    # HTTP server facade: axum router, auth middleware, shutdown; declares handlers/ stream/ error
└── server/
    ├── handlers.rs              # Route handlers: OpenAI + Anthropic endpoints
    ├── stream.rs                # SseBody: StreamResponse → axum Body
    └── error.rs                 # ServerError: OpenAI-compatible error JSON responses
```

**Additional files not in src/**:
- `examples/openai_adapter_cli/` — JSON request samples (basic_chat, reasoning_search, stop_sequence, stream_options, tool_call)
- `examples/*-script.txt` — Scripted input for CLI examples
- `py-e2e-tests/` — Python end-to-end test suite (gitignored runtime artifacts)

### Facade Module Pattern

`ds_core.rs`, `openai_adapter.rs`, `server.rs`, `request.rs`, `response.rs`, and `anthropic_compat.rs` are **facades**:
- They declare submodules with `mod` (keeping implementation private)
- They re-export only the minimal public interface via `pub use`
- They sometimes contain `#[cfg(test)]` test modules

This means the file tree does not directly map to the public API. To understand what a module exposes externally, read its facade file, not the directory listing.

### Binary / Library Split

- `main.rs` is a thin binary wrapper (~10 lines): init `env_logger`, parse CLI args, load config, call `server::run()`
- `lib.rs` defines the public API surface: `Config`, `DeepSeekCore`, `OpenAIAdapter`, `AnthropicCompat`, `StreamResponse`, etc.
- The crate can be built as both a library (`cargo build --lib`) and a binary (`cargo build --bin ds-free-api`)

## Key Architectural Patterns

### Account Pool Model
1 account = 1 session = 1 concurrency. Scale via more accounts in `config.toml`.

### Request Flow
`v0_chat()` → `get_account()` → `compute_pow()` → `edit_message(payload)` → `GuardedStream`

`completions.rs` hardcodes `message_id: 1` in `EditMessagePayload` because the health check during initialization already writes message 0 into the session.

### GuardedStream & Account Lifecycle
`AccountGuard` marks an account as `busy` and automatically releases it on `Drop`. `GuardedStream` wraps the SSE stream with an `AccountGuard`, so the account is held busy until the stream is fully consumed or dropped. This binds account concurrency to stream lifetime without explicit cleanup logic.

### Account Initialization Flow
`AccountPool::init()` spins up all accounts concurrently. Per-account initialization (`try_init_account`) follows:
1. `login` — obtain Bearer token
2. `create_session` — create chat session
3. `health_check` — send a test completion (with PoW) to verify the session is writable
4. `update_title` — rename session to "managed-by-ai-free-api"

Health check is required because an empty session will fail on `edit_message` with `invalid message id`.

### Request Pipeline (OpenAI)
```
JSON body → serde deserialize → normalize (validation/defaults) → tools extract → prompt build (ChatML) → resolver (model mapping) → ChatRequest
```

### Response Pipeline (OpenAI)
```
ds_core SSE bytes → SseStream (sse_parser) → StateStream (state/patch machine) → ConverterStream (converter) → ToolCallStream (tool_parser) → StopStream (stop sequences) → SSE bytes
```

All stream wrappers use `pin_project!` macro and implement the `Stream` trait with `poll_next`.

### Anthropic Compatibility Layer
The Anthropic compat layer (`anthropic_compat/`) is a **pure protocol translator** that sits on top of `openai_adapter`:
- Does NOT directly access `ds_core` — all data flows through `OpenAIAdapter`
- Request flow: `Anthropic JSON → to_openai_request() → OpenAIAdapter::chat_completions() / try_chat()`
- Response flow: `OpenAI SSE/JSON → from_chat_completion_stream() / from_chat_completion_bytes() → Anthropic SSE/JSON`
- Supports both streaming and non-streaming `/v1/messages`

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
DeepSeek's free API returns `0` for `prompt_tokens`. The adapter computes this server-side in `request.rs` using `tiktoken-rs` with the `cl100k_base` tokenizer (same family as GPT-4). The count is stored in `AdapterRequest.prompt_tokens`, passed through `handlers.rs`, and injected into the final `Usage` object in `converter.rs` for both streaming and non-streaming responses.

### Tool Calls via XML
The adapter injects tool definitions as natural language into the prompt and parses `<tool_calls>` XML in the response back into structured `tool_calls` JSON. Custom (non-function) tools with grammar/text format definitions are also supported. When a tool call is triggered, `finish_reason` may be `"tool_calls"` instead of `"stop"`.

### Obfuscation
Random base64 padding in SSE chunks to reach a target response size (~512 bytes), controlled by `stream_options.include_obfuscation` (defaults to true).

### Overloaded Retry
`OpenAIAdapter::try_chat()` retries up to 3 times with 200ms delay on `CoreError::Overloaded`.

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
| DeepSeek chat flow | `src/ds_core/` | accounts → pow → completions → client |
| OpenAI request parsing | `src/openai_adapter/request/` | normalize → tools → prompt → resolver |
| OpenAI response conversion | `src/openai_adapter/response/` | sse_parser → state → converter → tool_parser |
| Anthropic compat layer | `src/anthropic_compat/` | request mapping → openai_adapter → response mapping |
| OpenAI protocol types | `src/openai_adapter/types.rs` | Request/response structs, `#![allow(dead_code)]` |
| Model listing | `src/openai_adapter/models.rs` | Model registry and listing |
| HTTP server/routes | `src/server/` | handlers → stream → error |
| CLI examples | `examples/ds_core_cli.rs`, `examples/openai_adapter_cli.rs` | Interactive and script modes |
| Example request JSON | `examples/openai_adapter_cli/` | Pre-built ChatCompletionRequest samples |
| Code style / logging | `docs/code-style.md`, `docs/logging-spec.md` | Comments, naming, targets, levels |
| API reference | `docs/deepseek-api-reference.md` | DeepSeek endpoint details |

## Conventions

- **Config**: Uncommented values in `config.toml` = required; commented = optional with default
- **Module files**: `foo.rs` declares sub-modules, `foo/` contains implementation
- **Comments**: Chinese in source files (team preference)
- **Errors**: Chinese error messages for user-facing output
- **Logging**: `log` crate with explicit targets (see `docs/logging-spec.md`); `env_logger` in `main.rs` and examples. Untargeted logs (e.g., bare `log::info!`) are prohibited.
- **Visibility**: `pub(crate)` for types not part of the public API; facade modules keep submodules private with `mod`
- **Tests**: All tests are inline (`#[cfg(test)]` within `src/` files). `request.rs` has sync unit tests for parsing logic; `response.rs` has `tokio::test` async tests for stream aggregation. No separate `tests/` directory.

## Anti-Patterns

- Do NOT create separate config entry points — `src/config.rs` is the single source
- Do NOT implement provider logic outside its `*_core/` module
- Do NOT commit `config.toml` (only `config.example.toml`)
- Do NOT use `println!`/`eprintln!` in library code — use `log` crate with target
- Do NOT use untargeted log macros — always specify `target: "crate::module"`
- Do NOT access `ds_core` directly from `anthropic_compat` — always go through `OpenAIAdapter`

## Commands

```bash
# Setup (do not commit config.toml)
cp config.example.toml config.toml

# One-pass check (check + clippy + fmt + audit + unused deps)
just check

# Run the HTTP server
just serve
RUST_LOG=debug just serve

# Run ds_core_cli example
just ds-core-cli
RUST_LOG=debug just ds-core-cli
just ds-core-cli -- source examples/ds_core_cli-script.txt

# Run openai_adapter_cli example
just openai-adapter-cli

# Run specific test modules
just test-adapter-request
just test-adapter-response

# Run a single Rust test
cargo test converter_emits_role_and_content -- --exact

# Run all Rust tests
cargo test

# Run Python e2e tests (requires server running on port 5317)
just e2e

# Start server with e2e test config
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
```
