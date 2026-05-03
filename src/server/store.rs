//! 持久化存储 —— admin.json / api_keys.json / stats.json 的原子读写

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rand::RngExt;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use log::{info, warn};

/// 管理 admin.json 的数据
#[derive(Serialize, Deserialize, Clone)]
pub struct AdminStore {
    /// bcrypt 哈希后的密码
    pub password_hash: String,
    /// JWT 签名密钥（hex 编码的 32 字节随机值）
    pub jwt_secret: String,
    /// 最近一次 JWT 签发时间（用于吊销旧 token）
    #[serde(default)]
    pub jwt_issued_at: u64,
}

/// 管理 api_keys.json 的数据
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApiKeyEntry {
    pub key: String,
    pub description: String,
    pub created_at: u64,
}

pub type ApiKeyStore = Vec<ApiKeyEntry>;

/// 管理 stats.json 的数据
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct StatsStore {
    pub total_requests: u64,
    pub success_requests: u64,
    pub failed_requests: u64,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    /// 按模型拆分的统计（重启可恢复）
    #[serde(default)]
    pub model_stats: std::collections::HashMap<String, ModelStatsData>,
    /// 按 API Key 拆分的统计（重启可恢复，key 为脱敏后的前缀）
    #[serde(default)]
    pub key_stats: std::collections::HashMap<String, KeyStatsData>,
    /// 最近 N 条请求日志（重启可恢复）
    #[serde(default)]
    pub request_logs: Vec<RequestLogData>,
}

/// 持久化的模型统计数据
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModelStatsData {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub requests: u64,
}

/// 持久化的 API Key 统计数据
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KeyStatsData {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub requests: u64,
}

/// 持久化的请求日志条目
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RequestLogData {
    pub timestamp: u64,
    pub request_id: String,
    pub model: String,
    pub api_key: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub latency_ms: u64,
    pub success: bool,
}

/// 运行时存储管理器
pub struct StoreManager {
    base_dir: PathBuf,
    pub admin: Arc<RwLock<Option<AdminStore>>>,
    pub api_keys: Arc<RwLock<ApiKeyStore>>,
    /// API Key 快速查找索引（与 api_keys 同步更新）
    api_key_set: Arc<RwLock<HashSet<String>>>,
    pub stats: Arc<RwLock<StatsStore>>,
}

impl StoreManager {
    pub fn new(base_dir: &Path) -> Self {
        let admin_path = base_dir.join("admin.json");
        let keys_path = base_dir.join("api_keys.json");

        let admin = if admin_path.exists() {
            match read_json_file::<AdminStore>(&admin_path) {
                Ok(store) => {
                    info!(target: "store", "已加载 admin.json");
                    Some(store)
                }
                Err(e) => {
                    warn!(target: "store", "admin.json 解析失败: {}，将引导重新设置密码", e);
                    None
                }
            }
        } else {
            info!(target: "store", "admin.json 不存在，首次访问时将引导设置密码");
            None
        };

        let api_keys = if keys_path.exists() {
            match read_json_file::<ApiKeyStore>(&keys_path) {
                Ok(keys) => {
                    info!(target: "store", "已加载 api_keys.json ({} 个 Key)", keys.len());
                    keys
                }
                Err(e) => {
                    warn!(target: "store", "api_keys.json 解析失败: {}，使用空列表", e);
                    Vec::new()
                }
            }
        } else {
            info!(target: "store", "api_keys.json 不存在，使用空列表");
            Vec::new()
        };

        let stats = if base_dir.join("stats.json").exists() {
            match read_json_file::<StatsStore>(&base_dir.join("stats.json")) {
                Ok(s) => {
                    info!(target: "store", "已加载 stats.json");
                    s
                }
                Err(e) => {
                    warn!(target: "store", "stats.json 解析失败: {}，使用零值", e);
                    StatsStore::default()
                }
            }
        } else {
            StatsStore::default()
        };

        let key_set: HashSet<String> = api_keys.iter().map(|k| k.key.clone()).collect();

        Self {
            base_dir: base_dir.to_path_buf(),
            admin: Arc::new(RwLock::new(admin)),
            api_keys: Arc::new(RwLock::new(api_keys)),
            api_key_set: Arc::new(RwLock::new(key_set)),
            stats: Arc::new(RwLock::new(stats)),
        }
    }

    /// 保存 admin.json
    pub async fn save_admin(&self, store: &AdminStore) -> anyhow::Result<()> {
        let path = self.base_dir.join("admin.json");
        write_json_file(&path, store)?;
        *self.admin.write().await = Some(store.clone());
        Ok(())
    }

