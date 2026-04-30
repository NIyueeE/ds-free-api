//! Prompt 构建 —— 将 OpenAI messages 转换为 DeepSeek 原生标签格式
//!
//! 使用 `<｜System｜>`、`<｜User｜>`、`<｜Assistant｜>`、`<｜Tool｜>` 作为角色标记。
//! 若请求包含工具定义或行为指令，会嵌入到最后一个 `<｜Assistant｜>` 后的
//! 不闭合 `<think>` 块中，确保工具上下文始终紧邻模型生成位置。

use super::tools::ToolContext;
use crate::openai_adapter::response::{TOOL_CALL_END, TOOL_CALL_START};
use crate::openai_adapter::types::{ChatCompletionsRequest, ContentPart, Message, MessageContent};

/// 合并连续相同 role 的 message，避免 DeepSeek 模型对连续同角色标签产生混淆
fn merge_messages(messages: &[Message]) -> Vec<Message> {
    let mut merged: Vec<Message> = Vec::new();
    for msg in messages {
        if let Some(last) = merged.last_mut()
            && last.role == msg.role
        {
            // 合并 content
            if let Some(ref content) = msg.content {
                match &mut last.content {
                    Some(last_content) => match (last_content, content) {
                        (MessageContent::Text(a), MessageContent::Text(b)) => {
                            a.push('\n');
                            a.push_str(b);
                        }
                        (MessageContent::Parts(a), MessageContent::Parts(b)) => {
                            a.extend(b.clone());
                        }
                        // 不同类型 → 都转 text 拼接
                        (last_c, new_c) => {
                            let new_text = format_content(new_c);
                            let last_text = format_content(last_c);
                            *last_c = MessageContent::Text(format!("{}\n{}", last_text, new_text));
                        }
                    },
                    None => {
                        last.content = msg.content.clone();
                    }
                }
            }
            // 合并 tool_calls
            if let Some(ref calls) = msg.tool_calls {
                match &mut last.tool_calls {
                    Some(last_calls) => last_calls.extend(calls.clone()),
                    None => last.tool_calls = msg.tool_calls.clone(),
                }
            }
            // 覆盖字段：取最后一条的值
            if msg.name.is_some() {
                last.name.clone_from(&msg.name);
            }
            if msg.tool_call_id.is_some() {
                last.tool_call_id.clone_from(&msg.tool_call_id);
            }
            if msg.function_call.is_some() {
                last.function_call.clone_from(&msg.function_call);
            }
            if msg.refusal.is_some() {
                last.refusal.clone_from(&msg.refusal);
            }
            if msg.audio.is_some() {
                last.audio.clone_from(&msg.audio);
            }
            continue;
        }
        merged.push(msg.clone());
    }
    merged
}

