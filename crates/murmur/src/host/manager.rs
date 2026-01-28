//! Agent manager for host process.
//!
//! Manages a single agent subprocess, buffers its output, and provides
//! methods for the server to interact with the agent.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context as _};
use murmur_core::agent::{AgentRole, AgentState, ChatMessage, ChatRole};
use murmur_core::config::AgentBackend;
use murmur_core::stream::StreamMessage;
use murmur_protocol::host::{HostAgentInfo, StreamChatEntry, StreamEvent};
use tokio::io::{BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, watch, Mutex, RwLock};

use crate::ipc::jsonl::{read_jsonl, write_jsonl};

/// Size of the ring buffer for stream events.
const HISTORY_BUFFER_SIZE: usize = 1000;

/// Configuration for spawning an agent.
pub struct AgentConfig {
    pub agent_id: String,
    pub project: String,
    pub role: AgentRole,
    pub backend: AgentBackend,
    pub worktree: PathBuf,
    pub murmur_dir: PathBuf,
    pub socket_path: PathBuf,
    pub issue_id: Option<String>,
    pub initial_prompt: Option<String>,
    pub append_system_prompt: Option<String>,
}

/// Broadcast sender type for stream events.
pub type EventSender = tokio::sync::broadcast::Sender<StreamEvent>;

/// Manager for a single agent process.
pub struct Manager {
    config: AgentConfig,
    started_at: Instant,

    state: RwLock<AgentState>,
    pid: RwLock<Option<u32>>,
    exit_code: RwLock<Option<i32>>,
    codex_thread_id: RwLock<Option<String>>,
    description: RwLock<Option<String>>,

    stream_offset: AtomicI64,
    history_buffer: Mutex<VecDeque<StreamEvent>>,

    event_tx: EventSender,
    input_tx: mpsc::Sender<String>,
    abort_tx: watch::Sender<bool>,
}

impl Manager {
    /// Create a new manager and spawn the agent process.
    pub async fn spawn(config: AgentConfig) -> anyhow::Result<(Arc<Self>, Vec<tokio::task::JoinHandle<()>>)> {
        let (event_tx, _) = tokio::sync::broadcast::channel(256);
        let (input_tx, input_rx) = mpsc::channel(32);
        let (abort_tx, abort_rx) = watch::channel(false);

        let manager = Arc::new(Self {
            config,
            started_at: Instant::now(),
            state: RwLock::new(AgentState::Starting),
            pid: RwLock::new(None),
            exit_code: RwLock::new(None),
            codex_thread_id: RwLock::new(None),
            description: RwLock::new(None),
            stream_offset: AtomicI64::new(0),
            history_buffer: Mutex::new(VecDeque::with_capacity(HISTORY_BUFFER_SIZE)),
            event_tx,
            input_tx,
            abort_tx,
        });

        let tasks = manager.spawn_agent_process(input_rx, abort_rx).await?;
        Ok((manager, tasks))
    }

    /// Spawn the underlying agent process and I/O tasks.
    async fn spawn_agent_process(
        self: &Arc<Self>,
        input_rx: mpsc::Receiver<String>,
        abort_rx: watch::Receiver<bool>,
    ) -> anyhow::Result<Vec<tokio::task::JoinHandle<()>>> {
        let (child, stdin, stdout, pid) = match self.config.backend {
            AgentBackend::Claude => self.spawn_claude_process().await?,
            AgentBackend::Codex => {
                return Err(anyhow!("codex backend not yet supported in host mode"));
            }
        };

        *self.pid.write().await = Some(pid);
        *self.state.write().await = AgentState::Running;

        self.emit_state_event(AgentState::Running).await;

        let mut tasks = Vec::new();

        tasks.push(tokio::spawn({
            let manager = Arc::clone(self);
            let abort_rx = abort_rx.clone();
            async move {
                manager.stdin_writer(stdin, input_rx, abort_rx).await;
            }
        }));

        tasks.push(tokio::spawn({
            let manager = Arc::clone(self);
            let abort_rx = abort_rx.clone();
            async move {
                manager.stdout_reader(stdout, abort_rx).await;
            }
        }));

        tasks.push(tokio::spawn({
            let manager = Arc::clone(self);
            async move {
                manager.process_reaper(child, abort_rx).await;
            }
        }));

        if let Some(ref prompt) = self.config.initial_prompt {
            let _ = self.input_tx.send(prompt.clone()).await;
        }

        Ok(tasks)
    }