    /// 保存 api_keys.json
    pub async fn save_api_keys(&self, keys: &ApiKeyStore) -> anyhow::Result<()> {
        let path = self.base_dir.join("api_keys.json");
        write_json_file(&path, keys)?;
        let key_set: HashSet<String> = keys.iter().map(|k| k.key.clone()).collect();
        *self.api_keys.write().await = keys.clone();
        *self.api_key_set.write().await = key_set;
        Ok(())
    }

    /// 加载持久化的统计数据
    pub async fn load_stats(&self) -> StatsStore {
        self.stats.read().await.clone()
    }

    /// 保存 stats.json
    pub async fn save_stats(&self, store: &StatsStore) -> anyhow::Result<()> {
        let path = self.base_dir.join("stats.json");
        write_json_file(&path, store)?;
        *self.stats.write().await = store.clone();
        Ok(())
    }

    /// 检查是否已设置密码
    pub async fn has_password(&self) -> bool {
        self.admin.read().await.is_some()
    }

    /// 验证密码
    pub async fn verify_password(&self, plain: &str) -> bool {
        let guard = self.admin.read().await;
        if let Some(store) = guard.as_ref() {
            bcrypt::verify(plain, &store.password_hash).unwrap_or(false)
        } else {
            false
        }
    }

    /// 获取 JWT 密钥
    pub async fn jwt_secret(&self) -> Option<String> {
        self.admin.read().await.as_ref().map(|s| s.jwt_secret.clone())
    }

    /// 获取最近一次 JWT 签发时间（用于吊销旧 token）
    pub async fn jwt_issued_at(&self) -> Option<u64> {
        self.admin.read().await.as_ref().map(|s| s.jwt_issued_at).filter(|&t| t > 0)
    }

    /// 更新 jwt_issued_at 并持久化
    pub async fn set_jwt_issued_at(&self, iat: u64) {
        let mut guard = self.admin.write().await;
        if let Some(store) = guard.as_mut() {
            store.jwt_issued_at = iat;
            let updated = store.clone();
            drop(guard);
            let _ = self.save_admin(&updated).await;
        }
    }

    /// 查找 API Key 是否有效
    pub async fn is_valid_api_key(&self, key: &str) -> bool {
        // O(1) HashSet 查找
        self.api_key_set.read().await.contains(key)
    }

    /// 列出所有 API Key（脱敏）
    pub async fn list_api_keys_masked(&self) -> Vec<ApiKeyEntry> {
        let guard = self.api_keys.read().await;
        guard
            .iter()
            .map(|k| ApiKeyEntry {
                key: mask_key(&k.key),
                description: k.description.clone(),
                created_at: k.created_at,
            })
            .collect()
    }

    /// 添加 API Key
    pub async fn add_api_key(&self, description: String) -> anyhow::Result<String> {
        let key = generate_api_key();
        let entry = ApiKeyEntry {
            key: key.clone(),
            description,
            created_at: now_secs(),
        };
        let mut guard = self.api_keys.write().await;
        guard.push(entry);
        let keys = guard.clone();
        drop(guard);
        self.save_api_keys(&keys).await?;
        Ok(key)
    }

    /// 删除 API Key（按完整 key 匹配）
    pub async fn delete_api_key(&self, key: &str) -> anyhow::Result<bool> {
        let mut guard = self.api_keys.write().await;
        let before = guard.len();
        guard.retain(|k| k.key != key);
        if guard.len() == before {
            return Ok(false);
        }
        let keys = guard.clone();
        drop(guard);
        self.save_api_keys(&keys).await?;
        Ok(true)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn generate_api_key() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    format!("sk-{}", hex::encode(&bytes))
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        "***".to_string()
    } else {
        format!("{}***", &key[..8])
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 原子写入 JSON 文件：先写 .tmp 再 rename
fn write_json_file<T: Serialize>(path: &Path, data: &T) -> anyhow::Result<()> {
    let tmp_path = path.with_extension("tmp");
    let json = serde_json::to_string_pretty(data)?;
    fs::write(&tmp_path, &json)?;
    fs::rename(&tmp_path, path)?;
    // 设置文件权限 0600（仅 owner 可读写）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> anyhow::Result<T> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

/// 生成随机 hex 字符串（32 字节 = 64 hex 字符）
pub fn generate_hex_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    hex::encode(&bytes)
}

/// 对密码进行 bcrypt 哈希
pub fn hash_password(plain: &str) -> String {
    bcrypt::hash(plain, 12).expect("bcrypt hash 不应失败")
}

// hex 编码辅助（避免额外依赖）
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
