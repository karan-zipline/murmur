//! Background task that polls for new issue comments and delivers them to agents.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::{debug, info, warn};

use super::issue_backend::issue_backend_for_project;
use super::state::SharedState;
use super::now_ms;
use crate::dedup_store::DedupStore;

pub struct CommentPoller {
    shared: Arc<SharedState>,
    poll_interval: Duration,
    shutdown_rx: watch::Receiver<bool>,
}

impl CommentPoller {
    pub fn new(
        shared: Arc<SharedState>,
        poll_interval: Duration,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            shared,
            poll_interval,
            shutdown_rx,
        }
    }

    pub async fn run(mut self, dedup: Arc<tokio::sync::Mutex<DedupStore>>) {
        info!(interval_secs = %self.poll_interval.as_secs(), "comment poller started");
        let mut interval = tokio::time::interval(self.poll_interval);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.poll_all_agents(&dedup).await;
                }
                _ = self.shutdown_rx.changed() => {
                    if *self.shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }

        info!("comment poller stopped");
    }

    async fn poll_all_agents(&self, dedup: &Arc<tokio::sync::Mutex<DedupStore>>) {
        // Get all claimed issues from claims registry
        let claims = self.shared.claims.lock().await.list();

        for claim in claims {
            self.poll_agent_comments(&claim.project, &claim.issue_id, &claim.agent_id, dedup)
                .await;
        }
    }

    async fn poll_agent_comments(
        &self,
        project: &str,
        issue_id: &str,
        agent_id: &str,
        dedup: &Arc<tokio::sync::Mutex<DedupStore>>,
    ) {
        // Get agent runtime to check claim_started_at_ms
        let (since_ms, outbound_tx) = {
            let agents = self.shared.agents.lock().await;
            let Some(rt) = agents.agents.get(agent_id) else {
                return;
            };

            // If no claim time, set it to now (handles rehydrated agents)
            let since = rt.claim_started_at_ms;

            (since, rt.outbound_tx.clone())
        };

        // If agent doesn't have a claim time, set it now
        let since_ms = match since_ms {
            Some(ms) => ms,
            None => {
                let now = now_ms();
                let mut agents = self.shared.agents.lock().await;
                if let Some(rt) = agents.agents.get_mut(agent_id) {
                    rt.claim_started_at_ms = Some(now);
                }
                now
            }
        };

        // Get issue backend
        let Ok(backend) = issue_backend_for_project(&self.shared, project).await else {
            return;
        };

        // List comments since claim time
        let comments = match backend.list_comments(issue_id, Some(since_ms)).await {
            Ok(c) => c,
            Err(e) => {
                debug!(
                    project = %project,
                    issue_id = %issue_id,
                    error = %e,
                    "failed to list comments"
                );
                return;
            }
        };

        // Deliver each new comment
        let now = now_ms();
        for comment in comments {
            let dedup_id = format!("comment:{}:{}:{}", project, issue_id, comment.id);

            let is_new = {
                let mut store = dedup.lock().await;
                store.mark(&dedup_id, Some(project), now)
            };

            if is_new {
                self.deliver_comment(agent_id, issue_id, &comment, &outbound_tx)
                    .await;
            }
        }
    }

    async fn deliver_comment(
        &self,
        agent_id: &str,
        issue_id: &str,
        comment: &murmur_core::issue::Comment,
        outbound_tx: &tokio::sync::mpsc::Sender<murmur_core::agent::ChatMessage>,
    ) {
        let msg = format!(
            "New comment on issue #{} from {}:\n\n{}",
            issue_id, comment.author, comment.body
        );

        // Send via outbound channel
        let chat_msg = murmur_core::agent::ChatMessage::new(
            murmur_core::agent::ChatRole::User,
            msg,
            now_ms(),
        );

        if let Err(e) = outbound_tx.try_send(chat_msg) {
            warn!(
                agent_id = %agent_id,
                issue_id = %issue_id,
                error = %e,
                "failed to deliver comment to agent"
            );
        } else {
            info!(
                agent_id = %agent_id,
                issue_id = %issue_id,
                comment_author = %comment.author,
                "delivered comment to agent"
            );
        }
    }
}
