use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use pin_project_lite::pin_project;

use crate::OpenAIAdapterError;
use super::super::types::{
    ChatCompletionsResponse, ChatCompletionsResponseChunk, Response, ResponseEvent,
    OutputItemMessage, OutputTextPart,
};

static RESPONSE_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static ITEM_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_response_id() -> String {
    let n = RESPONSE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("resp-{:016x}", n)
}

fn next_item_id() -> String {
    let n = ITEM_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("msg-{:016x}", n)
}

pub fn now_secs_f64() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub fn from_chat_completions(chat: &ChatCompletionsResponse) -> Response {
    let mut output = Vec::new();
    let mut has_tool_calls = false;

    if let Some(choice) = chat.choices.first() {
        if let Some(content) = &choice.message.content
            && !content.is_empty()
        {
            output.push(super::super::types::OutputItem::Text { text: content.clone() });
        }

        if let Some(reasoning) = &choice.message.reasoning_content
            && !reasoning.is_empty()
        {
            output.push(super::super::types::OutputItem::Text { text: reasoning.clone() });
        }

        if let Some(tool_calls) = &choice.message.tool_calls {
            has_tool_calls = true;
            for tc in tool_calls {
                if let Some(func) = &tc.function {
                    output.push(super::super::types::OutputItem::ToolCall {
                        id: tc.id.clone(),
                        function: func.clone(),
                    });
                }
            }
        }
    }

    Response {
        id: chat.id.clone(),
        object: "response",
        created_at: chat.created as f64,
        model: chat.model.clone(),
        status: if has_tool_calls { "in_progress" } else { "completed" },
        output,
        usage: chat.usage.clone(),
        metadata: None,
        error: None,
        incomplete_details: None,
    }
}

