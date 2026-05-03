//! HTTP 服务器层 —— 薄路由壳，暴露 OpenAIAdapter 与 AnthropicCompat 为 HTTP 接口
//!
//! 本模块负责将 adapter / compat 层包装为 axum HTTP 服务。

mod admin;
mod auth;
mod error;
mod handlers;
pub mod runtime_log;
mod stats;
mod store;
mod stream;

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::Request,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;

use crate::anthropic_compat::AnthropicCompat;
use crate::config::Config;
use crate::openai_adapter::OpenAIAdapter;

use handlers::AppState;

/// Extension to carry the API key through the request
#[derive(Clone)]
pub(crate) struct ApiKeyExt(pub(crate) String);

/// 启动 HTTP 服务器
pub async fn run(config: Config) -> anyhow::Result<()> {
    let adapter = Arc::new(OpenAIAdapter::new(&config).await?);
    let anthropic_compat = Arc::new(AnthropicCompat::new(Arc::clone(&adapter)));
    let data_dir = std::env::var("DS_DATA_DIR").unwrap_or_else(|_| ".".to_string());
    let store = Arc::new(store::StoreManager::new(std::path::Path::new(&data_dir)));
    let stats = Arc::new(stats::Stats::new_with_store(Some(store.clone())));
    let login_limiter = Arc::new(auth::LoginLimiter::new());
    let state = AppState {
        adapter: adapter.clone(),
        anthropic_compat,
        stats: stats.clone(),
        config: Arc::new(config.clone()),
        store: store.clone(),
        login_limiter: login_limiter.clone(),
    };
    let router = build_router(state.clone());

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = TcpListener::bind(&addr).await?;
    log::info!(target: "http::server", "openai兼容base_url: http://{}", addr);
    log::info!(target: "http::server", "anthropic兼容base_url: http://{}/anthropic", addr);
    log::info!(target: "http::server", "管理面板: http://{}/admin", addr);

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    log::info!(target: "http::server", "HTTP 服务已停止，正在清理资源");
    stats.persist_now();
    state.adapter.shutdown().await;
    log::info!(target: "http::server", "清理完成");

    Ok(())
}

/// 构建路由器
fn build_router(state: AppState) -> Router {
    let store = state.store.clone();
    let cors_origins = state.config.server.cors_origins.clone();

    let public = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        // Admin auth (no JWT required)
        .route("/admin/api/setup", post(admin::admin_setup))
        .route("/admin/api/login", post(admin::admin_login));

    // API routes: Bearer token from api_keys.json
    let api_routes = Router::new()
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
        .layer(middleware::from_fn(move |req, next| {
            let store = store.clone();
            async move { api_key_middleware(req, next, store).await }
        }));

    // Admin routes: JWT auth
    let admin_store = state.store.clone();
    let admin_routes = Router::new()
        .route("/admin/api/status", get(admin::admin_status))
        .route("/admin/api/stats", get(admin::admin_stats))
        .route("/admin/api/models", get(admin::admin_models))
        .route("/admin/api/config", get(admin::admin_config))
        // API Key management
        .route("/admin/api/keys", get(admin::admin_list_keys))
        .route("/admin/api/keys", post(admin::admin_create_key))
        .route("/admin/api/keys/{key}", axum::routing::delete(admin::admin_delete_key))
        // Account management
        .route("/admin/api/accounts", post(admin::admin_add_account))
        .route("/admin/api/accounts/{id}", axum::routing::delete(admin::admin_remove_account))
        .route("/admin/api/accounts/{id}/relogin", post(admin::admin_relogin_account))
        // Config hot-reload
        .route("/admin/api/reload", post(admin::admin_reload_config))
        // Request logs
        .route("/admin/api/logs", get(admin::admin_logs))
        // Runtime logs
        .route("/admin/api/runtime-logs", get(admin::admin_runtime_logs))
        .layer(middleware::from_fn(move |req, next| {
            let store = admin_store.clone();
            async move { jwt_middleware(req, next, store).await }
        }));

    let router = public.merge(api_routes).merge(admin_routes);

    // 静态文件服务：/admin → web/dist/
    // 优先从文件系统读取（开发模式），回退到编译时嵌入的资源（release 二进制）
    let web_dist = std::path::Path::new("web/dist");
    let router = if web_dist.exists() {
        router.nest_service(
            "/admin",
            tower_http::services::ServeDir::new(web_dist)
                .fallback(tower_http::services::ServeFile::new("web/dist/index.html")),
        )
    } else {
        // 编译时嵌入：API 路由优先，其余走 SPA fallback
        router
            .route("/admin/assets/{*path}", get(serve_embedded_asset))
            .route("/admin/favicon.svg", get(serve_embedded_asset))
            .route("/admin/{*path}", get(serve_embedded_spa))
    };

    router.with_state(state).layer(build_cors_layer(&cors_origins))
}

