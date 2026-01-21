use std::collections::{BTreeMap, HashMap};

use murmur_protocol::{AgentInfo, PermissionBehavior, PermissionRequest, UserQuestion};

use super::chat::{self, ChatBuffer};
use super::editor::Editor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    AgentList,
    ChatView,
    InputLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputContext {
    Chat,
    QuestionOther,
    PlannerPrompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone)]
pub struct QuestionDraft {
    pub request_id: String,
    pub agent_id: String,
    pub question_index: usize,
    pub option_index: usize,
    pub answers: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct AbortConfirm {
    pub agent_id: String,
    pub role: murmur_protocol::AgentRole,
}

#[derive(Debug, Clone)]
pub enum PlannerFlow {
    Picking { selected: usize },
    Prompt { project: Option<String> },
}

#[derive(Debug, Clone)]
enum PendingAttention {
    Permission,
    Question,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub width: u16,
    pub height: u16,
    pub now_ms: u64,
    pub next_refresh_ms: u64,
    pub reconnect_attempt: u32,
    pub next_reconnect_ms: u64,

    pub focus: Focus,
    pub mode: Mode,
    pub connection: ConnectionState,

    pub agents: Vec<AgentInfo>,
    pub selected_agent: usize,
    pub chats: HashMap<String, ChatBuffer>,
    pub editor: Editor,
    pub input_context: InputContext,
    pub commit_count: u32,
    pub recent_commits: Vec<murmur_protocol::CommitRecord>,
    pub projects: Vec<murmur_protocol::ProjectInfo>,
    pub planner_flow: Option<PlannerFlow>,
    pub abort_confirm: Option<AbortConfirm>,
    pub pending_permissions: Vec<PermissionRequest>,
    pub pending_questions: Vec<UserQuestion>,
    pub question_draft: Option<QuestionDraft>,

