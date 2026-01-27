use std::sync::Arc;

use anyhow::Context as _;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use hmac::{Hmac, Mac as _};
use murmur_core::config::WebhookConfig;
use murmur_protocol::{Event, OrchestrationTickRequestedEvent, EVT_ORCHESTRATION_TICK_REQUESTED};
use serde::Deserialize;
use sha2::Sha256;
use tokio::sync::{watch, Mutex};

use crate::dedup_store::DedupStore;

use super::orchestration;
use super::{now_ms, SharedState};

#[derive(Clone)]
struct WebhookState {
    shared: Arc<SharedState>,
    config: EffectiveWebhookConfig,
    dedup: Arc<Mutex<DedupStore>>,
}

#[derive(Clone)]
struct EffectiveWebhookConfig {
    bind_addr: String,
    secret: String,
    path_prefix: String,
}

#[derive(Debug, Deserialize)]
struct WebhookQuery {
    #[serde(default)]
    project: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubActionOnly {
    #[serde(default)]
    action: String,
}

#[derive(Debug, Deserialize)]
struct LinearActionAndType {
    #[serde(default)]
    action: String,
    #[serde(default, rename = "type")]
    ty: String,
}

pub(in crate::daemon) async fn maybe_start_webhook_server(
    shared: Arc<SharedState>,
    mut shutdown_rx: watch::Receiver<bool>,
    dedup: Arc<Mutex<DedupStore>>,
) -> anyhow::Result<()> {
    let Some(cfg) = load_webhook_config(&shared).await else {
        return Ok(());
    };

    let state = WebhookState {
        shared,
        config: cfg.clone(),
        dedup,
    };

    let mut prefix = cfg.path_prefix.trim().to_owned();
    if prefix.is_empty() {
        prefix = "/webhooks".to_owned();
    }
    if !prefix.starts_with('/') {
        prefix.insert(0, '/');
    }
    while prefix.ends_with('/') && prefix.len() > 1 {
        prefix.pop();
    }

    let app = Router::new()
        .route("/health", get(health))
        .nest(
            &prefix,
            Router::new()
                .route("/github", post(github))
                .route("/linear", post(linear)),
        )
        .with_state(state);

    let bind_addr = normalize_bind_addr(cfg.bind_addr.trim());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("bind {}", bind_addr))?;
    let local = listener
        .local_addr()
        .with_context(|| format!("local addr {}", bind_addr))?;

