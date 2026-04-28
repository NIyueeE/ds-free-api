# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.2.5] - 2026-04-28

### Changed
- **请求管道统一**：`OpenAIAdapter` 对外只暴露一个 `chat_completions(req: ChatCompletionsRequest)` 方法，
  内部根据 `stream` 字段自动分流到 SSE 流或 JSON 聚合
- **移除中间结构体**：删除 `AdapterRequest` 和 `prepare` 函数，
  `ChatCompletionsRequest` 贯穿 normalize → tools → prompt → resolver → 分流全管道
- **Anthropic 请求转换直达**：`into_chat_completions()` 纯结构体转换 `MessagesRequest → ChatCompletionsRequest`，
  零 JSON 参与，handler 层 `serde_json::from_slice` 后全链路结构体操作
- **Anthropic 消息入口统一**：合并 `messages()` / `messages_stream()` 为单一 `messages(req: MessagesRequest)` 方法，
  新增 `AnthropicOutput` 枚举（`Stream` / `Json`），与 `ChatOutput` 完全对称；
  handler 不再提前解析 `stream` 字段，JSON 反序列化提至 handler 层对齐 OpenAI 路径
- **Anthropic 类型定义独立**：`types.rs` 专放 Anthropic 协议类型，`request.rs` 只放转换逻辑，
  与 `openai_adapter` 模块结构对称
- **`#![allow(dead_code)]` 精细化**：Anthropic 模块从文件级改为字段级标注，`ds_core/client.rs` 同样缩减为字段级
- **`ds_core` 文件上传顺序修正**：历史文件（`EMPTY.txt`）优先于外部文件上传，对齐对话阅读顺序
- **`ChatCompletionRequest` 重命名**：`ChatCompletionRequest` → `ChatCompletionsRequest`，
  命名对齐实际端点路径
- **`ChatOutput::Stream` 简化**：移除 `input_tokens` 字段，`prompt_tokens` 由 `ConverterStream` 
  在第一个 role chunk 的 usage 中携带，下游按需读取；`from_chat_completion_stream` 不再需要 `input_tokens` 参数
- **响应管道分离**：`StopStream` 拆为 `StopDetectStream`（stop 检测 + obfuscation，输出结构体）+ `SseSerializer`（仅序列化），
  `stream()` 返回 `ChunkStream` 而非 `StreamResponse`，SSE 序列化提至 handler 层
- **中间类型全面清除**：删除 `OpenAiCompletion`、`OpenAiChoice`、`OpenAiMessage`、`OpenAiToolCall`、
  `OpenAiFunctionCall`、`OpenAiCustomToolCall`、`OpenAiUsage`、`SseBuffer`、`OpenAiChunk`、`OpenAiChunkChoice`、`OpenAiDelta` 等 11 个中间类型
- **模型类型命名规范**：`Model` → `OpenAIModel`，`ModelList` → `OpenAIModelList`（openai 侧）；
  `ModelInfo` → `AnthropicModel`，`ModelListResponse` → `AnthropicModelList`（anthropic 侧）；
  模型列表和详情均输出结构体，序列化提至 handler 层
- **`raw_chat_stream` 重命名**：→ `raw_chat_completions_stream`，对齐 `chat_completions` 命名
- **响应类型重命名**：`ChatCompletion` → `ChatCompletionsResponse`，`ChatCompletionChunk` → `ChatCompletionsResponseChunk`，命名与请求端对齐

### Removed
- **`AdapterRequest` / `prepare` 函数**：被内联到 `chat_completions` 中
- **`parse_request` 方法**：不再需要，外部直接 `serde_json::from_slice` 构造 `ChatCompletionsRequest`
- **冗余单元测试**：删除 7 个重叠测试（`multimodal_user`、`tools_injection`、`tools_after_tool_role_message`、`function_call_none_ignores_functions`、`stream_true`、`aggregate_tool_calls_with_trailing_text`），合并 `stream_options_defaults`/`explicit` 为参数化测试
- **Anthropic 模块测试精炼**：删除 `top_k_not_mapped`、`stream_tool_calls`、`malformed_json_error`，
  合并 `image_base64`/`image_url`、`tool_calls`/`text_and_tool_calls`、`empty_content`/`null_content` 为参数化测试
