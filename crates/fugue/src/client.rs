use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context as _};
use fugue_core::paths::FuguePaths;
use fugue_protocol::{
    AgentAbortRequest, AgentChatHistoryRequest, AgentChatHistoryResponse, AgentClaimRequest,
    AgentCreateRequest, AgentCreateResponse, AgentDeleteRequest, AgentDescribeRequest,
    AgentDoneRequest, AgentIdleRequest, AgentListResponse, AgentSendMessageRequest,
    ClaimListRequest, ClaimListResponse, CommitListRequest, CommitListResponse,
    IssueCommentRequest, IssueCommitRequest, IssueCreateRequest, IssueCreateResponse,
    IssueGetRequest, IssueGetResponse, IssueListRequest, IssueListResponse, IssuePlanRequest,
    IssueReadyRequest, IssueReadyResponse, IssueUpdateRequest, IssueUpdateResponse,
    ManagerChatHistoryRequest, ManagerChatHistoryResponse, ManagerClearHistoryRequest,
    ManagerSendMessageRequest, ManagerStartRequest, ManagerStatusRequest, ManagerStatusResponse,
    ManagerStopRequest, OrchestrationStartRequest, OrchestrationStatusRequest,
    OrchestrationStatusResponse, OrchestrationStopRequest, PermissionListRequest,
    PermissionListResponse, PermissionRequestPayload, PermissionRespondPayload, PermissionResponse,
    PingResponse, PlanChatHistoryRequest, PlanChatHistoryResponse, PlanListRequest,
    PlanListResponse, PlanSendMessageRequest, PlanShowRequest, PlanShowResponse, PlanStartRequest,
    PlanStartResponse, PlanStopRequest, ProjectAddRequest, ProjectAddResponse,
    ProjectConfigGetRequest, ProjectConfigGetResponse, ProjectConfigSetRequest,
    ProjectConfigShowRequest, ProjectConfigShowResponse, ProjectListResponse, ProjectRemoveRequest,
    ProjectStatusRequest, ProjectStatusResponse, Request, Response, StatsRequest, StatsResponse,
    UserQuestionListRequest, UserQuestionListResponse, UserQuestionRequestPayload,
    UserQuestionRespondPayload, UserQuestionResponse, MSG_AGENT_ABORT, MSG_AGENT_CHAT_HISTORY,
    MSG_AGENT_CLAIM, MSG_AGENT_CREATE, MSG_AGENT_DELETE, MSG_AGENT_DESCRIBE, MSG_AGENT_DONE,
    MSG_AGENT_IDLE, MSG_AGENT_LIST, MSG_AGENT_SEND_MESSAGE, MSG_CLAIM_LIST, MSG_COMMIT_LIST,
    MSG_ISSUE_CLOSE, MSG_ISSUE_COMMENT, MSG_ISSUE_COMMIT, MSG_ISSUE_CREATE, MSG_ISSUE_GET,
    MSG_ISSUE_LIST, MSG_ISSUE_PLAN, MSG_ISSUE_READY, MSG_ISSUE_UPDATE, MSG_MANAGER_CHAT_HISTORY,
    MSG_MANAGER_CLEAR_HISTORY, MSG_MANAGER_SEND_MESSAGE, MSG_MANAGER_START, MSG_MANAGER_STATUS,
    MSG_MANAGER_STOP, MSG_ORCHESTRATION_START, MSG_ORCHESTRATION_STATUS, MSG_ORCHESTRATION_STOP,
    MSG_PERMISSION_LIST, MSG_PERMISSION_REQUEST, MSG_PERMISSION_RESPOND, MSG_PING,
    MSG_PLAN_CHAT_HISTORY, MSG_PLAN_LIST, MSG_PLAN_SEND_MESSAGE, MSG_PLAN_SHOW, MSG_PLAN_START,
    MSG_PLAN_STOP, MSG_PROJECT_ADD, MSG_PROJECT_CONFIG_GET, MSG_PROJECT_CONFIG_SET,
    MSG_PROJECT_CONFIG_SHOW, MSG_PROJECT_LIST, MSG_PROJECT_REMOVE, MSG_PROJECT_STATUS,
    MSG_QUESTION_LIST, MSG_QUESTION_REQUEST, MSG_QUESTION_RESPOND, MSG_SHUTDOWN, MSG_STATS,
};
use tokio::io::{BufReader, BufWriter};
use tokio::net::UnixStream;

