//! 鉴权模块 —— JWT 签发/验证 + 登录失败率限制

use std::sync::atomic::{AtomicU64, Ordering};

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use super::store::StoreManager;

// ── JWT ────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct TokenClaims {
    /// 固定为 "admin"
    pub sub: String,
    /// 签发时间戳（秒）
    pub iat: u64,
    /// 过期时间戳（秒）
    pub exp: u64,
}

/// JWT 有效期：24 小时
const JWT_EXPIRY_SECS: u64 = 24 * 3600;

/// 签发 JWT
pub async fn sign_jwt(store: &StoreManager) -> Option<String> {
    let secret = store.jwt_secret().await?;
    let now = epoch_secs();
    let claims = TokenClaims {
        sub: "admin".to_string(),
        iat: now,
        exp: now + JWT_EXPIRY_SECS,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .ok()?;

    // 更新 jwt_issued_at（用于吊销旧 token）
    store.set_jwt_issued_at(now).await;

    Some(token)
}

/// 验证 JWT，返回是否有效
pub async fn verify_jwt(store: &StoreManager, token: &str) -> bool {
    let secret = match store.jwt_secret().await {
        Some(s) => s,
        None => return false,
    };
    let mut validation = Validation::default();
    validation.leeway = 60; // 允许 60 秒时钟偏差
    let claims = match decode::<TokenClaims>(token, &DecodingKey::from_secret(secret.as_bytes()), &validation) {
        Ok(data) => data.claims,
        Err(_) => return false,
    };

    // 吊销检查：token 的 iat 必须 >= 存储的 jwt_issued_at
    // 改密码时会更新 jwt_issued_at，使旧 token 失效
    if let Some(min_iat) = store.jwt_issued_at().await {
        if claims.iat < min_iat {
            return false;
        }
    }

    true
}

// ── 登录失败率限制 ────────────────────────────────────────────────────────

/// 最大失败次数
const MAX_FAILURES: u64 = 5;
/// 锁定时长
const LOCKOUT_SECS: u64 = 300; // 5 分钟

pub struct LoginLimiter {
    fail_count: AtomicU64,
    locked_until: AtomicU64, // epoch secs，0 表示未锁定
}

impl LoginLimiter {
    pub fn new() -> Self {
        Self {
            fail_count: AtomicU64::new(0),
            locked_until: AtomicU64::new(0),
        }
    }

    /// 检查是否被锁定
    pub fn is_locked(&self) -> bool {
        let until = self.locked_until.load(Ordering::Relaxed);
        if until == 0 {
            return false;
        }
        if epoch_secs() >= until {
            // 锁定已过期，重置
            self.locked_until.store(0, Ordering::Relaxed);
            self.fail_count.store(0, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// 记录一次失败
    pub fn record_failure(&self) {
        let count = self.fail_count.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= MAX_FAILURES {
            self.locked_until
                .store(epoch_secs() + LOCKOUT_SECS, Ordering::Relaxed);
        }
    }

    /// 记录成功，重置计数
    pub fn record_success(&self) {
        self.fail_count.store(0, Ordering::Relaxed);
        self.locked_until.store(0, Ordering::Relaxed);
    }

    /// 剩余锁定秒数
    pub fn remaining_lock_secs(&self) -> u64 {
        let until = self.locked_until.load(Ordering::Relaxed);
        if until == 0 {
            return 0;
        }
        let now = epoch_secs();
        if now >= until {
            0
        } else {
            until - now
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
