//! Unix socket server for agent host.
//!
//! Accepts connections from the daemon and handles protocol requests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use murmur_protocol::host::{
    self, AttachRequest, AttachResponse, HostRequest, HostResponse, ListResponse, PingResponse,
    SendRequest, StatusResponse, StopRequest, StopResponse, StreamEvent, HOST_PROTOCOL_VERSION,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{watch, Mutex};

use super::manager::Manager;

/// Server for handling daemon connections.
pub struct Server {
    socket_path: PathBuf,
    manager: Arc<Manager>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    attached_clients: AtomicUsize,
    next_conn_id: AtomicU64,
}

impl Server {
    /// Create a new server.
    pub fn new(socket_path: PathBuf, manager: Arc<Manager>) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Self {
            socket_path,
            manager,
            shutdown_tx,
            shutdown_rx,
            attached_clients: AtomicUsize::new(0),
            next_conn_id: AtomicU64::new(1),
        }
    }

    /// Run the server, accepting connections until shutdown.
    pub async fn run(&self) -> anyhow::Result<()> {
        if self.socket_path.exists() {
            tokio::fs::remove_file(&self.socket_path)
                .await
                .context("remove stale socket")?;
        }

        if let Some(parent) = self.socket_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("create socket directory")?;
        }

        let listener = UnixListener::bind(&self.socket_path).context("bind socket")?;
        tracing::info!(socket = %self.socket_path.display(), "host server listening");

        let mut shutdown_rx = self.shutdown_rx.clone();
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let conn_id = self.next_conn_id.fetch_add(1, Ordering::Relaxed);
                            let manager = Arc::clone(&self.manager);
                            let shutdown_tx = self.shutdown_tx.clone();
                            let shutdown_rx = self.shutdown_rx.clone();

                            tokio::spawn(handle_connection(
                                conn_id,
                                stream,
                                manager,
                                shutdown_tx,
                                shutdown_rx,
                            ));
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "accept failed");
                        }
                    }
                }
            }
        }

        self.cleanup().await;
        Ok(())
    }

    /// Request server shutdown.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Get number of attached clients.
    pub fn attached_count(&self) -> usize {
        self.attached_clients.load(Ordering::Relaxed)
    }

    async fn cleanup(&self) {
        if self.socket_path.exists() {
            let _ = tokio::fs::remove_file(&self.socket_path).await;
        }
    }
}

async fn handle_connection(
    conn_id: u64,
    stream: UnixStream,
    manager: Arc<Manager>,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    tracing::debug!(conn_id, "new connection");

    let (reader, writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let writer = Arc::new(Mutex::new(BufWriter::new(writer)));
    let mut line = String::new();
    let mut attached = false;
    let mut event_rx: Option<tokio::sync::broadcast::Receiver<StreamEvent>> = None;

    loop {
        line.clear();

        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }

            event = async {
                if let Some(ref mut rx) = event_rx {
                    rx.recv().await.ok()
                } else {
                    std::future::pending::<Option<StreamEvent>>().await
                }
            } => {
                if let Some(event) = event {
                    let mut w = writer.lock().await;
                    if let Ok(json) = serde_json::to_string(&event) {
                        let _ = w.write_all(json.as_bytes()).await;
                        let _ = w.write_all(b"\n").await;
                        let _ = w.flush().await;
                    }
                }
            }

            result = reader.read_line(&mut line) => {
                match result {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        let request: HostRequest = match serde_json::from_str(trimmed) {
                            Ok(r) => r,
                            Err(err) => {
                                tracing::debug!(error = %err, "invalid request");
                                continue;
                            }
                        };

                        let response = handle_request(&request, &manager, &shutdown_tx, &mut attached, &mut event_rx).await;

                        let mut w = writer.lock().await;
                        if let Ok(json) = serde_json::to_string(&response) {
                            let _ = w.write_all(json.as_bytes()).await;
                            let _ = w.write_all(b"\n").await;
                            let _ = w.flush().await;
                        }
                    }
                    Err(err) => {
                        tracing::debug!(conn_id, error = %err, "read error");
                        break;
                    }
                }
            }
        }
    }

    tracing::debug!(conn_id, "connection closed");
}