use crate::ipc::jsonl::{read_jsonl, write_jsonl};

pub async fn ping(paths: &FuguePaths) -> anyhow::Result<PingResponse> {
    let req = Request {
        r#type: MSG_PING.to_owned(),
        id: new_request_id("ping"),
        payload: serde_json::Value::Null,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "ping failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse ping response payload")
}

pub async fn shutdown(paths: &FuguePaths) -> anyhow::Result<()> {
    let req = Request {
        r#type: MSG_SHUTDOWN.to_owned(),
        id: new_request_id("shutdown"),
        payload: serde_json::Value::Null,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "shutdown failed".to_owned())))
    }
}

pub async fn project_list(paths: &FuguePaths) -> anyhow::Result<ProjectListResponse> {
    let req = Request {
        r#type: MSG_PROJECT_LIST.to_owned(),
        id: new_request_id("project-list"),
        payload: serde_json::Value::Null,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "project.list failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse project.list payload")
}

pub async fn project_add(
    paths: &FuguePaths,
    name: String,
    remote_url: String,
    max_agents: Option<u16>,
    autostart: Option<bool>,
    backend: Option<String>,
) -> anyhow::Result<ProjectAddResponse> {
    let payload = ProjectAddRequest {
        name,
        remote_url,
        max_agents,
        autostart,
        backend,
    };

    let req = Request {
        r#type: MSG_PROJECT_ADD.to_owned(),
        id: new_request_id("project-add"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };

    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "project.add failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse project.add payload")
}

pub async fn project_remove(
    paths: &FuguePaths,
    name: String,
    delete_worktrees: bool,
) -> anyhow::Result<()> {
    let payload = ProjectRemoveRequest {
        name,
        delete_worktrees,
    };
    let req = Request {
        r#type: MSG_PROJECT_REMOVE.to_owned(),
        id: new_request_id("project-remove"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "project.remove failed".to_owned())))
    }
}

pub async fn project_config_show(
    paths: &FuguePaths,
    name: String,
) -> anyhow::Result<ProjectConfigShowResponse> {
    let payload = ProjectConfigShowRequest { name };
    let req = Request {
        r#type: MSG_PROJECT_CONFIG_SHOW.to_owned(),
        id: new_request_id("project-config-show"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "project.config.show failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse project.config.show payload")
}

pub async fn project_config_get(
    paths: &FuguePaths,
    name: String,
    key: String,
) -> anyhow::Result<ProjectConfigGetResponse> {
    let payload = ProjectConfigGetRequest { name, key };
    let req = Request {
        r#type: MSG_PROJECT_CONFIG_GET.to_owned(),
        id: new_request_id("project-config-get"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "project.config.get failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse project.config.get payload")
}

pub async fn project_config_set(
    paths: &FuguePaths,
    name: String,
    key: String,
    value: String,
) -> anyhow::Result<()> {
    let payload = ProjectConfigSetRequest { name, key, value };
    let req = Request {
        r#type: MSG_PROJECT_CONFIG_SET.to_owned(),
        id: new_request_id("project-config-set"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "project.config.set failed".to_owned())))
    }
}

pub async fn project_status(
    paths: &FuguePaths,
    name: String,
) -> anyhow::Result<ProjectStatusResponse> {
    let payload = ProjectStatusRequest { name };
    let req = Request {
        r#type: MSG_PROJECT_STATUS.to_owned(),
        id: new_request_id("project-status"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "project.status failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse project.status payload")
}

pub async fn agent_create(
    paths: &FuguePaths,
    project: String,
    issue_id: String,
    backend: Option<String>,
) -> anyhow::Result<AgentCreateResponse> {
    let payload = AgentCreateRequest {
        project,
        issue_id,
        backend,
    };
    let req = Request {
        r#type: MSG_AGENT_CREATE.to_owned(),
        id: new_request_id("agent-create"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "agent.create failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse agent.create payload")
}

pub async fn agent_list(paths: &FuguePaths) -> anyhow::Result<AgentListResponse> {
    let req = Request {
        r#type: MSG_AGENT_LIST.to_owned(),
        id: new_request_id("agent-list"),
        payload: serde_json::Value::Null,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "agent.list failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse agent.list payload")
}

pub async fn agent_abort(paths: &FuguePaths, agent_id: String, force: bool) -> anyhow::Result<()> {
    let payload = AgentAbortRequest { agent_id, force };
    let req = Request {
        r#type: MSG_AGENT_ABORT.to_owned(),
        id: new_request_id("agent-abort"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "agent.abort failed".to_owned())))
    }
}

