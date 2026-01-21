use fugue_protocol::{ChatMessage, ChatRole};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

pub const ROLE_BADGE_WIDTH: usize = 3;
pub const ROLE_BADGE_SPACER_WIDTH: usize = ROLE_BADGE_WIDTH + 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedChatLine {
    pub role: ChatRole,
    pub spans: Vec<Span<'static>>,
    pub show_badge: bool,
}

#[derive(Debug, Clone)]
pub struct ChatBuffer {
    pub messages: Vec<ChatMessage>,
    pub lines: Vec<RenderedChatLine>,
    pub scroll_top: usize,
    pub follow_tail: bool,
    pub history_loaded: bool,
}

impl ChatBuffer {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            lines: Vec::new(),
            scroll_top: 0,
            follow_tail: true,
            history_loaded: false,
        }
    }
}

pub fn merge_history(history: &[ChatMessage], existing: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut all = Vec::with_capacity(history.len() + existing.len());
    all.extend_from_slice(history);
    all.extend_from_slice(existing);
    all.sort_by(|a, b| a.ts_ms.cmp(&b.ts_ms));

    let mut out = Vec::with_capacity(all.len());
    for msg in all {
        let is_dup = out
            .iter()
            .rev()
            .take(8)
            .any(|m| is_duplicate_message(m, &msg));
        if !is_dup {
            out.push(msg);
        }
    }
    out
}

fn is_duplicate_message(existing: &ChatMessage, incoming: &ChatMessage) -> bool {
    if existing.role != incoming.role {
        return false;
    }

    if existing.role == ChatRole::Tool
        && (existing.tool_use_id.is_some() || incoming.tool_use_id.is_some())
    {
        return existing.tool_use_id == incoming.tool_use_id
            && existing.tool_name == incoming.tool_name
            && existing.tool_result == incoming.tool_result
            && existing.tool_input == incoming.tool_input
            && existing.is_error == incoming.is_error;
    }

    if existing.content != incoming.content {
        return false;
    }

    if existing.ts_ms == incoming.ts_ms {
        return true;
    }

    matches!(incoming.role, ChatRole::User) && existing.ts_ms.abs_diff(incoming.ts_ms) <= 2_000
}

pub fn rebuild_lines(
    messages: &[ChatMessage],
    width: usize,
    show_tool_events: bool,
) -> Vec<RenderedChatLine> {
    let content_width = width.saturating_sub(ROLE_BADGE_SPACER_WIDTH);
    let mut out = Vec::new();

    let mut last_tool_name = String::new();
    let mut last_visible_role: Option<ChatRole> = None;
    for msg in messages {
        if msg.role == ChatRole::Tool {
            if let Some(name) = msg
                .tool_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                last_tool_name = name.to_owned();
            }
            if show_tool_events {
                last_visible_role = Some(ChatRole::Tool);
                out.extend(render_tool_message(msg, &last_tool_name, content_width));
            }
            continue;
        }

        if matches!(last_visible_role, Some(role) if role != ChatRole::Tool) {
            out.push(RenderedChatLine {
                role: ChatRole::System,
                spans: vec![Span::raw("")],
                show_badge: false,
            });
        }
        last_visible_role = Some(msg.role);
        out.extend(render_markdown_message(msg, content_width));
    }

    out
}

fn render_markdown_message(message: &ChatMessage, width: usize) -> Vec<RenderedChatLine> {
    let lines = super::markdown::render_markdown(&message.content, width);
    let mut out = Vec::new();
    for (idx, line) in lines.into_iter().enumerate() {
        out.push(RenderedChatLine {
            role: message.role,
            spans: line.spans,
            show_badge: idx == 0,
        });
    }
    if out.is_empty() {
        out.push(RenderedChatLine {
            role: message.role,
            spans: vec![Span::raw("")],
            show_badge: true,
        });
    }
    out
}

