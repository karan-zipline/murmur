use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use super::chat;
use super::core::{help_items, ConnectionState, Focus, Mode, Model, PlannerFlow};

pub fn draw(frame: &mut Frame<'_>, model: &Model) {
    frame.render_widget(Clear, frame.size());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.size());

    draw_header(frame, model, chunks[0]);
    draw_main(frame, model, chunks[1]);
    draw_footer(frame, model, chunks[2]);
}

fn draw_header(frame: &mut Frame<'_>, model: &Model, area: ratatui::layout::Rect) {
    let conn = match model.connection {
        ConnectionState::Connected => {
            Span::styled("● connected", Style::default().fg(Color::Green))
        }
        ConnectionState::Connecting => {
            Span::styled("◌ connecting", Style::default().fg(Color::Yellow))
        }
        ConnectionState::Disconnected => {
            Span::styled("● disconnected", Style::default().fg(Color::Red))
        }
    };

    let running = model
        .agents
        .iter()
        .filter(|a| matches!(a.state, murmur_protocol::AgentState::Running))
        .count();
    let total = model.agents.len();

    let line = Line::from(vec![
        Span::styled("murmur", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        conn,
        Span::raw("  "),
        Span::styled(
            format!("agents: {running}/{total}"),
            Style::default().fg(Color::Gray),
        ),
        Span::raw("  "),
        Span::styled(
            format!("commits: {}", model.commit_count),
            Style::default().fg(Color::Gray),
        ),
        Span::raw("  "),
        Span::styled(
            format!("perms: {}", model.pending_permissions.len()),
            Style::default().fg(Color::Gray),
        ),
        Span::raw("  "),
        Span::styled(
            format!("questions: {}", model.pending_questions.len()),
            Style::default().fg(Color::Gray),
        ),
    ]);

    frame.render_widget(Paragraph::new(line), area);
}

fn draw_main(frame: &mut Frame<'_>, model: &Model, area: ratatui::layout::Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    let left_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(columns[0]);

    draw_agents(frame, model, left_rows[0]);
    draw_recent(frame, model, left_rows[1]);
    draw_chat(frame, model, columns[1]);
}

fn draw_agents(frame: &mut Frame<'_>, model: &Model, area: ratatui::layout::Rect) {
    let mut block = Block::default().title("Agents").borders(Borders::ALL);
    if matches!(model.focus, Focus::AgentList) && matches!(model.mode, Mode::Normal) {
        block = block.border_style(Style::default().fg(Color::Cyan));
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    if model.agents.is_empty() {
        lines.push(Line::from(Span::styled(
            "No agents",
            Style::default().fg(Color::Gray),
        )));
    } else {
        for (idx, agent) in model.agents.iter().enumerate() {
            let selected = idx == model.selected_agent;
            let prefix = if selected { "▶ " } else { "  " };
            let mut line = vec![Span::raw(prefix)];
            line.push(Span::styled(
                agent_state_icon(agent, model.now_ms),
                state_style(agent.state),
            ));
            line.push(Span::raw(" "));

            let (role_label, role_style) = match agent.role {
                murmur_protocol::AgentRole::Manager => (
                    "mgr",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                murmur_protocol::AgentRole::Planner => (
                    "pln",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                murmur_protocol::AgentRole::Coding => ("cod", Style::default().fg(Color::Gray)),
            };
            line.push(Span::styled(format!("{role_label:<3}"), role_style));
            line.push(Span::raw(" "));

            let id_style = match agent.role {
                murmur_protocol::AgentRole::Manager => Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
                murmur_protocol::AgentRole::Planner => Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                murmur_protocol::AgentRole::Coding => Style::default().add_modifier(Modifier::BOLD),
            };
            line.push(Span::styled(format!("{:<6}", agent.id), id_style));

            let has_perm = model
                .pending_permissions
                .iter()
                .any(|p| p.agent_id == agent.id);
            let has_question = model
                .pending_questions
                .iter()
                .any(|q| q.agent_id == agent.id);
            if has_perm {
                line.push(Span::raw(" "));
                line.push(Span::styled(
                    "P",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ));
            } else if has_question {
                line.push(Span::raw(" "));
                line.push(Span::styled(
                    "Q",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }

            line.push(Span::raw(" "));
            line.push(Span::styled(
                format!("{:<7}", agent.backend.as_deref().unwrap_or("-")),
                Style::default().fg(Color::Gray),
            ));
            line.push(Span::raw(" "));
            line.push(Span::styled(
                duration_label(model.now_ms, agent.created_at_ms),
                Style::default().fg(Color::Gray),
            ));
            line.push(Span::raw(" "));
            line.push(Span::styled(
                &agent.project,
                Style::default().fg(Color::Gray),
            ));
            line.push(Span::raw(" "));
            line.push(Span::raw(&agent.issue_id));

            let style = if selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            lines.push(Line::from(line).style(style));
        }
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, inner);
}

fn agent_state_icon(agent: &murmur_protocol::AgentInfo, now_ms: u64) -> &'static str {
    match agent.state {
        murmur_protocol::AgentState::Starting => "◌",
        murmur_protocol::AgentState::Running => spinner_frame(now_ms),
        murmur_protocol::AgentState::NeedsResolution => "!",
        murmur_protocol::AgentState::Exited => "✓",
        murmur_protocol::AgentState::Aborted => "×",
    }
}

fn spinner_frame(now_ms: u64) -> &'static str {
    const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let idx = ((now_ms / 120) as usize) % SPINNER.len();
    SPINNER[idx]
}

fn state_style(state: murmur_protocol::AgentState) -> Style {
    match state {
        murmur_protocol::AgentState::Starting => Style::default().fg(Color::Yellow),
        murmur_protocol::AgentState::Running => Style::default().fg(Color::Cyan),
        murmur_protocol::AgentState::NeedsResolution => Style::default().fg(Color::Red),
        murmur_protocol::AgentState::Exited => Style::default().fg(Color::Green),
        murmur_protocol::AgentState::Aborted => Style::default().fg(Color::Gray),
    }
}

fn duration_label(now_ms: u64, created_at_ms: u64) -> String {
    if now_ms == 0 || created_at_ms == 0 || now_ms < created_at_ms {
        return "--:--".to_owned();
    }
    format_duration_compact(now_ms - created_at_ms)
}

fn format_duration_compact(ms: u64) -> String {
    let secs = ms / 1000;
    let mins = secs / 60;
    let hours = mins / 60;
    let secs = secs % 60;
    let mins = mins % 60;

    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

fn draw_recent(frame: &mut Frame<'_>, model: &Model, area: ratatui::layout::Rect) {
    let block = Block::default().title("Recent").borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if model.recent_commits.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No recent commits",
                Style::default().fg(Color::Gray),
            ))),
            inner,
        );
        return;
    }

    let max_width = inner.width as usize;
    let mut lines = Vec::new();
    for c in &model.recent_commits {
        let sha7: String = c.sha.chars().take(7).collect();
        let raw = format!("{} {sha7} {}", c.project, c.issue_id);
        let truncated = truncate_chars(&raw, max_width);
        lines.push(Line::from(Span::raw(truncated)));
    }

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn truncate_chars(input: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max {
            break;
        }
        out.push(ch);
    }
    out
}

