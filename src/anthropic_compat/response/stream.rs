//! 流式响应映射 —— 将 OpenAI ChatCompletionChunk SSE 流映射为 Anthropic Message SSE 流

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::Stream;
use log::{debug, trace};
use pin_project_lite::pin_project;
use serde::Serialize;

use super::{ContentBlock, Message, Usage, finish_reason_map, map_id};
use crate::anthropic_compat::AnthropicCompatError;
use crate::openai_adapter::OpenAIAdapterError;

// ============================================================================
// Anthropic 流式事件类型
// ============================================================================

#[derive(Debug, Serialize)]
struct MessageStartEvent {
    #[serde(rename = "type")]
    ty: &'static str,
    message: Message,
}

#[derive(Debug, Serialize)]
struct ContentBlockStartEvent {
    #[serde(rename = "type")]
    ty: &'static str,
    index: usize,
    content_block: ContentBlock,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum ContentBlockDelta {
    TextDelta { text: String },
    ThinkingDelta { thinking: String },
}

#[derive(Debug, Serialize)]
struct ContentBlockDeltaEvent {
    #[serde(rename = "type")]
    ty: &'static str,
    index: usize,
    delta: ContentBlockDelta,
}

#[derive(Debug, Serialize)]
struct ContentBlockStopEvent {
    #[serde(rename = "type")]
    ty: &'static str,
    index: usize,
}

#[derive(Debug, Serialize)]
struct MessageDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequence: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct MessageDeltaUsage {
    output_tokens: u32,
}

#[derive(Debug, Serialize)]
struct MessageDeltaEvent {
    #[serde(rename = "type")]
    ty: &'static str,
    delta: MessageDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<MessageDeltaUsage>,
}

#[derive(Debug, Serialize)]
struct MessageStopEvent {
    #[serde(rename = "type")]
    ty: &'static str,
}

// ============================================================================
// OpenAI chunk 反序列化（最小化结构）
// ============================================================================

#[derive(Debug, serde::Deserialize)]
struct OpenAiChunk {
    id: String,
    model: String,
    choices: Vec<OpenAiChunkChoice>,
    #[serde(default)]
    usage: Option<super::OpenAiUsage>,
}

#[derive(Debug, serde::Deserialize)]
struct OpenAiChunkChoice {
    #[serde(default)]
    finish_reason: Option<String>,
    delta: OpenAiDelta,
}

#[derive(Debug, serde::Deserialize, Default)]
struct OpenAiDelta {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<super::OpenAiToolCall>>,
}

// ============================================================================
// SSE 解析缓冲
// ============================================================================

struct SseBuffer {
    buf: String,
}

impl SseBuffer {
    fn new() -> Self {
        Self { buf: String::new() }
    }

    fn feed(&mut self, bytes: &Bytes) -> Vec<String> {
        self.buf.push_str(&String::from_utf8_lossy(bytes));
        let mut events = Vec::new();
        while let Some(pos) = self.buf.find("\n\n") {
            let event = self.buf[..pos].to_string();
            self.buf.drain(..pos + 2);
            for line in event.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    events.push(data.to_string());
                    break;
                }
            }
        }
        events
    }
}

// ============================================================================
// 状态机
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum BlockKind {
    None,
    Thinking,
    Text,
    ToolUse,
}

struct StreamState {
    block_kind: BlockKind,
    block_index: usize,
    model: String,
    message_id: String,
    input_tokens: u32,
    completion_tokens: Option<u32>,
    started: bool,
    finished: bool,
}

impl StreamState {
    fn new(input_tokens: u32) -> Self {
        Self {
            block_kind: BlockKind::None,
            block_index: 0,
            model: String::new(),
            message_id: String::new(),
            input_tokens,
            completion_tokens: None,
            started: false,
            finished: false,
        }
    }

    fn start(&mut self, id: String, model: String) {
        self.message_id = map_id(&id);
        self.model = model;
        self.started = true;
    }

