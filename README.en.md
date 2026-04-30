<p align="center">
  <img src="https://raw.githubusercontent.com/NIyueeE/ds-free-api/main/assets/logo.svg" width="81" height="66">
</p>

<h1 align="center">DS-Free-API</h1>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/github/license/NIyueeE/ds-free-api.svg"></a>
  <img src="https://img.shields.io/github/v/release/NIyueeE/ds-free-api.svg">
  <img src="https://img.shields.io/badge/rust-1.95.0+-93450a.svg">
  <img src="https://github.com/NIyueeE/ds-free-api/actions/workflows/ci.yml/badge.svg">
</p>
<p align="center">
  <img src="https://img.shields.io/github/stars/NIyueeE/ds-free-api.svg">
  <img src="https://img.shields.io/github/forks/NIyueeE/ds-free-api.svg">
  <img src="https://img.shields.io/github/last-commit/NIyueeE/ds-free-api.svg">
  <img src="https://img.shields.io/github/languages/code-size/NIyueeE/ds-free-api.svg">
</p>

[中文](README.md)

A proxy that translates free DeepSeek web chat into standard OpenAI and Anthropic compatible APIs (supports chat completions and messages with streaming and tool calling).

## Highlights

- **Zero-cost API proxy**: Uses DeepSeek's free web interface — no API key required, compatible with OpenAI / Anthropic clients
- **Dual protocol**: OpenAI Chat Completions and Anthropic Messages API, drop-in replacement for mainstream clients
- **Tool calling ready**: Full OpenAI function calling with XML parsing + 3-layer self-repair pipeline (text → JSON → model fallback), covering 10+ malformed output patterns
- **File upload ready**: Supports OpenAI `file` / `image_url` content parts and Anthropic `image` / `document` content blocks — inline data URLs are automatically uploaded to the DeepSeek session;
  HTTP URLs trigger web search mode so the model can directly access the link
- **Rust implementation**: Single binary + single TOML config, native cross-platform performance
- **Multi-account pool**: Most-idle-first rotation, horizontal scalability for concurrency

## Quick Start