    tracing::info!(addr = %local, path_prefix = %prefix, "webhook server started");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            loop {
                if *shutdown_rx.borrow() {
                    break;
                }
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
        .context("webhook server exited")?;

    tracing::info!("webhook server stopped");
    Ok(())
}

async fn load_webhook_config(shared: &SharedState) -> Option<EffectiveWebhookConfig> {
    let cfg = shared.config.lock().await;
    let webhook: &WebhookConfig = cfg.webhook.as_ref()?;
    if !webhook.enabled {
        return None;
    }

    Some(EffectiveWebhookConfig {
        bind_addr: webhook.effective_bind_addr().to_owned(),
        secret: webhook.secret.clone(),
        path_prefix: webhook.effective_path_prefix().to_owned(),
    })
}

fn normalize_bind_addr(bind_addr: &str) -> String {
    let trimmed = bind_addr.trim();
    if let Some(port) = trimmed.strip_prefix(':') {
        return format!("0.0.0.0:{port}");
    }
    trimmed.to_owned()
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn github(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    Query(query): Query<WebhookQuery>,
    body: Bytes,
) -> impl IntoResponse {
    let project = match resolve_project(&headers, query.project) {
        Ok(v) => v,
        Err(msg) => return (StatusCode::BAD_REQUEST, msg).into_response(),
    };

    if !validate_github_signature(&state.config.secret, &headers, &body) {
        return (StatusCode::UNAUTHORIZED, "invalid signature").into_response();
    }

    let event = header_str(&headers, "X-GitHub-Event").unwrap_or_default();
    if event != "issues" && event != "issue_comment" {
        return StatusCode::OK.into_response();
    }

    let parsed: GitHubActionOnly = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid JSON").into_response(),
    };
    let action = parsed.action.trim();

    let should_tick = match event {
        "issues" => matches!(action, "opened" | "edited"),
        "issue_comment" => matches!(action, "created"),
        _ => false,
    };

    if !should_tick {
        return StatusCode::OK.into_response();
    }

    let delivery = header_str(&headers, "X-GitHub-Delivery");
    let dedup_id = delivery
        .map(|d| format!("github:{d}"))
        .unwrap_or_else(|| format!("github:{}", sha256_hex(&body)));

    if let Err(err) = process_tick_request(&state, &project, "github", &dedup_id).await {
        tracing::warn!(project = %project, error = %err, "webhook github processing failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "error").into_response();
    }

    StatusCode::OK.into_response()
}

async fn linear(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    Query(query): Query<WebhookQuery>,
    body: Bytes,
) -> impl IntoResponse {
    let project = match resolve_project(&headers, query.project) {
        Ok(v) => v,
        Err(msg) => return (StatusCode::BAD_REQUEST, msg).into_response(),
    };

    if !validate_linear_signature(&state.config.secret, &headers, &body) {
        return (StatusCode::UNAUTHORIZED, "invalid signature").into_response();
    }

    let parsed: LinearActionAndType = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid JSON").into_response(),
    };

    let ty = parsed.ty.trim();
    let action = parsed.action.trim();

    let should_tick = match ty {
        "Issue" => matches!(action, "create" | "update"),
        "Comment" => matches!(action, "create"),
        _ => false,
    };

    if !should_tick {
        return StatusCode::OK.into_response();
    }

    let delivery = header_str(&headers, "Linear-Delivery");
    let dedup_id = delivery
        .map(|d| format!("linear:{d}"))
        .unwrap_or_else(|| format!("linear:{}", sha256_hex(&body)));

    if let Err(err) = process_tick_request(&state, &project, "linear", &dedup_id).await {
        tracing::warn!(project = %project, error = %err, "webhook linear processing failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "error").into_response();
    }

    StatusCode::OK.into_response()
}

fn resolve_project(headers: &HeaderMap, query: Option<String>) -> Result<String, &'static str> {
    if let Some(p) = query
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
    {
        return Ok(p);
    }

    if let Some(p) = header_str(headers, "X-Murmur-Project")
        .or_else(|| header_str(headers, "X-Fab-Project"))
        .or_else(|| header_str(headers, "X-Project"))
    {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
    }

    Err("missing project")
}

fn header_str<'a>(headers: &'a HeaderMap, key: &str) -> Option<&'a str> {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn validate_github_signature(secret: &str, headers: &HeaderMap, body: &[u8]) -> bool {
    let secret = secret.trim();
    if secret.is_empty() {
        return true;
    }

    let sig = header_str(headers, "X-Hub-Signature-256").unwrap_or_default();
    let Some(sig) = sig.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = hex::decode(sig) else {
        return false;
    };

    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

fn validate_linear_signature(secret: &str, headers: &HeaderMap, body: &[u8]) -> bool {
    let secret = secret.trim();
    if secret.is_empty() {
        return true;
    }

    let sig = header_str(headers, "Linear-Signature").unwrap_or_default();
    let Ok(expected) = hex::decode(sig) else {
        return false;
    };

    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

fn sha256_hex(body: &[u8]) -> String {
    use sha2::Digest as _;
    let mut h = Sha256::new();
    h.update(body);
    hex::encode(h.finalize())
}

async fn process_tick_request(
    state: &WebhookState,
    project: &str,
    source: &str,
    dedup_id: &str,
) -> anyhow::Result<()> {
    let now_ms = now_ms();

    let (persist_path, persist_entries) = {
        let mut dedup = state.dedup.lock().await;
        let should_process = dedup.mark(dedup_id, Some(project), now_ms);
        if !should_process {
            return Ok(());
        }
        let path = dedup.path().to_owned();
        let entries = dedup.entries_snapshot();
        (path, entries)
    };

    let _ = DedupStore::save_snapshot(&persist_path, &persist_entries).await;

    emit_tick_requested(&state.shared, project, source, now_ms);

    if orchestration::orchestrator_is_running(state.shared.as_ref(), project).await {
        tokio::spawn(orchestration::request_orchestrator_tick(
            state.shared.clone(),
            project.to_owned(),
        ));
    }

    Ok(())
}

fn emit_tick_requested(shared: &SharedState, project: &str, source: &str, received_at_ms: u64) {
    let payload = serde_json::to_value(OrchestrationTickRequestedEvent {
        project: project.to_owned(),
        source: source.to_owned(),
        received_at_ms,
    })
    .unwrap_or(serde_json::Value::Null);

    let id = shared
        .next_event_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let _ = shared.events_tx.send(Event {
        r#type: EVT_ORCHESTRATION_TICK_REQUESTED.to_owned(),
        id: format!("evt-{id}"),
        payload,
    });
}
