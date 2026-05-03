//! 管理 API 路由处理器 —— 登录/设置密码、账号池状态、请求统计、模型列表、配置查看、API Key 管理

use axum::{
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::Response,
    body::Body,
};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use super::handlers::AppState;

// ── 请求/响应类型 ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SetupRequest {
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
}

#[derive(Deserialize)]
pub struct CreateKeyRequest {
    pub description: String,
}

#[derive(Deserialize)]
pub struct AddAccountRequest {
    pub email: String,
    pub mobile: String,
    pub area_code: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct CreateKeyResponse {
    pub key: String,
}

#[derive(Serialize)]
pub struct AdminStatusResponse {
    pub accounts: Vec<crate::ds_core::AccountStatus>,
    pub total: usize,
    pub idle: usize,
    pub busy: usize,
    pub error: usize,
    pub invalid: usize,
}

#[derive(Serialize)]
pub struct AdminStatsResponse {
    #[serde(flatten)]
    pub stats: super::stats::StatsSnapshot,
}

#[derive(Serialize)]
pub struct AdminConfigResponse {
    pub server: ServerConfigView,
    pub deepseek: DeepSeekConfigView,
    pub accounts: Vec<AccountView>,
}

#[derive(Serialize)]
pub struct ServerConfigView {
    pub host: String,
    pub port: u16,
}

#[derive(Serialize)]
pub struct DeepSeekConfigView {
    pub api_base: String,
    pub model_types: Vec<String>,
    pub max_input_tokens: Vec<u32>,
    pub max_output_tokens: Vec<u32>,
}

#[derive(Serialize)]
pub struct AccountView {
    pub email: String,
    pub mobile: String,
    pub area_code: String,
    pub password: String,
}

// ── 脱敏 ─────────────────────────────────────────────────────────────────

fn mask_config(config: &Config) -> AdminConfigResponse {
    AdminConfigResponse {
        server: ServerConfigView {
            host: config.server.host.clone(),
            port: config.server.port,
        },
        deepseek: DeepSeekConfigView {
            api_base: config.deepseek.api_base.clone(),
            model_types: config.deepseek.model_types.clone(),
            max_input_tokens: config.deepseek.max_input_tokens.clone(),
            max_output_tokens: config.deepseek.max_output_tokens.clone(),
        },
        accounts: config
            .accounts
            .iter()
            .map(|a| AccountView {
                email: a.email.clone(),
                mobile: a.mobile.clone(),
                area_code: a.area_code.clone(),
                password: "***".to_string(),
            })
            .collect(),
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────

/// POST /admin/api/setup — 首次设置密码
pub(crate) async fn admin_setup(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Response {
    // 已设置密码则不允许再次 setup
    if state.store.has_password().await {
        return error_response(StatusCode::FORBIDDEN, "密码已设置，请使用登录接口");
    }

    // 速率限制（防止 admin.json 被删除后暴力破解）
    if state.login_limiter.is_locked() {
        let remaining = state.login_limiter.remaining_lock_secs();
        return error_response(
            StatusCode::TOO_MANY_REQUESTS,
            &format!("请求次数过多，请 {} 秒后重试", remaining),
        );
    }

    let req: SetupRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &format!("请求格式错误: {}", e)),
    };

    if req.password.len() < 6 {
        state.login_limiter.record_failure();
        return error_response(StatusCode::BAD_REQUEST, "密码长度至少 6 位");
    }

    let password_hash = super::store::hash_password(&req.password);
    let jwt_secret = super::store::generate_hex_secret();
    let admin_store = super::store::AdminStore {
        password_hash,
        jwt_secret,
        jwt_issued_at: 0,
    };

    if let Err(e) = state.store.save_admin(&admin_store).await {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("保存失败: {}", e));
    }

    // 设置密码后自动签发 JWT
    let token = match super::auth::sign_jwt(&state.store).await {
        Some(t) => t,
        None => return error_response(StatusCode::INTERNAL_SERVER_ERROR, "JWT 签发失败"),
    };

    json_response(&LoginResponse { token })
}

/// POST /admin/api/login — 密码登录
pub(crate) async fn admin_login(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Response {
    // 未设置密码
    if !state.store.has_password().await {
        return error_response(StatusCode::FORBIDDEN, "未设置密码，请先使用 setup 接口");
    }

    // 登录失败率限制
    if state.login_limiter.is_locked() {
        let remaining = state.login_limiter.remaining_lock_secs();
        return error_response(
            StatusCode::TOO_MANY_REQUESTS,
            &format!("登录失败次数过多，请 {} 秒后重试", remaining),
        );
    }

    let req: LoginRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &format!("请求格式错误: {}", e)),
    };

