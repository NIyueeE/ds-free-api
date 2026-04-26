//! 工具调用解析 —— 滑动窗口检测 XML <tool_calls>，转换为结构化 tool_calls
//!
//! 算法核心：
//! - Detecting 状态：维护固定宽度 W 的扫描缓冲区，新 chunk 到来时
//!   先追加到缓冲区，扫描 `<tool_calls>`，未找到则释放超出 W 的安全部分
//! - CollectingXml 状态：检测到 `<tool_calls>` 后收集 XML 直到 `</tool_calls>`
//! - Done 状态：工具调用已发出，截断后续内容（防幻觉）

use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use futures::Stream;
use pin_project_lite::pin_project;

use log::{debug, warn};

use crate::openai_adapter::OpenAIAdapterError;
use crate::openai_adapter::types::{
    ChatCompletionChunk, ChunkChoice, Delta, FunctionCall, ToolCall,
};

static CALL_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
pub(crate) const MAX_XML_BUF_LEN: usize = 64 * 1024;

/// `<tool_calls>` 标记
const TAG_START: &str = "<tool_calls>";
/// `</tool_calls>` 闭合标记
const TAG_END: &str = "</tool_calls>";
/// 标记字节长度
const TAG_LEN: usize = TAG_START.len(); // 12
/// 滑动扫描窗口大小 = 标记长度 + 安全余量
/// 保证大 chunk 到来时不会将 `<tool_calls>` 前缀挤出窗口
const W: usize = TAG_LEN + 7; // 19

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
///
/// JSON 只允许 `\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\uXXXX`。
/// 遇到其他 `\X` 时将其修复为 `\\X`。
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
                    // 无效转义：双写反斜杠
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

