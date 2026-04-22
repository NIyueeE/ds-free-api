//! Anthropic 请求映射 —— 将 Anthropic Messages 请求映射为 OpenAI ChatCompletion 请求
//!
//! 不支持的字段（top_k、cache_control 等）兼容解析但不传入核心流程。

#![allow(dead_code)]

use log::debug;

use serde::Deserialize;
use serde_json::json;

use crate::anthropic_compat::AnthropicCompatError;

// ============================================================================
// Anthropic 请求类型
// ============================================================================

/// POST /v1/messages 请求体
#[derive(Debug, Deserialize)]
pub struct MessagesRequest {
    pub model: String,
    pub messages: Vec<MessageParam>,
    pub max_tokens: u32,

    #[serde(default)]
    pub system: Option<SystemContent>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub top_k: Option<u32>,
    #[serde(default)]
    pub tools: Option<Vec<ToolUnion>>,
    #[serde(default)]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default)]
    pub thinking: Option<ThinkingConfig>,
    #[serde(default)]
    pub metadata: Option<Metadata>,
    #[serde(default)]
    pub output_config: Option<OutputConfig>,

    // 兼容字段：解析但不消费
    #[serde(default)]
    pub cache_control: Option<CacheControlEphemeral>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub inference_geo: Option<String>,
    #[serde(default)]
    pub service_tier: Option<String>,

    // 兜底
    #[serde(flatten)]
    pub _extra: serde_json::Value,
}

/// 消息参数
#[derive(Debug, Deserialize, Clone)]
pub struct MessageParam {
    pub role: String,
    pub content: MessageContent,
}

/// 消息内容：纯文本或内容块数组
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// 内容块
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: Option<ToolResultContent>,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    RedactedThinking {
        data: String,
    },
    // 其他类型（document / search_result / server_tool_use 等）直接忽略
    #[serde(other)]
    Other,
}

/// 图片源
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ImageSource {
    #[serde(rename = "base64")]
    Base64 { data: String, media_type: String },
    #[serde(rename = "url")]
    Url { url: String },
}

/// tool_result 内容：字符串或块数组
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// system 参数：字符串或文本块数组
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum SystemContent {
    Text(String),
    Blocks(Vec<SystemTextBlock>),
}

/// system 文本块（仅提取 text，忽略 cache_control / citations）
#[derive(Debug, Deserialize, Clone)]
pub struct SystemTextBlock {
    pub text: String,
    #[serde(rename = "type")]
    pub ty: String,
}

/// 工具联合类型
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ToolUnion {
    #[serde(rename = "custom")]
    Custom {
        name: String,
        #[serde(default)]
        description: Option<String>,
        input_schema: serde_json::Value,
        #[serde(default)]
        strict: Option<bool>,
    },
    // 服务器工具（bash / code_execution / web_search 等）忽略
    #[serde(other)]
    Other,
}

/// tool_choice 参数
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ToolChoice {
    #[serde(rename = "auto")]
    Auto {
        #[serde(default)]
        disable_parallel_tool_use: bool,
    },
    #[serde(rename = "any")]
    Any {
        #[serde(default)]
        disable_parallel_tool_use: bool,
    },
    #[serde(rename = "tool")]
    Tool {
        name: String,
        #[serde(default)]
        disable_parallel_tool_use: bool,
    },
    #[serde(rename = "none")]
    None,
}

/// thinking 配置
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum ThinkingConfig {
    #[serde(rename = "enabled")]
    Enabled {
        budget_tokens: u32,
        #[serde(default)]
        display: Option<String>,
    },
    #[serde(rename = "disabled")]
    Disabled,
    #[serde(rename = "adaptive")]
    Adaptive {
        #[serde(default)]
        display: Option<String>,
    },
}

/// 请求元数据
#[derive(Debug, Deserialize, Clone)]
pub struct Metadata {
    #[serde(default)]
    pub user_id: Option<String>,
}

/// 输出配置
#[derive(Debug, Deserialize, Clone)]
pub struct OutputConfig {
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub format: Option<JsonOutputFormat>,
}

/// JSON 输出格式
#[derive(Debug, Deserialize, Clone)]
pub struct JsonOutputFormat {
    pub schema: serde_json::Value,
    #[serde(rename = "type")]
    pub ty: String,
}

/// cache_control（兼容解析）
#[derive(Debug, Deserialize, Clone)]
pub struct CacheControlEphemeral {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub ttl: Option<String>,
}

// ============================================================================
// 映射函数
// ============================================================================