fn render_tool_message(
    message: &ChatMessage,
    last_tool_name: &str,
    width: usize,
) -> Vec<RenderedChatLine> {
    let mut parts: Vec<Line<'static>> = Vec::new();

    if let Some(tool_name) = message
        .tool_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let input = message.tool_input.as_deref().unwrap_or("");
        parts.push(Line::from(render_tool_invocation(tool_name, input)));
    }

    let result = message.tool_result.as_deref().unwrap_or("");
    if !result.trim().is_empty() {
        let tool_name = message
            .tool_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(last_tool_name);
        parts.extend(render_tool_result(
            tool_name,
            result,
            width.saturating_sub(6),
            message.is_error,
        ));
    }

    let mut out = Vec::new();
    for (idx, line) in parts.into_iter().enumerate() {
        out.push(RenderedChatLine {
            role: ChatRole::Tool,
            spans: line.spans,
            show_badge: idx == 0,
        });
    }
    out
}

fn tool_tag_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn tool_result_style() -> Style {
    Style::default().fg(Color::Gray)
}

fn render_tool_invocation(tool_name: &str, tool_input: &str) -> Vec<Span<'static>> {
    let input = truncate_tool_input(tool_input);
    vec![
        Span::raw("  "),
        Span::styled(format!("[{tool_name}]"), tool_tag_style()),
        Span::raw(" "),
        Span::raw(input),
    ]
}

fn render_tool_result(
    tool_name: &str,
    tool_result: &str,
    max_width: usize,
    is_error: bool,
) -> Vec<Line<'static>> {
    let summary = summarize_tool_result(tool_name, tool_result, max_width, is_error);
    summary
        .split('\n')
        .enumerate()
        .map(|(idx, line)| {
            if idx == 0 {
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled("->", tool_result_style()),
                    Span::raw(" "),
                    Span::raw(line.to_owned()),
                ])
            } else {
                Line::from(Span::raw(line.to_owned()))
            }
        })
        .collect()
}

fn truncate_tool_input(input: &str) -> String {
    let mut input = input.replace('\n', " ");
    input = input.trim().to_owned();
    const MAX_LEN: usize = 80;
    if input.chars().count() > MAX_LEN {
        let (take, _) = super::markdown::split_at_char_boundary(&input, MAX_LEN - 3);
        return format!("{take}...");
    }
    input
}

fn truncate_result(result: &str, max_width: usize) -> String {
    let mut lines: Vec<&str> = result.split('\n').collect();
    const MAX_LINES: usize = 5;
    if lines.len() > MAX_LINES {
        lines.truncate(MAX_LINES);
        lines.push("...");
    }

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for line in lines {
        let trimmed = if line.chars().count() > max_width && max_width >= 3 {
            let (take, _) = super::markdown::split_at_char_boundary(line, max_width - 3);
            format!("{take}...")
        } else {
            line.to_owned()
        };
        out.push(trimmed);
    }

    join_with_indent(out)
}

fn summarize_tool_result(
    tool_name: &str,
    result: &str,
    max_width: usize,
    is_error: bool,
) -> String {
    if is_error {
        return format_full_result(result, max_width);
    }

    match tool_name {
        "Read" => {
            let mut line_count = result.chars().filter(|c| *c == '\n').count();
            if !result.ends_with('\n') && !result.is_empty() {
                line_count += 1;
            }
            format!("Read {}", format_line_count(line_count))
        }
        "Grep" => {
            let match_count = result
                .split('\n')
                .filter(|line| !line.trim().is_empty())
                .count();
            if match_count == 0 {
                "No matches".to_owned()
            } else {
                format_match_count(match_count)
            }
        }
        _ => truncate_result(result, max_width),
    }
}

fn format_full_result(result: &str, max_width: usize) -> String {
    let lines: Vec<String> = result
        .split('\n')
        .map(|line| {
            if line.chars().count() > max_width && max_width >= 3 {
                let (take, _) = super::markdown::split_at_char_boundary(line, max_width - 3);
                format!("{take}...")
            } else {
                line.to_owned()
            }
        })
        .collect();
    join_with_indent(lines)
}

fn join_with_indent(lines: Vec<String>) -> String {
    let mut parts = Vec::with_capacity(lines.len());
    for (idx, line) in lines.into_iter().enumerate() {
        if idx == 0 {
            parts.push(line);
        } else {
            parts.push(format!("     {line}"));
        }
    }
    parts.join("\n")
}

