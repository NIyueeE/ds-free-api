//! 非流式响应映射 —— 将 OpenAI ChatCompletion JSON 映射为 Anthropic Message JSON

use log::debug;

use super::{
    ContentBlock, Message, OpenAiCompletion, OpenAiToolCall, Usage, finish_reason_map, map_id,
};

/// 将 OpenAI ChatCompletion JSON 映射为 Anthropic Message JSON
pub fn from_chat_completion_bytes(openai_json: &[u8]) -> Result<Vec<u8>, serde_json::Error> {
    debug!(target: "anthropic_compat::response::aggregate", "开始映射非流式响应");
    let openai: OpenAiCompletion = serde_json::from_slice(openai_json)?;
    let msg = map_completion(&openai);
    debug!(target: "anthropic_compat::response::aggregate", "映射完成: content_blocks={}", msg.content.len());
    serde_json::to_vec(&msg)
}

fn map_completion(openai: &OpenAiCompletion) -> Message {
    let choice = openai.choices.first();
    let message = choice.map(|c| &c.message);

    let mut content: Vec<ContentBlock> = Vec::new();

    if let Some(msg) = message {
        if let Some(ref thinking) = msg.reasoning_content
            && !thinking.is_empty()
        {
            content.push(ContentBlock::Thinking {
                thinking: thinking.clone(),
                signature: String::new(),
            });
        }
        if let Some(ref text) = msg.content
            && !text.is_empty()
        {
            content.push(ContentBlock::Text { text: text.clone() });
        }
        if let Some(ref calls) = msg.tool_calls {
            for call in calls {
                let input = parse_tool_call_input(call);
                content.push(ContentBlock::ToolUse {
                    id: call.id.clone(),
                    name: tool_call_name(call),
                    input,
                });
            }
        }
    }

    let stop_reason = choice
        .and_then(|c| c.finish_reason.as_deref())
        .map(finish_reason_map);

    let usage = openai
        .usage
        .as_ref()
        .map(|u| Usage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        })
        .unwrap_or(Usage {
            input_tokens: 0,
            output_tokens: 0,
        });

    Message {
        id: map_id(&openai.id),
        ty: "message",
        role: "assistant",
        model: openai.model.clone(),
        content,
        stop_reason,
        stop_sequence: None,
        usage,
    }
}

fn tool_call_name(call: &OpenAiToolCall) -> String {
    call.function
        .as_ref()
        .map(|f| f.name.clone())
        .or_else(|| call.custom.as_ref().map(|c| c.name.clone()))
        .unwrap_or_default()
}