fn draw_chat(frame: &mut Frame<'_>, model: &Model, area: ratatui::layout::Rect) {
    if matches!(model.mode, Mode::Input) {
        let input_height = input_panel_height(model, area.height);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(input_height)])
            .split(area);
        draw_chat_panel(frame, model, rows[0]);
        draw_input_panel(frame, model, rows[1]);
        return;
    }

    draw_chat_panel(frame, model, area);
}

fn input_panel_height(model: &Model, max_height: u16) -> u16 {
    let inner = (model.editor.visual_lines() as u16).clamp(1, 6);
    let desired = inner.saturating_add(2).max(3);
    let max_total = max_height.saturating_sub(3).max(3);
    desired.min(max_total)
}

fn draw_chat_panel(frame: &mut Frame<'_>, model: &Model, area: ratatui::layout::Rect) {
    let title = match model.selected_agent() {
        Some(agent) => format!("Chat {}", agent.id),
        None => "Chat".to_owned(),
    };

    let mut block = Block::default().title(title).borders(Borders::ALL);
    if matches!(model.focus, Focus::ChatView) && matches!(model.mode, Mode::Normal) {
        block = block.border_style(Style::default().fg(Color::Cyan));
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(agent) = model.selected_agent() else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Select an agent to view chat",
                Style::default().fg(Color::Gray),
            ))),
            inner,
        );
        return;
    };

    let Some(buf) = model.selected_chat() else {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("No messages yet for ", Style::default().fg(Color::Gray)),
                Span::styled(&agent.id, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled(
                    "(waiting for agent.chat)",
                    Style::default().fg(Color::DarkGray),
                ),
            ])),
            inner,
        );
        return;
    };

    if buf.lines.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No messages yet",
                Style::default().fg(Color::Gray),
            ))),
            inner,
        );
        return;
    }

    let height = inner.height as usize;
    let start = buf.scroll_top.min(buf.lines.len());
    let end = (start + height).min(buf.lines.len());

    let spacer = " ".repeat(chat::ROLE_BADGE_SPACER_WIDTH);
    let mut lines = Vec::new();
    for line in &buf.lines[start..end] {
        let mut spans = Vec::new();
        if line.show_badge {
            let (badge, style) = role_badge(line.role);
            spans.push(Span::styled(badge, style.add_modifier(Modifier::BOLD)));
            spans.push(Span::raw(" "));
        } else {
            spans.push(Span::raw(&spacer));
        }
        spans.extend(line.spans.clone());
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), inner);

    if matches!(model.mode, Mode::Normal) {
        if model.abort_confirm.is_some() {
            draw_abort_confirm_overlay(frame, model, inner);
        } else if matches!(model.planner_flow, Some(PlannerFlow::Picking { .. })) {
            draw_planner_picker_overlay(frame, model, inner);
        } else {
            draw_attention_overlay(frame, model, inner);
        }
    }
}

