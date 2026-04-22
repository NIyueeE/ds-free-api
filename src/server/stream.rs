//! SSE 流桥接 —— 泛型 Stream 转 axum Body
//!
//! 支持 OpenAI 与 Anthropic 两种流式响应。

use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures::Stream;
use futures::StreamExt;

/// SSE 响应体包装器（泛型）
pub struct SseBody<S> {
    inner: S,
}

impl<S, E> SseBody<S>
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::fmt::Display + Send + Sync + 'static,
{
    pub fn new(stream: S) -> Self {
        Self { inner: stream }
    }
}

impl<S, E> IntoResponse for SseBody<S>
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::fmt::Display + Send + Sync + 'static,
{
    fn into_response(self) -> Response {
        let body = Body::from_stream(self.inner.map(|result| {
            result.map_err(|e| {
                log::error!(target: "http::response", "SSE stream error: {}", e);
                std::io::Error::other(e.to_string())
            })
        }));

        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/event-stream"),
                (header::CACHE_CONTROL, "no-cache"),
                (header::CONNECTION, "keep-alive"),
            ],
            body,
        )
            .into_response()
    }
}
