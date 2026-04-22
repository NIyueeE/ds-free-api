//! HTTP 服务器层 —— 薄路由壳，暴露 OpenAIAdapter 与 AnthropicCompat 为 HTTP 接口
//!
//! 本模块负责将 adapter / compat 层包装为 axum HTTP 服务。

mod error;
mod handlers;
mod stream;

use axum::{
    Router,
    extract::Request,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::sync::Arc;
use tokio::net::TcpListener;

use crate::anthropic_compat::AnthropicCompat;
use crate::config::Config;
use crate::openai_adapter::OpenAIAdapter;

use handlers::AppState;

/// 启动 HTTP 服务器
pub async fn run(config: Config) -> anyhow::Result<()> {
    let adapter = Arc::new(OpenAIAdapter::new(&config).await?);
    let anthropic_compat = Arc::new(AnthropicCompat::new(Arc::clone(&adapter)));
    let state = AppState {
        adapter,
        anthropic_compat,
    };
    let router = build_router(state.clone(), &config.server.api_tokens);

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = TcpListener::bind(&addr).await?;
    log::info!(target: "http::server", "服务器启动: {}", addr);

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    log::info!(target: "http::server", "HTTP 服务已停止，正在清理资源");
    state.adapter.shutdown().await;
    log::info!(target: "http::server", "清理完成");

    Ok(())
}

/// 构建路由器
fn build_router(state: AppState, api_tokens: &[crate::config::ApiToken]) -> Router {
    let has_auth = !api_tokens.is_empty();
    let tokens: Vec<String> = api_tokens.iter().map(|t| t.token.clone()).collect();

    let mut router = Router::new()
        .route("/", get(|| async { "ai-free-api" }))
        // OpenAI
        .route("/v1/chat/completions", post(handlers::chat_completions))
        .route("/v1/models", get(handlers::list_models))
        .route("/v1/models/{id}", get(handlers::get_model))
        // Anthropic
        .route("/anthropic/v1/messages", post(handlers::anthropic_messages))
        .route("/anthropic/v1/models", get(handlers::anthropic_list_models))
        .route(
            "/anthropic/v1/models/{id}",
            get(handlers::anthropic_get_model),
        )
        .with_state(state);

    if has_auth {
        router = router.layer(middleware::from_fn(move |req, next| {
            let tokens = tokens.clone();
            async move { auth_middleware(req, next, tokens).await }
        }));
    }

    router
}

/// API Token 鉴权中间件
async fn auth_middleware(req: Request, next: Next, tokens: Vec<String>) -> Response {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let valid = match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let token = header.strip_prefix("Bearer ").unwrap_or("");
            tokens.iter().any(|t| t == token)
        }
        _ => false,
    };

    if !valid {
        log::debug!(target: "http::response", "401 unauthorized request");
        return error::ServerError::Unauthorized.into_response();
    }

    next.run(req).await
}

/// 优雅关闭信号
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    log::info!(target: "http::server", "收到关闭信号，开始优雅关闭");
}