fn build_cors_layer(origins: &[String]) -> CorsLayer {
    use axum::http::Method;
    use axum::http::header;

    if origins.len() == 1 && origins[0] == "*" {
        return CorsLayer::permissive();
    }

    let allowed: Vec<axum::http::HeaderValue> = origins
        .iter()
        .filter_map(|o| o.parse::<axum::http::HeaderValue>().ok())
        .collect();

    if allowed.is_empty() {
        return CorsLayer::permissive();
    }

    CorsLayer::new()
        .allow_origin(allowed)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            axum::http::HeaderName::from_static("x-request-id"),
        ])
}

/// 编译时嵌入 web/dist/ 目录，release 二进制无需额外文件即可提供管理面板
#[derive(rust_embed::Embed)]
#[folder = "web/dist/"]
struct WebAssets;

/// 提供嵌入的静态资源（JS/CSS/SVG 等）
async fn serve_embedded_asset(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    use axum::http::{header, StatusCode};

    let file = WebAssets::get(&path);
    match file {
        Some(content) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data,
            )
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// SPA fallback: 所有非资源路径返回 index.html
async fn serve_embedded_spa() -> Response {
    use axum::http::{header, StatusCode};

    match WebAssets::get("index.html") {
        Some(content) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            content.data,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Serialize)]
struct RootResponse {
    endpoints: Vec<String>,
    message: &'static str,
}

async fn root() -> Json<RootResponse> {
    Json(RootResponse {
        endpoints: vec![
            "GET /".into(),
            "GET /health".into(),
            "POST /v1/chat/completions".into(),
            "GET /v1/models".into(),
            "GET /v1/models/{id}".into(),
            "POST /anthropic/v1/messages".into(),
            "GET /anthropic/v1/models".into(),
            "GET /anthropic/v1/models/{id}".into(),
            "GET /admin/api/status".into(),
            "GET /admin/api/stats".into(),
            "GET /admin/api/models".into(),
            "GET /admin/api/config".into(),
        ],
        message: "https://github.com/NIyueeE/ds-free-api",
    })
}

/// Health check endpoint
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok"
    }))
}

/// API Key 鉴权中间件（从 api_keys.json 校验 Bearer token）
async fn api_key_middleware(
    req: Request,
    next: Next,
    store: Arc<store::StoreManager>,
) -> Response {
    let token = extract_bearer_token(&req);
    let valid = match token {
        Some(t) => store.is_valid_api_key(t).await,
        None => false,
    };

    if !valid {
        log::debug!(target: "http::response", "401 unauthorized API request");
        return error::ServerError::Unauthorized.into_response();
    }

    // Inject the API key into request extensions for downstream handlers
    let key_ext = token.map(|t| ApiKeyExt(t.to_string()));
    let mut req = req;
    if let Some(ext) = key_ext {
        req.extensions_mut().insert(ext);
    }

    next.run(req).await
}

/// JWT 鉴权中间件（管理面板路由）
async fn jwt_middleware(
    req: Request,
    next: Next,
    store: Arc<store::StoreManager>,
) -> Response {
    let token = extract_bearer_token(&req);
    let valid = match token {
        Some(t) => auth::verify_jwt(&store, t).await,
        None => false,
    };

    if !valid {
        log::debug!(target: "http::response", "401 unauthorized admin request");
        return error::ServerError::Unauthorized.into_response();
    }

    next.run(req).await
}

/// 从 Authorization 头提取 Bearer token
fn extract_bearer_token(req: &Request) -> Option<&str> {
    req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
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
