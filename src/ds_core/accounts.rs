//! 账号池管理 —— 多账号负载均衡
//!
//! 1 account = 1 session = 1 concurrency。多并发需横向扩展账号数。

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU8, Ordering};
use std::time::SystemTime;

use dashmap::DashMap;
use futures::TryStreamExt;
use log::{debug, error, info, warn};
use tokio::sync::RwLock;

use crate::config::Account as AccountConfig;
use crate::ds_core::client::{ClientError, CompletionPayload, DsClient, LoginPayload};
use crate::ds_core::pow::{PowError, PowSolver};

/// 账号状态枚举
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountState {
    Idle = 0,
    Busy = 1,
    Error = 2,
    Invalid = 3,
}

impl AccountState {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Idle,
            1 => Self::Busy,
            2 => Self::Error,
            3 => Self::Invalid,
            _ => Self::Invalid,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Busy => "busy",
            Self::Error => "error",
            Self::Invalid => "invalid",
        }
    }
}

/// 账号状态信息
#[derive(serde::Serialize)]
pub struct AccountStatus {
    pub email: String,
    pub mobile: String,
    pub state: String,
    /// 最后释放时间戳（ms），0 表示从未使用
    pub last_released_ms: i64,
    /// 连续登录失败次数
    pub error_count: u8,
}

pub struct Account {
    token: std::sync::RwLock<Arc<str>>,
    email: String,
    mobile: String,
    state: AtomicU8,
    /// 账号最近一次释放的时间戳（ms），用于冷却判断
    last_released: AtomicI64,
    /// 连续登录失败次数
    error_count: AtomicU8,
    /// 原始凭据（用于重新登录）
    creds: AccountConfig,
}

/// 连续登录失败上限，达到后标记为 Invalid
const MAX_ERROR_COUNT: u8 = 3;

impl Account {
    pub fn token(&self) -> Arc<str> {
        self.token.read().unwrap().clone()
    }

    pub fn display_id(&self) -> &str {
        if !self.email.is_empty() {
            &self.email
        } else {
            &self.mobile
        }
    }

    pub fn state(&self) -> AccountState {
        AccountState::from_u8(self.state.load(Ordering::Relaxed))
    }

    #[allow(dead_code)]
    pub fn is_busy(&self) -> bool {
        self.state() == AccountState::Busy
    }

    pub fn is_available(&self) -> bool {
        self.state() == AccountState::Idle
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
        // 只有 Busy 状态才释放回 Idle（避免覆盖 Error/Invalid）
        self.account.state.compare_exchange(
            AccountState::Busy as u8,
            AccountState::Idle as u8,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ).ok();
        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        self.account.last_released.store(now_ms, Ordering::Relaxed);
    }
}

pub struct AccountPool {
    /// key = display_id (email or mobile), value = Account
    accounts: DashMap<String, Arc<Account>>,
    client: RwLock<Option<DsClient>>,
    solver: RwLock<Option<PowSolver>>,
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

    /// 账号已存在
    #[error("账号已存在: {0}")]
    AlreadyExists(String),

    /// 账号不存在
    #[error("账号不存在: {0}")]
    NotFound(String),

    /// 账号正在使用中，无法删除
    #[error("账号正在使用中: {0}")]
    AccountBusy(String),
}

impl AccountPool {
    pub fn new() -> Self {
        Self {
            accounts: DashMap::new(),
            client: RwLock::new(None),
            solver: RwLock::new(None),
        }
    }

