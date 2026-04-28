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

[English](README.en.md)

将免费的 DeepSeek 网页端对话反代并适配转换为标准的 OpenAI 与 Anthropic 兼容 API 协议（目前支持 chat completions 和 messages，包括流式返回与工具调用）。

## 项目亮点

- **零成本 API 代理**：使用 DeepSeek 免费网页端，无需官方 API Key，即可获得 OpenAI / Anthropic 兼容接口
- **双协议支持**：同时兼容 OpenAI Chat Completions 与 Anthropic Messages API，主流客户端即插即用
- **工具调用就绪**：OpenAI function calling 完整实现，工具解析 + 三层自修复管道（文本修复 → JSON 修复 → 模型兜底），覆盖 10+ 异常格式
- **Rust 实现**：单可执行文件 + 单 TOML 配置，跨平台原生高性能
- **多账号池**：空闲最久优先轮转，支持水平扩展并发

## 快速开始

去 [releases](https://github.com/NIyueeE/ds-free-api/releases) 下载对应平台后解压即可。

```
  .
  ├── ds-free-api          # 可执行文件
  ├── LICENSE
  ├── README.md
  ├── README.en.md
  └── config.example.toml  # 配置示例
```

### 配置

复制 `config.example.toml` 为 `config.toml`，和可执行文件保持在同一个路径下，或者使用 `./ds-free-api -c <config_path>`  指定配置路径。

### 运行

```bash
# 直接运行 (同目录下需要 config.toml)
./ds-free-api

# 指定配置路径
./ds-free-api -c /path/to/config.toml

# 调试模式
RUST_LOG=debug ./ds-free-api
```

这里只展示必填项。一个账号对应一个并发量。

> **并发说明**：DeepSeek 免费 API 对每个 session 有速率限制（`Messages too frequent. Try again later.`），单账号在频繁请求时会触发限流。本项目内置以下机制保障稳定：
> - **限流自动检测**：监听 SSE `hint` 事件中的 `rate_limit` 信号，快速识别限流
> - **指数退避重试**：检测到限流后自动重试，间隔为 1s→2s→4s→8s→16s，最多 6 次
> - **`stop_stream` 智能触发**：仅在客户端主动断连时调用，正常完成时跳过，避免请求冲突
>
> **推荐并行数 = 账号数 ÷ 2**。实测 4 账号 + 2 并发可 100% 通过全部压测场景。单账号 + 单并发在上述重试机制下也可跑通。

```toml
[server]
host = "127.0.0.1"
port = 5317

# API 访问令牌，留空则不鉴权
# [[server.api_tokens]]
# token = "sk-your-token"
# description = "开发测试"

# 邮箱和手机号二选一或都填，手机号目前好像只支持 +86
[[accounts]]
email = "user1@example.com"
mobile = ""
area_code = ""
password = "pass1"
```

这里分享几个免费的测试账号，不要发敏感信息（虽然程序每次会收尾删除会话，但是可能会遗留）。

```text
rivigol378@tatefarm.com
test12345

counterfeit1341@wplacetools.com
test12345

idyllic4202@wplacetools.com
test12345

slowly1285@wplacetools.com
test12345
```

想要自己多整几个账号并发的话，可以研究一下临时邮箱（有些可能不行），然后加魔法在国际版中多注册几个账号。

推荐临时邮箱网站：[tempmail.la](https://tempmail.la/) (有些后缀可能不行, 建议多尝试几次)

## API 端点

| 方法 | 路径                        | 说明                                         |
| ---- | --------------------------- | -------------------------------------------- |
| GET  | `/`                         | 健康检查                                     |
| POST | `/v1/chat/completions`      | 聊天补全（支持流式与工具调用）               |
| GET  | `/v1/models`                | 模型列表                                     |
| GET  | `/v1/models/{id}`           | 模型详情                                     |
| POST | `/anthropic/v1/messages`    | Anthropic Messages API（支持流式与工具调用） |
| GET  | `/anthropic/v1/models`      | 模型列表（Anthropic 格式）                   |
| GET  | `/anthropic/v1/models/{id}` | 模型详情（Anthropic 格式）                   |

## 模型映射

`config.toml` 中 `model_types`（默认 `["default", "expert"]`）自动映射：

| OpenAI 模型 ID     | DeepSeek 类型 |
| ------------------ | ------------- |
| `deepseek-default` | 快速模式      |
| `deepseek-expert`  | 专家模式      |

Anthropic 兼容层使用相同的模型 ID，通过 `/anthropic/v1/messages` 调用。

### 能力开关

- **深度思考**：默认已开启。如需显式关闭，请求体中加 `"reasoning_effort": "none"`。
- **智能搜索**：默认关闭。如需开启，请求体中加 `"web_search_options": {"search_context_size": "high"}`。

## 开发

需要 Rust 1.95.0+（见 `rust-toolchain.toml`）。

```bash
# 一键检查 (check + clippy + fmt + audit + unused deps)
just check

# 运行测试
cargo test

# 运行 HTTP 服务
just serve

# 统一协议调试 CLI（内置对话/比较/并发等模式）
just adapter-cli

# e2e 测试（需要服务已在 5317 端口运行，场景正交）
just e2e-basic    # 基础功能（双端点）
just e2e-repair   # 工具调用修复专项
just e2e-stress   # 多迭代压测（全部场景）

# 使用 e2e 专属配置启动服务
just e2e-serve
```

### 简要架构图：

```mermaid
flowchart TB
    %% ===== 主题定义 =====
    classDef client fill:#eff6ff,stroke:#3b82f6,stroke-width:3px,color:#1d4ed8,rx:14,ry:14
    classDef gateway fill:#fffbeb,stroke:#f59e0b,stroke-width:3px,color:#92400e,rx:12,ry:12
    classDef openai_adapter fill:#f8fafc,stroke:#0a9e7b,stroke-width:2px,color:#334155,rx:10,ry:10
    classDef anthropic_compat fill:#f8fafc,stroke:#d07354,stroke-width:2px,color:#334155,rx:10,ry:10
    classDef ds_core fill:#f8fafc,stroke:#3964fe,stroke-width:2px,color:#1e40af,rx:10,ry:10
    classDef external fill:#fef2f2,stroke:#ef4444,stroke-width:3px,color:#991b1b,rx:6,ry:6

    %% ===== 节点 =====
    Client(["🖥️ 客户端"]):::client

    subgraph GW ["🌐 HTTP 接入层"]
        Handler(["路由 / 鉴权 / 序列化"]):::gateway
    end

    subgraph PL ["⚙️ 协议处理层"]
        direction TB

        subgraph AC ["Anthropic 兼容层"]
            A2O["请求转换<br/>Anthropic → OpenAI"]:::anthropic_compat
            O2A["响应转换<br/>OpenAI → Anthropic"]:::anthropic_compat
        end

        subgraph OA ["OpenAI 适配层"]
            ReqPipe["请求管道<br/>校验 / 工具提取 / 提示词构建"]:::openai_adapter
            RespPipe["响应管道<br/>SSE 解析 / 格式转换 / 工具修复"]:::openai_adapter
        end
    end

    subgraph CL ["🔧 核心层 (ds_core)"]
        Pool["账号池轮转"]:::ds_core
        PoW["PoW 求解"]:::ds_core
        Session["会话编排<br/>创建销毁 / 历史上传"]:::ds_core
    end

    DeepSeek[("🔴 DeepSeek API")]:::external

    %% ===== 连接 =====
    Client -->|"HTTP 请求"| Handler

    Handler -->|"OpenAI 请求结构体"| ReqPipe
    Handler -->|"Anthropic 请求结构体"| A2O
    A2O -->|"OpenAI 请求结构体"| ReqPipe

    ReqPipe --> Pool
    Pool --> PoW
    PoW --> Session
    Session -->|"completion 端点"| DeepSeek

    Session -.->|"DeepSeek SSE数据流"| RespPipe
    RespPipe -.->|"OpenAI 响应结构体"| Handler
    RespPipe -.->|"OpenAI 响应结构体"| O2A
    O2A -.->|"Anthropic 响应结构体"| Handler

    %% ===== 子图样式 =====
    style GW fill:#fffbeb,stroke:#f59e0b,stroke-width:2px,stroke-dasharray: 5 5
    style PL fill:#fafafa,stroke:#94a3b8,stroke-width:2px
    style AC fill:#fdf0ec,stroke:#d07354,stroke-width:2px
    style OA fill:#e6f7f3,stroke:#0a9e7b,stroke-width:2px
    style CL fill:#eef2ff,stroke:#3964fe,stroke-width:2px,stroke-dasharray: 5 5
```

### 数据管道：

#### OpenAI (chat_completions) 处理管道:

```mermaid
flowchart TB
    %% ===== 主题定义 =====
    classDef ds_core fill:#eef2ff,stroke:#3964fe,stroke-width:2.5px,color:#1e40af,rx:10,ry:10
    classDef openai_adapter fill:#e6f7f3,stroke:#0a9e7b,stroke-width:2.5px,color:#065f46,rx:10,ry:10
    classDef step fill:#fffbeb,stroke:#f59e0b,stroke-width:1.5px,color:#334155,rx:6,ry:6
    classDef output fill:#fff7ed,stroke:#f97316,stroke-width:2.5px,color:#c2410c,rx:10,ry:10

    subgraph RQ ["请求处理"]
        direction TB
        Q1["ChatCompletionsRequest"]:::openai_adapter
        Q2["参数校验 + 默认值"]:::step
        Q3["工具定义提取 + 注入提示词"]:::step
        Q4["ChatML 提示词构建"]:::step
        Q5["模型映射 + 能力开关"]:::step
        Q6["限流重试<br/>指数退避 1s→2s→4s→8s→16s"]:::step
        Q7["ChatRequest → ds_core"]:::output
    end

    subgraph RS1 ["非流式响应"]
        direction TB
        OR1["ds_core SSE 流"]:::ds_core
        OR2["SSE 帧解析<br/>ContentDelta / Usage"]:::step
        OR3["状态机重组<br/>合并连续文本 / 累积 usage"]:::step
        OR4["chunk 聚合<br/>拼接 content / reasoning / tool_calls"]:::step
        OR5["ChatCompletionsResponse"]:::openai_adapter
    end

    subgraph RS2 ["流式响应"]
        direction TB
        OS1["ds_core SSE 流"]:::ds_core
        OS2["SSE 帧解析 + 状态机"]:::step
        OS3["Chunk 转换<br/>DsFrame → ChatCompletionsResponseChunk"]:::step
        OS4["工具调用 XML 解析"]:::step
        OS5["异常工具调用自修复"]:::step
        OS6["stop 序列检测 + obfuscation"]:::step
        OS7["ChatCompletionsResponseChunk"]:::openai_adapter
    end

    Q1 --> Q2 --> Q3 --> Q4 --> Q5 --> Q6 --> Q7
    OR1 --> OR2 --> OR3 --> OR4 --> OR5
    OS1 --> OS2 --> OS3 --> OS4 --> OS5 --> OS6 --> OS7

    style RQ fill:#f8fafc,stroke:#0a9e7b,stroke-width:2px
    style RS1 fill:#f8fafc,stroke:#0a9e7b,stroke-width:2px
    style RS2 fill:#f8fafc,stroke:#0a9e7b,stroke-width:2px
```

#### Anthropic (messages) 处理管道:

```mermaid
flowchart TB
    %% ===== 主题定义 =====
    classDef oai fill:#e6f7f3,stroke:#0a9e7b,stroke-width:2.5px,color:#065f46,rx:10,ry:10
    classDef anth fill:#fdf0ec,stroke:#d07354,stroke-width:2.5px,color:#7c3a2a,rx:10,ry:10
    classDef step fill:#fffbeb,stroke:#f59e0b,stroke-width:1.5px,color:#334155,rx:6,ry:6
    classDef output fill:#fff7ed,stroke:#f97316,stroke-width:2.5px,color:#c2410c,rx:10,ry:10

    subgraph RQ ["请求处理"]
        direction TB
        Q1["MessagesRequest"]:::anth
        Q2["消息展开<br/>System 前置 / 文本合并 / 图片映射"]:::step
        Q3["工具映射<br/>ToolUnion → OpenAI Tool"]:::step
        Q4["能力开关映射<br/>thinking → reasoning_effort"]:::step
        Q5["ChatCompletionsRequest"]:::oai
    end

    subgraph RS3 ["非流式响应"]
        direction TB
        AR1["ChatCompletionsResponse"]:::oai
        AR2["Content 拆解<br/>reasoning → Thinking<br/>content → Text<br/>tool_calls → ToolUse"]:::step
        AR3["ID 映射<br/>chatcmpl → msg<br/>call → toolu"]:::step
        AR4["MessagesResponse"]:::anth
    end

    subgraph RS4 ["流式响应"]
        direction TB
        AS1["ChatCompletionsResponseChunk 流"]:::oai
        AS2["Chunk 状态机<br/>块类型切换 / 索引递进"]:::step
        AS3["事件映射<br/>content → text_delta<br/>reasoning → thinking_delta<br/>tool_calls → input_json_delta"]:::step
        AS4["MessagesResponseChunk"]:::anth
    end

    Q1 --> Q2 --> Q3 --> Q4 --> Q5
    AR1 --> AR2 --> AR3 --> AR4
    AS1 --> AS2 --> AS3 --> AS4

    style RQ fill:#f8fafc,stroke:#d07354,stroke-width:2px
    style RS3 fill:#f8fafc,stroke:#d07354,stroke-width:2px
    style RS4 fill:#f8fafc,stroke:#d07354,stroke-width:2px
```

### e2e 测试

`py-e2e-tests/` 是基于 JSON 场景驱动的端到端测试框架，无需 pytest 依赖。分为三层：

| 层级       | 命令              | 覆盖范围                                              |
| ---------- | ----------------- | ----------------------------------------------------- |
| **Basic**  | `just e2e-basic`  | 基础功能场景（双端点 OpenAI + Anthropic），安全并发数 |
| **Repair** | `just e2e-repair` | 工具调用异常格式修复专项（OpenAI 单端点），安全并发数 |
| **Stress** | `just e2e-stress` | 全部场景 × 3 次迭代，安全并发数 + 1 并发              |

场景文件在 `scenarios/` 中按类型独立存放：

```
py-e2e-tests/
├── scenarios/
│   ├── basic/
│   │   ├── openai/         # 7 个基础场景（对话、推理、流式、工具调用等）
│   │   └── anthropic/      # 3 个基础场景（对话、推理、工具调用）
│   └── repair/             # 10 个工具损坏格式场景
├── runner.py               # 单次运行入口
├── stress_runner.py        # 多迭代压测入口
└── config.toml             # e2e 专用服务端配置
```

每个场景为独立 JSON 文件，包含请求参数和校验规则：

```json
{
  "name": "场景名称",
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

**可选**: 建议通过这个e2e测试后再提PR

## 许可证

[Apache License 2.0](LICENSE)

[DeepSeek 官方 API](https://platform.deepseek.com/top_up) 非常便宜，请大家多多支持官方服务。

本项目的初心是想体验官方网页端灰度测试的最新模型。

**严禁商用**，避免对官方服务器造成压力，否则风险自担。

~~还有deepseek依旧是国一模!!!~~