fn parse_tool_call_input(call: &OpenAiToolCall) -> serde_json::Value {
    if let Some(ref func) = call.function {
        serde_json::from_str(&func.arguments).unwrap_or_else(|_| serde_json::json!({}))
    } else if let Some(ref custom) = call.custom {
        custom
            .input
            .clone()
            .unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_response() {
        let openai_json = br#"{
            "id": "chatcmpl-1",
            "object": "chat.completion",
            "created": 1713700000,
            "model": "deepseek-default",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "hello world"
                },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        }"#;

        let bytes = from_chat_completion_bytes(openai_json).unwrap();
        let msg: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(msg["type"], "message");
        assert_eq!(msg["role"], "assistant");
        assert_eq!(msg["id"], "msg_1");
        assert_eq!(msg["model"], "deepseek-default");
        assert_eq!(msg["stop_reason"], "end_turn");
        assert_eq!(msg["usage"]["input_tokens"], 10);
        assert_eq!(msg["usage"]["output_tokens"], 5);
        assert_eq!(msg["content"].as_array().unwrap().len(), 1);
        assert_eq!(msg["content"][0]["type"], "text");
        assert_eq!(msg["content"][0]["text"], "hello world");
    }

    #[test]
    fn thinking_and_text_response() {
        let openai_json = br#"{
            "id": "chatcmpl-2",
            "model": "deepseek-expert",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "The answer is 42.",
                    "reasoning_content": "Let me think..."
                },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 20, "completion_tokens": 10 }
        }"#;

        let bytes = from_chat_completion_bytes(openai_json).unwrap();
        let msg: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(msg["content"].as_array().unwrap().len(), 2);
        assert_eq!(msg["content"][0]["type"], "thinking");
        assert_eq!(msg["content"][0]["thinking"], "Let me think...");
        assert_eq!(msg["content"][1]["type"], "text");
        assert_eq!(msg["content"][1]["text"], "The answer is 42.");
    }

    #[test]
    fn tool_calls_response() {
        let openai_json = br#"{
            "id": "chatcmpl-3",
            "model": "deepseek-default",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Beijing\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 15, "completion_tokens": 8 }
        }"#;

        let bytes = from_chat_completion_bytes(openai_json).unwrap();
        let msg: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(msg["stop_reason"], "tool_use");
        assert_eq!(msg["content"].as_array().unwrap().len(), 1);
        assert_eq!(msg["content"][0]["type"], "tool_use");
        assert_eq!(msg["content"][0]["id"], "call_abc");
        assert_eq!(msg["content"][0]["name"], "get_weather");
        assert_eq!(msg["content"][0]["input"]["city"], "Beijing");
    }

    #[test]
    fn text_and_tool_calls_response() {
        let openai_json = br#"{
            "id": "chatcmpl-4",
            "model": "deepseek-default",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Let me check the weather",
                    "tool_calls": [{
                        "id": "call_def",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 6 }
        }"#;

        let bytes = from_chat_completion_bytes(openai_json).unwrap();
        let msg: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(msg["content"].as_array().unwrap().len(), 2);
        assert_eq!(msg["content"][0]["type"], "text");
        assert_eq!(msg["content"][0]["text"], "Let me check the weather");
        assert_eq!(msg["content"][1]["type"], "tool_use");
        assert_eq!(msg["content"][1]["name"], "get_weather");
    }

    #[test]
    fn empty_content_skipped() {
        let openai_json = br#"{
            "id": "chatcmpl-5",
            "model": "deepseek-default",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": ""
                },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 1 }
        }"#;

        let bytes = from_chat_completion_bytes(openai_json).unwrap();
        let msg: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert!(msg["content"].as_array().unwrap().is_empty());
    }

    #[test]
    fn null_content_handled() {
        let openai_json = br#"{
            "id": "chatcmpl-6",
            "model": "deepseek-default",
            "choices": [{
                "message": {
                    "role": "assistant"
                },
                "finish_reason": "stop"
            }],
            "usage": null
        }"#;

        let bytes = from_chat_completion_bytes(openai_json).unwrap();
        let msg: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert!(msg["content"].as_array().unwrap().is_empty());
        assert_eq!(msg["usage"]["input_tokens"], 0);
        assert_eq!(msg["usage"]["output_tokens"], 0);
    }

    #[test]
    fn malformed_arguments_fallback_to_empty_object() {
        let openai_json = br#"{
            "id": "chatcmpl-7",
            "model": "deepseek-default",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_bad",
                        "type": "function",
                        "function": {
                            "name": "foo",
                            "arguments": "not-json"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 3 }
        }"#;

        let bytes = from_chat_completion_bytes(openai_json).unwrap();
        let msg: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(msg["content"][0]["input"], serde_json::json!({}));
    }

    #[test]
    fn no_choices_empty_content() {
        let openai_json = br#"{
            "id": "chatcmpl-empty",
            "model": "deepseek-default",
            "choices": [],
            "usage": { "prompt_tokens": 0, "completion_tokens": 0 }
        }"#;

        let bytes = from_chat_completion_bytes(openai_json).unwrap();
        let msg: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert!(msg["content"].as_array().unwrap().is_empty());
        assert_eq!(msg["stop_reason"], serde_json::Value::Null);
    }
}