    pub show_tool_events: bool,
    pub status: Option<String>,
}

impl Model {
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            now_ms: 0,
            next_refresh_ms: 0,
            reconnect_attempt: 0,
            next_reconnect_ms: 0,
            focus: Focus::AgentList,
            mode: Mode::Normal,
            connection: ConnectionState::Connecting,
            agents: Vec::new(),
            selected_agent: 0,
            chats: HashMap::new(),
            editor: Editor::new(),
            input_context: InputContext::Chat,
            commit_count: 0,
            recent_commits: Vec::new(),
            projects: Vec::new(),
            planner_flow: None,
            abort_confirm: None,
            pending_permissions: Vec::new(),
            pending_questions: Vec::new(),
            question_draft: None,
            show_tool_events: false,
            status: None,
        }
    }

    pub fn selected_agent(&self) -> Option<&AgentInfo> {
        self.agents.get(self.selected_agent)
    }

    pub fn selected_chat(&self) -> Option<&ChatBuffer> {
        let agent = self.selected_agent()?;
        self.chats.get(&agent.id)
    }

    pub fn pending_permission_for_selected(&self) -> Option<&PermissionRequest> {
        let agent_id = self.selected_agent()?.id.as_str();
        self.pending_permissions
            .iter()
            .filter(|p| p.agent_id == agent_id)
            .min_by(|a, b| {
                a.requested_at_ms
                    .cmp(&b.requested_at_ms)
                    .then(a.id.cmp(&b.id))
            })
    }

    pub fn pending_question_for_selected(&self) -> Option<&UserQuestion> {
        let agent_id = self.selected_agent()?.id.as_str();
        self.pending_questions
            .iter()
            .filter(|q| q.agent_id == agent_id)
            .min_by(|a, b| {
                a.requested_at_ms
                    .cmp(&b.requested_at_ms)
                    .then(a.id.cmp(&b.id))
            })
    }

    #[cfg(test)]
    pub fn validate(&self) -> Result<(), String> {
        if matches!(self.mode, Mode::Input) && !matches!(self.focus, Focus::InputLine) {
            return Err("input mode requires InputLine focus".to_owned());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Tab,
    MoveUp,
    MoveDown,
    GoTop,
    GoBottom,
    PageUp,
    PageDown,
    Enter,
    ShiftEnter,
    Backspace,
    Char(char),
    Cancel,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Init,
    Resize {
        width: u16,
        height: u16,
    },
    Tick {
        now_ms: u64,
    },
    Action(Action),

    AgentListLoaded(Result<Vec<AgentInfo>, String>),
    AgentChatReceived(murmur_protocol::AgentChatEvent),
    AgentChatHistoryLoaded {
        agent_id: String,
        result: Result<Vec<murmur_protocol::ChatMessage>, String>,
    },
    AgentSendMessageFinished {
        agent_id: String,
        result: Result<(), String>,
    },
    StatsLoaded(Result<murmur_protocol::StatsResponse, String>),
    CommitListLoaded(Result<Vec<murmur_protocol::CommitRecord>, String>),
    ProjectListLoaded(Result<Vec<murmur_protocol::ProjectInfo>, String>),
    PermissionListLoaded(Result<Vec<PermissionRequest>, String>),
    PermissionRequested(PermissionRequest),
    PermissionRespondFinished {
        id: String,
        result: Result<(), String>,
    },
    QuestionListLoaded(Result<Vec<UserQuestion>, String>),
    QuestionRequested(UserQuestion),
    QuestionRespondFinished {
        id: String,
        result: Result<(), String>,
    },
    AbortFinished {
        agent_id: String,
        result: Result<(), String>,
    },
    PlanStopFinished {
        plan_id: String,
        result: Result<(), String>,
    },
    PlanStartFinished(Result<murmur_protocol::PlanStartResponse, String>),

    StreamConnected,
    StreamDisconnected {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    FetchAgentList,
    FetchStats {
        project: Option<String>,
    },
    FetchCommitList {
        project: Option<String>,
        limit: u32,
    },
    FetchProjectList,
    FetchAgentChatHistory {
        agent_id: String,
        limit: u32,
    },
    SendAgentMessage {
        agent_id: String,
        message: String,
    },
    AbortAgent {
        agent_id: String,
    },
    StopPlan {
        plan_id: String,
    },
    FetchPermissionList {
        project: Option<String>,
    },
    RespondPermission {
        id: String,
        behavior: PermissionBehavior,
    },
    FetchQuestionList {
        project: Option<String>,
    },
    RespondQuestion {
        id: String,
        answers: BTreeMap<String, String>,
    },
    StartPlan {
        project: Option<String>,
        prompt: String,
    },
    AttachStream {
        projects: Vec<String>,
    },
    ReconnectStream,
    Quit,
}

pub fn reduce(mut model: Model, msg: Msg) -> (Model, Vec<Effect>) {
    let mut effects = Vec::new();

    match msg {
        Msg::Init => {
            model.connection = ConnectionState::Connecting;
            effects.push(Effect::FetchAgentList);
            effects.push(Effect::FetchStats { project: None });
            effects.push(Effect::FetchCommitList {
                project: None,
                limit: 20,
            });
            effects.push(Effect::FetchProjectList);
            effects.push(Effect::FetchPermissionList { project: None });
            effects.push(Effect::FetchQuestionList { project: None });
            effects.push(Effect::AttachStream { projects: vec![] });
        }
        Msg::Resize { width, height } => {
            model.width = width;
            model.height = height;

            let (chat_width, chat_height) = chat_viewport(&model);
            for buf in model.chats.values_mut() {
                chat::rewrap(buf, chat_width, chat_height, model.show_tool_events);
            }
        }
        Msg::Tick { now_ms } => {
            model.now_ms = now_ms;
            if model.next_refresh_ms == 0 && model.now_ms > 0 {
                model.next_refresh_ms = model.now_ms.saturating_add(5_000);
            } else if model.now_ms > 0 && model.now_ms >= model.next_refresh_ms {
                model.next_refresh_ms = model.now_ms.saturating_add(5_000);
                effects.push(Effect::FetchStats { project: None });
                effects.push(Effect::FetchCommitList {
                    project: None,
                    limit: 20,
                });
                effects.push(Effect::FetchPermissionList { project: None });
                effects.push(Effect::FetchQuestionList { project: None });
            }

            if matches!(model.connection, ConnectionState::Disconnected)
                && model.reconnect_attempt < RECONNECT_MAX_ATTEMPTS
            {
                if model.next_reconnect_ms == 0 && model.now_ms > 0 {
                    model.next_reconnect_ms = model
                        .now_ms
                        .saturating_add(reconnect_backoff_ms(model.reconnect_attempt));
                }

                if model.next_reconnect_ms == 0 || model.now_ms < model.next_reconnect_ms {
                    return (model, effects);
                }

                model.connection = ConnectionState::Connecting;
                effects.push(Effect::ReconnectStream);

                model.reconnect_attempt = model.reconnect_attempt.saturating_add(1);
                model.next_reconnect_ms = model
                    .now_ms
                    .saturating_add(reconnect_backoff_ms(model.reconnect_attempt));
            }
        }
        Msg::Action(action) => match action {
            Action::Quit => effects.push(Effect::Quit),
            Action::Tab => {
                if matches!(model.mode, Mode::Input) {
                    model.mode = Mode::Normal;
                    model.focus = Focus::ChatView;
                } else if matches!(model.mode, Mode::Normal) {
                    model.focus = match model.focus {
                        Focus::AgentList => Focus::ChatView,
                        Focus::ChatView | Focus::InputLine => Focus::AgentList,
                    };
                }
            }
            Action::Char(ch) => match model.mode {
                Mode::Input => {
                    model.editor.insert_char(ch);
                }
                Mode::Normal => {
                    if let Some(confirm) = model.abort_confirm.clone() {
                        match ch {
                            'y' => {
                                model.abort_confirm = None;
                                match confirm.role {
                                    murmur_protocol::AgentRole::Planner => {
                                        effects.push(Effect::StopPlan {
                                            plan_id: confirm.agent_id,
                                        });
                                    }
                                    _ => {
                                        effects.push(Effect::AbortAgent {
                                            agent_id: confirm.agent_id,
                                        });
                                    }
                                }
                            }
                            'n' => {
                                model.abort_confirm = None;
                            }
                            'q' => effects.push(Effect::Quit),
                            _ => {}
                        }
                        return (model, effects);
                    }

                    if let Some(PlannerFlow::Picking { selected }) = model.planner_flow.as_mut() {
                        let max = model.projects.len().saturating_add(1);
                        match ch {
                            'j' => *selected = (*selected + 1).min(max.saturating_sub(1)),
                            'k' => *selected = (*selected).saturating_sub(1),
                            'q' => effects.push(Effect::Quit),
                            _ => {}
                        }
                        return (model, effects);
                    }

                    match ch {
                        'p' => {
                            model.planner_flow = Some(PlannerFlow::Picking { selected: 0 });
                            effects.push(Effect::FetchProjectList);
                            return (model, effects);
                        }
                        'x' => {
                            if let Some(agent) = model.selected_agent() {
                                model.abort_confirm = Some(AbortConfirm {
                                    agent_id: agent.id.clone(),
                                    role: agent.role,
                                });
                            }
                            return (model, effects);
                        }
                        _ => {}
                    }

                    let attention = next_attention(&model);
                    let handled = match attention {
                        Some(PendingAttention::Permission) => {
                            handle_permission_key(&mut model, &mut effects, ch)
                        }
                        Some(PendingAttention::Question) => {
                            handle_question_key(&mut model, &mut effects, ch)
                        }
                        None => false,
                    };

                    if !handled {
                        match ch {
                            'q' => effects.push(Effect::Quit),
                            't' => {
                                model.show_tool_events = !model.show_tool_events;
                                let (chat_width, chat_height) = chat_viewport(&model);
                                for buf in model.chats.values_mut() {
                                    chat::rewrap(
                                        buf,
                                        chat_width,
                                        chat_height,
                                        model.show_tool_events,
                                    );
                                }
                            }
                            'j' => {
                                let (next, more) = reduce(model, Msg::Action(Action::MoveDown));
                                model = next;
                                effects.extend(more);
                            }
                            'k' => {
                                let (next, more) = reduce(model, Msg::Action(Action::MoveUp));
                                model = next;
                                effects.extend(more);
                            }
                            'g' => {
                                let (next, more) = reduce(model, Msg::Action(Action::GoTop));
                                model = next;
                                effects.extend(more);
                            }
                            'G' => {
                                let (next, more) = reduce(model, Msg::Action(Action::GoBottom));
                                model = next;
                                effects.extend(more);
                            }
                            'r' => {
                                if matches!(model.connection, ConnectionState::Disconnected) {
                                    model.connection = ConnectionState::Connecting;
                                    model.reconnect_attempt = 0;
                                    model.next_reconnect_ms = 0;
                                    effects.push(Effect::ReconnectStream);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            },
            Action::MoveUp => match model.mode {
                Mode::Input => model.editor.history_prev(),
                Mode::Normal => {
                    if matches!(model.focus, Focus::AgentList) {
                        let old = model.selected_agent;
                        model.selected_agent = model.selected_agent.saturating_sub(1);
                        if model.selected_agent != old {
                            queue_chat_history_if_needed(&model, &mut effects);
                            sync_question_draft(&mut model);
                        }
                    } else if matches!(model.focus, Focus::ChatView) {
                        let Some(agent_id) = model.selected_agent().map(|a| a.id.clone()) else {
                            return (model, effects);
                        };
                        if let Some(buf) = model.chats.get_mut(&agent_id) {
                            chat::scroll_up(buf, 1);
                        }
                    }
                }
            },
            Action::MoveDown => match model.mode {
                Mode::Input => model.editor.history_next(),
                Mode::Normal => {
                    if matches!(model.focus, Focus::AgentList) && !model.agents.is_empty() {
                        let old = model.selected_agent;
                        model.selected_agent =
                            (model.selected_agent + 1).min(model.agents.len() - 1);
                        if model.selected_agent != old {
                            queue_chat_history_if_needed(&model, &mut effects);
                            sync_question_draft(&mut model);
                        }
                    } else if matches!(model.focus, Focus::ChatView) {
                        let (_chat_width, chat_height) = chat_viewport(&model);
                        let Some(agent_id) = model.selected_agent().map(|a| a.id.clone()) else {
                            return (model, effects);
                        };
                        if let Some(buf) = model.chats.get_mut(&agent_id) {
                            chat::scroll_down(buf, chat_height, 1);
                        }
                    }
                }
            },
            Action::GoTop => {
                if matches!(model.mode, Mode::Normal) && matches!(model.focus, Focus::AgentList) {
                    let old = model.selected_agent;
                    model.selected_agent = 0;
                    if model.selected_agent != old {
                        queue_chat_history_if_needed(&model, &mut effects);
                        sync_question_draft(&mut model);
                    }
                } else if matches!(model.mode, Mode::Normal)
                    && matches!(model.focus, Focus::ChatView)
                {
                    let Some(agent_id) = model.selected_agent().map(|a| a.id.clone()) else {
                        return (model, effects);
                    };
                    if let Some(buf) = model.chats.get_mut(&agent_id) {
                        chat::jump_top(buf);
                    }
                }
            }
            Action::GoBottom => {
                if matches!(model.mode, Mode::Normal)
                    && matches!(model.focus, Focus::AgentList)
                    && !model.agents.is_empty()
                {
                    let old = model.selected_agent;
                    model.selected_agent = model.agents.len() - 1;
                    if model.selected_agent != old {
                        queue_chat_history_if_needed(&model, &mut effects);
                        sync_question_draft(&mut model);
                    }
                } else if matches!(model.mode, Mode::Normal)
                    && matches!(model.focus, Focus::ChatView)
                {
                    let (_chat_width, chat_height) = chat_viewport(&model);
                    let Some(agent_id) = model.selected_agent().map(|a| a.id.clone()) else {
                        return (model, effects);
                    };
                    if let Some(buf) = model.chats.get_mut(&agent_id) {
                        chat::jump_bottom(buf, chat_height);
                    }
                }
            }
            Action::PageUp => {
                if matches!(model.mode, Mode::Normal) && matches!(model.focus, Focus::ChatView) {
                    let (_chat_width, chat_height) = chat_viewport(&model);
                    let jump = chat_height.max(1);
                    let Some(agent_id) = model.selected_agent().map(|a| a.id.clone()) else {
                        return (model, effects);
                    };
                    if let Some(buf) = model.chats.get_mut(&agent_id) {
                        chat::scroll_up(buf, jump);
                    }
                }
            }
            Action::PageDown => {
                if matches!(model.mode, Mode::Normal) && matches!(model.focus, Focus::ChatView) {
                    let (_chat_width, chat_height) = chat_viewport(&model);
                    let jump = chat_height.max(1);
                    let Some(agent_id) = model.selected_agent().map(|a| a.id.clone()) else {
                        return (model, effects);
                    };
                    if let Some(buf) = model.chats.get_mut(&agent_id) {
                        chat::scroll_down(buf, chat_height, jump);
                    }
                }
            }
            Action::Enter => match model.mode {
                Mode::Normal => {
                    if model.abort_confirm.is_some() {
                        return (model, effects);
                    }
                    if let Some(PlannerFlow::Picking { selected }) = model.planner_flow.take() {
                        let project = if selected == 0 {
                            None
                        } else {
                            model
                                .projects
                                .get(selected.saturating_sub(1))
                                .map(|p| p.name.clone())
                        };
                        model.planner_flow = Some(PlannerFlow::Prompt { project });
                        model.mode = Mode::Input;
                        model.focus = Focus::InputLine;
                        model.input_context = InputContext::PlannerPrompt;
                        model.editor.history_cursor = None;
                        model.editor.clear_buffer();
                        return (model, effects);
                    }

                    model.mode = Mode::Input;
                    model.focus = Focus::InputLine;
                    model.input_context = InputContext::Chat;
                    model.editor.history_cursor = None;
                }
                Mode::Input => {
                    if let Some(message) = model.editor.take_submit() {
                        match model.input_context {
                            InputContext::Chat => {
                                let Some(agent_id) = model.selected_agent().map(|a| a.id.clone())
                                else {
                                    model.editor.buffer = message;
                                    model.status = Some("no agent selected".to_owned());
                                    return (model, effects);
                                };

                                model.mode = Mode::Normal;
                                model.focus = Focus::ChatView;
                                model.status = None;

                                let (chat_width, chat_height) = chat_viewport(&model);
                                let buf = model
                                    .chats
                                    .entry(agent_id.clone())
                                    .or_insert_with(ChatBuffer::new);
                                chat::append_message(
                                    buf,
                                    murmur_protocol::ChatMessage {
                                        role: murmur_protocol::ChatRole::User,
                                        content: message.clone(),
                                        tool_name: None,
                                        tool_input: None,
                                        tool_use_id: None,
                                        tool_result: None,
                                        is_error: false,
                                        ts_ms: model.now_ms,
                                    },
                                    chat_width,
                                    chat_height,
                                    model.show_tool_events,
                                );

                                effects.push(Effect::SendAgentMessage { agent_id, message });
                            }
                            InputContext::QuestionOther => {
                                let answer = message.trim().to_owned();
                                if answer.is_empty() {
                                    model.editor.buffer = message;
                                    model.status = Some("answer is required".to_owned());
                                    return (model, effects);
                                }

                                model.mode = Mode::Normal;
                                model.focus = Focus::ChatView;
                                model.input_context = InputContext::Chat;
                                model.status = None;

                                apply_question_other_answer(&mut model, &mut effects, answer);
                            }
                            InputContext::PlannerPrompt => {
                                let prompt = message.trim().to_owned();
                                if prompt.is_empty() {
                                    model.editor.buffer = message;
                                    model.status = Some("prompt is required".to_owned());
                                    return (model, effects);
                                }

                                let project = match model.planner_flow.as_ref() {
                                    Some(PlannerFlow::Prompt { project }) => project.clone(),
                                    _ => None,
                                };

                                model.mode = Mode::Normal;
                                model.focus = Focus::ChatView;
                                model.input_context = InputContext::Chat;
                                model.planner_flow = None;
                                model.status = None;

                                effects.push(Effect::StartPlan { project, prompt });
                            }
                        }
                    }
                }
            },
            Action::ShiftEnter => {
                if matches!(model.mode, Mode::Input) {
                    model.editor.insert_newline();
                }
            }
            Action::Backspace => {
                if matches!(model.mode, Mode::Input) {
                    model.editor.backspace();
                }
            }
            Action::Cancel => {
                if model.abort_confirm.is_some() {
                    model.abort_confirm = None;
                    return (model, effects);
                }
                if model.planner_flow.is_some() && matches!(model.mode, Mode::Normal) {
                    model.planner_flow = None;
                    return (model, effects);
                }
                if matches!(model.mode, Mode::Input) {
                    let was_planner_prompt =
                        matches!(model.input_context, InputContext::PlannerPrompt);
                    model.mode = Mode::Normal;
                    model.focus = Focus::ChatView;
                    model.input_context = InputContext::Chat;
                    if was_planner_prompt {
                        model.planner_flow = None;
                    }
                    model.editor.clear_buffer();
                }
            }
        },
        Msg::AgentListLoaded(result) => match result {
            Ok(agents) => {
                let prev_selected_id = model.selected_agent().map(|a| a.id.clone());

                let mut agents = agents;
                agents.sort_by(|a, b| {
                    role_rank(a.role)
                        .cmp(&role_rank(b.role))
                        .then(a.id.cmp(&b.id))
                });

                model.agents = agents;

                if model.agents.is_empty() {
                    model.selected_agent = 0;
                } else if let Some(id) = prev_selected_id
                    .as_deref()
                    .and_then(|id| model.agents.iter().position(|a| a.id == id))
                {
                    model.selected_agent = id;
                } else if model.selected_agent >= model.agents.len() {
                    model.selected_agent = model.agents.len() - 1;
                }
                model.status = None;

                queue_chat_history_if_needed(&model, &mut effects);
                sync_question_draft(&mut model);
            }
            Err(err) => {
                model.status = Some(err);
            }
        },
        Msg::AgentChatReceived(evt) => {
            let (chat_width, chat_height) = chat_viewport(&model);
            let buf = model
                .chats
                .entry(evt.agent_id)
                .or_insert_with(ChatBuffer::new);
            chat::append_message(
                buf,
                evt.message,
                chat_width,
                chat_height,
                model.show_tool_events,
            );
        }
        Msg::AgentChatHistoryLoaded { agent_id, result } => match result {
            Ok(history) => {
                let (chat_width, chat_height) = chat_viewport(&model);
                let buf = model.chats.entry(agent_id).or_insert_with(ChatBuffer::new);
                let merged = chat::merge_history(&history, &buf.messages);
                buf.messages = merged;
                buf.history_loaded = true;
                chat::rewrap(buf, chat_width, chat_height, model.show_tool_events);
            }
            Err(err) => {
                model.status = Some(err);
            }
        },
        Msg::AgentSendMessageFinished { agent_id, result } => match result {
            Ok(()) => {}
            Err(err) => {
                model.status = Some(err.clone());
                let (chat_width, chat_height) = chat_viewport(&model);
                let buf = model.chats.entry(agent_id).or_insert_with(ChatBuffer::new);
                chat::append_message(
                    buf,
                    murmur_protocol::ChatMessage {
                        role: murmur_protocol::ChatRole::System,
                        content: format!("send failed: {err}"),
                        tool_name: None,
                        tool_input: None,
                        tool_use_id: None,
                        tool_result: None,
                        is_error: false,
                        ts_ms: model.now_ms,
                    },
                    chat_width,
                    chat_height,
                    model.show_tool_events,
                );
            }
        },
        Msg::StatsLoaded(result) => match result {
            Ok(stats) => {
                model.commit_count = stats.commit_count;
            }
            Err(err) => model.status = Some(err),
        },
        Msg::CommitListLoaded(result) => match result {
            Ok(commits) => model.recent_commits = commits,
            Err(err) => model.status = Some(err),
        },
        Msg::ProjectListLoaded(result) => match result {
            Ok(projects) => {
                model.projects = projects;
                if let Some(PlannerFlow::Picking { selected }) = model.planner_flow.as_mut() {
                    let max = model.projects.len().saturating_add(1);
                    if max == 0 {
                        *selected = 0;
                    } else {
                        *selected = (*selected).min(max - 1);
                    }
                }
            }
            Err(err) => model.status = Some(err),
        },
        Msg::PermissionListLoaded(result) => match result {
            Ok(requests) => {
                model.pending_permissions = requests;
            }
            Err(err) => model.status = Some(err),
        },
        Msg::PermissionRequested(req) => {
            if !model.pending_permissions.iter().any(|p| p.id == req.id) {
                model.pending_permissions.push(req);
            }
        }
        Msg::PermissionRespondFinished { id, result } => match result {
            Ok(()) => {
                model.pending_permissions.retain(|p| p.id != id);
            }
            Err(err) => {
                model.status = Some(err);
                effects.push(Effect::FetchPermissionList { project: None });
            }
        },
        Msg::QuestionListLoaded(result) => match result {
            Ok(requests) => {
                model.pending_questions = requests;
                sync_question_draft(&mut model);
            }
            Err(err) => model.status = Some(err),
        },
        Msg::QuestionRequested(req) => {
            if !model.pending_questions.iter().any(|q| q.id == req.id) {
                model.pending_questions.push(req);
                sync_question_draft(&mut model);
            }
        }
        Msg::QuestionRespondFinished { id, result } => match result {
            Ok(()) => {
                model.pending_questions.retain(|q| q.id != id);
                if model
                    .question_draft
                    .as_ref()
                    .is_some_and(|q| q.request_id == id)
                {
                    model.question_draft = None;
                }
            }
            Err(err) => {
                model.status = Some(err);
                effects.push(Effect::FetchQuestionList { project: None });
            }
        },
        Msg::AbortFinished { agent_id, result } => match result {
            Ok(()) => {
                effects.push(Effect::FetchAgentList);
                model.status = None;
                if let Some(buf) = model.chats.get_mut(&agent_id) {
                    buf.follow_tail = true;
                }
            }
            Err(err) => model.status = Some(err),
        },
        Msg::PlanStopFinished { plan_id, result } => match result {
            Ok(()) => {
                effects.push(Effect::FetchAgentList);
                model.status = None;
                model.chats.remove(&plan_id);
            }
            Err(err) => model.status = Some(err),
        },
        Msg::PlanStartFinished(result) => match result {
            Ok(_resp) => {
                effects.push(Effect::FetchAgentList);
                model.status = None;
            }
            Err(err) => model.status = Some(err),
        },
        Msg::StreamConnected => {
            model.connection = ConnectionState::Connected;
            model.status = None;
            model.reconnect_attempt = 0;
            model.next_reconnect_ms = 0;

            effects.push(Effect::FetchAgentList);
            effects.push(Effect::FetchStats { project: None });
            effects.push(Effect::FetchCommitList {
                project: None,
                limit: 20,
            });
            effects.push(Effect::FetchProjectList);
            effects.push(Effect::FetchPermissionList { project: None });
            effects.push(Effect::FetchQuestionList { project: None });
        }
        Msg::StreamDisconnected { reason } => {
            let was_connected = matches!(model.connection, ConnectionState::Connected);
            model.connection = ConnectionState::Disconnected;
            model.status = Some(reason);

            if was_connected {
                model.reconnect_attempt = 0;
            }
            if model.next_reconnect_ms == 0 && model.now_ms > 0 {
                model.next_reconnect_ms = model
                    .now_ms
                    .saturating_add(reconnect_backoff_ms(model.reconnect_attempt));
            }
        }
    }

    (model, effects)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpItem {
    pub key: &'static str,
    pub desc: &'static str,
}

pub fn help_items(model: &Model) -> Vec<HelpItem> {
    let mut items = Vec::new();

    match model.mode {
        Mode::Input => {
            items.push(HelpItem {
                key: "Enter",
                desc: match model.input_context {
                    InputContext::Chat => "send",
                    InputContext::QuestionOther => "answer",
                    InputContext::PlannerPrompt => "start",
                },
            });
            items.push(HelpItem {
                key: "Shift+Enter",
                desc: "newline",
            });
            items.push(HelpItem {
                key: "↑/↓",
                desc: "history",
            });
            items.push(HelpItem {
                key: "Esc",
                desc: "cancel",
            });
            items.push(HelpItem {
                key: "Tab",
                desc: "exit",
            });
            items.push(HelpItem {
                key: "Ctrl+C",
                desc: "quit",
            });
            return items;
        }
        Mode::Normal => {}
    }

    if matches!(model.mode, Mode::Normal) && model.abort_confirm.is_some() {
        items.push(HelpItem {
            key: "y",
            desc: "confirm",
        });
        items.push(HelpItem {
            key: "n",
            desc: "cancel",
        });
        items.push(HelpItem {
            key: "Esc",
            desc: "cancel",
        });
        items.push(HelpItem {
            key: "q",
            desc: "quit",
        });
        return items;
    }

    items.push(HelpItem {
        key: "t",
        desc: if model.show_tool_events {
            "tool events on"
        } else {
            "tool events off"
        },
    });

    if matches!(model.mode, Mode::Normal)
        && matches!(model.planner_flow, Some(PlannerFlow::Picking { .. }))
    {
        items.push(HelpItem {
            key: "j/k",
            desc: "move",
        });
        items.push(HelpItem {
            key: "Enter",
            desc: "select",
        });
        items.push(HelpItem {
            key: "Esc",
            desc: "cancel",
        });
        items.push(HelpItem {
            key: "q",
            desc: "quit",
        });
        return items;
    }

    match next_attention(model) {
        Some(PendingAttention::Permission) => {
            items.push(HelpItem {
                key: "y",
                desc: "allow",
            });
            items.push(HelpItem {
                key: "n",
                desc: "deny",
            });
        }
        Some(PendingAttention::Question) => {
            items.push(HelpItem {
                key: "j/k",
                desc: "choose",
            });
            items.push(HelpItem {
                key: "y",
                desc: "submit",
            });
        }
        None => {}
    }

    if matches!(model.connection, ConnectionState::Disconnected) {
        items.push(HelpItem {
            key: "r",
            desc: "reconnect",
        });
    }

    match model.focus {
        Focus::AgentList => {
            items.push(HelpItem {
                key: "j/k",
                desc: "move",
            });
            items.push(HelpItem {
                key: "Tab",
                desc: "switch pane",
            });
            items.push(HelpItem {
                key: "q",
                desc: "quit",
            });
        }
        Focus::ChatView => {
            items.push(HelpItem {
                key: "j/k",
                desc: "scroll",
            });
            items.push(HelpItem {
                key: "PgUp/PgDn",
                desc: "page",
            });
            items.push(HelpItem {
                key: "Enter",
                desc: "input",
            });
            items.push(HelpItem {
                key: "Tab",
                desc: "switch pane",
            });
            items.push(HelpItem {
                key: "q",
                desc: "quit",
            });
        }
        Focus::InputLine => {
            items.push(HelpItem {
                key: "Esc",
                desc: "cancel",
            });
            items.push(HelpItem {
                key: "Tab",
                desc: "exit",
            });
            items.push(HelpItem {
                key: "q",
                desc: "quit",
            });
        }
    }

    items
}

fn chat_viewport(model: &Model) -> (usize, usize) {
    let width = model.width as usize;
    let height = model.height as usize;

    let left_w = width * 35 / 100;
    let right_w = width.saturating_sub(left_w);
    let chat_width = right_w.saturating_sub(2);

    let main_height = height.saturating_sub(2);
    let input_height = input_panel_height(model, main_height);
    let chat_height = main_height.saturating_sub(input_height).saturating_sub(2);

    (chat_width, chat_height)
}

fn input_panel_height(model: &Model, max_height: usize) -> usize {
    if !matches!(model.mode, Mode::Input) {
        return 0;
    }

    let inner = model.editor.visual_lines().clamp(1, 6);
    let desired = (inner + 2).max(3);
    let max_total = max_height.saturating_sub(3).max(3);
    desired.min(max_total)
}

fn queue_chat_history_if_needed(model: &Model, effects: &mut Vec<Effect>) {
    let Some(agent_id) = model.selected_agent().map(|a| a.id.clone()) else {
        return;
    };
    let needs = model
        .chats
        .get(&agent_id)
        .map(|c| !c.history_loaded)
        .unwrap_or(true);
    if needs {
        effects.push(Effect::FetchAgentChatHistory {
            agent_id,
            limit: 200,
        });
    }
}

fn role_rank(role: murmur_protocol::AgentRole) -> u8 {
    match role {
        murmur_protocol::AgentRole::Manager => 0,
        murmur_protocol::AgentRole::Planner => 1,
        murmur_protocol::AgentRole::Coding => 2,
    }
}

const RECONNECT_BASE_MS: u64 = 500;
const RECONNECT_MAX_MS: u64 = 10_000;
const RECONNECT_MAX_ATTEMPTS: u32 = 10;

fn reconnect_backoff_ms(attempt: u32) -> u64 {
    let shift = attempt.min(20);
    let exp = 1u64 << shift;
    RECONNECT_BASE_MS.saturating_mul(exp).min(RECONNECT_MAX_MS)
}

fn next_attention(model: &Model) -> Option<PendingAttention> {
    let permission = model.pending_permission_for_selected();
    let question = model.pending_question_for_selected();

    match (permission, question) {
        (Some(p), Some(q)) => {
            let p_key = (p.requested_at_ms, p.id.as_str());
            let q_key = (q.requested_at_ms, q.id.as_str());
            if p_key <= q_key {
                Some(PendingAttention::Permission)
            } else {
                Some(PendingAttention::Question)
            }
        }
        (Some(_), None) => Some(PendingAttention::Permission),
        (None, Some(_)) => Some(PendingAttention::Question),
        (None, None) => None,
    }
}

fn handle_permission_key(model: &mut Model, effects: &mut Vec<Effect>, ch: char) -> bool {
    let behavior = match ch {
        'y' => PermissionBehavior::Allow,
        'n' => PermissionBehavior::Deny,
        _ => return false,
    };

    let Some(req) = model.pending_permission_for_selected().cloned() else {
        return false;
    };

    effects.push(Effect::RespondPermission {
        id: req.id,
        behavior,
    });
    true
}

fn handle_question_key(model: &mut Model, effects: &mut Vec<Effect>, ch: char) -> bool {
    let Some(req) = model.pending_question_for_selected().cloned() else {
        model.question_draft = None;
        return false;
    };

    if model
        .question_draft
        .as_ref()
        .is_none_or(|d| d.request_id != req.id || d.agent_id != req.agent_id)
    {
        model.question_draft = Some(QuestionDraft {
            request_id: req.id.clone(),
            agent_id: req.agent_id.clone(),
            question_index: 0,
            option_index: 0,
            answers: BTreeMap::new(),
        });
    }

    let Some(draft) = model.question_draft.as_mut() else {
        return false;
    };

    if req.questions.is_empty() {
        return false;
    }

    if draft.question_index >= req.questions.len() {
        draft.question_index = 0;
        draft.option_index = 0;
        draft.answers.clear();
    }

    let item = &req.questions[draft.question_index];
    let other_index = item.options.len();
    draft.option_index = draft.option_index.min(other_index);

    match ch {
        'j' => {
            draft.option_index = (draft.option_index + 1).min(other_index);
            true
        }
        'k' => {
            draft.option_index = draft.option_index.saturating_sub(1);
            true
        }
        'y' => {
            if draft.option_index == other_index {
                model.mode = Mode::Input;
                model.focus = Focus::InputLine;
                model.input_context = InputContext::QuestionOther;
                model.editor.history_cursor = None;
                model.editor.clear_buffer();
                return true;
            }

            let Some(opt) = item.options.get(draft.option_index) else {
                return false;
            };
            draft.answers.insert(item.header.clone(), opt.label.clone());
            draft.question_index += 1;
            draft.option_index = 0;

            if draft.question_index >= req.questions.len() {
                effects.push(Effect::RespondQuestion {
                    id: req.id.clone(),
                    answers: draft.answers.clone(),
                });
            }

            true
        }
        _ => false,
    }
}

fn sync_question_draft(model: &mut Model) {
    let Some(req) = model.pending_question_for_selected().cloned() else {
        model.question_draft = None;
        return;
    };

    if model
        .question_draft
        .as_ref()
        .is_none_or(|d| d.request_id != req.id || d.agent_id != req.agent_id)
    {
        model.question_draft = Some(QuestionDraft {
            request_id: req.id,
            agent_id: req.agent_id,
            question_index: 0,
            option_index: 0,
            answers: BTreeMap::new(),
        });
        return;
    }

    let Some(draft) = model.question_draft.as_mut() else {
        return;
    };

    if req.questions.is_empty() {
        model.question_draft = None;
        return;
    }

    draft.question_index = draft.question_index.min(req.questions.len() - 1);
    let item = &req.questions[draft.question_index];
    draft.option_index = draft.option_index.min(item.options.len());
}

fn apply_question_other_answer(model: &mut Model, effects: &mut Vec<Effect>, answer: String) {
    let Some(req) = model.pending_question_for_selected().cloned() else {
        model.status = Some("no pending question".to_owned());
        model.question_draft = None;
        return;
    };

    if model
        .question_draft
        .as_ref()
        .is_none_or(|d| d.request_id != req.id || d.agent_id != req.agent_id)
    {
        model.status = Some("question state out of date".to_owned());
        model.question_draft = None;
        return;
    }

    let Some(draft) = model.question_draft.as_mut() else {
        return;
    };
    let Some(item) = req.questions.get(draft.question_index) else {
        model.status = Some("question index out of range".to_owned());
        model.question_draft = None;
        return;
    };

    draft.answers.insert(item.header.clone(), answer);
    draft.question_index += 1;
    draft.option_index = 0;

    if draft.question_index >= req.questions.len() {
        effects.push(Effect::RespondQuestion {
            id: req.id,
            answers: draft.answers.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_focus_in_normal_mode() {
        let model = Model::new();
        let (model, _effects) = reduce(model, Msg::Action(Action::Tab));
        assert_eq!(model.focus, Focus::ChatView);
        let (model, _effects) = reduce(model, Msg::Action(Action::Tab));
        assert_eq!(model.focus, Focus::AgentList);
    }

    #[test]
    fn enter_and_exit_input_mode_updates_focus() {
        let model = Model::new();
        let (model, _effects) = reduce(model, Msg::Action(Action::Enter));
        assert_eq!(model.mode, Mode::Input);
        assert_eq!(model.focus, Focus::InputLine);
        model.validate().unwrap();

        let (model, _effects) = reduce(model, Msg::Action(Action::Cancel));
        assert_eq!(model.mode, Mode::Normal);
        assert_eq!(model.focus, Focus::ChatView);
        model.validate().unwrap();
    }

    #[test]
    fn agent_selection_is_clamped_on_list_load() {
        let mut model = Model::new();
        model.selected_agent = 10;

        let agents = vec![AgentInfo {
            id: "a-1".to_owned(),
            project: "p".to_owned(),
            role: murmur_protocol::AgentRole::Coding,
            issue_id: "ISSUE-1".to_owned(),
            state: murmur_protocol::AgentState::Running,
            backend: None,
            description: None,
            worktree_dir: "/tmp".to_owned(),
            pid: None,
            exit_code: None,
            created_at_ms: 0,
        }];

        let (model, _effects) = reduce(model, Msg::AgentListLoaded(Ok(agents)));
        assert_eq!(model.selected_agent, 0);
    }

    #[test]
    fn stream_disconnect_sets_status_and_connection() {
        let model = Model::new();
        let (model, _effects) = reduce(
            model,
            Msg::StreamDisconnected {
                reason: "nope".to_owned(),
            },
        );
        assert_eq!(model.connection, ConnectionState::Disconnected);
        assert_eq!(model.status.as_deref(), Some("nope"));

        let (model, _effects) = reduce(model, Msg::StreamConnected);
        assert_eq!(model.connection, ConnectionState::Connected);
        assert_eq!(model.status, None);
    }

    #[test]
    fn input_enter_sends_message_and_appends_optimistically() {
        let mut model = Model::new();
        model.width = 80;
        model.height = 24;
        model.agents = vec![AgentInfo {
            id: "a-1".to_owned(),
            project: "p".to_owned(),
            role: murmur_protocol::AgentRole::Coding,
            issue_id: "ISSUE-1".to_owned(),
            state: murmur_protocol::AgentState::Running,
            backend: None,
            description: None,
            worktree_dir: "/tmp".to_owned(),
            pid: None,
            exit_code: None,
            created_at_ms: 1,
        }];
        model.selected_agent = 0;
        model.mode = Mode::Input;
        model.focus = Focus::InputLine;
        model.editor.buffer = "hello".to_owned();

        let (model, effects) = reduce(model, Msg::Action(Action::Enter));
        assert_eq!(model.mode, Mode::Normal);
        assert_eq!(model.focus, Focus::ChatView);
        assert_eq!(
            effects,
            vec![Effect::SendAgentMessage {
                agent_id: "a-1".to_owned(),
                message: "hello".to_owned()
            }]
        );

        let buf = model.chats.get("a-1").unwrap();
        assert!(buf
            .messages
            .iter()
            .any(|m| matches!(m.role, murmur_protocol::ChatRole::User) && m.content == "hello"));
    }

    #[test]
    fn send_error_sets_status_and_appends_system_message() {
        let mut model = Model::new();
        model.width = 80;
        model.height = 24;

        let (model, _effects) = reduce(
            model,
            Msg::AgentSendMessageFinished {
                agent_id: "a-1".to_owned(),
                result: Err("nope".to_owned()),
            },
        );
        assert_eq!(model.status.as_deref(), Some("nope"));
        let buf = model.chats.get("a-1").unwrap();
        assert!(buf.messages.iter().any(|m| {
            matches!(m.role, murmur_protocol::ChatRole::System) && m.content.contains("send failed")
        }));
    }

    #[test]
    fn tick_schedules_periodic_stats_and_recent_work_refresh() {
        let model = Model::new();
        let (model, effects) = reduce(model, Msg::Tick { now_ms: 1_000 });
        assert!(effects.is_empty());
        assert_eq!(model.next_refresh_ms, 6_000);

        let (model, effects) = reduce(model, Msg::Tick { now_ms: 6_000 });
        assert_eq!(model.next_refresh_ms, 11_000);
        assert_eq!(
            effects,
            vec![
                Effect::FetchStats { project: None },
                Effect::FetchCommitList {
                    project: None,
                    limit: 20
                },
                Effect::FetchPermissionList { project: None },
                Effect::FetchQuestionList { project: None },
            ]
        );
    }

    #[test]
    fn reconnect_backoff_is_exponential_and_clamped() {
        assert_eq!(reconnect_backoff_ms(0), 500);
        assert_eq!(reconnect_backoff_ms(1), 1_000);
        assert_eq!(reconnect_backoff_ms(2), 2_000);
        assert_eq!(reconnect_backoff_ms(3), 4_000);
        assert_eq!(reconnect_backoff_ms(4), 8_000);
        assert_eq!(reconnect_backoff_ms(5), 10_000);
        assert_eq!(reconnect_backoff_ms(10), 10_000);
    }

    #[test]
    fn tick_triggers_reconnect_stream_after_backoff() {
        let mut model = Model::new();
        model.connection = ConnectionState::Disconnected;
        model.now_ms = 1_000;
        model.next_reconnect_ms = 1_000;

        let (model, effects) = reduce(model, Msg::Tick { now_ms: 1_000 });
        assert_eq!(model.connection, ConnectionState::Connecting);
        assert_eq!(model.reconnect_attempt, 1);
        assert!(effects.contains(&Effect::ReconnectStream));
    }

    #[test]
    fn permission_allow_denies_oldest_request_for_selected_agent() {
        let mut model = Model::new();
        model.agents = vec![AgentInfo {
            id: "a-1".to_owned(),
            project: "demo".to_owned(),
            role: murmur_protocol::AgentRole::Coding,
            issue_id: "ISSUE-1".to_owned(),
            state: murmur_protocol::AgentState::Running,
            backend: None,
            description: None,
            worktree_dir: "/tmp".to_owned(),
            pid: None,
            exit_code: None,
            created_at_ms: 0,
        }];
        model.selected_agent = 0;
        model.pending_permissions = vec![
            PermissionRequest {
                id: "perm-new".to_owned(),
                agent_id: "a-1".to_owned(),
                project: "demo".to_owned(),
                tool_name: "Bash".to_owned(),
                tool_input: serde_json::json!({"command":"echo new"}),
                tool_use_id: None,
                requested_at_ms: 20,
            },
            PermissionRequest {
                id: "perm-old".to_owned(),
                agent_id: "a-1".to_owned(),
                project: "demo".to_owned(),
                tool_name: "Bash".to_owned(),
                tool_input: serde_json::json!({"command":"echo old"}),
                tool_use_id: None,
                requested_at_ms: 10,
            },
        ];

        let (_model, effects) = reduce(model, Msg::Action(Action::Char('y')));
        assert_eq!(
            effects,
            vec![Effect::RespondPermission {
                id: "perm-old".to_owned(),
                behavior: PermissionBehavior::Allow,
            }]
        );
    }

    #[test]
    fn question_navigation_and_submit_emits_respond_effect() {
        let mut model = Model::new();
        model.agents = vec![AgentInfo {
            id: "a-1".to_owned(),
            project: "demo".to_owned(),
            role: murmur_protocol::AgentRole::Coding,
            issue_id: "ISSUE-1".to_owned(),
            state: murmur_protocol::AgentState::Running,
            backend: None,
            description: None,
            worktree_dir: "/tmp".to_owned(),
            pid: None,
            exit_code: None,
            created_at_ms: 0,
        }];
        model.selected_agent = 0;

        let req = UserQuestion {
            id: "q-1".to_owned(),
            agent_id: "a-1".to_owned(),
            project: "demo".to_owned(),
            questions: vec![murmur_protocol::QuestionItem {
                question: "Pick one".to_owned(),
                header: "choice".to_owned(),
                multi_select: false,
                options: vec![
                    murmur_protocol::QuestionOption {
                        label: "A".to_owned(),
                        description: "Option A".to_owned(),
                    },
                    murmur_protocol::QuestionOption {
                        label: "B".to_owned(),
                        description: "Option B".to_owned(),
                    },
                ],
            }],
            requested_at_ms: 1,
        };

        let (model, _effects) = reduce(model, Msg::QuestionListLoaded(Ok(vec![req])));

        let (model, _effects) = reduce(model, Msg::Action(Action::Char('j')));
        assert_eq!(model.question_draft.as_ref().unwrap().option_index, 1);

        let (_model, effects) = reduce(model, Msg::Action(Action::Char('y')));
        assert_eq!(
            effects,
            vec![Effect::RespondQuestion {
                id: "q-1".to_owned(),
                answers: BTreeMap::from_iter([("choice".to_owned(), "B".to_owned())]),
            }]
        );
    }

    #[test]
    fn question_other_enters_input_and_submits_freeform_answer() {
        let mut model = Model::new();
        model.agents = vec![AgentInfo {
            id: "a-1".to_owned(),
            project: "demo".to_owned(),
            role: murmur_protocol::AgentRole::Coding,
            issue_id: "ISSUE-1".to_owned(),
            state: murmur_protocol::AgentState::Running,
            backend: None,
            description: None,
            worktree_dir: "/tmp".to_owned(),
            pid: None,
            exit_code: None,
            created_at_ms: 0,
        }];
        model.selected_agent = 0;

        let req = UserQuestion {
            id: "q-1".to_owned(),
            agent_id: "a-1".to_owned(),
            project: "demo".to_owned(),
            questions: vec![murmur_protocol::QuestionItem {
                question: "Pick one".to_owned(),
                header: "choice".to_owned(),
                multi_select: false,
                options: vec![murmur_protocol::QuestionOption {
                    label: "A".to_owned(),
                    description: "Option A".to_owned(),
                }],
            }],
            requested_at_ms: 1,
        };

        let (model, _effects) = reduce(model, Msg::QuestionListLoaded(Ok(vec![req])));
        let (model, _effects) = reduce(model, Msg::Action(Action::Char('j')));
        let (model, _effects) = reduce(model, Msg::Action(Action::Char('y')));
        assert_eq!(model.mode, Mode::Input);
        assert_eq!(model.input_context, InputContext::QuestionOther);

        let mut model = model;
        model.editor.buffer = "custom".to_owned();
        let (_model, effects) = reduce(model, Msg::Action(Action::Enter));
        assert_eq!(
            effects,
            vec![Effect::RespondQuestion {
                id: "q-1".to_owned(),
                answers: BTreeMap::from_iter([("choice".to_owned(), "custom".to_owned())]),
            }]
        );
    }

    #[test]
    fn abort_confirm_emits_abort_effect() {
        let mut model = Model::new();
        model.agents = vec![AgentInfo {
            id: "a-1".to_owned(),
            project: "demo".to_owned(),
            role: murmur_protocol::AgentRole::Coding,
            issue_id: "ISSUE-1".to_owned(),
            state: murmur_protocol::AgentState::Running,
            backend: None,
            description: None,
            worktree_dir: "/tmp".to_owned(),
            pid: None,
            exit_code: None,
            created_at_ms: 0,
        }];
        model.selected_agent = 0;

        let (model, _effects) = reduce(model, Msg::Action(Action::Char('x')));
        assert!(model.abort_confirm.is_some());

        let (_model, effects) = reduce(model, Msg::Action(Action::Char('y')));
        assert_eq!(
            effects,
            vec![Effect::AbortAgent {
                agent_id: "a-1".to_owned(),
            }]
        );
    }

    #[test]
    fn planner_flow_picks_project_and_starts_plan() {
        let mut model = Model::new();
        model.projects = vec![murmur_protocol::ProjectInfo {
            name: "demo".to_owned(),
            remote_url: "/tmp/origin.git".to_owned(),
            repo_dir: "/tmp/repo".to_owned(),
            max_agents: 1,
            running: false,
            backend: "claude".to_owned(),
        }];

        let (model, effects) = reduce(model, Msg::Action(Action::Char('p')));
        assert!(matches!(
            model.planner_flow,
            Some(PlannerFlow::Picking { .. })
        ));
        assert_eq!(effects, vec![Effect::FetchProjectList]);

        let (model, _effects) = reduce(model, Msg::Action(Action::Char('j')));
        assert!(matches!(
            model.planner_flow,
            Some(PlannerFlow::Picking { selected: 1 })
        ));

        let (model, _effects) = reduce(model, Msg::Action(Action::Enter));
        assert_eq!(model.mode, Mode::Input);
        assert_eq!(model.input_context, InputContext::PlannerPrompt);
        assert!(matches!(
            model.planner_flow,
            Some(PlannerFlow::Prompt { project: Some(ref p) }) if p == "demo"
        ));

        let mut model = model;
        model.editor.buffer = "make a plan".to_owned();
        let (_model, effects) = reduce(model, Msg::Action(Action::Enter));
        assert_eq!(
            effects,
            vec![Effect::StartPlan {
                project: Some("demo".to_owned()),
                prompt: "make a plan".to_owned(),
            }]
        );
    }
}