Download the latest release for your platform from [releases](https://github.com/NIyueeE/ds-free-api/releases) and extract.

```
  .
  ├── ds-free-api          # Executable
  ├── LICENSE
  ├── README.md
  ├── README.en.md
  └── config.example.toml  # Example config
```

### Configuration

Copy `config.example.toml` to `config.toml` in the same directory as the executable, or use `./ds-free-api -c <config_path>` to specify a custom path.

### Run

```bash
# Default (requires config.toml in current directory)
./ds-free-api

# Custom config path
./ds-free-api -c /path/to/config.toml

# Debug logging
RUST_LOG=debug ./ds-free-api
```

Only required fields are shown below. One account equals one concurrent session.

> **Concurrency notes**: DeepSeek free API has rate limits per session (`Messages too frequent. Try again later.`). This project has built-in protection:
> - **Rate limit detection**: Listens for SSE `hint` events with `rate_limit` signal
> - **Exponential backoff**: Auto-retry on rate limit at 1s→2s→4s→8s→16s, up to 6 attempts
> - **Smart `stop_stream`**: Only called on client disconnect, skipped on normal completion
>
> **Recommended parallelism = accounts ÷ 2**. Tested 4 accounts + 2 concurrent at 100% pass rate across all stress scenarios. Single account + single concurrency also works with the retry mechanism.

```toml
[server]
host = "127.0.0.1"
port = 5317

# API tokens for authentication, leave empty to disable
# [[server.api_tokens]]
# token = "sk-your-token"
# description = "Development"

# Email and/or mobile. Mobile currently appears to support only +86.
[[accounts]]
email = "user1@example.com"
mobile = ""
area_code = ""
password = "pass1"
```

### Free Test Accounts

```
rivigol378@tatefarm.com
test12345

counterfeit1341@wplacetools.com
test12345

idyllic4202@wplacetools.com
test12345

slowly1285@wplacetools.com
test12345
```

For more accounts, try temporary email services (some domains may not work) and register via the international version with a VPN.

Recommended temporary email: [tempmail.la](https://tempmail.la/) (some suffixes may not work, try multiple times)

## API Endpoints

| Method | Path                         | Description                                      |
| ------ | ---------------------------- | ------------------------------------------------ |
| GET    | `/`                          | Health check                                     |
| POST   | `/v1/chat/completions`       | Chat completions (streaming + tool calling)      |
| GET    | `/v1/models`                 | List models                                      |
| GET    | `/v1/models/{id}`            | Get model details                                |
| POST   | `/anthropic/v1/messages`     | Anthropic Messages API (streaming + tool calling)|
| GET    | `/anthropic/v1/models`       | List models (Anthropic format)                   |
| GET    | `/anthropic/v1/models/{id}`  | Get model details (Anthropic format)             |

## Model Mapping

`model_types` in `config.toml` (default: `["default", "expert"]`) maps to:

| OpenAI Model ID    | DeepSeek Mode  |
| ------------------ | -------------- |
| `deepseek-default` | Fast mode      |
| `deepseek-expert`  | Expert mode    |

The Anthropic compatibility layer uses the same model IDs via `/anthropic/v1/messages`.

### Feature Toggles

- **Reasoning**: Enabled by default. Set `"reasoning_effort": "none"` in the request body to disable.
- **Web search**: Enabled by default (DeepSeek backend injects stronger system prompts in search mode, improving tool call compliance). Set `"web_search_options": {"search_context_size": "none"}` to disable.
- **File upload**: Supports inline data URLs (automatically uploaded to the DeepSeek session) and HTTP URLs (triggers web search mode):

  **OpenAI:**
  ```json
  {"type": "file", "file": {"file_data": "data:text/plain;base64,...", "filename": "doc.txt"}}
  {"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}}
  {"type": "image_url", "image_url": {"url": "https://example.com/img.jpg"}}
  ```

  **Anthropic:**
  ```json
  {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "..."}}
  {"type": "document", "source": {"type": "base64", "media_type": "text/plain", "data": "..."}}
  {"type": "image", "source": {"type": "url", "url": "https://example.com/img.jpg"}}
  ```

## Development

Requires Rust 1.95.0+ (see `rust-toolchain.toml`).

> **Prompt Injection Strategy**: This project converts OpenAI message formats into DeepSeek native tags (`<｜User｜>` / `<｜Assistant｜>` / `<｜Tool｜〉`, etc.) and embeds a `<think>` block to guide the model's reasoning chain, injecting tool definitions and formatting instructions. For detailed research and implementation, see [`docs/deepseek-prompt-injection.md`](docs/deepseek-prompt-injection.md). If you have better ideas or findings, feel free to open an issue or PR.

```bash
# One-pass check (check + clippy + fmt + audit + unused deps)
just check

# Run tests
cargo test

# Run HTTP server
just serve

# Unified protocol debug CLI (chat/compare/concurrent modes)
just adapter-cli

# e2e tests (requires server running on port 5317)
just e2e-basic    # Basic features (dual endpoints)
just e2e-repair   # Tool call repair tests
just e2e-stress   # Multi-iteration stress test

# Start server with e2e config
just e2e-serve
```

### Architecture Overview

```mermaid
flowchart TB
    %% ===== Styles =====
    classDef client fill:#eff6ff,stroke:#3b82f6,stroke-width:3px,color:#1d4ed8,rx:14,ry:14
    classDef gateway fill:#fffbeb,stroke:#f59e0b,stroke-width:3px,color:#92400e,rx:12,ry:12
    classDef openai_adapter fill:#f8fafc,stroke:#0a9e7b,stroke-width:2px,color:#334155,rx:10,ry:10
    classDef anthropic_compat fill:#f8fafc,stroke:#d07354,stroke-width:2px,color:#334155,rx:10,ry:10
    classDef ds_core fill:#f8fafc,stroke:#3964fe,stroke-width:2px,color:#1e40af,rx:10,ry:10
    classDef external fill:#fef2f2,stroke:#ef4444,stroke-width:3px,color:#991b1b,rx:6,ry:6

    %% ===== Nodes =====
    Client(["Client"]):::client

    subgraph GW ["HTTP Layer"]
        Handler(["Router / Auth / Serialization"]):::gateway
    end

    subgraph PL ["Protocol Processing Layer"]
        direction TB

        subgraph AC ["Anthropic Compat Layer"]
            A2O["Request Mapping<br/>Anthropic → OpenAI"]:::anthropic_compat
            O2A["Response Mapping<br/>OpenAI → Anthropic"]:::anthropic_compat
        end

        subgraph OA ["OpenAI Adapter Layer"]
            ReqPipe["Request Pipeline<br/>Validation / Tools / Prompt Building"]:::openai_adapter
            RespPipe["Response Pipeline<br/>SSE Parsing / Chunk Conversion / Tool Repair"]:::openai_adapter
        end
    end

    subgraph CL ["Core Layer (ds_core)"]
        Pool["Account Pool"]:::ds_core
        PoW["PoW Solver"]:::ds_core
        Session["Session Lifecycle<br/>Create / Upload / Completion / Destroy"]:::ds_core
    end

    DeepSeek[("DeepSeek API")]:::external

    %% ===== Connections =====
    Client -->|"HTTP Request"| Handler

    Handler -->|"OpenAI Request Struct"| ReqPipe
    Handler -->|"Anthropic Request Struct"| A2O
    A2O -->|"OpenAI Request Struct"| ReqPipe

    ReqPipe --> Pool
    Pool --> PoW
    PoW --> Session
    Session -->|"completion endpoint"| DeepSeek

    Session -.->|"DeepSeek SSE Stream"| RespPipe
    RespPipe -.->|"OpenAI Response Struct"| Handler
    RespPipe -.->|"OpenAI Response Struct"| O2A
    O2A -.->|"Anthropic Response Struct"| Handler

    %% ===== Subgraph Styles =====
    style GW fill:#fffbeb,stroke:#f59e0b,stroke-width:2px,stroke-dasharray: 5 5
    style PL fill:#fafafa,stroke:#94a3b8,stroke-width:2px
    style AC fill:#fdf0ec,stroke:#d07354,stroke-width:2px
    style OA fill:#e6f7f3,stroke:#0a9e7b,stroke-width:2px
    style CL fill:#eef2ff,stroke:#3964fe,stroke-width:2px,stroke-dasharray: 5 5
```

### Data Pipelines

#### OpenAI (chat_completions) Pipeline:

```mermaid
flowchart TB
    %% ===== Styles =====
    classDef ds_core fill:#eef2ff,stroke:#3964fe,stroke-width:2.5px,color:#1e40af,rx:10,ry:10
    classDef openai_adapter fill:#e6f7f3,stroke:#0a9e7b,stroke-width:2.5px,color:#065f46,rx:10,ry:10
    classDef step fill:#fffbeb,stroke:#f59e0b,stroke-width:1.5px,color:#334155,rx:6,ry:6
    classDef output fill:#fff7ed,stroke:#f97316,stroke-width:2.5px,color:#c2410c,rx:10,ry:10

    subgraph RQ ["Request Pipeline"]
        direction TB
        Q1["ChatCompletionsRequest"]:::openai_adapter
        Q2["Validation + Defaults"]:::step
        Q3["Tool/File Extraction + Prompt Injection"]:::step
        Q4["DeepSeek Native Tag Prompt Building"]:::step
        Q5["Model Resolution + Feature Toggles"]:::step
        Q6["Rate Limit Retry<br/>Exponential Backoff 1s→2s→4s→8s→16s"]:::step
        Q7["ChatRequest → ds_core"]:::output
    end

    subgraph RS1 ["Non-Streaming Response"]
        direction TB
        OR1["ds_core SSE Stream"]:::ds_core
        OR2["SSE Frame Parsing<br/>ContentDelta / Usage"]:::step
        OR3["State Machine<br/>Merge Text / Accumulate Usage"]:::step
        OR4["Chunk Aggregation<br/>Concat content / reasoning / tool_calls"]:::step
        OR5["ChatCompletionsResponse"]:::openai_adapter
    end

    subgraph RS2 ["Streaming Response"]
        direction TB
        OS1["ds_core SSE Stream"]:::ds_core
        OS2["SSE Parsing + State Machine"]:::step
        OS3["Chunk Conversion<br/>DsFrame → ChatCompletionsResponseChunk"]:::step
        OS4["Tool Call XML Parsing"]:::step
        OS5["Malformed Tool Call Repair"]:::step
        OS6["Stop Sequence Detection + Obfuscation"]:::step
        OS7["ChatCompletionsResponseChunk"]:::openai_adapter
    end

    Q1 --> Q2 --> Q3 --> Q4 --> Q5 --> Q6 --> Q7
    OR1 --> OR2 --> OR3 --> OR4 --> OR5
    OS1 --> OS2 --> OS3 --> OS4 --> OS5 --> OS6 --> OS7

    style RQ fill:#f8fafc,stroke:#0a9e7b,stroke-width:2px
    style RS1 fill:#f8fafc,stroke:#0a9e7b,stroke-width:2px
    style RS2 fill:#f8fafc,stroke:#0a9e7b,stroke-width:2px
```

#### Anthropic (messages) Pipeline:

```mermaid
flowchart TB
    %% ===== Styles =====
    classDef oai fill:#e6f7f3,stroke:#0a9e7b,stroke-width:2.5px,color:#065f46,rx:10,ry:10
    classDef anth fill:#fdf0ec,stroke:#d07354,stroke-width:2.5px,color:#7c3a2a,rx:10,ry:10
    classDef step fill:#fffbeb,stroke:#f59e0b,stroke-width:1.5px,color:#334155,rx:6,ry:6
    classDef output fill:#fff7ed,stroke:#f97316,stroke-width:2.5px,color:#c2410c,rx:10,ry:10

    subgraph RQ ["Request Pipeline"]
        direction TB
        Q1["MessagesRequest"]:::anth
        Q2["Message Expansion<br/>System Prepend / Text Merge / Image/Doc Mapping"]:::step
        Q3["Tool Mapping<br/>ToolUnion → OpenAI Tool"]:::step
        Q4["Feature Mapping<br/>thinking → reasoning_effort"]:::step
        Q5["ChatCompletionsRequest"]:::oai
    end

    subgraph RS3 ["Non-Streaming Response"]
        direction TB
        AR1["ChatCompletionsResponse"]:::oai
        AR2["Content Decomposition<br/>reasoning → Thinking<br/>content → Text<br/>tool_calls → ToolUse"]:::step
        AR3["ID Mapping<br/>chatcmpl → msg<br/>call → toolu"]:::step
        AR4["MessagesResponse"]:::anth
    end

    subgraph RS4 ["Streaming Response"]
        direction TB
        AS1["ChatCompletionsResponseChunk Stream"]:::oai
        AS2["Chunk State Machine<br/>Block Type Transition / Index Tracking"]:::step
        AS3["Event Mapping<br/>content → text_delta<br/>reasoning → thinking_delta<br/>tool_calls → input_json_delta"]:::step
        AS4["MessagesResponseChunk"]:::anth
    end

    Q1 --> Q2 --> Q3 --> Q4 --> Q5
    AR1 --> AR2 --> AR3 --> AR4
    AS1 --> AS2 --> AS3 --> AS4

    style RQ fill:#f8fafc,stroke:#d07354,stroke-width:2px
    style RS3 fill:#f8fafc,stroke:#d07354,stroke-width:2px
    style RS4 fill:#f8fafc,stroke:#d07354,stroke-width:2px
```

### e2e Tests

`py-e2e-tests/` is a JSON scenario-driven end-to-end test framework (no pytest dependency). Three levels:

| Level      | Command           | Coverage                                                |
| ---------- | ----------------- | ------------------------------------------------------- |
| **Basic**  | `just e2e-basic`  | Core features (OpenAI + Anthropic endpoints), safe concurrency |
| **Repair** | `just e2e-repair` | Malformed tool call repair tests (OpenAI endpoint), safe concurrency |
| **Stress** | `just e2e-stress` | All scenarios × 3 iterations, safe concurrency + 1 concurrency |

Scenarios are organized by type in `scenarios/`:

```
py-e2e-tests/
├── scenarios/
│   ├── basic/
│   │   ├── openai/         # 10 basic scenarios (chat, reasoning, streaming, tool calls, file upload, image upload, HTTP link, etc.)
│   │   └── anthropic/      # 6 basic scenarios (chat, reasoning, tool calls, document upload, image upload, HTTP link)
│   └── repair/             # 10 malformed tool call scenarios
├── runner.py               # Single-run entry point
├── stress_runner.py        # Multi-iteration stress test entry point
└── config.toml             # e2e-specific server config
```

Each scenario is a standalone JSON file with request parameters and validation rules:

```json
{
  "name": "Scenario Name",
  "endpoint": "openai|anthropic",
  "category": "basic|repair",
  "models": ["deepseek-default", "deepseek-expert"],
  "messages": [{"role": "user", "content": "..."}],
  "tools": [...],
  "tool_choice": "auto",
  "request": {"stream": false},
  "checks": {
    "has_tool_calls": true,
    "tool_names": ["get_weather"],
    "finish_reason": "tool_calls",
    "no_error": true
  }
}
```

### e2e CLI Options

**`just e2e-basic` and `just e2e-repair` (single run):**

| Option | Description |
|--------|-------------|
| `scenario_dir` | Scenario directory, e.g. `scenarios/basic` or `scenarios/repair` |
| `--endpoint` | Filter by endpoint: `openai` / `anthropic` |
| `--model` | Filter by model: `deepseek-default` / `deepseek-expert` |
| `--filter` | Filter by keyword(s) in scenario name (space-separated, e.g. `--filter file image`) |
| `--parallel` | Concurrency, default `accounts ÷ 2` |
| `--show-output` | Show model response summaries, tool calls, and finish reasons |
| `--report` | Output JSON report path |

**`just e2e-stress` (stress test):**

| Option | Description |
|--------|-------------|
| `--iterations` | Iterations per scenario, default 3 |
| `--models` | Filter by model list |
| `--filter` | Filter by keyword(s) in scenario name (space-separated) |
| `--parallel` | Concurrency, default `accounts ÷ 2 + 1` |
| `--show-output` | Show model output |
| `--report` | Output JSON report path |

Examples:

```bash
# Quick verification of file/image upload scenarios
just e2e-basic --filter file image --show-output

# OpenAI endpoint only, expert model
just e2e-basic --endpoint openai --model deepseek-expert

# Sequential debugging
just e2e-basic --endpoint openai --parallel 1 --show-output

# Stress test: tool repair scenarios × 5 iterations
just e2e-stress --filter repair --iterations 5

# Output JSON report
just e2e-basic --report result.json
```

**Recommended**: Run e2e tests before submitting a PR.

## License

[Apache License 2.0](LICENSE)

[DeepSeek official API](https://platform.deepseek.com/top_up) is very affordable — please support the official service if you can.

This project was created to experience the latest models from DeepSeek's web grayscale testing.

**Commercial use is strictly prohibited** to avoid burdening official servers. Use at your own risk.
