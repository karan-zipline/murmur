use serde::Deserialize;

use super::{ContentBlock, FlexContent, NestedMessage, StreamMessage, Usage};

#[derive(Debug, Deserialize)]
struct CodexEvent {
    #[serde(rename = "type")]
    r#type: String,
    #[serde(default)]
    thread_id: String,
    #[serde(default)]
    item: Option<CodexItem>,
    #[serde(default)]
    usage: Option<CodexUsage>,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct CodexItem {
    id: String,
    #[serde(rename = "type")]
    r#type: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    aggregated_output: String,
    #[serde(default)]
    exit_code: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct CodexUsage {
    input_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    output_tokens: u64,
}

pub fn parse_stream_message_line(line: &str) -> Result<Option<StreamMessage>, serde_json::Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let evt: CodexEvent = serde_json::from_str(trimmed)?;
    Ok(convert_event(evt))
}

fn convert_event(evt: CodexEvent) -> Option<StreamMessage> {
    match evt.r#type.as_str() {
        "thread.started" => Some(StreamMessage {
            r#type: "system".to_owned(),
            subtype: Some("init".to_owned()),
            message: None,
            result: None,
            is_error: false,
            thread_id: Some(evt.thread_id),
        }),
        "turn.started" => None,
        "turn.completed" => evt.usage.map(|u| StreamMessage {
            r#type: "assistant".to_owned(),
            subtype: None,
            message: Some(NestedMessage {
                role: "assistant".to_owned(),
                content: Vec::new(),
                model: None,
                stop_reason: None,
                usage: Some(Usage {
                    input_tokens: u.input_tokens,
                    output_tokens: u.output_tokens,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: Some(u.cached_input_tokens),
                }),
            }),
            result: None,
            is_error: false,
            thread_id: None,
        }),
        "item.started" => match evt.item {
            Some(item) if item.r#type == "command_execution" => {
                let input = serde_json::json!({ "command": item.command });
                Some(StreamMessage {
                    r#type: "assistant".to_owned(),
                    subtype: None,
                    message: Some(NestedMessage {
                        role: "assistant".to_owned(),
                        content: vec![ContentBlock {
                            r#type: "tool_use".to_owned(),
                            id: item.id,
                            name: "Bash".to_owned(),
                            input,
                            ..ContentBlock {
                                r#type: "tool_use".to_owned(),
                                text: String::new(),
                                id: String::new(),
                                name: String::new(),
                                input: serde_json::Value::Null,
                                content: FlexContent::default(),
                                tool_use_id: String::new(),
                                is_error: false,
                            }
                        }],
                        model: None,
                        stop_reason: None,
                        usage: None,
                    }),
                    result: None,
                    is_error: false,
                    thread_id: None,
                })
            }
            _ => None,
        },
        "item.completed" => {
            let item = evt.item?;
            match item.r#type.as_str() {
                "reasoning" => Some(StreamMessage {
                    r#type: "assistant".to_owned(),
                    subtype: None,
                    message: Some(NestedMessage {
                        role: "assistant".to_owned(),
                        content: vec![ContentBlock {
                            r#type: "text".to_owned(),
                            text: item.text,
                            ..ContentBlock {
                                r#type: "text".to_owned(),
                                text: String::new(),
                                id: String::new(),
                                name: String::new(),
                                input: serde_json::Value::Null,
                                content: FlexContent::default(),
                                tool_use_id: String::new(),
                                is_error: false,
                            }
                        }],
                        model: None,
                        stop_reason: None,
                        usage: None,
                    }),
                    result: None,
                    is_error: false,
                    thread_id: None,
                }),
                "command_execution" => {
                    let is_error = item.exit_code.unwrap_or(0) != 0;
                    Some(StreamMessage {
                        r#type: "user".to_owned(),
                        subtype: None,
                        message: Some(NestedMessage {
                            role: "user".to_owned(),
                            content: vec![ContentBlock {
                                r#type: "tool_result".to_owned(),
                                tool_use_id: item.id,
                                content: FlexContent(item.aggregated_output),
                                is_error,
                                ..ContentBlock {
                                    r#type: "tool_result".to_owned(),
                                    text: String::new(),
                                    id: String::new(),
                                    name: String::new(),
                                    input: serde_json::Value::Null,
                                    content: FlexContent::default(),
                                    tool_use_id: String::new(),
                                    is_error: false,
                                }
                            }],
                            model: None,
                            stop_reason: None,
                            usage: None,
                        }),
                        result: None,
                        is_error: false,
                        thread_id: None,
                    })
                }
                "agent_message" => Some(StreamMessage {
                    r#type: "assistant".to_owned(),
                    subtype: None,
                    message: Some(NestedMessage {
                        role: "assistant".to_owned(),
                        content: vec![ContentBlock {
                            r#type: "text".to_owned(),
                            text: item.text,
                            ..ContentBlock {
                                r#type: "text".to_owned(),
                                text: String::new(),
                                id: String::new(),
                                name: String::new(),
                                input: serde_json::Value::Null,
                                content: FlexContent::default(),
                                tool_use_id: String::new(),
                                is_error: false,
                            }
                        }],
                        model: None,
                        stop_reason: None,
                        usage: None,
                    }),
                    result: None,
                    is_error: false,
                    thread_id: None,
                }),
                _ => None,
            }
        }
        "error" => Some(StreamMessage {
            r#type: "result".to_owned(),
            subtype: None,
            message: None,
            result: Some(evt.message),
            is_error: true,
            thread_id: None,
        }),
        "warning" => Some(StreamMessage {
            r#type: "system".to_owned(),
            subtype: Some("warning".to_owned()),
            message: None,
            result: Some(evt.message),
            is_error: false,
            thread_id: None,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_thread_started() {
        let raw = r#"{"type":"thread.started","thread_id":"t-123"}"#;
        let msg = parse_stream_message_line(raw).unwrap().unwrap();
        assert_eq!(msg.r#type, "system");
        assert_eq!(msg.subtype.as_deref(), Some("init"));
        assert_eq!(msg.thread_id.as_deref(), Some("t-123"));
    }

    #[test]
    fn parses_agent_message() {
        let raw = r#"{"type":"item.completed","item":{"id":"i-1","type":"agent_message","text":"hello"}} "#;
        let msg = parse_stream_message_line(raw).unwrap().unwrap();
        assert_eq!(msg.r#type, "assistant");
        let nested = msg.message.unwrap();
        assert_eq!(nested.role, "assistant");
        assert_eq!(nested.content.len(), 1);
        assert_eq!(nested.content[0].text, "hello");
    }
}
