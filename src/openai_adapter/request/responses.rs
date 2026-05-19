use super::super::types::*;

pub fn into_chat_completions(req: ResponseRequest) -> ChatCompletionsRequest {
    let messages = convert_input_to_messages(&req.input, req.instructions.as_deref());
    let tools = req.tools.map(convert_tools);

    ChatCompletionsRequest {
        model: req.model,
        messages,
        stream: req.stream,
        tools,
        tool_choice: req.tool_choice,
        max_tokens: req.max_output_tokens,
        temperature: req.temperature,
        top_p: req.top_p,
        stop: req.stop,
        reasoning_effort: req.reasoning.and_then(|r| r.effort),
        stream_options: req.stream_options,
        web_search_options: None,
        ..Default::default()
    }
}

fn convert_input_to_messages(input: &ResponseInput, instructions: Option<&str>) -> Vec<Message> {
    let mut messages = Vec::new();

    if let Some(instructions) = instructions {
        messages.push(Message {
            role: "system".to_string(),
            content: Some(MessageContent::Text(instructions.to_string())),
            ..Default::default()
        });
    }

    match input {
        ResponseInput::Text(text) => {
            messages.push(Message {
                role: "user".to_string(),
                content: Some(MessageContent::Text(text.clone())),
                ..Default::default()
            });
        }
        ResponseInput::Turns(turns) => {
            for turn in turns {
                let content = convert_content_items(&turn.content);
                let mut message = Message {
                    role: turn.role.clone(),
                    content: Some(content),
                    name: turn.name.clone(),
                    ..Default::default()
                };

                if turn.role == "tool" {
                    if let Some(result) = &turn.tool_result {
                        if let Some(content) = &result.content {
                            message.content = Some(MessageContent::Text(content.clone()));
                        } else if let Some(error) = &result.error {
                            message.content = Some(MessageContent::Text(format!("Error: {}", error)));
                        }
                    }
                }

                messages.push(message);
            }
        }
    }

    messages
}

fn convert_content_items(items: &[ContentItem]) -> MessageContent {
    if items.len() == 1 {
        match &items[0] {
            ContentItem::InputText { text } => MessageContent::Text(text.clone()),
            ContentItem::OutputText { text } => MessageContent::Text(text.clone()),
            ContentItem::InputImage { image_url } => MessageContent::Parts(vec![ContentPart {
                ty: "image_url".to_string(),
                image_url: Some(ImageUrlContent {
                    url: image_url.clone(),
                    detail: None,
                }),
                ..Default::default()
            }]),
        }
    } else {
        let parts: Vec<ContentPart> = items
            .iter()
            .map(|item| match item {
                ContentItem::InputText { text } => ContentPart {
                    ty: "text".to_string(),
                    text: Some(text.clone()),
                    ..Default::default()
                },
                ContentItem::OutputText { text } => ContentPart {
                    ty: "text".to_string(),
                    text: Some(text.clone()),
                    ..Default::default()
                },
                ContentItem::InputImage { image_url } => ContentPart {
                    ty: "image_url".to_string(),
                    image_url: Some(ImageUrlContent {
                        url: image_url.clone(),
                        detail: None,
                    }),
                    ..Default::default()
                },
            })
            .collect();
        MessageContent::Parts(parts)
    }
}

fn convert_tools(tools: Vec<ResponseTool>) -> Vec<Tool> {
    tools
        .into_iter()
        .map(|t| Tool {
            ty: t.ty,
            function: t.function,
            custom: None,
        })
        .collect()
}
