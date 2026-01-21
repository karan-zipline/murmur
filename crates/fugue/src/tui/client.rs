use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use fugue_protocol::{
    AgentChatHistoryResponse, AgentListResponse, CommitListResponse, PermissionBehavior,
    PermissionListResponse, PlanStartResponse, ProjectListResponse, StatsResponse,
    UserQuestionListResponse,
};
use std::collections::BTreeMap;

pub struct EventStream {
    rx: mpsc::UnboundedReceiver<anyhow::Result<fugue_protocol::Event>>,
    join: JoinHandle<()>,
}

impl EventStream {
    pub(crate) fn new(
        rx: mpsc::UnboundedReceiver<anyhow::Result<fugue_protocol::Event>>,
        join: JoinHandle<()>,
    ) -> Self {
        Self { rx, join }
    }

    pub async fn recv(&mut self) -> Option<anyhow::Result<fugue_protocol::Event>> {
        self.rx.recv().await
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        self.join.abort();
    }
}

#[async_trait]
pub trait TuiClient: Send + Sync {
    async fn stream_events(&self, projects: Vec<String>) -> Result<EventStream>;
    async fn project_list(&self) -> Result<ProjectListResponse>;
    async fn agent_list(&self) -> Result<AgentListResponse>;
    async fn agent_chat_history(
        &self,
        agent_id: String,
        limit: Option<u32>,
    ) -> Result<AgentChatHistoryResponse>;
    async fn agent_send_message(&self, agent_id: String, message: String) -> Result<()>;
    async fn agent_abort(&self, agent_id: String) -> Result<()>;
    async fn commit_list(
        &self,
        project: Option<String>,
        limit: Option<u32>,
    ) -> Result<CommitListResponse>;
    async fn stats(&self, project: Option<String>) -> Result<StatsResponse>;
    async fn permission_list(&self, project: Option<String>) -> Result<PermissionListResponse>;
    async fn permission_respond(&self, id: String, behavior: PermissionBehavior) -> Result<()>;
    async fn question_list(&self, project: Option<String>) -> Result<UserQuestionListResponse>;
    async fn question_respond(&self, id: String, answers: BTreeMap<String, String>) -> Result<()>;
    async fn plan_start(
        &self,
        project: Option<String>,
        prompt: String,
    ) -> Result<PlanStartResponse>;
    async fn plan_stop(&self, plan_id: String) -> Result<()>;
}
