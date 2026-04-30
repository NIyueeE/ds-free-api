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

use crate::anthropic_compat::{
    AnthropicCompat, AnthropicCompatError, AnthropicOutput, MessagesRequest,
};
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
        ChatOutput::Stream(stream) => {
            let sse = crate::openai_adapter::response::sse_stream(stream);
            log::debug!(target: "http::response", "req={} 200 SSE stream started", request_id);
            Ok(SseBody::new(sse)
                .with_header(X_DS_ACCOUNT, &result.account_id)
                .into_response())
        }
        ChatOutput::Json(json) => {
            let bytes = serde_json::to_vec(&json).unwrap();
            log::debug!(target: "http::response", "req={} 200 JSON response {} bytes", request_id, bytes.len());
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .header(X_DS_ACCOUNT, &result.account_id)
                .body(Body::from(bytes))
                .unwrap()
                .into_response())
        }
    }
}

/// GET /v1/models
pub(crate) async fn list_models(State(state): State<AppState>) -> Response {
    log::debug!(target: "http::request", "GET /v1/models");
    let bytes = serde_json::to_vec(&state.adapter.list_models()).unwrap();
    log::debug!(target: "http::response", "200 JSON response {} bytes", bytes.len());
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(bytes),
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
        Some(model) => {
            let bytes = serde_json::to_vec(&model).unwrap();
            log::debug!(target: "http::response", "200 JSON response {} bytes", bytes.len());
            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                Body::from(bytes),
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

    let req: MessagesRequest = serde_json::from_slice(&body)
        .map_err(|e| AnthropicCompatError::BadRequest(format!("bad request: {}", e)))?;
    log::debug!(target: "http::request", "req={} POST /anthropic/v1/messages stream={}", request_id, req.stream);

    let result = state.anthropic_compat.messages(req, &request_id).await?;
    match result.data {
        AnthropicOutput::Stream(stream) => {
            use futures::StreamExt;
            let sse = stream.map(|chunk| match chunk {
                Ok(c) => c
                    .to_sse_bytes()
                    .map_err(|e| AnthropicCompatError::Internal(e.to_string())),
                Err(e) => Err(e),
            });
            log::debug!(target: "http::response", "req={} 200 SSE stream started", request_id);
            Ok(SseBody::new(sse)
                .with_header(X_DS_ACCOUNT, &result.account_id)
                .into_response())
        }
        AnthropicOutput::Json(json) => {
            let bytes = serde_json::to_vec(&json).unwrap();
            log::debug!(target: "http::response", "req={} 200 JSON response {} bytes", request_id, bytes.len());
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .header(X_DS_ACCOUNT, &result.account_id)
                .body(Body::from(bytes))
                .unwrap()
                .into_response())
        }
    }
}

/// GET /anthropic/v1/models
pub(crate) async fn anthropic_list_models(State(state): State<AppState>) -> Response {
    log::debug!(target: "http::request", "GET /anthropic/v1/models");
    let bytes = serde_json::to_vec(&state.anthropic_compat.list_models()).unwrap();
    log::debug!(target: "http::response", "200 JSON response {} bytes", bytes.len());
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(bytes),
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
        Some(model) => {
            let bytes = serde_json::to_vec(&model).unwrap();
            log::debug!(target: "http::response", "200 JSON response {} bytes", bytes.len());
            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                Body::from(bytes),
            )
                .into_response())
        }
        None => Err(ServerError::NotFound(id)),
    }
}
