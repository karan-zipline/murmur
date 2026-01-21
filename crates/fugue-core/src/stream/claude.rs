use super::StreamMessage;

pub fn parse_stream_message_line(line: &str) -> Result<Option<StreamMessage>, serde_json::Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let msg: StreamMessage = serde_json::from_str(trimmed)?;
    Ok(Some(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_assistant_text_message() {
        let raw = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#;
        let msg = parse_stream_message_line(raw).unwrap().unwrap();
        assert_eq!(msg.r#type, "assistant");
        let nested = msg.message.unwrap();
        assert_eq!(nested.role, "assistant");
        assert_eq!(nested.content.len(), 1);
        assert_eq!(nested.content[0].r#type, "text");
        assert_eq!(nested.content[0].text, "hi");
    }
}
