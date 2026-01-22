use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use murmur_core::agent::{AgentEvent, AgentState, ChatMessage, ChatRole};
use murmur_core::commits::{CommitLog, CommitRecord as CoreCommitRecord};
use murmur_core::config::{AgentBackend, MergeStrategy};
use murmur_protocol::{
    AgentAbortRequest, AgentChatHistoryRequest, AgentChatHistoryResponse, AgentClaimRequest,
    AgentCreateRequest, AgentCreateResponse, AgentDeleteRequest, AgentDescribeRequest,
    AgentDoneRequest, AgentIdleRequest, AgentListResponse, AgentSendMessageRequest, Event, Request,
    Response, EVT_AGENT_IDLE, MSG_AGENT_ABORT, MSG_AGENT_CHAT_HISTORY, MSG_AGENT_CLAIM,
    MSG_AGENT_CREATE, MSG_AGENT_DELETE, MSG_AGENT_DESCRIBE, MSG_AGENT_DONE, MSG_AGENT_IDLE,
    MSG_AGENT_LIST, MSG_AGENT_SEND_MESSAGE,
};

use crate::github::{parse_github_nwo, GithubBackend};
use crate::providers;

use super::super::merge::{
    merge_agent_branch_direct, merge_lock_for_project, prepare_agent_branch_pull_request,
    MergeAttempt, PullRequestAttempt,
};
use super::super::{
    agent_info_from_record, cleanup_agent_runtime, emit_agent_chat_event,
    issue_backend_for_project, mark_issue_completed, now_ms, persist_agents_runtime,
    release_claims_for_agent, spawn_agent, stop_agent_runtime_keep_worktree, to_proto_chat_message,
    SharedState,
};
use super::error_response;