pub async fn agent_delete(paths: &FuguePaths, agent_id: String) -> anyhow::Result<()> {
    let payload = AgentDeleteRequest { agent_id };
    let req = Request {
        r#type: MSG_AGENT_DELETE.to_owned(),
        id: new_request_id("agent-delete"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "agent.delete failed".to_owned())))
    }
}

pub async fn agent_idle(paths: &FuguePaths, agent_id: String) -> anyhow::Result<()> {
    let payload = AgentIdleRequest { agent_id };
    let req = Request {
        r#type: MSG_AGENT_IDLE.to_owned(),
        id: new_request_id("agent-idle"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "agent.idle failed".to_owned())))
    }
}

pub async fn agent_send_message(
    paths: &FuguePaths,
    agent_id: String,
    message: String,
) -> anyhow::Result<()> {
    let payload = AgentSendMessageRequest { agent_id, message };
    let req = Request {
        r#type: MSG_AGENT_SEND_MESSAGE.to_owned(),
        id: new_request_id("agent-send-message"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "agent.send_message failed".to_owned())))
    }
}

pub async fn agent_claim(
    paths: &FuguePaths,
    agent_id: String,
    issue_id: String,
) -> anyhow::Result<()> {
    let payload = AgentClaimRequest { agent_id, issue_id };
    let req = Request {
        r#type: MSG_AGENT_CLAIM.to_owned(),
        id: new_request_id("agent-claim"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "agent.claim failed".to_owned())))
    }
}

pub async fn agent_describe(
    paths: &FuguePaths,
    agent_id: String,
    description: String,
) -> anyhow::Result<()> {
    let payload = AgentDescribeRequest {
        agent_id,
        description,
    };
    let req = Request {
        r#type: MSG_AGENT_DESCRIBE.to_owned(),
        id: new_request_id("agent-describe"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "agent.describe failed".to_owned())))
    }
}

pub async fn agent_chat_history(
    paths: &FuguePaths,
    agent_id: String,
    limit: Option<u32>,
) -> anyhow::Result<AgentChatHistoryResponse> {
    let payload = AgentChatHistoryRequest { agent_id, limit };
    let req = Request {
        r#type: MSG_AGENT_CHAT_HISTORY.to_owned(),
        id: new_request_id("agent-chat-history"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "agent.chat_history failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse agent.chat_history payload")
}

pub async fn agent_done(
    paths: &FuguePaths,
    agent_id: String,
    task_id: Option<String>,
    error: Option<String>,
) -> anyhow::Result<()> {
    let payload = AgentDoneRequest {
        agent_id,
        task_id,
        error,
    };
    let req = Request {
        r#type: MSG_AGENT_DONE.to_owned(),
        id: new_request_id("agent-done"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "agent.done failed".to_owned())))
    }
}

pub async fn orchestration_start(paths: &FuguePaths, project: String) -> anyhow::Result<()> {
    let payload = OrchestrationStartRequest { project };
    let req = Request {
        r#type: MSG_ORCHESTRATION_START.to_owned(),
        id: new_request_id("orchestration-start"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "orchestration.start failed".to_owned())))
    }
}

pub async fn orchestration_stop(paths: &FuguePaths, project: String) -> anyhow::Result<()> {
    let payload = OrchestrationStopRequest { project };
    let req = Request {
        r#type: MSG_ORCHESTRATION_STOP.to_owned(),
        id: new_request_id("orchestration-stop"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "orchestration.stop failed".to_owned())))
    }
}