pub fn from_chat_completion_stream(
    stream: Pin<Box<dyn Stream<Item = Result<ChatCompletionsResponseChunk, OpenAIAdapterError>> + Send>>,
) -> Pin<Box<dyn Stream<Item = Result<ResponseEvent, OpenAIAdapterError>> + Send>> {
    Box::pin(ResponseEventStream {
        inner: stream,
        response_id: next_response_id(),
        item_id: next_item_id(),
        created_at: now_secs_f64(),
        model: String::new(),
        state: StreamState::Initial,
        sequence_number: 0,
        accumulated_text: String::new(),
        pending_events: Vec::new(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamState {
    Initial,
    InProgress,
    Done,
}

pin_project! {
    struct ResponseEventStream<S> {
        #[pin]
        inner: S,
        response_id: String,
        item_id: String,
        created_at: f64,
        model: String,
        state: StreamState,
        sequence_number: u32,
        accumulated_text: String,
        pending_events: Vec<ResponseEvent>,
    }
}

impl<S> Stream for ResponseEventStream<S>
where
    S: Stream<Item = Result<ChatCompletionsResponseChunk, OpenAIAdapterError>> + Send,
{
    type Item = Result<ResponseEvent, OpenAIAdapterError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        // 如果有待处理的事件，先返回它们
        if !this.pending_events.is_empty() {
            return Poll::Ready(Some(Ok(this.pending_events.remove(0))));
        }

        match this.state {
            StreamState::Initial => {
                *this.state = StreamState::InProgress;
                
                // 发送 response.created 事件
                let response = Response {
                    id: this.response_id.clone(),
                    object: "response",
                    status: "in_progress",
                    created_at: *this.created_at,
                    service_tier: Some("default"),
                    model: this.model.clone(),
                    output: Vec::new(),
                    usage: None,
                    instructions: None,
                    max_output_tokens: None,
                    metadata: None,
                    error: None,
                    incomplete_details: None,
                };
                let seq = *this.sequence_number;
                *this.sequence_number += 1;
                
                Poll::Ready(Some(Ok(ResponseEvent::Created {
                    response,
                    sequence_number: seq,
                })))
            }
            StreamState::InProgress => {
                match this.inner.as_mut().poll_next(cx) {
                    Poll::Ready(None) => {
                        *this.state = StreamState::Done;
                        
                        // 发送完成事件
                        let seq1 = *this.sequence_number;
                        *this.sequence_number += 1;
                        
                        let text_done_event = ResponseEvent::OutputTextDone {
                            content_index: 0,
                            item_id: this.item_id.clone(),
                            output_index: 0,
                            sequence_number: seq1,
                            text: this.accumulated_text.clone(),
                        };
                        
                        let seq2 = *this.sequence_number;
                        *this.sequence_number += 1;
                        
                        let content_part = OutputTextPart {
                            part_type: "output_text",
                            annotations: Vec::new(),
                            logprobs: Vec::new(),
                            text: this.accumulated_text.clone(),
                        };
                        let content_part_done_event = ResponseEvent::ContentPartDone {
                            content_index: 0,
                            item_id: this.item_id.clone(),
                            output_index: 0,
                            part: content_part,
                            sequence_number: seq2,
                        };
                        
                        let seq3 = *this.sequence_number;
                        *this.sequence_number += 1;
                        
                        let item = OutputItemMessage {
                            id: this.item_id.clone(),
                            message_type: "message",
                            status: "completed",
                            content: vec![OutputTextPart {
                                part_type: "output_text",
                                annotations: Vec::new(),
                                logprobs: Vec::new(),
                                text: this.accumulated_text.clone(),
                            }],
                            phase: "final_answer",
                            role: "assistant",
                        };
                        let output_item_done_event = ResponseEvent::OutputItemDone {
                            item,
                            output_index: 0,
                            sequence_number: seq3,
                        };
                        
                        let seq4 = *this.sequence_number;
                        *this.sequence_number += 1;
                        
                        let response = Response {
                            id: this.response_id.clone(),
                            object: "response",
                            status: "completed",
                            created_at: *this.created_at,
                            service_tier: Some("default"),
                            model: this.model.clone(),
                            output: Vec::new(),
                            usage: None,
                            instructions: None,
                            max_output_tokens: None,
                            metadata: None,
                            error: None,
                            incomplete_details: None,
                        };
                        let completed_event = ResponseEvent::Completed {
                            response,
                            sequence_number: seq4,
                        };
                        
                        this.pending_events.push(content_part_done_event);
                        this.pending_events.push(output_item_done_event);
                        this.pending_events.push(completed_event);
                        
                        Poll::Ready(Some(Ok(text_done_event)))
                    }
                    Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
                    Poll::Ready(Some(Ok(chunk))) => {
                        if this.model.is_empty() {
                            *this.model = chunk.model.clone();
                        }
                        
                        // 如果是第一次有内容，需要先发送 in_progress、output_item.added、content_part.added
                        if this.accumulated_text.is_empty() && this.sequence_number == &1 {
                            // 发送 response.in_progress
                            let seq1 = *this.sequence_number;
                            *this.sequence_number += 1;
                            let response = Response {
                                id: this.response_id.clone(),
                                object: "response",
                                created_at: *this.created_at,
                                model: this.model.clone(),
                                status: "in_progress",
                                output: Vec::new(),
                                usage: None,
                                metadata: None,
                                error: None,
                                incomplete_details: None,
                            };
                            this.pending_events.push(ResponseEvent::InProgress {
                                response,
                                sequence_number: seq1,
                            });
                            
                            // 发送 response.output_item.added
                            let seq2 = *this.sequence_number;
                            *this.sequence_number += 1;
                            let item = OutputItemMessage {
                                id: this.item_id.clone(),
                                message_type: "message",
                                status: "in_progress",
                                content: Vec::new(),
                                phase: "final_answer",
                                role: "assistant",
                            };
                            this.pending_events.push(ResponseEvent::OutputItemAdded {
                                item,
                                output_index: 0,
                                sequence_number: seq2,
                            });
                            
                            // 发送 response.content_part.added
                            let seq3 = *this.sequence_number;
                            *this.sequence_number += 1;
                            let part = OutputTextPart {
                                part_type: "output_text",
                                annotations: Vec::new(),
                                logprobs: Vec::new(),
                                text: String::new(),
                            };
                            this.pending_events.push(ResponseEvent::ContentPartAdded {
                                content_index: 0,
                                item_id: this.item_id.clone(),
                                output_index: 0,
                                part,
                                sequence_number: seq3,
                            });
                        }
                        
                        // 处理文本增量
                        if let Some(choice) = chunk.choices.first() {
                            if let Some(content) = &choice.delta.content
                                && !content.is_empty()
                            {
                                this.accumulated_text.push_str(content);
                                
                                let seq = *this.sequence_number;
                                *this.sequence_number += 1;
                                
                                let delta_event = ResponseEvent::OutputTextDelta {
                                    content_index: 0,
                                    delta: content.clone(),
                                    item_id: this.item_id.clone(),
                                    output_index: 0,
                                    sequence_number: seq,
                                };
                                
                                if this.pending_events.is_empty() {
                                    return Poll::Ready(Some(Ok(delta_event)));
                                }
                                
                                this.pending_events.push(delta_event);
                                let first_event = this.pending_events.remove(0);
                                return Poll::Ready(Some(Ok(first_event)));
                            }
                            
                            // 检查是否完成
                            if let Some(_finish_reason) = choice.finish_reason {
                                // 发送 response.output_text.done
                                let seq1 = *this.sequence_number;
                                *this.sequence_number += 1;
                                
                                let text_done_event = ResponseEvent::OutputTextDone {
                                    content_index: 0,
                                    item_id: this.item_id.clone(),
                                    output_index: 0,
                                    sequence_number: seq1,
                                    text: this.accumulated_text.clone(),
                                };
                                
                                // 发送 response.content_part.done
                                let seq2 = *this.sequence_number;
                                *this.sequence_number += 1;
                                
                                let part = OutputTextPart {
                                    part_type: "output_text",
                                    annotations: Vec::new(),
                                    logprobs: Vec::new(),
                                    text: this.accumulated_text.clone(),
                                };
                                let content_part_done_event = ResponseEvent::ContentPartDone {
                                    content_index: 0,
                                    item_id: this.item_id.clone(),
                                    output_index: 0,
                                    part,
                                    sequence_number: seq2,
                                };
                                
                                // 发送 response.output_item.done
                                let seq3 = *this.sequence_number;
                                *this.sequence_number += 1;
                                
                                let item = OutputItemMessage {
                                    id: this.item_id.clone(),
                                    message_type: "message",
                                    status: "completed",
                                    content: vec![OutputTextPart {
                                        part_type: "output_text",
                                        annotations: Vec::new(),
                                        logprobs: Vec::new(),
                                        text: this.accumulated_text.clone(),
                                    }],
                                    phase: "final_answer",
                                    role: "assistant",
                                };
                                let output_item_done_event = ResponseEvent::OutputItemDone {
                                    item,
                                    output_index: 0,
                                    sequence_number: seq3,
                                };
                                
                                // 发送 response.completed
                                let seq4 = *this.sequence_number;
                                *this.sequence_number += 1;
                                
                                let mut response = Response {
                                    id: this.response_id.clone(),
                                    object: "response",
                                    created_at: *this.created_at,
                                    model: this.model.clone(),
                                    status: "completed",
                                    output: Vec::new(),
                                    usage: None,
                                    metadata: None,
                                    error: None,
                                    incomplete_details: None,
                                };
                                response.usage = chunk.usage.clone();
                                let completed_event = ResponseEvent::Completed {
                                    response,
                                    sequence_number: seq4,
                                };
                                
                                this.pending_events.push(content_part_done_event);
                                this.pending_events.push(output_item_done_event);
                                this.pending_events.push(completed_event);
                                *this.state = StreamState::Done;
                                
                                return Poll::Ready(Some(Ok(text_done_event)));
                            }
                        }
                        
                        if !this.pending_events.is_empty() {
                            let first_event = this.pending_events.remove(0);
                            return Poll::Ready(Some(Ok(first_event)));
                        }
                        
                        Poll::Pending
                    }
                    Poll::Pending => Poll::Pending,
                }
            }
            StreamState::Done => Poll::Ready(None),
        }
    }
}

impl Clone for ResponseEvent {
    fn clone(&self) -> Self {
        match self {
            ResponseEvent::Created { response, sequence_number } => {
                ResponseEvent::Created { 
                    response: response.clone(), 
                    sequence_number: *sequence_number 
                }
            }
            ResponseEvent::InProgress { response, sequence_number } => {
                ResponseEvent::InProgress { 
                    response: response.clone(), 
                    sequence_number: *sequence_number 
                }
            }
            ResponseEvent::OutputItemAdded { item, output_index, sequence_number } => {
                ResponseEvent::OutputItemAdded { 
                    item: item.clone(), 
                    output_index: *output_index, 
                    sequence_number: *sequence_number 
                }
            }
            ResponseEvent::ContentPartAdded { content_index, item_id, output_index, part, sequence_number } => {
                ResponseEvent::ContentPartAdded { 
                    content_index: *content_index, 
                    item_id: item_id.clone(), 
                    output_index: *output_index, 
                    part: part.clone(), 
                    sequence_number: *sequence_number 
                }
            }
            ResponseEvent::OutputTextDelta { content_index, delta, item_id, output_index, sequence_number } => {
                ResponseEvent::OutputTextDelta { 
                    content_index: *content_index, 
                    delta: delta.clone(), 
                    item_id: item_id.clone(), 
                    output_index: *output_index, 
                    sequence_number: *sequence_number 
                }
            }
            ResponseEvent::OutputTextDone { content_index, item_id, output_index, sequence_number, text } => {
                ResponseEvent::OutputTextDone { 
                    content_index: *content_index, 
                    item_id: item_id.clone(), 
                    output_index: *output_index, 
                    sequence_number: *sequence_number, 
                    text: text.clone() 
                }
            }
            ResponseEvent::ContentPartDone { content_index, item_id, output_index, part, sequence_number } => {
                ResponseEvent::ContentPartDone { 
                    content_index: *content_index, 
                    item_id: item_id.clone(), 
                    output_index: *output_index, 
                    part: part.clone(), 
                    sequence_number: *sequence_number 
                }
            }
            ResponseEvent::OutputItemDone { item, output_index, sequence_number } => {
                ResponseEvent::OutputItemDone { 
                    item: item.clone(), 
                    output_index: *output_index, 
                    sequence_number: *sequence_number 
                }
            }
            ResponseEvent::Completed { response, sequence_number } => {
                ResponseEvent::Completed { 
                    response: response.clone(), 
                    sequence_number: *sequence_number 
                }
            }
            ResponseEvent::Error { error, status } => {
                ResponseEvent::Error { 
                    error: error.clone(), 
                    status: *status 
                }
            }
        }
    }
}

pub fn response_event_sse_serialize(event: &ResponseEvent) -> Result<bytes::Bytes, OpenAIAdapterError> {
    let mut buf = Vec::with_capacity(512);
    
    // 添加事件类型
    let event_type = match event {
        ResponseEvent::Created { .. } => "response.created",
        ResponseEvent::InProgress { .. } => "response.in_progress",
        ResponseEvent::OutputItemAdded { .. } => "response.output_item.added",
        ResponseEvent::ContentPartAdded { .. } => "response.content_part.added",
        ResponseEvent::OutputTextDelta { .. } => "response.output_text.delta",
        ResponseEvent::OutputTextDone { .. } => "response.output_text.done",
        ResponseEvent::ContentPartDone { .. } => "response.content_part.done",
        ResponseEvent::OutputItemDone { .. } => "response.output_item.done",
        ResponseEvent::Completed { .. } => "response.completed",
        ResponseEvent::Error { .. } => "error",
    };
    
    buf.extend_from_slice(b"event: ");
    buf.extend_from_slice(event_type.as_bytes());
    buf.extend_from_slice(b"\n");
    
    // 添加数据：直接序列化事件内容
    buf.extend_from_slice(b"data: ");
    serde_json::to_writer(&mut buf, event).map_err(OpenAIAdapterError::from)?;
    buf.extend_from_slice(b"\n\n");
    
    Ok(bytes::Bytes::from(buf))
}

#[allow(dead_code)]
pub fn response_chunk_sse_serialize(_chunk: &super::super::types::ResponseChunk) -> bytes::Bytes {
    bytes::Bytes::new()
}