    fn make_message_start(&self) -> MessageStartEvent {
        MessageStartEvent {
            ty: "message_start",
            message: Message {
                id: self.message_id.clone(),
                ty: "message",
                role: "assistant",
                model: self.model.clone(),
                content: Vec::new(),
                stop_reason: None,
                stop_sequence: None,
                usage: Usage {
                    input_tokens: self.input_tokens,
                    output_tokens: 0,
                },
            },
        }
    }

    fn transition_to(&mut self, kind: BlockKind) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        if self.block_kind != BlockKind::None {
            events.push(StreamEvent::ContentBlockStop {
                index: self.block_index,
            });
            self.block_index += 1;
        }
        self.block_kind = kind;
        events
    }

    fn handle_chunk(&mut self, chunk: OpenAiChunk) -> Vec<StreamEvent> {
        let mut events = Vec::new();

        // role chunk → message_start
        if !self.started
            && let Some(choice) = chunk.choices.first()
            && choice.delta.role.is_some()
        {
            self.start(chunk.id, chunk.model);
            events.push(StreamEvent::MessageStart(self.make_message_start()));
            return events;
        }

        let choice = match chunk.choices.first() {
            Some(c) => c,
            None => {
                // usage-only chunk
                if let Some(u) = chunk.usage {
                    self.completion_tokens = Some(u.completion_tokens);
                }
                return events;
            }
        };

        let delta = &choice.delta;

        // reasoning_content
        if let Some(ref text) = delta.reasoning_content
            && !text.is_empty()
        {
            if self.block_kind != BlockKind::Thinking {
                events.extend(self.transition_to(BlockKind::Thinking));
                events.push(StreamEvent::ContentBlockStart {
                    index: self.block_index,
                    content_block: ContentBlock::Thinking {
                        thinking: String::new(),
                        signature: String::new(),
                    },
                });
            }
            events.push(StreamEvent::ContentBlockDelta {
                index: self.block_index,
                delta: ContentBlockDelta::ThinkingDelta {
                    thinking: text.clone(),
                },
            });
        }

        // content
        if let Some(ref text) = delta.content
            && !text.is_empty()
        {
            if self.block_kind != BlockKind::Text {
                events.extend(self.transition_to(BlockKind::Text));
                events.push(StreamEvent::ContentBlockStart {
                    index: self.block_index,
                    content_block: ContentBlock::Text {
                        text: String::new(),
                    },
                });
            }
            events.push(StreamEvent::ContentBlockDelta {
                index: self.block_index,
                delta: ContentBlockDelta::TextDelta { text: text.clone() },
            });
        }

        // tool_calls（一次性完整输出）
        if let Some(ref calls) = delta.tool_calls
            && !calls.is_empty()
        {
            events.extend(self.transition_to(BlockKind::ToolUse));
            for call in calls {
                let input = if let Some(ref func) = call.function {
                    serde_json::from_str(&func.arguments).unwrap_or_else(|_| serde_json::json!({}))
                } else if let Some(ref custom) = call.custom {
                    custom
                        .input
                        .clone()
                        .unwrap_or_else(|| serde_json::json!({}))
                } else {
                    serde_json::json!({})
                };
                let name = call
                    .function
                    .as_ref()
                    .map(|f| f.name.clone())
                    .or_else(|| call.custom.as_ref().map(|c| c.name.clone()))
                    .unwrap_or_default();
                events.push(StreamEvent::ContentBlockStart {
                    index: self.block_index,
                    content_block: ContentBlock::ToolUse {
                        id: call.id.clone(),
                        name,
                        input,
                    },
                });
                events.push(StreamEvent::ContentBlockStop {
                    index: self.block_index,
                });
                self.block_index += 1;
            }
            // tool_use 是每个 call 一个 block，最后一个已经 +1 了
            // 但 transition_to 时已经设为 ToolUse，现在设为 None 表示当前无活跃 block
            self.block_kind = BlockKind::None;
        }

        // finish_reason
        if let Some(ref reason) = choice.finish_reason
            && !self.finished
        {
            self.finished = true;
            events.extend(self.transition_to(BlockKind::None));
            let stop_reason = finish_reason_map(reason);
            events.push(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: Some(stop_reason),
                    stop_sequence: None,
                },
                usage: Some(MessageDeltaUsage {
                    output_tokens: self.completion_tokens.unwrap_or(0),
                }),
            });
            events.push(StreamEvent::MessageStop);
        }

        events
    }
}

