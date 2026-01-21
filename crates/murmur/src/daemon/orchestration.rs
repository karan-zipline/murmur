use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use murmur_core::orchestration::orchestrator_tick;
use tokio::sync::watch;

use super::{issue_backend_for_project, spawn_agent_with_kickoff, SharedState};

pub(in crate::daemon) async fn request_orchestrator_tick(
    shared: Arc<SharedState>,
    project: String,
) {
    if let Err(err) = orchestrator_tick_once(shared, &project).await {
        tracing::warn!(project = %project, error = %err, "orchestrator tick requested failed");
    }
}

pub(in crate::daemon) async fn count_active_agents(shared: &SharedState, project: &str) -> u32 {
    let agents = shared.agents.lock().await;
    agents
        .agents
        .values()
        .filter(|a| a.record.project == project)
        .filter(|a| a.record.role == murmur_core::agent::AgentRole::Coding)
        .filter(|a| {
            matches!(
                a.record.state,
                murmur_core::agent::AgentState::Starting
                    | murmur_core::agent::AgentState::Running
                    | murmur_core::agent::AgentState::NeedsResolution
            )
        })
        .count() as u32
}

pub(in crate::daemon) async fn count_active_claims(shared: &SharedState, project: &str) -> u32 {
    let claim_entries = {
        let claims = shared.claims.lock().await;
        claims.list()
    };

    let agent_ids = {
        let agents = shared.agents.lock().await;
        agents.agents.keys().cloned().collect::<BTreeSet<_>>()
    };

    claim_entries
        .into_iter()
        .filter(|c| c.project == project)
        .filter(|c| agent_ids.contains(&c.agent_id))
        .count() as u32
}

pub(in crate::daemon) async fn orchestrator_is_running(
    shared: &SharedState,
    project: &str,
) -> bool {
    let mut orchestrators = shared.orchestrators.lock().await;
    if let Some(rt) = orchestrators.get(project) {
        if rt.task.is_finished() {
            orchestrators.remove(project);
            return false;
        }
        return true;
    }
    false
}

fn orchestration_interval() -> Duration {
    let ms = std::env::var("FUGUE_ORCHESTRATOR_INTERVAL_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(500);
    Duration::from_millis(ms.clamp(50, 60_000))
}

pub(in crate::daemon) async fn orchestrator_loop(
    shared: Arc<SharedState>,
    project: String,
    mut stop_rx: watch::Receiver<bool>,
) {
    let mut global_shutdown = shared.shutdown.subscribe();

    let mut interval = tokio::time::interval(orchestration_interval());
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        if *stop_rx.borrow() || *global_shutdown.borrow() {
            break;
        }

        tokio::select! {
            _ = stop_rx.changed() => {},
            _ = global_shutdown.changed() => {},
            _ = interval.tick() => {
                if let Err(err) = orchestrator_tick_once(shared.clone(), &project).await {
                    tracing::warn!(project = %project, error = %err, "orchestrator tick failed");
                }
            }
        }
    }
}

async fn orchestrator_tick_once(shared: Arc<SharedState>, project: &str) -> anyhow::Result<()> {
    let max_agents = {
        let cfg = shared.config.lock().await;
        cfg.project(project)
            .map(|p| p.max_agents as usize)
            .ok_or_else(|| anyhow!("project not found"))?
    };

    let ready = {
        let backend = issue_backend_for_project(shared.as_ref(), project)
            .await
            .map_err(anyhow::Error::msg)?;
        backend.ready().await?
    };

    let active_agents = count_active_agents(shared.as_ref(), project).await as usize;
    let claims = { shared.claims.lock().await.clone() };
    let completed = {
        let completed = shared.completed_issues.lock().await;
        completed.get(project).cloned().unwrap_or_default()
    };

    let plan = orchestrator_tick(
        project,
        active_agents,
        max_agents,
        ready
            .iter()
            .map(|i| i.id.as_str())
            .filter(|id| !completed.contains(*id)),
        &claims,
    );

    for issue_id in plan.issue_ids {
        let active = count_active_agents(shared.as_ref(), project).await as usize;
        if active >= max_agents {
            break;
        }

        let issue_id_for_log = issue_id.clone();

        let title = ready
            .iter()
            .find(|i| i.id == issue_id)
            .map(|i| i.title.trim())
            .filter(|t| !t.is_empty())
            .unwrap_or("");

        let kickoff = if title.is_empty() {
            format!("Start work on issue {issue_id}.")
        } else {
            format!("Start work on issue {issue_id}: {title}")
        };

        if let Err(err) = spawn_agent_with_kickoff(
            shared.clone(),
            project.to_owned(),
            issue_id,
            Some(kickoff),
            None,
        )
        .await
        {
            tracing::warn!(project = %project, issue_id = %issue_id_for_log, error = ?err, "spawn agent failed");
        }
    }

    Ok(())
}
