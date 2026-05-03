//! OpenAI 模型列表响应生成
//!
//! 基于 DeepSeek model_types + model_aliases 静态生成 OpenAI /models 响应。

use std::collections::HashMap;

use crate::openai_adapter::types::{OpenAIModel, OpenAIModelList};

const MODEL_CREATED: u64 = 1_090_108_800;
const MODEL_OWNED_BY: &str = "deepseek-web (proxied by https://github.com/NIyueeE)";

/// 根据 model_types + aliases 生成模型列表
pub fn list(
    model_types: &[String],
    max_input_tokens: &[u32],
    max_output_tokens: &[u32],
    aliases: &HashMap<String, String>,
) -> OpenAIModelList {
    let mut data: Vec<OpenAIModel> = model_types
        .iter()
        .enumerate()
        .map(|(idx, ty)| {
            let input = max_input_tokens.get(idx).copied();
            let output = max_output_tokens.get(idx).copied();
            make_model(&format!("deepseek-{}", ty), input, output)
        })
        .collect();

    // 添加别名模型
    for (alias, ty) in aliases {
        if let Some(idx) = model_types.iter().position(|t| t == ty) {
            let input = max_input_tokens.get(idx).copied();
            let output = max_output_tokens.get(idx).copied();
            data.push(make_model(alias, input, output));
        }
    }

    OpenAIModelList {
        object: "list",
        data,
    }
}

/// 查询单个模型
pub fn get(
    model_types: &[String],
    max_input_tokens: &[u32],
    max_output_tokens: &[u32],
    aliases: &HashMap<String, String>,
    id: &str,
) -> Option<OpenAIModel> {
    let target = id.to_lowercase();

    // 先查 model_types
    if let Some((idx, ty)) = model_types
        .iter()
        .enumerate()
        .find(|(_, ty)| format!("deepseek-{}", ty).to_lowercase() == target)
    {
        let input = max_input_tokens.get(idx).copied();
        let output = max_output_tokens.get(idx).copied();
        return Some(make_model(&format!("deepseek-{}", ty), input, output));
    }

    // 再查 aliases
    if let Some(ty) = aliases.get(&target) {
        if let Some(idx) = model_types.iter().position(|t| t == ty) {
            let input = max_input_tokens.get(idx).copied();
            let output = max_output_tokens.get(idx).copied();
            return Some(make_model(&target, input, output));
        }
    }

    None
}

fn make_model(id: &str, input: Option<u32>, output: Option<u32>) -> OpenAIModel {
    OpenAIModel {
        id: id.to_string(),
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
}
