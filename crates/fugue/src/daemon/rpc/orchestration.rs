use std::sync::Arc;
use std::time::Duration;

use fugue_protocol::{
    OrchestrationStartRequest, OrchestrationStatusRequest, OrchestrationStatusResponse,
    OrchestrationStopRequest, Request, Response, MSG_ORCHESTRATION_START, MSG_ORCHESTRATION_STATUS,
    MSG_ORCHESTRATION_STOP,
};
use tokio::sync::watch;

use super::super::orchestration::{
    count_active_agents, count_active_claims, orchestrator_is_running, orchestrator_loop,
};
use super::super::{state::OrchestratorRuntime, SharedState};
use super::error_response;

pub(in crate::daemon) async fn handle_orchestration_start(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<OrchestrationStartRequest, _> = serde_json::from_value(payload);
    let start = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    {
        let cfg = shared.config.lock().await;
        if cfg.project(&start.project).is_none() {
            return error_response(req, "project not found");
        }
    }

    let mut orchestrators = shared.orchestrators.lock().await;
    if let Some(existing) = orchestrators.get(&start.project) {
        if !existing.task.is_finished() {
            return Response {
                r#type: MSG_ORCHESTRATION_START.to_owned(),
                id: req.id,
                success: true,
                error: None,
                payload: serde_json::Value::Null,
            };
        }
    }
    orchestrators.remove(&start.project);

    let (stop_tx, stop_rx) = watch::channel(false);
    let project = start.project.clone();
    let task = tokio::spawn(orchestrator_loop(shared.clone(), project.clone(), stop_rx));
    orchestrators.insert(
        project,
        OrchestratorRuntime {
            shutdown_tx: stop_tx,
            task,
        },
    );

    Response {
        r#type: MSG_ORCHESTRATION_START.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_orchestration_stop(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<OrchestrationStopRequest, _> = serde_json::from_value(payload);
    let stop = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let runtime = {
        let mut orchestrators = shared.orchestrators.lock().await;
        orchestrators.remove(&stop.project)
    };

    if let Some(rt) = runtime {
        let _ = rt.shutdown_tx.send(true);
        tokio::spawn(async move {
            let mut task = rt.task;
            if tokio::time::timeout(Duration::from_secs(3), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = tokio::time::timeout(Duration::from_secs(3), task).await;
            }
        });
    }

    Response {
        r#type: MSG_ORCHESTRATION_STOP.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_orchestration_status(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<OrchestrationStatusRequest, _> = serde_json::from_value(payload);
    let status = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let max_agents = {
        let cfg = shared.config.lock().await;
        let Some(p) = cfg.project(&status.project) else {
            return error_response(req, "project not found");
        };
        p.max_agents
    };

    let active_agents = count_active_agents(shared, &status.project).await;
    let active_claims = count_active_claims(shared, &status.project).await;
    let running = orchestrator_is_running(shared, &status.project).await;

    let payload = OrchestrationStatusResponse {
        project: status.project,
        running,
        max_agents,
        active_agents,
        active_claims,
    };

    Response {
        r#type: MSG_ORCHESTRATION_STATUS.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}
