//! Anthropic 协议兼容层 —— 基于 openai_adapter 提供 Anthropic API 兼容接口
//!
//! 本模块不直接访问 ds_core，所有数据通过 openai_adapter 获取并做格式映射。
//! 请求流向：Anthropic JSON → openai_adapter 请求映射 → ds_core → 响应映射回 Anthropic 格式。

mod models;
pub(crate) mod request;
pub(crate) mod response;

/// Anthropic 流式响应类型
pub type StreamResponse = Pin<Box<dyn Stream<Item = Result<Bytes, AnthropicCompatError>> + Send>>;

use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use futures::Stream;
use log::debug;

use crate::openai_adapter::{OpenAIAdapter, OpenAIAdapterError};

/// Anthropic 兼容层
pub struct AnthropicCompat {
    openai_adapter: Arc<OpenAIAdapter>,
}

impl AnthropicCompat {
    /// 创建兼容层实例
    pub fn new(openai_adapter: Arc<OpenAIAdapter>) -> Self {
        Self { openai_adapter }
    }

    /// POST /v1/messages (非流式)
    ///
    /// 将 Anthropic 请求映射为 OpenAI 请求，获取响应后再映射回 Anthropic Message 格式。
    pub async fn messages(&self, body: &[u8]) -> Result<Vec<u8>, AnthropicCompatError> {
        debug!(target: "anthropic_compat", "收到 messages 请求");
        let openai_body = request::to_openai_request(body)?;
        let openai_json = self.openai_adapter.chat_completions(&openai_body).await?;
        response::from_chat_completion_bytes(&openai_json)
            .map_err(|e| AnthropicCompatError::Internal(format!("json error: {}", e)))
    }

    /// POST /v1/messages (流式)
    ///
    /// 将 Anthropic 请求映射为 OpenAI 请求，返回 Anthropic 格式的 SSE 字节流。
    pub async fn messages_stream(
        &self,
        body: &[u8],
    ) -> Result<StreamResponse, AnthropicCompatError> {
        debug!(target: "anthropic_compat", "收到流式 messages 请求");
        let openai_body = request::to_openai_request(body)?;
        let openai_req = self
            .openai_adapter
            .parse_request(&openai_body)
            .map_err(AnthropicCompatError::from)?;
        let input_tokens = openai_req.prompt_tokens;
        let ds_stream = self
            .openai_adapter
            .try_chat(openai_req.ds_req)
            .await
            .map_err(OpenAIAdapterError::from)?;
        let openai_stream = crate::openai_adapter::response::stream(
            ds_stream,
            openai_req.model,
            openai_req.include_usage,
            openai_req.include_obfuscation,
            openai_req.stop,
            openai_req.prompt_tokens,
        );
        Ok(response::from_chat_completion_stream(
            openai_stream,
            input_tokens,
        ))
    }

    /// GET /v1/models
    ///
    /// 返回 Anthropic 格式的模型列表。
    pub fn list_models(&self) -> Vec<u8> {
        debug!(target: "anthropic_compat", "收到模型列表请求");
        models::list(&self.openai_adapter)
    }

    /// GET /v1/models/{model_id}
    ///
    /// 返回指定模型的 Anthropic 格式详情。
    pub fn get_model(&self, model_id: &str) -> Option<Vec<u8>> {
        debug!(target: "anthropic_compat", "查询模型: {}", model_id);
        models::get(&self.openai_adapter, model_id)
    }
}

/// Anthropic 兼容层错误类型
#[derive(Debug, thiserror::Error)]
pub enum AnthropicCompatError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("service overloaded")]
    Overloaded,
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<OpenAIAdapterError> for AnthropicCompatError {
    fn from(e: OpenAIAdapterError) -> Self {
        match e {
            OpenAIAdapterError::BadRequest(msg) => Self::BadRequest(msg),
            OpenAIAdapterError::Overloaded => Self::Overloaded,
            OpenAIAdapterError::ProviderError(msg) => Self::Internal(msg),
            OpenAIAdapterError::Internal(msg) => Self::Internal(msg),
        }
    }
}

impl AnthropicCompatError {
    /// 返回对应 HTTP 状态码
    pub fn status_code(&self) -> u16 {
        match self {
            Self::BadRequest(_) => 400,
            Self::Overloaded => 429,
            Self::Internal(_) => 500,
        }
    }
}
