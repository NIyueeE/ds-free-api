//! HTTP 路由处理器 —— 薄路由层，委托给 OpenAIAdapter / AnthropicCompat
//!
//! 所有业务逻辑在 adapter 中，handler 只做参数提取和响应格式化。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::{
    body::Body,
    extract::{Path, State, FromRequestParts},
    http::{StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::Stream;
use pin_project_lite::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::anthropic_compat::{
    AnthropicCompat, AnthropicCompatError, AnthropicOutput, MessagesRequest,
};
use crate::config::Config;
use crate::openai_adapter::{
    ChatCompletionsRequest, ChatOutput, OpenAIAdapter, OpenAIAdapterError,
};

use super::auth::LoginLimiter;
use super::error::ServerError;
use super::stats::Stats;
use super::store::StoreManager;
use super::stream::SseBody;

/// Extract the API key from request extensions (injected by api_key_middleware)
pub(crate) struct ApiKey(pub(crate) Option<String>);

impl<S> FromRequestParts<S> for ApiKey
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let key = parts.extensions.get::<super::ApiKeyExt>().map(|e| e.0.clone());
        Ok(ApiKey(key))
    }
}

/// Guard that records token usage to Stats on Drop
struct TokenGuard {
    stats: Arc<Stats>,
    prompt_tokens: u64,
    completion_tokens: Arc<std::sync::atomic::AtomicU64>,
    model: String,
    api_key: Option<String>,
    request_id: String,
    latency_ms: u64,
    success: bool,
}

impl Drop for TokenGuard {
    fn drop(&mut self) {
        let ct = self.completion_tokens.load(std::sync::atomic::Ordering::Relaxed);
        self.stats.record_tokens_for_model_and_key(
            &self.model,
            self.api_key.as_deref(),
            self.prompt_tokens,
            ct,
        );
        // Append request log asynchronously
        let stats = self.stats.clone();
        let log = super::stats::RequestLog {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            request_id: self.request_id.clone(),
            model: self.model.clone(),
            api_key: self.api_key.as_deref().map(|k| {
                if k.len() > 8 { format!("{}***", &k[..8]) } else { "***".to_string() }
            }).unwrap_or_default(),
            prompt_tokens: self.prompt_tokens,
            completion_tokens: ct,
            latency_ms: self.latency_ms,
            success: self.success,
        };
        tokio::spawn(async move {
            stats.append_log(log);
        });
    }
}

pin_project! {
    /// Stream wrapper that holds a TokenGuard; guard fires on Drop (stream end)
    struct TokenGuardStream<S> {
        #[pin]
        inner: S,
        _guard: TokenGuard,
    }
}

impl<S, E> Stream for TokenGuardStream<S>
where
    S: Stream<Item = Result<Bytes, E>>,
{
    type Item = Result<Bytes, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().inner.poll_next(cx)
    }
}

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_request_id() -> String {
    format!("req-{:x}", REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed))
}

const X_DS_ACCOUNT: &str = "x-ds-account";

/// 脱敏账号 ID：邮箱/手机号只保留前 3 字符 + ***
fn mask_account_id(id: &str) -> String {
    if id.len() <= 3 {
        "***".to_string()
    } else {
        format!("{}***", &id[..3])
    }
}

/// 应用状态
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) adapter: Arc<OpenAIAdapter>,
    pub(crate) anthropic_compat: Arc<AnthropicCompat>,
    pub(crate) stats: Arc<Stats>,
    pub(crate) config: Arc<Config>,
    pub(crate) store: Arc<StoreManager>,
    pub(crate) login_limiter: Arc<LoginLimiter>,
}