pub async fn orchestration_status(
    paths: &FuguePaths,
    project: String,
) -> anyhow::Result<OrchestrationStatusResponse> {
    let payload = OrchestrationStatusRequest { project };
    let req = Request {
        r#type: MSG_ORCHESTRATION_STATUS.to_owned(),
        id: new_request_id("orchestration-status"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "orchestration.status failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse orchestration.status payload")
}

pub async fn claim_list(
    paths: &FuguePaths,
    project: Option<String>,
) -> anyhow::Result<ClaimListResponse> {
    let payload = ClaimListRequest { project };
    let req = Request {
        r#type: MSG_CLAIM_LIST.to_owned(),
        id: new_request_id("claim-list"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "claim.list failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse claim.list payload")
}

pub async fn commit_list(
    paths: &FuguePaths,
    project: Option<String>,
    limit: Option<u32>,
) -> anyhow::Result<CommitListResponse> {
    let payload = CommitListRequest { project, limit };
    let req = Request {
        r#type: MSG_COMMIT_LIST.to_owned(),
        id: new_request_id("commit-list"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "commit.list failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse commit.list payload")
}

pub async fn stats(paths: &FuguePaths, project: Option<String>) -> anyhow::Result<StatsResponse> {
    let payload = StatsRequest { project };
    let req = Request {
        r#type: MSG_STATS.to_owned(),
        id: new_request_id("stats"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "stats failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse stats payload")
}

pub async fn issue_list(paths: &FuguePaths, project: String) -> anyhow::Result<IssueListResponse> {
    let payload = IssueListRequest { project };
    let req = Request {
        r#type: MSG_ISSUE_LIST.to_owned(),
        id: new_request_id("issue-list"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "issue.list failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse issue.list payload")
}

pub async fn issue_get(
    paths: &FuguePaths,
    project: String,
    id: String,
) -> anyhow::Result<IssueGetResponse> {
    let payload = IssueGetRequest { project, id };
    let req = Request {
        r#type: MSG_ISSUE_GET.to_owned(),
        id: new_request_id("issue-get"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "issue.get failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse issue.get payload")
}

pub async fn issue_ready(
    paths: &FuguePaths,
    project: String,
) -> anyhow::Result<IssueReadyResponse> {
    let payload = IssueReadyRequest { project };
    let req = Request {
        r#type: MSG_ISSUE_READY.to_owned(),
        id: new_request_id("issue-ready"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "issue.ready failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse issue.ready payload")
}

pub async fn issue_create(
    paths: &FuguePaths,
    req_payload: IssueCreateRequest,
) -> anyhow::Result<IssueCreateResponse> {
    let req = Request {
        r#type: MSG_ISSUE_CREATE.to_owned(),
        id: new_request_id("issue-create"),
        payload: serde_json::to_value(req_payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "issue.create failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse issue.create payload")
}

pub async fn issue_update(
    paths: &FuguePaths,
    req_payload: IssueUpdateRequest,
) -> anyhow::Result<IssueUpdateResponse> {
    let req = Request {
        r#type: MSG_ISSUE_UPDATE.to_owned(),
        id: new_request_id("issue-update"),
        payload: serde_json::to_value(req_payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "issue.update failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse issue.update payload")
}

pub async fn issue_close(paths: &FuguePaths, project: String, id: String) -> anyhow::Result<()> {
    let payload = fugue_protocol::IssueCloseRequest { project, id };
    let req = Request {
        r#type: MSG_ISSUE_CLOSE.to_owned(),
        id: new_request_id("issue-close"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "issue.close failed".to_owned())))
    }
}

pub async fn issue_comment(
    paths: &FuguePaths,
    project: String,
    id: String,
    body: String,
) -> anyhow::Result<()> {
    let payload = IssueCommentRequest { project, id, body };
    let req = Request {
        r#type: MSG_ISSUE_COMMENT.to_owned(),
        id: new_request_id("issue-comment"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "issue.comment failed".to_owned())))
    }
}

pub async fn issue_plan(
    paths: &FuguePaths,
    project: String,
    id: String,
    plan: String,
) -> anyhow::Result<()> {
    let payload = IssuePlanRequest { project, id, plan };
    let req = Request {
        r#type: MSG_ISSUE_PLAN.to_owned(),
        id: new_request_id("issue-plan"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "issue.plan failed".to_owned())))
    }
}

pub async fn issue_commit(paths: &FuguePaths, project: String) -> anyhow::Result<()> {
    let payload = IssueCommitRequest { project };
    let req = Request {
        r#type: MSG_ISSUE_COMMIT.to_owned(),
        id: new_request_id("issue-commit"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "issue.commit failed".to_owned())))
    }
}

pub async fn permission_request(
    paths: &FuguePaths,
    payload: PermissionRequestPayload,
) -> anyhow::Result<PermissionResponse> {
    let req = Request {
        r#type: MSG_PERMISSION_REQUEST.to_owned(),
        id: new_request_id("permission-request"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request_with_timeout(paths, req, Duration::from_secs(5 * 60)).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "permission.request failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse permission.request payload")
}

pub async fn permission_list(
    paths: &FuguePaths,
    project: Option<String>,
) -> anyhow::Result<PermissionListResponse> {
    let payload = PermissionListRequest { project };
    let req = Request {
        r#type: MSG_PERMISSION_LIST.to_owned(),
        id: new_request_id("permission-list"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "permission.list failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse permission.list payload")
}

pub async fn permission_respond(
    paths: &FuguePaths,
    payload: PermissionRespondPayload,
) -> anyhow::Result<()> {
    let req = Request {
        r#type: MSG_PERMISSION_RESPOND.to_owned(),
        id: new_request_id("permission-respond"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "permission.respond failed".to_owned())))
    }
}

pub async fn question_request(
    paths: &FuguePaths,
    payload: UserQuestionRequestPayload,
) -> anyhow::Result<UserQuestionResponse> {
    let req = Request {
        r#type: MSG_QUESTION_REQUEST.to_owned(),
        id: new_request_id("question-request"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request_with_timeout(paths, req, Duration::from_secs(5 * 60)).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "question.request failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse question.request payload")
}

pub async fn question_list(
    paths: &FuguePaths,
    project: Option<String>,
) -> anyhow::Result<UserQuestionListResponse> {
    let payload = UserQuestionListRequest { project };
    let req = Request {
        r#type: MSG_QUESTION_LIST.to_owned(),
        id: new_request_id("question-list"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "question.list failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse question.list payload")
}

pub async fn question_respond(
    paths: &FuguePaths,
    payload: UserQuestionRespondPayload,
) -> anyhow::Result<()> {
    let req = Request {
        r#type: MSG_QUESTION_RESPOND.to_owned(),
        id: new_request_id("question-respond"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "question.respond failed".to_owned())))
    }
}

pub async fn plan_start(
    paths: &FuguePaths,
    project: Option<String>,
    prompt: String,
) -> anyhow::Result<PlanStartResponse> {
    let payload = PlanStartRequest { project, prompt };
    let req = Request {
        r#type: MSG_PLAN_START.to_owned(),
        id: new_request_id("plan-start"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request_with_timeout(paths, req, Duration::from_secs(5 * 60)).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "plan.start failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse plan.start payload")
}

pub async fn plan_stop(paths: &FuguePaths, id: String) -> anyhow::Result<()> {
    let payload = PlanStopRequest { id };
    let req = Request {
        r#type: MSG_PLAN_STOP.to_owned(),
        id: new_request_id("plan-stop"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "plan.stop failed".to_owned())))
    }
}

