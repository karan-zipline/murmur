use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::{ChatMessage, ChatRole};

pub mod claude;
pub mod codex;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamMessage {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<NestedMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedMessage {
    pub role: String,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub content: FlexContent,
    #[serde(default)]
    pub tool_use_id: String,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlexContent(pub String);

impl<'de> Deserialize<'de> for FlexContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(s) => Ok(FlexContent(s)),
            Value::Array(parts) => {
                #[derive(Deserialize)]
                struct Part {
                    #[serde(default)]
                    text: String,
                }

                let mut texts = Vec::new();
                for part in parts {
                    if let Ok(p) = serde_json::from_value::<Part>(part) {
                        if !p.text.is_empty() {
                            texts.push(p.text);
                        }
                    }
                }
                Ok(FlexContent(texts.join("\n")))
            }
            other => Ok(FlexContent(other.to_string())),
        }
    }
}

impl Serialize for FlexContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputMessage {
    #[serde(rename = "type")]
    pub r#type: String,
    pub message: MessageBody,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageBody {
    pub role: String,
    pub content: String,
}

impl StreamMessage {
    pub fn to_chat_messages(&self, now_ms: u64) -> Vec<ChatMessage> {
        let Some(message) = &self.message else {
            return match self.r#type.as_str() {
                "system" | "result" => self
                    .result
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|content| ChatMessage::new(ChatRole::System, content.to_owned(), now_ms))
                    .into_iter()
                    .collect(),
                _ => Vec::new(),
            };
        };

        let role = match message.role.as_str() {
            "assistant" => ChatRole::Assistant,
            "user" => ChatRole::User,
            "system" => ChatRole::System,
            _ => ChatRole::Assistant,
        };

        let mut out = Vec::new();
        for block in &message.content {
            match block.r#type.as_str() {
                "text" => {
                    if !block.text.is_empty() {
                        out.push(ChatMessage::new(role, block.text.clone(), now_ms));
                    }
                }
                "tool_use" => {
                    let input = format_tool_input(&block.name, &block.input);
                    let tool_use_id = {
                        let s = block.id.trim();
                        if s.is_empty() {
                            None
                        } else {
                            Some(s.to_owned())
                        }
                    };
                    let msg = ChatMessage::tool_use(block.name.clone(), input, tool_use_id, now_ms);
                    if !msg.content.trim().is_empty() {
                        out.push(msg);
                    }
                }
                "tool_result" => {
                    if !block.content.0.is_empty() {
                        let tool_use_id = {
                            let s = block.tool_use_id.trim();
                            if s.is_empty() {
                                None
                            } else {
                                Some(s.to_owned())
                            }
                        };
                        out.push(ChatMessage::tool_result(
                            block.content.0.clone(),
                            tool_use_id,
                            block.is_error,
                            now_ms,
                        ));
                    }
                }
                _ => {}
            }
        }

        out
    }
}

pub fn format_tool_input(name: &str, input: &Value) -> String {
    let obj = match input.as_object() {
        Some(v) => v,
        None => return input.to_string(),
    };

    match name {
        "Bash" => obj
            .get("command")
            .and_then(|v| v.as_str())
            .map(|cmd| truncate(cmd, 100))
            .unwrap_or_else(|| format_generic_input(obj)),
        "Read" | "Write" | "Edit" => obj
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(|p| p.to_owned())
            .unwrap_or_else(|| format_generic_input(obj)),
        "Glob" => {
            let pattern = obj.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("");
            match (pattern.is_empty(), path.is_empty()) {
                (false, false) => format!("{pattern} in {path}"),
                (false, true) => pattern.to_owned(),
                _ => format_generic_input(obj),
            }
        }
        "Grep" => {
            let pattern = obj.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("");
            match (pattern.is_empty(), path.is_empty()) {
                (false, false) => format!("{pattern:?} in {path}"),
                (false, true) => format!("{pattern:?}"),
                _ => format_generic_input(obj),
            }
        }
        _ => format_generic_input(obj),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if max <= 3 {
        return s.chars().take(max).collect();
    }

    let take = max - 3;
    let mut it = s.chars();
    let prefix: String = it.by_ref().take(take).collect();
    if it.next().is_none() {
        return prefix;
    }
    format!("{prefix}...")
}

fn format_generic_input(obj: &serde_json::Map<String, Value>) -> String {
    let mut parts = Vec::new();
    for (k, v) in obj {
        let value = match v {
            Value::String(s) => truncate(s, 50),
            other => other.to_string(),
        };
        parts.push(format!("{k}={value}"));
    }
    parts.sort();
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flex_content_accepts_string() {
        let raw = r#""hello""#;
        let got: FlexContent = serde_json::from_str(raw).unwrap();
        assert_eq!(got, FlexContent("hello".to_owned()));
        assert_eq!(serde_json::to_string(&got).unwrap(), r#""hello""#);
    }

    #[test]
    fn flex_content_accepts_array_of_parts() {
        let raw = r#"[{"type":"text","text":"a"},{"type":"text","text":"b"}]"#;
        let got: FlexContent = serde_json::from_str(raw).unwrap();
        assert_eq!(got.0, "a\nb");
    }

    #[test]
    fn stream_message_to_chat_messages_preserves_order() {
        let msg = StreamMessage {
            r#type: "assistant".to_owned(),
            subtype: None,
            result: None,
            is_error: false,
            thread_id: None,
            message: Some(NestedMessage {
                role: "assistant".to_owned(),
                content: vec![
                    ContentBlock {
                        r#type: "text".to_owned(),
                        text: "one".to_owned(),
                        ..ContentBlock {
                            r#type: "text".to_owned(),
                            text: "one".to_owned(),
                            id: String::new(),
                            name: String::new(),
                            input: Value::Null,
                            content: FlexContent::default(),
                            tool_use_id: String::new(),
                            is_error: false,
                        }
                    },
                    ContentBlock {
                        r#type: "tool_use".to_owned(),
                        name: "Bash".to_owned(),
                        input: serde_json::json!({"command":"echo hi"}),
                        ..ContentBlock {
                            r#type: "tool_use".to_owned(),
                            text: String::new(),
                            id: String::new(),
                            name: "Bash".to_owned(),
                            input: serde_json::json!({"command":"echo hi"}),
                            content: FlexContent::default(),
                            tool_use_id: String::new(),
                            is_error: false,
                        }
                    },
                    ContentBlock {
                        r#type: "tool_result".to_owned(),
                        content: FlexContent("ok".to_owned()),
                        ..ContentBlock {
                            r#type: "tool_result".to_owned(),
                            text: String::new(),
                            id: String::new(),
                            name: String::new(),
                            input: Value::Null,
                            content: FlexContent("ok".to_owned()),
                            tool_use_id: String::new(),
                            is_error: false,
                        }
                    },
                ],
                model: None,
                stop_reason: None,
                usage: None,
            }),
        };

        let got = msg.to_chat_messages(123);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].role, ChatRole::Assistant);
        assert_eq!(got[0].content, "one");
        assert_eq!(got[1].role, ChatRole::Tool);
        assert!(got[1].content.contains("Bash"));
        assert_eq!(got[2].role, ChatRole::Tool);
        assert_eq!(got[2].content, "ok");
    }
}
