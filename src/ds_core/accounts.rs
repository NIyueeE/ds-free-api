//! 账号池管理 —— 多账号负载均衡
//!
//! 1 account = 1 session = 1 concurrency。多并发需横向扩展账号数。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::config::Account as AccountConfig;
use crate::ds_core::client::{
    ClientError, CompletionPayload, DsClient, LoginPayload, UpdateTitlePayload,
};
use crate::ds_core::pow::{PowError, PowSolver};
use futures::TryStreamExt;
use log::{debug, info, warn};

/// 账号状态信息
pub struct AccountStatus {
    pub email: String,
    pub mobile: String,
}

pub struct Account {
    token: String,
    email: String,
    mobile: String,
    sessions: HashMap<String, String>,
    is_busy: AtomicBool,
}

impl Account {
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn session_id(&self, model_type: &str) -> Option<&str> {
        self.sessions.get(model_type).map(|s| s.as_str())
    }

    pub fn is_busy(&self) -> bool {
        self.is_busy.load(Ordering::Relaxed)
    }
}

/// 持有期间账号标记为 busy，Drop 时自动释放
pub struct AccountGuard {
    account: Arc<Account>,
}

impl AccountGuard {
    pub fn account(&self) -> &Account {
        &self.account
    }
}

impl Drop for AccountGuard {
    fn drop(&mut self) {
        self.account.is_busy.store(false, Ordering::Relaxed);
    }
}

pub struct AccountPool {
    accounts: Vec<Arc<Account>>,
    index: AtomicUsize,
}

#[derive(Debug, thiserror::Error)]
pub enum PoolError {
    /// 所有账号初始化失败（没有可用账号）
    #[error("所有账号初始化失败")]
    AllAccountsFailed,

