//! Anthropic Models API 响应生成
//!
//! 基于 openai_adapter 的模型列表，转换为 Anthropic /v1/models 响应格式。

use log::debug;

use serde::{Deserialize, Serialize};

use crate::openai_adapter::OpenAIAdapter;

// ============================================================================
// 临时结构：反序列化 OpenAI 格式的模型列表
// ============================================================================

#[derive(Debug, Deserialize)]
struct OpenAIModelList {
    data: Vec<OpenAIModel>,
}

#[derive(Debug, Deserialize)]
struct OpenAIModel {
    id: String,
    created: u64,
    #[serde(default)]
    max_input_tokens: Option<u32>,
    #[serde(default)]
    max_output_tokens: Option<u32>,
}

// ============================================================================
// Anthropic 协议类型
// ============================================================================

/// 模型能力信息
#[derive(Debug, Serialize, Deserialize)]
struct ModelCapabilities {
    thinking: ThinkingCapability,
    image_input: CapabilitySupport,
    pdf_input: CapabilitySupport,
    structured_outputs: CapabilitySupport,
}

/// Thinking 能力
#[derive(Debug, Serialize, Deserialize)]
struct ThinkingCapability {
    supported: bool,
    types: ThinkingTypes,
}

/// Thinking 类型支持
#[derive(Debug, Serialize, Deserialize)]
struct ThinkingTypes {
    enabled: CapabilitySupport,
    adaptive: CapabilitySupport,
}

/// 单项能力支持
#[derive(Debug, Serialize, Deserialize)]
struct CapabilitySupport {
    supported: bool,
}

/// 单个模型信息
#[derive(Debug, Serialize, Deserialize)]
struct ModelInfo {
    id: String,
    #[serde(rename = "type")]
    ty: String,
    display_name: String,
    created_at: String,
    /// 暂不填充，待 openai_adapter 返回实际值后启用
    #[serde(skip_serializing_if = "Option::is_none")]
    max_input_tokens: Option<u32>,
    /// 暂不填充，待 openai_adapter 返回实际值后启用
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    capabilities: ModelCapabilities,
}

/// 模型列表响应
#[derive(Debug, Serialize, Deserialize)]
struct ModelListResponse {
    data: Vec<ModelInfo>,
    has_more: bool,
    first_id: String,
    last_id: String,
}

// ============================================================================
// 响应生成
// ============================================================================

/// 根据 openai_adapter 的模型列表生成 Anthropic 格式响应
pub(crate) fn list(adapter: &OpenAIAdapter) -> Vec<u8> {
    debug!(target: "anthropic_compat::models", "生成模型列表");
    list_from_json(&adapter.list_models())
}

/// 查询单个模型
pub(crate) fn get(adapter: &OpenAIAdapter, model_id: &str) -> Option<Vec<u8>> {
    debug!(target: "anthropic_compat::models", "查询模型: {}", model_id);
    get_from_json(&adapter.list_models(), model_id)
}

