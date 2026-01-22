use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: &str = "0.1";

pub const MSG_PING: &str = "ping";
pub const MSG_SHUTDOWN: &str = "shutdown";
pub const MSG_ATTACH: &str = "attach";
pub const MSG_DETACH: &str = "detach";

pub const MSG_PROJECT_ADD: &str = "project.add";
pub const MSG_PROJECT_REMOVE: &str = "project.remove";
pub const MSG_PROJECT_LIST: &str = "project.list";
pub const MSG_PROJECT_CONFIG_SHOW: &str = "project.config.show";
pub const MSG_PROJECT_CONFIG_GET: &str = "project.config.get";
pub const MSG_PROJECT_CONFIG_SET: &str = "project.config.set";
pub const MSG_PROJECT_STATUS: &str = "project.status";

pub const MSG_AGENT_CREATE: &str = "agent.create";
pub const MSG_AGENT_LIST: &str = "agent.list";
pub const MSG_AGENT_DELETE: &str = "agent.delete";
pub const MSG_AGENT_ABORT: &str = "agent.abort";
pub const MSG_AGENT_SEND_MESSAGE: &str = "agent.send_message";
pub const MSG_AGENT_CHAT_HISTORY: &str = "agent.chat_history";
pub const MSG_AGENT_DONE: &str = "agent.done";
pub const MSG_AGENT_IDLE: &str = "agent.idle";
pub const MSG_AGENT_CLAIM: &str = "agent.claim";
pub const MSG_AGENT_DESCRIBE: &str = "agent.describe";

pub const MSG_ISSUE_LIST: &str = "issue.list";
pub const MSG_ISSUE_GET: &str = "issue.get";
pub const MSG_ISSUE_READY: &str = "issue.ready";
pub const MSG_ISSUE_CREATE: &str = "issue.create";
pub const MSG_ISSUE_UPDATE: &str = "issue.update";
pub const MSG_ISSUE_CLOSE: &str = "issue.close";
pub const MSG_ISSUE_COMMENT: &str = "issue.comment";
pub const MSG_ISSUE_COMMIT: &str = "issue.commit";
pub const MSG_ISSUE_PLAN: &str = "issue.plan";

pub const MSG_ORCHESTRATION_START: &str = "orchestration.start";
pub const MSG_ORCHESTRATION_STOP: &str = "orchestration.stop";
pub const MSG_ORCHESTRATION_STATUS: &str = "orchestration.status";

pub const MSG_CLAIM_LIST: &str = "claim.list";
pub const MSG_COMMIT_LIST: &str = "commit.list";
pub const MSG_STATS: &str = "stats";

pub const MSG_PLAN_START: &str = "plan.start";
pub const MSG_PLAN_STOP: &str = "plan.stop";
pub const MSG_PLAN_LIST: &str = "plan.list";
pub const MSG_PLAN_SEND_MESSAGE: &str = "plan.send_message";
pub const MSG_PLAN_CHAT_HISTORY: &str = "plan.chat_history";
pub const MSG_PLAN_SHOW: &str = "plan.show";

pub const MSG_MANAGER_START: &str = "manager.start";
pub const MSG_MANAGER_STOP: &str = "manager.stop";
pub const MSG_MANAGER_STATUS: &str = "manager.status";
pub const MSG_MANAGER_SEND_MESSAGE: &str = "manager.send_message";
pub const MSG_MANAGER_CHAT_HISTORY: &str = "manager.chat_history";
pub const MSG_MANAGER_CLEAR_HISTORY: &str = "manager.clear_history";

pub const EVT_HEARTBEAT: &str = "heartbeat";
pub const EVT_AGENT_CHAT: &str = "agent.chat";
pub const EVT_AGENT_CREATED: &str = "agent.created";
pub const EVT_AGENT_DELETED: &str = "agent.deleted";
pub const EVT_PERMISSION_REQUEST: &str = "permission.requested";
pub const EVT_USER_QUESTION: &str = "question.requested";
pub const EVT_AGENT_IDLE: &str = "agent.idle";
pub const EVT_ORCHESTRATION_TICK_REQUESTED: &str = "orchestration.tick_requested";

