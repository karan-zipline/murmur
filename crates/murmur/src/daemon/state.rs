use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use murmur_core::agent::{AgentRecord, ChatHistory, ChatMessage};
use murmur_core::claims::ClaimRegistry;
use murmur_core::commits::CommitLog;
use murmur_core::config::{AgentBackend, ConfigFile};
use murmur_core::paths::MurmurPaths;
use murmur_protocol::{
    Event, PermissionRequest, PermissionResponse, UserQuestion, UserQuestionResponse,
};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::dedup_store::DedupStore;
use crate::git::Git;

use super::DaemonHandle;

pub(super) const DEFAULT_CHAT_CAPACITY: usize = 200;

pub(super) struct AgentRuntime {
    pub(super) record: AgentRecord,
    pub(super) backend: AgentBackend,
    pub(super) codex_thread_id: Option<String>,
    pub(super) chat: ChatHistory,
    pub(super) last_idle_at_ms: Option<u64>,
    /// Timestamp when the agent claimed the issue (for comment polling).
    pub(super) claim_started_at_ms: Option<u64>,
    pub(super) outbound_tx: mpsc::Sender<ChatMessage>,
    pub(super) abort_tx: watch::Sender<bool>,
    pub(super) tasks: Vec<tokio::task::JoinHandle<()>>,
}

#[derive(Default)]
pub(super) struct AgentsState {
    pub(super) agents: BTreeMap<String, AgentRuntime>,
}

pub(super) struct OrchestratorRuntime {
    pub(super) shutdown_tx: watch::Sender<bool>,
    pub(super) task: tokio::task::JoinHandle<()>,
}

pub(super) struct PendingPermission {
    pub(super) request: PermissionRequest,
    pub(super) respond_tx: oneshot::Sender<PermissionResponse>,
}

#[derive(Default)]
pub(super) struct PendingPermissions {
    pub(super) pending: BTreeMap<String, PendingPermission>,
}

impl PendingPermissions {
    pub(super) fn insert(
        &mut self,
        request: PermissionRequest,
        respond_tx: oneshot::Sender<PermissionResponse>,
    ) {
        self.pending.insert(
            request.id.clone(),
            PendingPermission {
                request,
                respond_tx,
            },
        );
    }

    pub(super) fn list(&self) -> Vec<PermissionRequest> {
        self.pending.values().map(|p| p.request.clone()).collect()
    }

    pub(super) fn respond(&mut self, response: PermissionResponse) -> bool {
        let Some(pending) = self.pending.remove(&response.id) else {
            return false;
        };
        let _ = pending.respond_tx.send(response);
        true
    }

    pub(super) fn cancel_for_project(&mut self, project: &str) -> usize {
        let ids = self
            .pending
            .iter()
            .filter(|(_, p)| p.request.project == project)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let n = ids.len();
        for id in ids {
            self.pending.remove(&id);
        }
        n
    }
}

pub(super) struct PendingQuestion {
    pub(super) request: UserQuestion,
    pub(super) respond_tx: oneshot::Sender<UserQuestionResponse>,
}

#[derive(Default)]
pub(super) struct PendingQuestions {
    pub(super) pending: BTreeMap<String, PendingQuestion>,
}

impl PendingQuestions {
    pub(super) fn insert(
        &mut self,
        request: UserQuestion,
        respond_tx: oneshot::Sender<UserQuestionResponse>,
    ) {
        self.pending.insert(
            request.id.clone(),
            PendingQuestion {
                request,
                respond_tx,
            },
        );
    }

    pub(super) fn list(&self) -> Vec<UserQuestion> {
        self.pending.values().map(|q| q.request.clone()).collect()
    }

    pub(super) fn respond(&mut self, response: UserQuestionResponse) -> bool {
        let Some(pending) = self.pending.remove(&response.id) else {
            return false;
        };
        let _ = pending.respond_tx.send(response);
        true
    }

    pub(super) fn cancel_for_project(&mut self, project: &str) -> usize {
        let ids = self
            .pending
            .iter()
            .filter(|(_, q)| q.request.project == project)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let n = ids.len();
        for id in ids {
            self.pending.remove(&id);
        }
        n
    }
}

pub(super) struct SharedState {
    pub(super) pid: u32,
    pub(super) started_at: SystemTime,
    pub(super) started_at_instant: Instant,
    pub(super) events_tx: broadcast::Sender<Event>,
    pub(super) shutdown: DaemonHandle,
    pub(super) next_event_id: AtomicU64,
    pub(super) next_conn_id: AtomicU64,
    pub(super) next_agent_id: AtomicU64,
    pub(super) next_plan_id: AtomicU64,
    pub(super) paths: MurmurPaths,
    pub(super) git: Git,
    pub(super) config: tokio::sync::Mutex<ConfigFile>,
    pub(super) agents: tokio::sync::Mutex<AgentsState>,
    pub(super) claims: tokio::sync::Mutex<ClaimRegistry>,
    pub(super) pending_permissions: tokio::sync::Mutex<PendingPermissions>,
    pub(super) pending_questions: tokio::sync::Mutex<PendingQuestions>,
    pub(super) completed_issues: tokio::sync::Mutex<BTreeMap<String, BTreeSet<String>>>,
    pub(super) orchestrators: tokio::sync::Mutex<BTreeMap<String, OrchestratorRuntime>>,
    pub(super) merge_locks: tokio::sync::Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub(super) commits: tokio::sync::Mutex<BTreeMap<String, CommitLog>>,
    pub(super) dedup: Arc<tokio::sync::Mutex<DedupStore>>,
    /// Tracks the last user activity timestamp for each project.
    /// Used by the orchestrator to pause spawning when users are active.
    pub(super) user_activity: tokio::sync::Mutex<BTreeMap<String, Instant>>,
}

impl SharedState {
    /// Record user activity for a project.
    pub(super) async fn record_user_activity(&self, project: &str) {
        let mut activity = self.user_activity.lock().await;
        activity.insert(project.to_owned(), Instant::now());
    }

    /// Get seconds since last user activity for a project.
    /// Returns None if no activity has been recorded.
    pub(super) async fn seconds_since_activity(&self, project: &str) -> Option<u64> {
        let activity = self.user_activity.lock().await;
        activity.get(project).map(|t| t.elapsed().as_secs())
    }

    /// Check if user is currently intervening (within silence threshold).
    /// Returns false if threshold is 0 (intervention detection disabled).
    pub(super) async fn is_user_intervening(&self, project: &str, threshold_secs: u64) -> bool {
        if threshold_secs == 0 {
            return false; // Intervention detection disabled
        }
        match self.seconds_since_activity(project).await {
            Some(secs) => secs < threshold_secs,
            None => false,
        }
    }
}
