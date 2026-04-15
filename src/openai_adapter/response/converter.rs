//! OpenAI Chunk 生成器 —— 将 DsFrame 映射为 ChatCompletionChunk

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use pin_project_lite::pin_project;

use crate::openai_adapter::OpenAIAdapterError;
use crate::openai_adapter::types::{ChatCompletionChunk, ChunkChoice, Delta, Usage};

use super::state::DsFrame;
use super::{next_chatcmpl_id, now_secs};

fn make_usage_chunk(usage: Usage, model: &str) -> ChatCompletionChunk {
    ChatCompletionChunk {
        id: next_chatcmpl_id(),
        object: "chat.completion.chunk",
        created: now_secs(),
        model: model.to_string(),
        choices: vec![],
        usage: Some(usage),
        service_tier: None,
        system_fingerprint: None,
    }
}

fn make_usage(prompt_tokens: u32, completion_tokens: u32) -> Usage {
    Usage {
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
        prompt_tokens_details: None,
        completion_tokens_details: None,
    }
}

fn make_chunk(model: &str, delta: Delta, finish: Option<&'static str>) -> ChatCompletionChunk {
    ChatCompletionChunk {
        id: next_chatcmpl_id(),
        object: "chat.completion.chunk",
        created: now_secs(),
        model: model.to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta,
            finish_reason: finish,
            logprobs: None,
        }],
        usage: None,
        service_tier: None,
        system_fingerprint: None,
    }
}

pin_project! {
    #[allow(unused_doc_comments)]
    /// 将 DsFrame 增量帧映射为 OpenAI ChatCompletionChunk 的流转换器
    pub struct ConverterStream<S> {
        #[pin]
        inner: S,
        model: String,
        include_usage: bool,
        include_obfuscation: bool,
        prompt_tokens: u32,
        finished: bool,
        usage_value: Option<u32>,
    }
}

impl<S> ConverterStream<S> {
    /// 创建 Chunk 转换流
    pub fn new(
        inner: S,
        model: String,
        include_usage: bool,
        include_obfuscation: bool,
        prompt_tokens: u32,
    ) -> Self {
        Self {
            inner,
            model,
            include_usage,
            include_obfuscation,
            prompt_tokens,
            finished: false,
            usage_value: None,
        }
    }
}

impl<S> Stream for ConverterStream<S>
where
    S: Stream<Item = Result<DsFrame, OpenAIAdapterError>>,
{
    type Item = Result<ChatCompletionChunk, OpenAIAdapterError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        // 如果已结束且有待发 usage，优先发送
        if *this.finished
            && *this.include_usage
            && let Some(u) = this.usage_value.take()
        {
            return Poll::Ready(Some(Ok(make_usage_chunk(
                make_usage(*this.prompt_tokens, u),
                this.model,
            ))));
        }

        loop {
            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(frame))) => match frame {
                    DsFrame::Role => {
                        return Poll::Ready(Some(Ok(make_chunk(
                            this.model,
                            Delta {
                                role: Some("assistant"),
                                ..Default::default()
                            },
                            None,
                        ))));
                    }
                    DsFrame::ThinkDelta(text) => {
                        return Poll::Ready(Some(Ok(make_chunk(
                            this.model,
                            Delta {
                                reasoning_content: Some(text),
                                ..Default::default()
                            },
                            None,
                        ))));
                    }
                    DsFrame::ContentDelta(text) => {
                        return Poll::Ready(Some(Ok(make_chunk(
                            this.model,
                            Delta {
                                content: Some(text),
                                ..Default::default()
                            },
                            None,
                        ))));
                    }
                    DsFrame::Status(status) if status == "FINISHED" && !*this.finished => {
                        *this.finished = true;
                        return Poll::Ready(Some(Ok(make_chunk(
                            this.model,
                            Delta::default(),
                            Some("stop"),
                        ))));
                    }
                    DsFrame::Status(_) => {}
                    DsFrame::Usage(u) => {
                        *this.usage_value = Some(u);
                        if *this.finished && *this.include_usage {
                            return Poll::Ready(Some(Ok(make_usage_chunk(
                                make_usage(*this.prompt_tokens, u),
                                this.model,
                            ))));
                        }
                    }
                    DsFrame::Finish if !*this.finished => {
                        *this.finished = true;
                        return Poll::Ready(Some(Ok(make_chunk(
                            this.model,
                            Delta::default(),
                            Some("stop"),
                        ))));
                    }
                    DsFrame::Finish => {}
                },
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
                Poll::Ready(None) => {
                    if *this.finished
                        && *this.include_usage
                        && let Some(u) = this.usage_value.take()
                    {
                        return Poll::Ready(Some(Ok(make_usage_chunk(
                            make_usage(*this.prompt_tokens, u),
                            this.model,
                        ))));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;

    use super::super::state::DsFrame;

    use super::*;

    #[tokio::test]
    async fn converter_emits_role_and_content() {
        let frames = futures::stream::iter(vec![
            Ok(DsFrame::Role),
            Ok(DsFrame::ContentDelta("hello".into())),
        ]);
        let mut conv = ConverterStream::new(frames, "deepseek-default".into(), false, false, 0);
        let chunk1 = conv.next().await.unwrap().unwrap();
        assert_eq!(chunk1.choices[0].delta.role, Some("assistant"));
        let chunk2 = conv.next().await.unwrap().unwrap();
        assert_eq!(chunk2.choices[0].delta.content.as_deref(), Some("hello"));
    }
}