    pub async fn init(
        &self,
        creds: Vec<AccountConfig>,
        client: &DsClient,
        solver: &PowSolver,
    ) -> Result<(), PoolError> {
        use futures::future::join_all;
        use std::sync::Arc;
        use tokio::sync::Semaphore;

        // 限制并发初始化数，避免对 DeepSeek 端和本地连接池造成压力
        let semaphore = Arc::new(Semaphore::new(13));
        let futures: Vec<_> = creds
            .into_iter()
            .map(|creds| {
                let client = client.clone();
                let solver = solver.clone();
                let sem = semaphore.clone();
                async move {
                    let _permit = sem.acquire().await.expect("信号量未关闭");
                    let display_id = if creds.mobile.is_empty() {
                        creds.email.clone()
                    } else {
                        creds.mobile.clone()
                    };
                    match init_account(&creds, &client, &solver).await {
                        Ok(account) => {
                            info!(target: "ds_core::accounts", "账号 {} 初始化成功", display_id);
                            Some((display_id, Arc::new(account)))
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
        let initialized: Vec<(String, Arc<Account>)> = results.into_iter().flatten().collect();

        if initialized.is_empty() {
            error!(target: "ds_core::accounts", "所有账号初始化失败");
            return Err(PoolError::AllAccountsFailed);
        }

        for (id, account) in initialized {
            self.accounts.insert(id, account);
        }
        Ok(())
    }

    /// 动态添加账号（运行时初始化）
    pub async fn add_account(
        &self,
        creds: &AccountConfig,
        client: &DsClient,
        solver: &PowSolver,
    ) -> Result<String, PoolError> {
        let display_id = if creds.mobile.is_empty() {
            creds.email.clone()
        } else {
            creds.mobile.clone()
        };

        // 检查是否已存在（DashMap O(1) 查找）
        if self.accounts.contains_key(&display_id) {
            return Err(PoolError::AlreadyExists(display_id));
        }

        let account = init_account(creds, client, solver).await?;
        let _id = account.display_id().to_string();
        self.accounts.insert(display_id.clone(), Arc::new(account));
        info!(target: "ds_core::accounts", "动态添加账号 {} 成功", display_id);
        Ok(display_id)
    }

    /// 动态移除账号（仅空闲账号可移除）
    pub async fn remove_account(&self, email_or_mobile: &str) -> Result<String, PoolError> {
        let account = self.accounts.get(email_or_mobile)
            .ok_or_else(|| PoolError::NotFound(email_or_mobile.to_string()))?;

        if account.is_busy() {
            return Err(PoolError::AccountBusy(email_or_mobile.to_string()));
        }

        // 也允许移除 Error/Invalid 状态的账号
        drop(account);
        let (_, removed) = self.accounts.remove(email_or_mobile)
            .ok_or_else(|| PoolError::NotFound(email_or_mobile.to_string()))?;
        let id = removed.display_id().to_string();
        info!(target: "ds_core::accounts", "动态移除账号 {}", id);
        Ok(id)
    }

    /// 获取空闲最久的可用账号，带等待：无可用账号时最多等待 `timeout_ms` 毫秒
    pub async fn get_account_with_wait(&self, timeout_ms: u64) -> Option<AccountGuard> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            if let Some(g) = self.get_account() {
                return Some(g);
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    /// 获取空闲最久的可用账号（不等待，立即返回）
    ///
    /// 遍历所有账号，选冷却已过且空闲时间最长的那个，最大化每次使用间隔。
    /// DashMap 无锁读，不阻塞并发请求。
    pub fn get_account(&self) -> Option<AccountGuard> {
        if self.accounts.is_empty() {
            return None;
        }

        let now_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let mut best: Option<Arc<Account>> = None;
        let mut best_idle = i64::MIN;

        for entry in self.accounts.iter() {
            let account = entry.value();
            if !account.is_available() {
                continue;
            }
            let idle = now_ms - account.last_released.load(Ordering::Relaxed);
            if idle > best_idle {
                best_idle = idle;
                best = Some(Arc::clone(account));
            }
        }

        let account = best?;
        account
            .state
            .compare_exchange(
                AccountState::Idle as u8,
                AccountState::Busy as u8,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .ok()?;
        Some(AccountGuard { account })
    }

    /// 获取所有账号的详细状态
    pub fn account_statuses(&self) -> Vec<AccountStatus> {
        self.accounts
            .iter()
            .map(|entry| {
                let a = entry.value();
                AccountStatus {
                    email: a.email.clone(),
                    mobile: a.mobile.clone(),
                    state: a.state().as_str().to_string(),
                    last_released_ms: a.last_released.load(Ordering::Relaxed),
                    error_count: a.error_count.load(Ordering::Relaxed),
                }
            })
            .collect()
    }

    /// 优雅关闭（新流程无持久 session，无需清理）
    pub async fn shutdown(&self, _client: &DsClient) {}

    /// 存储 client 和 solver 供恢复任务使用
    pub async fn set_client_solver(&self, client: DsClient, solver: PowSolver) {
        *self.client.write().await = Some(client);
        *self.solver.write().await = Some(solver);
    }

    /// 标记账号为 Error 状态（请求失败时调用）
    pub fn mark_error(&self, email_or_mobile: &str) {
        if let Some(entry) = self.accounts.get(email_or_mobile) {
            let account = entry.value();
            // 只从 Busy 转到 Error（避免覆盖 Invalid）
            account.state.compare_exchange(
                AccountState::Busy as u8,
                AccountState::Error as u8,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ).ok();
            warn!(target: "ds_core::accounts", "账号 {} 标记为 Error", account.display_id());
        }
    }

    /// 手动重新登录指定账号（管理员触发）
    /// 成功 → Idle，失败 → error_count++，≥3 则 Invalid
    pub async fn re_login_single(&self, email_or_mobile: &str) -> Result<(), String> {
        let client_opt = self.client.read().await.clone();
        let solver_opt = self.solver.read().await.clone();
        let (client, solver) = match (client_opt, solver_opt) {
            (Some(c), Some(s)) => (c, s),
            _ => return Err("client/solver 未初始化".to_string()),
        };

        let account = self.accounts.get(email_or_mobile)
            .ok_or_else(|| format!("账号 {} 不存在", email_or_mobile))?;
        let account = account.value();

        // 只允许 Error/Invalid 状态的账号重登
        let state = account.state();
        if state != AccountState::Error && state != AccountState::Invalid {
            return Err(format!("账号状态为 {}，仅 Error/Invalid 可重登", state.as_str()));
        }

        Self::re_login_account(account, &client, &solver).await;

        // 检查重登后状态
        let new_state = account.state();
        if new_state == AccountState::Idle {
            Ok(())
        } else {
            Err(format!("重登失败，当前状态: {}", new_state.as_str()))
        }
    }

    /// 尝试重新登录 Error 状态的账号
    /// 成功 → Idle，失败 → error_count++，≥3 则 Invalid
    async fn re_login_account(account: &Account, client: &DsClient, solver: &PowSolver) {
        let display_id = account.display_id().to_string();
        match try_init_account(&account.creds, client, solver).await {
            Ok(new_account) => {
                // 更新 token
                *account.token.write().unwrap() = new_account.token.read().unwrap().clone();
                account.state.store(AccountState::Idle as u8, Ordering::Relaxed);
                account.error_count.store(0, Ordering::Relaxed);
                info!(target: "ds_core::accounts", "账号 {} 重新登录成功", display_id);
            }
            Err(e) => {
                let count = account.error_count.fetch_add(1, Ordering::Relaxed) + 1;
                if count >= MAX_ERROR_COUNT {
                    account.state.store(AccountState::Invalid as u8, Ordering::Relaxed);
                    error!(target: "ds_core::accounts", "账号 {} 连续 {} 次重登失败，标记为 Invalid: {}", display_id, count, e);
                } else {
                    warn!(target: "ds_core::accounts", "账号 {} 重登失败 ({}次): {}", display_id, count, e);
                }
            }
        }
    }

    /// 启动后台恢复任务：每 60 秒扫描 Error 账号并尝试重新登录
    pub fn start_recovery_task(self: &Arc<Self>) {
        let pool = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

                let client_opt = pool.client.read().await.clone();
                let solver_opt = pool.solver.read().await.clone();
                let (client, solver) = match (client_opt, solver_opt) {
                    (Some(c), Some(s)) => (c, s),
                    _ => continue,
                };

                for entry in pool.accounts.iter() {
                    let account = entry.value();
                    if account.state() == AccountState::Error {
                        Self::re_login_account(account, &client, &solver).await;
                    }
                }
            }
        });
    }
}

async fn init_account(
    creds: &AccountConfig,
    client: &DsClient,
    solver: &PowSolver,
) -> Result<Account, PoolError> {
    let mut last_error = None;

    for attempt in 1..=3 {
        match try_init_account(creds, client, solver).await {
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

    let display_id = if creds.mobile.is_empty() {
        &creds.email
    } else {
        &creds.mobile
    };

    // 健康检查：创建临时 session → 发送 test completion → 删除 session
    let session_id = client.create_session(&token).await?;
    if let Err(e) = health_check(&token, &session_id, client, solver, "default", display_id).await {
        // 即使健康检查失败也要清理 session
        let _ = client.delete_session(&token, &session_id).await;
        return Err(e);
    }
    let _ = client.delete_session(&token, &session_id).await;

    Ok(Account {
        token: std::sync::RwLock::new(token.into()),
        email: creds.email.clone(),
        mobile: creds.mobile.clone(),
        state: AtomicU8::new(AccountState::Idle as u8),
        last_released: AtomicI64::new(0),
        error_count: AtomicU8::new(0),
        creds: creds.clone(),
    })
}

async fn health_check(
    token: &str,
    session_id: &str,
    client: &DsClient,
    solver: &PowSolver,
    model_type: &str,
    display_id: &str,
) -> Result<(), PoolError> {
    let start = std::time::Instant::now();
    let challenge = client
        .create_pow_challenge(token, "/api/v0/chat/completion")
        .await?;

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

    debug!(
        target: "ds_core::accounts",
        "health_check 完成 model_type={} account={} elapsed={:?}",
        model_type, display_id, start.elapsed()
    );
    Ok(())
}