// 内部实现，方便测试
fn list_from_json(openai_json: &[u8]) -> Vec<u8> {
    let openai_list: OpenAIModelList = match serde_json::from_slice(openai_json) {
        Ok(list) => list,
        Err(_) => return br#"{"data":[],"has_more":false,"first_id":"","last_id":""}"#.to_vec(),
    };

    let data: Vec<ModelInfo> = openai_list
        .data
        .into_iter()
        .map(|m| to_anthropic_model(&m))
        .collect();

    let first_id = data.first().map(|m| m.id.clone()).unwrap_or_default();
    let last_id = data.last().map(|m| m.id.clone()).unwrap_or_default();

    let resp = ModelListResponse {
        data,
        has_more: false,
        first_id,
        last_id,
    };

    serde_json::to_vec(&resp)
        .unwrap_or_else(|_| br#"{"data":[],"has_more":false,"first_id":"","last_id":""}"#.to_vec())
}

fn get_from_json(openai_json: &[u8], model_id: &str) -> Option<Vec<u8>> {
    let openai_list: OpenAIModelList = serde_json::from_slice(openai_json).ok()?;

    openai_list
        .data
        .into_iter()
        .find(|m| m.id == model_id)
        .map(|m| {
            let info = to_anthropic_model(&m);
            serde_json::to_vec(&info).unwrap_or_else(|_| br#"{}"#.to_vec())
        })
}

// ============================================================================
// 模型映射
// ============================================================================

/// 将 OpenAI 模型信息映射为 Anthropic ModelInfo
fn to_anthropic_model(m: &OpenAIModel) -> ModelInfo {
    let display_name = id_to_display_name(&m.id);

    ModelInfo {
        id: m.id.clone(),
        ty: "model".to_string(),
        display_name,
        created_at: unix_to_rfc3339(m.created),
        max_input_tokens: m.max_input_tokens,
        max_tokens: m.max_output_tokens,
        capabilities: ModelCapabilities {
            thinking: ThinkingCapability {
                supported: true,
                types: ThinkingTypes {
                    enabled: CapabilitySupport { supported: true },
                    adaptive: CapabilitySupport { supported: true },
                },
            },
            image_input: CapabilitySupport { supported: true },
            pdf_input: CapabilitySupport { supported: true },
            structured_outputs: CapabilitySupport { supported: true },
        },
    }
}

/// 将 "deepseek-expert" 解析为 "DeepSeek Expert"
fn id_to_display_name(id: &str) -> String {
    id.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 将 Unix 时间戳（秒）转为 RFC 3339 格式字符串（UTC）
///
/// 手动实现，避免引入日期库。支持 1970–2100 年范围。
fn unix_to_rfc3339(secs: u64) -> String {
    let days = secs / 86_400;
    let rem_secs = secs % 86_400;

    let hour = (rem_secs / 3_600) as u32;
    let rem = rem_secs % 3_600;
    let minute = (rem / 60) as u32;
    let second = (rem % 60) as u32;

    let mut year = 1970u64;
    let mut remaining_days = days;

    loop {
        let year_len = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < year_len {
            break;
        }
        remaining_days -= year_len;
        year += 1;
    }

    let month_days = [
        31,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];

    let mut month = 1u32;
    let mut day = (remaining_days + 1) as u32;
    for dim in &month_days {
        if day <= *dim {
            break;
        }
        day -= dim;
        month += 1;
    }

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, minute, second
    )
}

fn is_leap_year(y: u64) -> bool {
    y.is_multiple_of(4) && !y.is_multiple_of(100) || y.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_maps_openai_models() {
        let openai_json = br#"{"object":"list","data":[{"id":"deepseek-default","object":"model","created":1090108800,"owned_by":"x"},{"id":"deepseek-expert","object":"model","created":1090108800,"owned_by":"x"}]}"#;
        let anthropic_json = list_from_json(openai_json);
        let resp: ModelListResponse = serde_json::from_slice(&anthropic_json).unwrap();
        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.data[0].id, "deepseek-default");
        assert_eq!(resp.data[1].id, "deepseek-expert");
        assert_eq!(resp.data[0].ty, "model");
        assert!(resp.data[0].capabilities.thinking.supported);
        assert!(resp.data[0].capabilities.image_input.supported);
        assert!(resp.data[0].capabilities.pdf_input.supported);
        assert_eq!(resp.data[0].created_at, "2004-07-18T00:00:00Z");
        assert!(resp.data[0].max_input_tokens.is_none());
    }

    #[test]
    fn get_finds_existing_model() {
        let openai_json = br#"{"object":"list","data":[{"id":"deepseek-default","object":"model","created":1090108800,"owned_by":"x"}]}"#;
        let json = get_from_json(openai_json, "deepseek-default").unwrap();
        let info: ModelInfo = serde_json::from_slice(&json).unwrap();
        assert_eq!(info.id, "deepseek-default");
        assert_eq!(info.display_name, "Deepseek Default");
    }

    #[test]
    fn get_returns_none_for_missing_model() {
        let openai_json = br#"{"object":"list","data":[]}"#;
        assert!(get_from_json(openai_json, "deepseek-default").is_none());
    }

    #[test]
    fn list_handles_empty_data() {
        let anthropic_json = list_from_json(br#"{"object":"list","data":[]}"#);
        let resp: ModelListResponse = serde_json::from_slice(&anthropic_json).unwrap();
        assert!(resp.data.is_empty());
        assert!(!resp.has_more);
    }

    #[test]
    fn list_handles_malformed_json() {
        let json = list_from_json(b"not-json");
        assert_eq!(
            &json,
            br#"{"data":[],"has_more":false,"first_id":"","last_id":""}"#
        );
    }
}
