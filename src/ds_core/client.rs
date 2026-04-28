//! DeepSeek HTTP 客户端 —— 原始 API 调用层
//!
//! 无状态管理：无缓存、无重试、无会话状态。
//! 每个方法对应一个 REST 端点（详见 docs/ds-api-reference.md）。
//! 流方法（completion/edit_message）返回原始字节流，由上层解析 SSE。
//!
//! 仅包含最小业务逻辑：HTTP 错误码和业务错误码解析（into_result）。

use bytes::Bytes;
use futures::{Stream, TryStreamExt};
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use thiserror::Error;

// API 端点常量
const ENDPOINT_USERS_LOGIN: &str = "/users/login";
const ENDPOINT_CHAT_SESSION_CREATE: &str = "/chat_session/create";
const ENDPOINT_CHAT_SESSION_DELETE: &str = "/chat_session/delete";
#[allow(dead_code)]
const ENDPOINT_CHAT_SESSION_UPDATE_TITLE: &str = "/chat_session/update_title";
const ENDPOINT_CHAT_CREATE_POW_CHALLENGE: &str = "/chat/create_pow_challenge";
const ENDPOINT_CHAT_COMPLETION: &str = "/chat/completion";
#[allow(dead_code)]
const ENDPOINT_CHAT_EDIT_MESSAGE: &str = "/chat/edit_message";
const ENDPOINT_CHAT_STOP_STREAM: &str = "/chat/stop_stream";
const ENDPOINT_FILE_UPLOAD: &str = "/file/upload_file";
const ENDPOINT_FILE_FETCH: &str = "/file/fetch_files";

#[derive(Debug, Error)]
pub enum ClientError {
    /// HTTP 层错误（网络、超时、DNS 等）
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// HTTP 状态码非 2xx
    #[error("HTTP status {status}: {body}")]
    Status { status: u16, body: String },

    /// 业务错误：API 返回 HTTP 200 但 biz_code 非 0
    #[error("Business error: code={code}, msg={msg}")]
    Business { code: i64, msg: String },

    /// JSON 解析失败
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    /// Header 值包含非法字符
    #[error("Invalid header value: {0}")]
    InvalidHeader(String),
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    code: i64,
    msg: String,
    data: Option<EnvelopeData<T>>,
}

#[derive(Debug, Deserialize)]
struct EnvelopeData<T> {
    biz_code: i64,
    biz_msg: String,
    biz_data: Option<T>,
}

