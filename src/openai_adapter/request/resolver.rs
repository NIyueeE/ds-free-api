//! 模型解析 —— 将 OpenAI model 字段映射为 ds_core 能力标志
//!
//! 通过外部注入的 registry 实现模型别名到 model_type 的动态映射。

use std::collections::HashMap;

use crate::openai_adapter::types::WebSearchOptions;

/// 模型解析结果
pub struct ModelResolution {
    /// ds_core 使用的 model_type
    pub model_type: String,
    pub thinking_enabled: bool,
    pub search_enabled: bool,
}

/// 根据 model_id 和扩展参数解析模型配置
///
/// thinking_enabled 在 reasoning_effort 为 minimal/low/medium/high/xhigh 时启用。
/// 若 reasoning_effort 未提供，默认按 "high" 处理（即 reasoning 默认开启）。
/// search_enabled 在 web_search_options 存在时启用，默认关闭。
pub fn resolve(
    registry: &HashMap<String, String>,
    model_id: &str,
    reasoning_effort: Option<&str>,
    web_search_options: Option<&WebSearchOptions>,
) -> Result<ModelResolution, String> {
    let key = model_id.to_lowercase();
    let model_type = registry
        .get(&key)
        .cloned()
        .ok_or_else(|| format!("不支持的模型: {}", model_id))?;

    let reasoning_effort = reasoning_effort.unwrap_or("high");
    let thinking_enabled = matches!(
        reasoning_effort,
        "minimal" | "low" | "medium" | "high" | "xhigh"
    );

    let search_enabled = web_search_options.is_some();

    Ok(ModelResolution {
        model_type,
        thinking_enabled,
        search_enabled,
    })
}