/// POST /v1/chat/completions
pub(crate) async fn chat_completions(
    State(state): State<AppState>,
    ApiKey(api_key): ApiKey,
    body: Bytes,
) -> Result<Response, ServerError> {
    let request_id = next_request_id();
    let timer = super::stats::RequestTimer::new(&state.stats);
    let timer_start = std::time::Instant::now();
    let req: ChatCompletionsRequest = serde_json::from_slice(&body)
        .map_err(|e| OpenAIAdapterError::BadRequest(format!("bad request: {}", e)))?;
    log::debug!(target: "http::request", "req={} POST /v1/chat/completions stream={}", request_id, req.stream);
    let model = req.model.clone();

    let result = state.adapter.chat_completions(req, &request_id).await;
    match &result {
        Ok(_) => timer.mark_success(),
        Err(_) => timer.mark_failure(),
    };
    let result = result?;
    match result.data {
        ChatOutput::Stream(stream) => {
            let prompt_tokens = result.prompt_tokens as u64;
            let stats = state.stats.clone();
            let model_for_cb = model.clone();
            let key_for_cb = api_key.clone();
            let rid_for_cb = request_id.clone();
            let latency_ms = timer_start.elapsed().as_millis() as u64;
            let sse = crate::openai_adapter::response::sse_stream_with_callback(
                stream,
                Box::new(move |completion_tokens: u64| {
                    stats.record_tokens_for_model_and_key(&model_for_cb, key_for_cb.as_deref(), prompt_tokens, completion_tokens);
                    // Append request log
                    let log = super::stats::RequestLog {
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        request_id: rid_for_cb,
                        model: model_for_cb.clone(),
                        api_key: key_for_cb.as_deref().map(|k| {
                            if k.len() > 8 { format!("{}***", &k[..8]) } else { "***".to_string() }
                        }).unwrap_or_default(),
                        prompt_tokens,
                        completion_tokens,
                        latency_ms,
                        success: true,
                    };
                    let stats_c = stats.clone();
                    tokio::spawn(async move { stats_c.append_log(log); });
                }),
            );
            log::debug!(target: "http::response", "req={} 200 SSE stream started", request_id);
            Ok(SseBody::new(sse)
                .with_header(X_DS_ACCOUNT, &mask_account_id(&result.account_id))
                .into_response())
        }
        ChatOutput::Json(json) => {
            let pt = result.prompt_tokens as u64;
            let ct = json.usage.as_ref().map(|u| u.completion_tokens as u64).unwrap_or(0);
            state.stats.record_tokens_for_model_and_key(&model, api_key.as_deref(), pt, ct);
            // Append request log
            let log = super::stats::RequestLog {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                request_id: request_id.clone(),
                model: model.clone(),
                api_key: api_key.as_deref().map(|k| {
                    if k.len() > 8 { format!("{}***", &k[..8]) } else { "***".to_string() }
                }).unwrap_or_default(),
                prompt_tokens: pt,
                completion_tokens: ct,
                latency_ms: timer_start.elapsed().as_millis() as u64,
                success: true,
            };
            let stats = state.stats.clone();
            tokio::spawn(async move { stats.append_log(log); });
            let bytes = serde_json::to_vec(&json).unwrap();
            log::debug!(target: "http::response", "req={} 200 JSON response {} bytes", request_id, bytes.len());
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .header(X_DS_ACCOUNT, &mask_account_id(&result.account_id))
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
    ApiKey(api_key): ApiKey,
    body: Bytes,
) -> Result<Response, ServerError> {
    let request_id = next_request_id();
    let timer = super::stats::RequestTimer::new(&state.stats);
    let timer_start = std::time::Instant::now();

    let req: MessagesRequest = serde_json::from_slice(&body)
        .map_err(|e| AnthropicCompatError::BadRequest(format!("bad request: {}", e)))?;
    log::debug!(target: "http::request", "req={} POST /anthropic/v1/messages stream={}", request_id, req.stream);
    let model = req.model.clone();

    let result = state.anthropic_compat.messages(req, &request_id).await;
    match &result {
        Ok(_) => timer.mark_success(),
        Err(_) => timer.mark_failure(),
    };
    let result = result?;
    match result.data {
        AnthropicOutput::Stream(stream) => {
            let prompt_tokens = result.prompt_tokens as u64;
            let stats = state.stats.clone();
            let completion_tokens = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
            let ct_ref = completion_tokens.clone();
            use futures::StreamExt;
            let sse = stream.inspect(move |chunk| {
                if let Ok(c) = chunk {
                    if let Some(ot) = c.output_tokens() {
                        ct_ref.fetch_add(ot as u64, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }).map(|chunk| match chunk {
                Ok(c) => c
                    .to_sse_bytes()
                    .map_err(|e| AnthropicCompatError::Internal(e.to_string())),
                Err(e) => Err(e),
            });
            // Attach guard as a stream wrapper so it drops when the stream is consumed/dropped
            let latency = timer_start.elapsed().as_millis() as u64;
            let guarded = TokenGuardStream {
                inner: sse,
                _guard: TokenGuard { stats: stats.clone(), prompt_tokens, completion_tokens, model: model.clone(), api_key: api_key.clone(), request_id: request_id.clone(), latency_ms: latency, success: true },
            };
            log::debug!(target: "http::response", "req={} 200 SSE stream started", request_id);
            Ok(SseBody::new(guarded)
                .with_header(X_DS_ACCOUNT, &mask_account_id(&result.account_id))
                .into_response())
        }
        AnthropicOutput::Json(json) => {
            let pt = result.prompt_tokens as u64;
            let ct = json.usage.output_tokens as u64;
            state.stats.record_tokens_for_model_and_key(&model, api_key.as_deref(), pt, ct);
            // Append request log
            let log = super::stats::RequestLog {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                request_id: request_id.clone(),
                model: model.clone(),
                api_key: api_key.as_deref().map(|k| {
                    if k.len() > 8 { format!("{}***", &k[..8]) } else { "***".to_string() }
                }).unwrap_or_default(),
                prompt_tokens: pt,
                completion_tokens: ct,
                latency_ms: timer_start.elapsed().as_millis() as u64,
                success: true,
            };
            let stats = state.stats.clone();
            tokio::spawn(async move { stats.append_log(log); });
            let bytes = serde_json::to_vec(&json).unwrap();
            log::debug!(target: "http::response", "req={} 200 JSON response {} bytes", request_id, bytes.len());
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .header(X_DS_ACCOUNT, &mask_account_id(&result.account_id))
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
