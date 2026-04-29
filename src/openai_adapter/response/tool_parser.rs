//! 工具调用解析 —— 滑动窗口检测 `<tool_calls>...</tool_calls>`，转换为结构化 tool_calls
//!
//! 算法核心：
//! - Detecting 状态：维护固定宽度 W 的扫描缓冲区，新 chunk 到来时
//!   先追加到缓冲区，扫描 `<tool_calls>`（或回退 `<tool_call>`），未找到则释放超出 W 的安全部分
//! - CollectingXml 状态：检测到标记后收集内容直到 `</tool_calls>`
//! - Done 状态：工具调用已发出，截断后续内容（防幻觉）

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use futures::Stream;
use pin_project_lite::pin_project;

use log::{debug, warn};

use crate::openai_adapter::OpenAIAdapterError;
use crate::openai_adapter::types::{
    ChatCompletionsResponseChunk, ChunkChoice, Delta, FunctionCall, ToolCall,
};

static CALL_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
pub(crate) const MAX_XML_BUF_LEN: usize = 64 * 1024;

/// 工具调用开始标记（主：带 s）
pub(crate) const TOOL_CALL_START: &str = "<tool_calls>";
/// 工具调用结束标记（主）
pub(crate) const TOOL_CALL_END: &str = "</tool_calls>";
/// 滑动扫描窗口大小（安全上界：64 字节足够覆盖所有自定义标签）
const W: usize = 71; // 64 + 7

/// 工具调用标签配置（主标签 `<tool_calls>` 硬编码，此处只包含回退/自定义标签）
#[derive(Debug, Clone)]
pub struct TagConfig {
    pub starts: Vec<String>,
    pub ends: Vec<String>,
}

impl TagConfig {
    /// 从配置文件加载回退标签
    pub fn from_config(cfg: &crate::config::ToolCallTagConfig) -> Self {
        Self {
            starts: cfg.extra_starts.clone(),
            ends: cfg.extra_ends.clone(),
        }
    }
}

pub(crate) fn contains_start_tag_with(s: &str, cfg: &TagConfig) -> bool {
    s.contains(TOOL_CALL_START) || cfg.starts.iter().any(|t| s.contains(t.as_str()))
}

pub(crate) fn find_start_tag_with<'a>(s: &'a str, cfg: &TagConfig) -> Option<(usize, &'a str)> {
    if let Some(pos) = s.find(TOOL_CALL_START) {
        return Some((pos, &s[pos..pos + TOOL_CALL_START.len()]));
    }
    for start in &cfg.starts {
        if let Some(pos) = s.find(start.as_str()) {
            return Some((pos, &s[pos..pos + start.len()]));
        }
    }
    None
}

pub(crate) fn find_end_tag_with<'a>(s: &'a str, cfg: &TagConfig) -> Option<(usize, &'a str)> {
    if let Some(pos) = s.find(TOOL_CALL_END) {
        return Some((pos, &s[pos..pos + TOOL_CALL_END.len()]));
    }
    for end in &cfg.ends {
        if let Some(pos) = s.find(end.as_str()) {
            return Some((pos, &s[pos..pos + end.len()]));
        }
    }
    None
}

fn next_call_id() -> String {
    let n = CALL_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("call_{:016x}", n)
}

/// 返回不超过 `max` 的最大 UTF-8 字符边界偏移
fn floor_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut i = max;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// 检查指定位置之前是否处于未闭合的 markdown 代码块中
fn is_inside_code_fence(xml: &str, tag_pos: usize) -> bool {
    let before = &xml[..tag_pos];
    before.matches("```").count() % 2 == 1
}

