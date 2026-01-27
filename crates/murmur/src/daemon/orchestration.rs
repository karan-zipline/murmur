use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use murmur_core::orchestration::orchestrator_tick;
use tokio::sync::watch;

use super::{issue_backend_for_project, spawn_agent_without_issue, SharedState};

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
    // Get config values: max_agents and silence_threshold
    let (max_agents, silence_threshold) = {
        let cfg = shared.config.lock().await;
        let max = cfg
            .project(project)
            .map(|p| p.max_agents as usize)
            .ok_or_else(|| anyhow!("project not found"))?;
        let threshold = cfg.silence_threshold_for_project(project);
        (max, threshold)
    };

    // Check user intervention before spawning
    if shared.is_user_intervening(project, silence_threshold).await {
        let secs = shared.seconds_since_activity(project).await.unwrap_or(0);
        tracing::debug!(
            project = %project,
            seconds_since_activity = secs,
            threshold = silence_threshold,
            "skipping spawn: user intervention active"
        );
        return Ok(());
    }

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

    // Use orchestrator_tick to determine how many unclaimed issues exist
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

    // Spawn agents without pre-assigning issues.
    // Agents will find and claim issues themselves using `mm issue ready` and `mm agent claim`.
    let to_spawn = plan.issue_ids.len();
    let kickstart = build_kickstart_prompt(project, &shared.paths.socket_path);

    for _ in 0..to_spawn {
        let active = count_active_agents(shared.as_ref(), project).await as usize;
        if active >= max_agents {
            break;
        }

        if let Err(err) =
            spawn_agent_without_issue(shared.clone(), project.to_owned(), kickstart.clone()).await
        {
            tracing::warn!(project = %project, error = ?err, "spawn agent failed");
        }
    }

    Ok(())
}

fn build_kickstart_prompt(project: &str, socket_path: &Path) -> String {
    let socket_path = socket_path.to_string_lossy();
    format!(
        r###"The `mm` command is available on PATH (use `mm`, not `./mm`).

You are a Murmur coding agent for project `{project}`.

The Murmur daemon is already running and reachable at:
`{socket_path}`

(`MURMUR_SOCKET_PATH` is already set for you; you should not need `--socket-path`.)

## Find Work

1. List available issues: `mm issue ready`
2. Claim one: `mm agent claim <issue-id>`
3. Set your status: `mm agent describe "<short description>"`

If the issue you picked is already claimed, pick a different one from `mm issue ready`.
If no issues are available (or you did not claim anything), run `mm agent done` to exit; it should not create a PR or attempt merges.

## Workflow

Read the issue carefully and decide how to proceed:

1. **IMPLEMENT**: If the issue is clear, proceed with coding.
2. **ASK QUESTIONS**: If you need clarification, use `mm issue comment <issue-id> --body "Your question"` to ask, then run `mm agent done` (do NOT close the issue).
3. **DECOMPOSE**: If the issue is complex:
   - Add a plan with `mm issue plan <issue-id> --body "## Steps\n- Step 1\n- Step 2"`
   - Create sub-issues with `mm issue create "Sub-task title" --depends-on <issue-id>`
   - Then run `mm agent done` (do NOT close the parent issue).

## When Implementation is Complete

1. Run all relevant tests and quality checks
2. Commit your changes with a descriptive message
3. Close the issue: `mm issue close <issue-id>`
4. Signal completion: `mm agent done`

## Important Notes

- Do NOT run `mm daemon` or `mm project start/stop` (the daemon is already running)
- Do NOT run `git push` — merging and pushing happens automatically when you run `mm agent done`
- Only close an issue when you have COMPLETED the implementation
- Do NOT close if you only added comments, a plan, or created sub-issues
"###,
    )
}
