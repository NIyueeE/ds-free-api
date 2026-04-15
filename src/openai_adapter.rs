//! OpenAI 协议适配层 —— OpenAI JSON 与 ds_core 内部格式的双向转换
//!
//! 本模块负责将 OpenAI 兼容的 HTTP 请求转换为 ds_core 内部格式，
//! 并将 ds_core 的响应转换为 OpenAI 兼容的 JSON 格式。
//!
//! 对外暴露最小接口：OpenAIAdapter, OpenAIAdapterError

use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;

use crate::ds_core::{CoreError, DeepSeekCore};

mod models;
pub(crate) mod request;
pub(crate) mod response;
mod types;

/// 流式响应类型
pub type StreamResponse = Pin<Box<dyn Stream<Item = Result<Bytes, OpenAIAdapterError>> + Send>>;

/// OpenAI 适配器
pub struct OpenAIAdapter {
    ds_core: DeepSeekCore,
    model_types: Vec<String>,
    model_registry: std::collections::HashMap<String, String>,
}

impl OpenAIAdapter {
    /// 创建适配器实例
    pub async fn new(config: &crate::config::Config) -> Result<Self, OpenAIAdapterError> {
        let ds_core = DeepSeekCore::new(config).await?;
        let model_registry = config.deepseek.model_registry();
        Ok(Self {
            ds_core,
            model_types: config.deepseek.model_types.clone(),
            model_registry,
        })
    }

    /// 解析请求体为 AdapterRequest（仅解析一次，避免双重 JSON 解析）
    pub(crate) fn parse_request(
        &self,
        body: &[u8],
    ) -> Result<request::AdapterRequest, OpenAIAdapterError> {
        request::parse(body, &self.model_registry)
    }

    /// POST /v1/chat/completions (非流式)
    ///
    /// 底层复用流式接口，将 SSE 流聚合为单个 JSON 对象后返回
    pub async fn chat_completions(&self, body: &[u8]) -> Result<Vec<u8>, OpenAIAdapterError> {
        let req = request::parse(body, &self.model_registry)?;
        let stream = self.try_chat(req.ds_req).await?;
        response::aggregate(stream, req.model, req.stop, req.prompt_tokens).await
    }

    /// POST /v1/chat/completions (流式)
    pub async fn chat_completions_stream(
        &self,
        body: &[u8],
    ) -> Result<StreamResponse, OpenAIAdapterError> {
        let req = request::parse(body, &self.model_registry)?;
        let stream = self.try_chat(req.ds_req).await?;
        Ok(response::stream(
            stream,
            req.model,
            req.include_usage,
            req.include_obfuscation,
            req.stop,
            req.prompt_tokens,
        ))
    }

    /// 内部辅助：对 `Overloaded` 进行短延迟轮询重试，降低瞬时并发峰值导致的失败率
    pub(crate) async fn try_chat(
        &self,
        req: crate::ds_core::ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Bytes, CoreError>> + Send>>, CoreError> {
        const MAX_RETRIES: usize = 3;
        const RETRY_DELAY_MS: u64 = 200;

        for attempt in 0..MAX_RETRIES {
            match self.ds_core.v0_chat(req.clone()).await {
                Ok(stream) => return Ok(Box::pin(stream)),
                Err(CoreError::Overloaded) if attempt + 1 < MAX_RETRIES => {
                    tokio::time::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(CoreError::Overloaded)
    }

    /// GET /v1/models
    pub fn list_models(&self) -> Vec<u8> {
        models::list(&self.model_types)
    }

    /// GET /v1/models/{model_id}
    pub fn get_model(&self, model_id: &str) -> Option<Vec<u8>> {
        models::get(&self.model_types, model_id)
    }

    /// 获取 ds_core 账号池状态
    pub fn account_statuses(&self) -> Vec<crate::ds_core::AccountStatus> {
        self.ds_core.account_statuses()
    }

    /// 优雅关闭
    pub async fn shutdown(&self) {
        self.ds_core.shutdown().await;
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
        }
    }
}