    if state.store.verify_password(&req.password).await {
        state.login_limiter.record_success();
        let token = match super::auth::sign_jwt(&state.store).await {
            Some(t) => t,
            None => return error_response(StatusCode::INTERNAL_SERVER_ERROR, "JWT 签发失败"),
        };
        json_response(&LoginResponse { token })
    } else {
        state.login_limiter.record_failure();
        error_response(StatusCode::UNAUTHORIZED, "密码错误")
    }
}

/// GET /admin/api/status
pub(crate) async fn admin_status(State(state): State<AppState>) -> Response {
    let statuses = state.adapter.account_statuses();
    let total = statuses.len();
    let busy = statuses.iter().filter(|a| a.state == "busy").count();
    let idle = statuses.iter().filter(|a| a.state == "idle").count();
    let error = statuses.iter().filter(|a| a.state == "error").count();
    let invalid = statuses.iter().filter(|a| a.state == "invalid").count();

    let resp = AdminStatusResponse {
        accounts: statuses,
        total,
        idle,
        busy,
        error,
        invalid,
    };
    json_response(&resp)
}

/// GET /admin/api/stats
pub(crate) async fn admin_stats(State(state): State<AppState>) -> Response {
    let snapshot = state.stats.snapshot();
    let resp = AdminStatsResponse { stats: snapshot };
    json_response(&resp)
}

/// GET /admin/api/models
pub(crate) async fn admin_models(State(state): State<AppState>) -> Response {
    let models = state.adapter.list_models();
    json_response(&models)
}

/// GET /admin/api/config
pub(crate) async fn admin_config(State(state): State<AppState>) -> Response {
    let config_view = mask_config(&state.config);
    json_response(&config_view)
}

/// GET /admin/api/keys — 列出 API Key（脱敏）
pub(crate) async fn admin_list_keys(State(state): State<AppState>) -> Response {
    let keys = state.store.list_api_keys_masked().await;
    json_response(&keys)
}

/// POST /admin/api/keys — 创建 API Key
pub(crate) async fn admin_create_key(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Response {
    let req: CreateKeyRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &format!("请求格式错误: {}", e)),
    };

    match state.store.add_api_key(req.description).await {
        Ok(key) => json_response(&CreateKeyResponse { key }),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("创建失败: {}", e)),
    }
}

/// DELETE /admin/api/keys/{key} — 删除 API Key
pub(crate) async fn admin_delete_key(
    Path(key): Path<String>,
    State(state): State<AppState>,
) -> Response {
    match state.store.delete_api_key(&key).await {
        Ok(true) => json_response(&serde_json::json!({"ok": true})),
        Ok(false) => error_response(StatusCode::NOT_FOUND, "API Key 不存在"),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("删除失败: {}", e)),
    }
}

/// POST /admin/api/accounts — 动态添加账号
pub(crate) async fn admin_add_account(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Response {
    let req: AddAccountRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &format!("请求格式错误: {}", e)),
    };

    if req.email.is_empty() && req.mobile.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "email 和 mobile 不能同时为空");
    }

    let creds = crate::config::Account {
        email: req.email,
        mobile: req.mobile,
        area_code: req.area_code,
        password: req.password,
    };

    match state.adapter.add_account(&creds).await {
        Ok(id) => json_response(&serde_json::json!({"ok": true, "id": id})),
        Err(e) => error_response(StatusCode::CONFLICT, &e.to_string()),
    }
}

