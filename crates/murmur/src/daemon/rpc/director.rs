use std::sync::Arc;

use murmur_core::agent::{AgentEvent, AgentRecord, AgentRole, ChatHistory, ChatMessage, ChatRole};
use murmur_core::config::AgentBackend;
use murmur_protocol::{
    DirectorChatHistoryRequest, DirectorChatHistoryResponse, DirectorSendMessageRequest,
    DirectorStartRequest, DirectorStartResponse, DirectorStatusResponse, Request, Response,
    MSG_DIRECTOR_CHAT_HISTORY, MSG_DIRECTOR_CLEAR_HISTORY, MSG_DIRECTOR_SEND_MESSAGE,
    MSG_DIRECTOR_START, MSG_DIRECTOR_STATUS, MSG_DIRECTOR_STOP,
};
use tokio::sync::{mpsc, watch};

use super::super::prompts::build_director_system_prompt;
use super::super::{
    agent_info_from_record, cleanup_agent_runtime, emit_agent_chat_event, now_ms,
    persist_agents_runtime, spawn_claude_agent_process, AgentRuntime, SharedState,
    DEFAULT_CHAT_CAPACITY,
};
use super::error_response;

use crate::permissions;

/// The director agent ID (singleton).
pub const DIRECTOR_ID: &str = "director";

