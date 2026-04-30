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
use std::time::Duration;

use futures::Stream;
use pin_project_lite::pin_project;

use log::{debug, trace, warn};

use crate::openai_adapter::OpenAIAdapterError;
use crate::openai_adapter::types::{
    ChatCompletionsResponseChunk, ChunkChoice, Delta, FunctionCall, ToolCall,
};

static CALL_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
pub(crate) const MAX_XML_BUF_LEN: usize = 64 * 1024;

pub(crate) const TOOL_CALL_START: &str = "<|tool▁calls▁begin|>";
pub(crate) const TOOL_CALL_END: &str = "<|tool▁calls▁end|>";
const W: usize = 71;

const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct TagConfig {
    pub starts: Vec<String>,
    pub ends: Vec<String>,
}

impl TagConfig {
    pub fn from_config(cfg: &crate::config::ToolCallTagConfig) -> Self {
        Self {
            starts: cfg.extra_starts.clone(),
            ends: cfg.extra_ends.clone(),
        }
    }
}

fn match_start_tag<'a>(s: &'a str, tag: &str) -> Option<(usize, &'a str)> {
    let partial = tag.trim_end_matches('>');
    if let Some(pos) = s.find(partial) {
        Some((pos, &s[pos..pos + partial.len()]))
    } else {
        None
    }
}

pub(crate) fn contains_start_tag_with(s: &str, cfg: &TagConfig) -> bool {
    let partial = TOOL_CALL_START.trim_end_matches('>');
    if s.contains(partial) {
        return true;
    }
    for start in &cfg.starts {
        if s.contains(start.trim_end_matches('>')) {
            return true;
        }
    }
    false
}

pub(crate) fn find_start_tag_with<'a>(s: &'a str, cfg: &TagConfig) -> Option<(usize, &'a str)> {
    if let Some(m) = match_start_tag(s, TOOL_CALL_START) {
        return Some(m);
    }
    for start in &cfg.starts {
        if let Some(m) = match_start_tag(s, start) {
            return Some(m);
        }
    }
    None
}

pub(crate) fn find_end_tag_with<'a>(
    s: &'a str,
    from: usize,
    cfg: &TagConfig,
    start_tag: Option<&str>,
) -> Option<(usize, &'a str)> {
    let search = &s[from..];
    if let Some(st) = start_tag {
        let open_tag = st.trim_end_matches('>');
        let close_tag = format!("</{}>", &open_tag[1..]);
        if let Some(pos) = search.find(&close_tag) {
            let abs = from + pos;
            return Some((abs, &s[abs..abs + close_tag.len()]));
        }
    }

    // 无论 start_tag 是否提供，都尝试已知结束标签
    for end in std::iter::once(TOOL_CALL_END).chain(cfg.ends.iter().map(|s| s.as_str())) {
        if let Some(pos) = search.find(end) {
            let abs = from + pos;
            return Some((abs, &s[abs..abs + end.len()]));
        }
    }
    if let Some(st) = start_tag
        && let Some((pos, tag)) = match_start_tag(search, st)
    {
        return Some((from + pos, &s[from + pos..from + pos + tag.len()]));
    }
    if let Some((pos, tag)) = match_start_tag(search, TOOL_CALL_START) {
        return Some((from + pos, &s[from + pos..from + pos + tag.len()]));
    }
    for start in &cfg.starts {
        if let Some((pos, tag)) = match_start_tag(search, start) {
            return Some((from + pos, &s[from + pos..from + pos + tag.len()]));
        }
    }
    None
}

fn is_start_tag(tag: &str, cfg: &TagConfig) -> bool {
    if !tag.starts_with('<') {
        return false;
    }
    let partial = TOOL_CALL_START.trim_end_matches('>');
    if partial.starts_with(tag) || tag.starts_with(partial) {
        return true;
    }
    for start in &cfg.starts {
        let p = start.trim_end_matches('>');
        if p.starts_with(tag) || tag.starts_with(p) {
            return true;
        }
    }
    false
}