/// 将 Anthropic Messages 请求 JSON 映射为 OpenAI ChatCompletion 请求 JSON
pub fn to_openai_request(body: &[u8]) -> Result<Vec<u8>, AnthropicCompatError> {
    let req: MessagesRequest = serde_json::from_slice(body)
        .map_err(|e| AnthropicCompatError::BadRequest(format!("bad request: {}", e)))?;
    debug!(target: "anthropic_compat::request", "解析成功: model={}, messages={}, stream={}", req.model, req.messages.len(), req.stream);

    let mut openai = serde_json::Map::new();

    openai.insert("model".to_string(), json!(req.model));
    openai.insert("max_tokens".to_string(), json!(req.max_tokens));

    // messages: system 前置 + messages 转换
    let mut messages = Vec::new();
    if let Some(system) = req.system {
        messages.push(system_to_openai(&system));
    }
    for msg in &req.messages {
        messages.extend(message_param_to_openai(msg));
    }
    openai.insert("messages".to_string(), json!(messages));

    // stream
    if req.stream {
        openai.insert("stream".to_string(), json!(true));
    }

    // stop_sequences -> stop
    if let Some(stop) = req.stop_sequences
        && !stop.is_empty()
    {
        openai.insert("stop".to_string(), json!(stop));
    }

    // temperature
    if let Some(t) = req.temperature {
        openai.insert("temperature".to_string(), json!(t));
    }

    // top_p
    if let Some(p) = req.top_p {
        openai.insert("top_p".to_string(), json!(p));
    }

    // top_k: 当前不映射到 OpenAI（OpenAI 无 top_k 参数）

    // tools
    let mut parallel_tool_calls_disabled = false;
    if let Some(tools) = req.tools {
        let openai_tools: Vec<serde_json::Value> =
            tools.iter().filter_map(tool_union_to_openai).collect();
        if !openai_tools.is_empty() {
            openai.insert("tools".to_string(), json!(openai_tools));
        }
    }

    // tool_choice
    if let Some(tc) = req.tool_choice {
        parallel_tool_calls_disabled = tc.disable_parallel();
        openai.insert("tool_choice".to_string(), tc.to_openai());
    }

    if parallel_tool_calls_disabled {
        openai.insert("parallel_tool_calls".to_string(), json!(false));
    }

    // thinking -> reasoning_effort
    if let Some(thinking) = req.thinking {
        let effort = match thinking {
            ThinkingConfig::Enabled { .. } | ThinkingConfig::Adaptive { .. } => "high",
            ThinkingConfig::Disabled => "minimal",
        };
        openai.insert("reasoning_effort".to_string(), json!(effort));
    }

    // output_config.format -> response_format
    if let Some(output_config) = req.output_config
        && let Some(fmt) = output_config.format
    {
        openai.insert(
            "response_format".to_string(),
            json!({
                "type": "json_schema",
                "json_schema": fmt.schema
            }),
        );
    }

    serde_json::to_vec(&openai)
        .map_err(|e| AnthropicCompatError::Internal(format!("json error: {}", e)))
}

// ============================================================================
// 辅助函数
// ============================================================================

fn system_to_openai(system: &SystemContent) -> serde_json::Value {
    let text = match system {
        SystemContent::Text(t) => t.clone(),
        SystemContent::Blocks(blocks) => blocks
            .iter()
            .map(|b| b.text.clone())
            .collect::<Vec<_>>()
            .join("\n"),
    };
    json!({"role": "system", "content": text})
}

fn message_param_to_openai(msg: &MessageParam) -> Vec<serde_json::Value> {
    let blocks = match &msg.content {
        MessageContent::Text(t) => {
            return vec![json!({"role": msg.role, "content": t})];
        }
        MessageContent::Blocks(b) => b,
    };

    match msg.role.as_str() {
        "assistant" => assistant_blocks_to_openai(blocks),
        "user" => user_blocks_to_openai(blocks),
        _ => {
            // 其他 role 直接当作文本处理
            let text = extract_text_from_blocks(blocks);
            vec![json!({"role": msg.role, "content": text})]
        }
    }
}

/// 将 assistant 的 content blocks 映射为 OpenAI 消息
/// - text -> content
/// - tool_use -> tool_calls
/// - thinking / redacted_thinking / other -> 跳过
fn assistant_blocks_to_openai(blocks: &[ContentBlock]) -> Vec<serde_json::Value> {
    let mut texts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in blocks {
        match block {
            ContentBlock::Text { text } => texts.push(text.clone()),
            ContentBlock::ToolUse { id, name, input } => {
                tool_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": input.to_string()
                    }
                }));
            }
            ContentBlock::Thinking { .. }
            | ContentBlock::RedactedThinking { .. }
            | ContentBlock::Image { .. }
            | ContentBlock::ToolResult { .. }
            | ContentBlock::Other => {}
        }
    }

    let mut msg = serde_json::Map::new();
    msg.insert("role".to_string(), json!("assistant"));

    let content = if texts.is_empty() {
        json!(null)
    } else {
        json!(texts.join("\n"))
    };
    msg.insert("content".to_string(), content);

    if !tool_calls.is_empty() {
        msg.insert("tool_calls".to_string(), json!(tool_calls));
    }

    vec![json!(msg)]
}

