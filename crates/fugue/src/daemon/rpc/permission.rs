use std::sync::atomic::Ordering;
use std::sync::Arc;

use fugue_core::agent::ChatRole;
use fugue_core::config::PermissionsChecker;
use fugue_protocol::{
    PermissionListRequest, PermissionListResponse, PermissionRequest, PermissionRequestPayload,
    PermissionRespondPayload, PermissionResponse, Request, Response, EVT_PERMISSION_REQUEST,
    MSG_PERMISSION_LIST, MSG_PERMISSION_REQUEST, MSG_PERMISSION_RESPOND,
};
use tokio::sync::oneshot;

use crate::llm_auth;
use crate::providers;

use super::super::{now_ms, SharedState};
use super::error_response;

fn generate_request_id() -> String {
    let mut buf = [0u8; 4];
    if getrandom::getrandom(&mut buf).is_ok() {
        let mut out = String::with_capacity(8);
        for b in buf {
            use std::fmt::Write as _;
            let _ = write!(&mut out, "{b:02x}");
        }
        return out;
    }
    let nonce = now_ms();
    format!("perm-{nonce}")
}

pub(in crate::daemon) async fn handle_permission_request(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<PermissionRequestPayload, _> = serde_json::from_value(payload);
    let request = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let raw_agent_id = request.agent_id.trim();
    if raw_agent_id.is_empty() {
        return error_response(req, "agent_id is required");
    }
    if request.tool_name.trim().is_empty() {
        return error_response(req, "tool_name is required");
    }

    let agent_id = raw_agent_id.trim_start_matches("plan:").to_owned();
    if agent_id.trim().is_empty() {
        return error_response(req, "agent_id is required");
    }

    let (project, issue_id, agent_description, chat_tail) = {
        let agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get(&agent_id) else {
            return error_response(req, "agent not found");
        };
        (
            rt.record.project.clone(),
            rt.record.issue_id.clone(),
            rt.record.description.clone(),
            rt.chat.tail(10),
        )
    };

    let checker = {
        let cfg = shared.config.lock().await;
        cfg.project(&project)
            .map(|p| p.permissions_checker)
            .unwrap_or(PermissionsChecker::Manual)
    };

    if checker == PermissionsChecker::Llm {
        match try_llm_authorize(
            shared.as_ref(),
            &project,
            &issue_id,
            agent_description.as_deref(),
            &chat_tail,
            &request,
        )
        .await
        {
            LlmAuthorizeOutcome::Decision(resp) => {
                return Response {
                    r#type: MSG_PERMISSION_REQUEST.to_owned(),
                    id: req.id,
                    success: true,
                    error: None,
                    payload: serde_json::to_value(resp).unwrap_or(serde_json::Value::Null),
                };
            }
            LlmAuthorizeOutcome::Blocked(message) => {
                return Response {
                    r#type: MSG_PERMISSION_REQUEST.to_owned(),
                    id: req.id,
                    success: true,
                    error: None,
                    payload: serde_json::to_value(PermissionResponse {
                        id: generate_request_id(),
                        behavior: fugue_protocol::PermissionBehavior::Deny,
                        message: Some(message),
                        interrupt: false,
                    })
                    .unwrap_or(serde_json::Value::Null),
                };
            }
        }
    }

    let id = generate_request_id();
    let requested_at_ms = now_ms();
    let stored = PermissionRequest {
        id: id.clone(),
        agent_id,
        project,
        tool_name: request.tool_name,
        tool_input: request.tool_input,
        tool_use_id: request.tool_use_id,
        requested_at_ms,
    };

    let (tx, rx) = oneshot::channel::<PermissionResponse>();
    {
        let mut pending = shared.pending_permissions.lock().await;
        pending.insert(stored.clone(), tx);
    }

    let evt_payload = serde_json::to_value(&stored).unwrap_or(serde_json::Value::Null);
    let evt_id = shared.next_event_id.fetch_add(1, Ordering::Relaxed);
    let _ = shared.events_tx.send(fugue_protocol::Event {
        r#type: EVT_PERMISSION_REQUEST.to_owned(),
        id: format!("evt-{evt_id}"),
        payload: evt_payload,
    });

    let response = match rx.await {
        Ok(v) => v,
        Err(_) => return error_response(req, "permission request canceled"),
    };

    Response {
        r#type: MSG_PERMISSION_REQUEST.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
    }
}

enum LlmAuthorizeOutcome {
    Decision(PermissionResponse),
    Blocked(String),
}

