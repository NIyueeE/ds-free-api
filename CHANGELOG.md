# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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