/// 构建 DeepSeek 原生标签格式的 prompt 字符串
/// 顺序: [system(含 reminder)] [历史 user/tool/assistant 轮次...] <｜Assistant｜><think>[reminder]
pub fn build(req: &ChatCompletionsRequest, tool_ctx: &ToolContext) -> String {
    let messages = merge_messages(&req.messages);
    let mut parts: Vec<String> = messages.iter().map(format_message).collect();

    let mut tool_sections: Vec<String> = Vec::new();

    if let Some(text) = tool_ctx.format_block.as_deref() {
        tool_sections.push(format!("### 格式规范\n{}", text));
    }
    if let Some(text) = tool_ctx.defs_text.as_deref() {
        tool_sections.push(format!("### 工具定义\n{}", text));
    }
    if let Some(text) = tool_ctx.instruction_text.as_deref() {
        tool_sections.push(format!("### 调用指令\n{}", text));
    }

    let mut reminder_parts: Vec<String> = Vec::new();

    if !tool_sections.is_empty() {
        reminder_parts.push(format!("## 工具调用\n{}", tool_sections.join("\n\n")));
    }

    // response_format 降级：将格式约束注入到 <think> 块中
    if let Some(rf) = &req.response_format {
        let text = match rf.ty.as_str() {
            "json_object" => {
                "请直接输出合法的 JSON 对象，不要包含任何 markdown 代码块标记或其他解释性文字。"
                    .into()
            }
            "json_schema" => {
                let schema_text = rf
                    .json_schema
                    .as_ref()
                    .map(|s| serde_json::to_string(s).unwrap_or_default())
                    .unwrap_or_default();
                if schema_text.is_empty() {
                    "以 JSON 的形式输出。".into()
                } else {
                    format!(
                        "以 JSON 的形式输出，输出的 JSON 需遵守以下的格式：\n\n~~~json\n{}\n~~~",
                        schema_text
                    )
                }
            }
            "text" => String::new(),
            _ => format!("请以 {} 格式输出。", rf.ty),
        };
        if !text.is_empty() {
            reminder_parts.push(format!("## 输出格式\n{}", text));
        }
    }

    if !reminder_parts.is_empty() {
        let reminder_body = reminder_parts.join("\n\n");

        // System 尾部注入完整 reminder（不含"嗯"前缀，含工具定义）
        let sys_content = format!("\n\n{}", reminder_body);
        if let Some(sys) = parts.iter_mut().find(|p| p.starts_with("<｜System｜>")) {
            if let Some(end) = sys.rfind('\n') {
                sys.insert_str(end, &sys_content);
            }
        } else {
            parts.insert(0, format!("<｜System｜>{}<｜System｜>\n", sys_content));
        }

        // <think> 中不含工具定义，只含格式规范和调用指令
        let mut think_sections: Vec<String> = Vec::new();
        if let Some(text) = tool_ctx.format_block.as_deref() {
            think_sections.push(format!("### 格式规范\n{}", text));
        }
        if let Some(text) = tool_ctx.instruction_text.as_deref() {
            think_sections.push(format!("### 调用指令\n{}", text));
        }
        let mut think_parts: Vec<String> = Vec::new();
        if !think_sections.is_empty() {
            think_parts.push(format!("## 工具调用\n{}", think_sections.join("\n\n")));
        }
        // response_format only in think
        if let Some(rf) = &req.response_format {
            let text = match rf.ty.as_str() {
                "json_object" => {
                    "请直接输出合法的 JSON 对象，不要包含任何 markdown 代码块标记或其他解释性文字。"
                        .into()
                }
                "json_schema" => {
                    let schema_text = rf
                        .json_schema
                        .as_ref()
                        .map(|s| serde_json::to_string(s).unwrap_or_default())
                        .unwrap_or_default();
                    if schema_text.is_empty() {
                        "以 JSON 的形式输出。".into()
                    } else {
                        format!(
                            "以 JSON 的形式输出，输出的 JSON 需遵守以下的格式：\n\n~~~json\n{}\n~~~",
                            schema_text
                        )
                    }
                }
                "text" => String::new(),
                _ => format!("请以 {} 格式输出。", rf.ty),
            };
            if !text.is_empty() {
                think_parts.push(format!("## 输出格式\n{}", text));
            }
        }
        if !think_parts.is_empty() {
            let think_reminder = format!(
                "嗯，我刚刚被系统提醒需要遵循以下内容:\n\n{}",
                think_parts.join("\n\n")
            );
            parts.push(format!("<｜Assistant｜><think>{}\n", think_reminder));
        }
    }

    parts.join("")
}

fn role_tag(role: &str) -> String {
    let mut r = role.to_string();
    if let Some(c) = r.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    format!("<｜{}｜>", r)
}

fn format_message(msg: &Message) -> String {
    let body = match msg.role.as_str() {
        "assistant" => format_assistant(msg),
        "tool" => format_tool(msg),
        "function" => format_function(msg),
        _ => format_generic(msg),
    };
    let tag = role_tag(&msg.role);
    format!("{}{}\n", tag, body)
}

fn format_generic(msg: &Message) -> String {
    let mut parts = Vec::new();
    if let Some(name) = &msg.name {
        parts.push(format!("(name: {name})"));
    }
    if let Some(content) = &msg.content {
        parts.push(format_content(content));
    }
    parts.join("\n")
}

