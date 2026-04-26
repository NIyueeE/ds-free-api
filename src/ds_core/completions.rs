//! 对话请求编排 —— 调用 edit_message 返回 SSE 流

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::Stream;
use pin_project_lite::pin_project;

use crate::ds_core::CoreError;
use crate::ds_core::accounts::{AccountGuard, AccountPool};
use crate::ds_core::client::{DsClient, EditMessagePayload};
use crate::ds_core::pow::PowSolver;

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub prompt: String,
    pub thinking_enabled: bool,
    pub search_enabled: bool,
    pub model_type: String,
}

pin_project! {
    pub struct GuardedStream<S> {
        #[pin]
        stream: S,
        _guard: AccountGuard,
    }
}

impl<S> GuardedStream<S> {
    pub fn new(stream: S, guard: AccountGuard) -> Self {
        Self {
            stream,
            _guard: guard,
        }
    }
}

impl<S, E> Stream for GuardedStream<S>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::fmt::Display,
{
    type Item = Result<Bytes, CoreError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.project().stream.poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(bytes))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(CoreError::Stream(e.to_string())))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

/// v0_chat 的返回结果，包含 SSE 流和使用的账号标识
pub struct ChatResponse {
    pub stream: Pin<Box<dyn Stream<Item = Result<Bytes, CoreError>> + Send>>,
    /// 使用的账号标识（优先 email，否则 mobile）
    pub account_id: String,
}

pub struct Completions {
    client: DsClient,
    solver: PowSolver,
    pool: AccountPool,
}

impl Completions {
    pub fn new(client: DsClient, solver: PowSolver, pool: AccountPool) -> Self {
        Self {
            client,
            solver,
            pool,
        }
    }

    pub async fn v0_chat(
        &self,
        req: ChatRequest,
    ) -> Result<ChatResponse, CoreError> {
        let guard = self
            .pool
            .get_account(&req.model_type)
            .ok_or(CoreError::Overloaded)?;

        let account = guard.account();
        let token = account.token().to_string();
        let account_id = account.display_id().to_string();
        let session_id = account
            .session_id(&req.model_type)
            .expect("初始化时已保证存在该 model_type 的 session")
            .to_string();

        let pow_header = self.compute_pow(&token).await?;

        let payload = EditMessagePayload {
            chat_session_id: session_id,
            message_id: 1,
            prompt: req.prompt,
            search_enabled: req.search_enabled,
            thinking_enabled: req.thinking_enabled,
            model_type: req.model_type.clone(),
        };

        let stream = self
            .client
            .edit_message(&token, &pow_header, &payload)
            .await?;

        Ok(ChatResponse {
            stream: Box::pin(GuardedStream::new(stream, guard)),
            account_id,
        })
    }

    async fn compute_pow(&self, token: &str) -> Result<String, CoreError> {
        let challenge_data = self.client.create_pow_challenge(token).await?;
        let result = self.solver.solve(&challenge_data)?;
        Ok(result.to_header())
    }

    pub fn account_statuses(&self) -> Vec<crate::ds_core::accounts::AccountStatus> {
        self.pool.account_statuses()
    }

    /// 优雅关闭：清理所有账号的 session
    pub async fn shutdown(&self) {
        self.pool.shutdown(&self.client).await;
    }
}