fn next_call_id() -> String {
    let n = CALL_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("call_{:016x}", n)
}

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

fn is_inside_code_fence(xml: &str, tag_pos: usize) -> bool {
    xml[..tag_pos].matches("```").count() % 2 == 1
}

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

pub fn parse_tool_calls(xml: &str) -> Option<(Vec<ToolCall>, String)> {
    parse_tool_calls_with(xml, &TagConfig::from_config(&Default::default()))
}

pub fn parse_tool_calls_with(xml: &str, cfg: &TagConfig) -> Option<(Vec<ToolCall>, String)> {
    let (start, start_tag) = find_start_tag_with(xml, cfg)?;
    let after_start = start + start_tag.len();
    if is_inside_code_fence(xml, start) {
        return None;
    }

    let (end, inner_end) = match find_end_tag_with(xml, after_start, cfg, Some(start_tag)) {
        Some((pos, matched_end)) => (pos + matched_end.len(), pos),
        None => (xml.len(), xml.len()),
    };
    let inner = &xml[after_start..inner_end];

    let arr = match inner.find('[') {
        Some(arr_start) => {
            let arr_end = inner.rfind(']').map(|p| p + 1).unwrap_or(inner.len());
            let json_str = &inner[arr_start..arr_end];
            if json_str.trim() == "[]" {
                return None;
            }
            match serde_json::from_str::<Vec<serde_json::Value>>(json_str) {
                Ok(a) => a,
                Err(_) => {
                    let repaired = repair_json(json_str).unwrap_or_default();
                    let obj_str = repaired.trim_start_matches('[');
                    let obj_start = obj_str.find('{')?;
                    let obj_end = obj_str.rfind('}').map(|p| p + 1).unwrap_or(obj_str.len());
                    serde_json::from_str(&obj_str[obj_start..obj_end])
                        .ok()
                        .filter(|v: &serde_json::Value| v.is_object())
                        .map(|v| vec![v])?
                }
            }
        }
        None => {
            if let Some(obj_start) = inner.find('{') {
                let obj_end = inner.rfind('}').map(|p| p + 1).unwrap_or(inner.len());
                let json_str = &inner[obj_start..obj_end];
                let obj = serde_json::from_str(json_str)
                    .ok()
                    .filter(|v: &serde_json::Value| v.is_object())
                    .or_else(|| {
                        let repaired = repair_json(json_str)?;
                        serde_json::from_str(&repaired)
                            .ok()
                            .filter(|v: &serde_json::Value| v.is_object())
                    })?;
                vec![obj]
            } else {
                return parse_invoke_calls(inner, &xml[..start], &xml[end..]);
            }
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

fn parse_invoke_calls(inner: &str, prefix: &str, suffix: &str) -> Option<(Vec<ToolCall>, String)> {
    use std::collections::BTreeMap;
    let mut calls = Vec::new();
    let mut pos = 0;
    let lower = inner.to_lowercase();
    while let Some(invoke_start) = lower[pos..].find("<invoke ") {
        let abs_start = pos + invoke_start;
        let name_attr = &inner[abs_start..];
        let name_start = name_attr.find("name=\"")? + 6;
        let name_end = name_attr[name_start..].find('"')?;
        let name = &name_attr[name_start..name_start + name_end];
        let close_tag = "</invoke>";
        let rest = &lower[abs_start..];
        let close_pos = rest.find(close_tag)?;
        let invoke_body = &inner[abs_start..abs_start + close_pos + close_tag.len()];
        let mut params: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        let mut ppos = 0;
        let body_lower = invoke_body.to_lowercase();
        while let Some(p_start) = body_lower[ppos..].find("<parameter ") {
            let p_abs = ppos + p_start;
            let p_attr = &invoke_body[p_abs..];
            let p_name_start = p_attr.find("name=\"")? + 6;
            let p_name_end = p_attr[p_name_start..].find('"')?;
            let p_name = &p_attr[p_name_start..p_name_start + p_name_end];
            let p_body_start = p_attr.find('>')? + 1;
            let p_close = String::from("</parameter>");
            let p_close_pos = p_attr[p_body_start..].find(&p_close)?;
            let p_value = &p_attr[p_body_start..p_body_start + p_close_pos];
            let val: serde_json::Value = serde_json::from_str(p_value.trim())
                .unwrap_or_else(|_| serde_json::Value::String(p_value.to_string()));
            params.insert(p_name.to_string(), val);
            let p_end = p_body_start + p_close_pos + p_close.len();
            ppos += p_start + p_end;
        }
        let arguments = serde_json::to_string(&params).unwrap_or_else(|_| "{}".into());
        calls.push(ToolCall {
            id: next_call_id(),
            ty: "function".to_string(),
            function: Some(FunctionCall {
                name: name.to_string(),
                arguments,
            }),
            custom: None,
            index: calls.len() as u32,
        });
        pos = abs_start + close_pos + close_tag.len();
    }
    if calls.is_empty() {
        return None;
    }
    Some((calls, prefix.to_string() + suffix))
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
    CollectingXml { buf: String, start_tag: String },
    Done,
}

pin_project! {
    pub struct ToolCallStream<S> {
        #[pin]
        inner: S,
        state: ToolParseState,
        model: String,
        finish_emitted: bool,
        repair_pending: Option<String>,
        tag_config: Arc<TagConfig>,
        last_keepalive: tokio::time::Instant,
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
            last_keepalive: tokio::time::Instant::now(),
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
            if matches!(&this.state, ToolParseState::CollectingXml { .. })
                && this.last_keepalive.elapsed() >= KEEPALIVE_INTERVAL
            {
                trace!(target: "adapter", ">>> keepalive: 发送空工具增量");
                *this.last_keepalive = tokio::time::Instant::now();
                return Poll::Ready(Some(Ok(ChatCompletionsResponseChunk {
                    id: "chatcmpl-keepalive".into(),
                    object: "chat.completion.chunk",
                    created: 0,
                    model: this.model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: Delta {
                            tool_calls: Some(vec![ToolCall {
                                id: String::new(),
                                ty: "function".into(),
                                function: Some(FunctionCall {
                                    name: String::new(),
                                    arguments: String::new(),
                                }),
                                custom: None,
                                index: 0,
                            }]),
                            ..Default::default()
                        },
                        finish_reason: None,
                        logprobs: None,
                    }],
                    usage: None,
                    service_tier: None,
                    system_fingerprint: None,
                })));
            }

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

                                let maybe_tag = find_start_tag_with(buffer, this.tag_config)
                                    .map(|(pos, tag)| (pos, tag.to_string()));
                                if let Some((pos, start_tag)) = maybe_tag {
                                    trace!(target: "adapter", ">>> 检测到 start_tag={}, buf_len={}", start_tag, buffer.len());
                                    let before = buffer[..pos].to_string();
                                    let rest = std::mem::take(buffer)[pos..].to_string();
                                    if let Some((end_pos, matched_end)) = find_end_tag_with(
                                        &rest,
                                        start_tag.len(),
                                        this.tag_config,
                                        Some(&start_tag),
                                    ) {
                                        let inner = &rest[start_tag.len()..end_pos];
                                        if is_start_tag(matched_end, this.tag_config)
                                            && inner.trim().is_empty()
                                        {
                                            if before.is_empty() {
                                                *this.state = ToolParseState::CollectingXml {
                                                    buf: rest,
                                                    start_tag: start_tag.clone(),
                                                };
                                            } else {
                                                choice.delta.content = Some(before);
                                                *this.state = ToolParseState::CollectingXml {
                                                    buf: rest,
                                                    start_tag: start_tag.clone(),
                                                };
                                            }
                                            continue;
                                        }
                                        let end_abs = end_pos + matched_end.len();
                                        let collected = &rest[..end_abs];
                                        if let Some((calls, _)) = parse_tool_calls(collected) {
                                            debug!(target: "adapter", "tool_parser 解析出 {} 个工具调用", calls.len());
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
                                            trace!(target: "adapter", "tool_parser 解析失败，collected=\n{}", &collected[..collected.len().min(500)]);
                                            warn!(target: "adapter", "tool_parser 解析失败→请求修复");
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
                                        *this.state = ToolParseState::CollectingXml {
                                            buf: rest,
                                            start_tag: start_tag.clone(),
                                        };
                                        continue;
                                    }
                                    choice.delta.content = Some(before);
                                    *this.state = ToolParseState::CollectingXml {
                                        buf: rest,
                                        start_tag: start_tag.clone(),
                                    };
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

                            ToolParseState::CollectingXml { buf, start_tag } => {
                                buf.push_str(&content);
                                if buf.len() > MAX_XML_BUF_LEN {
                                    debug!(target: "adapter", "tool_parser 缓冲超限，回退纯文本");
                                    let flushed = std::mem::take(buf);
                                    *this.state = ToolParseState::Detecting {
                                        buffer: String::new(),
                                    };
                                    choice.delta.content = Some(flushed);
                                    return Poll::Ready(Some(Ok(chunk)));
                                }
                                let start_end = buf.find('>').map(|p| p + 1).unwrap_or(0);
                                if let Some((end_pos, en_tag)) = find_end_tag_with(
                                    buf,
                                    start_end,
                                    this.tag_config,
                                    Some(start_tag),
                                ) {
                                    let inner = &buf[start_end..end_pos];
                                    if is_start_tag(en_tag, this.tag_config)
                                        && inner.trim().is_empty()
                                    {
                                        continue;
                                    }
                                    let end_abs = end_pos + en_tag.len();
                                    let collected = buf[..end_abs].to_string();
                                    let _tail = buf.split_off(end_abs);
                                    if let Some((calls, _)) = parse_tool_calls(&collected) {
                                        debug!(target: "adapter", "tool_parser 解析出 {} 个工具调用", calls.len());
                                        choice.delta.content = None;
                                        choice.delta.tool_calls = Some(calls);
                                        if choice.finish_reason == Some("stop") {
                                            choice.finish_reason = Some("tool_calls");
                                        }
                                        *this.state = ToolParseState::Done;
                                    } else {
                                        trace!(target: "adapter", "tool_parser 解析失败(流结束)，collected=\n{}", &collected[..collected.len().min(500)]);
                                        warn!(target: "adapter", "tool_parser 解析失败→请求修复");
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
                            ToolParseState::CollectingXml { buf, start_tag: _ } => {
                                if choice.finish_reason.is_some() {
                                    let flushed = std::mem::take(buf);
                                    if let Some((calls, _)) = parse_tool_calls(&flushed) {
                                        debug!(target: "adapter", "tool_parser 流结束时解析出 {} 个工具调用", calls.len());
                                        choice.delta.tool_calls = Some(calls);
                                        if choice.finish_reason == Some("stop") {
                                            choice.finish_reason = Some("tool_calls");
                                        }
                                    } else {
                                        warn!(target: "adapter", "tool_parser finish→请求修复");
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
                                    let mut end =
                                        make_end_chunk(this.model, Delta::default(), "tool_calls");
                                    if let Some(ref u) = chunk.usage {
                                        end.usage = Some(u.clone());
                                    }
                                    return Poll::Ready(Some(Ok(end)));
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
                    ToolParseState::CollectingXml { buf, start_tag: _ } => {
                        if let Some((calls, _)) = parse_tool_calls(&buf) {
                            debug!(target: "adapter", "tool_parser 流结束时解析出 {} 个工具调用", calls.len());
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
                            warn!(target: "adapter", "tool_parser 流结束→请求修复");
                            return Poll::Ready(Some(Err(
                                OpenAIAdapterError::ToolCallRepairNeeded(buf),
                            )));
                        }
                    }
                    ToolParseState::Done => return Poll::Ready(None),
                },
                Poll::Pending => break,
            }
        }
        Poll::Pending
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
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn parse_json_multiple_tools() {
        let xml = tool(
            r#"[{"name": "get_weather", "arguments": {}}, {"name": "get_time", "arguments": {"tz": "bj"}}]"#,
        );
        let (calls, remaining) = parse_tool_calls(&xml).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(calls.len(), 2);
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
    }

    #[test]
    fn repair_backslashes_passes_valid_escapes() {
        assert_eq!(
            repair_invalid_backslashes(r#"hello\nworld"#),
            r#"hello\nworld"#
        );
    }
    #[test]
    fn repair_backslashes_fixes_invalid_escapes() {
        assert_eq!(repair_invalid_backslashes(r#"C:\Users\name"#).len(), 14);
    }
    #[test]
    fn repair_backslashes_keeps_valid_n() {
        assert_eq!(
            repair_invalid_backslashes(r#"line1\nline2"#),
            r#"line1\nline2"#
        );
    }
    #[test]
    fn repair_unquoted_keys_basic() {
        assert_eq!(
            repair_unquoted_keys(r#"{name: "get_weather"}"#),
            r#"{"name": "get_weather"}"#
        );
    }
    #[test]
    fn repair_unquoted_keys_array() {
        assert_eq!(
            repair_unquoted_keys(r#"[{name: "f", arguments: {}}]"#),
            r#"[{"name": "f", "arguments": {}}]"#
        );
    }

    #[test]
    fn parse_tool_calls_with_unquoted_keys() {
        let xml = tool(r#"[{name: "get_weather", arguments: {city: "北京"}}]"#);
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn parse_tool_calls_with_invalid_backslashes() {
        let xml = tool(r#"[{"name": "read_file", "arguments": {"path": "C:\Users\name"}}]"#);
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn parse_tool_calls_with_both_repairs() {
        let xml = tool(r#"[{name: "read_file", arguments: {path: "C:\file"}}]"#);
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn parse_tool_calls_inside_code_fence_skipped() {
        let xml = format!(
            "示例：\n```json\n{TOOL_CALL_START}[{{\"name\": \"get_weather\", \"arguments\": {{}}}}]{TOOL_CALL_END}\n```"
        );
        assert!(parse_tool_calls(&xml).is_none());
    }

    #[test]
    fn parse_tool_calls_not_inside_code_fence() {
        assert!(parse_tool_calls(&tool(r#"[{"name": "get_weather", "arguments": {}}]"#)).is_some());
    }

    #[test]
    fn parse_tool_calls_tool_call_inside_value_not_skipped() {
        let xml = tool(
            r#"[{"name": "format_code", "arguments": {"code": "```rust\nfn main() {}\n```"}}]"#,
        );
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn code_fence_detection() {
        assert!(!is_inside_code_fence("普通文本", 0));
    }

    #[test]
    fn parse_tool_calls_single_object() {
        let xml = tool(r#"{"name": "get_weather", "arguments": {"city": "北京"}}"#);
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn parse_tool_calls_single_object_with_newlines() {
        let xml = format!(
            "{TOOL_CALL_START}\n{{\"name\": \"Bash\", \"arguments\": {{\"command\": \"ls\"}}}}\n{TOOL_CALL_END}"
        );
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn parse_tool_calls_single_object_with_surrounding_text() {
        let xml = format!(
            "{TOOL_CALL_START}以下是工具调用：{{\"name\": \"f\", \"arguments\": {{}}}}{TOOL_CALL_END}"
        );
        let (_calls, remaining) = parse_tool_calls(&xml).unwrap();
        assert_eq!(remaining, "");
    }

    #[test]
    fn parse_tool_calls_single_object_unquoted_keys() {
        let xml = tool(r#"{name: "get_weather", arguments: {city: "北京"}}"#);
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn parse_tool_calls_single_object_and_repair_backslashes() {
        let xml = tool(r#"{"name": "read_file", "arguments": {"path": "C:\Users\name"}}"#);
        let (calls, _) = parse_tool_calls(&xml).unwrap();
        assert_eq!(calls.len(), 1);
    }
}
