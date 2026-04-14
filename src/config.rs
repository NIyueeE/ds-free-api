//! 配置加载模块 —— 统一配置入口
//!
//! 支持 `-c <path>` 命令行参数，默认值见下方函数。
//! config.toml 中注释项使用代码默认值。

use serde::Deserialize;
use std::path::Path;

/// 应用配置根结构
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// 账号池（必需）
    pub accounts: Vec<Account>,
    /// DeepSeek 相关配置
    #[serde(default)]
    pub deepseek: DeepSeekConfig,
    /// HTTP 服务器配置（必填）
    pub server: ServerConfig,
}

/// 单个账号配置
#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    /// 邮箱（与 mobile 二选一）
    pub email: String,
    /// 手机号（与 email 二选一）
    pub mobile: String,
    /// 区号（与 mobile 配合使用，如 "+86"）
    pub area_code: String,
    /// 密码
    pub password: String,
}

/// DeepSeek 客户端配置
#[derive(Debug, Clone, Deserialize)]
pub struct DeepSeekConfig {
    /// API 基础地址
    #[serde(default = "default_api_base")]
    pub api_base: String,
    /// WASM 文件完整 URL（PoW 计算所需，版本号可能变动）
    #[serde(default = "default_wasm_url")]
    pub wasm_url: String,
    /// User-Agent 请求头
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
    /// X-Client-Version 请求头（用于 expert 模型等功能）
    #[serde(default = "default_client_version")]
    pub client_version: String,
    /// X-Client-Platform 请求头
    #[serde(default = "default_client_platform")]
    pub client_platform: String,
    /// 定义支持的模型类型列表，每种类型会自动映射为 OpenAI 的 model_id：deepseek-<type>
    #[serde(default = "default_model_types")]
    pub model_types: Vec<String>,
}

impl Default for DeepSeekConfig {
    fn default() -> Self {
        Self {
            api_base: default_api_base(),
            wasm_url: default_wasm_url(),
            user_agent: default_user_agent(),
            client_version: default_client_version(),
            client_platform: default_client_platform(),
            model_types: default_model_types(),
        }
    }
}

fn default_model_types() -> Vec<String> {
    vec!["default".to_string(), "expert".to_string()]
}

impl DeepSeekConfig {
    /// 生成 OpenAI 模型注册表映射
    ///
    /// key 为小写的 model_id（如 deepseek-default），value 为内部 model_type（如 default）
    pub fn model_registry(&self) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        for ty in &self.model_types {
            map.insert(format!("deepseek-{}", ty).to_lowercase(), ty.clone());
        }
        map
    }
}

/// HTTP 服务器配置（必填）
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// 监听地址
    pub host: String,
    /// 监听端口
    pub port: u16,
    /// API 访问令牌列表，留空则不鉴权
    #[serde(default)]
    pub api_tokens: Vec<ApiToken>,
    // TODO: admin_password — 等控制面板端点实现时再加
}

/// API 访问令牌
#[derive(Debug, Clone, Deserialize)]
pub struct ApiToken {
    /// 令牌值（如 sk-xxx）
    pub token: String,
    /// 描述说明
    #[serde(default)]
    pub description: String,
}

/// 默认 API 基础地址
fn default_api_base() -> String {
    "https://chat.deepseek.com/api/v0".to_string()
}

/// 默认 WASM 文件 URL（版本号可能变动，建议配置文件中显式指定）
fn default_wasm_url() -> String {
    "https://fe-static.deepseek.com/chat/static/sha3_wasm_bg.7b9ca65ddd.wasm".to_string()
}

/// 默认 User-Agent
fn default_user_agent() -> String {
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36".to_string()
}

/// 默认 X-Client-Version
fn default_client_version() -> String {
    "2.0.0".to_string()
}

/// 默认 X-Client-Platform
fn default_client_platform() -> String {
    "web".to_string()
}

impl Config {
    /// 从指定路径加载配置
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::de::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// 解析命令行参数并加载配置
    ///
    /// 支持 `-c <path>` 指定配置文件路径，默认使用 `config.toml`
    pub fn load_with_args(args: impl Iterator<Item = String>) -> Result<Self, ConfigError> {
        let mut config_path = None;
        let mut iter = args.skip(1); // 跳过程序名

        while let Some(arg) = iter.next() {
            if arg == "-c" {
                if let Some(path) = iter.next() {
                    config_path = Some(path);
                } else {
                    return Err(ConfigError::Cli("-c 参数需要指定路径".to_string()));
                }
            }
        }

        let path = config_path.unwrap_or_else(|| "config.toml".to_string());
        Self::load(&path)
    }

