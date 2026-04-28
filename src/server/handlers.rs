//! HTTP 路由处理器 —— 薄路由层，委托给 OpenAIAdapter / AnthropicCompat
//!
//! 所有业务逻辑在 adapter 中，handler 只做参数提取和响应格式化。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::{
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;

use crate::anthropic_compat::{AnthropicCompat, AnthropicCompatError};
use crate::openai_adapter::{
    ChatCompletionsRequest, ChatOutput, OpenAIAdapter, OpenAIAdapterError,
};

use super::error::ServerError;
use super::stream::SseBody;

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_request_id() -> String {
    format!("req-{:x}", REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed))
}

const X_DS_ACCOUNT: &str = "x-ds-account";

/// 应用状态
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) adapter: Arc<OpenAIAdapter>,
    pub(crate) anthropic_compat: Arc<AnthropicCompat>,
}

/// POST /v1/chat/completions
pub(crate) async fn chat_completions(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Response, ServerError> {
    let request_id = next_request_id();
    let req: ChatCompletionsRequest = serde_json::from_slice(&body)
        .map_err(|e| OpenAIAdapterError::BadRequest(format!("bad request: {}", e)))?;
    log::debug!(target: "http::request", "req={} POST /v1/chat/completions stream={}", request_id, req.stream);

    let result = state.adapter.chat_completions(req, &request_id).await?;
    match result.data {
        ChatOutput::Stream { stream, .. } => {
            log::debug!(target: "http::response", "req={} 200 SSE stream started", request_id);
            Ok(SseBody::new(stream)
                .with_header(X_DS_ACCOUNT, &result.account_id)
                .into_response())
        }
        ChatOutput::Json(json) => {
            log::debug!(target: "http::response", "req={} 200 JSON response {} bytes", request_id, json.len());
            let body = Body::from(json);
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .header(X_DS_ACCOUNT, &result.account_id)
                .body(body)
                .unwrap()
                .into_response())
        }
    }
}

/// GET /v1/models
pub(crate) async fn list_models(State(state): State<AppState>) -> Response {
    log::debug!(target: "http::request", "GET /v1/models");
    let json = state.adapter.list_models();
    log::debug!(target: "http::response", "200 JSON response {} bytes", json.len());
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(json),
    )
        .into_response()
}

/// GET /v1/models/{id}
pub(crate) async fn get_model(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, ServerError> {
    log::debug!(target: "http::request", "GET /v1/models/{}", id);

    match state.adapter.get_model(&id) {
        Some(json) => {
            log::debug!(target: "http::response", "200 JSON response {} bytes", json.len());
            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                Body::from(json),
            )
                .into_response())
        }
        None => Err(ServerError::NotFound(id)),
    }
}

// ============================================================================
// Anthropic 兼容路由
// ============================================================================

/// POST /anthropic/v1/messages
pub(crate) async fn anthropic_messages(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Response, ServerError> {
    let request_id = next_request_id();
    log::debug!(target: "http::request", "req={} anthropic body: {}", request_id, String::from_utf8_lossy(&body));
    // 先解析出 stream 字段决定走流式还是非流式
    let stream = serde_json::from_slice::<crate::anthropic_compat::request::MessagesRequest>(&body)
        .map(|req| req.stream)
        .map_err(AnthropicCompatError::from)?;

    log::debug!(target: "http::request", "req={} POST /anthropic/v1/messages stream={}", request_id, stream);

    if stream {
        let result = state
            .anthropic_compat
            .messages_stream(&body, &request_id)
            .await?;
        log::debug!(target: "http::response", "req={} 200 SSE stream started", request_id);
        Ok(SseBody::new(result.data)
            .with_header(X_DS_ACCOUNT, &result.account_id)
            .into_response())
    } else {
        let result = state.anthropic_compat.messages(&body, &request_id).await?;
        log::debug!(target: "http::response", "req={} 200 JSON response {} bytes", request_id, result.data.len());
        let body = Body::from(result.data);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .header(X_DS_ACCOUNT, &result.account_id)
            .body(body)
            .unwrap()
            .into_response())
    }
}

/// GET /anthropic/v1/models
pub(crate) async fn anthropic_list_models(State(state): State<AppState>) -> Response {
    log::debug!(target: "http::request", "GET /anthropic/v1/models");
    let json = state.anthropic_compat.list_models();
    log::debug!(target: "http::response", "200 JSON response {} bytes", json.len());
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(json),
    )
        .into_response()
}

/// GET /anthropic/v1/models/{id}
pub(crate) async fn anthropic_get_model(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, ServerError> {
    log::debug!(target: "http::request", "GET /anthropic/v1/models/{}", id);

    match state.anthropic_compat.get_model(&id) {
        Some(json) => {
            log::debug!(target: "http::response", "200 JSON response {} bytes", json.len());
            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                Body::from(json),
            )
                .into_response())
        }
        None => Err(ServerError::NotFound(id)),
    }
}

impl From<serde_json::Error> for AnthropicCompatError {
    fn from(e: serde_json::Error) -> Self {
        Self::BadRequest(format!("bad request: {}", e))
    }
}
