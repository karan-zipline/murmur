use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use murmur_protocol::{
    AttachRequest, Event, HeartbeatEvent, Request, Response, EVT_HEARTBEAT, MSG_AGENT_ABORT,
    MSG_AGENT_CHAT_HISTORY, MSG_AGENT_CLAIM, MSG_AGENT_CREATE, MSG_AGENT_DELETE,
    MSG_AGENT_DESCRIBE, MSG_AGENT_DONE, MSG_AGENT_IDLE, MSG_AGENT_LIST, MSG_AGENT_SEND_MESSAGE,
    MSG_AGENT_SYNC_COMMENTS, MSG_ATTACH, MSG_CLAIM_LIST, MSG_COMMIT_LIST, MSG_DETACH,
    MSG_ISSUE_CLOSE, MSG_ISSUE_COMMENT, MSG_ISSUE_COMMIT, MSG_ISSUE_CREATE, MSG_ISSUE_GET,
    MSG_ISSUE_LIST, MSG_ISSUE_LIST_COMMENTS, MSG_ISSUE_PLAN, MSG_ISSUE_READY, MSG_ISSUE_UPDATE,
    MSG_MANAGER_CHAT_HISTORY, MSG_MANAGER_CLEAR_HISTORY, MSG_MANAGER_SEND_MESSAGE,
    MSG_MANAGER_START, MSG_MANAGER_STATUS, MSG_MANAGER_STOP, MSG_ORCHESTRATION_START,
    MSG_ORCHESTRATION_STATUS, MSG_ORCHESTRATION_STOP, MSG_PERMISSION_LIST, MSG_PERMISSION_REQUEST,
    MSG_PERMISSION_RESPOND, MSG_PING, MSG_PLAN_CHAT_HISTORY, MSG_PLAN_LIST, MSG_PLAN_SEND_MESSAGE,
    MSG_PLAN_SHOW, MSG_PLAN_START, MSG_PLAN_STOP, MSG_PROJECT_ADD, MSG_PROJECT_CONFIG_GET,
    MSG_PROJECT_CONFIG_SET, MSG_PROJECT_CONFIG_SHOW, MSG_PROJECT_LIST, MSG_PROJECT_REMOVE,
    MSG_PROJECT_STATUS, MSG_QUESTION_LIST, MSG_QUESTION_REQUEST, MSG_QUESTION_RESPOND,
    MSG_SHUTDOWN, MSG_STATS,
};
use tokio::io::{AsyncWriteExt, BufReader, BufWriter};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, watch};

use crate::ipc::jsonl::{read_jsonl, write_jsonl};

use super::{rpc, DaemonHandle, SharedState};

#[derive(Debug)]
enum Outbound {
    Response(Response),
    Event(Event),
}

pub(super) async fn bind_socket(path: &Path) -> anyhow::Result<UnixListener> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create socket dir: {}", parent.display()))?;
    }

    if path.exists() {
        tokio::fs::remove_file(path)
            .await
            .with_context(|| format!("remove existing socket: {}", path.display()))?;
    }

    let listener = UnixListener::bind(path)
        .with_context(|| format!("bind unix socket: {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(listener)
}

pub(super) async fn cleanup_socket(path: &Path) {
    let _ = tokio::fs::remove_file(path).await;
}

pub(super) async fn accept_loop(
    listener: UnixListener,
    shared: Arc<SharedState>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        if *shutdown_rx.borrow() {
            break;
        }

        tokio::select! {
            _ = shutdown_rx.changed() => {},
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _addr)) => {
                        let conn_id = shared.next_conn_id.fetch_add(1, Ordering::Relaxed);
                        let shared = shared.clone();
                        tokio::spawn(async move {
                            if let Err(err) = handle_connection(stream, shared, conn_id).await {
                                tracing::debug!(conn_id, error = %err, "connection ended with error");
                            }
                        });
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "accept failed");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        }
    }
}

