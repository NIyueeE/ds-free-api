//! Anthropic 响应映射 —— 将 OpenAI ChatCompletion 映射为 Anthropic Message
//!
//! 门面模块：声明子模块，暴露共享类型和辅助函数。
//! `MessagesResponse` / `Usage` 定义在 `types.rs`（与请求类型同模块）。

mod aggregate;
mod stream;

pub(crate) use aggregate::from_chat_completions;
pub(crate) use stream::from_chat_completion_stream;

/// 响应内容块——在 `types.rs` 中定义为 `ResponseContentBlock`，此处别名保持子模块兼容
pub(crate) use crate::anthropic_compat::types::ResponseContentBlock as ContentBlock;

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
    } else if let Some(suffix) = openai_id.strip_prefix("call_") {
        format!("toolu_{}", suffix)
    } else {
        format!("msg_{}", openai_id)
    }
}