impl<T: serde::de::DeserializeOwned> Envelope<T> {
    fn into_result(self) -> Result<T, ClientError> {
        if self.code != 0 {
            return Err(ClientError::Business {
                code: self.code,
                msg: self.msg,
            });
        }
        let data = self.data.ok_or_else(|| ClientError::Business {
            code: -1,
            msg: "missing data".into(),
        })?;
        if data.biz_code != 0 {
            return Err(ClientError::Business {
                code: data.biz_code,
                msg: data.biz_msg,
            });
        }
        match data.biz_data {
            Some(t) => Ok(t),
            None => {
                // 允许 biz_data 为 null，尝试从 null 构造 T（仅当 T 是 Option 时成功）
                serde_json::from_value(serde_json::Value::Null).map_err(|_| ClientError::Business {
                    code: -1,
                    msg: "missing biz_data".into(),
                })
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LoginPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mobile: Option<String>,
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area_code: Option<String>,
    pub device_id: String,
    pub os: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginData {
    pub code: i64,
    pub msg: String,
    pub user: UserInfo,
}

#[derive(Debug, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub token: String,
    pub email: Option<String>,
    pub mobile_number: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateSessionData {
    pub id: String,
}

// 包装类型：biz_data 里面嵌套了 chat_session 对象
#[derive(Debug, Deserialize)]
struct CreateSessionWrapper {
    chat_session: CreateSessionData,
}

#[derive(Debug, Deserialize)]
pub struct UploadFileData {
    pub id: String,
    #[allow(dead_code)]
    pub status: String,
    #[allow(dead_code)]
    pub file_name: String,
    #[allow(dead_code)]
    pub file_size: i64,
}

#[derive(Debug, Deserialize)]
pub struct FetchFilesData {
    pub files: Vec<FileInfo>,
}

#[derive(Debug, Deserialize)]
pub struct FileInfo {
    #[allow(dead_code)]
    pub id: String,
    pub status: String,
    pub file_name: String,
    #[allow(dead_code)]
    pub file_size: i64,
    #[serde(default)]
    pub token_usage: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ChallengeData {
    pub algorithm: String,
    pub challenge: String,
    pub salt: String,
    pub signature: String,
    pub difficulty: i64,
    #[allow(dead_code)]
    pub expire_after: i64,
    pub expire_at: i64,
    pub target_path: String,
}

// 包装类型：biz_data 里面嵌套了 challenge 对象
#[derive(Debug, Deserialize)]
struct ChallengeWrapper {
    challenge: ChallengeData,
}

#[derive(Debug, Serialize)]
pub struct CompletionPayload {
    pub chat_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_message_id: Option<i64>,
    pub model_type: String,
    pub prompt: String,
    pub ref_file_ids: Vec<String>,
    pub thinking_enabled: bool,
    pub search_enabled: bool,
    pub preempt: bool,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct EditMessagePayload {
    pub chat_session_id: String,
    pub message_id: i64,
    pub prompt: String,
    pub search_enabled: bool,
    pub thinking_enabled: bool,
    pub model_type: String,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct UpdateTitlePayload {
    pub chat_session_id: String,
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct StopStreamPayload {
    pub chat_session_id: String,
    pub message_id: i64,
}

#[derive(Clone)]
pub struct DsClient {
    http: reqwest::Client,
    api_base: String,
    wasm_url: String,
    user_agent: String,
    client_version: String,
    client_platform: String,
}

impl DsClient {
    pub fn new(
        api_base: String,
        wasm_url: String,
        user_agent: String,
        client_version: String,
        client_platform: String,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_base,
            wasm_url,
            user_agent,
            client_version,
            client_platform,
        }
    }

    fn auth_headers(&self, token: &str) -> Result<reqwest::header::HeaderMap, ClientError> {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_str(&self.user_agent)
                .map_err(|e| ClientError::InvalidHeader(format!("User-Agent: {e}")))?,
        );
        h.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| ClientError::InvalidHeader(format!("Authorization: {e}")))?,
        );
        h.insert(
            "X-Client-Version",
            reqwest::header::HeaderValue::from_str(&self.client_version)
                .map_err(|e| ClientError::InvalidHeader(format!("X-Client-Version: {e}")))?,
        );
        h.insert(
            "X-Client-Platform",
            reqwest::header::HeaderValue::from_str(&self.client_platform)
                .map_err(|e| ClientError::InvalidHeader(format!("X-Client-Platform: {e}")))?,
        );
        Ok(h)
    }

    fn auth_headers_with_pow(
        &self,
        token: &str,
        pow_response: &str,
    ) -> Result<reqwest::header::HeaderMap, ClientError> {
        let mut h = self.auth_headers(token)?;
        h.insert(
            "X-Ds-Pow-Response",
            reqwest::header::HeaderValue::from_str(pow_response)
                .map_err(|e| ClientError::InvalidHeader(format!("X-Ds-Pow-Response: {e}")))?,
        );
        Ok(h)
    }

    async fn parse_envelope<T: serde::de::DeserializeOwned>(
        resp: reqwest::Response,
    ) -> Result<T, ClientError> {
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Status {
                status: status.as_u16(),
                body,
            });
        }
        let envelope: Envelope<T> = resp.json().await?;
        envelope.into_result()
    }

    pub async fn login(&self, payload: &LoginPayload) -> Result<LoginData, ClientError> {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_str(&self.user_agent)
                .map_err(|e| ClientError::InvalidHeader(format!("User-Agent: {e}")))?,
        );
        let resp = self
            .http
            .post(format!("{}{}", self.api_base, ENDPOINT_USERS_LOGIN))
            .headers(h)
            .json(payload)
            .send()
            .await?;
        Self::parse_envelope::<LoginData>(resp).await
    }

    pub async fn create_session(&self, token: &str) -> Result<String, ClientError> {
        let resp = self
            .http
            .post(format!("{}{}", self.api_base, ENDPOINT_CHAT_SESSION_CREATE))
            .headers(self.auth_headers(token)?)
            .json(&serde_json::json!({}))
            .send()
            .await?;
        let wrapper: CreateSessionWrapper = Self::parse_envelope(resp).await?;
        let data = wrapper.chat_session;
        Ok(data.id)
    }

    pub async fn delete_session(&self, token: &str, session_id: &str) -> Result<(), ClientError> {
        let resp = self
            .http
            .post(format!("{}{}", self.api_base, ENDPOINT_CHAT_SESSION_DELETE))
            .headers(self.auth_headers(token)?)
            .json(&serde_json::json!({ "chat_session_id": session_id }))
            .send()
            .await?;
        Self::parse_envelope::<Option<()>>(resp).await?;
        Ok(())
    }

    pub async fn create_pow_challenge(
        &self,
        token: &str,
        target_path: &str,
    ) -> Result<ChallengeData, ClientError> {
        let resp = self
            .http
            .post(format!(
                "{}{}",
                self.api_base, ENDPOINT_CHAT_CREATE_POW_CHALLENGE
            ))
            .headers(self.auth_headers(token)?)
            .json(&serde_json::json!({ "target_path": target_path }))
            .send()
            .await?;
        let wrapper: ChallengeWrapper = Self::parse_envelope(resp).await?;
        let challenge = wrapper.challenge;
        Ok(challenge)
    }

    pub async fn completion(
        &self,
        token: &str,
        pow_response: &str,
        payload: &CompletionPayload,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Bytes, ClientError>> + Send>>, ClientError> {
        let resp = self
            .http
            .post(format!("{}{}", self.api_base, ENDPOINT_CHAT_COMPLETION))
            .headers(self.auth_headers_with_pow(token, pow_response)?)
            .json(payload)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Status {
                status: status.as_u16(),
                body,
            });
        }

        Ok(Box::pin(resp.bytes_stream().map_err(ClientError::Http)))
    }

