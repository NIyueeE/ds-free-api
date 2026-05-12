//! DeepSeek Patch 状态机 —— 解析 p/o/v 路径操作并产出增量帧

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use pin_project_lite::pin_project;

use log::{trace, warn};

use crate::openai_adapter::OpenAIAdapterError;

use super::sse_parser::SseEvent;

const FRAG_THINK: &str = "THINK";
const FRAG_RESPONSE: &str = "RESPONSE";

/// 从 DeepSeek 流中解析出的单帧增量
#[derive(Debug, Clone)]
pub enum DsFrame {
    /// event: ready，用于生成 delta.role = assistant
    Role,
    /// THINK fragment 追加的文本
    ThinkDelta(String),
    /// RESPONSE fragment 追加的文本
    ContentDelta(String),
    /// response/status 变化
    Status(String),
    /// accumulated_token_usage 数值
    Usage(u32),
    /// event: finish 或最终状态
    Finish,
}

#[derive(Debug, Default)]
struct Fragment {
    ty: String,
    content: String,
}

/// 维护 DeepSeek 响应的 patch 状态，产出可供 converter 消费的增量帧
#[derive(Debug, Default)]
pub struct DsState {
    current_path: Option<String>,
    fragments: Vec<Fragment>,
    status: Option<String>,
    accumulated_token_usage: Option<u32>,
}

impl DsState {
    /// 消费一个 SSE 事件，返回零个或多个增量帧
    pub fn apply_event(&mut self, evt: &SseEvent) -> Vec<DsFrame> {
        let mut frames = Vec::new();

        match evt.event.as_deref() {
            Some("ready") => frames.push(DsFrame::Role),
            Some("finish") => frames.push(DsFrame::Finish),
            _ => {}
        }

        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&evt.data) {
            frames.extend(self.apply_patch_value(val));
        }

