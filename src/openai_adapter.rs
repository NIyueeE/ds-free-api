//! OpenAI 协议适配层 —— OpenAI JSON 与 ds_core 内部格式的双向转换
//!
//! 本模块负责将 OpenAI 兼容的 HTTP 请求转换为 ds_core 内部格式，
//! 并将 ds_core 的响应转换为 OpenAI 兼容的 JSON 格式。
//!
//! 对外暴露最小接口：OpenAIAdapter, OpenAIAdapterError

use bytes::Bytes;
use futures::{Stream, StreamExt};
use std::pin::Pin;
use std::sync::Arc;

use crate::ds_core::{CoreError, DeepSeekCore};

mod models;
pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod types;

pub use types::{ChatCompletionsRequest, ChatCompletionsResponse, ChatCompletionsResponseChunk};

/// 流式响应类型（SSE 字节流）
pub type StreamResponse = Pin<Box<dyn Stream<Item = Result<Bytes, OpenAIAdapterError>> + Send>>;

/// 流式响应结构体流
pub type ChunkStream =
    Pin<Box<dyn Stream<Item = Result<ChatCompletionsResponseChunk, OpenAIAdapterError>> + Send>>;

/// Chat Completions 统一输出
pub enum ChatOutput {
    Stream(ChunkStream),
    Json(ChatCompletionsResponse),
}

/// adapter 层通用结果包装：携带请求结果和账号标识
pub struct ChatResult<T> {
    pub data: T,
    pub account_id: String,
}

/// OpenAI 适配器
pub struct OpenAIAdapter {
    ds_core: Arc<DeepSeekCore>,
    model_types: Vec<String>,
    model_registry: std::collections::HashMap<String, String>,
    max_input_tokens: Vec<u32>,
    max_output_tokens: Vec<u32>,
    tag_config: Arc<response::TagConfig>,
}

impl OpenAIAdapter {
    /// 创建适配器实例
    pub async fn new(config: &crate::config::Config) -> Result<Self, OpenAIAdapterError> {
        let ds_core = Arc::new(DeepSeekCore::new(config).await?);
        let model_registry = config.deepseek.model_registry();
        Ok(Self {
            ds_core,
            model_types: config.deepseek.model_types.clone(),
            model_registry,
            max_input_tokens: config.deepseek.max_input_tokens.clone(),
            max_output_tokens: config.deepseek.max_output_tokens.clone(),
            tag_config: Arc::new(response::TagConfig::from_config(&config.deepseek.tool_call)),
        })
    }