    /// 验证配置有效性
    fn validate(&self) -> Result<(), ConfigError> {
        if self.accounts.is_empty() {
            return Err(ConfigError::Validation("至少需要一个账号配置".to_string()));
        }
        if self.deepseek.model_types.is_empty() {
            return Err(ConfigError::Validation("model_types 不能为空".to_string()));
        }
        Ok(())
    }
}

/// 配置加载错误类型
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML 解析错误: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("配置验证错误: {0}")]
    Validation(String),
    #[error("命令行参数错误: {0}")]
    Cli(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn minimal_toml(extra: &str) -> String {
        format!(
            r#"
[[accounts]]
email = "test@example.com"
mobile = ""
area_code = ""
password = "pass"

[server]
host = "127.0.0.1"
port = 5317
{extra}
"#
        )
    }

    fn write_temp(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    // --- Config::load ---

    #[test]
    fn load_minimal_config() {
        let f = write_temp(&minimal_toml(""));
        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(cfg.accounts.len(), 1);
        assert_eq!(cfg.accounts[0].email, "test@example.com");
        assert_eq!(cfg.server.port, 5317);
    }

    #[test]
    fn load_missing_file_returns_io_error() {
        let err = Config::load("/nonexistent/path/config.toml").unwrap_err();
        assert!(matches!(err, ConfigError::Io(_)));
    }

    #[test]
    fn load_invalid_toml_returns_toml_error() {
        let f = write_temp("this is not toml ][");
        let err = Config::load(f.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)));
    }

    // --- validate ---

    #[test]
    fn validate_rejects_empty_accounts() {
        // accounts = [] 是合法 TOML，但 validate() 应拒绝空账号列表
        let toml = r#"
accounts = []

[server]
host = "127.0.0.1"
port = 5317
"#;
        let f = write_temp(toml);
        let err = Config::load(f.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    #[test]
    fn validate_rejects_empty_model_types() {
        let toml = minimal_toml("[deepseek]\nmodel_types = []");
        let f = write_temp(&toml);
        let err = Config::load(f.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Validation(_)));
    }

    // --- load_with_args ---

    #[test]
    fn load_with_args_uses_c_flag() {
        let f = write_temp(&minimal_toml(""));
        let path = f.path().to_str().unwrap().to_string();
        let args = vec!["prog".to_string(), "-c".to_string(), path];
        let cfg = Config::load_with_args(args.into_iter()).unwrap();
        assert_eq!(cfg.server.host, "127.0.0.1");
    }

    #[test]
    fn load_with_args_c_without_path_returns_cli_error() {
        let args = vec!["prog".to_string(), "-c".to_string()];
        let err = Config::load_with_args(args.into_iter()).unwrap_err();
        assert!(matches!(err, ConfigError::Cli(_)));
    }

    // --- model_registry ---

    #[test]
    fn model_registry_maps_types_correctly() {
        let cfg = DeepSeekConfig::default();
        let registry = cfg.model_registry();
        assert_eq!(
            registry.get("deepseek-default").map(|s| s.as_str()),
            Some("default")
        );
        assert_eq!(
            registry.get("deepseek-expert").map(|s| s.as_str()),
            Some("expert")
        );
    }

    #[test]
    fn model_registry_is_lowercase() {
        let cfg = DeepSeekConfig {
            model_types: vec!["Default".to_string(), "EXPERT".to_string()],
            ..DeepSeekConfig::default()
        };
        let registry = cfg.model_registry();
        assert!(registry.contains_key("deepseek-default"));
        assert!(registry.contains_key("deepseek-expert"));
    }

    // --- 默认值回归 ---

    #[test]
    fn default_client_version_is_not_too_low() {
        // 回归：1.3.0 / 1.7.9 会触发 CLIENT_VERSION_TOO_LOW (code 40005)
        // 必须 >= 2.0.0，此处钉住当前默认值不低于该阈值
        let version = default_client_version();
        let parts: Vec<u32> = version.split('.').filter_map(|s| s.parse().ok()).collect();
        assert!(
            parts[0] >= 2,
            "client_version 默认值 {version} 低于 2.0.0，会触发 CLIENT_VERSION_TOO_LOW"
        );
    }

    #[test]
    fn api_tokens_default_empty() {
        let f = write_temp(&minimal_toml(""));
        let cfg = Config::load(f.path()).unwrap();
        assert!(cfg.server.api_tokens.is_empty());
    }
}
