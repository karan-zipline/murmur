use std::sync::Arc;

use murmur_core::agent::{AgentEvent, AgentRecord, AgentRole, ChatHistory, ChatMessage, ChatRole};
use murmur_core::config::AgentBackend;
use murmur_protocol::{
    ManagerChatHistoryRequest, ManagerChatHistoryResponse, ManagerClearHistoryRequest,
    ManagerSendMessageRequest, ManagerStartRequest, ManagerStatusRequest, ManagerStatusResponse,
    ManagerStopRequest, Request, Response, MSG_MANAGER_CHAT_HISTORY, MSG_MANAGER_CLEAR_HISTORY,
    MSG_MANAGER_SEND_MESSAGE, MSG_MANAGER_START, MSG_MANAGER_STATUS, MSG_MANAGER_STOP,
};
use tokio::sync::{mpsc, watch};

use super::super::prompts::build_manager_prompt;
use super::super::{
    agent_info_from_record, cleanup_agent_runtime, emit_agent_chat_event, now_ms,
    persist_agents_runtime, spawn_claude_agent_process, SharedState, DEFAULT_CHAT_CAPACITY,
};
use super::error_response;

use crate::permissions;
use crate::worktrees::WorktreeManager;

pub(in crate::daemon) async fn handle_manager_start(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ManagerStartRequest, _> = serde_json::from_value(payload);
    let start = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let project = start.project.trim();
    if project.is_empty() {
        return error_response(req, "project is required");
    }

    let backend = {
        let cfg = shared.config.lock().await;
        let Some(p) = cfg.project(project) else {
            return error_response(req, "project not found");
        };
        p.agent_backend
    };

    let manager_id = manager_agent_id(project);

    if let Some(existing) = { shared.agents.lock().await.agents.remove(&manager_id) } {
        if matches!(
            existing.record.state,
            murmur_core::agent::AgentState::Starting
                | murmur_core::agent::AgentState::Running
                | murmur_core::agent::AgentState::NeedsResolution
        ) {
            shared
                .agents
                .lock()
                .await
                .agents
                .insert(manager_id.clone(), existing);
            return error_response(req, "manager already running");
        }

        if let Err(err) = cleanup_agent_runtime(shared.clone(), existing).await {
            return error_response(req, &format!("cleanup existing manager failed: {err:#}"));
        }
    }

    let allowed_tools = if backend == AgentBackend::Claude {
        let allowed_patterns = match permissions::load_manager_allowed_patterns(&shared.paths).await
        {
            Ok(v) => v,
            Err(err) => {
                return error_response(req, &format!("load manager patterns failed: {err:#}"));
            }
        };
        allowed_patterns
            .iter()
            .filter_map(|p| pattern_to_claude_bash_tool(p))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let wtm = WorktreeManager::new(&shared.git, &shared.paths);

    // Clean up orphaned worktree if it exists but manager isn't registered in memory.
    // This can happen after daemon restart when the worktree wasn't cleaned up.
    let orphan_wt_dir = wtm
        .project_worktrees_dir(project)
        .join(format!("wt-{manager_id}"));
    if orphan_wt_dir.exists() {
        tracing::info!(
            manager_id = %manager_id,
            worktree = %orphan_wt_dir.display(),
            "cleaning up orphaned manager worktree"
        );
        if let Err(err) = wtm.remove_worktree(project, &orphan_wt_dir).await {
            tracing::warn!(
                manager_id = %manager_id,
                error = %err,
                "failed to remove orphaned worktree via git, attempting force removal"
            );
            // Force remove the directory if git worktree remove fails
            if let Err(rm_err) = tokio::fs::remove_dir_all(&orphan_wt_dir).await {
                return error_response(
                    req,
                    &format!("cleanup orphaned worktree failed: {rm_err:#}"),
                );
            }
        }
    }

    let wt = match wtm.create_agent_worktree(project, &manager_id).await {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("create manager worktree: {err:#}")),
    };

    let created_at_ms = now_ms();
    let mut record = AgentRecord::new(
        manager_id.clone(),
        project.to_owned(),
        AgentRole::Manager,
        "manager".to_owned(),
        created_at_ms,
        wt.dir.to_string_lossy().to_string(),
    );

    let (outbound_tx, outbound_rx) = mpsc::channel::<ChatMessage>(32);
    let (abort_tx, abort_rx) = watch::channel(false);

    let mut pending_claude = None;
    if backend == AgentBackend::Claude {
        let system_prompt = build_manager_prompt(project);
        let (child, stdin, stdout, pid) = match spawn_claude_agent_process(
            &manager_id,
            project,
            &wt.dir,
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
                let _ = wtm.remove_worktree(project, &wt.dir).await;
                return error_response(req, &format!("spawn manager failed: {err:#}"));
            }
        };
        record = record.apply_event(AgentEvent::Spawned { pid }, created_at_ms);
        pending_claude = Some((child, stdin, stdout));
    }

    {
        let mut agents = shared.agents.lock().await;
        agents.agents.insert(
            manager_id.clone(),
            super::super::AgentRuntime {
                record: record.clone(),
                backend,
                codex_thread_id: None,
                chat: ChatHistory::new(DEFAULT_CHAT_CAPACITY),
                last_idle_at_ms: None,
                outbound_tx: outbound_tx.clone(),
                abort_tx,
                tasks: Vec::new(),
            },
        );
    }

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
                manager_id.clone(),
                stdout,
                abort_rx.clone(),
            )));
            tasks.push(tokio::spawn(super::super::claude_reaper(
                shared.clone(),
                manager_id.clone(),
                child,
                abort_rx.clone(),
            )));
        }
        AgentBackend::Codex => {
            tasks.push(tokio::spawn(super::super::codex_worker(
                shared.clone(),
                manager_id.clone(),
                wt.dir.clone(),
                outbound_rx,
                abort_rx.clone(),
            )));
        }
    }

    {
        let mut agents = shared.agents.lock().await;
        if let Some(rt) = agents.agents.get_mut(&manager_id) {
            rt.tasks = tasks;
        }
    }

    let sys = ChatMessage::new(
        ChatRole::System,
        "Waiting for messages...".to_owned(),
        now_ms(),
    );
    {
        let mut agents = shared.agents.lock().await;
        if let Some(rt) = agents.agents.get_mut(&manager_id) {
            rt.chat.push(sys.clone());
        }
    }
    emit_agent_chat_event(shared.as_ref(), &manager_id, project, sys);

    persist_agents_runtime(shared).await;

    Response {
        r#type: MSG_MANAGER_START.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_manager_stop(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ManagerStopRequest, _> = serde_json::from_value(payload);
    let stop = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let project = stop.project.trim();
    if project.is_empty() {
        return error_response(req, "project is required");
    }

    let manager_id = manager_agent_id(project);

    let runtime = {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.remove(&manager_id) else {
            return error_response(req, "manager not found");
        };
        if rt.record.role != AgentRole::Manager {
            agents.agents.insert(manager_id.clone(), rt);
            return error_response(req, "agent is not a manager");
        }
        rt
    };

    if let Err(err) = cleanup_agent_runtime(shared.clone(), runtime).await {
        return error_response(req, &format!("cleanup manager failed: {err:#}"));
    }

    persist_agents_runtime(shared).await;

    Response {
        r#type: MSG_MANAGER_STOP.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_manager_status(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ManagerStatusRequest, _> = serde_json::from_value(payload);
    let status = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let project = status.project.trim();
    if project.is_empty() {
        return error_response(req, "project is required");
    }

    let manager_id = manager_agent_id(project);

    let manager = {
        let agents = shared.agents.lock().await;
        agents
            .agents
            .get(&manager_id)
            .filter(|rt| rt.record.role == AgentRole::Manager)
            .map(|rt| agent_info_from_record(&rt.record, rt.backend))
    };

    let payload = ManagerStatusResponse {
        project: project.to_owned(),
        manager,
    };

    Response {
        r#type: MSG_MANAGER_STATUS.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_manager_send_message(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ManagerSendMessageRequest, _> = serde_json::from_value(payload);
    let send = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let project = send.project.trim();
    if project.is_empty() {
        return error_response(req, "project is required");
    }

    let manager_id = manager_agent_id(project);

    let now_ms = now_ms();
    let msg = ChatMessage::new(ChatRole::User, send.message, now_ms);

    let outbound_tx = {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get_mut(&manager_id) else {
            return error_response(req, "manager not found");
        };
        if rt.record.role != AgentRole::Manager {
            return error_response(req, "agent is not a manager");
        }
        if rt.record.state == murmur_core::agent::AgentState::Aborted {
            return error_response(req, "manager is aborted");
        }

        rt.chat.push(msg.clone());
        rt.outbound_tx.clone()
    };

    emit_agent_chat_event(shared.as_ref(), &manager_id, project, msg.clone());
    if outbound_tx.send(msg).await.is_err() {
        return error_response(req, "manager channel closed");
    }

    Response {
        r#type: MSG_MANAGER_SEND_MESSAGE.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_manager_chat_history(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ManagerChatHistoryRequest, _> = serde_json::from_value(payload);
    let hist = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let project = hist.project.trim();
    if project.is_empty() {
        return error_response(req, "project is required");
    }

    let manager_id = manager_agent_id(project);
    let limit = hist.limit.unwrap_or(50) as usize;

    let messages = {
        let agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get(&manager_id) else {
            return error_response(req, "manager not found");
        };
        if rt.record.role != AgentRole::Manager {
            return error_response(req, "agent is not a manager");
        }
        rt.chat.tail(limit)
    };

    let payload = ManagerChatHistoryResponse {
        project: project.to_owned(),
        messages: messages
            .into_iter()
            .map(super::super::to_proto_chat_message)
            .collect(),
    };

    Response {
        r#type: MSG_MANAGER_CHAT_HISTORY.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_manager_clear_history(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ManagerClearHistoryRequest, _> = serde_json::from_value(payload);
    let clear = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let project = clear.project.trim();
    if project.is_empty() {
        return error_response(req, "project is required");
    }

    let manager_id = manager_agent_id(project);

    {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get_mut(&manager_id) else {
            return error_response(req, "manager not found");
        };
        if rt.record.role != AgentRole::Manager {
            return error_response(req, "agent is not a manager");
        }
        rt.chat = ChatHistory::new(DEFAULT_CHAT_CAPACITY);
    }

    persist_agents_runtime(shared).await;

    Response {
        r#type: MSG_MANAGER_CLEAR_HISTORY.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

fn manager_agent_id(project: &str) -> String {
    format!("manager-{project}")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_patterns_to_claude_bash_tools() {
        let cases = [
            ("", None),
            (":*", Some("Bash(*)")),
            ("mm:*", Some("Bash(mm *)")),
            ("git :*", Some("Bash(git *)")),
            ("rm :*", Some("Bash(rm *)")),
            ("prefix:*", Some("Bash(prefix *)")),
            ("prefix:*  ", Some("Bash(prefix *)")),
        ];

        for (pattern, want) in cases {
            let got = pattern_to_claude_bash_tool(pattern);
            assert_eq!(got.as_deref(), want, "pattern={pattern:?}");
        }
    }
}
