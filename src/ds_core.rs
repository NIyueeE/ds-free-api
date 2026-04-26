//! DeepSeek 核心模块 —— OpenAI API 到 DeepSeek 的适配层
//!
//! 对外暴露最小接口：DeepSeekCore, CoreError, ChatRequest

mod accounts;
mod client;
mod completions;
mod pow;

pub use accounts::AccountStatus;
pub use completions::{ChatRequest, ChatResponse};

use crate::config::Config;
use accounts::AccountPool;
use client::{ClientError, DsClient};
use pow::{PowError, PowSolver};

/// 内核层错误类型
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// 服务过载：所有账号都在忙或不健康
    #[error("no available account")]
    Overloaded,

    /// PoW 计算失败
    #[error("proof of work failed: {0}")]
    ProofOfWorkFailed(#[from] PowError),

    /// 提供商错误：网络、业务错误、Token 失效等
    #[error("provider: {0}")]
    ProviderError(String),

    /// 流处理错误：连接中断等
    #[error("stream error: {0}")]
    Stream(String),
}

impl From<ClientError> for CoreError {
    fn from(e: ClientError) -> Self {
        CoreError::ProviderError(e.to_string())
    }
}

pub struct DeepSeekCore {
    completions: crate::ds_core::completions::Completions,
}

impl DeepSeekCore {
    pub async fn new(config: &Config) -> Result<Self, CoreError> {
        let client = DsClient::new(
            config.deepseek.api_base.clone(),
            config.deepseek.wasm_url.clone(),
            config.deepseek.user_agent.clone(),
            config.deepseek.client_version.clone(),
            config.deepseek.client_platform.clone(),
        );

        let wasm_bytes = client.get_wasm().await?;
        let solver = PowSolver::new(&wasm_bytes)?;

        let mut pool = AccountPool::new();
        pool.init(
            config.accounts.clone(),
            config.deepseek.model_types.clone(),
            &client,
            &solver,
        )
        .await
        .map_err(|e| match e {
            accounts::PoolError::AllAccountsFailed => {
                CoreError::ProviderError("所有账号初始化失败".to_string())
            }
            accounts::PoolError::Client(e) => CoreError::ProviderError(e.to_string()),
            accounts::PoolError::Pow(e) => CoreError::ProofOfWorkFailed(e),
            accounts::PoolError::Validation(msg) => {
                CoreError::ProviderError(format!("配置错误: {}", msg))
            }
        })?;

        let completions = crate::ds_core::completions::Completions::new(client, solver, pool);

        Ok(Self { completions })
    }

    /// 发起对话请求，返回 SSE 字节流 + 账号标识
    ///
    /// 流结束或丢弃时自动释放账号
    pub async fn v0_chat(
        &self,
        req: ChatRequest,
        request_id: &str,
    ) -> Result<ChatResponse, CoreError> {
        self.completions.v0_chat(req, request_id).await
    }

    pub fn account_statuses(&self) -> Vec<AccountStatus> {
        self.completions.account_statuses()
    }

    /// 优雅关闭：清理所有账号的 session
    pub async fn shutdown(&self) {
        self.completions.shutdown().await;
    }
}
