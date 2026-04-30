# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.2.5] - 2026-04-30

### Added
- **流式工具调用保活机制**：`CollectingXml` 和修复等待期间每 1s 发送空工具增量块
  （OpenAI: `tool_calls[{index:0}]`，Anthropic: 持续 thinking 块 `"tool_calls..."`）
- **XML `<invoke>` 格式解析**：直接解析 `<invoke name="..."><parameter>` 格式，无需修复管道
- **修复模型工具定义注入**：修复请求携带工具列表，帮助从破碎文本推测正确参数
- **修复模型 JSON 转义提示**：提醒修复模型对字符串值中的引号和换行符进行转义
- **全链路日志追踪增强**：`<<<` / `>>>` 格式统一，覆盖 ds_core SSE → OpenAI chunk → Anthropic SSE 三层
- **Anthropic Ping 事件支持**：`MessagesResponseChunk` 新增 `Ping` 变体
- **Anthropic 流式响应测试**：新增 12 个测试覆盖 text/thinking/tool_calls/keepalive/优雅关闭等场景
- **模块可见性收紧**：内部 submodule 的 `pub` 改为 `pub(crate)`，API 表面更精确

### Changed
- **工具调用主标签改为 `<|tool▁calls▁begin|>` / `<|tool▁calls▁end|>`**：
  使用 ASCII `|` 替代全角 `｜`，避免后端特殊处理；模型识别和遵循度显著提升，幻觉大幅减少
- **Prompt 格式对齐官方 chat_template**：
  - `<｜end▁of▁sentence｜>` 在每个 `<｜User｜>` 前闭合上一轮 assistant + 工具结果
  - `format_message` 去掉尾随 `\n`，角色标签间紧凑
  - `<｜tool▁outputs▁begin｜>` 替换工具结果的 Markdown 噪声格式
  - 连续 tool messages 合并为单一 `<｜tool▁outputs▁begin｜>` 块
- **回退标签策略改为实验驱动**：默认回退列表清空，增量添加发现的幻觉变体（如 `<|tool_calls_begin|>`）
- **规则文本使用 `{TOOL_CALL_START}` 常量**：tools.rs 的 3 处硬编码 `<tool_calls>` 改为常量引用
- **`find_end_tag_with` 修复**：已知结束标签回退不再依赖 `start_tag` 推导，避免 `begin`→`end` 推导失败
- **`docs/deepseek-prompt-injection.md`**：更新实验发现和当前策略
- **README 中英文**：新增工具调用标签幻觉用户自维护说明

### Fixed
- **文件上传错误处理分层**：历史文件（`EMPTY.txt`）上传失败时回退为完整 prompt 内联发送，
  不再静默丢失上下文；外部文件上传失败直接返回错误，不再静默跳过
- **Anthropic 响应格式不对齐**：`message_start` 补回 `stop_reason: null` / `stop_sequence: null`；
  `message_delta` 始终包含 `usage.output_tokens`（标准版 Anthropic 需要这些字段）
- **Anthropic usage 始终为 0**：`stream_options.include_usage` 默认未设置导致 usage 丢失，
  修复为 Anthropic 请求强制开启；ConverterStream 将 usage 合并到 finish chunk；
  ToolCallStream Done 状态保留 usage

### Removed
- **冗余 converter 单元测试**：`converter_emits_role_and_content` 已被 response.rs 集成测试全覆盖

### Added
- **Prompt 注入调研文档**：[`docs/deepseek-prompt-injection.md`](docs/deepseek-prompt-injection.md)，
  记录 DeepSeek 网页端原生标签（`<｜User｜>` / `<｜Assistant｜>` 等）的分析与注入策略调研过程
- **OpenAI adapter 文件提取**：新增 `files.rs`，支持从 `file` / `image_url` content part 提取内联 data URL
  为 `FilePayload` 自动上传到 DeepSeek 会话；HTTP URL 自动标记开启搜索模式
- **Anthropic compat Document 支持**：`ContentBlock` 新增 `Document` 变体，
  base64 文档映射为 OpenAI `file` content part 上传，URL 文档触发搜索模式