async fn handle_connection(
    stream: UnixStream,
    shared: Arc<SharedState>,
    conn_id: u64,
) -> anyhow::Result<()> {
    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let writer = BufWriter::new(write_half);

    let (out_tx, out_rx) = mpsc::channel::<Outbound>(256);
    let writer_task = tokio::spawn(connection_writer(writer, out_rx, conn_id));

    let mut stream_task: Option<tokio::task::JoinHandle<()>> = None;

    loop {
        let maybe_req: Option<Request> = read_jsonl(&mut reader).await?;
        let Some(req) = maybe_req else { break };

        let req_type = req.r#type.clone();
        match req_type.as_str() {
            MSG_PING => {
                let resp = rpc::handle_ping(&shared, req)?;
                if out_tx.send(Outbound::Response(resp)).await.is_err() {
                    break;
                }
            }
            MSG_SHUTDOWN => {
                let resp = Response {
                    r#type: MSG_SHUTDOWN.to_owned(),
                    id: req.id,
                    success: true,
                    error: None,
                    payload: serde_json::Value::Null,
                };
                let _ = out_tx.send(Outbound::Response(resp)).await;
                shared.shutdown.request_shutdown();
                break;
            }
            MSG_PROJECT_LIST => {
                let resp = rpc::handle_project_list(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PROJECT_ADD => {
                let resp = rpc::handle_project_add(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PROJECT_REMOVE => {
                let resp = rpc::handle_project_remove(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PROJECT_CONFIG_SHOW => {
                let resp = rpc::handle_project_config_show(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PROJECT_CONFIG_GET => {
                let resp = rpc::handle_project_config_get(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PROJECT_CONFIG_SET => {
                let resp = rpc::handle_project_config_set(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PROJECT_STATUS => {
                let resp = rpc::handle_project_status(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_AGENT_CREATE => {
                let resp = rpc::handle_agent_create(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_AGENT_LIST => {
                let resp = rpc::handle_agent_list(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_AGENT_ABORT => {
                let resp = rpc::handle_agent_abort(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_AGENT_DELETE => {
                let resp = rpc::handle_agent_delete(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_AGENT_SEND_MESSAGE => {
                let resp = rpc::handle_agent_send_message(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_AGENT_CLAIM => {
                let resp = rpc::handle_agent_claim(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_AGENT_DESCRIBE => {
                let resp = rpc::handle_agent_describe(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_AGENT_CHAT_HISTORY => {
                let resp = rpc::handle_agent_chat_history(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_AGENT_DONE => {
                let resp = rpc::handle_agent_done(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_AGENT_IDLE => {
                let resp = rpc::handle_agent_idle(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_AGENT_SYNC_COMMENTS => {
                let resp = rpc::handle_agent_sync_comments(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ORCHESTRATION_START => {
                let resp = rpc::handle_orchestration_start(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ORCHESTRATION_STOP => {
                let resp = rpc::handle_orchestration_stop(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ORCHESTRATION_STATUS => {
                let resp = rpc::handle_orchestration_status(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_CLAIM_LIST => {
                let resp = rpc::handle_claim_list(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_COMMIT_LIST => {
                let resp = rpc::handle_commit_list(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_STATS => {
                let resp = rpc::handle_stats(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PERMISSION_REQUEST => {
                let resp = rpc::handle_permission_request(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PERMISSION_LIST => {
                let resp = rpc::handle_permission_list(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PERMISSION_RESPOND => {
                let resp = rpc::handle_permission_respond(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_QUESTION_REQUEST => {
                let resp = rpc::handle_question_request(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_QUESTION_LIST => {
                let resp = rpc::handle_question_list(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_QUESTION_RESPOND => {
                let resp = rpc::handle_question_respond(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PLAN_START => {
                let resp = rpc::handle_plan_start(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PLAN_STOP => {
                let resp = rpc::handle_plan_stop(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PLAN_LIST => {
                let resp = rpc::handle_plan_list(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PLAN_SEND_MESSAGE => {
                let resp = rpc::handle_plan_send_message(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PLAN_CHAT_HISTORY => {
                let resp = rpc::handle_plan_chat_history(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_PLAN_SHOW => {
                let resp = rpc::handle_plan_show(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_MANAGER_START => {
                let resp = rpc::handle_manager_start(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_MANAGER_STOP => {
                let resp = rpc::handle_manager_stop(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_MANAGER_STATUS => {
                let resp = rpc::handle_manager_status(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_MANAGER_SEND_MESSAGE => {
                let resp = rpc::handle_manager_send_message(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_MANAGER_CHAT_HISTORY => {
                let resp = rpc::handle_manager_chat_history(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_MANAGER_CLEAR_HISTORY => {
                let resp = rpc::handle_manager_clear_history(shared.clone(), req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ISSUE_LIST => {
                let resp = rpc::handle_issue_list(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ISSUE_GET => {
                let resp = rpc::handle_issue_get(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ISSUE_READY => {
                let resp = rpc::handle_issue_ready(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ISSUE_CREATE => {
                let resp = rpc::handle_issue_create(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ISSUE_UPDATE => {
                let resp = rpc::handle_issue_update(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ISSUE_CLOSE => {
                let resp = rpc::handle_issue_close(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ISSUE_COMMENT => {
                let resp = rpc::handle_issue_comment(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ISSUE_LIST_COMMENTS => {
                let resp = rpc::handle_issue_list_comments(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ISSUE_PLAN => {
                let resp = rpc::handle_issue_plan(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ISSUE_COMMIT => {
                let resp = rpc::handle_issue_commit(&shared, req).await;
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            MSG_ATTACH => {
                if stream_task.is_some() {
                    let _ = out_tx
                        .send(Outbound::Response(rpc::error_response(
                            req,
                            "already attached",
                        )))
                        .await;
                    continue;
                }

                let attach: AttachRequest = serde_json::from_value(req.payload).unwrap_or_default();
                let resp = Response {
                    r#type: MSG_ATTACH.to_owned(),
                    id: req.id,
                    success: true,
                    error: None,
                    payload: serde_json::Value::Null,
                };
                out_tx.send(Outbound::Response(resp)).await?;

                let mut rx = shared.events_tx.subscribe();
                let out_tx_events = out_tx.clone();
                stream_task = Some(tokio::spawn(async move {
                    let filter = attach
                        .projects
                        .into_iter()
                        .map(|p| p.trim().to_owned())
                        .filter(|p| !p.is_empty())
                        .collect::<std::collections::BTreeSet<String>>();
                    loop {
                        match rx.recv().await {
                            Ok(evt) => {
                                if !filter.is_empty() {
                                    let project = evt
                                        .payload
                                        .get("project")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default();
                                    if !filter.contains(project) {
                                        continue;
                                    }
                                }
                                if out_tx_events.send(Outbound::Event(evt)).await.is_err() {
                                    break;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }));
            }
            MSG_DETACH => {
                if let Some(task) = stream_task.take() {
                    task.abort();
                }
                let resp = Response {
                    r#type: MSG_DETACH.to_owned(),
                    id: req.id,
                    success: true,
                    error: None,
                    payload: serde_json::Value::Null,
                };
                let _ = out_tx.send(Outbound::Response(resp)).await;
            }
            other => {
                let _ = out_tx
                    .send(Outbound::Response(rpc::error_response(
                        req,
                        &format!("unknown request type: {other}"),
                    )))
                    .await;
            }
        }
    }

    drop(out_tx);
    if let Some(task) = stream_task.take() {
        task.abort();
    }
    let _ = writer_task.await;

    Ok(())
}

async fn connection_writer<W>(
    mut writer: W,
    mut out_rx: mpsc::Receiver<Outbound>,
    conn_id: u64,
) -> anyhow::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    while let Some(msg) = out_rx.recv().await {
        match msg {
            Outbound::Response(resp) => {
                write_jsonl(&mut writer, &resp).await?;
            }
            Outbound::Event(evt) => {
                write_jsonl(&mut writer, &evt).await?;
            }
        }
    }

    writer.shutdown().await.ok();
    tracing::debug!(conn_id, "connection writer exiting");
    Ok(())
}

pub(super) async fn heartbeat_loop(
    shared: Arc<SharedState>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = tick.tick() => {
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let payload = serde_json::to_value(HeartbeatEvent { now_ms }).unwrap_or(serde_json::Value::Null);
                let id = shared.next_event_id.fetch_add(1, Ordering::Relaxed);
                let _ = shared.events_tx.send(Event {
                    r#type: EVT_HEARTBEAT.to_owned(),
                    id: format!("evt-{id}"),
                    payload,
                });
            }
        }
    }
}

pub(super) async fn shutdown_signal_watcher(handle: DaemonHandle) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = signal(SignalKind::terminate()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = async { if let Some(s) = sigterm.as_mut() { s.recv().await; } } => {},
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }

    handle.request_shutdown();
}