pub(in crate::daemon) async fn handle_director_start(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<DirectorStartRequest, _> = serde_json::from_value(payload);
    let start = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    // Determine backend - use the provided one or default to Claude
    let backend = match start.backend.as_deref() {
        Some("codex") => AgentBackend::Codex,
        _ => AgentBackend::Claude,
    };

    // Check if director already running
    if let Some(existing) = { shared.agents.lock().await.agents.remove(DIRECTOR_ID) } {
        if matches!(
            existing.record.state,
            murmur_core::agent::AgentState::Starting
                | murmur_core::agent::AgentState::Running
                | murmur_core::agent::AgentState::Idle
                | murmur_core::agent::AgentState::NeedsResolution
        ) {
            shared
                .agents
                .lock()
                .await
                .agents
                .insert(DIRECTOR_ID.to_owned(), existing);
            return error_response(req, "director already running");
        }

        if let Err(err) = cleanup_agent_runtime(shared.clone(), existing).await {
            return error_response(req, &format!("cleanup existing director failed: {err:#}"));
        }
    }

    // Create director working directory if it doesn't exist
    let director_dir = shared.paths.murmur_dir.join("director");
    if !director_dir.exists() {
        if let Err(err) = tokio::fs::create_dir_all(&director_dir).await {
            return error_response(req, &format!("create director dir failed: {err:#}"));
        }
    }

    // Get all projects for system prompt
    let projects = {
        let cfg = shared.config.lock().await;
        let project_cfgs = cfg.projects.clone();
        drop(cfg);

        let mut projects = Vec::new();
        for p in project_cfgs {
            let running =
                crate::daemon::orchestration::orchestrator_is_running(&shared, &p.name).await;
            projects.push(murmur_protocol::ProjectInfo {
                name: p.name.clone(),
                remote_url: p.remote_url.clone(),
                repo_dir: shared
                    .paths
                    .projects_dir
                    .join(&p.name)
                    .join("repo")
                    .to_string_lossy()
                    .to_string(),
                max_agents: p.max_agents,
                running,
                backend: format!("{:?}", p.effective_coding_backend()).to_ascii_lowercase(),
                user_intervening: false,
            });
        }
        projects
    };

    // Load allowed tools for Claude
    let allowed_tools = if backend == AgentBackend::Claude {
        let allowed_patterns =
            match permissions::load_director_allowed_patterns(&shared.paths).await {
                Ok(v) => v,
                Err(err) => {
                    return error_response(req, &format!("load director patterns failed: {err:#}"));
                }
            };
        allowed_patterns
            .iter()
            .filter_map(|p| pattern_to_claude_bash_tool(p))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let created_at_ms = now_ms();
    let mut record = AgentRecord::new(
        DIRECTOR_ID.to_owned(),
        String::new(), // No project for director
        AgentRole::Director,
        "director".to_owned(),
        created_at_ms,
        director_dir.to_string_lossy().to_string(),
    );

    let (outbound_tx, outbound_rx) = mpsc::channel::<ChatMessage>(32);
    let (abort_tx, abort_rx) = watch::channel(false);

    let mut pending_claude = None;
    if backend == AgentBackend::Claude {
        let system_prompt = build_director_system_prompt(&projects);
        let (child, stdin, stdout, pid) = match spawn_claude_agent_process(
            DIRECTOR_ID,
            "",
            &director_dir,
            &shared.paths.murmur_dir,
            &shared.paths.socket_path,
            Some(&allowed_tools),
            true,
            Some(&system_prompt),
        )
        .await
        {
            Ok(v) => v,
            Err(err) => {
                return error_response(req, &format!("spawn director failed: {err:#}"));
            }
        };
        record = record.apply_event(AgentEvent::Spawned { pid }, created_at_ms);
        pending_claude = Some((child, stdin, stdout));
    }

    // Store in agents map
    {
        let mut agents = shared.agents.lock().await;
        agents.agents.insert(
            DIRECTOR_ID.to_owned(),
            AgentRuntime {
                record: record.clone(),
                backend,
                codex_thread_id: None,
                chat: ChatHistory::new(DEFAULT_CHAT_CAPACITY),
                last_idle_at_ms: None,
                claim_started_at_ms: None,
                outbound_tx: outbound_tx.clone(),
                abort_tx,
                tasks: Vec::new(),
            },
        );
    }

    // Spawn background tasks
    let mut tasks = Vec::new();
    match backend {
        AgentBackend::Claude => {
            let Some((child, stdin, stdout)) = pending_claude else {
                return error_response(req, "claude process missing after spawn");
            };
            tasks.push(tokio::spawn(super::super::claude_stdin_writer(
                outbound_rx,
                stdin,
                abort_rx.clone(),
            )));
            tasks.push(tokio::spawn(super::super::claude_stdout_reader(
                shared.clone(),
                DIRECTOR_ID.to_owned(),
                stdout,
                abort_rx.clone(),
            )));
            tasks.push(tokio::spawn(super::super::claude_reaper(
                shared.clone(),
                DIRECTOR_ID.to_owned(),
                child,
                abort_rx.clone(),
            )));
        }
        AgentBackend::Codex => {
            tasks.push(tokio::spawn(super::super::codex_worker(
                shared.clone(),
                DIRECTOR_ID.to_owned(),
                director_dir.clone(),
                outbound_rx,
                abort_rx.clone(),
            )));
        }
    }

    // Store tasks
    {
        let mut agents = shared.agents.lock().await;
        if let Some(rt) = agents.agents.get_mut(DIRECTOR_ID) {
            rt.tasks = tasks;
        }
    }

    // Initial system message
    let sys = ChatMessage::new(
        ChatRole::System,
        "Waiting for messages...".to_owned(),
        now_ms(),
    );
    {
        let mut agents = shared.agents.lock().await;
        if let Some(rt) = agents.agents.get_mut(DIRECTOR_ID) {
            rt.chat.push(sys.clone());
        }
    }
    emit_agent_chat_event(shared.as_ref(), DIRECTOR_ID, "", sys);

    persist_agents_runtime(shared).await;

    let response_payload = DirectorStartResponse {
        id: DIRECTOR_ID.to_owned(),
    };

    Response {
        r#type: MSG_DIRECTOR_START.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(response_payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_director_stop(
    shared: Arc<SharedState>,
    req: Request,
) -> Response {
    let runtime = {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.remove(DIRECTOR_ID) else {
            return error_response(req, "director not running");
        };
        if rt.record.role != AgentRole::Director {
            agents.agents.insert(DIRECTOR_ID.to_owned(), rt);
            return error_response(req, "agent is not the director");
        }
        rt
    };

    if let Err(err) = cleanup_agent_runtime(shared.clone(), runtime).await {
        return error_response(req, &format!("cleanup director failed: {err:#}"));
    }

    persist_agents_runtime(shared).await;

    Response {
        r#type: MSG_DIRECTOR_STOP.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_director_status(
    shared: &SharedState,
    req: Request,
) -> Response {
    let director = {
        let agents = shared.agents.lock().await;
        agents
            .agents
            .get(DIRECTOR_ID)
            .filter(|rt| rt.record.role == AgentRole::Director)
            .map(|rt| agent_info_from_record(&rt.record, rt.backend))
    };

    let response = match director {
        Some(info) => DirectorStatusResponse {
            running: true,
            state: Some(info.state),
            backend: info.backend,
        },
        None => DirectorStatusResponse {
            running: false,
            state: None,
            backend: None,
        },
    };

    Response {
        r#type: MSG_DIRECTOR_STATUS.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_director_send_message(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<DirectorSendMessageRequest, _> = serde_json::from_value(payload);
    let send = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let now_ms = now_ms();
    let msg = ChatMessage::new(ChatRole::User, send.message, now_ms);

    let outbound_tx = {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get_mut(DIRECTOR_ID) else {
            return error_response(req, "director not running");
        };
        if rt.record.role != AgentRole::Director {
            return error_response(req, "agent is not the director");
        }
        if rt.record.state == murmur_core::agent::AgentState::Aborted {
            return error_response(req, "director is aborted");
        }

        rt.chat.push(msg.clone());
        rt.outbound_tx.clone()
    };

    emit_agent_chat_event(shared.as_ref(), DIRECTOR_ID, "", msg.clone());
    if outbound_tx.send(msg).await.is_err() {
        return error_response(req, "director channel closed");
    }

    Response {
        r#type: MSG_DIRECTOR_SEND_MESSAGE.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_director_chat_history(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<DirectorChatHistoryRequest, _> = serde_json::from_value(payload);
    let hist = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let limit = hist.limit.unwrap_or(50) as usize;

    let messages = {
        let agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get(DIRECTOR_ID) else {
            return error_response(req, "director not running");
        };
        if rt.record.role != AgentRole::Director {
            return error_response(req, "agent is not the director");
        }
        rt.chat.tail(limit)
    };

    let response = DirectorChatHistoryResponse {
        messages: messages
            .into_iter()
            .map(super::super::to_proto_chat_message)
            .collect(),
    };

    Response {
        r#type: MSG_DIRECTOR_CHAT_HISTORY.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_director_clear_history(
    shared: Arc<SharedState>,
    req: Request,
) -> Response {
    {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get_mut(DIRECTOR_ID) else {
            return error_response(req, "director not running");
        };
        if rt.record.role != AgentRole::Director {
            return error_response(req, "agent is not the director");
        }
        rt.chat = ChatHistory::new(DEFAULT_CHAT_CAPACITY);
    }

    persist_agents_runtime(shared).await;

    Response {
        r#type: MSG_DIRECTOR_CLEAR_HISTORY.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

fn pattern_to_claude_bash_tool(pattern: &str) -> Option<String> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return None;
    }

    if pattern == ":*" {
        return Some("Bash(*)".to_owned());
    }

    if let Some(prefix) = pattern.strip_suffix(":*") {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            return Some("Bash(*)".to_owned());
        }
        return Some(format!("Bash({prefix} *)"));
    }

    Some(format!("Bash({pattern})"))
}