/// DELETE /admin/api/accounts/{id} — 动态移除账号
pub(crate) async fn admin_remove_account(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Response {
    match state.adapter.remove_account(&id).await {
        Ok(removed_id) => json_response(&serde_json::json!({"ok": true, "id": removed_id})),
        Err(e) => {
            let status = match &e {
                crate::ds_core::PoolError::NotFound(_) => StatusCode::NOT_FOUND,
                crate::ds_core::PoolError::AccountBusy(_) => StatusCode::CONFLICT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            error_response(status, &e.to_string())
        }
    }
}

/// POST /admin/api/accounts/{id}/relogin — 手动重新登录 Error/Invalid 账号
pub(crate) async fn admin_relogin_account(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Response {
    match state.adapter.re_login_single(&id).await {
        Ok(()) => json_response(&serde_json::json!({"ok": true, "id": id})),
        Err(e) => error_response(StatusCode::BAD_REQUEST, &e),
    }
}

/// POST /admin/api/reload — 热重载 config.toml 中的账号
pub(crate) async fn admin_reload_config(State(state): State<AppState>) -> Response {
    let config_path = std::env::var("DS_CONFIG")
        .unwrap_or_else(|_| "config.toml".to_string());
    let new_config = match crate::config::Config::load(&config_path) {
        Ok(c) => c,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &format!("配置加载失败: {}", e)),
    };

    let old_accounts = state.adapter.account_statuses();
    let old_ids: Vec<String> = old_accounts.iter()
        .map(|a| if !a.email.is_empty() { a.email.clone() } else { a.mobile.clone() })
        .collect();

    // Add new accounts
    let mut added = 0usize;
    let mut failed = 0usize;
    for acct in &new_config.accounts {
        let id = if !acct.email.is_empty() { &acct.email } else { &acct.mobile };
        if !old_ids.contains(id) {
            match state.adapter.add_account(acct).await {
                Ok(_) => added += 1,
                Err(e) => {
                    log::warn!(target: "admin", "添加账号 {} 失败: {}", id, e);
                    failed += 1;
                }
            }
        }
    }

    // Remove accounts not in new config
    let mut removed = 0usize;
    let new_ids: Vec<&str> = new_config.accounts.iter()
        .map(|a| if !a.email.is_empty() { a.email.as_str() } else { a.mobile.as_str() })
        .collect();
    for old_id in &old_ids {
        if !new_ids.contains(&old_id.as_str()) && !old_id.is_empty() {
            match state.adapter.remove_account(old_id).await {
                Ok(_) => removed += 1,
                Err(e) => {
                    log::warn!(target: "admin", "移除账号 {} 失败: {}", old_id, e);
                }
            }
        }
    }

    json_response(&serde_json::json!({
        "ok": true,
        "added": added,
        "removed": removed,
        "failed": failed,
    }))
}

#[derive(Deserialize)]
pub struct LogsQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize { 50 }

/// GET /admin/api/logs — 获取最近的请求日志
pub(crate) async fn admin_logs(
    Query(query): Query<LogsQuery>,
    State(state): State<AppState>,
) -> Response {
    let logs = state.stats.recent_logs(query.limit);
    json_response(&logs)
}

#[derive(Deserialize)]
pub struct RuntimeLogsQuery {
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_runtime_limit")]
    pub limit: usize,
}

fn default_runtime_limit() -> usize { 100 }

/// GET /admin/api/runtime-logs — 分页查询运行日志
pub(crate) async fn admin_runtime_logs(
    Query(query): Query<RuntimeLogsQuery>,
) -> Response {
    let (total, logs) = super::runtime_log::query_logs(query.offset, query.limit).await;
    json_response(&serde_json::json!({
        "total": total,
        "offset": query.offset,
        "limit": query.limit,
        "logs": logs,
    }))
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn json_response<T: Serialize>(data: &T) -> Response {
    let bytes = serde_json::to_vec(data).unwrap();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .unwrap()
}

fn error_response(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({"error": message});
    let bytes = serde_json::to_vec(&body).unwrap();
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .unwrap()
}