    /// POST /v1/chat/completions（统一入口）
    ///
    /// 内部校验参数、构建 ChatML prompt、按 stream 标记分流：
    /// - stream=true  → 返回 SSE 字节流
    /// - stream=false → 将 SSE 流聚合为单个 JSON 对象后返回
    pub async fn chat_completions(
        &self,
        mut req: ChatCompletionsRequest,
        request_id: &str,
    ) -> Result<ChatResult<ChatOutput>, OpenAIAdapterError> {
        use crate::openai_adapter::types::{
            FunctionCallOption, NamedFunction, NamedToolChoice, Tool, ToolChoice,
        };

        // 兼容旧版 functions / function_call → tools / tool_choice
        if req.tools.as_ref().map(|t| t.is_empty()).unwrap_or(true)
            && let Some(functions) = req.functions.clone()
            && !functions.is_empty()
        {
            req.tools = Some(
                functions
                    .into_iter()
                    .map(|f| Tool {
                        ty: "function".to_string(),
                        function: Some(f),
                        custom: None,
                    })
                    .collect(),
            );
        }
        if req.tool_choice.is_none()
            && let Some(fc) = req.function_call.clone()
        {
            req.tool_choice = Some(match fc {
                FunctionCallOption::Mode(mode) => ToolChoice::Mode(mode),
                FunctionCallOption::Named(named) => ToolChoice::Named(NamedToolChoice {
                    ty: "function".to_string(),
                    function: NamedFunction { name: named.name },
                }),
            });
        }

        let norm = request::normalize::apply(&req).map_err(OpenAIAdapterError::BadRequest)?;
        let tool_ctx = request::tools::extract(&req).map_err(OpenAIAdapterError::BadRequest)?;
        let prompt = request::prompt::build(&req, &tool_ctx);
        let model_res = request::resolver::resolve(
            &self.model_registry,
            &req.model,
            req.reasoning_effort.as_deref(),
            req.web_search_options.as_ref(),
        )
        .map_err(OpenAIAdapterError::BadRequest)?;

        let prompt_tokens = tiktoken_rs::cl100k_base()
            .map(|bpe| bpe.encode_with_special_tokens(&prompt).len() as u32)
            .unwrap_or(0);

        let file_result = request::files::extract(&req);
        let chat_req = crate::ds_core::ChatRequest {
            prompt,
            thinking_enabled: model_res.thinking_enabled,
            search_enabled: model_res.search_enabled || file_result.has_http_urls,
            model_type: model_res.model_type,
            files: file_result.files,
        };

        let chat_resp = self.try_chat(chat_req, request_id).await?;
        let account_id = chat_resp.account_id;

        // 为修复模型准备工具定义信息
        let tool_defs = req.tools.as_ref().map(|tools| {
            tools
                .iter()
                .filter_map(|t| t.function.as_ref())
                .map(|f| {
                    format!(
                        "- {}: {}",
                        f.name,
                        serde_json::to_string(&f.parameters).unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        });

        if req.stream {
            let repair_fn = self.create_repair_fn(request_id, tool_defs.clone());
            let s = response::stream(
                chat_resp.stream,
                req.model,
                response::StreamCfg {
                    include_usage: norm.include_usage,
                    include_obfuscation: norm.include_obfuscation,
                    stop: norm.stop,
                    prompt_tokens,
                    repair_fn: Some(repair_fn),
                    tag_config: self.tag_config.clone(),
                },
            );
            Ok(ChatResult {
                data: ChatOutput::Stream(s),
                account_id,
            })
        } else {
            let repair_fn = self.create_repair_fn(request_id, tool_defs);
            let json = response::aggregate(
                chat_resp.stream,
                req.model,
                response::StreamCfg {
                    include_usage: true,
                    include_obfuscation: false,
                    stop: norm.stop,
                    prompt_tokens,
                    repair_fn: Some(repair_fn),
                    tag_config: self.tag_config.clone(),
                },
            )
            .await?;
            Ok(ChatResult {
                data: ChatOutput::Json(json),
                account_id,
            })
        }
    }

    /// 内部辅助：对 `Overloaded` 进行指数退避重试
    pub(crate) async fn try_chat(
        &self,
        req: crate::ds_core::ChatRequest,
        request_id: &str,
    ) -> Result<crate::ds_core::ChatResponse, CoreError> {
        const MAX_RETRIES: usize = 6;
        const BASE_DELAY_MS: u64 = 1000;

        for attempt in 0..MAX_RETRIES {
            match self.ds_core.v0_chat(req.clone(), request_id).await {
                Ok(resp) => return Ok(resp),
                Err(CoreError::Overloaded) if attempt + 1 < MAX_RETRIES => {
                    let delay = BASE_DELAY_MS * (1 << attempt);
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(CoreError::Overloaded)
    }

    /// GET /v1/models
    pub fn list_models(&self) -> types::OpenAIModelList {
        models::list(
            &self.model_types,
            &self.max_input_tokens,
            &self.max_output_tokens,
        )
    }

    /// GET /v1/models/{model_id}
    pub fn get_model(&self, model_id: &str) -> Option<types::OpenAIModel> {
        models::get(
            &self.model_types,
            &self.max_input_tokens,
            &self.max_output_tokens,
            model_id,
        )
    }

    /// 原始 DeepSeek SSE 流（不经 OpenAI 协议转换）
    ///
    /// 用于流分析：对比原始响应与 OpenAI 转换后的差异，定位转换 bug
    pub async fn raw_chat_completions_stream(
        &self,
        body: &[u8],
        request_id: &str,
    ) -> Result<ChatResult<StreamResponse>, OpenAIAdapterError> {
        let chat_req: ChatCompletionsRequest = serde_json::from_slice(body)
            .map_err(|e| OpenAIAdapterError::BadRequest(format!("bad request: {}", e)))?;
        let model_res = request::resolver::resolve(
            &self.model_registry,
            &chat_req.model,
            chat_req.reasoning_effort.as_deref(),
            chat_req.web_search_options.as_ref(),
        )
        .map_err(OpenAIAdapterError::BadRequest)?;
        let ds_req = crate::ds_core::ChatRequest {
            prompt: request::prompt::build(
                &chat_req,
                &request::tools::extract(&chat_req).map_err(OpenAIAdapterError::BadRequest)?,
            ),
            thinking_enabled: model_res.thinking_enabled,
            search_enabled: model_res.search_enabled,
            model_type: model_res.model_type,
            files: vec![],
        };
        let chat_resp = self.try_chat(ds_req, request_id).await?;
        let data = Box::pin(
            chat_resp
                .stream
                .map(|r| r.map_err(OpenAIAdapterError::from)),
        );
        Ok(ChatResult {
            data,
            account_id: chat_resp.account_id,
        })
    }

    /// 获取 ds_core 账号池状态
    pub fn account_statuses(&self) -> Vec<crate::ds_core::AccountStatus> {
        self.ds_core.account_statuses()
    }

    /// 优雅关闭
    pub async fn shutdown(&self) {
        self.ds_core.shutdown().await;
    }

    /// 创建 tool_calls 修复闭包，捕获 Arc<DeepSeekCore> 发起修复请求
    pub(crate) fn create_repair_fn(
        &self,
        request_id: &str,
        tool_defs: Option<String>,
    ) -> response::RepairFn {
        use std::sync::atomic::{AtomicU16, Ordering};
        let core = self.ds_core.clone();
        let req_id = request_id.to_string();
        let seq = Arc::new(AtomicU16::new(0));
        let tag_config = self.tag_config.clone();
        let tools_info = tool_defs.unwrap_or_default();
        Arc::new(move |tool_text: String| {
            let core = core.clone();
            let req_id = req_id.clone();
            let seq = seq.clone();
            let tag_config = tag_config.clone();
            let tools_info = tools_info.clone();
            Box::pin(async move {
                use crate::ds_core::ChatRequest;
                let n = seq.fetch_add(1, Ordering::Relaxed);
                let repair_req_id = format!("{}-repair-{}", req_id, n);
                let mut prompt = String::new();
                if !tools_info.is_empty() {
                    prompt.push_str(&format!("可用的工具定义：\n{}\n\n", tools_info));
                }
                prompt.push_str(&format!(
                    "请将以下代码块中的内容提取并转换为合法的工具调用 JSON 数组。\
                     \n每个元素必须包含 \"name\"（字符串）和 \"arguments\"（对象）字段。\
                     \n只输出 JSON 数组本身，不要加 code fence，不要其他文字解释。\
                     \n注意：字符串值中的引号和换行符必须用反斜杠转义（如 \\\" 和 \\n）。\
                     \n\n需要修复的内容：\n~~~\n{tool_text}\n~~~"
                ));
                let req = ChatRequest {
                    prompt,
                    thinking_enabled: false,
                    search_enabled: false,
                    model_type: "default".to_string(),
                    files: vec![],
                };
                log::debug!(
                    target: "adapter",
                    "{} 发起修复请求: len={}", repair_req_id, tool_text.len()
                );
                let resp = core
                    .v0_chat(req, &repair_req_id)
                    .await
                    .map_err(OpenAIAdapterError::from)?;
                response::execute_tool_repair(resp.stream, &tag_config).await
            })
        })
    }
}

/// 适配器错误类型
#[derive(Debug, thiserror::Error)]
pub enum OpenAIAdapterError {
    /// 请求格式错误
    #[error("bad request: {0}")]
    BadRequest(String),

    /// 服务过载，无可用的 ds_core 账号
    #[error("service overloaded")]
    Overloaded,

    /// 上游提供商错误（网络、业务错误等）
    #[error("provider error: {0}")]
    ProviderError(String),

    /// 内部错误（序列化、流转换等）
    #[error("internal error: {0}")]
    Internal(String),

    /// tool_calls 标记解析失败，携带 `{TOOL_CALL_START}...{TOOL_CALL_END}` 内的原始文本
    #[error("tool_calls repair needed: {0}")]
    ToolCallRepairNeeded(String),
}

impl From<CoreError> for OpenAIAdapterError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::Overloaded => Self::Overloaded,
            CoreError::ProofOfWorkFailed(err) => {
                Self::Internal(format!("proof of work failed: {}", err))
            }
            CoreError::ProviderError(msg) => Self::ProviderError(msg),
            CoreError::Stream(msg) => Self::Internal(msg),
        }
    }
}

impl From<serde_json::Error> for OpenAIAdapterError {
    fn from(e: serde_json::Error) -> Self {
        Self::Internal(format!("json serialization failed: {}", e))
    }
}

impl OpenAIAdapterError {
    /// 返回对应 HTTP 状态码
    pub fn status_code(&self) -> u16 {
        match self {
            Self::BadRequest(_) => 400,
            Self::Overloaded => 429,
            Self::ProviderError(_) => 502,
            Self::Internal(_) => 500,
            Self::ToolCallRepairNeeded(_) => 500,
        }
    }
}