// ============================================================================
// 内部事件枚举（序列化前中间表示）
// ============================================================================

enum StreamEvent {
    MessageStart(MessageStartEvent),
    ContentBlockStart {
        index: usize,
        content_block: ContentBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: ContentBlockDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: MessageDelta,
        usage: Option<MessageDeltaUsage>,
    },
    MessageStop,
}

impl StreamEvent {
    fn to_sse_bytes(&self) -> Result<Bytes, serde_json::Error> {
        let json = match self {
            StreamEvent::MessageStart(e) => serde_json::to_string(e)?,
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => serde_json::to_string(&ContentBlockStartEvent {
                ty: "content_block_start",
                index: *index,
                content_block: content_block.clone(),
            })?,
            StreamEvent::ContentBlockDelta { index, delta } => {
                serde_json::to_string(&ContentBlockDeltaEvent {
                    ty: "content_block_delta",
                    index: *index,
                    delta: match delta {
                        ContentBlockDelta::TextDelta { text } => {
                            ContentBlockDelta::TextDelta { text: text.clone() }
                        }
                        ContentBlockDelta::ThinkingDelta { thinking } => {
                            ContentBlockDelta::ThinkingDelta {
                                thinking: thinking.clone(),
                            }
                        }
                    },
                })?
            }
            StreamEvent::ContentBlockStop { index } => {
                serde_json::to_string(&ContentBlockStopEvent {
                    ty: "content_block_stop",
                    index: *index,
                })?
            }
            StreamEvent::MessageDelta { delta, usage } => {
                serde_json::to_string(&MessageDeltaEvent {
                    ty: "message_delta",
                    delta: MessageDelta {
                        stop_reason: delta.stop_reason.clone(),
                        stop_sequence: delta.stop_sequence.clone(),
                    },
                    usage: usage.clone(),
                })?
            }
            StreamEvent::MessageStop => {
                serde_json::to_string(&MessageStopEvent { ty: "message_stop" })?
            }
        };
        Ok(Bytes::from(format!(
            "event: {}\ndata: {}\n\n",
            self.event_name(),
            json
        )))
    }

    fn event_name(&self) -> &'static str {
        match self {
            StreamEvent::MessageStart(_) => "message_start",
            StreamEvent::ContentBlockStart { .. } => "content_block_start",
            StreamEvent::ContentBlockDelta { .. } => "content_block_delta",
            StreamEvent::ContentBlockStop { .. } => "content_block_stop",
            StreamEvent::MessageDelta { .. } => "message_delta",
            StreamEvent::MessageStop => "message_stop",
        }
    }
}

// ============================================================================
// AnthropicStream 转换器
// ============================================================================

pin_project! {
    struct AnthropicStream<S> {
        #[pin]
        inner: S,
        state: StreamState,
        buffer: SseBuffer,
        pending_events: Vec<StreamEvent>,
    }
}

impl<S> AnthropicStream<S> {
    fn new(inner: S, input_tokens: u32) -> Self {
        Self {
            inner,
            state: StreamState::new(input_tokens),
            buffer: SseBuffer::new(),
            pending_events: Vec::new(),
        }
    }
}

