//! Anthropic 响应映射 —— 将 OpenAI ChatCompletion 映射为 Anthropic Message
//!
//! 门面模块：定义共享类型，声明子模块。

mod aggregate;
mod stream;

pub(crate) use aggregate::from_chat_completion_bytes;
pub(crate) use stream::from_chat_completion_stream;

use serde::{Deserialize, Serialize};

/// Anthropic 非流式消息响应（流式的 message_start 也复用此结构）
#[derive(Debug, Serialize)]
pub struct Message {
    pub id: String,
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub role: &'static str,
    pub model: String,
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: Usage,
}

/// Content block 变体
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

/// Token 用量
#[derive(Debug, Serialize, Clone)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

// ============================================================================
// 共享反序列化类型（OpenAI 极简结构）
// ============================================================================

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiCompletion {
    pub id: String,
    pub model: String,
    pub choices: Vec<OpenAiChoice>,
    #[serde(default)]
    pub usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiChoice {
    #[serde(default)]
    pub finish_reason: Option<String>,
    pub message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct OpenAiToolCall {
    pub id: String,
    #[serde(default)]
    pub function: Option<OpenAiFunctionCall>,
    #[serde(default)]
    pub custom: Option<OpenAiCustomToolCall>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct OpenAiFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct OpenAiCustomToolCall {
    pub name: String,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub(crate) struct OpenAiUsage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
}

// ============================================================================
// 共享辅助函数
// ============================================================================

pub(crate) fn finish_reason_map(reason: &str) -> String {
    match reason {
        "stop" => "end_turn".to_string(),
        "tool_calls" => "tool_use".to_string(),
        _ => reason.to_string(),
    }
}

/// OpenAI id 格式为 chatcmpl-xxx，映射为 msg_xxx
pub(crate) fn map_id(openai_id: &str) -> String {
    if let Some(hex) = openai_id.strip_prefix("chatcmpl-") {
        format!("msg_{}", hex)
    } else {
        format!("msg_{}", openai_id)
    }
}