fn draw_input_panel(frame: &mut Frame<'_>, model: &Model, area: ratatui::layout::Rect) {
    let title = match model.input_context {
        super::core::InputContext::Chat => "Input",
        super::core::InputContext::QuestionOther => "Answer",
        super::core::InputContext::PlannerPrompt => "Planner Prompt",
    };

    let mut block = Block::default().title(title).borders(Borders::ALL);
    if matches!(model.focus, Focus::InputLine) {
        block = block.border_style(Style::default().fg(Color::Cyan));
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if model.editor.buffer.is_empty() {
        let placeholder = match model.input_context {
            super::core::InputContext::Chat => "Type a message…",
            super::core::InputContext::QuestionOther => "Type an answer…",
            super::core::InputContext::PlannerPrompt => "Describe what you want planned…",
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                placeholder,
                Style::default().fg(Color::DarkGray),
            ))),
            inner,
        );
        return;
    }

    let mut content = model.editor.buffer.clone();
    content.push('█');
    frame.render_widget(Paragraph::new(content).wrap(Wrap { trim: false }), inner);
}

fn role_badge(role: murmur_protocol::ChatRole) -> (&'static str, Style) {
    match role {
        murmur_protocol::ChatRole::User => ("[U]", Style::default().fg(Color::Yellow)),
        murmur_protocol::ChatRole::Assistant => ("[A]", Style::default().fg(Color::Cyan)),
        murmur_protocol::ChatRole::Tool => ("[T]", Style::default().fg(Color::Magenta)),
        murmur_protocol::ChatRole::System => ("[S]", Style::default().fg(Color::Gray)),
    }
}