impl<S> Stream for AnthropicStream<S>
where
    S: Stream<Item = Result<Bytes, OpenAIAdapterError>>,
{
    type Item = Result<Bytes, AnthropicCompatError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        // 优先输出待处理事件
        if !this.pending_events.is_empty() {
            let event = this.pending_events.remove(0);
            return Poll::Ready(Some(event.to_sse_bytes().map_err(|e| {
                AnthropicCompatError::Internal(format!("json serialization failed: {}", e))
            })));
        }

        loop {
            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    trace!(target: "anthropic_compat::response::stream", "收到 SSE 字节: {} bytes", bytes.len());
                    let datas = this.buffer.feed(&bytes);
                    for data in datas {
                        let chunk: OpenAiChunk = match serde_json::from_str(&data) {
                            Ok(c) => c,
                            Err(e) => {
                                return Poll::Ready(Some(Err(AnthropicCompatError::Internal(
                                    format!("json parse failed: {}", e),
                                ))));
                            }
                        };
                        let events = this.state.handle_chunk(chunk);
                        this.pending_events.extend(events);
                    }
                    if !this.pending_events.is_empty() {
                        let event = this.pending_events.remove(0);
                        return Poll::Ready(Some(event.to_sse_bytes().map_err(|e| {
                            AnthropicCompatError::Internal(format!(
                                "json serialization failed: {}",
                                e
                            ))
                        })));
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(AnthropicCompatError::from(e))));
                }
                Poll::Ready(None) => {
                    debug!(target: "anthropic_compat::response::stream", "OpenAI 流结束, started={}, finished={}", this.state.started, this.state.finished);
                    // 流结束但未收到 finish_reason：优雅关闭
                    if !this.state.finished && this.state.started {
                        this.state.finished = true;
                        let mut events = this.state.transition_to(BlockKind::None);
                        events.push(StreamEvent::MessageDelta {
                            delta: MessageDelta {
                                stop_reason: None,
                                stop_sequence: None,
                            },
                            usage: Some(MessageDeltaUsage {
                                output_tokens: this.state.completion_tokens.unwrap_or(0),
                            }),
                        });
                        events.push(StreamEvent::MessageStop);
                        this.pending_events.extend(events);
                    }
                    if !this.pending_events.is_empty() {
                        let event = this.pending_events.remove(0);
                        return Poll::Ready(Some(event.to_sse_bytes().map_err(|e| {
                            AnthropicCompatError::Internal(format!(
                                "json serialization failed: {}",
                                e
                            ))
                        })));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// ============================================================================
// 公共入口
// ============================================================================

/// 将 OpenAI ChatCompletionChunk SSE 流映射为 Anthropic Message SSE 流
pub fn from_chat_completion_stream<S>(
    openai_stream: S,
    input_tokens: u32,
) -> Pin<Box<dyn Stream<Item = Result<Bytes, AnthropicCompatError>> + Send>>
where
    S: Stream<Item = Result<Bytes, OpenAIAdapterError>> + Send + 'static,
{
    debug!(target: "anthropic_compat::response::stream", "启动流式响应映射, input_tokens={}", input_tokens);
    Box::pin(AnthropicStream::new(openai_stream, input_tokens))
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    fn openai_sse(data: &str) -> Bytes {
        Bytes::from(format!("data: {}\n\n", data))
    }

    async fn collect_events(
        st: Pin<Box<dyn Stream<Item = Result<Bytes, AnthropicCompatError>> + Send>>,
    ) -> Vec<(String, serde_json::Value)> {
        let mut out = Vec::new();
        let mut st = st;
        while let Some(res) = st.next().await {
            let text = String::from_utf8(res.unwrap().to_vec()).unwrap();
            let mut event_name = String::new();
            let mut data_json = String::new();
            for line in text.lines() {
                if line.starts_with("event: ") {
                    event_name = line.strip_prefix("event: ").unwrap().to_string();
                } else if line.starts_with("data: ") {
                    data_json = line.strip_prefix("data: ").unwrap().to_string();
                }
            }
            out.push((event_name, serde_json::from_str(&data_json).unwrap()));
        }
        out
    }

    #[tokio::test]
    async fn stream_plain_text() {
        let chunks = vec![
            Ok(openai_sse(
                r#"{"id":"chatcmpl-1","model":"deepseek-default","choices":[{"delta":{"role":"assistant"}}]}"#,
            )),
            Ok(openai_sse(
                r#"{"id":"chatcmpl-1","model":"deepseek-default","choices":[{"delta":{"content":"hello"}}]}"#,
            )),
            Ok(openai_sse(
                r#"{"id":"chatcmpl-1","model":"deepseek-default","choices":[{"delta":{"content":" world"},"finish_reason":"stop"}]}"#,
            )),
        ];
        let events = collect_events(from_chat_completion_stream(
            futures::stream::iter(chunks),
            10,
        ))
        .await;

        assert_eq!(events[0].0, "message_start");
        assert_eq!(events[0].1["message"]["id"], "msg_1");
        assert_eq!(events[0].1["message"]["usage"]["input_tokens"], 10);
        assert_eq!(events[0].1["message"]["usage"]["output_tokens"], 0);

        assert_eq!(events[1].0, "content_block_start");
        assert_eq!(events[1].1["index"], 0);
        assert_eq!(events[1].1["content_block"]["type"], "text");

        assert_eq!(events[2].0, "content_block_delta");
        assert_eq!(events[2].1["delta"]["type"], "text_delta");
        assert_eq!(events[2].1["delta"]["text"], "hello");

        assert_eq!(events[3].0, "content_block_delta");
        assert_eq!(events[3].1["delta"]["text"], " world");

        assert_eq!(events[4].0, "content_block_stop");
        assert_eq!(events[4].1["index"], 0);

        assert_eq!(events[5].0, "message_delta");
        assert_eq!(events[5].1["delta"]["stop_reason"], "end_turn");

        assert_eq!(events[6].0, "message_stop");
    }

    #[tokio::test]
    async fn stream_thinking_then_text() {
        let chunks = vec![
            Ok(openai_sse(
                r#"{"id":"chatcmpl-2","model":"deepseek-expert","choices":[{"delta":{"role":"assistant"}}]}"#,
            )),
            Ok(openai_sse(
                r#"{"id":"chatcmpl-2","model":"deepseek-expert","choices":[{"delta":{"reasoning_content":"Let me think..."}}]}"#,
            )),
            Ok(openai_sse(
                r#"{"id":"chatcmpl-2","model":"deepseek-expert","choices":[{"delta":{"content":"The answer is 42."},"finish_reason":"stop"}]}"#,
            )),
        ];
        let events = collect_events(from_chat_completion_stream(
            futures::stream::iter(chunks),
            20,
        ))
        .await;

        // message_start
        assert_eq!(events[0].0, "message_start");

        // thinking block
        assert_eq!(events[1].0, "content_block_start");
        assert_eq!(events[1].1["content_block"]["type"], "thinking");
        assert_eq!(events[2].0, "content_block_delta");
        assert_eq!(events[2].1["delta"]["type"], "thinking_delta");
        assert_eq!(events[2].1["delta"]["thinking"], "Let me think...");
        assert_eq!(events[3].0, "content_block_stop");

        // text block
        assert_eq!(events[4].0, "content_block_start");
        assert_eq!(events[4].1["content_block"]["type"], "text");
        assert_eq!(events[5].0, "content_block_delta");
        assert_eq!(events[5].1["delta"]["text"], "The answer is 42.");
        assert_eq!(events[6].0, "content_block_stop");

        // finish
        assert_eq!(events[7].0, "message_delta");
        assert_eq!(events[7].1["delta"]["stop_reason"], "end_turn");
        assert_eq!(events[8].0, "message_stop");
    }

    #[tokio::test]
    async fn stream_tool_calls() {
        let chunks = vec![
            Ok(openai_sse(
                r#"{"id":"chatcmpl-3","model":"deepseek-default","choices":[{"delta":{"role":"assistant"}}]}"#,
            )),
            Ok(openai_sse(
                r#"{"id":"chatcmpl-3","model":"deepseek-default","choices":[{"delta":{"tool_calls":[{"id":"call_abc","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Beijing\"}"}}]},"finish_reason":"tool_calls"}]}"#,
            )),
        ];
        let events = collect_events(from_chat_completion_stream(
            futures::stream::iter(chunks),
            15,
        ))
        .await;

        assert_eq!(events[0].0, "message_start");

        // tool_use block (start + stop, no delta)
        assert_eq!(events[1].0, "content_block_start");
        assert_eq!(events[1].1["content_block"]["type"], "tool_use");
        assert_eq!(events[1].1["content_block"]["id"], "call_abc");
        assert_eq!(events[1].1["content_block"]["name"], "get_weather");
        assert_eq!(events[1].1["content_block"]["input"]["city"], "Beijing");

        assert_eq!(events[2].0, "content_block_stop");

        assert_eq!(events[3].0, "message_delta");
        assert_eq!(events[3].1["delta"]["stop_reason"], "tool_use");

        assert_eq!(events[4].0, "message_stop");
    }

    #[tokio::test]
    async fn stream_with_usage() {
        let chunks = vec![
            Ok(openai_sse(
                r#"{"id":"chatcmpl-4","model":"m","choices":[{"delta":{"role":"assistant"}}]}"#,
            )),
            Ok(openai_sse(
                r#"{"id":"chatcmpl-4","model":"m","choices":[{"delta":{"content":"x"}}]}"#,
            )),
            Ok(openai_sse(
                r#"{"id":"chatcmpl-4","model":"m","choices":[],"usage":{"prompt_tokens":5,"completion_tokens":12,"total_tokens":17}}"#,
            )),
            Ok(openai_sse(
                r#"{"id":"chatcmpl-4","model":"m","choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            )),
        ];
        let events = collect_events(from_chat_completion_stream(
            futures::stream::iter(chunks),
            5,
        ))
        .await;

        let md_idx = events
            .iter()
            .position(|(n, _)| n == "message_delta")
            .unwrap();
        assert_eq!(events[md_idx].1["delta"]["stop_reason"], "end_turn");
        assert_eq!(events[md_idx].1["usage"]["output_tokens"], 12);
    }

    #[tokio::test]
    async fn stream_text_and_tool_calls() {
        let chunks = vec![
            Ok(openai_sse(
                r#"{"id":"chatcmpl-5","model":"m","choices":[{"delta":{"role":"assistant"}}]}"#,
            )),
            Ok(openai_sse(
                r#"{"id":"chatcmpl-5","model":"m","choices":[{"delta":{"content":"Let me check"}}]}"#,
            )),
            Ok(openai_sse(
                r#"{"id":"chatcmpl-5","model":"m","choices":[{"delta":{"tool_calls":[{"id":"call_def","type":"function","function":{"name":"get_weather","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#,
            )),
        ];
        let events = collect_events(from_chat_completion_stream(
            futures::stream::iter(chunks),
            12,
        ))
        .await;

        // text block
        let text_start = events
            .iter()
            .position(|(n, v)| n == "content_block_start" && v["content_block"]["type"] == "text")
            .unwrap();
        assert_eq!(events[text_start + 1].1["delta"]["text"], "Let me check");

        // tool_use block
        let tool_start = events
            .iter()
            .position(|(n, v)| {
                n == "content_block_start" && v["content_block"]["type"] == "tool_use"
            })
            .unwrap();
        assert_eq!(events[tool_start].1["content_block"]["name"], "get_weather");

        // finish
        let md_idx = events
            .iter()
            .position(|(n, _)| n == "message_delta")
            .unwrap();
        assert_eq!(events[md_idx].1["delta"]["stop_reason"], "tool_use");
    }
}
