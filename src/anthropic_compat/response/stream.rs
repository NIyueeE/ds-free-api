//! 流式响应映射 —— 将 ChatCompletionsResponseChunk 流映射为 MessagesResponseChunk 流

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use log::{debug, trace};
use pin_project_lite::pin_project;

use super::{finish_reason_map, map_id};
use crate::anthropic_compat::AnthropicCompatError;
use crate::anthropic_compat::types::{
    ContentBlockDelta, MessagesResponse, MessagesResponseChunk, ResponseContentBlock, Usage,
};
use crate::openai_adapter::OpenAIAdapterError;
use crate::openai_adapter::types::ChatCompletionsResponseChunk;

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
    fn new() -> Self {
        Self {
            block_kind: BlockKind::None,
            block_index: 0,
            model: String::new(),
            message_id: String::new(),
            input_tokens: 0,
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

    fn make_message_start(&self) -> MessagesResponseChunk {
        MessagesResponseChunk::MessageStart {
            message: MessagesResponse {
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

    fn transition_to(&mut self, kind: BlockKind) -> Vec<MessagesResponseChunk> {
        let mut events = Vec::new();
        if self.block_kind != BlockKind::None {
            events.push(MessagesResponseChunk::ContentBlockStop {
                index: self.block_index,
            });
            self.block_index += 1;
        }
        self.block_kind = kind;
        events
    }

    fn handle_chunk(&mut self, chunk: ChatCompletionsResponseChunk) -> Vec<MessagesResponseChunk> {
        let mut events = Vec::new();

        // 保活块 → 持续 thinking 块（不要独立块免干扰客户端）
        if chunk.id == "chatcmpl-keepalive" && self.started {
            if self.block_kind != BlockKind::Thinking {
                events.extend(self.transition_to(BlockKind::Thinking));
                events.push(MessagesResponseChunk::ContentBlockStart {
                    index: self.block_index,
                    content_block: ResponseContentBlock::Thinking {
                        thinking: String::new(),
                        signature: String::new(),
                    },
                });
            }
            events.push(MessagesResponseChunk::ContentBlockDelta {
                index: self.block_index,
                delta: ContentBlockDelta::Thinking {
                    thinking: "tool_calls...".to_string(),
                },
            });
            return events;
        }

        // role chunk → message_start（此时 chunk 已携带 prompt_tokens）
        if !self.started
            && let Some(choice) = chunk.choices.first()
            && choice.delta.role == Some("assistant")
        {
            self.start(chunk.id, chunk.model);
            if let Some(ref u) = chunk.usage {
                self.input_tokens = u.prompt_tokens;
            }
            events.push(self.make_message_start());
            return events;
        }

        // 优先提取 usage（可能独立 chunk 或与 finish 同 chunk）
        if let Some(ref u) = chunk.usage {
            self.completion_tokens = Some(u.completion_tokens);
        }

        let choice = match chunk.choices.first() {
            Some(c) => c,
            None => return events,
        };

        let delta = &choice.delta;

        // reasoning_content
        if let Some(ref text) = delta.reasoning_content
            && !text.is_empty()
        {
            if self.block_kind != BlockKind::Thinking {
                events.extend(self.transition_to(BlockKind::Thinking));
                events.push(MessagesResponseChunk::ContentBlockStart {
                    index: self.block_index,
                    content_block: ResponseContentBlock::Thinking {
                        thinking: String::new(),
                        signature: String::new(),
                    },
                });
            }
            events.push(MessagesResponseChunk::ContentBlockDelta {
                index: self.block_index,
                delta: ContentBlockDelta::Thinking {
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
                events.push(MessagesResponseChunk::ContentBlockStart {
                    index: self.block_index,
                    content_block: ResponseContentBlock::Text {
                        text: String::new(),
                    },
                });
            }
            events.push(MessagesResponseChunk::ContentBlockDelta {
                index: self.block_index,
                delta: ContentBlockDelta::Text { text: text.clone() },
            });
        }

        // tool_calls（一次性完整输出）
        if let Some(ref calls) = delta.tool_calls
            && !calls.is_empty()
        {
            events.extend(self.transition_to(BlockKind::ToolUse));
            for call in calls {
                let (name, partial_json) = if let Some(ref func) = call.function {
                    (func.name.clone(), func.arguments.clone())
                } else if let Some(ref custom) = call.custom {
                    let json =
                        serde_json::to_string(&custom.input).unwrap_or_else(|_| "{}".to_string());
                    (custom.name.clone(), json)
                } else {
                    (String::new(), "{}".to_string())
                };
                events.push(MessagesResponseChunk::ContentBlockStart {
                    index: self.block_index,
                    content_block: ResponseContentBlock::ToolUse {
                        id: map_id(&call.id),
                        name: name.clone(),
                        input: serde_json::json!({}),
                    },
                });
                events.push(MessagesResponseChunk::ContentBlockDelta {
                    index: self.block_index,
                    delta: ContentBlockDelta::InputJson { partial_json },
                });
                events.push(MessagesResponseChunk::ContentBlockStop {
                    index: self.block_index,
                });
                self.block_index += 1;
            }
            self.block_kind = BlockKind::None;
        }

        // finish_reason
        if let Some(reason) = choice.finish_reason
            && !self.finished
        {
            self.finished = true;
            events.extend(self.transition_to(BlockKind::None));
            let stop_reason = finish_reason_map(reason);
            events.push(MessagesResponseChunk::MessageDelta {
                stop_reason: Some(stop_reason),
                stop_sequence: None,
                output_tokens: Some(self.completion_tokens.unwrap_or(0)),
            });
            events.push(MessagesResponseChunk::MessageStop);
        }

        events
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
        pending_events: Vec<MessagesResponseChunk>,
    }
}

impl<S> AnthropicStream<S> {
    fn new(inner: S) -> Self {
        Self {
            inner,
            state: StreamState::new(),
            pending_events: Vec::new(),
        }
    }
}

impl<S> Stream for AnthropicStream<S>
where
    S: Stream<Item = Result<ChatCompletionsResponseChunk, OpenAIAdapterError>>,
{
    type Item = Result<MessagesResponseChunk, AnthropicCompatError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        // 优先输出待处理事件
        if !this.pending_events.is_empty() {
            let event = this.pending_events.remove(0);
            return Poll::Ready(Some(Ok(event)));
        }

        loop {
            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    trace!(target: "anthropic_compat::response::stream", "<<< {}",
                        serde_json::to_string(&chunk).unwrap_or_default());
                    let events = this.state.handle_chunk(chunk);
                    this.pending_events.extend(events);
                    if !this.pending_events.is_empty() {
                        let event = this.pending_events.remove(0);
                        return Poll::Ready(Some(Ok(event)));
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(AnthropicCompatError::from(e))));
                }
                Poll::Ready(None) => {
                    debug!(target: "anthropic_compat::response::stream", "流结束, started={}, finished={}", this.state.started, this.state.finished);
                    // 流结束但未收到 finish_reason：优雅关闭
                    if !this.state.finished && this.state.started {
                        this.state.finished = true;
                        let mut events: Vec<MessagesResponseChunk> =
                            this.state.transition_to(BlockKind::None);
                        events.push(MessagesResponseChunk::MessageDelta {
                            stop_reason: None,
                            stop_sequence: None,
                            output_tokens: Some(this.state.completion_tokens.unwrap_or(0)),
                        });
                        events.push(MessagesResponseChunk::MessageStop);
                        this.pending_events.extend(events);
                    }
                    if !this.pending_events.is_empty() {
                        let event = this.pending_events.remove(0);
                        return Poll::Ready(Some(Ok(event)));
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

/// 将 ChatCompletionsResponseChunk 流映射为 MessagesResponseChunk 流
pub fn from_chat_completion_stream<S>(
    openai_stream: S,
) -> Pin<Box<dyn Stream<Item = Result<MessagesResponseChunk, AnthropicCompatError>> + Send>>
where
    S: Stream<Item = Result<ChatCompletionsResponseChunk, OpenAIAdapterError>> + Send + 'static,
{
    debug!(target: "anthropic_compat::response::stream", "启动流式响应映射");
    Box::pin(AnthropicStream::new(openai_stream))
}