async fn handle_request(
    req: &HostRequest,
    manager: &Arc<Manager>,
    shutdown_tx: &watch::Sender<bool>,
    attached: &mut bool,
    event_rx: &mut Option<tokio::sync::broadcast::Receiver<StreamEvent>>,
) -> HostResponse {
    match req.msg_type.as_str() {
        host::msg::PING => handle_ping(req, manager).await,
        host::msg::STATUS => handle_status(req, manager).await,
        host::msg::LIST => handle_list(req, manager).await,
        host::msg::ATTACH => handle_attach(req, manager, attached, event_rx).await,
        host::msg::DETACH => handle_detach(req, attached, event_rx).await,
        host::msg::SEND => handle_send(req, manager).await,
        host::msg::STOP => handle_stop(req, manager, shutdown_tx).await,
        _ => HostResponse::err(&req.msg_type, &req.id, "unknown message type"),
    }
}

async fn handle_ping(req: &HostRequest, manager: &Arc<Manager>) -> HostResponse {
    let payload = PingResponse {
        version: HOST_PROTOCOL_VERSION.to_owned(),
        uptime_secs: manager.uptime_secs(),
    };
    HostResponse::ok_with_payload(&req.msg_type, &req.id, &payload)
}

async fn handle_status(req: &HostRequest, manager: &Arc<Manager>) -> HostResponse {
    let agent = manager.agent_info().await;
    let payload = StatusResponse {
        agent,
        stream_offset: manager.stream_offset(),
        attached_clients: 0, // TODO: track properly
    };
    HostResponse::ok_with_payload(&req.msg_type, &req.id, &payload)
}

async fn handle_list(req: &HostRequest, manager: &Arc<Manager>) -> HostResponse {
    let agent = manager.agent_info().await;
    let payload = ListResponse {
        agents: vec![agent],
    };
    HostResponse::ok_with_payload(&req.msg_type, &req.id, &payload)
}

async fn handle_attach(
    req: &HostRequest,
    manager: &Arc<Manager>,
    attached: &mut bool,
    event_rx: &mut Option<tokio::sync::broadcast::Receiver<StreamEvent>>,
) -> HostResponse {
    let _attach_req: AttachRequest = req
        .payload
        .as_ref()
        .and_then(|p| serde_json::from_value(p.clone()).ok())
        .unwrap_or(AttachRequest { offset: 0 });

    *attached = true;
    *event_rx = Some(manager.subscribe());

    let payload = AttachResponse {
        current_offset: manager.stream_offset(),
    };

    // Note: Buffered events from offset should be sent after the response.
    // The caller can request them separately or we send them on the stream.
    // For simplicity, we'll rely on the daemon to request history if needed.

    HostResponse::ok_with_payload(&req.msg_type, &req.id, &payload)
}

async fn handle_detach(
    req: &HostRequest,
    attached: &mut bool,
    event_rx: &mut Option<tokio::sync::broadcast::Receiver<StreamEvent>>,
) -> HostResponse {
    *attached = false;
    *event_rx = None;
    HostResponse::ok(&req.msg_type, &req.id)
}

async fn handle_send(req: &HostRequest, manager: &Arc<Manager>) -> HostResponse {
    let send_req: SendRequest = match req
        .payload
        .as_ref()
        .and_then(|p| serde_json::from_value(p.clone()).ok())
    {
        Some(r) => r,
        None => {
            return HostResponse::err(&req.msg_type, &req.id, "missing or invalid payload");
        }
    };

    match manager.send_message(&send_req.input).await {
        Ok(()) => HostResponse::ok(&req.msg_type, &req.id),
        Err(err) => HostResponse::err(&req.msg_type, &req.id, err.to_string()),
    }
}

async fn handle_stop(
    req: &HostRequest,
    manager: &Arc<Manager>,
    shutdown_tx: &watch::Sender<bool>,
) -> HostResponse {
    let stop_req: StopRequest = req
        .payload
        .as_ref()
        .and_then(|p| serde_json::from_value(p.clone()).ok())
        .unwrap_or(StopRequest {
            force: false,
            timeout_secs: 30,
            reason: None,
        });

    let timeout = Duration::from_secs(stop_req.timeout_secs as u64);
    match manager.stop(stop_req.force, timeout).await {
        Ok(exit_code) => {
            let _ = shutdown_tx.send(true);
            let payload = StopResponse {
                stopped: true,
                exit_code: Some(exit_code),
            };
            HostResponse::ok_with_payload(&req.msg_type, &req.id, &payload)
        }
        Err(err) => HostResponse::err(&req.msg_type, &req.id, err.to_string()),
    }
}
