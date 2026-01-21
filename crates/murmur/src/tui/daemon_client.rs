use anyhow::{anyhow, Context as _};
use async_trait::async_trait;
use murmur_core::paths::MurmurPaths;
use murmur_protocol::{AttachRequest, Request, Response, MSG_ATTACH};
use std::collections::BTreeMap;
use tokio::io::{BufReader, BufWriter};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use crate::ipc::jsonl::{read_jsonl, write_jsonl};

use super::client::{EventStream, TuiClient};

#[derive(Debug, Clone)]
pub struct DaemonTuiClient {
    paths: MurmurPaths,
}

impl DaemonTuiClient {
    pub fn new(paths: MurmurPaths) -> Self {
        Self { paths }
    }
}

fn new_request_id(prefix: &str) -> String {
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}-{now_ns}")
}

#[async_trait]
impl TuiClient for DaemonTuiClient {
    async fn stream_events(&self, projects: Vec<String>) -> anyhow::Result<EventStream> {
        let stream = UnixStream::connect(&self.paths.socket_path)
            .await
            .with_context(|| format!("connect: {}", self.paths.socket_path.display()))?;

        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut writer = BufWriter::new(write_half);

        let attach = Request {
            r#type: MSG_ATTACH.to_owned(),
            id: new_request_id("attach"),
            payload: serde_json::to_value(AttachRequest { projects })
                .context("serialize attach request")?,
        };
        write_jsonl(&mut writer, &attach)
            .await
            .context("write attach request")?;

        loop {
            let Some(value) = read_jsonl::<_, serde_json::Value>(&mut reader)
                .await
                .context("read attach response")?
            else {
                return Err(anyhow!("unexpected EOF waiting for attach response"));
            };

            if value.get("success").is_none() {
                continue;
            }

            let resp: Response = serde_json::from_value(value).context("parse attach response")?;
            if !resp.success {
                return Err(anyhow!(resp
                    .error
                    .unwrap_or_else(|| "attach failed".to_owned())));
            }
            break;
        }

        let (tx, rx) = mpsc::unbounded_channel();

        let join = tokio::spawn(async move {
            let _writer_guard = writer;
            loop {
                let value = match read_jsonl::<_, serde_json::Value>(&mut reader).await {
                    Ok(v) => v,
                    Err(err) => {
                        let _ = tx.send(Err(anyhow!(err)));
                        break;
                    }
                };

                let Some(value) = value else {
                    let _ = tx.send(Err(anyhow!("event stream closed")));
                    break;
                };

                if value.get("success").is_some() {
                    continue;
                }

                let evt: murmur_protocol::Event = match serde_json::from_value(value) {
                    Ok(evt) => evt,
                    Err(_) => continue,
                };
                if tx.send(Ok(evt)).is_err() {
                    break;
                }
            }
        });

        Ok(EventStream::new(rx, join))
    }

    async fn project_list(&self) -> anyhow::Result<murmur_protocol::ProjectListResponse> {
        crate::client::project_list(&self.paths).await
    }

    async fn agent_list(&self) -> anyhow::Result<murmur_protocol::AgentListResponse> {
        crate::client::agent_list(&self.paths).await
    }

    async fn agent_chat_history(
        &self,
        agent_id: String,
        limit: Option<u32>,
    ) -> anyhow::Result<murmur_protocol::AgentChatHistoryResponse> {
        crate::client::agent_chat_history(&self.paths, agent_id, limit).await
    }

    async fn agent_send_message(&self, agent_id: String, message: String) -> anyhow::Result<()> {
        crate::client::agent_send_message(&self.paths, agent_id, message).await
    }

    async fn agent_abort(&self, agent_id: String) -> anyhow::Result<()> {
        crate::client::agent_abort(&self.paths, agent_id, false).await
    }

    async fn commit_list(
        &self,
        project: Option<String>,
        limit: Option<u32>,
    ) -> anyhow::Result<murmur_protocol::CommitListResponse> {
        crate::client::commit_list(&self.paths, project, limit).await
    }

    async fn stats(
        &self,
        project: Option<String>,
    ) -> anyhow::Result<murmur_protocol::StatsResponse> {
        crate::client::stats(&self.paths, project).await
    }

    async fn permission_list(
        &self,
        project: Option<String>,
    ) -> anyhow::Result<murmur_protocol::PermissionListResponse> {
        crate::client::permission_list(&self.paths, project).await
    }

    async fn permission_respond(
        &self,
        id: String,
        behavior: murmur_protocol::PermissionBehavior,
    ) -> anyhow::Result<()> {
        crate::client::permission_respond(
            &self.paths,
            murmur_protocol::PermissionRespondPayload {
                id,
                behavior,
                message: None,
                interrupt: false,
            },
        )
        .await
    }

    async fn question_list(
        &self,
        project: Option<String>,
    ) -> anyhow::Result<murmur_protocol::UserQuestionListResponse> {
        crate::client::question_list(&self.paths, project).await
    }

    async fn question_respond(
        &self,
        id: String,
        answers: BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        crate::client::question_respond(
            &self.paths,
            murmur_protocol::UserQuestionRespondPayload { id, answers },
        )
        .await
    }

    async fn plan_start(
        &self,
        project: Option<String>,
        prompt: String,
    ) -> anyhow::Result<murmur_protocol::PlanStartResponse> {
        crate::client::plan_start(&self.paths, project, prompt).await
    }

    async fn plan_stop(&self, plan_id: String) -> anyhow::Result<()> {
        crate::client::plan_stop(&self.paths, plan_id).await
    }
}