    #[allow(dead_code)]
    pub async fn edit_message(
        &self,
        token: &str,
        pow_response: &str,
        payload: &EditMessagePayload,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Bytes, ClientError>> + Send>>, ClientError> {
        let resp = self
            .http
            .post(format!("{}{}", self.api_base, ENDPOINT_CHAT_EDIT_MESSAGE))
            .headers(self.auth_headers_with_pow(token, pow_response)?)
            .json(payload)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Status {
                status: status.as_u16(),
                body,
            });
        }

        Ok(Box::pin(resp.bytes_stream().map_err(ClientError::Http)))
    }

    #[allow(dead_code)]
    pub async fn update_title(
        &self,
        token: &str,
        payload: &UpdateTitlePayload,
    ) -> Result<(), ClientError> {
        let resp = self
            .http
            .post(format!(
                "{}{}",
                self.api_base, ENDPOINT_CHAT_SESSION_UPDATE_TITLE
            ))
            .headers(self.auth_headers(token)?)
            .json(payload)
            .send()
            .await?;
        Self::parse_envelope::<serde::de::IgnoredAny>(resp).await?;
        Ok(())
    }

    /// 取消正在进行的流式输出，不需要 PoW
    pub async fn stop_stream(
        &self,
        token: &str,
        payload: &StopStreamPayload,
    ) -> Result<(), ClientError> {
        let resp = self
            .http
            .post(format!("{}{}", self.api_base, ENDPOINT_CHAT_STOP_STREAM))
            .headers(self.auth_headers(token)?)
            .json(payload)
            .send()
            .await?;
        Self::parse_envelope::<Option<()>>(resp).await?;
        Ok(())
    }

    /// 上传文件，返回文件元数据（id, status 等）
    pub async fn upload_file(
        &self,
        token: &str,
        pow_response: &str,
        filename: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<UploadFileData, ClientError> {
        let part = Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str(content_type)?;
        let form = Form::new().part("file", part);

        let resp = self
            .http
            .post(format!("{}{}", self.api_base, ENDPOINT_FILE_UPLOAD))
            .headers(self.auth_headers_with_pow(token, pow_response)?)
            .multipart(form)
            .send()
            .await?;
        Self::parse_envelope::<UploadFileData>(resp).await
    }

    /// 查询文件状态，返回文件列表（含 status: PENDING/SUCCESS/FAILED）
    pub async fn fetch_files(
        &self,
        token: &str,
        file_ids: &[String],
    ) -> Result<FetchFilesData, ClientError> {
        let ids = file_ids.join(",");
        let resp = self
            .http
            .get(format!("{}{}", self.api_base, ENDPOINT_FILE_FETCH))
            .headers(self.auth_headers(token)?)
            .query(&[("file_ids", &ids)])
            .send()
            .await?;
        Self::parse_envelope::<FetchFilesData>(resp).await
    }

    pub async fn get_wasm(&self) -> Result<Bytes, ClientError> {
        let resp = self.http.get(&self.wasm_url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Status {
                status: status.as_u16(),
                body,
            });
        }
        Ok(resp.bytes().await?)
    }
}
