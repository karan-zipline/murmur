use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InlineMode {
    Normal,
    Code,
    Bold,
    BoldCode,
}

pub fn render_markdown(message: &str, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return vec![Line::from("")];
    }

    let mut lines = Vec::new();
    let mut in_code_block = false;

    for raw in message.split('\n') {
        let trimmed = raw.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            for chunk in hard_wrap(raw, width) {
                lines.push(Line::from(vec![Span::styled(chunk, code_block_style())]));
            }
            continue;
        }

        if let Some(heading) = parse_heading(trimmed) {
            let wrapped = wrap_tokens(&[(heading.to_owned(), heading_style())], width);
            for spans in wrapped {
                lines.push(Line::from(spans));
            }
            lines.push(Line::from(""));
            continue;
        }

        let tokens = tokenize_inline(raw);
        for wrapped in wrap_tokens(&tokens, width) {
            lines.push(Line::from(wrapped));
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(""));
    }
    lines
}

fn code_block_style() -> Style {
    Style::default()
        .fg(Color::LightGreen)
        .bg(Color::Rgb(30, 30, 30))
}

fn inline_code_style() -> Style {
    Style::default().fg(Color::White).bg(Color::Rgb(50, 50, 50))
}

fn bold_style() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

fn heading_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn parse_heading(line: &str) -> Option<&str> {
    let mut chars = line.chars();
    let mut hashes = 0usize;
    while let Some('#') = chars.next() {
        hashes += 1;
        if hashes >= 6 {
            break;
        }
    }
    if hashes == 0 {
        return None;
    }
    let rest = &line[hashes..];
    let rest = rest.strip_prefix(' ')?;
    Some(rest.trim())
}

fn tokenize_inline(input: &str) -> Vec<(String, Style)> {
    let mut out: Vec<(String, Style)> = Vec::new();
    let mut buf = String::new();
    let mut mode = InlineMode::Normal;
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    let flush = |buf: &mut String, mode: InlineMode, out: &mut Vec<(String, Style)>| {
        if buf.is_empty() {
            return;
        }
        let style = match mode {
            InlineMode::Normal => Style::default(),
            InlineMode::Code => inline_code_style(),
            InlineMode::Bold => bold_style(),
            InlineMode::BoldCode => inline_code_style().add_modifier(Modifier::BOLD),
        };
        out.push((std::mem::take(buf), style));
    };

    while i < chars.len() {
        if chars[i] == '`' {
            flush(&mut buf, mode, &mut out);
            mode = match mode {
                InlineMode::Normal => InlineMode::Code,
                InlineMode::Code => InlineMode::Normal,
                InlineMode::Bold => InlineMode::BoldCode,
                InlineMode::BoldCode => InlineMode::Bold,
            };
            i += 1;
            continue;
        }

        if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            flush(&mut buf, mode, &mut out);
            mode = match mode {
                InlineMode::Normal => InlineMode::Bold,
                InlineMode::Bold => InlineMode::Normal,
                InlineMode::Code => InlineMode::BoldCode,
                InlineMode::BoldCode => InlineMode::Code,
            };
            i += 2;
            continue;
        }

        buf.push(chars[i]);
        i += 1;
    }

    flush(&mut buf, mode, &mut out);
    out
}

fn wrap_tokens(tokens: &[(String, Style)], width: usize) -> Vec<Vec<Span<'static>>> {
    if tokens.is_empty() {
        return vec![vec![Span::raw("")]];
    }

    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut current_len = 0usize;

    let mut pieces: Vec<(String, Style, bool)> = Vec::new();
    for (text, style) in tokens {
        let mut buf = String::new();
        let mut in_space = None::<bool>;
        for ch in text.chars() {
            let is_space = ch.is_whitespace();
            match in_space {
                None => {
                    buf.push(ch);
                    in_space = Some(is_space);
                }
                Some(prev) if prev == is_space => buf.push(ch),
                Some(prev) => {
                    pieces.push((std::mem::take(&mut buf), *style, prev));
                    buf.push(ch);
                    in_space = Some(is_space);
                }
            }
        }
        if let Some(is_space) = in_space {
            pieces.push((buf, *style, is_space));
        }
    }

    for (text, style, is_space) in pieces {
        if text.is_empty() {
            continue;
        }

        if is_space {
            if current_len == 0 {
                continue;
            }
            let len = text.chars().count();
            if current_len + len <= width {
                current.push(Span::styled(text, style));
                current_len += len;
            } else {
                lines.push(std::mem::take(&mut current));
                current_len = 0;
            }
            continue;
        }

        let mut remaining = text.as_str();
        loop {
            if remaining.is_empty() {
                break;
            }
            let available = width.saturating_sub(current_len);
            if available == 0 {
                lines.push(std::mem::take(&mut current));
                current_len = 0;
                continue;
            }

            let remaining_len = remaining.chars().count();
            if remaining_len <= available {
                current.push(Span::styled(remaining.to_owned(), style));
                current_len += remaining_len;
                break;
            }

            if remaining_len > width {
                if current_len > 0 {
                    lines.push(std::mem::take(&mut current));
                    current_len = 0;
                    continue;
                }
                let (take, rest) = split_at_char_boundary(remaining, width);
                lines.push(vec![Span::styled(take.to_owned(), style)]);
                remaining = rest;
                continue;
            }

            lines.push(std::mem::take(&mut current));
            current_len = 0;
        }
    }

    if current.is_empty() {
        current.push(Span::raw(""));
    }
    lines.push(current);
    lines
}

fn hard_wrap(input: &str, width: usize) -> Vec<String> {
    if input.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut remaining = input;
    while !remaining.is_empty() {
        let (take, rest) = split_at_char_boundary(remaining, width);
        out.push(take.to_owned());
        remaining = rest;
    }
    out
}

pub(crate) fn split_at_char_boundary(input: &str, max_chars: usize) -> (&str, &str) {
    if max_chars == 0 {
        return ("", input);
    }
    if input.chars().count() <= max_chars {
        return (input, "");
    }
    let end = input
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| input.len());
    input.split_at(end)
}
