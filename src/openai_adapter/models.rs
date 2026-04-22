//! OpenAI 模型列表响应生成
//!
//! 基于 DeepSeek model_types 静态生成 OpenAI /models 响应。

use crate::openai_adapter::types::{Model, ModelList};

const MODEL_CREATED: u64 = 1_090_108_800;
const MODEL_OWNED_BY: &str = "deepseek-web (proxied by https://github.com/NIyueeE)";

/// 根据 model_types 生成模型列表 JSON
pub fn list(
    model_types: &[String],
    max_input_tokens: &[u32],
    max_output_tokens: &[u32],
) -> Vec<u8> {
    let data: Vec<Model> = model_types
        .iter()
        .enumerate()
        .map(|(idx, ty)| {
            let input = max_input_tokens.get(idx).copied();
            let output = max_output_tokens.get(idx).copied();
            Model {
                id: format!("deepseek-{}", ty),
                object: "model",
                created: MODEL_CREATED,
                owned_by: MODEL_OWNED_BY,
                max_input_tokens: input,
                max_output_tokens: output,
                context_length: input,
                context_window: input,
                max_context_length: input,
                max_tokens: output,
                max_completion_tokens: output,
            }
        })
        .collect();

    serde_json::to_vec(&ModelList {
        object: "list",
        data,
    })
    .unwrap_or_else(|_| br#"{"object":"list","data":[]}"#.to_vec())
}

/// 查询单个模型
pub fn get(
    model_types: &[String],
    max_input_tokens: &[u32],
    max_output_tokens: &[u32],
    id: &str,
) -> Option<Vec<u8>> {
    let target = id.to_lowercase();
    model_types
        .iter()
        .enumerate()
        .find(|(_, ty)| format!("deepseek-{}", ty).to_lowercase() == target)
        .map(|(idx, ty)| {
            let input = max_input_tokens.get(idx).copied();
            let output = max_output_tokens.get(idx).copied();
            serde_json::to_vec(&Model {
                id: format!("deepseek-{}", ty),
                object: "model",
                created: MODEL_CREATED,
                owned_by: MODEL_OWNED_BY,
                max_input_tokens: input,
                max_output_tokens: output,
                context_length: input,
                context_window: input,
                max_context_length: input,
                max_tokens: output,
                max_completion_tokens: output,
            })
            .unwrap_or_else(|_| br#"{}"#.to_vec())
        })
}
