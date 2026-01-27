use murmur_core::agent::{AgentRecord, AgentRole, ChatMessage, ChatRole};
use murmur_core::config::AgentBackend;
use murmur_protocol::{
    AgentInfo, AgentRole as ProtoAgentRole, AgentState as ProtoAgentState,
    ChatMessage as ProtoChatMessage, ChatRole as ProtoChatRole, Issue, IssueStatus, IssueSummary,
};

pub(in crate::daemon) fn to_proto_issue_summary(issue: &murmur_core::issue::Issue) -> IssueSummary {
    IssueSummary {
        id: issue.id.clone(),
        title: issue.title.clone(),
        status: to_proto_issue_status(issue.status),
        priority: issue.priority,
        issue_type: issue.issue_type.clone(),
    }
}

pub(in crate::daemon) fn to_proto_issue(issue: murmur_core::issue::Issue) -> Issue {
    Issue {
        id: issue.id,
        title: issue.title,
        description: issue.description,
        status: to_proto_issue_status(issue.status),
        priority: issue.priority,
        issue_type: issue.issue_type,
        dependencies: issue.dependencies,
        labels: issue.labels,
        links: issue.links,
        created_at_ms: issue.created_at_ms,
    }
}

pub(in crate::daemon) fn to_proto_issue_status(status: murmur_core::issue::Status) -> IssueStatus {
    match status {
        murmur_core::issue::Status::Open => IssueStatus::Open,
        murmur_core::issue::Status::Closed => IssueStatus::Closed,
        murmur_core::issue::Status::Blocked => IssueStatus::Blocked,
    }
}

pub(in crate::daemon) fn from_proto_issue_status(
    status: IssueStatus,
) -> murmur_core::issue::Status {
    match status {
        IssueStatus::Open => murmur_core::issue::Status::Open,
        IssueStatus::Closed => murmur_core::issue::Status::Closed,
        IssueStatus::Blocked => murmur_core::issue::Status::Blocked,
    }
}

pub(in crate::daemon) fn agent_info_from_record(
    record: &AgentRecord,
    backend: AgentBackend,
) -> AgentInfo {
    AgentInfo {
        id: record.id.clone(),
        project: record.project.clone(),
        role: match record.role {
            AgentRole::Coding => ProtoAgentRole::Coding,
            AgentRole::Planner => ProtoAgentRole::Planner,
            AgentRole::Manager => ProtoAgentRole::Manager,
        },
        issue_id: record.issue_id.clone(),
        state: match record.state {
            murmur_core::agent::AgentState::Starting => ProtoAgentState::Starting,
            murmur_core::agent::AgentState::Running => ProtoAgentState::Running,
            murmur_core::agent::AgentState::Idle => ProtoAgentState::Idle,
            murmur_core::agent::AgentState::NeedsResolution => ProtoAgentState::NeedsResolution,
            murmur_core::agent::AgentState::Exited => ProtoAgentState::Exited,
            murmur_core::agent::AgentState::Aborted => ProtoAgentState::Aborted,
        },
        created_at_ms: record.created_at_ms,
        updated_at_ms: record.updated_at_ms,
        backend: Some(
            match backend {
                AgentBackend::Claude => "claude",
                AgentBackend::Codex => "codex",
            }
            .to_owned(),
        ),
        description: record.description.clone(),
        worktree_dir: record.worktree_dir.clone(),
        pid: record.pid,
        exit_code: record.exit_code,
        codex_thread_id: record.codex_thread_id.clone(),
    }
}

pub(in crate::daemon) fn to_proto_chat_message(msg: ChatMessage) -> ProtoChatMessage {
    ProtoChatMessage {
        role: match msg.role {
            ChatRole::User => ProtoChatRole::User,
            ChatRole::Assistant => ProtoChatRole::Assistant,
            ChatRole::Tool => ProtoChatRole::Tool,
            ChatRole::System => ProtoChatRole::System,
        },
        content: msg.content,
        tool_name: msg.tool_name,
        tool_input: msg.tool_input,
        tool_use_id: msg.tool_use_id,
        tool_result: msg.tool_result,
        is_error: msg.is_error,
        ts_ms: msg.ts_ms,
    }
}