        frames
    }

    fn apply_patch_value(&mut self, val: serde_json::Value) -> Vec<DsFrame> {
        let mut frames = Vec::new();
        let has_p = val.get("p").is_some();
        let op = val.get("o").and_then(|v| v.as_str());

        if has_p && let Some(p) = val.get("p").and_then(|v| v.as_str()) {
            self.current_path = Some(p.to_string());
        }

        let Some(v) = val.get("v") else {
            return frames;
        };

        if has_p || op.is_some() {
            if let Some(path) = self.current_path.clone() {
                if path == "response" && op == Some("BATCH") {
                    if let Some(arr) = v.as_array() {
                        for item in arr {
                            let sub = self.apply_patch_value(item.clone());
                            frames.extend(sub);
                        }
                    }
                } else {
                    frames.extend(self.apply_path(&path, op, v));
                }
            }
        } else if self.current_path.is_some() {
            let path = self.current_path.clone().unwrap();
            frames.extend(self.apply_path(&path, None, v));
        } else {
            // 无 current_path 的纯 v 视为 response snapshot，只输出相对旧状态的新后缀。
            if let Some(response) = v.get("response")
                && let Some(arr) = response.get("fragments").and_then(|f| f.as_array())
            {
                let mut next_fragments = Vec::new();
                for (idx, frag) in arr.iter().enumerate() {
                    if let Some(ty) = frag.get("type").and_then(|t| t.as_str()) {
                        let content = frag
                            .get("content")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !content.is_empty() {
                            let old = self.fragments.get(idx).map(|f| f.content.as_str());
                            let delta = snapshot_delta(old, &content);
                            if !delta.is_empty() {
                                match ty {
                                    FRAG_THINK => frames.push(DsFrame::ThinkDelta(delta.to_string())),
                                    FRAG_RESPONSE => {
                                        frames.push(DsFrame::ContentDelta(delta.to_string()))
                                    }
                                    _ => {}
                                }
                            }
                        }
                        next_fragments.push(Fragment {
                            ty: ty.to_string(),
                            content,
                        });
                    }
                }
                self.fragments = next_fragments;
            }
        }

        frames
    }

    fn apply_path(
        &mut self,
        path: &str,
        op: Option<&str>,
        val: &serde_json::Value,
    ) -> Vec<DsFrame> {
        let mut frames = Vec::new();

        match path {
            "response/status" => {
                if let Some(s) = val.as_str() {
                    self.status = Some(s.to_string());
                    if s == "FINISHED" {
                        let has_response = self
                            .fragments
                            .iter()
                            .any(|f| f.ty == "RESPONSE" && !f.content.is_empty());
                        if !has_response {
                            warn!(
                                target: "adapter",
                                "状态机 FINISHED 但无 RESPONSE 内容: fragments={:?}, status={:?}, accumulated_token_usage={:?}",
                                self.fragments.iter().map(|f| format!("{}/{}", f.ty, f.content.len())).collect::<Vec<_>>(),
                                self.status, self.accumulated_token_usage
                            );
                        }
                    }
                    frames.push(DsFrame::Status(s.to_string()));
                }
            }
            "response/accumulated_token_usage" | "accumulated_token_usage" => {
                if let Some(n) = val.as_u64() {
                    let u = u32::try_from(n).unwrap_or(u32::MAX);
                    self.accumulated_token_usage = Some(u);
                    frames.push(DsFrame::Usage(u));
                }
            }
            "response/fragments/-1/content" => {
                if let Some(s) = val.as_str()
                    && let Some(frag) = self.fragments.last_mut()
                {
                    let is_snapshot = op != Some("APPEND") && looks_like_snapshot(&frag.content, s);
                    let content = if is_snapshot {
                        snapshot_delta(Some(&frag.content), s)
                    } else if op == Some("APPEND") {
                        append_delta(&frag.content, s)
                    } else {
                        s
                    };
                    match frag.ty.as_str() {
                        FRAG_THINK => {
                            if is_snapshot {
                                frag.content = s.to_string();
                            } else if !content.is_empty() {
                                frag.content.push_str(content);
                            }
                            if !content.is_empty() {
                                frames.push(DsFrame::ThinkDelta(content.to_string()));
                            }
                        }
                        FRAG_RESPONSE => {
                            if is_snapshot {
                                frag.content = s.to_string();
                            } else if !content.is_empty() {
                                frag.content.push_str(content);
                            }
                            if !content.is_empty() {
                                frames.push(DsFrame::ContentDelta(content.to_string()));
                            }
                        }
                        _ => {
                            // TOOL_SEARCH / TOOL_OPEN 等内部片段不映射到用户可见文本
                        }
                    }
                }
            }
            "response/fragments" if op == Some("APPEND") => {
                if let Some(arr) = val.as_array() {
                    for item in arr {
                        if let Some(ty) = item.get("type").and_then(|t| t.as_str()) {
                            let content = item
                                .get("content")
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .to_string();
                            let visible = if content.is_empty() {
                                String::new()
                            } else {
                                let old = self
                                    .fragments
                                    .iter()
                                    .rev()
                                    .find(|f| f.ty == ty)
                                    .map(|f| f.content.as_str());
                                repeated_fragment_delta(old, &content).to_string()
                            };
                            self.fragments.push(Fragment {
                                ty: ty.to_string(),
                                content,
                            });
                            if !visible.is_empty() {
                                match ty {
                                    FRAG_THINK => frames.push(DsFrame::ThinkDelta(visible)),
                                    FRAG_RESPONSE => {
                                        frames.push(DsFrame::ContentDelta(visible))
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        frames
    }
}

fn looks_like_snapshot(old: &str, new: &str) -> bool {
    new.starts_with(old) || old == new || old.contains(new)
}

fn append_delta<'a>(old: &str, new: &'a str) -> &'a str {
    const REPLAY_MIN_CHARS: usize = 16;
    if new.len() >= REPLAY_MIN_CHARS && old.ends_with(new) {
        ""
    } else {
        new
    }
}

fn repeated_fragment_delta<'a>(old: Option<&str>, new: &'a str) -> &'a str {
    match old {
        Some(prev) if prev == new => "",
        Some(prev) => append_delta(prev, new),
        None => new,
    }
}

fn snapshot_delta<'a>(old: Option<&str>, new: &'a str) -> &'a str {
    match old {
        Some(prev) if looks_like_snapshot(prev, new) && new.starts_with(prev) => &new[prev.len()..],
        Some(prev) if looks_like_snapshot(prev, new) => "",
        _ => new,
    }
}

pin_project! {
    // 对 SSE 事件流应用 patch 状态机的包装流
    pub struct StateStream<S> {
        #[pin]
        inner: S,
        state: DsState,
        pending: Vec<DsFrame>,
    }
}

impl<S> StateStream<S> {
    /// 创建状态流包装器
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            state: DsState::default(),
            pending: Vec::new(),
        }
    }
}