- **e2e 文件上传测试场景**：新增 6 个场景（OpenAI 文件/图片/HTTP 链接 + Anthropic 文档/图片/HTTP 链接）

### Changed
- **智能搜索默认开启**：DeepSeek 后端在搜索模式下注入更强的系统提示词，提升工具调用遵循度
- **工具调用标签回退扩展**：新增 `<function_calls>`、`<|tool_calls|>`、`<function-call>` 到默认回退；
  开始标签支持部分匹配（不要求 `>`）；结束标签确认开始后只匹配对应的 `</xxx>` 和 `<xxx>`
- **JSON 解析强化**：支持缺失 `]` 和 `}` 的未闭合 JSON，自动取至末尾或回退单对象解析
- **工具调用规则强化**：新增 `**核心：**` 规则禁止工具调用前输出解释性文字；移除重复的包裹指令
- **reminder 前缀优化**：`<think>` 块开头改为 `嗯，我刚刚被系统提醒需要遵循以下内容:`
- **消息合并**：连续相同 role 的消息在 prompt 构建时自动合并，避免 DeepSeek 混淆
- **工具示例格式优化**：单行 `<tool_calls>[JSON]</tool_calls>`；描述用 `~~~markdown`
- **`format_part` 改进**：`image_url` HTTP URL 输出 `[请访问这个链接: {url}]` 替代无意义占位符；
  `file` content part 保留 `text` 描述字段
- **`openai_adapter.rs`**：接入 `files::extract`，HTTP URL 时自动开启搜索模式
- **`anthropic_compat/request.rs`**：`user_blocks_to_messages` 增加 `FilePart` 处理和 `infer_doc_filename`
- **e2e 测试套件增强**：实时进度展示、`--show-output` 参数显示模型输出、`--filter` 支持多个关键词、
  汇总表增加端点列、使用完整模型 ID
- **README 中英文同步**：新增文件上传功能说明、能力开关示例、e2e CLI 参数文档
- **主标签改为 `<tool_calls>`**：从 `<tool_call>`（无 s）改为带 s 版本，更贴近模型自然输出
- **回退标签分离**：`FALLBACK_STARTS` 和 `FALLBACK_ENDS` 独立数组，不要求成对匹配；
  支持从 `config.toml` 配置额外幻觉标签，默认值统一在 `config.rs`
- **`stream()` / `aggregate()` 参数精简**：合并为 `StreamCfg` 结构体，参数从 8 个减至 3 个
- **mermaid 架构图同步**：标注文件提取和文档映射步骤
- **Prompt 格式重构**：从 ChatML（`<|im_start|>` / `<|im_end|>`）迁移到 DeepSeek 原生标签格式
  （`<｜{Role}｜>{content}\n`），`role_tag` 改为首字母大写而非映射表
- **Reminder 注入方式变更**：从独立的 `<|im_start|>reminder` 块改为嵌入最后一个
  `<｜Assistant｜>` 后的不闭合 `<think>` 块中，前缀 `我被系统提醒如下信息:`
- **工具调用指令统一**：`(工具调用请使用 <tool_call> 和 </tool_call> 包裹。)`
  从追加入 user/tool 消息改为统一放在 `<think>` 块末尾
- **移除尾部 assistant**：不再追加 `<|im_start|>assistant`，模型生成由 `<think>`
  块中的 reminder 引导触发
- **历史拆分解析适配**：`parse_chatml_blocks` → `parse_native_blocks`，
  基于 `<｜Role｜>` 标签解析，内容截止到下一个 `<｜` 或 EOF，无需闭合标签
- **README / README.en.md 同步**：更新 Prompt 注入策略说明及数据管道 mermaid 图中的标签描述
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
- **`KeepaliveStream`（HTTP 层保活）**：已从 `SseBody` 移除，保活逻辑下移到 `ToolCallStream`/`RepairStream`
- **`<think>` 内包裹指令** `(工具调用请使用 ... 包裹。)` 已移除（与 rules 重复）
- **Reasoning 重定向逻辑**：`Detecting`/`CollectingXml` 不再处理 `reasoning_content`
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