fn draw_footer(frame: &mut Frame<'_>, model: &Model, area: ratatui::layout::Rect) {
    if let Some(status) = model.status.as_ref() {
        let line = Line::from(vec![
            Span::styled("Error: ", Style::default().fg(Color::Red)),
            Span::raw(status),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let items = help_items(model);
    let mut spans = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            item.key,
            Style::default().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(": "));
        spans.push(Span::styled(item.desc, Style::default().fg(Color::Gray)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttentionKind {
    Permission,
    Question,
}

fn active_attention(model: &Model) -> Option<AttentionKind> {
    let p = model.pending_permission_for_selected();
    let q = model.pending_question_for_selected();
    match (p, q) {
        (Some(p), Some(q)) => {
            let p_key = (p.requested_at_ms, p.id.as_str());
            let q_key = (q.requested_at_ms, q.id.as_str());
            if p_key <= q_key {
                Some(AttentionKind::Permission)
            } else {
                Some(AttentionKind::Question)
            }
        }
        (Some(_), None) => Some(AttentionKind::Permission),
        (None, Some(_)) => Some(AttentionKind::Question),
        (None, None) => None,
    }
}

fn draw_attention_overlay(frame: &mut Frame<'_>, model: &Model, area: Rect) {
    let Some(kind) = active_attention(model) else {
        return;
    };

    let overlay_height = area.height.min(8).max(area.height.min(3));
    let overlay = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(overlay_height),
        width: area.width,
        height: overlay_height,
    };

    frame.render_widget(Clear, overlay);

    match kind {
        AttentionKind::Permission => {
            let Some(req) = model.pending_permission_for_selected() else {
                return;
            };
            draw_permission_overlay(frame, overlay, req);
        }
        AttentionKind::Question => {
            let Some(req) = model.pending_question_for_selected() else {
                return;
            };
            draw_question_overlay(frame, model, overlay, req);
        }
    }
}

fn draw_abort_confirm_overlay(frame: &mut Frame<'_>, model: &Model, area: Rect) {
    let Some(confirm) = model.abort_confirm.as_ref() else {
        return;
    };

    let overlay_height = area.height.min(5).max(area.height.min(3));
    let overlay_width = area.width.min(60).max(area.width.min(24));
    let overlay = Rect {
        x: area.x + area.width.saturating_sub(overlay_width) / 2,
        y: area.y + area.height.saturating_sub(overlay_height) / 2,
        width: overlay_width,
        height: overlay_height,
    };

    frame.render_widget(Clear, overlay);

    let title = match confirm.role {
        murmur_protocol::AgentRole::Planner => format!("Stop planner {}", confirm.agent_id),
        _ => format!("Abort agent {}", confirm.agent_id),
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    let lines = vec![Line::from(vec![
        Span::styled("y", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" confirm   "),
        Span::styled("n", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" cancel"),
    ])];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_planner_picker_overlay(frame: &mut Frame<'_>, model: &Model, area: Rect) {
    let Some(PlannerFlow::Picking { selected }) = model.planner_flow.as_ref() else {
        return;
    };

    let options = model.projects.len().saturating_add(1);
    if options == 0 {
        return;
    }

    let selected = (*selected).min(options - 1);
    let overlay_width = area.width.min(60).max(area.width.min(30));
    let overlay_height = area.height.min(14).max(area.height.min(6));
    let overlay = Rect {
        x: area.x + area.width.saturating_sub(overlay_width) / 2,
        y: area.y + area.height.saturating_sub(overlay_height) / 2,
        width: overlay_width,
        height: overlay_height,
    };

    frame.render_widget(Clear, overlay);

    let block = Block::default()
        .title("Start planner")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    let mut lines = Vec::new();
    for idx in 0..options {
        let name = if idx == 0 {
            "(none)".to_owned()
        } else {
            model
                .projects
                .get(idx - 1)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "-".to_owned())
        };
        let prefix = if idx == selected { "▶ " } else { "  " };
        let style = if idx == selected {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(name, style),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Enter", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" select   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" cancel"),
    ]));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn draw_permission_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    req: &murmur_protocol::PermissionRequest,
) {
    let block = Block::default()
        .title(format!("Permission: {}", req.tool_name))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let summary = permission_summary(req);
    let lines = vec![
        Line::from(vec![
            Span::styled("Agent ", Style::default().fg(Color::Gray)),
            Span::styled(&req.agent_id, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled("Project ", Style::default().fg(Color::Gray)),
            Span::styled(&req.project, Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(Span::raw(summary)),
        Line::from(vec![
            Span::styled("y", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" allow   "),
            Span::styled("n", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" deny"),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn permission_summary(req: &murmur_protocol::PermissionRequest) -> String {
    if req.tool_name == "Bash" {
        if let Some(cmd) = req.tool_input.get("command").and_then(|v| v.as_str()) {
            return format!("Bash: {cmd}");
        }
    }
    if req.tool_name == "WriteFile" {
        if let Some(path) = req.tool_input.get("path").and_then(|v| v.as_str()) {
            return format!("WriteFile: {path}");
        }
    }
    if let Ok(compact) = serde_json::to_string(&req.tool_input) {
        return format!("{}: {compact}", req.tool_name);
    }
    req.tool_name.clone()
}

fn draw_question_overlay(
    frame: &mut Frame<'_>,
    model: &Model,
    area: Rect,
    req: &murmur_protocol::UserQuestion,
) {
    let block = Block::default()
        .title("Question")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if req.questions.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No questions provided",
                Style::default().fg(Color::Gray),
            ))),
            inner,
        );
        return;
    }

    let (q_idx, opt_idx, answered) = match model.question_draft.as_ref() {
        Some(d) if d.request_id == req.id => (d.question_index, d.option_index, d.answers.len()),
        _ => (0, 0, 0),
    };

    let q_idx = q_idx.min(req.questions.len() - 1);
    let item = &req.questions[q_idx];
    let other_index = item.options.len();
    let opt_idx = opt_idx.min(other_index);

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            format!("{} ({}/{})", item.question, q_idx + 1, req.questions.len()),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("answered: {answered}"),
            Style::default().fg(Color::Gray),
        ),
    ]));

    for (idx, opt) in item.options.iter().enumerate() {
        let selected = idx == opt_idx;
        let prefix = if selected { "▶ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(&opt.label, style),
            Span::raw(" "),
            Span::styled(&opt.description, Style::default().fg(Color::Gray)),
        ]));
    }

    let other_selected = opt_idx == other_index;
    let prefix = if other_selected { "▶ " } else { "  " };
    let style = if other_selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    lines.push(Line::from(vec![
        Span::raw(prefix),
        Span::styled("Other…", style),
    ]));

    lines.push(Line::from(vec![
        Span::styled("j/k", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" select   "),
        Span::styled("y", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" submit"),
    ]));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer_text(terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        let area = buf.area;
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf.get(x, y).symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn renders_empty_scaffold() {
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        let model = Model::new();
        terminal.draw(|f| draw(f, &model)).unwrap();

        let text = buffer_text(&mut terminal);
        assert!(text.contains("Agents"));
        assert!(text.contains("Chat"));
        assert!(text.contains("Recent"));
        assert!(text.contains("Tab"));
        assert!(text.contains("quit"));
    }

    #[test]
    fn renders_agents_with_state_backend_and_duration() {
        let backend = ratatui::backend::TestBackend::new(80, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        let mut model = Model::new();
        model.now_ms = 1_000_000;
        model.agents = vec![
            murmur_protocol::AgentInfo {
                id: "a-1".to_owned(),
                project: "demo".to_owned(),
                role: murmur_protocol::AgentRole::Coding,
                issue_id: "ISSUE-1".to_owned(),
                state: murmur_protocol::AgentState::Running,
                created_at_ms: 1_000_000 - 65_000,
                backend: Some("codex".to_owned()),
                description: None,
                worktree_dir: "/tmp".to_owned(),
                pid: None,
                exit_code: None,
            },
            murmur_protocol::AgentInfo {
                id: "manager-demo".to_owned(),
                project: "demo".to_owned(),
                role: murmur_protocol::AgentRole::Manager,
                issue_id: "manager".to_owned(),
                state: murmur_protocol::AgentState::Running,
                created_at_ms: 1_000_000 - 1_000,
                backend: Some("claude".to_owned()),
                description: None,
                worktree_dir: "/tmp".to_owned(),
                pid: None,
                exit_code: None,
            },
            murmur_protocol::AgentInfo {
                id: "a-2".to_owned(),
                project: "demo".to_owned(),
                role: murmur_protocol::AgentRole::Coding,
                issue_id: "ISSUE-2".to_owned(),
                state: murmur_protocol::AgentState::Exited,
                created_at_ms: 0,
                backend: None,
                description: None,
                worktree_dir: "/tmp".to_owned(),
                pid: None,
                exit_code: Some(0),
            },
        ];
        model.selected_agent = 0;

        terminal.draw(|f| draw(f, &model)).unwrap();

        let text = buffer_text(&mut terminal);
        assert!(text.contains("a-1"));
        assert!(text.contains("mgr"));
        assert!(text.contains("codex"));
        assert!(text.contains("01:05"));
        assert!(text.contains(spinner_frame(model.now_ms)));
        assert!(text.contains("✓"));
        assert!(text.contains("--:--"));
    }

    #[test]
    fn renders_chat_with_role_badges_and_wrapping() {
        let backend = ratatui::backend::TestBackend::new(80, 14);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        let mut model = Model::new();
        model.agents = vec![murmur_protocol::AgentInfo {
            id: "a-1".to_owned(),
            project: "demo".to_owned(),
            role: murmur_protocol::AgentRole::Coding,
            issue_id: "ISSUE-1".to_owned(),
            state: murmur_protocol::AgentState::Running,
            created_at_ms: 1,
            backend: Some("codex".to_owned()),
            description: None,
            worktree_dir: "/tmp".to_owned(),
            pid: None,
            exit_code: None,
        }];
        model.selected_agent = 0;

        let mut buf = chat::ChatBuffer::new();
        chat::append_message(
            &mut buf,
            murmur_protocol::ChatMessage {
                role: murmur_protocol::ChatRole::Assistant,
                content: "hello world".to_owned(),
                tool_name: None,
                tool_input: None,
                tool_use_id: None,
                tool_result: None,
                is_error: false,
                ts_ms: 1,
            },
            10,
            8,
            false,
        );
        model.chats.insert("a-1".to_owned(), buf);

        terminal.draw(|f| draw(f, &model)).unwrap();

        let text = buffer_text(&mut terminal);
        assert!(text.contains("[A]"));
        assert!(text.contains("hello"));
        assert!(text.contains("world"));
    }

    #[test]
    fn input_panel_height_grows_with_multiline_buffer() {
        let backend = ratatui::backend::TestBackend::new(80, 14);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        let mut model = Model::new();
        model.mode = Mode::Input;
        model.focus = Focus::InputLine;

        model.editor.buffer = "hello".to_owned();
        terminal.draw(|f| draw(f, &model)).unwrap();
        let y1 = buffer_text(&mut terminal)
            .lines()
            .position(|l| l.contains("Input"))
            .unwrap();

        model.editor.buffer = "hello\nworld".to_owned();
        terminal.draw(|f| draw(f, &model)).unwrap();
        let y2 = buffer_text(&mut terminal)
            .lines()
            .position(|l| l.contains("Input"))
            .unwrap();

        assert!(y2 < y1);
    }

    #[test]
    fn renders_recent_commits_and_header_count() {
        let backend = ratatui::backend::TestBackend::new(60, 14);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        let mut model = Model::new();
        model.commit_count = 42;
        model.recent_commits = vec![murmur_protocol::CommitRecord {
            project: "demo".to_owned(),
            sha: "abcdef0123456789".to_owned(),
            branch: "main".to_owned(),
            agent_id: "a-1".to_owned(),
            issue_id: "ISSUE-1234567890".to_owned(),
            merged_at_ms: 1,
        }];

        terminal.draw(|f| draw(f, &model)).unwrap();

        let text = buffer_text(&mut terminal);
        assert!(text.contains("commits: 42"));
        assert!(text.contains("demo abcdef0"));
    }
}
