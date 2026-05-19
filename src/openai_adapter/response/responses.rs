use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use pin_project_lite::pin_project;

use crate::OpenAIAdapterError;
use super::super::types::*;

static RESPONSE_ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_response_id() -> String {
    let n = RESPONSE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("resp-{:016x}", n)
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
        if let Some(content) = &choice.message.content {
            if !content.is_empty() {
                output.push(OutputItem::Text { text: content.clone() });
            }
        }

        if let Some(reasoning) = &choice.message.reasoning_content {
            if !reasoning.is_empty() {
                output.push(OutputItem::Text { text: reasoning.clone() });
            }
        }

        if let Some(tool_calls) = &choice.message.tool_calls {
            has_tool_calls = true;
            for tc in tool_calls {
                if let Some(func) = &tc.function {
                    output.push(OutputItem::ToolCall {
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
) -> Pin<Box<dyn Stream<Item = Result<ResponseChunk, OpenAIAdapterError>> + Send>> {
    Box::pin(ResponseChunkStream {
        inner: stream,
        response_id: next_response_id(),
        created_at: now_secs_f64(),
        model: String::new(),
        first_chunk: true,
    })
}

pin_project! {
    struct ResponseChunkStream<S> {
        #[pin]
        inner: S,
        response_id: String,
        created_at: f64,
        model: String,
        first_chunk: bool,
    }
}

impl<S> Stream for ResponseChunkStream<S>
where
    S: Stream<Item = Result<ChatCompletionsResponseChunk, OpenAIAdapterError>> + Send,
{
    type Item = Result<ResponseChunk, OpenAIAdapterError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(Some(Ok(chunk))) => {
                if *this.first_chunk {
                    *this.first_chunk = false;
                    *this.model = chunk.model.clone();
                }

                let delta = chunk.choices.first().and_then(|choice| {
                    let mut text: Option<String> = None;
                    let mut tool_call: Option<ToolCallDelta> = None;

                    if let Some(content) = &choice.delta.content {
                        text = Some(content.clone());
                    }

                    if let Some(tcs) = &choice.delta.tool_calls {
                        if let Some(tc) = tcs.first() {
                            tool_call = Some(ToolCallDelta {
                                id: tc.id.clone(),
                                function: tc.function.clone(),
                            });
                        }
                    }

                    if text.is_some() || tool_call.is_some() {
                        Some(ChunkDelta { text, tool_call })
                    } else {
                        None
                    }
                });

                let finish_reason = chunk.choices.first().and_then(|c| c.finish_reason);
                let is_finish = finish_reason.is_some();

                let status = if is_finish {
                    Some("completed".to_string())
                } else {
                    None
                };

                let response_chunk = ResponseChunk {
                    id: this.response_id.clone(),
                    object: "response.chunk",
                    created_at: *this.created_at,
                    model: this.model.clone(),
                    delta,
                    output: None,
                    status,
                    usage: chunk.usage.clone(),
                };

                Poll::Ready(Some(Ok(response_chunk)))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub fn response_chunk_sse_serialize(chunk: &ResponseChunk) -> Result<bytes::Bytes, OpenAIAdapterError> {
    let mut buf = Vec::with_capacity(256);
    buf.extend_from_slice(b"data: ");
    serde_json::to_writer(&mut buf, chunk).map_err(OpenAIAdapterError::from)?;
    buf.extend_from_slice(b"\n\n");
    Ok(bytes::Bytes::from(buf))
}