async fn try_llm_authorize(
    shared: &SharedState,
    project: &str,
    issue_id: &str,
    agent_description: Option<&str>,
    chat_tail: &[fugue_core::agent::ChatMessage],
    request: &PermissionRequestPayload,
) -> LlmAuthorizeOutcome {
    const DEFAULT_PROVIDER: &str = "anthropic";
    const DEFAULT_MODEL: &str = "claude-haiku-4-5";

    let (provider, model, api_key, api_url) = {
        let cfg = shared.config.lock().await;
        let provider =
            providers::llm_auth_provider(&cfg).unwrap_or_else(|| DEFAULT_PROVIDER.to_owned());
        let model = providers::llm_auth_model(&cfg).unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        let Some(provider_parsed) = llm_auth::Provider::parse(&provider) else {
            tracing::warn!(project = %project, provider = %provider, "llm permissions: unknown provider");
            return LlmAuthorizeOutcome::Blocked(
                "LLM authorization failed - operation blocked".to_owned(),
            );
        };

        match provider_parsed {
            llm_auth::Provider::Anthropic => {
                let Some(api_key) = providers::anthropic_api_key(&cfg) else {
                    return LlmAuthorizeOutcome::Blocked(
                        "LLM authorization failed - operation blocked".to_owned(),
                    );
                };
                let api_url = providers::anthropic_api_url(&cfg);
                (provider_parsed, model, api_key, api_url)
            }
            llm_auth::Provider::OpenAI => {
                let Some(api_key) = providers::openai_api_key(&cfg) else {
                    return LlmAuthorizeOutcome::Blocked(
                        "LLM authorization failed - operation blocked".to_owned(),
                    );
                };
                let api_url = providers::openai_api_url(&cfg);
                (provider_parsed, model, api_key, api_url)
            }
        }
    };

    let tool_input = serde_json::to_string(&request.tool_input).unwrap_or_else(|_| "{}".to_owned());

    let agent_task = agent_description
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let issue = issue_id.trim();
            if issue.is_empty() {
                String::new()
            } else {
                format!("Work on issue {issue}")
            }
        });

    let conversation_ctx = chat_tail
        .iter()
        .filter_map(|m| {
            let content = m.content.trim();
            if content.is_empty() {
                return None;
            }
            let role = match m.role {
                ChatRole::User => "user",
                ChatRole::Assistant => "assistant",
                ChatRole::Tool => "tool",
                ChatRole::System => "system",
            };
            Some(format!("{role}: {content}"))
        })
        .collect::<Vec<_>>();

    let auth = match llm_auth::Authorizer::new(provider, model, api_key, api_url) {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!(project = %project, error = %err, "llm permissions: failed to initialize authorizer");
            return LlmAuthorizeOutcome::Blocked(
                "LLM authorization failed - operation blocked".to_owned(),
            );
        }
    };

    let result = match auth
        .authorize(llm_auth::Request {
            tool_name: request.tool_name.clone(),
            tool_input,
            agent_task,
            conversation_ctx,
        })
        .await
    {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!(project = %project, error = %err, "llm permissions: authorizer call failed");
            return LlmAuthorizeOutcome::Blocked(
                "LLM authorization failed - operation blocked".to_owned(),
            );
        }
    };

    match result.decision {
        llm_auth::Decision::Safe => LlmAuthorizeOutcome::Decision(PermissionResponse {
            id: generate_request_id(),
            behavior: fugue_protocol::PermissionBehavior::Allow,
            message: None,
            interrupt: false,
        }),
        llm_auth::Decision::Unsafe => LlmAuthorizeOutcome::Decision(PermissionResponse {
            id: generate_request_id(),
            behavior: fugue_protocol::PermissionBehavior::Deny,
            message: (!result.rationale.trim().is_empty()).then_some(result.rationale),
            interrupt: false,
        }),
        llm_auth::Decision::Unsure => LlmAuthorizeOutcome::Blocked(
            "Blocked by LLM authorization: unable to determine safety".to_owned(),
        ),
    }
}

pub(in crate::daemon) async fn handle_permission_list(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<PermissionListRequest, _> = serde_json::from_value(payload);
    let list = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let mut requests = {
        let pending = shared.pending_permissions.lock().await;
        pending.list()
    };
    if let Some(project) = list.project.as_deref() {
        requests.retain(|r| r.project == project);
    }
    requests.sort_by(|a, b| {
        a.requested_at_ms
            .cmp(&b.requested_at_ms)
            .then(a.id.cmp(&b.id))
    });

    let payload = PermissionListResponse { requests };
    Response {
        r#type: MSG_PERMISSION_LIST.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_permission_respond(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<PermissionRespondPayload, _> = serde_json::from_value(payload);
    let respond = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    if respond.id.trim().is_empty() {
        return error_response(req, "id is required");
    }

    let ok = {
        let mut pending = shared.pending_permissions.lock().await;
        pending.respond(PermissionResponse {
            id: respond.id,
            behavior: respond.behavior,
            message: respond.message,
            interrupt: respond.interrupt,
        })
    };
    if !ok {
        return error_response(req, "permission request not found");
    }

    Response {
        r#type: MSG_PERMISSION_RESPOND.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}