pub const MSG_PERMISSION_REQUEST: &str = "permission.request";
pub const MSG_PERMISSION_RESPOND: &str = "permission.respond";
pub const MSG_PERMISSION_LIST: &str = "permission.list";

pub const MSG_QUESTION_REQUEST: &str = "question.request";
pub const MSG_QUESTION_RESPOND: &str = "question.respond";
pub const MSG_QUESTION_LIST: &str = "question.list";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(default)]
    pub id: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingResponse {
    pub version: String,
    pub protocol: String,
    pub pid: u32,
    pub started_at_ms: u64,
    pub uptime_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AttachRequest {
    #[serde(default)]
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatEvent {
    pub now_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentChatEvent {
    pub agent_id: String,
    pub project: String,
    pub message: ChatMessage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCreatedEvent {
    pub agent: AgentInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDeletedEvent {
    pub agent_id: String,
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectAddRequest {
    pub name: String,
    pub remote_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_agents: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autostart: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectAddResponse {
    pub name: String,
    pub remote_url: String,
    pub repo_dir: String,
    pub max_agents: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRemoveRequest {
    pub name: String,
    #[serde(default)]
    pub delete_worktrees: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectListResponse {
    pub projects: Vec<ProjectInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    pub remote_url: String,
    pub repo_dir: String,
    pub max_agents: u16,
    pub running: bool,
    pub backend: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfigShowRequest {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfigShowResponse {
    pub name: String,
    pub config: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfigGetRequest {
    pub name: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfigGetResponse {
    pub name: String,
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfigSetRequest {
    pub name: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectStatusRequest {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectStatusResponse {
    pub name: String,
    pub repo_dir: String,
    pub socket_path: String,

    pub repo_exists: bool,
    pub socket_reachable: bool,
    pub remote_url_configured: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_url_actual: Option<String>,
    pub remote_matches: bool,

    pub orchestrator_running: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Coding,
    Planner,
    Manager,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Starting,
    Running,
    NeedsResolution,
    Exited,
    Aborted,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
    Tool,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<String>,
    #[serde(default)]
    pub is_error: bool,
    pub ts_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub project: String,
    pub role: AgentRole,
    pub issue_id: String,
    pub state: AgentState,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub worktree_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCreateRequest {
    pub project: String,
    pub issue_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCreateResponse {
    pub agent: AgentInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentListResponse {
    pub agents: Vec<AgentInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAbortRequest {
    pub agent_id: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDeleteRequest {
    pub agent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSendMessageRequest {
    pub agent_id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentClaimRequest {
    pub agent_id: String,
    pub issue_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDescribeRequest {
    pub agent_id: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentChatHistoryRequest {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentChatHistoryResponse {
    pub agent_id: String,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDoneRequest {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdleRequest {
    pub agent_id: String,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionBehavior {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequestPayload {
    pub agent_id: String,
    pub tool_name: String,
    pub tool_input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    pub agent_id: String,
    pub project: String,
    pub tool_name: String,
    pub tool_input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    pub requested_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionResponse {
    pub id: String,
    pub behavior: PermissionBehavior,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub interrupt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRespondPayload {
    pub id: String,
    pub behavior: PermissionBehavior,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub interrupt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PermissionListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionListResponse {
    pub requests: Vec<PermissionRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionItem {
    pub question: String,
    pub header: String,
    #[serde(rename = "multiSelect")]
    pub multi_select: bool,
    pub options: Vec<QuestionOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserQuestionRequestPayload {
    pub agent_id: String,
    pub questions: Vec<QuestionItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserQuestion {
    pub id: String,
    pub agent_id: String,
    pub project: String,
    pub questions: Vec<QuestionItem>,
    pub requested_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserQuestionResponse {
    pub id: String,
    pub answers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserQuestionRespondPayload {
    pub id: String,
    pub answers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UserQuestionListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserQuestionListResponse {
    pub requests: Vec<UserQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStartRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStartResponse {
    pub id: String,
    #[serde(default)]
    pub project: String,
    pub worktree_dir: String,
    pub plan_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStopRequest {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlanListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanListResponse {
    pub plans: Vec<AgentInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanSendMessageRequest {
    pub id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanChatHistoryRequest {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanChatHistoryResponse {
    pub id: String,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanShowRequest {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanShowResponse {
    pub id: String,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagerStartRequest {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagerStopRequest {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagerStatusRequest {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagerStatusResponse {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manager: Option<AgentInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagerSendMessageRequest {
    pub project: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagerChatHistoryRequest {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagerChatHistoryResponse {
    pub project: String,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagerClearHistoryRequest {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationTickRequestedEvent {
    pub project: String,
    pub source: String,
    pub received_at_ms: u64,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueStatus {
    Open,
    Closed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: IssueStatus,
    pub priority: i32,
    #[serde(rename = "type")]
    pub issue_type: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub links: Vec<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueSummary {
    pub id: String,
    pub title: String,
    pub status: IssueStatus,
    pub priority: i32,
    #[serde(rename = "type")]
    pub issue_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueListRequest {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueListResponse {
    pub issues: Vec<IssueSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueGetRequest {
    pub project: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueGetResponse {
    pub issue: Issue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueReadyRequest {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueReadyResponse {
    pub issues: Vec<IssueSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueCreateRequest {
    pub project: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub issue_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub links: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueCreateResponse {
    pub issue: Issue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IssueUpdateRequest {
    pub project: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<IssueStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub issue_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueUpdateResponse {
    pub issue: Issue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueCloseRequest {
    pub project: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueCommentRequest {
    pub project: String,
    pub id: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssuePlanRequest {
    pub project: String,
    pub id: String,
    pub plan: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueCommitRequest {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationStartRequest {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationStopRequest {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationStatusRequest {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationStatusResponse {
    pub project: String,
    pub running: bool,
    pub max_agents: u16,
    pub active_agents: u32,
    pub active_claims: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimInfo {
    pub project: String,
    pub issue_id: String,
    pub agent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimListResponse {
    pub claims: Vec<ClaimInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitListRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitRecord {
    pub project: String,
    pub sha: String,
    pub branch: String,
    pub agent_id: String,
    pub issue_id: String,
    pub merged_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitListResponse {
    pub commits: Vec<CommitRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StatsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageStats {
    #[serde(default)]
    pub output_tokens: i64,
    #[serde(default)]
    pub percent: i32,
    #[serde(default)]
    pub window_end: String,
    #[serde(default)]
    pub time_left: String,
    #[serde(default)]
    pub plan_limit: i64,
    #[serde(default)]
    pub plan: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsResponse {
    pub commit_count: u32,
    pub usage: UsageStats,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let req = Request {
            r#type: MSG_PING.to_owned(),
            id: "req-1".to_owned(),
            payload: Value::Null,
        };

        let json = serde_json::to_string(&req).unwrap();
        let back: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn response_round_trip() {
        let resp = Response {
            r#type: MSG_PING.to_owned(),
            id: "req-1".to_owned(),
            success: true,
            error: None,
            payload: serde_json::json!({"ok": true}),
        };

        let json = serde_json::to_string(&resp).unwrap();
        let back: Response = serde_json::from_str(&json).unwrap();
        assert_eq!(back, resp);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let raw = r#"{"type":"ping","id":"x","payload":{},"extra":123}"#;
        let req: Request = serde_json::from_str(raw).unwrap();
        assert_eq!(req.r#type, "ping");
        assert_eq!(req.id, "x");
    }

    #[test]
    fn missing_fields_default() {
        let raw = r#"{"type":"ping"}"#;
        let req: Request = serde_json::from_str(raw).unwrap();
        assert_eq!(req.r#type, "ping");
        assert_eq!(req.id, "");
        assert_eq!(req.payload, Value::Null);
    }
}
