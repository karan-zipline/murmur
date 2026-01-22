use std::path::Path;
use std::sync::Arc;

use murmur_core::agent::{AgentEvent, AgentRecord, AgentRole, ChatHistory, ChatMessage, ChatRole};
use murmur_core::config::AgentBackend;
use murmur_protocol::{
    AgentInfo, PlanChatHistoryRequest, PlanChatHistoryResponse, PlanListRequest, PlanListResponse,
    PlanSendMessageRequest, PlanShowRequest, PlanShowResponse, PlanStartRequest, PlanStartResponse,
    PlanStopRequest, Request, Response, MSG_PLAN_CHAT_HISTORY, MSG_PLAN_LIST,
    MSG_PLAN_SEND_MESSAGE, MSG_PLAN_SHOW, MSG_PLAN_START, MSG_PLAN_STOP,
};
use tokio::sync::{mpsc, watch};

use super::super::{
    agent_info_from_record, cleanup_agent_runtime, emit_agent_chat_event, now_ms,
    persist_agents_runtime, spawn_claude_agent_process, to_proto_chat_message, SharedState,
    DEFAULT_CHAT_CAPACITY,
};
use super::error_response;

use crate::worktrees::WorktreeManager;

pub(in crate::daemon) async fn handle_plan_start(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<PlanStartRequest, _> = serde_json::from_value(payload);
    let start = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let prompt = start.prompt.trim();
    if prompt.is_empty() {
        return error_response(req, "prompt is required");
    }

    let project = start
        .project
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_default();

    if !project.is_empty() {
        let cfg = shared.config.lock().await;
        if cfg.project(&project).is_none() {
            return error_response(req, "project not found");
        }
    }

    let plan_num = shared
        .next_plan_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let plan_id = format!("plan-{plan_num}");

    let plan_path = match write_plan_stub(
        &shared,
        &plan_id,
        if project.is_empty() {
            "(none)"
        } else {
            &project
        },
        prompt,
    )
    .await
    {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("write plan file: {err:#}")),
    };

    let (wt, backend) = if !project.is_empty() {
        let wtm = WorktreeManager::new(&shared.git, &shared.paths);
        let wt = match wtm.create_agent_worktree(&project, &plan_id).await {
            Ok(v) => v,
            Err(err) => return error_response(req, &format!("create planner worktree: {err:#}")),
        };

        let backend = {
            let cfg = shared.config.lock().await;
            cfg.project(&project)
                .map(|p| p.effective_planner_backend())
                .unwrap_or_default()
        };

        (wt, backend)
    } else {
        let dir = shared.paths.murmur_dir.join("planners").join(&plan_id);
        if let Err(err) = tokio::fs::create_dir_all(&dir).await {
            return error_response(req, &format!("create planner dir: {err}"));
        }

        (
            crate::worktrees::Worktree {
                dir,
                branch: String::new(),
                base_branch: String::new(),
            },
            AgentBackend::Codex,
        )
    };

    let created_at_ms = now_ms();
    let mut record = AgentRecord::new(
        plan_id.clone(),
        project.clone(),
        AgentRole::Planner,
        plan_id.clone(),
        created_at_ms,
        wt.dir.to_string_lossy().to_string(),
    );

    let (outbound_tx, outbound_rx) = mpsc::channel::<ChatMessage>(32);
    let (abort_tx, abort_rx) = watch::channel(false);

    let mut pending_claude = None;
    if backend == AgentBackend::Claude {
        let (child, stdin, stdout, pid) = match spawn_claude_agent_process(
            &plan_id,
            &project,
            &wt.dir,
            &shared.paths.socket_path,
            None,
            false,
            None,
        )
        .await
        {
            Ok(v) => v,
            Err(err) => {
                if project.trim().is_empty() {
                    let _ = tokio::fs::remove_dir_all(&wt.dir).await;
                } else {
                    let wtm = WorktreeManager::new(&shared.git, &shared.paths);
                    let _ = wtm.remove_worktree(&project, &wt.dir).await;
                }
                return error_response(req, &format!("spawn planner: {err:#}"));
            }
        };

        record = record.apply_event(AgentEvent::Spawned { pid }, created_at_ms);
        pending_claude = Some((child, stdin, stdout));
    }

    {
        let mut agents = shared.agents.lock().await;
        agents.agents.insert(
            plan_id.clone(),
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
                plan_id.clone(),
                stdout,
                abort_rx.clone(),
            )));
            tasks.push(tokio::spawn(super::super::claude_reaper(
                shared.clone(),
                plan_id.clone(),
                child,
                abort_rx.clone(),
            )));
        }
        AgentBackend::Codex => {
            tasks.push(tokio::spawn(super::super::codex_worker(
                shared.clone(),
                plan_id.clone(),
                wt.dir.clone(),
                outbound_rx,
                abort_rx.clone(),
            )));
        }
    }

    {
        let mut agents = shared.agents.lock().await;
        if let Some(rt) = agents.agents.get_mut(&plan_id) {
            rt.tasks = tasks;
        }
    }

    let kickoff = build_planner_prompt(
        &plan_id,
        &project,
        plan_path.to_string_lossy().as_ref(),
        prompt,
    );
    send_kickoff_message(shared.clone(), &plan_id, &project, kickoff).await;

    persist_agents_runtime(shared).await;

    let payload = PlanStartResponse {
        id: plan_id,
        project,
        worktree_dir: wt.dir.to_string_lossy().to_string(),
        plan_path: plan_path.to_string_lossy().to_string(),
    };

    Response {
        r#type: MSG_PLAN_START.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_plan_stop(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<PlanStopRequest, _> = serde_json::from_value(payload);
    let stop = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let plan_id = stop.id.trim();
    if plan_id.is_empty() {
        return error_response(req, "id is required");
    }

    let runtime = {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.remove(plan_id) else {
            return error_response(req, "planner not found");
        };
        if rt.record.role != AgentRole::Planner {
            agents.agents.insert(plan_id.to_owned(), rt);
            return error_response(req, "agent is not a planner");
        }
        rt
    };

    if let Err(err) = cleanup_agent_runtime(shared.clone(), runtime).await {
        return error_response(req, &format!("cleanup planner failed: {err:#}"));
    }

    persist_agents_runtime(shared).await;

    Response {
        r#type: MSG_PLAN_STOP.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_plan_list(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<PlanListRequest, _> = serde_json::from_value(payload);
    let list: PlanListRequest = parsed.unwrap_or_default();

    let project_filter = list
        .project
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let agents = shared.agents.lock().await;
    let mut plans = agents
        .agents
        .values()
        .filter(|a| a.record.role == AgentRole::Planner)
        .filter(|a| match project_filter {
            Some(p) => a.record.project == p,
            None => true,
        })
        .map(|a| agent_info_from_record(&a.record, a.backend))
        .collect::<Vec<AgentInfo>>();
    plans.sort_by(|a, b| a.id.cmp(&b.id));

    let payload = PlanListResponse { plans };

    Response {
        r#type: MSG_PLAN_LIST.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_plan_send_message(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<PlanSendMessageRequest, _> = serde_json::from_value(payload);
    let send = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let plan_id = send.id.trim();
    if plan_id.is_empty() {
        return error_response(req, "id is required");
    }

    let now_ms = now_ms();
    let msg = ChatMessage::new(ChatRole::User, send.message, now_ms);

    let (outbound_tx, project) = {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get_mut(plan_id) else {
            return error_response(req, "planner not found");
        };
        if rt.record.role != AgentRole::Planner {
            return error_response(req, "agent is not a planner");
        }
        if rt.record.state == murmur_core::agent::AgentState::Aborted {
            return error_response(req, "planner is aborted");
        }
        if rt.backend == AgentBackend::Claude
            && !matches!(
                rt.record.state,
                murmur_core::agent::AgentState::Running
                    | murmur_core::agent::AgentState::NeedsResolution
            )
        {
            return error_response(req, "planner is not running");
        }

        rt.chat.push(msg.clone());
        (rt.outbound_tx.clone(), rt.record.project.clone())
    };

    emit_agent_chat_event(shared.as_ref(), plan_id, &project, msg.clone());
    if outbound_tx.send(msg).await.is_err() {
        return error_response(req, "planner channel closed");
    }

    Response {
        r#type: MSG_PLAN_SEND_MESSAGE.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_plan_chat_history(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<PlanChatHistoryRequest, _> = serde_json::from_value(payload);
    let hist = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let plan_id = hist.id.trim();
    if plan_id.is_empty() {
        return error_response(req, "id is required");
    }
    let limit = hist.limit.unwrap_or(50) as usize;

    let messages = {
        let agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get(plan_id) else {
            return error_response(req, "planner not found");
        };
        if rt.record.role != AgentRole::Planner {
            return error_response(req, "agent is not a planner");
        }
        rt.chat.tail(limit)
    };

    let payload = PlanChatHistoryResponse {
        id: plan_id.to_owned(),
        messages: messages.into_iter().map(to_proto_chat_message).collect(),
    };

    Response {
        r#type: MSG_PLAN_CHAT_HISTORY.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_plan_show(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<PlanShowRequest, _> = serde_json::from_value(payload);
    let show = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let plan_id = show.id.trim();
    if plan_id.is_empty() {
        return error_response(req, "id is required");
    }

    let path = match plan_file_path(&shared.paths.plans_dir, plan_id) {
        Ok(v) => v,
        Err(err) => return error_response(req, &err.to_string()),
    };

    let contents = match tokio::fs::read_to_string(&path).await {
        Ok(v) => v,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return error_response(req, "plan not found");
        }
        Err(err) => return error_response(req, &format!("read plan file: {err}")),
    };

    let payload = PlanShowResponse {
        id: plan_id.to_owned(),
        contents,
    };

    Response {
        r#type: MSG_PLAN_SHOW.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

async fn write_plan_stub(
    shared: &SharedState,
    plan_id: &str,
    project: &str,
    prompt: &str,
) -> anyhow::Result<std::path::PathBuf> {
    tokio::fs::create_dir_all(&shared.paths.plans_dir).await?;

    let dest = plan_file_path(&shared.paths.plans_dir, plan_id)?;
    let tmp = plan_file_path(&shared.paths.plans_dir, &format!("{plan_id}.tmp"))?;

    let contents = format!("# Plan: {plan_id}\n\nProject: {project}\n\n## Prompt\n\n{prompt}\n");

    tokio::fs::write(&tmp, contents).await?;
    tokio::fs::rename(&tmp, &dest).await?;
    Ok(dest)
}

fn plan_file_path(plans_dir: &Path, plan_id: &str) -> anyhow::Result<std::path::PathBuf> {
    let name = if plan_id.ends_with(".md") {
        plan_id.to_owned()
    } else {
        format!("{plan_id}.md")
    };

    let path = murmur_core::paths::safe_join(plans_dir, &name)
        .map_err(|e| anyhow::anyhow!("invalid plan id {plan_id:?}: {e}"))?;
    Ok(path)
}

fn build_planner_prompt(plan_id: &str, project: &str, plan_path: &str, prompt: &str) -> String {
    let target = if project.trim().is_empty() {
        "the codebase".to_owned()
    } else {
        format!("the \"{project}\" codebase")
    };
    format!(
        r#"You are a planning agent for Murmur. Your job is to explore {target} and produce an implementation plan.

## Task

{prompt}

## Output artifact

Write your final plan to:

{plan_path}

Use Markdown. Keep it actionable and broken down into small, committable tickets.

Plan ID: {plan_id}
"#
    )
}

async fn send_kickoff_message(
    shared: Arc<SharedState>,
    agent_id: &str,
    project: &str,
    kickoff: String,
) {
    let msg = ChatMessage::new(ChatRole::User, kickoff, now_ms());

    let outbound = {
        let mut agents = shared.agents.lock().await;
        if let Some(rt) = agents.agents.get_mut(agent_id) {
            rt.chat.push(msg.clone());
            Some(rt.outbound_tx.clone())
        } else {
            None
        }
    };

    if let Some(tx) = outbound {
        emit_agent_chat_event(shared.as_ref(), agent_id, project, msg.clone());
        let _ = tx.send(msg).await;
    }
}
