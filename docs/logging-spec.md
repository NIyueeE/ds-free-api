# 日志系统规范

## 原则

1. **库代码零输出**：`ds_core/` 等库模块只使用 `log` crate，绝不直接打印到 stdout/stderr
2. **调用方控制权**：日志级别、格式、输出目标由应用层（main.rs / examples）决定
3. **结构化 target**：通过 target 路径实现模块级过滤

## 日志级别

| 级别 | 使用场景 | 示例 |
|------|----------|------|
| `ERROR` | 需要人工介入的致命错误 | 所有账号初始化失败、PoW 计算崩溃、配置错误 |
| `WARN` | 降级但可继续的异常 | 单个账号初始化失败、session 清理失败、限流、账号池耗尽、SSE 流中断、tool_parser 解析失败触发修复 |
| `INFO` | 关键生命周期事件 | 账号初始化成功、服务启动/关闭 |
| `DEBUG` | 调试信息 | HTTP 请求/响应摘要、账号分配、SSE 事件类型 |
| `TRACE` | 最细粒度数据 | SSE 原始事件内容、Anthropic 转换细节 |

## Target 规范

格式：`crate::module` 或 `crate::module::submodule`

| 模块 | Target | 说明 |
|------|--------|------|
| `ds_core::accounts` | `ds_core::accounts` | 账号池生命周期、分配、健康检查、限流检测 |
| `ds_core::client` | `ds_core::client` | HTTP 请求/响应、API 调用 |
| `ds_core::completions` | `ds_core::accounts` | 对话编排、SSE 流处理、stop_stream（和 accounts 共用 target）|
| `ds_core::pow` | `ds_core::accounts` | PoW 计算（和 accounts 共用 target）|
| `openai_adapter` | `adapter` | OpenAI 协议适配层（请求解析、响应转换、SSE 解析、tool_parser）|
| `anthropic_compat` | `anthropic_compat` | Anthropic 协议兼容层入口 |
| `anthropic_compat::request` | `anthropic_compat::request` | Anthropic → OpenAI 请求映射 |
| `anthropic_compat::models` | `anthropic_compat::models` | Anthropic 模型列表 |
| `anthropic_compat::response::stream` | `anthropic_compat::response::stream` | Anthropic 流式响应转换 |
| `anthropic_compat::response::aggregate` | `anthropic_compat::response::aggregate` | Anthropic 非流式响应聚合 |
| `server` | `http::server` | 服务生命周期（启动、关闭信号）|
| `server::handlers` | `http::request` / `http::response` | HTTP 请求摘要（路径、stream 标记）、响应摘要（状态码、字节数）|
| `server::error` | `http::response` | HTTP 错误响应（状态码、错误消息）|
| `server::stream` | `http::response` | SSE 流错误 |

## 代码规范

### 库代码（ds_core/）

```rust
use log::{info, debug, warn, error};

// INFO: 关键生命周期
info!(target: "ds_core::accounts", "账号 {} 初始化成功", display_id);

// WARN: 单个失败可降级
warn!(target: "ds_core::accounts", "账号 {} 初始化失败: {}", display_id, e);

// WARN: 限流 / 账号耗尽
warn!(target: "ds_core::accounts", "req={} 账号池无可用账号: model_type={}", request_id, model_type);

// ERROR: 所有账号全部失败
error!(target: "ds_core::accounts", "所有账号初始化失败");

// DEBUG: PoW 调试信息
debug!(target: "ds_core::accounts", "health_check model_type={}", model_type);
```

### 响应转换层（openai_adapter/）

```rust
use log::{debug, trace, warn};

// TRACE: 原始 SSE 事件
trace!(target: "adapter", "<<< {} {}", event, data);

// WARN: SSE 流中断（上游连接异常）
warn!(target: "adapter", "SSE 流错误: {}", e);

// WARN: tool_parser 修复触发
warn!(target: "adapter", "tool_parser 解析失败→请求修复");

// DEBUG: 正常解析
debug!(target: "adapter", "tool_parser 解析出 {} 个工具调用", count);
```

### 应用层（examples/ / main.rs / server/）

```rust
// DEBUG: HTTP 请求摘要（handler 入口）
debug!(target: "http::request", "req={} POST /v1/chat/completions stream={}", req_id, stream);

// DEBUG: HTTP 响应摘要（handler 出口）
debug!(target: "http::response", "req={} 200 JSON response {} bytes", req_id, len);

// ERROR: SSE 流错误（响应发送阶段）
error!(target: "http::response", "SSE stream error: {}", e);
```

## 运行时控制

```bash
# 默认级别（info）
just serve

# 调试模式 - 查看所有 debug 日志
RUST_LOG=debug just serve

# 模块级过滤 - 只看 accounts 的 debug
RUST_LOG=ds_core::accounts=debug just serve

# 多级组合 - accounts 用 debug，其他用 warn
RUST_LOG=ds_core::accounts=debug,ds_core::client=warn,info just serve

# 完全静默（仅错误）
RUST_LOG=error just serve

# 输出到文件
RUST_LOG=debug just serve 2> server.log

# 关注限流事件和请求跟踪
RUST_LOG=ds_core::accounts=debug,adapter=warn just serve
```

## 禁止事项

- ❌ 库代码中直接使用 `println!` / `eprintln!`
- ❌ 使用无 target 的日志宏（如 `log::info!` 不加 target）
- ❌ 在日志中打印敏感信息（token、password）
- ❌ 高频 TRACE 日志（如每个 SSE 字节）默认开启

## 依赖配置

**Cargo.toml**
```toml
[dependencies]
log = "0.4"

[dev-dependencies]
env_logger = { version = "0.11", default-features = false, features = ["auto-color"] }
```

注：`auto-color` 特性在终端中自动添加颜色，在非 TTY 环境自动禁用。