    /// Spawn the claude subprocess.
    async fn spawn_claude_process(
        &self,
    ) -> anyhow::Result<(Child, ChildStdin, ChildStdout, u32)> {
        let hook_exe_prefix = hook_exe_prefix();
        let socket_path_str = self.config.socket_path.to_string_lossy();
        let hook_timeout_sec = 5 * 60;

        let settings = serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": render_shell_command(&[
                            &hook_exe_prefix,
                            "--socket-path",
                            socket_path_str.as_ref(),
                            "hook",
                            "PreToolUse"
                        ]),
                        "timeout": hook_timeout_sec,
                    }]
                }],
                "PermissionRequest": [{
                    "matcher": "*",
                    "hooks": [{
                        "type": "command",
                        "command": render_shell_command(&[
                            &hook_exe_prefix,
                            "--socket-path",
                            socket_path_str.as_ref(),
                            "hook",
                            "PermissionRequest"
                        ]),
                        "timeout": hook_timeout_sec,
                    }]
                }],
                "Stop": [{
                    "hooks": [{
                        "type": "command",
                        "command": render_shell_command(&[
                            &hook_exe_prefix,
                            "--socket-path",
                            socket_path_str.as_ref(),
                            "hook",
                            "Stop"
                        ]),
                        "timeout": 10,
                    }]
                }]
            }
        });

        let is_manager = matches!(self.config.role, AgentRole::Manager | AgentRole::Director);

        let settings_json =
            serde_json::to_string(&settings).context("serialize claude settings")?;

        let mut cmd = tokio::process::Command::new("claude");
        cmd.args([
            "--output-format",
            "stream-json",
            "--input-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "default",
            "--settings",
            &settings_json,
        ])
        .env("MURMUR_AGENT_ID", &self.config.agent_id)
        .env("MURMUR_DIR", &self.config.murmur_dir)
        .env("MURMUR_PROJECT", &self.config.project)
        .env("MURMUR_SOCKET_PATH", &self.config.socket_path)
        .current_dir(&self.config.worktree)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

        if is_manager {
            cmd.env("FUGUE_MANAGER", "1").env("FAB_MANAGER", "1");
        }

        if let Some(ref prompt) = self.config.append_system_prompt {
            let trimmed = prompt.trim();
            if !trimmed.is_empty() {
                cmd.args(["--append-system-prompt", trimmed]);
            }
        }

        let mut child = cmd.spawn().context("spawn claude")?;
        let pid = child.id().ok_or_else(|| anyhow!("claude pid missing"))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("stdin missing"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("stdout missing"))?;

        Ok((child, stdin, stdout, pid))
    }

    /// Write input messages to the agent's stdin.
    async fn stdin_writer(
        &self,
        stdin: ChildStdin,
        mut input_rx: mpsc::Receiver<String>,
        mut abort_rx: watch::Receiver<bool>,
    ) {
        let mut writer = BufWriter::new(stdin);
        loop {
            tokio::select! {
                _ = abort_rx.changed() => {
                    if *abort_rx.borrow() {
                        break;
                    }
                }
                msg = input_rx.recv() => {
                    let Some(content) = msg else { break };
                    let input = murmur_core::stream::InputMessage {
                        r#type: "user".to_owned(),
                        message: murmur_core::stream::MessageBody {
                            role: "user".to_owned(),
                            content,
                        },
                        session_id: "default".to_owned(),
                        parent_tool_use_id: None,
                    };
                    if write_jsonl(&mut writer, &input).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    /// Read output from the agent's stdout.
    async fn stdout_reader(&self, stdout: ChildStdout, mut abort_rx: watch::Receiver<bool>) {
        let mut reader = BufReader::new(stdout);
        loop {
            tokio::select! {
                _ = abort_rx.changed() => {
                    if *abort_rx.borrow() {
                        break;
                    }
                }
                msg = read_jsonl::<_, StreamMessage>(&mut reader) => {
                    match msg {
                        Ok(Some(stream_msg)) => {
                            self.handle_stream_message(stream_msg).await;
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
            }
        }
    }

    /// Wait for the agent process to exit.
    async fn process_reaper(&self, mut child: Child, mut abort_rx: watch::Receiver<bool>) {
        let status = tokio::select! {
            status = child.wait() => status.ok(),
            _ = abort_rx.changed() => {
                if *abort_rx.borrow() {
                    let _ = child.start_kill();
                }
                child.wait().await.ok()
            }
        };

        let code = status.and_then(|s| s.code());
        *self.exit_code.write().await = code;

        let new_state = if *self.abort_tx.borrow() {
            AgentState::Aborted
        } else {
            AgentState::Exited
        };

        *self.state.write().await = new_state;
        *self.pid.write().await = None;

        self.emit_state_event(new_state).await;
    }

    /// Handle a stream message from the agent.
    async fn handle_stream_message(&self, msg: StreamMessage) {
        let now_ms = now_ms();
        let chat_messages = msg.to_chat_messages(now_ms);

        if let Some(ref thread_id) = msg.thread_id {
            *self.codex_thread_id.write().await = Some(thread_id.clone());
        }

        for chat in chat_messages {
            self.emit_chat_event(chat).await;
        }
    }

    /// Emit a state change event.
    async fn emit_state_event(&self, state: AgentState) {
        let event = self.create_event("state", |e| {
            e.state = Some(state_to_string(state));
        });
        self.buffer_and_broadcast(event).await;
    }

    /// Emit a chat event.
    async fn emit_chat_event(&self, msg: ChatMessage) {
        let event = self.create_event("chat", |e| {
            e.chat = Some(StreamChatEntry {
                role: role_to_string(msg.role),
                content: msg.content,
                tool_name: msg.tool_name,
                tool_input: msg.tool_input,
                tool_use_id: msg.tool_use_id,
                tool_result: msg.tool_result,
                is_error: msg.is_error,
                ts_ms: msg.ts_ms,
            });
        });
        self.buffer_and_broadcast(event).await;
    }

    /// Create a stream event with the next offset.
    fn create_event(&self, event_type: &str, f: impl FnOnce(&mut StreamEvent)) -> StreamEvent {
        let offset = self.stream_offset.fetch_add(1, Ordering::SeqCst) + 1;
        let mut event = StreamEvent {
            event_type: event_type.to_owned(),
            agent_id: self.config.agent_id.clone(),
            offset,
            timestamp: chrono_now(),
            data: None,
            state: None,
            chat: None,
        };
        f(&mut event);
        event
    }

    /// Add event to buffer and broadcast to subscribers.
    async fn buffer_and_broadcast(&self, event: StreamEvent) {
        {
            let mut buf = self.history_buffer.lock().await;
            if buf.len() >= HISTORY_BUFFER_SIZE {
                buf.pop_front();
            }
            buf.push_back(event.clone());
        }

        let _ = self.event_tx.send(event);
    }

    /// Send a message to the agent.
    pub async fn send_message(&self, content: &str) -> anyhow::Result<()> {
        self.input_tx
            .send(content.to_owned())
            .await
            .map_err(|_| anyhow!("agent input channel closed"))
    }

    /// Stop the agent.
    pub async fn stop(&self, force: bool, timeout: Duration) -> anyhow::Result<i32> {
        let _ = self.abort_tx.send(true);

        if !force && !timeout.is_zero() {
            tokio::time::sleep(timeout).await;
        }

        let code = self.exit_code.read().await.unwrap_or(-1);
        Ok(code)
    }

    /// Get current agent info.
    pub async fn agent_info(&self) -> HostAgentInfo {
        let state = *self.state.read().await;
        let pid = *self.pid.read().await;
        let exit_code = *self.exit_code.read().await;
        let codex_thread_id = self.codex_thread_id.read().await.clone();
        let description = self.description.read().await.clone();

        HostAgentInfo {
            id: self.config.agent_id.clone(),
            project: self.config.project.clone(),
            state: state_to_string(state),
            pid,
            worktree: self.config.worktree.to_string_lossy().to_string(),
            started_at_ms: self.started_at_ms(),
            task: None,
            description,
            backend: backend_to_string(self.config.backend),
            role: role_enum_to_string(self.config.role),
            issue_id: self.config.issue_id.clone(),
            codex_thread_id,
            exit_code,
        }
    }

    /// Get current stream offset.
    pub fn stream_offset(&self) -> i64 {
        self.stream_offset.load(Ordering::SeqCst)
    }

    /// Get uptime in seconds.
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    /// Get events from buffer starting from offset.
    pub async fn get_buffered_events(&self, from_offset: i64) -> Vec<StreamEvent> {
        let buf = self.history_buffer.lock().await;
        buf.iter()
            .filter(|e| e.offset > from_offset)
            .cloned()
            .collect()
    }

    /// Subscribe to new events.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<StreamEvent> {
        self.event_tx.subscribe()
    }

    /// Check if agent has exited.
    pub async fn is_exited(&self) -> bool {
        let state = *self.state.read().await;
        matches!(state, AgentState::Exited | AgentState::Aborted)
    }

    fn started_at_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
            .saturating_sub(self.started_at.elapsed().as_millis() as u64)
    }
}

fn state_to_string(state: AgentState) -> String {
    match state {
        AgentState::Starting => "starting",
        AgentState::Running => "running",
        AgentState::Idle => "idle",
        AgentState::NeedsResolution => "needs_resolution",
        AgentState::Exited => "exited",
        AgentState::Aborted => "aborted",
    }
    .to_owned()
}

fn role_to_string(role: ChatRole) -> String {
    match role {
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
        ChatRole::Tool => "tool",
        ChatRole::System => "system",
    }
    .to_owned()
}

fn role_enum_to_string(role: AgentRole) -> String {
    match role {
        AgentRole::Coding => "coding",
        AgentRole::Planner => "planner",
        AgentRole::Manager => "manager",
        AgentRole::Director => "director",
    }
    .to_owned()
}

fn backend_to_string(backend: AgentBackend) -> String {
    match backend {
        AgentBackend::Claude => "claude",
        AgentBackend::Codex => "codex",
    }
    .to_owned()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn chrono_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

/// Get the mm binary path prefix for hooks.
fn hook_exe_prefix() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("mm")))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "mm".to_owned())
}

/// Render a shell command for hook configuration.
fn render_shell_command(args: &[&str]) -> String {
    args.iter()
        .map(|a| {
            if a.contains(' ') || a.contains('"') || a.contains('\'') {
                format!("\"{}\"", a.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                (*a).to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