impl<S, E> Stream for StateStream<S>
where
    S: Stream<Item = Result<SseEvent, E>>,
    E: Into<OpenAIAdapterError>,
{
    type Item = Result<DsFrame, OpenAIAdapterError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if let Some(frame) = this.pending.pop() {
            return Poll::Ready(Some(Ok(frame)));
        }

        loop {
            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(evt))) => {
                    let frames = this.state.apply_event(&evt);
                    if frames.is_empty() {
                        continue;
                    }
                    let mut frames = frames;
                    let first = frames.remove(0);
                    trace!(target: "adapter", ">>> state: {}", trace_frame(&first));
                    // 剩余帧按正序压入 pending（先压后出的会逆序，所以逆序 extend）
                    this.pending.extend(frames.into_iter().rev());
                    return Poll::Ready(Some(Ok(first)));
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(e.into())));
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// TRACE 日志用：截断长文本，其余变体直接 Debug
fn trace_frame(frame: &DsFrame) -> String {
    const MAX_LEN: usize = 60;
    match frame {
        DsFrame::ContentDelta(s) | DsFrame::ThinkDelta(s) => {
            let ty = if matches!(frame, DsFrame::ContentDelta(_)) {
                "ContentDelta"
            } else {
                "ThinkDelta"
            };
            if s.len() > MAX_LEN {
                format!("{}(\"{}\")", ty, &s[..MAX_LEN])
            } else {
                format!("{:?}", frame)
            }
        }
        _ => format!("{:?}", frame),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_content_with_explicit_append() {
        let mut state = DsState::default();
        state.fragments.push(Fragment {
            ty: "RESPONSE".into(),
            content: "".into(),
        });
        let evt = SseEvent {
            event: None,
            data: r#"{"p":"response/fragments/-1/content","o":"APPEND","v":"hello"}"#.into(),
        };
        let frames = state.apply_event(&evt);
        assert!(matches!(&frames[0], DsFrame::ContentDelta(s) if s == "hello"));
    }

    #[test]
    fn append_content_with_bare_v_after_path_set() {
        let mut state = DsState::default();
        state.fragments.push(Fragment {
            ty: "RESPONSE".into(),
            content: "hello".into(),
        });
        state.current_path = Some("response/fragments/-1/content".into());
        let evt = SseEvent {
            event: None,
            data: r#"{"v":" world"}"#.into(),
        };
        let frames = state.apply_event(&evt);
        assert!(matches!(&frames[0], DsFrame::ContentDelta(s) if s == " world"));
    }

    #[test]
    fn snapshot_then_append() {
        let mut state = DsState::default();
        let evt = SseEvent {
            event: None,
            data: r#"{"v":{"response":{"fragments":[{"type":"THINK","content":"hi"}]}}}"#.into(),
        };
        let frames = state.apply_event(&evt);
        assert!(matches!(&frames[0], DsFrame::ThinkDelta(s) if s == "hi"));
    }

    #[test]
    fn content_set_with_path_emits_only_new_suffix() {
        let mut state = DsState::default();
        state.fragments.push(Fragment {
            ty: "RESPONSE".into(),
            content: "hello".into(),
        });
        let evt = SseEvent {
            event: None,
            data: r#"{"p":"response/fragments/-1/content","v":"hello world"}"#.into(),
        };
        let frames = state.apply_event(&evt);
        assert_eq!(frames.len(), 1);
        assert!(matches!(&frames[0], DsFrame::ContentDelta(s) if s == " world"));
    }

    #[test]
    fn content_path_without_op_keeps_append_when_not_snapshot() {
        let mut state = DsState::default();
        state.fragments.push(Fragment {
            ty: "RESPONSE".into(),
            content: "hello".into(),
        });
        let evt = SseEvent {
            event: None,
            data: r#"{"p":"response/fragments/-1/content","v":" world"}"#.into(),
        };
        let frames = state.apply_event(&evt);
        assert_eq!(frames.len(), 1);
        assert!(matches!(&frames[0], DsFrame::ContentDelta(s) if s == " world"));
        assert_eq!(state.fragments[0].content, "hello world");
    }

    #[test]
    fn repeated_append_chunk_is_ignored() {
        let mut state = DsState::default();
        state.fragments.push(Fragment {
            ty: "RESPONSE".into(),
            content: "prefix repeated repeated".into(),
        });
        let evt = SseEvent {
            event: None,
            data: r#"{"p":"response/fragments/-1/content","o":"APPEND","v":" repeated repeated"}"#.into(),
        };
        let frames = state.apply_event(&evt);
        assert!(frames.is_empty());
        assert_eq!(state.fragments[0].content, "prefix repeated repeated");
    }

    #[test]
    fn repeated_fragment_append_initial_content_is_ignored() {
        let mut state = DsState::default();
        state.fragments.push(Fragment {
            ty: "RESPONSE".into(),
            content: "hello world".into(),
        });
        let evt = SseEvent {
            event: None,
            data: r#"{"p":"response/fragments","o":"APPEND","v":[{"type":"RESPONSE","content":"hello world"}]}"#.into(),
        };
        let frames = state.apply_event(&evt);
        assert!(frames.is_empty());
    }

    #[test]
    fn repeated_snapshot_does_not_reemit_content() {
        let mut state = DsState::default();
        let first = SseEvent {
            event: None,
            data: r#"{"v":{"response":{"fragments":[{"type":"RESPONSE","content":"hello"}]}}}"#.into(),
        };
        let second = first.clone();

        let frames = state.apply_event(&first);
        assert!(matches!(&frames[0], DsFrame::ContentDelta(s) if s == "hello"));
        let frames = state.apply_event(&second);
        assert!(frames.is_empty());
    }

    #[test]
    fn ready_and_finish_events() {
        let mut state = DsState::default();
        assert!(matches!(
            state.apply_event(&SseEvent {
                event: Some("ready".into()),
                data: "{}".into(),
            })[0],
            DsFrame::Role
        ));
        assert!(matches!(
            state.apply_event(&SseEvent {
                event: Some("finish".into()),
                data: "{}".into(),
            })[0],
            DsFrame::Finish
        ));
    }

    #[test]
    fn batch_accumulated_token_usage() {
        let mut state = DsState::default();
        let evt = SseEvent {
            event: None,
            data: r#"{"p":"response","o":"BATCH","v":[{"p":"accumulated_token_usage","v":41},{"p":"quasi_status","v":"FINISHED"}]}"#.into(),
        };
        let frames = state.apply_event(&evt);
        assert!(matches!(
            &frames[0],
            DsFrame::Usage(u) if *u == 41
        ));
    }
}