/// 将 user 的 content blocks 映射为 OpenAI 消息
/// - text -> text content
/// - image -> image_url content parts
/// - tool_result -> tool role message(s)
/// - thinking / other -> 跳过
fn user_blocks_to_openai(blocks: &[ContentBlock]) -> Vec<serde_json::Value> {
    let mut text_parts = Vec::new();
    let mut image_parts = Vec::new();
    let mut tool_results = Vec::new();

    for block in blocks {
        match block {
            ContentBlock::Text { text } => text_parts.push(text.clone()),
            ContentBlock::Image { source } => {
                let url = match source {
                    ImageSource::Base64 { data, media_type } => {
                        format!("data:{};base64,{}", media_type, data)
                    }
                    ImageSource::Url { url } => url.clone(),
                };
                image_parts.push(url);
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
            } => {
                let text = match content {
                    Some(ToolResultContent::Text(t)) => t.clone(),
                    Some(ToolResultContent::Blocks(b)) => extract_text_from_blocks(b),
                    None => String::new(),
                };
                tool_results.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_use_id,
                    "content": text
                }));
            }
            ContentBlock::Thinking { .. }
            | ContentBlock::RedactedThinking { .. }
            | ContentBlock::ToolUse { .. }
            | ContentBlock::Other => {}
        }
    }

    let mut result = Vec::new();

    // 文本 + 图片合并为一个 user message
    if !text_parts.is_empty() || !image_parts.is_empty() {
        if image_parts.is_empty() {
            // 纯文本：合并为单个字符串
            result.push(json!({"role": "user", "content": text_parts.join("\n")}));
        } else {
            // 包含图片：使用 parts 数组
            let mut parts = Vec::new();
            for text in &text_parts {
                parts.push(json!({"type": "text", "text": text}));
            }
            for url in &image_parts {
                parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
            }
            result.push(json!({"role": "user", "content": parts}));
        }
    }

    // tool_result 作为独立的 tool role messages
    result.extend(tool_results);

    result
}