/// 修复 JSON 中未加引号的 key（如 `{name: "value"}` → `{"name": "value"}`）
fn repair_unquoted_keys(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 32);
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if (chars[i] == '{' || chars[i] == ',') && i + 1 < len {
            out.push(chars[i]);
            i += 1;
            // 跳过空白
            while i < len && chars[i].is_whitespace() {
                out.push(chars[i]);
                i += 1;
            }
            // 若后面跟的是一个未被引号括起来的标识符，且紧跟 `:`，则补引号
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

/// 对 JSON 字符串依次尝试修复：无效转义 → 未引号 key，若修复后合法则返回
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
/// 标签内格式为 JSON 数组：
/// `<tool_calls>[{"name": "get_weather", "arguments": {"city": "北京"}}]</tool_calls>`
///
/// 若内容位于 markdown 代码块内则跳过解析（防误触发）。
/// 若标准 JSON 解析失败，会尝试宽松修复（无效转义、未引号 key）后重试。
pub fn parse_tool_calls(xml: &str) -> Option<(Vec<ToolCall>, String)> {
    let start = xml.find(TAG_START)?;
    let after_start = start + TAG_START.len();

    // 跳过 markdown 代码块中的工具示例
    if is_inside_code_fence(xml, start) {
        return None;
    }

    // 闭合标签可选：有则截断尾部幻觉，无则取到末尾
    let (end, inner_end) = match xml.find(TAG_END) {
        Some(pos) => (pos + TAG_END.len(), pos),
        None => (xml.len(), xml.len()),
    };
    let inner = &xml[after_start..inner_end];

    // 找到第一个 [ 和最后一个 ] 来提取 JSON 数组，容许标签内有非 JSON 文本
    // 若没有 [，作为单个 JSON 对象兜底（等价于单元素数组）
    let arr: Vec<serde_json::Value> = match inner.find('[') {
        Some(arr_start) => {
            let arr_end = inner.rfind(']')? + 1;
            let json_str = &inner[arr_start..arr_end];
            // 标准解析；失败时尝试宽松修复
            let arr: Option<Vec<serde_json::Value>> = serde_json::from_str(json_str).ok();
            arr.or_else(|| {
                let repaired = repair_json(json_str)?;
                serde_json::from_str(&repaired).ok()
            })?
        }
        None => {
            // 定位第一个 { 和最后一个 } 来提取 JSON 对象，容许多余文本
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
            Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "{}".into()),
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

fn make_end_chunk(model: &str, delta: Delta, finish_reason: &'static str) -> ChatCompletionChunk {
    ChatCompletionChunk {
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
    /// 滑动窗口扫描：累积内容，W 宽度窗口检测 `<tool_calls>`
    Detecting {
        /// 累积缓冲区：保留尾部 W 个字节用于标记检测
        buffer: String,
    },
    /// 检测到 `<tool_calls>`，收集 XML 直到 `</tool_calls>`
    CollectingXml(String),
    /// 工具调用已发出，截断后续内容
    Done,
}

pin_project! {
    // 在 content delta 中检测并解析 XML <tool_calls> 的流转换器
    //
    // 使用固定宽度 W 的滑动窗口：新内容进入缓冲区，扫描后再释放安全部分，
    // 确保 `<tool_calls>` 碎片不会溢出窗口。检测到标记后收集完整 XML，
    // 解析为结构化 tool_calls 并发出。
    pub struct ToolCallStream<S> {
        #[pin]
        inner: S,
        state: ToolParseState,
        model: String,
        finish_emitted: bool,
        // 待修复的原始工具调用 XML：下一次 poll 时返回 Err 触发上层修复
        repair_pending: Option<String>,
    }
}

impl<S> ToolCallStream<S> {
    /// 创建工具调用解析流
    pub fn new(inner: S, model: String) -> Self {
        Self {
            inner,
            state: ToolParseState::Detecting {
                buffer: String::new(),
            },
            model,
            finish_emitted: false,
            repair_pending: None,
        }
    }
}

impl<S> Stream for ToolCallStream<S>
where
    S: Stream<Item = Result<ChatCompletionChunk, OpenAIAdapterError>>,
{
    type Item = Result<ChatCompletionChunk, OpenAIAdapterError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if let Some(raw_xml) = this.repair_pending.take() {
            debug!(target: "adapter", "tool_parser 发出修复请求");
            return Poll::Ready(Some(Err(OpenAIAdapterError::ToolCallRepairNeeded(raw_xml))));
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

                                // 扫描缓冲区是否包含 <tool_calls>
                                if let Some(pos) = buffer.find(TAG_START) {
                                    debug!(
                                        target: "adapter",
                                        "tool_parser 检测到 <tool_calls>，缓冲区大小={}",
                                        buffer.len()
                                    );
                                    let before = buffer[..pos].to_string();
                                    let rest = std::mem::take(buffer)[pos..].to_string();

                                    // 检查闭合标签是否也在缓冲区中
                                    if let Some(end_pos) = rest.find(TAG_END) {
                                        let end_abs = end_pos + TAG_END.len();
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

                                    // 无闭合标签，进入收集状态
                                    if before.is_empty() {
                                        *this.state = ToolParseState::CollectingXml(rest);
                                        continue; // 无前导文本，吞掉此 chunk
                                    }
                                    choice.delta.content = Some(before);
                                    *this.state = ToolParseState::CollectingXml(rest);
                                    return Poll::Ready(Some(Ok(chunk)));
                                } else {
                                    // 无标记，安全释放超出窗口的部分
                                    let safe =
                                        floor_char_boundary(buffer, buffer.len().saturating_sub(W));
                                    if safe > 0 {
                                        choice.delta.content = Some(buffer[..safe].to_string());
                                        buffer.drain(..safe);
                                        return Poll::Ready(Some(Ok(chunk)));
                                    }
                                    // 内容在扫描窗口内，暂不释放
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
                                if let Some(end_pos) = buf.find(TAG_END) {
                                    let end_abs = end_pos + TAG_END.len();
                                    let collected = buf[..end_abs].to_string();
                                    let _tail = buf.split_off(end_abs);

                                    if let Some((calls, _)) = parse_tool_calls(&collected) {
                                        debug!(
                                            target: "adapter",
                                            "tool_parser 解析出 {} 个工具调用",
                                            calls.len()
                                        );
                                        // 闭合标签之后的内容是模型幻觉（如继续生成多轮对话），丢弃
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
                                // XML 未闭合，继续收集
                                continue;
                            }

                            ToolParseState::Done => {
                                // 已解析 tool_calls，丢弃后续幻觉内容，主动关闭流
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
                        // 无 content 的 delta（finish_reason、role、reasoning 等）
                        match &mut this.state {
                            ToolParseState::Detecting { buffer } => {
                                if choice.finish_reason.is_some() {
                                    // finish chunk，冲刷剩余缓冲
                                    if !buffer.is_empty() {
                                        choice.delta.content = Some(std::mem::take(buffer));
                                    }
                                    return Poll::Ready(Some(Ok(chunk)));
                                }
                                // 非 finish（role、reasoning 等），直接透传
                                return Poll::Ready(Some(Ok(chunk)));
                            }

                            ToolParseState::CollectingXml(buf) => {
                                if choice.finish_reason.is_some() {
                                    // finish 到达，尝试解析（闭合标签可选）
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
                                // 非 finish（如 reasoning），透传
                                return Poll::Ready(Some(Ok(chunk)));
                            }

                            ToolParseState::Done => {
                                // 已解析 tool_calls，主动关闭流
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
                Poll::Ready(None) => {
                    // 流结束，冲刷残留缓冲
                    match std::mem::replace(this.state, ToolParseState::Done) {
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
                            // 流结束，尝试解析（闭合标签可选）
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
                    }
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_tool_calls() {
        let xml =
            r#"<tool_calls>[{"name": "get_weather", "arguments": {"city": "北京"}}]</tool_calls>"#;
        let (calls, remaining) = parse_tool_calls(xml).unwrap();
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
        // 模型可能在 JSON 前后加废话
        let xml = r#"<tool_calls>
以下是工具调用：
[{"name": "f", "arguments": {}}]
</tool_calls>"#;
        let (calls, _remaining) = parse_tool_calls(xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "f");
    }

    #[test]
    fn parse_json_multiple_tools() {
        let xml = r#"<tool_calls>[{"name": "get_weather", "arguments": {}}, {"name": "get_time", "arguments": {"tz": "bj"}}]</tool_calls>"#;
        let (calls, remaining) = parse_tool_calls(xml).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].index, 0);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "get_weather");
        assert_eq!(calls[1].index, 1);
        assert_eq!(calls[1].function.as_ref().unwrap().name, "get_time");
    }

    #[test]
    fn parse_json_with_trailing_text() {
        let xml =
            r#"<tool_calls>[{"name": "get_weather", "arguments": {}}]</tool_calls> trailing text"#;
        let (calls, remaining) = parse_tool_calls(xml).unwrap();
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
        // `\U` invalid → `\\U`（多加一个反斜杠），`\n` 是合法转义保持不变
        assert_eq!(result.len(), input.len() + 1, "应在 \\U 处多一个反斜杠");
        assert_eq!(result.as_bytes()[2], b'\\');
        assert_eq!(result.as_bytes()[3], b'\\');
        assert_eq!(result.as_bytes()[4], b'U');
    }

    #[test]
    fn repair_backslashes_keeps_valid_n() {
        // `\n` 是合法 JSON 转义，保持不变
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
        let xml = r#"<tool_calls>[{name: "get_weather", arguments: {city: "北京"}}]</tool_calls>"#;
        let (calls, _) = parse_tool_calls(xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "get_weather");
    }

    #[test]
    fn parse_tool_calls_with_invalid_backslashes() {
        let xml = r#"<tool_calls>[{"name": "read_file", "arguments": {"path": "C:\Users\name"}}]</tool_calls>"#;
        let (calls, _) = parse_tool_calls(xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "read_file");
    }

    #[test]
    fn parse_tool_calls_with_both_repairs() {
        // unquoted keys + invalid backslashes 同时出现
        let xml = r#"<tool_calls>[{name: "read_file", arguments: {path: "C:\file"}}]</tool_calls>"#;
        let (calls, _) = parse_tool_calls(xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "read_file");
    }

    // --- code fence ---

    #[test]
    fn parse_tool_calls_inside_code_fence_skipped() {
        // 模型在 markdown 代码块中展示工具调用示例
        let xml = "示例：\n```json\n<tool_calls>[{\"name\": \"get_weather\", \"arguments\": {}}]</tool_calls>\n```";
        assert!(parse_tool_calls(xml).is_none());
    }

    #[test]
    fn parse_tool_calls_not_inside_code_fence() {
        // 正常工具调用，不在代码块内
        let xml = r#"<tool_calls>[{"name": "get_weather", "arguments": {}}]</tool_calls>"#;
        assert!(parse_tool_calls(xml).is_some());
    }

    #[test]
    fn parse_tool_calls_tool_call_inside_value_not_skipped() {
        // 工具调用中包含代码块标记作为参数值，不应跳过
        let xml = r#"<tool_calls>[{"name": "format_code", "arguments": {"code": "```rust\nfn main() {}\n```"}}]</tool_calls>"#;
        let (calls, _) = parse_tool_calls(xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "format_code");
    }

    // --- is_inside_code_fence ---

    #[test]
    fn code_fence_detection() {
        assert!(!is_inside_code_fence("普通文本", 0));
        assert!(is_inside_code_fence("```\n<tool_calls>", 5));
        assert!(!is_inside_code_fence("```\ncode\n```\n<tool_calls>", 17));
    }

    // --- single object fallback ---

    #[test]
    fn parse_tool_calls_single_object() {
        let xml =
            r#"<tool_calls>{"name": "get_weather", "arguments": {"city": "北京"}}</tool_calls>"#;
        let (calls, _) = parse_tool_calls(xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "get_weather");
        assert_eq!(
            calls[0].function.as_ref().unwrap().arguments,
            r#"{"city":"北京"}"#
        );
    }

    #[test]
    fn parse_tool_calls_single_object_with_newlines() {
        let xml = "<tool_calls>\n{\"name\": \"Bash\", \"arguments\": {\"command\": \"ls\"}}\n</tool_calls>";
        let (calls, _) = parse_tool_calls(xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "Bash");
    }

    #[test]
    fn parse_tool_calls_single_object_with_surrounding_text() {
        let xml = r#"<tool_calls>以下是工具调用：{"name": "f", "arguments": {}}</tool_calls>"#;
        let (calls, remaining) = parse_tool_calls(xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(remaining, "");
    }

    #[test]
    fn parse_tool_calls_single_object_unquoted_keys() {
        let xml = r#"<tool_calls>{name: "get_weather", arguments: {city: "北京"}}</tool_calls>"#;
        let (calls, _) = parse_tool_calls(xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "get_weather");
    }

    #[test]
    fn parse_tool_calls_single_object_and_repair_backslashes() {
        let xml = r#"<tool_calls>{"name": "read_file", "arguments": {"path": "C:\Users\name"}}</tool_calls>"#;
        let (calls, _) = parse_tool_calls(xml).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.as_ref().unwrap().name, "read_file");
    }
}