fn format_line_count(count: usize) -> String {
    if count == 1 {
        "1 line".to_owned()
    } else {
        format!("{count} lines")
    }
}

fn format_match_count(count: usize) -> String {
    if count == 1 {
        "1 match".to_owned()
    } else {
        format!("{count} matches")
    }
}

pub fn append_message(
    buffer: &mut ChatBuffer,
    message: ChatMessage,
    width: usize,
    viewport_height: usize,
    show_tool_events: bool,
) {
    if buffer
        .messages
        .iter()
        .rev()
        .take(8)
        .any(|m| is_duplicate_message(m, &message))
    {
        return;
    }
    buffer.messages.push(message.clone());
    buffer.lines = rebuild_lines(&buffer.messages, width, show_tool_events);
    clamp_scroll_after_content_change(buffer, viewport_height);
}

pub fn rewrap(
    buffer: &mut ChatBuffer,
    width: usize,
    viewport_height: usize,
    show_tool_events: bool,
) {
    buffer.lines = rebuild_lines(&buffer.messages, width, show_tool_events);
    clamp_scroll_after_content_change(buffer, viewport_height);
}

pub fn scroll_up(buffer: &mut ChatBuffer, lines: usize) {
    buffer.follow_tail = false;
    buffer.scroll_top = buffer.scroll_top.saturating_sub(lines);
}

pub fn scroll_down(buffer: &mut ChatBuffer, viewport_height: usize, lines: usize) {
    let max = max_scroll_top(buffer.lines.len(), viewport_height);
    let next = (buffer.scroll_top + lines).min(max);
    if max.saturating_sub(next) <= 2 {
        buffer.scroll_top = max;
        buffer.follow_tail = true;
    } else {
        buffer.scroll_top = next;
        buffer.follow_tail = false;
    }
}

pub fn jump_top(buffer: &mut ChatBuffer) {
    buffer.follow_tail = false;
    buffer.scroll_top = 0;
}

pub fn jump_bottom(buffer: &mut ChatBuffer, viewport_height: usize) {
    buffer.scroll_top = max_scroll_top(buffer.lines.len(), viewport_height);
    buffer.follow_tail = true;
}

fn clamp_scroll_after_content_change(buffer: &mut ChatBuffer, viewport_height: usize) {
    let max = max_scroll_top(buffer.lines.len(), viewport_height);
    if buffer.follow_tail {
        buffer.scroll_top = max;
    } else {
        buffer.scroll_top = buffer.scroll_top.min(max);
        if buffer.scroll_top == max {
            buffer.follow_tail = true;
        }
    }
}