fn extract_text_from_blocks(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tool_union_to_openai(tool: &ToolUnion) -> Option<serde_json::Value> {
    match tool {
        ToolUnion::Custom {
            name,
            description,
            input_schema,
            strict,
        } => Some(json!({
            "type": "function",
            "function": {
                "name": name,
                "description": description.as_deref().unwrap_or(""),
                "parameters": input_schema,
                "strict": strict.unwrap_or(false)
            }
        })),
        ToolUnion::Other => None,
    }
}

impl ToolChoice {
    fn disable_parallel(&self) -> bool {
        match self {
            ToolChoice::Auto {
                disable_parallel_tool_use,
            }
            | ToolChoice::Any {
                disable_parallel_tool_use,
            }
            | ToolChoice::Tool {
                disable_parallel_tool_use,
                ..
            } => *disable_parallel_tool_use,
            ToolChoice::None => false,
        }
    }

    fn to_openai(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto { .. } => json!("auto"),
            ToolChoice::Any { .. } => json!("required"),
            ToolChoice::Tool { name, .. } => json!({
                "type": "function",
                "function": { "name": name }
            }),
            ToolChoice::None => json!("none"),
        }
    }
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_openai(json: &[u8]) -> serde_json::Value {
        serde_json::from_slice(json).unwrap()
    }

    #[test]
    fn basic_user_message() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 1024
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        assert_eq!(openai["model"], "deepseek-default");
        assert_eq!(openai["max_tokens"], 1024);
        assert_eq!(openai["messages"].as_array().unwrap().len(), 1);
        assert_eq!(openai["messages"][0]["role"], "user");
        assert_eq!(openai["messages"][0]["content"], "Hello");
    }

    #[test]
    fn system_as_string() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "system": "You are a helpful assistant."
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        let msgs = openai["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are a helpful assistant.");
        assert_eq!(msgs[1]["role"], "user");
    }

    #[test]
    fn system_as_blocks() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "system": [{"type": "text", "text": "Sys1"}, {"type": "text", "text": "Sys2"}]
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        let msgs = openai["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["content"], "Sys1\nSys2");
    }

    #[test]
    fn user_with_text_blocks() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "Hello"}, {"type": "text", "text": "World"}]}
            ],
            "max_tokens": 1024
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        // 多文本块合并为单个字符串
        assert_eq!(openai["messages"][0]["content"], "Hello\nWorld");
    }

    #[test]
    fn assistant_with_tool_use() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "Let me check"},
                        {"type": "tool_use", "id": "toolu_01", "name": "get_weather", "input": {"city": "Beijing"}}
                    ]
                }
            ],
            "max_tokens": 1024
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        let msg = &openai["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert_eq!(msg["content"], "Let me check");
        let tool_calls = msg["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["id"], "toolu_01");
        assert_eq!(tool_calls[0]["type"], "function");
        assert_eq!(tool_calls[0]["function"]["name"], "get_weather");
        assert_eq!(
            tool_calls[0]["function"]["arguments"],
            r#"{"city":"Beijing"}"#
        );
    }

    #[test]
    fn user_with_tool_result() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "tool_result", "tool_use_id": "toolu_01", "content": "25C"}
                    ]
                }
            ],
            "max_tokens": 1024
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        let msgs = openai["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "tool");
        assert_eq!(msgs[0]["tool_call_id"], "toolu_01");
        assert_eq!(msgs[0]["content"], "25C");
    }

    #[test]
    fn stream_and_stop_sequences() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "stream": true,
            "stop_sequences": ["STOP", "HALT"],
            "temperature": 0.7,
            "top_p": 0.9
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        assert_eq!(openai["stream"], true);
        let stop = openai["stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0], "STOP");
        let temp = openai["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 0.001, "temperature mismatch: {}", temp);
        let top_p = openai["top_p"].as_f64().unwrap();
        assert!((top_p - 0.9).abs() < 0.001, "top_p mismatch: {}", top_p);
    }

    #[test]
    fn tools_mapping() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "tools": [
                {
                    "type": "custom",
                    "name": "get_weather",
                    "description": "Get weather",
                    "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}}
                }
            ],
            "tool_choice": {"type": "auto"}
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        let tools = openai["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "get_weather");
        assert_eq!(openai["tool_choice"], "auto");
    }

    #[test]
    fn tool_choice_named_tool() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "tools": [{"type": "custom", "name": "get_weather", "input_schema": {}}],
            "tool_choice": {"type": "tool", "name": "get_weather"}
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        let tc = &openai["tool_choice"];
        assert_eq!(tc["type"], "function");
        assert_eq!(tc["function"]["name"], "get_weather");
    }

    #[test]
    fn tool_choice_disable_parallel() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "tools": [{"type": "custom", "name": "f", "input_schema": {}}],
            "tool_choice": {"type": "auto", "disable_parallel_tool_use": true}
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        assert_eq!(openai["parallel_tool_calls"], false);
    }

    #[test]
    fn thinking_enabled() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "thinking": {"type": "enabled", "budget_tokens": 2048}
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        assert_eq!(openai["reasoning_effort"], "high");
    }

    #[test]
    fn thinking_disabled() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "thinking": {"type": "disabled"}
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        assert_eq!(openai["reasoning_effort"], "minimal");
    }

    #[test]
    fn output_config_json_schema() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "output_config": {"format": {"type": "json_schema", "schema": {"type": "object"}}}
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        assert_eq!(openai["response_format"]["type"], "json_schema");
        assert_eq!(openai["response_format"]["json_schema"]["type"], "object");
    }

    #[test]
    fn unknown_content_blocks_skipped() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Hello"},
                        {"type": "document", "source": {"type": "url", "url": "http://example.com"}}
                    ]
                }
            ],
            "max_tokens": 1024
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        assert_eq!(openai["messages"][0]["content"], "Hello");
    }

    #[test]
    fn top_k_not_mapped() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "top_k": 40
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        assert!(openai.get("top_k").is_none());
    }

    #[test]
    fn server_tools_ignored() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1024,
            "tools": [
                {"type": "custom", "name": "my_tool", "input_schema": {}},
                {"type": "bash_20250124", "name": "bash"}
            ]
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        let tools = openai["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "my_tool");
    }

    #[test]
    fn image_base64_mapped() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Describe this"},
                        {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "abc123"}}
                    ]
                }
            ],
            "max_tokens": 1024
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        let content = openai["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/jpeg;base64,abc123"
        );
    }

    #[test]
    fn image_url_mapped() {
        let body = br#"{
            "model": "deepseek-default",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "image", "source": {"type": "url", "url": "https://example.com/img.jpg"}}
                    ]
                }
            ],
            "max_tokens": 1024
        }"#;

        let openai = parse_openai(&to_openai_request(body).unwrap());
        let content = openai["messages"][0]["content"].as_array().unwrap();
        assert_eq!(
            content[0]["image_url"]["url"],
            "https://example.com/img.jpg"
        );
    }

    #[test]
    fn malformed_json_error() {
        let body = b"not-json";
        let err = to_openai_request(body).unwrap_err();
        assert!(matches!(err, AnthropicCompatError::BadRequest(_)));
    }
}