- **`stream_tool_calls_repair_with_live_ds` 忽略测试**：已由 `py-e2e-tests/scenarios/repair/` 覆盖，不再保留被 `#[ignore]` 的死代码

## [0.2.4] - 2026-04-27

### Added
- **历史对话文件化**：多轮对话历史自动拆分上传为独立文件，绕过 DeepSeek 单次输入长度限制。
  对适配器层完全透明，上传失败不影响主流程，自动退化为纯文本发送
- **临时 Session 生命周期**：每次请求创建独立 session，请求结束自动清理（stop_stream + delete_session），
  彻底杜绝 session 泄漏和 TTL 过期残留
- **工具调用自修复**：当模型输出的 tool_calls 格式异常时，使用 DeepSeek 自身修复损坏的 JSON/XML，
  流式和非流式路径均覆盖，大幅提升工具调用成功率
- **arguments 类型归一**：自动处理 arguments 为 JSON 字符串的异常情况，避免客户端双重转义解析失败
- **`input_exceeds_limit` 检测**：识别输入超长错误并返回明确错误信息，不再静默失败
- **全链路日志追踪**：`req-{n}` 标识贯穿 handler → adapter → ds_core 全层，
  `x-ds-account` 响应头标识处理账号，单次请求可完整 grep 追踪
- **TRACE 级别字节追踪**：流管道各层 TRACE 日志，可观察字节在 SSE 管道中的完整转换过程
- **`/` 端点**：免鉴权返回可用端点列表和项目地址
- **e2e 测试重构**：从 pytest 迁移为 JSON 场景驱动框架，场景独立存放，配置动态读取

### Changed
- **请求流程重构**：从"持久 session + edit_message"升级为"临时 session + completion + 文件上传"，
  每次请求独立生命周期，不再依赖预创建的持久 session
- **限流自动重试**：检测到 rate_limit 时以 1s→2s→4s→8s→16s 指数退避自动重试（最多 6 次），
  对用户透明，大幅降低限流导致的请求失败
- **Prompt 构建优化**：reminder 插入位置调整到最后一轮对话之前，确保模型优先遵循指令；
  工具描述的代码块格式化；工具调用结果的 Markdown 结构化展示
- **推理控制语义修正**：禁用思考时使用 `"none"` 替代 `"minimal"`，语义更明确
- **日志级别规范化**：账号池耗尽提升为 `WARN`，常规分配降为 `DEBUG`，
  新增 session/上传/PoW 等 debug 日志，health_check 合并为单条带耗时日志

### Removed
- 账号初始化不再按 model_type 管理 session，移除 session 持久化和 update_title 逻辑
- 移除旧 pytest e2e 测试目录（被 JSON 场景驱动框架替代）

### Test Results

#### py-e2e-tests
- **4 账号 + 3 并发 + 3 迭代**：17 场景 × 2 模型 × 3 次 = 102 次请求，成功率 100%，总耗时 5.5 分钟
- 覆盖场景：基础对话、深度思考、流式、标准工具调用，以及 10 种 tool_calls 损坏格式
  （XML/JSON 混合、字段名不一致、arguments 字符串、括号不匹配/缺失、
  name/arguments 互换、参数外溢等），修复管道全部正确兜底

#### claude-code 测试
```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:5317/anthropic
export ANTHROPIC_AUTH_TOKEN=sk-test
export ANTHROPIC_DEFAULT_OPUS_MODEL=deepseek-expert
export ANTHROPIC_DEFAULT_SONNET_MODEL=deepseek-expert
export ANTHROPIC_DEFAULT_HAIKU_MODEL=deepseek-default
claude
```
- 基本稳定, 工具解析时会使得claude-code暂时卡住是正常现象, 部分情况可能出现模型不遵循指令导致工具调用指令泄漏
- 其他编程工具没有大量测试, 希望大家积极反馈

## [0.2.3] - 2026-04-24

### Added
- Tool call XML 解析增强：增加 `repair_invalid_backslashes` 与 `repair_unquoted_keys`
  宽松修复，当模型输出的 JSON 包含未引号 key 或无效转义时自动修复后重试
- 增加 `is_inside_code_fence` 检查：跳过 markdown 代码块中的工具示例，防止误解析
- 新增 Anthropic 协议压测脚本 `stress_test_tools_anthropic.py`，与 OpenAI 版对称
- 示例文件正交化：`examples/adapter_cli/` 下按功能拆分为
  `basic_chat`/`stream`/`stop`/`reasoning`/`web_search`/`reasoning_search`/`tool_call` 等独立文件
