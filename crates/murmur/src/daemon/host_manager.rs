//! Host Manager for the daemon.
//!
//! Manages connections to agent host processes and provides methods for
//! spawning agents, sending messages, and receiving events.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context as _};
use murmur_core::agent::AgentRole;
use murmur_core::config::AgentBackend;
use murmur_protocol::host::{
    self, HostAgentInfo, HostRequest, HostResponse, PingResponse, SendRequest, StatusResponse,
    StopRequest, StopResponse,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::sync::{Mutex, RwLock};

/// Configuration for spawning an agent via host.
#[derive(Debug, Clone)]
pub struct AgentSpawnConfig {
    pub agent_id: String,
    pub project: String,
    pub role: AgentRole,
    pub backend: AgentBackend,
    pub worktree: PathBuf,
    pub issue_id: Option<String>,
    pub initial_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
}

/// Client connection to a single agent host.
struct HostClient {
    agent_id: String,
    socket_path: PathBuf,
    stream: Mutex<Option<UnixStream>>,
}

impl HostClient {
    fn new(agent_id: String, socket_path: PathBuf) -> Self {
        Self {
            agent_id,
            socket_path,
            stream: Mutex::new(None),
        }
    }

    async fn connect(&self) -> anyhow::Result<()> {
        tracing::debug!(agent_id = %self.agent_id, socket = %self.socket_path.display(), "connecting to host");

        let stream = tokio::time::timeout(
            Duration::from_secs(5),
            UnixStream::connect(&self.socket_path),
        )
        .await
        .context("connect timeout")?
        .context("connect failed")?;

        *self.stream.lock().await = Some(stream);
        tracing::debug!(agent_id = %self.agent_id, "connected to host");
        Ok(())
    }

    async fn disconnect(&self) {
        *self.stream.lock().await = None;
    }

    async fn is_connected(&self) -> bool {
        self.stream.lock().await.is_some()
    }

    async fn send_request(&self, req: &HostRequest) -> anyhow::Result<HostResponse> {
        let mut guard = self.stream.lock().await;
        let stream = guard.as_mut().ok_or_else(|| anyhow!("not connected"))?;

        let json = serde_json::to_string(req).context("serialize request")?;

        stream
            .write_all(json.as_bytes())
            .await
            .context("write request")?;
        stream.write_all(b"\n").await.context("write newline")?;
        stream.flush().await.context("flush")?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();

        tokio::time::timeout(Duration::from_secs(30), reader.read_line(&mut line))
            .await
            .context("read timeout")?
            .context("read response")?;

        let resp: HostResponse =
            serde_json::from_str(line.trim()).context("parse response")?;

        Ok(resp)
    }

    async fn ping(&self) -> anyhow::Result<PingResponse> {
        let req = HostRequest {
            msg_type: host::msg::PING.to_owned(),
            id: "ping-1".to_owned(),
            payload: None,
        };
        let resp = self.send_request(&req).await?;
        if !resp.success {
            anyhow::bail!("ping failed: {:?}", resp.error);
        }
        let payload: PingResponse = resp
            .payload
            .and_then(|p| serde_json::from_value(p).ok())
            .ok_or_else(|| anyhow!("missing ping payload"))?;
        Ok(payload)
    }

    async fn status(&self) -> anyhow::Result<StatusResponse> {
        let req = HostRequest {
            msg_type: host::msg::STATUS.to_owned(),
            id: "status-1".to_owned(),
            payload: None,
        };
        let resp = self.send_request(&req).await?;
        if !resp.success {
            anyhow::bail!("status failed: {:?}", resp.error);
        }
        let payload: StatusResponse = resp
            .payload
            .and_then(|p| serde_json::from_value(p).ok())
            .ok_or_else(|| anyhow!("missing status payload"))?;
        Ok(payload)
    }

    async fn send_message(&self, content: &str) -> anyhow::Result<()> {
        let req = HostRequest {
            msg_type: host::msg::SEND.to_owned(),
            id: "send-1".to_owned(),
            payload: Some(serde_json::to_value(SendRequest {
                input: content.to_owned(),
            })?),
        };
        let resp = self.send_request(&req).await?;
        if !resp.success {
            anyhow::bail!("send failed: {:?}", resp.error);
        }
        Ok(())
    }

    async fn stop(&self, force: bool, timeout_secs: u32) -> anyhow::Result<StopResponse> {
        let req = HostRequest {
            msg_type: host::msg::STOP.to_owned(),
            id: "stop-1".to_owned(),
            payload: Some(serde_json::to_value(StopRequest {
                force,
                timeout_secs,
                reason: None,
            })?),
        };
        let resp = self.send_request(&req).await?;
        if !resp.success {
            anyhow::bail!("stop failed: {:?}", resp.error);
        }
        let payload: StopResponse = resp
            .payload
            .and_then(|p| serde_json::from_value(p).ok())
            .ok_or_else(|| anyhow!("missing stop payload"))?;
        Ok(payload)
    }
}

/// Manager for agent host processes.
pub struct HostManager {
    hosts_dir: PathBuf,
    murmur_dir: PathBuf,
    daemon_socket: PathBuf,
    clients: RwLock<HashMap<String, Arc<HostClient>>>,
}

impl HostManager {
    /// Create a new host manager.
    pub fn new(hosts_dir: PathBuf, murmur_dir: PathBuf, daemon_socket: PathBuf) -> Self {
        Self {
            hosts_dir,
            murmur_dir,
            daemon_socket,
            clients: RwLock::new(HashMap::new()),
        }
    }

    /// Spawn a new agent via murmur-host.
    pub async fn spawn_agent(&self, config: AgentSpawnConfig) -> anyhow::Result<HostAgentInfo> {
        tokio::fs::create_dir_all(&self.hosts_dir)
            .await
            .context("create hosts directory")?;

        let socket_path = self.hosts_dir.join(format!("{}.sock", config.agent_id));

        if socket_path.exists() {
            tokio::fs::remove_file(&socket_path)
                .await
                .context("remove stale socket")?;
        }

        let murmur_host_bin = self.find_murmur_host_binary()?;

        let mut cmd = Command::new(&murmur_host_bin);
        cmd.arg("--agent-id")
            .arg(&config.agent_id)
            .arg("--project")
            .arg(&config.project)
            .arg("--role")
            .arg(role_to_string(config.role))
            .arg("--backend")
            .arg(backend_to_string(config.backend))
            .arg("--worktree")
            .arg(&config.worktree)
            .arg("--socket-dir")
            .arg(&self.hosts_dir)
            .arg("--murmur-dir")
            .arg(&self.murmur_dir)
            .arg("--daemon-socket")
            .arg(&self.daemon_socket);

        if let Some(ref issue_id) = config.issue_id {
            cmd.arg("--issue-id").arg(issue_id);
        }
        if let Some(ref prompt) = config.initial_prompt {
            cmd.arg("--initial-prompt").arg(prompt);
        }
        if let Some(ref prompt) = config.append_system_prompt {
            cmd.arg("--append-system-prompt").arg(prompt);
        }

        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        let _child = cmd.spawn().context("spawn murmur-host")?;

        self.wait_for_socket(&socket_path).await?;

        let client = Arc::new(HostClient::new(config.agent_id.clone(), socket_path));
        client.connect().await?;

        let status = client.status().await?;

        self.clients
            .write()
            .await
            .insert(config.agent_id.clone(), client);

        Ok(status.agent)
    }

    /// Find the murmur-host binary.
    fn find_murmur_host_binary(&self) -> anyhow::Result<PathBuf> {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let candidate = dir.join("murmur-host");
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }

        let candidate = PathBuf::from("murmur-host");
        if which::which(&candidate).is_ok() {
            return Ok(candidate);
        }

        anyhow::bail!("murmur-host binary not found")
    }

    /// Wait for the host socket to appear.
    async fn wait_for_socket(&self, socket_path: &Path) -> anyhow::Result<()> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if socket_path.exists() {
                return Ok(());
            }
            if tokio::time::Instant::now() > deadline {
                anyhow::bail!("timeout waiting for host socket");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Send a message to an agent.
    pub async fn send_message(&self, agent_id: &str, content: &str) -> anyhow::Result<()> {
        let clients = self.clients.read().await;
        let client = clients
            .get(agent_id)
            .ok_or_else(|| anyhow!("agent not found: {}", agent_id))?;

        if !client.is_connected().await {
            client.connect().await.context("reconnect to host")?;
        }

        client.send_message(content).await
    }

    /// Stop an agent.
    pub async fn stop_agent(&self, agent_id: &str, force: bool) -> anyhow::Result<StopResponse> {
        let client = {
            let clients = self.clients.read().await;
            Arc::clone(
                clients
                    .get(agent_id)
                    .ok_or_else(|| anyhow!("agent not found: {}", agent_id))?,
            )
        };

        let response = client.stop(force, 30).await?;
        client.disconnect().await;

        self.clients.write().await.remove(agent_id);
        Ok(response)
    }

    /// Get status for an agent.
    pub async fn status(&self, agent_id: &str) -> anyhow::Result<StatusResponse> {
        let clients = self.clients.read().await;
        let client = clients
            .get(agent_id)
            .ok_or_else(|| anyhow!("agent not found: {}", agent_id))?;
        client.status().await
    }

    /// Discover and reconnect to running hosts.
    pub async fn discover_and_reconnect(&self) -> anyhow::Result<Vec<HostAgentInfo>> {
        let mut discovered = Vec::new();

        if !self.hosts_dir.exists() {
            return Ok(discovered);
        }

        let mut entries = tokio::fs::read_dir(&self.hosts_dir)
            .await
            .context("read hosts directory")?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("sock") {
                continue;
            }

            match self.probe_host(&path).await {
                Ok(status) => {
                    let agent_id = status.agent.id.clone();
                    let client = Arc::new(HostClient::new(agent_id.clone(), path));

                    if let Err(err) = client.connect().await {
                        tracing::debug!(
                            agent_id = %agent_id,
                            error = %err,
                            "failed to connect to host"
                        );
                        continue;
                    }

                    self.clients.write().await.insert(agent_id, client);
                    discovered.push(status.agent);
                }
                Err(err) => {
                    tracing::debug!(
                        path = %path.display(),
                        error = %err,
                        "removing stale socket"
                    );
                    let _ = tokio::fs::remove_file(&path).await;
                }
            }
        }

        Ok(discovered)
    }

    /// Probe a host socket to check if it's alive.
    async fn probe_host(&self, socket_path: &Path) -> anyhow::Result<StatusResponse> {
        let stream = tokio::time::timeout(
            Duration::from_secs(2),
            UnixStream::connect(socket_path),
        )
        .await
        .context("connect timeout")?
        .context("connect failed")?;

        let client = HostClient::new(String::new(), socket_path.to_path_buf());
        *client.stream.lock().await = Some(stream);

        // Quick health check first
        let _ping = client.ping().await.context("ping failed")?;

        client.status().await
    }

    /// Get list of connected agents.
    pub async fn list_agents(&self) -> Vec<String> {
        self.clients.read().await.keys().cloned().collect()
    }

    /// Check if an agent is managed by host.
    pub async fn has_agent(&self, agent_id: &str) -> bool {
        self.clients.read().await.contains_key(agent_id)
    }
}

fn role_to_string(role: AgentRole) -> &'static str {
    match role {
        AgentRole::Coding => "coding",
        AgentRole::Planner => "planner",
        AgentRole::Manager => "manager",
        AgentRole::Director => "director",
    }
}

fn backend_to_string(backend: AgentBackend) -> &'static str {
    match backend {
        AgentBackend::Claude => "claude",
        AgentBackend::Codex => "codex",
    }
}