/// 修复 JSON 中无效的反斜杠转义序列
fn repair_invalid_backslashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(&next)
                    if matches!(next, '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u') =>
                {
                    out.push('\\');
                    out.push(next);
                    chars.next();
                }
                Some(&next) => {
                    out.push('\\');
                    out.push('\\');
                    out.push(next);
                    chars.next();
                }
                None => {
                    out.push('\\');
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// 修复 JSON 中未加引号的 key
fn repair_unquoted_keys(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 32);
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if (chars[i] == '{' || chars[i] == ',') && i + 1 < len {
            out.push(chars[i]);
            i += 1;
            while i < len && chars[i].is_whitespace() {
                out.push(chars[i]);
                i += 1;
            }
            if i < len && (chars[i].is_alphabetic() || chars[i] == '_') {
                let key_start = i;
                while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                if i < len && chars[i] == ':' {
                    out.push('"');
                    out.extend(&chars[key_start..i]);
                    out.push('"');
                } else {
                    out.extend(&chars[key_start..i]);
                    continue;
                }
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// 对 JSON 字符串依次尝试修复：无效转义 → 未引号 key
fn repair_json(s: &str) -> Option<String> {
    let step1 = repair_invalid_backslashes(s);
    if serde_json::from_str::<serde_json::Value>(&step1).is_ok() {
        return Some(step1);
    }
    let step2 = repair_unquoted_keys(&step1);
    if serde_json::from_str::<serde_json::Value>(&step2).is_ok() {
        return Some(step2);
    }
    None
}

/// 解析 `<tool_calls>...</tool_calls>` 中的 JSON 数组，返回结构化 ToolCall 列表
///
/// 标记内格式为 JSON 数组：
/// `<tool_calls>[{"name": "get_weather", "arguments": {"city": "北京"}}]</tool_calls>`
/// 兼容自定义回退标签。
pub fn parse_tool_calls(xml: &str) -> Option<(Vec<ToolCall>, String)> {
    parse_tool_calls_with(xml, &TagConfig::from_config(&Default::default()))
}

pub fn parse_tool_calls_with(xml: &str, cfg: &TagConfig) -> Option<(Vec<ToolCall>, String)> {
    let (start, start_tag) = find_start_tag_with(xml, cfg)?;
    let after_start = start + start_tag.len();

    if is_inside_code_fence(xml, start) {
        return None;
    }

    let (end, inner_end) = match find_end_tag_with(xml, cfg) {
        Some((pos, matched_end)) => (pos + matched_end.len(), pos),
        None => (xml.len(), xml.len()),
    };
    let inner = &xml[after_start..inner_end];

    let arr: Vec<serde_json::Value> = match inner.find('[') {
        Some(arr_start) => {
            let arr_end = inner.rfind(']')? + 1;
            let json_str = &inner[arr_start..arr_end];
            let arr: Option<Vec<serde_json::Value>> = serde_json::from_str(json_str).ok();
            arr.or_else(|| {
                let repaired = repair_json(json_str)?;
                serde_json::from_str(&repaired).ok()
            })?
        }
        None => {
            let obj_start = inner.find('{')?;
            let obj_end = inner.rfind('}')? + 1;
            let json_str = &inner[obj_start..obj_end];
            let obj: Option<serde_json::Value> = serde_json::from_str(json_str)
                .ok()
                .filter(|v: &serde_json::Value| v.is_object());
            let obj = obj.or_else(|| {
                let repaired = repair_json(json_str)?;
                serde_json::from_str(&repaired).ok()
            })?;
            vec![obj]
        }
    };

    let mut calls = Vec::new();
    for item in arr {
        let name = item.get("name")?.as_str()?.to_string();
        let arguments = match item.get("arguments") {
            Some(v) => {
                if let Some(s) = v.as_str() {
                    serde_json::from_str::<serde_json::Value>(s)
                        .ok()
                        .and_then(|obj| serde_json::to_string(&obj).ok())
                        .unwrap_or_else(|| s.to_string())
                } else {
                    serde_json::to_string(v).unwrap_or_else(|_| "{}".into())
                }
            }
            None => "{}".into(),
        };
        calls.push(ToolCall {
            id: next_call_id(),
            ty: "function".to_string(),
            function: Some(FunctionCall { name, arguments }),
            custom: None,
            index: calls.len() as u32,
        });
    }

    if calls.is_empty() {
        return None;
    }

    let remaining = xml[..start].to_string() + &xml[end..];
    Some((calls, remaining))
}

fn make_end_chunk(
    model: &str,
    delta: Delta,
    finish_reason: &'static str,
) -> ChatCompletionsResponseChunk {
    ChatCompletionsResponseChunk {
        id: "chatcmpl-end".to_string(),
        object: "chat.completion.chunk",
        created: 0,
        model: model.to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta,
            finish_reason: Some(finish_reason),
            logprobs: None,
        }],
        usage: None,
        service_tier: None,
        system_fingerprint: None,
    }
}

#[derive(Debug)]
enum ToolParseState {
    Detecting { buffer: String },
    CollectingXml(String),
    Done,
}

pin_project! {
    // 在 content delta 中检测并解析 `<tool_call>` 的流转换器
    //
    // 使用固定宽度 W 的滑动窗口：新内容进入缓冲区，扫描后再释放安全部分，
    // 确保 `<tool_call>` 碎片不会溢出窗口。检测到标记后收集完整内容，
    // 解析为结构化 tool_calls 并发出。
    pub struct ToolCallStream<S> {
        #[pin]
        inner: S,
        state: ToolParseState,
        model: String,
        finish_emitted: bool,
        repair_pending: Option<String>,
        tag_config: Arc<TagConfig>,
    }
}

impl<S> ToolCallStream<S> {
    pub fn new(inner: S, model: String, tag_config: Arc<TagConfig>) -> Self {
        Self {
            inner,
            state: ToolParseState::Detecting {
                buffer: String::new(),
            },
            model,
            finish_emitted: false,
            repair_pending: None,
            tag_config,
        }
    }
}

impl<S> Stream for ToolCallStream<S>
where
    S: Stream<Item = Result<ChatCompletionsResponseChunk, OpenAIAdapterError>>,
{
    type Item = Result<ChatCompletionsResponseChunk, OpenAIAdapterError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if let Some(tool_text) = this.repair_pending.take() {
            debug!(target: "adapter", "tool_parser 发出修复请求");
            return Poll::Ready(Some(Err(OpenAIAdapterError::ToolCallRepairNeeded(
                tool_text,
            ))));
        }

        loop {
            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(mut chunk))) => {
                    let choice = match chunk.choices.first_mut() {
                        Some(c) => c,
                        None => return Poll::Ready(Some(Ok(chunk))),
                    };

                    if let Some(content) = choice.delta.content.take() {
                        if content.is_empty() {
                            choice.delta.content = Some(content);
                            return Poll::Ready(Some(Ok(chunk)));
                        }

                        match &mut this.state {
                            ToolParseState::Detecting { buffer } => {
                                buffer.push_str(&content);

                                if let Some((pos, matched_start)) =
                                    find_start_tag_with(buffer, this.tag_config)
                                {
                                    debug!(
                                        target: "adapter",
                                        "tool_parser 检测到 {}，缓冲区大小={}",
                                        matched_start,
                                        buffer.len()
                                    );
                                    let before = buffer[..pos].to_string();
                                    let rest = std::mem::take(buffer)[pos..].to_string();

                                    if let Some((end_pos, matched_end)) =
                                        find_end_tag_with(&rest, this.tag_config)
                                    {
                                        let end_abs = end_pos + matched_end.len();
                                        let collected = &rest[..end_abs];

                                        if let Some((calls, _)) = parse_tool_calls(collected) {
                                            debug!(
                                                target: "adapter",
                                                "tool_parser 解析出 {} 个工具调用",
                                                calls.len()
                                            );
                                            choice.delta.content = if before.is_empty() {
                                                None
                                            } else {
                                                Some(before)
                                            };
                                            choice.delta.tool_calls = Some(calls);
                                            if choice.finish_reason == Some("stop") {
                                                choice.finish_reason = Some("tool_calls");
                                            }
                                            *this.state = ToolParseState::Done;
                                        } else {
                                            warn!(
                                                target: "adapter",
                                                "tool_parser 解析失败→请求修复"
                                            );
                                            let collected = collected.to_string();
                                            if before.is_empty() {
                                                return Poll::Ready(Some(Err(
                                                    OpenAIAdapterError::ToolCallRepairNeeded(
                                                        collected,
                                                    ),
                                                )));
                                            }
                                            choice.delta.content = Some(before);
                                            *this.repair_pending = Some(collected);
                                            return Poll::Ready(Some(Ok(chunk)));
                                        }
                                        return Poll::Ready(Some(Ok(chunk)));
                                    }

                                    if before.is_empty() {
                                        *this.state = ToolParseState::CollectingXml(rest);
                                        continue;
                                    }
                                    choice.delta.content = Some(before);
                                    *this.state = ToolParseState::CollectingXml(rest);
                                    return Poll::Ready(Some(Ok(chunk)));
                                } else {
                                    let safe =
                                        floor_char_boundary(buffer, buffer.len().saturating_sub(W));
                                    if safe > 0 {
                                        choice.delta.content = Some(buffer[..safe].to_string());
                                        buffer.drain(..safe);
                                        return Poll::Ready(Some(Ok(chunk)));
                                    }
                                    continue;
                                }
                            }

                            ToolParseState::CollectingXml(buf) => {
                                buf.push_str(&content);
                                if buf.len() > MAX_XML_BUF_LEN {
                                    debug!(
                                        target: "adapter",
                                        "tool_parser 缓冲超限，回退纯文本"
                                    );
                                    let flushed = std::mem::take(buf);
                                    *this.state = ToolParseState::Detecting {
                                        buffer: String::new(),
                                    };
                                    choice.delta.content = Some(flushed);
                                    return Poll::Ready(Some(Ok(chunk)));
                                }
                                if let Some((end_pos, end_tag)) =
                                    find_end_tag_with(buf, this.tag_config)
                                {
                                    let end_abs = end_pos + end_tag.len();
                                    let collected = buf[..end_abs].to_string();
                                    let _tail = buf.split_off(end_abs);

                                    if let Some((calls, _)) = parse_tool_calls(&collected) {
                                        debug!(
                                            target: "adapter",
                                            "tool_parser 解析出 {} 个工具调用",
                                            calls.len()
                                        );
                                        choice.delta.content = None;
                                        choice.delta.tool_calls = Some(calls);
                                        if choice.finish_reason == Some("stop") {
                                            choice.finish_reason = Some("tool_calls");
                                        }
                                        *this.state = ToolParseState::Done;
                                    } else {
                                        warn!(
                                            target: "adapter",
                                            "tool_parser 解析失败→请求修复"
                                        );
                                        return Poll::Ready(Some(Err(
                                            OpenAIAdapterError::ToolCallRepairNeeded(collected),
                                        )));
                                    }
                                    return Poll::Ready(Some(Ok(chunk)));
                                }
                                continue;
                            }

                            ToolParseState::Done => {
                                if !*this.finish_emitted {
                                    *this.finish_emitted = true;
                                    let chunk =
                                        make_end_chunk(this.model, Delta::default(), "tool_calls");
                                    return Poll::Ready(Some(Ok(chunk)));
                                }
                                return Poll::Ready(None);
                            }
                        }
                    } else {
                        match &mut this.state {
                            ToolParseState::Detecting { buffer } => {
                                if choice.finish_reason.is_some() {
                                    if !buffer.is_empty() {
                                        choice.delta.content = Some(std::mem::take(buffer));
                                    }
                                    return Poll::Ready(Some(Ok(chunk)));
                                }
                                return Poll::Ready(Some(Ok(chunk)));
                            }

                            ToolParseState::CollectingXml(buf) => {
                                if choice.finish_reason.is_some() {
                                    let flushed = std::mem::take(buf);
                                    if let Some((calls, _)) = parse_tool_calls(&flushed) {
                                        debug!(
                                            target: "adapter",
                                            "tool_parser 流结束时解析出 {} 个工具调用",
                                            calls.len()
                                        );
                                        choice.delta.tool_calls = Some(calls);
                                        if choice.finish_reason == Some("stop") {
                                            choice.finish_reason = Some("tool_calls");
                                        }
                                    } else {
                                        warn!(
                                            target: "adapter",
                                            "tool_parser finish→请求修复"
                                        );
                                        *this.state = ToolParseState::Done;
                                        return Poll::Ready(Some(Err(
                                            OpenAIAdapterError::ToolCallRepairNeeded(flushed),
                                        )));
                                    }
                                    *this.state = ToolParseState::Done;
                                    return Poll::Ready(Some(Ok(chunk)));
                                }
                                return Poll::Ready(Some(Ok(chunk)));
                            }

                            ToolParseState::Done => {
                                if !*this.finish_emitted {
                                    *this.finish_emitted = true;
                                    let chunk =
                                        make_end_chunk(this.model, Delta::default(), "tool_calls");
                                    return Poll::Ready(Some(Ok(chunk)));
                                }
                                return Poll::Ready(None);
                            }
                        }
                    }
                }
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
                Poll::Ready(None) => match std::mem::replace(this.state, ToolParseState::Done) {
                    ToolParseState::Detecting { buffer } => {
                        if !buffer.is_empty() {
                            let chunk = make_end_chunk(
                                this.model,
                                Delta {
                                    content: Some(buffer),
                                    ..Default::default()
                                },
                                "stop",
                            );
                            return Poll::Ready(Some(Ok(chunk)));
                        }
                        return Poll::Ready(None);
                    }
                    ToolParseState::CollectingXml(buf) => {
                        if let Some((calls, _)) = parse_tool_calls(&buf) {
                            debug!(
                                target: "adapter",
                                "tool_parser 流结束时解析出 {} 个工具调用",
                                calls.len()
                            );
                            let chunk = make_end_chunk(
                                this.model,
                                Delta {
                                    tool_calls: Some(calls),
                                    ..Default::default()
                                },
                                "tool_calls",
                            );
                            return Poll::Ready(Some(Ok(chunk)));
                        } else {
                            warn!(
                                target: "adapter",
                                "tool_parser 流结束→请求修复"
                            );
                            return Poll::Ready(Some(Err(
                                OpenAIAdapterError::ToolCallRepairNeeded(buf),
                            )));
                        }
                    }
                    ToolParseState::Done => return Poll::Ready(None),
                },
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(content: &str) -> String {
        format!("{TOOL_CALL_START}{content}{TOOL_CALL_END}")
    }

    fn tool_ts(content: &str, suffix: &str) -> String {
        format!("{TOOL_CALL_START}{content}{TOOL_CALL_END}{suffix}")
    }

    #[test]
    fn parse_json_tool_calls() {
        let xml = tool(r#"[{"name": "get_weather", "arguments": {"city": "北京"}}]"#);
        let (calls, remaining) = parse_tool_calls(&xml).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "get_weather");
        assert_eq!(
            calls[0].function.as_ref().unwrap().arguments,
            r#"{"city":"北京"}"#
        );
    }

    #[test]
    fn parse_json_with_surrounding_text() {
        let xml = format!(
            "{TOOL_CALL_START}\n\t以下是工具调用：\n\t[{{\"name\": \"f\", \"arguments\": {{}}}}]\n\t{TOOL_CALL_END}"
        );
        let (calls, _remaining) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "f");
    }

    #[test]
    fn parse_json_multiple_tools() {
        let xml = tool(
            r#"[{"name": "get_weather", "arguments": {}}, {"name": "get_time", "arguments": {"tz": "bj"}}]"#,
        );
        let (calls, remaining) = parse_tool_calls(&xml).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].index, 0);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "get_weather");
        assert_eq!(calls[1].index, 1);
        assert_eq!(calls[1].function.as_ref().unwrap().name, "get_time");
    }

    #[test]
    fn parse_json_with_trailing_text() {
        let xml = tool_ts(
            r#"[{"name": "get_weather", "arguments": {}}]"#,
            " trailing text",
        );
        let (calls, remaining) = parse_tool_calls(&xml).unwrap();
        assert_eq!(remaining, " trailing text");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "get_weather");
    }

    // --- repair_invalid_backslashes ---

    #[test]
    fn repair_backslashes_passes_valid_escapes() {
        assert_eq!(
            repair_invalid_backslashes(r#"hello\nworld"#),
            r#"hello\nworld"#
        );
        assert_eq!(repair_invalid_backslashes(r#"\"quoted\""#), r#"\"quoted\""#);
        assert_eq!(repair_invalid_backslashes(r#"tab\there"#), r#"tab\there"#);
        assert_eq!(
            repair_invalid_backslashes(r#"\\backslash"#),
            r#"\\backslash"#
        );
    }

    #[test]
    fn repair_backslashes_fixes_invalid_escapes() {
        let input = r#"C:\Users\name"#;
        let result = repair_invalid_backslashes(input);
        assert_eq!(result.len(), input.len() + 1);
        assert_eq!(result.as_bytes()[2], b'\\');
        assert_eq!(result.as_bytes()[3], b'\\');
        assert_eq!(result.as_bytes()[4], b'U');
    }

    #[test]
    fn repair_backslashes_keeps_valid_n() {
        let result = repair_invalid_backslashes(r#"line1\nline2"#);
        assert_eq!(result, r#"line1\nline2"#);
    }

    #[test]
    fn repair_backslashes_mixed_valid_and_invalid() {
        assert_eq!(
            repair_invalid_backslashes("line1\nline2\tend\r\n"),
            "line1\nline2\tend\r\n"
        );
    }

    // --- repair_unquoted_keys ---

    #[test]
    fn repair_unquoted_keys_basic() {
        let input = r#"{name: "get_weather"}"#;
        let expected = r#"{"name": "get_weather"}"#;
        assert_eq!(repair_unquoted_keys(input), expected);
    }

    #[test]
    fn repair_unquoted_keys_nested() {
        let input = r#"{city: "bj", extra: {a: 1}}"#;
        let expected = r#"{"city": "bj", "extra": {"a": 1}}"#;
        assert_eq!(repair_unquoted_keys(input), expected);
    }

    #[test]
    fn repair_unquoted_keys_array() {
        let input = r#"[{name: "f", arguments: {}}]"#;
        let expected = r#"[{"name": "f", "arguments": {}}]"#;
        assert_eq!(repair_unquoted_keys(input), expected);
    }

    #[test]
    fn repair_unquoted_keys_quoted_keys_untouched() {
        let input = r#"{"name": "f", "args": {"city": "bj"}}"#;
        assert_eq!(repair_unquoted_keys(input), input);
    }

    // --- parse_tool_calls with repair ---

    #[test]
    fn parse_tool_calls_with_unquoted_keys() {
        let xml = tool(r#"[{name: "get_weather", arguments: {city: "北京"}}]"#);
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "get_weather");
    }

    #[test]
    fn parse_tool_calls_with_invalid_backslashes() {
        let xml = tool(r#"[{"name": "read_file", "arguments": {"path": "C:\Users\name"}}]"#);
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "read_file");
    }

    #[test]
    fn parse_tool_calls_with_both_repairs() {
        let xml = tool(r#"[{name: "read_file", arguments: {path: "C:\file"}}]"#);
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "read_file");
    }

    // --- code fence ---

    #[test]
    fn parse_tool_calls_inside_code_fence_skipped() {
        let xml = format!(
            "示例：\n```json\n{TOOL_CALL_START}[{{\"name\": \"get_weather\", \"arguments\": {{}}}}]{TOOL_CALL_END}\n```"
        );
        assert!(parse_tool_calls(&xml).is_none());
    }

    #[test]
    fn parse_tool_calls_not_inside_code_fence() {
        let xml = tool(r#"[{"name": "get_weather", "arguments": {}}]"#);
        assert!(parse_tool_calls(&xml).is_some());
    }

    #[test]
    fn parse_tool_calls_tool_call_inside_value_not_skipped() {
        let xml = tool(
            r#"[{"name": "format_code", "arguments": {"code": "```rust\nfn main() {}\n```"}}]"#,
        );
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "format_code");
    }

    // --- is_inside_code_fence ---

    #[test]
    fn code_fence_detection() {
        assert!(!is_inside_code_fence("普通文本", 0));
        let s = format!("```\n{TOOL_CALL_START}");
        assert!(is_inside_code_fence(&s, 5));
        let s = format!("```\ncode\n```\n{TOOL_CALL_START}");
        assert!(!is_inside_code_fence(&s, 13));
    }

    // --- single object fallback ---

    #[test]
    fn parse_tool_calls_single_object() {
        let xml = tool(r#"{"name": "get_weather", "arguments": {"city": "北京"}}"#);
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "get_weather");
        assert_eq!(
            calls[0].function.as_ref().unwrap().arguments,
            r#"{"city":"北京"}"#
        );
    }

    #[test]
    fn parse_tool_calls_single_object_with_newlines() {
        let xml = format!(
            "{TOOL_CALL_START}\n{{\"name\": \"Bash\", \"arguments\": {{\"command\": \"ls\"}}}}\n{TOOL_CALL_END}"
        );
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "Bash");
    }

    #[test]
    fn parse_tool_calls_single_object_with_surrounding_text() {
        let xml = format!(
            "{TOOL_CALL_START}以下是工具调用：{{\"name\": \"f\", \"arguments\": {{}}}}{TOOL_CALL_END}"
        );
        let (calls, remaining) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(remaining, "");
    }

    #[test]
    fn parse_tool_calls_single_object_unquoted_keys() {
        let xml = tool(r#"{name: "get_weather", arguments: {city: "北京"}}"#);
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "get_weather");
    }

    #[test]
    fn parse_tool_calls_single_object_and_repair_backslashes() {
        let xml = tool(r#"{"name": "read_file", "arguments": {"path": "C:\Users\name"}}"#);
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "read_file");
    }
}