- 默认 adapter-cli 配置文件路径指向 `py-e2e-tests/config.toml`

### Changed
- 账号池选择策略：从**轮询线性探测**改为**空闲最久优先**，最大化账号复用间隔
- 移除固定的冷却时间常量，选择算法天然避免账号被过快重用
- 同步更新中英文 README，增加并发经验说明

### Stress Test Results

针对 4 账号池的 70 请求压测（7 场景 × 2 模型 × 5 迭代）：

| 策略 | 并发 | 成功率 | 平均耗时 |
|------|------|--------|----------|
| 轮询 + 无冷却 | 3 | 25.7% | 2.57s |
| 轮询 + 2s 冷却 | 3 | 97.1% | 10.46s |
| **空闲最久优先 + 无冷却** | **2** | **100%** | **10.14s** |
| **空闲最久优先 + 无冷却 (Anthropic)** | **2** | **100%** | **11.31s** |

结论：稳定安全并发 ≈ 账号数 ÷ 2，空闲最久优先策略可在不设冷却的前提下实现 100% 成功率。

## [0.2.2] - 2026-04-22

### Added
- Anthropic Messages API 兼容层：
  - `/anthropic/v1/messages` streaming + non-streaming 端点
  - `/anthropic/v1/models` list/get 端点（Anthropic 格式）
  - 请求映射：Anthropic JSON → OpenAI ChatCompletion
  - 响应映射：OpenAI SSE/JSON → Anthropic Message SSE/JSON
- OpenAI adapter 向后兼容：
  - 已弃用的 `functions`/`function_call` 自动映射为 `tools`/`tool_choice`
  - `response_format` 降级：在 ChatML prompt 中注入 JSON/Schema 约束（`text` 类型为 no-op）
- CI 发布流程改进：
  - tag 触发 release（`push.tags v*`）
  - CHANGELOG 自动提取版本说明
  - 发布前校验 Cargo.toml 版本与 tag 一致

### Changed
- Rust toolchain 升级到 1.95.0，CI workflow 同步更新
- justfile 添加 `set positional-arguments`，安全传递带空格的参数
- Python E2E 测试套件重组为 `openai_endpoint/` 和 `anthropic_endpoint/`
- 启动日志显示 OpenAI 和 Anthropic base URLs
- README/README.en.md 添加 SVG 图标、GitHub badges、同步文档
- LICENSE 添加版权声明 `Copyright 2026 NIyueeE`
- CLAUDE.md/AGENTS.md 同步更新

### Fixed
- Anthropic 流式工具调用协议：使用 `input_json_delta` 事件逐步传输工具参数
- Tool use ID 映射一致性：`call_{suffix}` → `toolu_{suffix}`
- Anthropic 工具定义兼容：处理缺少 `type` 字段的情况（Claude Code 客户端）

## [0.2.1] - 2026-04-15

### Added
- 默认开启深度思考：`reasoning_effort` 默认设为 `high`，搜索默认关闭。
- WASM 动态探测：`pow.rs` 改为基于签名的动态 export 探测，不再硬编码 `__wbindgen_export_0`，降低 DeepSeek 更新 WASM 后启动失败的风险。
- 新增 Python E2E 测试套件：覆盖 auth、models、chat completions、tool calling 等场景。
- 新增 `tiktoken-rs` 依赖，用于服务端 prompt token 计算。
- CI 新增 `cargo audit` 与 `cargo machete` 检查。

### Changed
- 账号初始化优化：日志在手机号为空时自动回退显示邮箱。
- 更新 `axum`、`cranelift` 等核心依赖至最新 patch 版本。
- Client Version 保持与网页端一致的 `1.8.0`。

### Removed
- 移除未使用的 `tower` 依赖。

## [0.2.0] - 2026-04-13

### Added
- 项目从 Python 全面重构到 Rust，带来原生高性能和跨平台支持。
- OpenAI 兼容 API（`/v1/chat/completions`、`/v1/models`）。
- 账号池轮转 + PoW 求解 + SSE 流式响应。
- 深度思考和智能搜索支持。
- Tool calling（XML 解析）。
- GitHub CI + 多平台 Release（8 目标平台）。
- 兼容最新 DeepSeek Web 后端接口。