pub async fn plan_list(
    paths: &FuguePaths,
    project: Option<String>,
) -> anyhow::Result<PlanListResponse> {
    let payload = PlanListRequest { project };
    let req = Request {
        r#type: MSG_PLAN_LIST.to_owned(),
        id: new_request_id("plan-list"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "plan.list failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse plan.list payload")
}

pub async fn plan_send_message(
    paths: &FuguePaths,
    id: String,
    message: String,
) -> anyhow::Result<()> {
    let payload = PlanSendMessageRequest { id, message };
    let req = Request {
        r#type: MSG_PLAN_SEND_MESSAGE.to_owned(),
        id: new_request_id("plan-send-message"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "plan.send_message failed".to_owned())))
    }
}

pub async fn plan_chat_history(
    paths: &FuguePaths,
    id: String,
    limit: Option<u32>,
) -> anyhow::Result<PlanChatHistoryResponse> {
    let payload = PlanChatHistoryRequest { id, limit };
    let req = Request {
        r#type: MSG_PLAN_CHAT_HISTORY.to_owned(),
        id: new_request_id("plan-chat-history"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "plan.chat_history failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse plan.chat_history payload")
}

pub async fn plan_show(paths: &FuguePaths, id: String) -> anyhow::Result<PlanShowResponse> {
    let payload = PlanShowRequest { id };
    let req = Request {
        r#type: MSG_PLAN_SHOW.to_owned(),
        id: new_request_id("plan-show"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "plan.show failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse plan.show payload")
}

pub async fn manager_start(paths: &FuguePaths, project: String) -> anyhow::Result<()> {
    let payload = ManagerStartRequest { project };
    let req = Request {
        r#type: MSG_MANAGER_START.to_owned(),
        id: new_request_id("manager-start"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request_with_timeout(paths, req, Duration::from_secs(5 * 60)).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "manager.start failed".to_owned())))
    }
}