fn format_assistant(msg: &Message) -> String {
    let mut parts = Vec::new();
    if let Some(content) = &msg.content {
        parts.push(format_content(content));
    }
    if let Some(tool_calls) = &msg.tool_calls {
        let items: Vec<String> = tool_calls
            .iter()
            .filter_map(|tc| {
                tc.function.as_ref().map(|func| {
                    let args = serde_json::from_str::<serde_json::Value>(&func.arguments)
                        .unwrap_or(serde_json::Value::Null);
                    format!(
                        "{{\"name\": {}, \"arguments\": {}}}",
                        serde_json::to_string(&func.name).unwrap_or_else(|_| "\"\"".into()),
                        serde_json::to_string(&args).unwrap_or_else(|_| "null".into()),
                    )
                })
            })
            .collect();
        parts.push(format!(
            "{TOOL_CALL_START}\n[{}]\n{TOOL_CALL_END}",
            items.join(", ")
        ));
    }
    if let Some(fc) = &msg.function_call {
        let args = serde_json::from_str::<serde_json::Value>(&fc.arguments)
            .unwrap_or(serde_json::Value::Null);
        let item = format!(
            "{{\"name\": {}, \"arguments\": {}}}",
            serde_json::to_string(&fc.name).unwrap_or_else(|_| "\"\"".into()),
            serde_json::to_string(&args).unwrap_or_else(|_| "null".into()),
        );
        parts.push(format!("{TOOL_CALL_START}\n[{item}]\n{TOOL_CALL_END}"));
    }
    if let Some(refusal) = &msg.refusal {
        parts.push(format!("(refusal: {refusal})"));
    }
    parts.join("\n")
}

fn format_tool(msg: &Message) -> String {
    let mut parts = Vec::new();
    parts.push("# 工具调用结果".to_string());
    if let Some(name) = &msg.name {
        parts.push(format!("## 工具名称: {}", name));
    }
    if let Some(id) = &msg.tool_call_id {
        parts.push(format!("## 调用id: {}", id));
    }
    parts.push("## 调用结果:".to_string());
    if let Some(content) = &msg.content {
        parts.push("~~~".to_string());
        parts.push(format_content(content));
        parts.push("~~~".to_string());
    }
    parts.join("\n")
}

fn format_function(msg: &Message) -> String {
    let mut parts = Vec::new();
    if let Some(name) = &msg.name {
        parts.push(format!("(name: {name})"));
    }
    if let Some(content) = &msg.content {
        parts.push(format_content(content));
    }
    parts.join("\n")
}

fn format_content(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Parts(parts) => {
            parts.iter().map(format_part).collect::<Vec<_>>().join("\n")
        }
    }
}

fn format_part(part: &ContentPart) -> String {
    match part.ty.as_str() {
        "text" => part.text.clone().unwrap_or_default(),
        "refusal" => part.refusal.clone().unwrap_or_default(),
        "image_url" => {
            if let Some(img) = &part.image_url {
                if img.url.starts_with("http://") || img.url.starts_with("https://") {
                    format!("[请访问这个链接: {}]", img.url)
                } else {
                    let detail = img.detail.as_deref().unwrap_or("auto");
                    format!("[图片: detail={detail}]")
                }
            } else {
                "[图片]".to_string()
            }
        }
        "input_audio" => {
            let fmt = part
                .input_audio
                .as_ref()
                .map(|a| a.format.as_str())
                .unwrap_or("unknown");
            format!("[音频: format={fmt}]")
        }
        "file" => {
            let filename = part
                .file
                .as_ref()
                .and_then(|f| f.filename.as_deref())
                .unwrap_or("unknown");
            let desc = part.text.as_deref().filter(|t| !t.is_empty());
            match desc {
                Some(d) => format!("[文件: {d} (filename={filename})]"),
                None => format!("[文件: filename={filename}]"),
            }
        }
        _ => format!("[未支持的内容类型: {}]", part.ty),
    }
}