fn max_scroll_top(total_lines: usize, viewport_height: usize) -> usize {
    total_lines.saturating_sub(viewport_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_chat_message_and_marks_first_line() {
        let msg = ChatMessage {
            role: ChatRole::Assistant,
            content: "hello world".to_owned(),
            tool_name: None,
            tool_input: None,
            tool_use_id: None,
            tool_result: None,
            is_error: false,
            ts_ms: 1,
        };

        let lines = rebuild_lines(&[msg], 10, false);
        assert!(lines.len() >= 2);
        assert!(lines[0].show_badge);
        assert!(!lines[1].show_badge);
        let rendered: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("hello"));
        assert!(rendered.contains("world"));
    }

    #[test]
    fn scroll_follow_tail_sticks_when_appending() {
        let mut buf = ChatBuffer::new();
        let viewport_height = 2;
        let width = 20;

        append_message(
            &mut buf,
            ChatMessage {
                role: ChatRole::User,
                content: "one".to_owned(),
                tool_name: None,
                tool_input: None,
                tool_use_id: None,
                tool_result: None,
                is_error: false,
                ts_ms: 1,
            },
            width,
            viewport_height,
            false,
        );
        append_message(
            &mut buf,
            ChatMessage {
                role: ChatRole::User,
                content: "two".to_owned(),
                tool_name: None,
                tool_input: None,
                tool_use_id: None,
                tool_result: None,
                is_error: false,
                ts_ms: 2,
            },
            width,
            viewport_height,
            false,
        );
        append_message(
            &mut buf,
            ChatMessage {
                role: ChatRole::User,
                content: "three".to_owned(),
                tool_name: None,
                tool_input: None,
                tool_use_id: None,
                tool_result: None,
                is_error: false,
                ts_ms: 3,
            },
            width,
            viewport_height,
            false,
        );

        assert!(buf.follow_tail);
        let max = max_scroll_top(buf.lines.len(), viewport_height);
        assert_eq!(buf.scroll_top, max);

        scroll_up(&mut buf, 1);
        assert!(!buf.follow_tail);
        assert_eq!(buf.scroll_top, max.saturating_sub(1));

        append_message(
            &mut buf,
            ChatMessage {
                role: ChatRole::User,
                content: "four".to_owned(),
                tool_name: None,
                tool_input: None,
                tool_use_id: None,
                tool_result: None,
                is_error: false,
                ts_ms: 4,
            },
            width,
            viewport_height,
            false,
        );
        assert!(!buf.follow_tail);
        assert_eq!(buf.scroll_top, max.saturating_sub(1));

        scroll_down(&mut buf, viewport_height, 100);
        assert!(buf.follow_tail);
        assert_eq!(
            buf.scroll_top,
            max_scroll_top(buf.lines.len(), viewport_height)
        );
    }

    #[test]
    fn merge_history_dedupes_and_keeps_sorted() {
        let history = vec![
            ChatMessage {
                role: ChatRole::User,
                content: "one".to_owned(),
                tool_name: None,
                tool_input: None,
                tool_use_id: None,
                tool_result: None,
                is_error: false,
                ts_ms: 1,
            },
            ChatMessage {
                role: ChatRole::User,
                content: "two".to_owned(),
                tool_name: None,
                tool_input: None,
                tool_use_id: None,
                tool_result: None,
                is_error: false,
                ts_ms: 2,
            },
        ];

        let existing = vec![
            ChatMessage {
                role: ChatRole::User,
                content: "two".to_owned(),
                tool_name: None,
                tool_input: None,
                tool_use_id: None,
                tool_result: None,
                is_error: false,
                ts_ms: 2,
            },
            ChatMessage {
                role: ChatRole::Assistant,
                content: "three".to_owned(),
                tool_name: None,
                tool_input: None,
                tool_use_id: None,
                tool_result: None,
                is_error: false,
                ts_ms: 3,
            },
        ];

        let merged = merge_history(&history, &existing);
        assert_eq!(
            merged,
            vec![
                ChatMessage {
                    role: ChatRole::User,
                    content: "one".to_owned(),
                    tool_name: None,
                    tool_input: None,
                    tool_use_id: None,
                    tool_result: None,
                    is_error: false,
                    ts_ms: 1
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: "two".to_owned(),
                    tool_name: None,
                    tool_input: None,
                    tool_use_id: None,
                    tool_result: None,
                    is_error: false,
                    ts_ms: 2
                },
                ChatMessage {
                    role: ChatRole::Assistant,
                    content: "three".to_owned(),
                    tool_name: None,
                    tool_input: None,
                    tool_use_id: None,
                    tool_result: None,
                    is_error: false,
                    ts_ms: 3
                },
            ]
        );
    }

    #[test]
    fn hides_tool_messages_by_default() {
        let msgs = vec![
            ChatMessage {
                role: ChatRole::Tool,
                content: String::new(),
                tool_name: Some("Read".to_owned()),
                tool_input: Some("README.md".to_owned()),
                tool_use_id: Some("t1".to_owned()),
                tool_result: Some("hello\nworld\n".to_owned()),
                is_error: false,
                ts_ms: 1,
            },
            ChatMessage {
                role: ChatRole::Assistant,
                content: "done".to_owned(),
                tool_name: None,
                tool_input: None,
                tool_use_id: None,
                tool_result: None,
                is_error: false,
                ts_ms: 2,
            },
        ];

        let hidden = rebuild_lines(&msgs, 60, false);
        assert_eq!(hidden.len(), 1);

        let shown = rebuild_lines(&msgs, 60, true);
        assert!(shown.len() >= 3);
        let rendered: String = shown
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("[Read]"));
        assert!(rendered.contains("Read 2 lines"));
    }
}