pub async fn manager_stop(paths: &FuguePaths, project: String) -> anyhow::Result<()> {
    let payload = ManagerStopRequest { project };
    let req = Request {
        r#type: MSG_MANAGER_STOP.to_owned(),
        id: new_request_id("manager-stop"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "manager.stop failed".to_owned())))
    }
}

pub async fn manager_status(
    paths: &FuguePaths,
    project: String,
) -> anyhow::Result<ManagerStatusResponse> {
    let payload = ManagerStatusRequest { project };
    let req = Request {
        r#type: MSG_MANAGER_STATUS.to_owned(),
        id: new_request_id("manager-status"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "manager.status failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse manager.status payload")
}

pub async fn manager_send_message(
    paths: &FuguePaths,
    project: String,
    message: String,
) -> anyhow::Result<()> {
    let payload = ManagerSendMessageRequest { project, message };
    let req = Request {
        r#type: MSG_MANAGER_SEND_MESSAGE.to_owned(),
        id: new_request_id("manager-send-message"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(
            resp.error
                .unwrap_or_else(|| "manager.send_message failed".to_owned())
        ))
    }
}

pub async fn manager_chat_history(
    paths: &FuguePaths,
    project: String,
    limit: Option<u32>,
) -> anyhow::Result<ManagerChatHistoryResponse> {
    let payload = ManagerChatHistoryRequest { project, limit };
    let req = Request {
        r#type: MSG_MANAGER_CHAT_HISTORY.to_owned(),
        id: new_request_id("manager-chat-history"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if !resp.success {
        return Err(anyhow!(resp
            .error
            .unwrap_or_else(|| "manager.chat_history failed".to_owned())));
    }
    serde_json::from_value(resp.payload).context("parse manager.chat_history payload")
}

pub async fn manager_clear_history(paths: &FuguePaths, project: String) -> anyhow::Result<()> {
    let payload = ManagerClearHistoryRequest { project };
    let req = Request {
        r#type: MSG_MANAGER_CLEAR_HISTORY.to_owned(),
        id: new_request_id("manager-clear-history"),
        payload: serde_json::to_value(payload).context("serialize payload")?,
    };
    let resp = request(paths, req).await?;
    if resp.success {
        Ok(())
    } else {
        Err(anyhow!(resp.error.unwrap_or_else(|| {
            "manager.clear_history failed".to_owned()
        })))
    }
}

pub async fn request(paths: &FuguePaths, req: Request) -> anyhow::Result<Response> {
    request_with_timeout(paths, req, Duration::from_secs(2)).await
}

pub async fn request_with_timeout(
    paths: &FuguePaths,
    req: Request,
    read_timeout: Duration,
) -> anyhow::Result<Response> {
    let socket = paths.socket_path.clone();

    let stream = tokio::time::timeout(Duration::from_secs(1), UnixStream::connect(&socket))
        .await
        .context("connect timeout")?
        .with_context(|| format!("connect: {}", socket.display()))?;

    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut writer = BufWriter::new(write_half);

    write_jsonl(&mut writer, &req)
        .await
        .context("write request")?;

    let resp: Response =
        tokio::time::timeout(read_timeout, async { read_jsonl(&mut reader).await })
            .await
            .context("read timeout")?
            .context("read response")?
            .ok_or_else(|| anyhow!("unexpected EOF reading response"))?;

    Ok(resp)
}

fn new_request_id(prefix: &str) -> String {
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}-{now_ns}")
}
