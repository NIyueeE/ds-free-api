//! 对话请求编排 —— create_session → upload → PoW → completion → delete_session
//!
//! 每次请求创建新 session，结束后立即清理。历史对话通过文件上传传递。

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use pin_project_lite::pin_project;

use crate::ds_core::CoreError;
use crate::ds_core::accounts::{AccountGuard, AccountPool};
use crate::ds_core::client::{CompletionPayload, DsClient, StopStreamPayload};
use crate::ds_core::pow::PowSolver;

pub(crate) struct ActiveSession {
    pub(crate) token: String,
    pub(crate) session_id: String,
    pub(crate) message_id: i64,
}

const TAG_START: &str = "<｜";
const TAG_END: &str = "｜>";
const SESSION_HISTORY_FILE: &str = "EMPTY.txt";
const UPLOAD_POLL_INTERVAL_MS: u64 = 2000;
const UPLOAD_POLL_MAX_RETRIES: usize = 30; // 60s 总超时

#[derive(Debug, Clone)]
pub struct FilePayload {
    pub filename: String,
    pub content: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub prompt: String,
    pub thinking_enabled: bool,
    pub search_enabled: bool,
    pub model_type: String,
    pub files: Vec<FilePayload>,
}

/// v0_chat 返回值：SSE 字节流 + 账号标识
pub struct ChatResponse {
    pub stream: Pin<Box<dyn Stream<Item = Result<Bytes, CoreError>> + Send>>,
    pub account_id: String,
}

pin_project! {
    pub struct GuardedStream<S> {
        #[pin]
        stream: S,
        _guard: AccountGuard,
        client: DsClient,
        token: String,
        session_id: String,
        message_id: i64,
        finished: bool,
        sessions: Arc<Mutex<HashMap<String, ActiveSession>>>,
    }

    impl<S> PinnedDrop for GuardedStream<S> {
        fn drop(this: Pin<&mut Self>) {
            let this = this.project();
            let client = this.client.clone();
            let token = this.token.clone();
            let session_id = this.session_id.clone();
            let message_id = *this.message_id;
            let finished = *this.finished;
            let sessions = this.sessions.clone();

            // 从活跃 session 追踪中移除
            sessions.lock().unwrap().remove(&session_id);

            tokio::spawn(async move {
                // 流未自然结束时通知服务端停止生成
                if !finished {
                    let payload = StopStreamPayload {
                        chat_session_id: session_id.clone(),
                        message_id,
                    };
                    if let Err(e) = client.stop_stream(&token, &payload).await {
                        log::warn!(target: "ds_core::accounts", "stop_stream 失败: {}", e);
                    }
                }
                // 无论流是否完成，都清理临时 session
                if let Err(e) = client.delete_session(&token, &session_id).await {
                    log::warn!(target: "ds_core::accounts", "delete_session 失败: {}", e);
                }
            });
        }
    }
}

impl<S> GuardedStream<S> {
    pub fn new(
        stream: S,
        guard: AccountGuard,
        client: DsClient,
        token: String,
        session_id: String,
        message_id: i64,
        sessions: Arc<Mutex<HashMap<String, ActiveSession>>>,
    ) -> Self {
        Self {
            stream,
            _guard: guard,
            client,
            token,
            session_id,
            message_id,
            finished: false,
            sessions,
        }
    }
}