    /// 下游客户端错误（网络、API 错误等）
    #[error("客户端错误: {0}")]
    Client(#[from] ClientError),

    /// PoW 计算失败（WASM 执行错误）
    #[error("PoW 计算失败: {0}")]
    Pow(#[from] PowError),

    /// 账号配置验证失败
    #[error("账号配置错误: {0}")]
    Validation(String),
}

impl AccountPool {
    pub fn new() -> Self {
        Self {
            accounts: Vec::new(),
            index: AtomicUsize::new(0),
        }
    }

    pub async fn init(
        &mut self,
        creds: Vec<AccountConfig>,
        model_types: Vec<String>,
        client: &DsClient,
        solver: &PowSolver,
    ) -> Result<(), PoolError> {
        use futures::future::join_all;

        // 全并发初始化所有账号
        let futures: Vec<_> = creds
            .into_iter()
            .map(|creds| {
                let client = client.clone();
                let solver = solver.clone();
                let model_types = model_types.clone();
                async move {
                    let display_id = if creds.mobile.is_empty() {
                        creds.email.clone()
                    } else {
                        creds.mobile.clone()
                    };
                    match init_account(&creds, &client, &solver, &model_types).await {
                        Ok(account) => {
                            info!(target: "ds_core::accounts", "账号 {} 初始化成功", display_id);
                            Some(Arc::new(account))
                        }
                        Err(e) => {
                            warn!(target: "ds_core::accounts", "账号 {} 初始化失败: {}", display_id, e);
                            None
                        }
                    }
                }
            })
            .collect();

        let results = join_all(futures).await;
        self.accounts = results.into_iter().flatten().collect();

        if self.accounts.is_empty() {
            return Err(PoolError::AllAccountsFailed);
        }

        Ok(())
    }

    /// 轮询获取一个空闲的账号（必须拥有指定 model_type 的 session）
    pub fn get_account(&self, model_type: &str) -> Option<AccountGuard> {
        if self.accounts.is_empty() {
            return None;
        }

        let idx = self.index.fetch_add(1, Ordering::Relaxed) % self.accounts.len();

        for i in 0..self.accounts.len() {
            let account = &self.accounts[(idx + i) % self.accounts.len()];
            if account.session_id(model_type).is_some()
                && !account.is_busy()
                && account
                    .is_busy
                    .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
            {
                return Some(AccountGuard {
                    account: account.clone(),
                });
            }
        }

        None
    }

    /// 获取所有账号的详细状态
    pub fn account_statuses(&self) -> Vec<AccountStatus> {
        self.accounts
            .iter()
            .map(|a| AccountStatus {
                email: a.email.clone(),
                mobile: a.mobile.clone(),
            })
            .collect()
    }

    /// 优雅关闭：清理所有账号的所有 session
    pub async fn shutdown(&self, client: &DsClient) {
        use futures::future::join_all;

        let futures: Vec<_> = self
            .accounts
            .iter()
            .flat_map(|account| {
                let token = account.token().to_string();
                account
                    .sessions
                    .values()
                    .map(move |session_id| {
                        let client = client.clone();
                        let token = token.clone();
                        async move {
                            if let Err(e) = client.delete_session(&token, session_id).await {
                                warn!(
                                    target: "ds_core::accounts",
                                    "清理 session 失败 ({}): {}",
                                    &token[..8.min(token.len())],
                                    e
                                );
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        join_all(futures).await;
    }
}

async fn init_account(
    creds: &AccountConfig,
    client: &DsClient,
    solver: &PowSolver,
    model_types: &[String],
) -> Result<Account, PoolError> {
    let mut last_error = None;

    for attempt in 1..=3 {
        match try_init_account(creds, client, solver, model_types).await {
            Ok(account) => return Ok(account),
            Err(e) => {
                last_error = Some(e);
                if attempt < 3 {
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
            }
        }
    }

    Err(last_error.expect("循环至少执行一次"))
}

async fn try_init_account(
    creds: &AccountConfig,
    client: &DsClient,
    solver: &PowSolver,
    model_types: &[String],
) -> Result<Account, PoolError> {
    // 验证：email 和 mobile 至少一个非空
    if creds.email.is_empty() && creds.mobile.is_empty() {
        return Err(PoolError::Validation(
            "email 和 mobile 不能同时为空".to_string(),
        ));
    }

    let login_payload = LoginPayload {
        email: if creds.email.is_empty() {
            None
        } else {
            Some(creds.email.clone())
        },
        mobile: if creds.mobile.is_empty() {
            None
        } else {
            Some(creds.mobile.clone())
        },
        password: creds.password.clone(),
        area_code: if creds.area_code.is_empty() {
            None
        } else {
            Some(creds.area_code.clone())
        },
        device_id: String::new(),
        os: "web".to_string(),
    };

    let login_data = client.login(&login_payload).await?;
    debug!(
        target: "ds_core::client",
        "登录响应: code={}, msg={}, user_id={}, email={:?}, mobile={:?}",
        login_data.code,
        login_data.msg,
        login_data.user.id,
        login_data.user.email,
        login_data.user.mobile_number
    );
    let token = login_data.user.token;

    let mut sessions = HashMap::new();
    for model_type in model_types {
        let session_id = client.create_session(&token).await?;
        health_check(&token, &session_id, client, solver, model_type).await?;

        let title_payload = UpdateTitlePayload {
            chat_session_id: session_id.clone(),
            title: format!("auto-managed-{}-DO-NOT-DELETE", model_type),
        };
        client.update_title(&token, &title_payload).await?;

        sessions.insert(model_type.clone(), session_id);
    }

    Ok(Account {
        token,
        email: creds.email.clone(),
        mobile: creds.mobile.clone(),
        sessions,
        is_busy: AtomicBool::new(false),
    })
}

async fn health_check(
    token: &str,
    session_id: &str,
    client: &DsClient,
    solver: &PowSolver,
    model_type: &str,
) -> Result<(), PoolError> {
    debug!(target: "ds_core::accounts", "health_check model_type={}", model_type);
    let challenge = client.create_pow_challenge(token).await?;

    let result = solver.solve(&challenge)?;
    let pow_header = result.to_header();

    let payload = CompletionPayload {
        chat_session_id: session_id.to_string(),
        parent_message_id: None,
        model_type: model_type.to_string(),
        prompt: "只回复`Hello, world!`".to_string(),
        ref_file_ids: vec![],
        thinking_enabled: false,
        search_enabled: false,
        preempt: false,
    };

    let mut stream = client.completion(token, &pow_header, &payload).await?;
    // 消费流确保消息写入
    while let Some(chunk) = stream.try_next().await? {
        let _ = chunk;
    }

    debug!(target: "ds_core::accounts", "health_check 完成 model_type={}", model_type);
    Ok(())
}
