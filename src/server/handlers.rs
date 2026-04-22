//! HTTP 路由处理器 —— 薄路由层，委托给 OpenAIAdapter / AnthropicCompat
//!
//! 所有业务逻辑在 adapter 中，handler 只做参数提取和响应格式化。

use axum::{
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use std::sync::Arc;

use crate::anthropic_compat::{AnthropicCompat, AnthropicCompatError};
use crate::openai_adapter::request::AdapterRequest;
use crate::openai_adapter::{OpenAIAdapter, OpenAIAdapterError, StreamResponse};

use super::error::ServerError;
use super::stream::SseBody;

/// 应用状态
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) adapter: Arc<OpenAIAdapter>,
    pub(crate) anthropic_compat: Arc<AnthropicCompat>,
}

/// POST /v1/chat/completions (解析一次 JSON，根据 stream 字段走不同路径)
pub(crate) async fn chat_completions(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Response, ServerError> {
    let req = state.adapter.parse_request(&body)?;
    log::debug!(target: "http::request", "POST /v1/chat/completions stream={}", req.stream);

    match handle_chat(&state.adapter, req).await {
        Ok(ChatResult::Stream(stream)) => {
            log::debug!(target: "http::response", "200 SSE stream started");
            Ok(SseBody::new(stream).into_response())
        }
        Ok(ChatResult::Json(json)) => {
            log::debug!(target: "http::response", "200 JSON response {} bytes", json.len());
            Ok((
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                Body::from(json),
            )
                .into_response())
        }
        Err(e) => Err(e.into()),
    }
}

enum ChatResult {
    Stream(StreamResponse),
    Json(Vec<u8>),
}

async fn handle_chat(
    adapter: &OpenAIAdapter,
    req: AdapterRequest,
) -> Result<ChatResult, OpenAIAdapterError> {
    let model = req.model.clone();
    let stop = req.stop.clone();
    let stream = req.stream;
    let include_usage = req.include_usage;
    let include_obfuscation = req.include_obfuscation;
    let prompt_tokens = req.prompt_tokens;

    let ds_stream = adapter.try_chat(req.ds_req).await?;

    if stream {
        log::debug!(target: "http::response", "200 SSE stream started");
        Ok(ChatResult::Stream(crate::openai_adapter::response::stream(
            ds_stream,
            model,
            include_usage,
            include_obfuscation,
            stop,
            prompt_tokens,
        )))
    } else {
        let json =
            crate::openai_adapter::response::aggregate(ds_stream, model, stop, prompt_tokens)
                .await?;
        log::debug!(target: "http::response", "200 JSON response {} bytes", json.len());
        Ok(ChatResult::Json(json))
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
    // 先解析出 stream 字段决定走流式还是非流式
    let stream = serde_json::from_slice::<crate::anthropic_compat::request::MessagesRequest>(&body)
        .map(|req| req.stream)
        .map_err(AnthropicCompatError::from)?;

    log::debug!(target: "http::request", "POST /anthropic/v1/messages stream={}", stream);

    if stream {
        let anthropic_stream = state.anthropic_compat.messages_stream(&body).await?;
        log::debug!(target: "http::response", "200 SSE stream started");
        Ok(SseBody::new(anthropic_stream).into_response())
    } else {
        let json = state.anthropic_compat.messages(&body).await?;
        log::debug!(target: "http::response", "200 JSON response {} bytes", json.len());
        Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            Body::from(json),
        )
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