pub(in crate::daemon) async fn handle_agent_create(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<AgentCreateRequest, _> = serde_json::from_value(payload);
    let create = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    {
        let cfg = shared.config.lock().await;
        if cfg.project(&create.project).is_none() {
            return error_response(req, "project not found");
        }
    }

    let backend_override = match create
        .backend
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some("claude") => Some(AgentBackend::Claude),
        Some("codex") => Some(AgentBackend::Codex),
        Some(other) => return error_response(req, &format!("unknown backend: {other}")),
        None => None,
    };

    let record = match spawn_agent(
        shared.clone(),
        create.project,
        create.issue_id,
        backend_override,
    )
    .await
    {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("{err:#}")),
    };

    let backend = {
        let agents = shared.agents.lock().await;
        agents
            .agents
            .get(&record.id)
            .map(|rt| rt.backend)
            .unwrap_or_default()
    };

    let payload = AgentCreateResponse {
        agent: agent_info_from_record(&record, backend),
    };

    Response {
        r#type: MSG_AGENT_CREATE.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_agent_list(shared: &SharedState, req: Request) -> Response {
    let agents = shared.agents.lock().await;
    let mut infos = agents
        .agents
        .values()
        .map(|a| agent_info_from_record(&a.record, a.backend))
        .collect::<Vec<_>>();
    infos.sort_by(|a, b| a.id.cmp(&b.id));

    let payload = AgentListResponse { agents: infos };

    Response {
        r#type: MSG_AGENT_LIST.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_agent_abort(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<AgentAbortRequest, _> = serde_json::from_value(payload);
    let abort = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let now_ms = now_ms();
    let mut quit_msg: Option<ChatMessage> = None;

    // Acquire both locks in consistent order (agents first, then claims) to prevent deadlocks
    // and hold them together to ensure atomic abort + claim release.
    let (abort_tx, outbound_tx, project) = {
        let mut agents = shared.agents.lock().await;
        let mut claims = shared.claims.lock().await;

        let Some(rt) = agents.agents.get_mut(&abort.agent_id) else {
            return error_response(req, "agent not found");
        };

        if !abort.force {
            let msg = ChatMessage::new(ChatRole::User, "/quit".to_owned(), now_ms);
            rt.chat.push(msg.clone());
            quit_msg = Some(msg);
        }

        rt.record = rt
            .record
            .apply_event(AgentEvent::Aborted { by: "user" }, now_ms);

        // Release claims while still holding the agents lock to prevent race condition
        // where a claim could be created between setting Aborted state and releasing claims.
        let released = claims.release_by_agent(&abort.agent_id);
        *claims = released;

        (
            rt.abort_tx.clone(),
            rt.outbound_tx.clone(),
            rt.record.project.clone(),
        )
    };

    if let Some(msg) = quit_msg {
        emit_agent_chat_event(shared.as_ref(), &abort.agent_id, &project, msg.clone());
        let _ = outbound_tx.send(msg).await;

        let abort_tx = abort_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let _ = abort_tx.send(true);
        });
    } else {
        let _ = abort_tx.send(true);
    }
    persist_agents_runtime(shared.clone()).await;

    Response {
        r#type: MSG_AGENT_ABORT.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_agent_delete(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<AgentDeleteRequest, _> = serde_json::from_value(payload);
    let delete = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let runtime = {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.remove(&delete.agent_id) else {
            return error_response(req, "agent not found");
        };
        rt
    };

    if let Err(err) = cleanup_agent_runtime(shared.clone(), runtime).await {
        return error_response(req, &format!("cleanup agent failed: {err:#}"));
    }

    release_claims_for_agent(&shared, &delete.agent_id).await;
    persist_agents_runtime(shared.clone()).await;

    Response {
        r#type: MSG_AGENT_DELETE.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_agent_send_message(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<AgentSendMessageRequest, _> = serde_json::from_value(payload);
    let send = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let now_ms = now_ms();
    let msg = ChatMessage::new(ChatRole::User, send.message.clone(), now_ms);

    let (outbound_tx, project) = {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get_mut(&send.agent_id) else {
            return error_response(req, "agent not found");
        };
        if rt.record.state == murmur_core::agent::AgentState::Aborted {
            return error_response(req, "agent is aborted");
        }
        if rt.backend == AgentBackend::Claude
            && !matches!(
                rt.record.state,
                murmur_core::agent::AgentState::Running
                    | murmur_core::agent::AgentState::NeedsResolution
            )
        {
            return error_response(req, "agent is not running");
        }

        rt.chat.push(msg.clone());
        (rt.outbound_tx.clone(), rt.record.project.clone())
    };

    emit_agent_chat_event(shared.as_ref(), &send.agent_id, &project, msg.clone());

    if outbound_tx.send(msg).await.is_err() {
        return error_response(req, "agent channel closed");
    }

    Response {
        r#type: MSG_AGENT_SEND_MESSAGE.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_agent_claim(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<AgentClaimRequest, _> = serde_json::from_value(payload);
    let claim = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let agent_id = claim.agent_id.trim();
    if agent_id.is_empty() {
        return error_response(req, "agent_id is required");
    }
    let issue_id = claim.issue_id.trim();
    if issue_id.is_empty() {
        return error_response(req, "issue_id is required");
    }

    let now_ms = now_ms();

    // Acquire both locks in a consistent order (agents first, then claims) to prevent deadlocks.
    // Hold both locks together to prevent race conditions with abort.
    let mut agents = shared.agents.lock().await;
    let mut claims = shared.claims.lock().await;

    let Some(rt) = agents.agents.get_mut(agent_id) else {
        return error_response(req, "agent not found");
    };
    if rt.record.role != murmur_core::agent::AgentRole::Coding {
        return error_response(req, "agent is not a coding agent");
    }
    if rt.record.state == AgentState::Aborted {
        return error_response(req, "agent is aborted");
    }

    let project = rt.record.project.clone();

    if let Some(existing) = claims.agent_for(&project, issue_id) {
        if existing != agent_id {
            return error_response(req, "issue already claimed");
        }
        // Already claimed by this agent, just update the record
        rt.record = rt
            .record
            .apply_event(AgentEvent::AssignedIssue { issue_id }, now_ms);
        drop(claims);
        drop(agents);
        persist_agents_runtime(shared.clone()).await;
        return Response {
            r#type: MSG_AGENT_CLAIM.to_owned(),
            id: req.id,
            success: true,
            error: None,
            payload: serde_json::Value::Null,
        };
    }

    let next = claims
        .release_by_agent(agent_id)
        .claim(&project, issue_id, agent_id);
    *claims = match next {
        Ok(v) => v,
        Err(_) => return error_response(req, "issue already claimed"),
    };

    rt.record = rt
        .record
        .apply_event(AgentEvent::AssignedIssue { issue_id }, now_ms);

    drop(claims);
    drop(agents);

    persist_agents_runtime(shared.clone()).await;

    Response {
        r#type: MSG_AGENT_CLAIM.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_agent_describe(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<AgentDescribeRequest, _> = serde_json::from_value(payload);
    let describe = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let agent_id = describe.agent_id.trim();
    if agent_id.is_empty() {
        return error_response(req, "agent_id is required");
    }

    let now_ms = now_ms();
    {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get_mut(agent_id) else {
            return error_response(req, "agent not found");
        };
        rt.record = rt.record.apply_event(
            AgentEvent::Described {
                description: &describe.description,
            },
            now_ms,
        );
    }

    persist_agents_runtime(shared.clone()).await;

    Response {
        r#type: MSG_AGENT_DESCRIBE.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_agent_chat_history(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<AgentChatHistoryRequest, _> = serde_json::from_value(payload);
    let hist = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let limit = hist.limit.unwrap_or(50) as usize;

    let messages = {
        let agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get(&hist.agent_id) else {
            return error_response(req, "agent not found");
        };
        rt.chat.tail(limit)
    };

    let payload = AgentChatHistoryResponse {
        agent_id: hist.agent_id,
        messages: messages.into_iter().map(to_proto_chat_message).collect(),
    };

    Response {
        r#type: MSG_AGENT_CHAT_HISTORY.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_agent_done(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<AgentDoneRequest, _> = serde_json::from_value(payload);
    let done = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let raw_id = done.agent_id.trim();
    if raw_id.is_empty() {
        return error_response(req, "agent_id is required");
    }
    let agent_id = raw_id.trim_start_matches("plan:").to_owned();

    let (project, issue_id, worktree_dir, role) = {
        let agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get(&agent_id) else {
            return error_response(req, "agent not found");
        };
        (
            rt.record.project.clone(),
            rt.record.issue_id.clone(),
            rt.record.worktree_dir.clone(),
            rt.record.role,
        )
    };

    if role == murmur_core::agent::AgentRole::Planner {
        let runtime = {
            let mut agents = shared.agents.lock().await;
            let Some(rt) = agents.agents.remove(&agent_id) else {
                return error_response(req, "planner not found");
            };
            rt
        };

        if let Err(err) = cleanup_agent_runtime(shared.clone(), runtime).await {
            return error_response(req, &format!("cleanup planner failed: {err:#}"));
        }

        persist_agents_runtime(shared.clone()).await;

        return Response {
            r#type: MSG_AGENT_DONE.to_owned(),
            id: req.id,
            success: true,
            error: None,
            payload: serde_json::Value::Null,
        };
    }

    if role != murmur_core::agent::AgentRole::Coding {
        return error_response(req, "agent.done only supports coding and planner agents");
    }

    let merge_strategy = {
        let cfg = shared.config.lock().await;
        cfg.project(&project)
            .map(|p| p.merge_strategy)
            .unwrap_or(MergeStrategy::Direct)
    };

    let lock = merge_lock_for_project(shared.as_ref(), &project).await;
    let _guard = lock.lock().await;

    match merge_strategy {
        MergeStrategy::Direct => {
            let attempt = match merge_agent_branch_direct(
                shared.as_ref(),
                &project,
                &agent_id,
                Path::new(&worktree_dir),
            )
            .await
            {
                Ok(v) => v,
                Err(err) => return error_response(req, &format!("merge failed: {err:#}")),
            };

            let merged = match attempt {
                MergeAttempt::Conflict { branch, error } => {
                    let now_ms = now_ms();
                    let msg = ChatMessage::new(
                        ChatRole::System,
                        format!("merge conflict on {branch}: {error}"),
                        now_ms,
                    );

                    {
                        let mut agents = shared.agents.lock().await;
                        if let Some(rt) = agents.agents.get_mut(&agent_id) {
                            rt.record = rt.record.apply_event(
                                AgentEvent::NeedsResolution {
                                    reason: "merge conflict",
                                },
                                now_ms,
                            );
                            rt.chat.push(msg.clone());
                        }
                    }

                    emit_agent_chat_event(shared.as_ref(), &agent_id, &project, msg);
                    persist_agents_runtime(shared.clone()).await;
                    return error_response(req, "merge conflict (agent needs resolution)");
                }
                MergeAttempt::Merged(merged) => merged,
            };

            let backend = match issue_backend_for_project(shared.as_ref(), &project).await {
                Ok(v) => v,
                Err(msg) => {
                    let now_ms = now_ms();
                    let msg = ChatMessage::new(
                        ChatRole::System,
                        format!("merge succeeded but issue backend error: {msg}"),
                        now_ms,
                    );
                    {
                        let mut agents = shared.agents.lock().await;
                        if let Some(rt) = agents.agents.get_mut(&agent_id) {
                            rt.record = rt.record.apply_event(
                                AgentEvent::NeedsResolution {
                                    reason: "issue backend",
                                },
                                now_ms,
                            );
                            rt.chat.push(msg.clone());
                        }
                    }
                    emit_agent_chat_event(shared.as_ref(), &agent_id, &project, msg);
                    persist_agents_runtime(shared.clone()).await;
                    return error_response(req, "merge succeeded but could not load issue backend");
                }
            };

            if let Err(err) = backend.close(now_ms(), &issue_id).await {
                let now_ms = now_ms();
                let msg = ChatMessage::new(
                    ChatRole::System,
                    format!("merge succeeded but failed to close issue {issue_id}: {err:#}"),
                    now_ms,
                );
                {
                    let mut agents = shared.agents.lock().await;
                    if let Some(rt) = agents.agents.get_mut(&agent_id) {
                        rt.record = rt.record.apply_event(
                            AgentEvent::NeedsResolution {
                                reason: "issue close",
                            },
                            now_ms,
                        );
                        rt.chat.push(msg.clone());
                    }
                }
                emit_agent_chat_event(shared.as_ref(), &agent_id, &project, msg);
                persist_agents_runtime(shared.clone()).await;
                return error_response(req, "merge succeeded but issue close failed");
            }

            if let Err(err) = backend.commit("issue: update tickets").await {
                let now_ms = now_ms();
                let msg = ChatMessage::new(
                    ChatRole::System,
                    format!("issue close succeeded but ticket commit failed: {err:#}"),
                    now_ms,
                );
                {
                    let mut agents = shared.agents.lock().await;
                    if let Some(rt) = agents.agents.get_mut(&agent_id) {
                        rt.record = rt.record.apply_event(
                            AgentEvent::NeedsResolution {
                                reason: "issue commit",
                            },
                            now_ms,
                        );
                        rt.chat.push(msg.clone());
                    }
                }
                emit_agent_chat_event(shared.as_ref(), &agent_id, &project, msg);
                persist_agents_runtime(shared.clone()).await;
                return error_response(req, "issue close succeeded but ticket commit failed");
            }

            {
                let mut commits = shared.commits.lock().await;
                commits
                    .entry(project.clone())
                    .or_insert_with(CommitLog::default)
                    .add(CoreCommitRecord {
                        sha: merged.sha.clone(),
                        branch: merged.branch.clone(),
                        agent_id: agent_id.clone(),
                        issue_id: issue_id.clone(),
                        merged_at_ms: now_ms(),
                    });
            }

            let runtime = {
                let mut agents = shared.agents.lock().await;
                let Some(rt) = agents.agents.remove(&agent_id) else {
                    return error_response(req, "agent not found");
                };
                rt
            };

            if let Err(err) = cleanup_agent_runtime(shared.clone(), runtime).await {
                return error_response(req, &format!("cleanup agent failed: {err:#}"));
            }

            mark_issue_completed(&shared, &project, &issue_id).await;
            release_claims_for_agent(&shared, &agent_id).await;
            persist_agents_runtime(shared.clone()).await;

            Response {
                r#type: MSG_AGENT_DONE.to_owned(),
                id: req.id,
                success: true,
                error: None,
                payload: serde_json::Value::Null,
            }
        }
        MergeStrategy::PullRequest => {
            let pr_attempt = match prepare_agent_branch_pull_request(
                shared.as_ref(),
                &project,
                &agent_id,
                Path::new(&worktree_dir),
            )
            .await
            {
                Ok(v) => v,
                Err(err) => {
                    return error_response(req, &format!("prepare pull request failed: {err:#}"))
                }
            };

            let prep = match pr_attempt {
                PullRequestAttempt::Conflict { branch, error } => {
                    let now_ms = now_ms();
                    let msg = ChatMessage::new(
                        ChatRole::System,
                        format!("rebase conflict on {branch}: {error}"),
                        now_ms,
                    );

                    {
                        let mut agents = shared.agents.lock().await;
                        if let Some(rt) = agents.agents.get_mut(&agent_id) {
                            rt.record = rt.record.apply_event(
                                AgentEvent::NeedsResolution {
                                    reason: "rebase conflict",
                                },
                                now_ms,
                            );
                            rt.chat.push(msg.clone());
                        }
                    }

                    emit_agent_chat_event(shared.as_ref(), &agent_id, &project, msg);
                    persist_agents_runtime(shared.clone()).await;
                    return error_response(req, "rebase conflict (agent needs resolution)");
                }
                PullRequestAttempt::Ready(prep) => prep,
            };

            let agent_description = {
                let agents = shared.agents.lock().await;
                agents
                    .agents
                    .get(&agent_id)
                    .and_then(|rt| rt.record.description.clone())
                    .unwrap_or_default()
            };

            let pr_title = if agent_description.trim().is_empty() {
                format!("Agent {agent_id} changes")
            } else {
                agent_description.trim().to_owned()
            };

            let pr_body = if issue_id.trim().parse::<i64>().is_ok() {
                format!("Closes #{issue_id}\n\nChanges from agent {agent_id}")
            } else {
                format!("Changes from agent {agent_id}")
            };

            let (token, graphql_url, allowed_authors, remote_url) = {
                let cfg = shared.config.lock().await;
                let token = providers::github_token(&cfg).ok_or_else(|| {
                    anyhow::anyhow!(
                        "github token not set (set GITHUB_TOKEN/GH_TOKEN or [providers.github].token)"
                    )
                });
                let graphql_url = providers::github_graphql_url(&cfg);
                let project_cfg = cfg.project(&project).cloned();
                let allowed_authors = project_cfg
                    .as_ref()
                    .map(|p| p.allowed_authors.clone())
                    .unwrap_or_default();
                let remote_url = project_cfg
                    .as_ref()
                    .map(|p| p.remote_url.clone())
                    .unwrap_or_default();
                (token, graphql_url, allowed_authors, remote_url)
            };
            let token = match token {
                Ok(v) => v,
                Err(err) => return error_response(req, &format!("{err:#}")),
            };

            let (owner, repo) = match parse_github_nwo(&remote_url) {
                Some(v) => v,
                None => {
                    let now_ms = now_ms();
                    let msg = ChatMessage::new(
                        ChatRole::System,
                        format!(
                            "failed to initialize github client: not a github remote: {remote_url}"
                        ),
                        now_ms,
                    );
                    {
                        let mut agents = shared.agents.lock().await;
                        if let Some(rt) = agents.agents.get_mut(&agent_id) {
                            rt.record = rt.record.apply_event(
                                AgentEvent::NeedsResolution {
                                    reason: "github init",
                                },
                                now_ms,
                            );
                            rt.chat.push(msg.clone());
                        }
                    }
                    emit_agent_chat_event(shared.as_ref(), &agent_id, &project, msg);
                    persist_agents_runtime(shared.clone()).await;
                    return error_response(req, "pull request creation failed");
                }
            };

            let github = match GithubBackend::new(owner, repo, token, allowed_authors, graphql_url)
            {
                Ok(v) => v,
                Err(err) => {
                    let now_ms = now_ms();
                    let msg = ChatMessage::new(
                        ChatRole::System,
                        format!("failed to initialize github client: {err:#}"),
                        now_ms,
                    );
                    {
                        let mut agents = shared.agents.lock().await;
                        if let Some(rt) = agents.agents.get_mut(&agent_id) {
                            rt.record = rt.record.apply_event(
                                AgentEvent::NeedsResolution {
                                    reason: "github init",
                                },
                                now_ms,
                            );
                            rt.chat.push(msg.clone());
                        }
                    }
                    emit_agent_chat_event(shared.as_ref(), &agent_id, &project, msg);
                    persist_agents_runtime(shared.clone()).await;
                    return error_response(req, "pull request creation failed");
                }
            };

            let pr_url = match github
                .create_pull_request(&prep.base_branch, &prep.branch, &pr_title, &pr_body)
                .await
            {
                Ok(v) => v,
                Err(err) => {
                    let now_ms = now_ms();
                    let msg = ChatMessage::new(
                        ChatRole::System,
                        format!("failed to create pull request: {err:#}"),
                        now_ms,
                    );
                    {
                        let mut agents = shared.agents.lock().await;
                        if let Some(rt) = agents.agents.get_mut(&agent_id) {
                            rt.record = rt.record.apply_event(
                                AgentEvent::NeedsResolution {
                                    reason: "github pr",
                                },
                                now_ms,
                            );
                            rt.chat.push(msg.clone());
                        }
                    }
                    emit_agent_chat_event(shared.as_ref(), &agent_id, &project, msg);
                    persist_agents_runtime(shared.clone()).await;
                    return error_response(req, "pull request creation failed");
                }
            };

            {
                let now_ms = now_ms();
                let msg = ChatMessage::new(
                    ChatRole::System,
                    format!("pull request created: {pr_url} (sha {})", prep.sha),
                    now_ms,
                );

                let mut agents = shared.agents.lock().await;
                if let Some(rt) = agents.agents.get_mut(&agent_id) {
                    let updated_desc = match rt.record.description.as_deref().map(str::trim) {
                        Some(s) if !s.is_empty() => format!("{s} (PR: {pr_url})"),
                        _ => format!("PR: {pr_url}"),
                    };
                    rt.record.description = Some(updated_desc);
                    rt.record.updated_at_ms = now_ms;
                    rt.chat.push(msg.clone());
                }
                drop(agents);
                emit_agent_chat_event(shared.as_ref(), &agent_id, &project, msg);
            }

            if let Err(err) = stop_agent_runtime_keep_worktree(shared.clone(), &agent_id).await {
                return error_response(req, &format!("stop agent failed: {err:#}"));
            }

            mark_issue_completed(&shared, &project, &issue_id).await;
            release_claims_for_agent(&shared, &agent_id).await;
            persist_agents_runtime(shared.clone()).await;

            Response {
                r#type: MSG_AGENT_DONE.to_owned(),
                id: req.id,
                success: true,
                error: None,
                payload: serde_json::Value::Null,
            }
        }
    }
}

pub(in crate::daemon) async fn handle_agent_idle(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<AgentIdleRequest, _> = serde_json::from_value(payload);
    let idle = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    if idle.agent_id.trim().is_empty() {
        return error_response(req, "agent_id is required");
    }

    let now_ms = now_ms();

    let project = {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get_mut(&idle.agent_id) else {
            return Response {
                r#type: MSG_AGENT_IDLE.to_owned(),
                id: req.id,
                success: true,
                error: None,
                payload: serde_json::Value::Null,
            };
        };
        rt.last_idle_at_ms = Some(now_ms);
        rt.record.project.clone()
    };

    let evt_payload = serde_json::json!({
        "agent_id": idle.agent_id,
        "project": project,
        "idle_at_ms": now_ms,
    });
    let id = shared
        .next_event_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let _ = shared.events_tx.send(Event {
        r#type: EVT_AGENT_IDLE.to_owned(),
        id: format!("evt-{id}"),
        payload: evt_payload,
    });

    Response {
        r#type: MSG_AGENT_IDLE.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}