impl<S, E> Stream for GuardedStream<S>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::fmt::Display,
{
    type Item = Result<Bytes, CoreError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        match this.stream.poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(bytes))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(CoreError::Stream(e.to_string())))),
            Poll::Ready(None) => {
                *this.finished = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

pub struct Completions {
    client: DsClient,
    solver: PowSolver,
    pool: AccountPool,
    active_sessions: Arc<Mutex<HashMap<String, ActiveSession>>>,
}

impl Completions {
    pub fn new(client: DsClient, solver: PowSolver, pool: AccountPool) -> Self {
        Self {
            client,
            solver,
            pool,
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn v0_chat(
        &self,
        req: ChatRequest,
        request_id: &str,
    ) -> Result<ChatResponse, CoreError> {
        // 1. 获取空闲账号
        let guard = self.pool.get_account().ok_or_else(|| {
            log::warn!(
                target: "ds_core::accounts",
                "req={} 账号池无可用账号", request_id
            );
            CoreError::Overloaded
        })?;

        let account = guard.account();
        let account_id = account.display_id().to_string();
        let token = account.token().to_string();

        log::debug!(
            target: "ds_core::accounts",
            "req={} 分配账号: model_type={}, token={}..{}",
            request_id, req.model_type,
            &account_id[..4.min(account_id.len())],
            &account_id[account_id.len().saturating_sub(4)..]
        );

        // 2. 拆分历史（支持 ChatML 和非 ChatML 格式）
        let (inline_prompt, history_content) = split_history_prompt(&req.prompt);

        if !history_content.is_empty() {
            log::debug!(
                target: "ds_core::accounts",
                "req={} 触发历史拆分, history_size={}", request_id, history_content.len()
            );
        }

        // 3. 创建临时 session
        let session_id = self.client.create_session(&token).await?;
        log::debug!(
            target: "ds_core::accounts",
            "req={} 创建 session: id={}", request_id, session_id
        );

        // 4. 上传文件：先历史文件，再外部文件（对话阅读顺序）
        let mut ref_file_ids: Vec<String> = Vec::new();
        // 历史文件上传失败时退回到完整 prompt 内联发送
        let mut history_upload_failed = false;

        if !history_content.is_empty() {
            match self
                .upload_and_poll(
                    &token,
                    SESSION_HISTORY_FILE,
                    "text/plain",
                    history_content.as_bytes(),
                    request_id,
                )
                .await
            {
                Ok(file_id) => ref_file_ids.push(file_id),
                Err(e) => {
                    log::warn!(
                        target: "ds_core::accounts",
                        "req={} 历史文件上传失败，退回内联发送: {}", request_id, e
                    );
                    history_upload_failed = true;
                }
            }
        }

        for file in &req.files {
            match self
                .upload_and_poll(
                    &token,
                    &file.filename,
                    &file.content_type,
                    &file.content,
                    request_id,
                )
                .await
            {
                Ok(file_id) => ref_file_ids.push(file_id),
                Err(e) => {
                    log::warn!(
                        target: "ds_core::accounts",
                        "req={} 外部文件上传失败 ({}): {}", request_id, file.filename, e
                    );
                    return Err(CoreError::ProviderError(format!(
                        "外部文件上传失败 ({}): {}",
                        file.filename, e
                    )));
                }
            }
        }

        // 5. 计算 PoW（completion 专用）
        let pow_header = self
            .compute_pow_for_target(&token, "/api/v0/chat/completion")
            .await?;
        log::debug!(
            target: "ds_core::accounts",
            "req={} completion PoW 计算完成", request_id
        );

        // 6. 发起 completion（历史文件上传失败时退回到完整 prompt 内联发送）
        let completion_prompt = if history_upload_failed {
            &req.prompt
        } else {
            &inline_prompt
        };

        log::trace!(
            target: "ds_core::accounts",
            "req={} completion 请求: ref_file_ids={:?}, history_fallback={}, prompt=\n{}\n---历史文件内容---\n{}",
            request_id, ref_file_ids, history_upload_failed, completion_prompt, history_content
        );

        let payload = CompletionPayload {
            chat_session_id: session_id.clone(),
            parent_message_id: None,
            model_type: req.model_type.clone(),
            prompt: completion_prompt.clone(),
            ref_file_ids,
            thinking_enabled: req.thinking_enabled,
            search_enabled: req.search_enabled,
            preempt: false,
        };

        let mut raw_stream = self
            .client
            .completion(&token, &pow_header, &payload)
            .await?;

        // 7. 收集字节直到拿到前两个 SSE 事件（ready + hint/update_session）
        let mut buf = Vec::new();
        let mut text_buf = String::new();
        let (ready_block, second_block) = loop {
            let chunk = raw_stream
                .next()
                .await
                .ok_or_else(|| {
                    let raw = String::from_utf8_lossy(&buf);
                    log::error!(
                        target: "ds_core::accounts",
                        "req={} 空 SSE 流, 已收到 {} 字节: {}", request_id, buf.len(), raw
                    );
                    CoreError::Stream(format!("空 SSE 流 (已收到 {} 字节)", buf.len()))
                })?
                .map_err(|e| CoreError::Stream(e.to_string()))?;
            log::trace!(
                target: "ds_core::accounts",
                "req={} <<< ({} bytes) {}", request_id, chunk.len(), String::from_utf8_lossy(&chunk)
            );
            buf.extend_from_slice(&chunk);
            text_buf.push_str(&String::from_utf8_lossy(&chunk));

            if let Some((first, second)) = split_two_events(&text_buf) {
                break (first.to_owned(), second.to_owned());
            }
        };

        let (_, stop_id) = parse_ready_message_ids(ready_block.as_bytes());

        // 8. 检查 hint 事件（rate_limit / input_exceeds_limit）
        if let Some(err) = check_hint(&second_block) {
            if let CoreError::Overloaded = &err {
                log::warn!(
                    target: "ds_core::accounts",
                    "req={} hint 限流: rate_limit_reached", request_id
                );
            } else {
                let hint_detail = second_block
                    .lines()
                    .find_map(|l| l.strip_prefix("data: "))
                    .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
                    .and_then(|v| {
                        v.get("content")
                            .or(v.get("finish_reason"))
                            .and_then(|c| c.as_str().map(String::from))
                    })
                    .unwrap_or_else(|| "(unknown)".into());
                log::warn!(
                    target: "ds_core::accounts",
                    "req={} hint 错误: {}", request_id, hint_detail
                );
            }
            let _ = self.client.delete_session(&token, &session_id).await;
            log::debug!(
                target: "ds_core::accounts",
                "req={} hint 后清理 session: id={}", request_id, session_id
            );
            return Err(err);
        }

        log::debug!(
            target: "ds_core::accounts",
            "req={} SSE ready: resp_msg={}", request_id, stop_id
        );

        // 9. 注册活跃 session（含 message_id 用于 stop_stream）
        {
            let mut map = self.active_sessions.lock().unwrap();
            map.insert(
                session_id.clone(),
                ActiveSession {
                    token: token.clone(),
                    session_id: session_id.clone(),
                    message_id: stop_id,
                },
            );
        }

        // 10. 用原始 buf 重建流（包含已消耗的 chunk）
        let stream =
            futures::stream::once(futures::future::ready(Ok(Bytes::from(buf)))).chain(raw_stream);

        Ok(ChatResponse {
            stream: Box::pin(GuardedStream::new(
                Box::pin(stream),
                guard,
                self.client.clone(),
                token,
                session_id,
                stop_id,
                self.active_sessions.clone(),
            )),
            account_id,
        })
    }

    async fn compute_pow_for_target(
        &self,
        token: &str,
        target_path: &str,
    ) -> Result<String, CoreError> {
        let challenge_data = self.client.create_pow_challenge(token, target_path).await?;
        let result = self.solver.solve(&challenge_data).map_err(|e| {
            log::warn!(target: "ds_core::accounts", "PoW 计算失败: {}", e);
            CoreError::ProofOfWorkFailed(e)
        })?;
        Ok(result.to_header())
    }

    /// 上传文件并轮询直到 SUCCESS 或超时
    async fn upload_and_poll(
        &self,
        token: &str,
        filename: &str,
        content_type: &str,
        content: &[u8],
        request_id: &str,
    ) -> Result<String, CoreError> {
        let pow_header = self
            .compute_pow_for_target(token, "/api/v0/file/upload_file")
            .await?;

        let upload_data = self
            .client
            .upload_file(token, &pow_header, filename, content_type, content.to_vec())
            .await?;
        let file_id = upload_data.id;

        for _ in 0..UPLOAD_POLL_MAX_RETRIES {
            let fetch_data = self
                .client
                .fetch_files(token, std::slice::from_ref(&file_id))
                .await?;
            if let Some(file) = fetch_data.files.first() {
                match file.status.as_str() {
                    "SUCCESS" => {
                        log::debug!(
                            target: "ds_core::accounts",
                            "req={} 文件上传成功: file_id={}, tokens={:?}, name={}",
                            request_id, file_id, file.token_usage, file.file_name
                        );
                        return Ok(file_id);
                    }
                    "FAILED" => {
                        return Err(CoreError::ProviderError(format!(
                            "文件上传失败: {}",
                            file.file_name
                        )));
                    }
                    _ => {} // PENDING，继续轮询
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(UPLOAD_POLL_INTERVAL_MS)).await;
        }
        Err(CoreError::ProviderError("文件处理超时".into()))
    }

    pub fn account_statuses(&self) -> Vec<crate::ds_core::accounts::AccountStatus> {
        self.pool.account_statuses()
    }

    /// 优雅关闭：清理所有残留的活跃 session
    pub async fn shutdown(&self) {
        let sessions = {
            let mut map = self.active_sessions.lock().unwrap();
            std::mem::take(&mut *map)
        };

        if sessions.is_empty() {
            self.pool.shutdown(&self.client).await;
            return;
        }

        log::info!(
            target: "ds_core::accounts",
            "shutdown: 清理 {} 个残留 session", sessions.len()
        );

        use futures::future::join_all;
        let futures: Vec<_> = sessions
            .into_values()
            .map(|s| {
                let client = self.client.clone();
                async move {
                    let payload = StopStreamPayload {
                        chat_session_id: s.session_id.clone(),
                        message_id: s.message_id,
                    };
                    let _ = client.stop_stream(&s.token, &payload).await;
                    let _ = client
                        .delete_session(&s.token, &s.session_id)
                        .await
                        .inspect_err(|e| {
                            log::warn!(
                                target: "ds_core::accounts",
                                "shutdown 清理 session {} 失败: {}",
                                s.session_id, e
                            );
                        });
                }
            })
            .collect();
        join_all(futures).await;

        self.pool.shutdown(&self.client).await;
    }
}

// ── ChatML 解析与历史拆分 ──────────────────────────────────────────────

struct ChatBlock {
    role: String,
    content: String,
}

fn role_tag(role: &str) -> String {
    let mut r = role.to_string();
    if let Some(c) = r.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    format!("<｜{}｜>", r)
}

/// 解析 DeepSeek 原生标签格式的 prompt 为结构化块
///
/// 格式: `<｜Role｜>content\n`（无闭合标签），内容截止到下一个 `<｜` 或字符串末尾。
fn parse_native_blocks(prompt: &str) -> Vec<ChatBlock> {
    let mut blocks = Vec::new();
    let mut pos = 0;
    while let Some(start_idx) = prompt[pos..].find(TAG_START) {
        let abs_start = pos + start_idx;
        let role_start = abs_start + TAG_START.len();
        let role_end = match prompt[role_start..].find(TAG_END) {
            Some(i) => role_start + i,
            None => break,
        };
        let role = prompt[role_start..role_end].trim().to_lowercase();
        let content_start = role_end + TAG_END.len();
        let content_end = match prompt[content_start..].find(TAG_START) {
            Some(i) => content_start + i,
            None => prompt.len(),
        };
        let content = prompt[content_start..content_end]
            .trim_end_matches('\n')
            .to_string();
        blocks.push(ChatBlock { role, content });
        pos = content_end;
    }
    blocks
}

/// 拆分 prompt 为 inline_prompt 和 history_content
///
/// 找到最后一个 user/tool 块，以其为界：
/// - inline = 最后一个 user/tool 块 → 末尾（含其后的 assistant/<think> 等）
/// - history = 其余所有块，包装为 [file content end] … [file content begin] 格式上传
fn split_history_prompt(prompt: &str) -> (String, String) {
    let blocks = parse_native_blocks(prompt);
    let split_idx = match blocks
        .iter()
        .rposition(|b| b.role == "user" || b.role == "tool")
    {
        Some(i) if i > 0 => i,
        _ => return (prompt.to_string(), String::new()),
    };

    let mut inline = String::new();
    for block in &blocks[split_idx..] {
        inline.push_str(&role_tag(&block.role));
        inline.push_str(&block.content);
        inline.push('\n');
    }

    let mut history = String::new();
    history.push_str("[file content end]\n\n");
    for block in &blocks[..split_idx] {
        history.push_str(&role_tag(&block.role));
        history.push_str(&block.content);
        history.push('\n');
    }
    history.push_str("[file name]: IGNORE\n[file content begin]\n");

    (inline, history)
}

// ── SSE 解析辅助 ──────────────────────────────────────────────────────

/// 从字符串中提取前两个完整 SSE 事件块
fn split_two_events(buf: &str) -> Option<(&str, &str)> {
    let parts: Vec<&str> = buf.splitn(3, "\n\n").collect();
    if parts.len() < 3 {
        return None;
    }
    Some((parts[0], parts[1]))
}

/// 检查 hint 事件，返回错误（rate_limit → Overloaded, input_exceeds_limit → ProviderError）
fn check_hint(event_block: &str) -> Option<CoreError> {
    let is_hint = event_block.lines().any(|l| {
        l.trim()
            .strip_prefix("event:")
            .is_some_and(|v| v.trim() == "hint")
    });
    if !is_hint {
        return None;
    }
    if event_block.contains("rate_limit") {
        return Some(CoreError::Overloaded);
    }
    if event_block.contains("input_exceeds_limit") {
        return Some(CoreError::ProviderError(
            "输入内容超长，请缩短后重试".into(),
        ));
    }
    None
}

/// 从第一个 SSE ready 事件中解析 request/response_message_id
///
/// 格式: `event: ready\ndata: {"request_message_id":1,"response_message_id":2,...}\n\n`
///
/// 返回 `(request_msg_id, response_msg_id)`，未找到时兜底为 `(1, 2)`
fn parse_ready_message_ids(chunk: &[u8]) -> (i64, i64) {
    let text = std::str::from_utf8(chunk).ok();
    if let Some(text) = text {
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data: ")
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(data)
                && let (Some(r), Some(s)) = (
                    val.get("request_message_id").and_then(|v| v.as_i64()),
                    val.get("response_message_id").and_then(|v| v.as_i64()),
                )
            {
                return (r, s);
            }
        }
    }
    (1, 2)
}
