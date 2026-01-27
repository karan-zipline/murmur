use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context as _};
use murmur_core::agent::{
    AgentEvent, AgentRecord, AgentRole, AgentState, ChatHistory, ChatMessage, ChatRole,
};
use murmur_core::claims::ClaimRegistry;
use murmur_core::config::AgentBackend;
use murmur_core::paths::MurmurPaths;
use murmur_core::stream::{InputMessage, MessageBody, StreamMessage};
use murmur_protocol::{
    AgentChatEvent, AgentCreatedEvent, Event, EVT_AGENT_CHAT, EVT_AGENT_CREATED,
};
use tokio::io::{BufReader, BufWriter};
use tokio::sync::{broadcast, mpsc, watch};

use crate::config_store;
use crate::git::Git;
use crate::ipc::jsonl::{read_jsonl, write_jsonl};
use crate::runtime_store;
use crate::worktrees::WorktreeManager;

mod claude;
mod issue_backend;
mod merge;
mod orchestration;
mod prompts;
mod proto;
mod rpc;
mod server;
mod state;
mod webhook;

use issue_backend::issue_backend_for_project;
use proto::{
    agent_info_from_record, from_proto_issue_status, to_proto_chat_message, to_proto_issue,
    to_proto_issue_summary,
};
use state::{AgentRuntime, AgentsState, SharedState, DEFAULT_CHAT_CAPACITY};

#[derive(Clone)]
pub struct DaemonHandle {
    shutdown_tx: watch::Sender<bool>,
}

impl DaemonHandle {
    pub fn request_shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.shutdown_tx.subscribe()
    }
}

pub struct DaemonRuntime {
    pub handle: DaemonHandle,
    pub local_addr: String,
}

pub async fn run_foreground(paths: &MurmurPaths) -> anyhow::Result<()> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let handle = DaemonHandle { shutdown_tx };

    let socket = server::bind_socket(&paths.socket_path).await?;

    let pid = std::process::id();
    let started_at = SystemTime::now();
    let started_at_instant = Instant::now();

    let (events_tx, _) = broadcast::channel::<Event>(1024);

    let config = config_store::load(paths).await?;
    let git = Git::default();
    let next_agent_id = detect_next_agent_id_seed(paths, &git).await;
    let next_plan_id = detect_next_plan_id_seed(paths).await;

    let shared = Arc::new(SharedState {
        pid,
        started_at,
        started_at_instant,
        events_tx,
        shutdown: handle.clone(),
        next_event_id: AtomicU64::new(1),
        next_conn_id: AtomicU64::new(1),
        next_agent_id: AtomicU64::new(next_agent_id),
        next_plan_id: AtomicU64::new(next_plan_id),
        paths: paths.clone(),
        git,
        config: tokio::sync::Mutex::new(config),
        agents: tokio::sync::Mutex::new(AgentsState::default()),
        claims: tokio::sync::Mutex::new(ClaimRegistry::default()),
        pending_permissions: tokio::sync::Mutex::new(state::PendingPermissions::default()),
        pending_questions: tokio::sync::Mutex::new(state::PendingQuestions::default()),
        completed_issues: tokio::sync::Mutex::new(std::collections::BTreeMap::new()),
        orchestrators: tokio::sync::Mutex::new(std::collections::BTreeMap::new()),
        merge_locks: tokio::sync::Mutex::new(std::collections::BTreeMap::new()),
        commits: tokio::sync::Mutex::new(std::collections::BTreeMap::new()),
    });

    // Restore agents from disk so that agents from previous sessions are recognized
    if let Err(err) = rehydrate_agents(shared.clone()).await {
        tracing::warn!(error = %err, "failed to rehydrate agents from disk");
    }

    tokio::spawn(server::heartbeat_loop(shared.clone(), shutdown_rx.clone()));
    tokio::spawn(server::shutdown_signal_watcher(handle.clone()));
    tokio::spawn({
        let shared = shared.clone();
        let shutdown_rx = shutdown_rx.clone();
        async move {
            if let Err(err) = webhook::maybe_start_webhook_server(shared, shutdown_rx).await {
                tracing::warn!(error = %err, "webhook server failed");
            }
        }
    });

    tracing::info!("daemon starting (foreground)");
    tracing::info!(socket = %paths.socket_path.display(), "daemon bound socket");
    println!("ready");
    tracing::info!("daemon ready");

    server::accept_loop(socket, shared, shutdown_rx).await;

    server::cleanup_socket(&paths.socket_path).await;
    tracing::info!("daemon shutting down");
    Ok(())
}

async fn spawn_agent(
    shared: Arc<SharedState>,
    project: String,
    issue_id: String,
    backend_override: Option<AgentBackend>,
) -> anyhow::Result<AgentRecord> {
    spawn_agent_with_kickoff(shared, project, issue_id, None, backend_override).await
}

/// Spawn an agent without pre-assigning an issue.
/// The agent will use the kickstart prompt to find and claim issues itself.
async fn spawn_agent_without_issue(
    shared: Arc<SharedState>,
    project: String,
    kickoff_message: String,
) -> anyhow::Result<AgentRecord> {
    let agent_num = shared.next_agent_id.fetch_add(1, Ordering::Relaxed);
    let agent_id = format!("a-{agent_num}");

    // No claim here - agent will claim an issue after being spawned

    let wtm = WorktreeManager::new(&shared.git, &shared.paths);
    let wt = match wtm.create_agent_worktree(&project, &agent_id).await {
        Ok(wt) => wt,
        Err(err) => {
            return Err(err).with_context(|| format!("create worktree for agent {agent_id}"));
        }
    };

    let created_at_ms = now_ms();

    let backend = {
        let cfg = shared.config.lock().await;
        cfg.project(&project)
            .map(|p| p.effective_coding_backend())
            .unwrap_or_default()
    };

    // Empty issue_id - agent will claim one via mm agent claim
    let record = AgentRecord::new(
        agent_id.clone(),
        project.clone(),
        AgentRole::Coding,
        String::new(),
        created_at_ms,
        wt.dir.to_string_lossy().to_string(),
    );

    let (outbound_tx, outbound_rx) = mpsc::channel::<ChatMessage>(32);
    let (abort_tx, abort_rx) = watch::channel(false);

    // CRITICAL: Register agent BEFORE spawning process to avoid race condition
    // where agent tries to call `mm agent claim` before it's registered.
    {
        let mut agents = shared.agents.lock().await;
        agents.agents.insert(
            agent_id.clone(),
            AgentRuntime {
                record: record.clone(),
                backend,
                codex_thread_id: None,
                chat: ChatHistory::new(DEFAULT_CHAT_CAPACITY),
                last_idle_at_ms: None,
                outbound_tx: outbound_tx.clone(),
                abort_tx: abort_tx.clone(),
                tasks: Vec::new(),
            },
        );
    }

    // Emit agent created event so TUI can refresh its agent list
    emit_agent_created_event(shared.as_ref(), &agent_info_from_record(&record, backend));

    // NOW spawn the process - agent is already registered
    let mut pending_claude = None;
    let mut record = record;
    if backend == AgentBackend::Claude {
        let (child, stdin, stdout, pid) = match spawn_claude_agent_process(
            &agent_id,
            &project,
            &wt.dir,
            &shared.paths.murmur_dir,
            &shared.paths.socket_path,
            None,
            false,
            None,
        )
        .await
        {
            Ok(v) => v,
            Err(err) => {
                // Cleanup: remove agent registration on spawn failure
                {
                    let mut agents = shared.agents.lock().await;
                    agents.agents.remove(&agent_id);
                }
                let _ = wtm.remove_worktree(&project, &wt.dir).await;
                return Err(err).with_context(|| format!("spawn claude agent {agent_id}"));
            }
        };

        record = record.apply_event(AgentEvent::Spawned { pid }, created_at_ms);
        pending_claude = Some((child, stdin, stdout));

        // Update record with PID
        {
            let mut agents = shared.agents.lock().await;
            if let Some(rt) = agents.agents.get_mut(&agent_id) {
                rt.record = record.clone();
            }
        }
    }

    let mut tasks = Vec::new();
    match backend {
        AgentBackend::Claude => {
            let Some((child, stdin, stdout)) = pending_claude else {
                return Err(anyhow!("claude process missing after spawn"));
            };
            tasks.push(tokio::spawn(claude_stdin_writer(
                outbound_rx,
                stdin,
                abort_rx.clone(),
            )));
            tasks.push(tokio::spawn(claude_stdout_reader(
                shared.clone(),
                agent_id.clone(),
                stdout,
                abort_rx.clone(),
            )));
            tasks.push(tokio::spawn(claude_reaper(
                shared.clone(),
                agent_id.clone(),
                child,
                abort_rx.clone(),
            )));
        }
        AgentBackend::Codex => {
            tasks.push(tokio::spawn(codex_worker(
                shared.clone(),
                agent_id.clone(),
                wt.dir.clone(),
                outbound_rx,
                abort_rx.clone(),
            )));
        }
    }

    {
        let mut agents = shared.agents.lock().await;
        if let Some(rt) = agents.agents.get_mut(&agent_id) {
            rt.tasks = tasks;
        }
    }

    // Send the kickstart message to the agent, but do not store or emit it.
    // (We want the TUI chat view to start at the first agent response.)
    let msg = ChatMessage::new(ChatRole::User, kickoff_message, now_ms());
    let _ = outbound_tx.send(msg).await;

    persist_agents_runtime(shared).await;
    Ok(record)
}

async fn spawn_agent_with_kickoff(
    shared: Arc<SharedState>,
    project: String,
    issue_id: String,
    kickoff_message: Option<String>,
    backend_override: Option<AgentBackend>,
) -> anyhow::Result<AgentRecord> {
    let agent_num = shared.next_agent_id.fetch_add(1, Ordering::Relaxed);
    let agent_id = format!("a-{agent_num}");

    {
        let mut claims = shared.claims.lock().await;
        let next = claims.claim(&project, &issue_id, &agent_id)?;
        *claims = next;
    }

    let wtm = WorktreeManager::new(&shared.git, &shared.paths);
    let wt = match wtm.create_agent_worktree(&project, &agent_id).await {
        Ok(wt) => wt,
        Err(err) => {
            release_claim(&shared, &project, &issue_id).await;
            return Err(err).with_context(|| format!("create worktree for agent {agent_id}"));
        }
    };

    let created_at_ms = now_ms();

    let backend = match backend_override {
        Some(v) => v,
        None => {
            let cfg = shared.config.lock().await;
            cfg.project(&project)
                .map(|p| p.effective_coding_backend())
                .unwrap_or_default()
        }
    };

    let record = AgentRecord::new(
        agent_id.clone(),
        project.clone(),
        AgentRole::Coding,
        issue_id.clone(),
        created_at_ms,
        wt.dir.to_string_lossy().to_string(),
    );

    let (outbound_tx, outbound_rx) = mpsc::channel::<ChatMessage>(32);
    let (abort_tx, abort_rx) = watch::channel(false);

    // CRITICAL: Register agent BEFORE spawning process to avoid race condition
    // where agent tries to call `mm agent claim` before it's registered.
    {
        let mut agents = shared.agents.lock().await;
        agents.agents.insert(
            agent_id.clone(),
            AgentRuntime {
                record: record.clone(),
                backend,
                codex_thread_id: None,
                chat: ChatHistory::new(DEFAULT_CHAT_CAPACITY),
                last_idle_at_ms: None,
                outbound_tx: outbound_tx.clone(),
                abort_tx: abort_tx.clone(),
                tasks: Vec::new(),
            },
        );
    }

    // Emit agent created event so TUI can refresh its agent list
    emit_agent_created_event(shared.as_ref(), &agent_info_from_record(&record, backend));

    // NOW spawn the process - agent is already registered
    let mut pending_claude = None;
    let mut record = record;
    if backend == AgentBackend::Claude {
        let (child, stdin, stdout, pid) = match spawn_claude_agent_process(
            &agent_id,
            &project,
            &wt.dir,
            &shared.paths.murmur_dir,
            &shared.paths.socket_path,
            None,
            false,
            None,
        )
        .await
        {
            Ok(v) => v,
            Err(err) => {
                // Cleanup: remove agent registration on spawn failure
                {
                    let mut agents = shared.agents.lock().await;
                    agents.agents.remove(&agent_id);
                }
                let _ = wtm.remove_worktree(&project, &wt.dir).await;
                release_claim(&shared, &project, &issue_id).await;
                return Err(err).with_context(|| format!("spawn claude agent {agent_id}"));
            }
        };

        record = record.apply_event(AgentEvent::Spawned { pid }, created_at_ms);
        pending_claude = Some((child, stdin, stdout));

        // Update record with PID
        {
            let mut agents = shared.agents.lock().await;
            if let Some(rt) = agents.agents.get_mut(&agent_id) {
                rt.record = record.clone();
            }
        }
    }

    let mut tasks = Vec::new();
    match backend {
        AgentBackend::Claude => {
            let Some((child, stdin, stdout)) = pending_claude else {
                return Err(anyhow!("claude process missing after spawn"));
            };
            tasks.push(tokio::spawn(claude_stdin_writer(
                outbound_rx,
                stdin,
                abort_rx.clone(),
            )));
            tasks.push(tokio::spawn(claude_stdout_reader(
                shared.clone(),
                agent_id.clone(),
                stdout,
                abort_rx.clone(),
            )));
            tasks.push(tokio::spawn(claude_reaper(
                shared.clone(),
                agent_id.clone(),
                child,
                abort_rx.clone(),
            )));
        }
        AgentBackend::Codex => {
            tasks.push(tokio::spawn(codex_worker(
                shared.clone(),
                agent_id.clone(),
                wt.dir.clone(),
                outbound_rx,
                abort_rx.clone(),
            )));
        }
    }

    {
        let mut agents = shared.agents.lock().await;
        if let Some(rt) = agents.agents.get_mut(&agent_id) {
            rt.tasks = tasks;
        }
    }

    if let Some(message) = kickoff_message {
        let msg = ChatMessage::new(ChatRole::User, message, now_ms());
        let _ = outbound_tx.send(msg).await;
    }

    persist_agents_runtime(shared).await;
    Ok(record)
}

async fn release_claim(shared: &SharedState, project: &str, issue_id: &str) {
    let mut claims = shared.claims.lock().await;
    *claims = claims.release(project, issue_id);
}

async fn release_claims_for_agent(shared: &SharedState, agent_id: &str) {
    let mut claims = shared.claims.lock().await;
    *claims = claims.release_by_agent(agent_id);
}

async fn mark_issue_completed(shared: &SharedState, project: &str, issue_id: &str) {
    let mut completed = shared.completed_issues.lock().await;
    completed
        .entry(project.to_owned())
        .or_default()
        .insert(issue_id.to_owned());
}

async fn cleanup_agent_runtime(
    shared: Arc<SharedState>,
    mut runtime: AgentRuntime,
) -> anyhow::Result<()> {
    let _ = runtime.abort_tx.send(true);
    for task in runtime.tasks.drain(..) {
        let _ = tokio::time::timeout(Duration::from_secs(3), task).await;
    }

    let worktree_dir = Path::new(&runtime.record.worktree_dir);

    if runtime.record.project.trim().is_empty() {
        tokio::fs::remove_dir_all(worktree_dir)
            .await
            .with_context(|| format!("remove planner dir: {}", worktree_dir.display()))?;
        return Ok(());
    }

    let wtm = WorktreeManager::new(&shared.git, &shared.paths);
    wtm.remove_worktree(&runtime.record.project, worktree_dir)
        .await
        .context("remove worktree")?;

    Ok(())
}

async fn spawn_claude_agent_process(
    agent_id: &str,
    project: &str,
    worktree_dir: &Path,
    murmur_dir: &Path,
    socket_path: &Path,
    permissions_allow: Option<&[String]>,
    is_manager: bool,
    append_system_prompt: Option<&str>,
) -> anyhow::Result<(
    tokio::process::Child,
    tokio::process::ChildStdin,
    tokio::process::ChildStdout,
    u32,
)> {
    let hook_exe_prefix = claude::hook_exe_prefix();
    let socket_path_str = socket_path.to_string_lossy();

    let hook_timeout_sec = 5 * 60;
    let mut settings = serde_json::json!({
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "*",
                    "hooks": [
                        {
                            "type": "command",
                            "command": claude::render_shell_command(&[&hook_exe_prefix, "--socket-path", socket_path_str.as_ref(), "hook", "PreToolUse"]),
                            "timeout": hook_timeout_sec,
                        }
                    ]
                }
            ],
            "PermissionRequest": [
                {
                    "matcher": "*",
                    "hooks": [
                        {
                            "type": "command",
                            "command": claude::render_shell_command(&[&hook_exe_prefix, "--socket-path", socket_path_str.as_ref(), "hook", "PermissionRequest"]),
                            "timeout": hook_timeout_sec,
                        }
                    ]
                }
            ],
            "Stop": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": claude::render_shell_command(&[&hook_exe_prefix, "--socket-path", socket_path_str.as_ref(), "hook", "Stop"]),
                            "timeout": 10,
                        }
                    ]
                }
            ]
        }
    });

    if let Some(allow) = permissions_allow.filter(|p| !p.is_empty()) {
        if let Some(obj) = settings.as_object_mut() {
            obj.insert(
                "permissions".to_owned(),
                serde_json::json!({
                    "allow": allow,
                }),
            );
        }
    }
    let settings_json = serde_json::to_string(&settings).context("serialize claude settings")?;
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
    .env("MURMUR_AGENT_ID", agent_id)
    .env("MURMUR_DIR", murmur_dir)
    .env("MURMUR_PROJECT", project)
    .env("MURMUR_SOCKET_PATH", socket_path)
    .current_dir(worktree_dir)
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::null());

    if is_manager {
        cmd.env("FUGUE_MANAGER", "1").env("FAB_MANAGER", "1");
    }

    if let Some(prompt) = append_system_prompt
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        cmd.args(["--append-system-prompt", prompt]);
    }

    let mut child = cmd.spawn().context("spawn claude agent")?;
    let pid = child
        .id()
        .ok_or_else(|| anyhow!("claude agent pid missing"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("claude agent stdin missing"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("claude agent stdout missing"))?;

    Ok((child, stdin, stdout, pid))
}

async fn claude_stdin_writer(
    mut rx: mpsc::Receiver<ChatMessage>,
    stdin: tokio::process::ChildStdin,
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
            msg = rx.recv() => {
                let Some(msg) = msg else { break };
                let input = InputMessage {
                    r#type: "user".to_owned(),
                    message: MessageBody {
                        role: "user".to_owned(),
                        content: msg.content,
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

async fn claude_stdout_reader(
    shared: Arc<SharedState>,
    agent_id: String,
    stdout: tokio::process::ChildStdout,
    mut abort_rx: watch::Receiver<bool>,
) {
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
                    Ok(Some(stream_msg)) => apply_stream_message(&shared, &agent_id, stream_msg).await,
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        }
    }
}

async fn claude_reaper(
    shared: Arc<SharedState>,
    agent_id: String,
    mut child: tokio::process::Child,
    mut abort_rx: watch::Receiver<bool>,
) {
    let status = tokio::select! {
        status = child.wait() => status.ok(),
        _ = abort_rx.changed() => {
            if *abort_rx.borrow() {
                let _ = child.start_kill();
            }
            child.wait().await.ok()
        }
    };

    let exit_code = status.and_then(|s| s.code());
    let now_ms = now_ms();

    let mut agents = shared.agents.lock().await;
    let Some(rt) = agents.agents.get_mut(&agent_id) else {
        return;
    };

    if rt.record.state == murmur_core::agent::AgentState::Aborted {
        rt.record.exit_code = exit_code;
        rt.record.updated_at_ms = now_ms;
    } else {
        rt.record = rt
            .record
            .apply_event(AgentEvent::Exited { code: exit_code }, now_ms);
    }

    drop(agents);
    persist_agents_runtime(shared).await;
}

async fn codex_worker(
    shared: Arc<SharedState>,
    agent_id: String,
    worktree_dir: std::path::PathBuf,
    mut rx: mpsc::Receiver<ChatMessage>,
    mut abort_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = abort_rx.changed() => {
                if *abort_rx.borrow() {
                    break;
                }
            }
            msg = rx.recv() => {
                let Some(msg) = msg else { break };
                if *abort_rx.borrow() {
                    break;
                }
                if let Err(err) = codex_run_turn(shared.clone(), &agent_id, &worktree_dir, msg.content, abort_rx.clone()).await {
                    tracing::warn!(agent_id = %agent_id, error = %err, "codex turn failed");
                    let sys = ChatMessage::new(ChatRole::System, format!("codex error: {err:#}"), now_ms());
                    let project = {
                        let mut agents = shared.agents.lock().await;
                        if let Some(rt) = agents.agents.get_mut(&agent_id) {
                            rt.chat.push(sys.clone());
                            Some(rt.record.project.clone())
                        } else {
                            None
                        }
                    };
                    if let Some(project) = project {
                        emit_agent_chat_event(shared.as_ref(), &agent_id, &project, sys);
                    }
                }
            }
        }
    }
}

async fn codex_run_turn(
    shared: Arc<SharedState>,
    agent_id: &str,
    worktree_dir: &Path,
    prompt: String,
    mut abort_rx: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    use tokio::io::AsyncBufReadExt as _;

    let (thread_id, project, role) = {
        let agents = shared.agents.lock().await;
        let rt = agents.agents.get(agent_id);
        (
            rt.and_then(|rt| rt.codex_thread_id.clone()),
            rt.map(|rt| rt.record.project.clone()).unwrap_or_default(),
            rt.map(|rt| rt.record.role),
        )
    };

    let is_manager = matches!(role, Some(AgentRole::Manager));
    let prompt = if is_manager && thread_id.is_none() {
        let system_prompt = prompts::build_manager_prompt(&project);
        format!("{system_prompt}\n\n## User Message\n\n{prompt}")
    } else {
        prompt
    };

    let (mut child, stdout, pid) = spawn_codex_turn_process(
        agent_id,
        &project,
        worktree_dir,
        &shared.paths.murmur_dir,
        &shared.paths.socket_path,
        thread_id.as_deref(),
        &prompt,
        is_manager,
    )
    .await?;

    {
        let now_ms = now_ms();
        let mut agents = shared.agents.lock().await;
        if let Some(rt) = agents.agents.get_mut(agent_id) {
            if rt.record.state != murmur_core::agent::AgentState::Aborted {
                rt.record = rt.record.apply_event(AgentEvent::Spawned { pid }, now_ms);
            }
        }
    }

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        tokio::select! {
            _ = abort_rx.changed() => {
                if *abort_rx.borrow() {
                    let _ = child.start_kill();
                    break;
                }
            }
            res = reader.read_line(&mut line) => {
                let n = res.context("read codex stdout")?;
                if n == 0 {
                    break;
                }
                match murmur_core::stream::codex::parse_stream_message_line(&line) {
                    Ok(Some(stream_msg)) => apply_stream_message(&shared, agent_id, stream_msg).await,
                    Ok(None) => {}
                    Err(err) => {
                        tracing::debug!(agent_id = %agent_id, error = %err, "codex parse error");
                    }
                }
            }
        }
    }

    let status = child.wait().await.ok();
    let exit_code = status.and_then(|s| s.code());
    let now_ms = now_ms();

    let idle_project = {
        let mut agents = shared.agents.lock().await;
        if let Some(rt) = agents.agents.get_mut(agent_id) {
            if rt.record.state == murmur_core::agent::AgentState::Aborted {
                rt.record.exit_code = exit_code;
                rt.record.updated_at_ms = now_ms;
                None
            } else {
                rt.record.pid = None;
                rt.record.exit_code = exit_code;
                rt.record.updated_at_ms = now_ms;

                // Transition to Idle after turn completes
                if rt.record.state == AgentState::Running {
                    rt.record = rt.record.apply_event(AgentEvent::BecameIdle, now_ms);
                    rt.last_idle_at_ms = Some(now_ms);
                    Some(rt.record.project.clone())
                } else {
                    None
                }
            }
        } else {
            None
        }
    };

    // Emit idle event for TUI
    if let Some(project) = idle_project {
        emit_agent_state_changed_event(&shared, agent_id, &project, AgentState::Idle);
    }

    persist_agents_runtime(shared).await;
    Ok(())
}

async fn spawn_codex_turn_process(
    agent_id: &str,
    project: &str,
    worktree_dir: &Path,
    murmur_dir: &Path,
    socket_path: &Path,
    thread_id: Option<&str>,
    prompt: &str,
    is_manager: bool,
) -> anyhow::Result<(tokio::process::Child, tokio::process::ChildStdout, u32)> {
    let mut cmd = tokio::process::Command::new("codex");
    cmd.arg("exec");
    if let Some(socket_dir) = socket_path.parent() {
        cmd.arg("--add-dir").arg(socket_dir);
        if socket_dir != murmur_dir {
            cmd.arg("--add-dir").arg(murmur_dir);
        }
    } else {
        cmd.arg("--add-dir").arg(murmur_dir);
    }
    if let Some(thread_id) = thread_id {
        cmd.arg("resume");
        cmd.args([
            "--json",
            "--full-auto",
            "-c",
            r#"model_reasoning_effort="xhigh""#,
            "-c",
            "shell_environment_policy.inherit=all",
            "-c",
            "sandbox_workspace_write.network_access=true",
        ]);
        cmd.arg(thread_id);
        cmd.arg(prompt);
    } else {
        cmd.args([
            "--json",
            "--full-auto",
            "-c",
            r#"model_reasoning_effort="xhigh""#,
            "-c",
            "shell_environment_policy.inherit=all",
            "-c",
            "sandbox_workspace_write.network_access=true",
        ]);
        cmd.arg(prompt);
    }

    cmd.env("MURMUR_AGENT_ID", agent_id)
        .env("MURMUR_DIR", murmur_dir)
        .env("MURMUR_PROJECT", project)
        .env("MURMUR_SOCKET_PATH", socket_path)
        .current_dir(worktree_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    if is_manager {
        cmd.env("FUGUE_MANAGER", "1").env("FAB_MANAGER", "1");
    }

    let mut child = cmd.spawn().context("spawn codex")?;
    let pid = child.id().ok_or_else(|| anyhow!("codex pid missing"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("codex stdout missing"))?;
    Ok((child, stdout, pid))
}

async fn apply_stream_message(shared: &SharedState, agent_id: &str, msg: StreamMessage) {
    let now_ms = now_ms();
    let chat_messages = msg.to_chat_messages(now_ms);

    // Note: Idle state detection is handled via:
    // - Claude: Stop hook -> handle_agent_idle RPC
    // - Codex: Turn completion in codex_run_turn

    let project = {
        let mut agents = shared.agents.lock().await;
        let Some(rt) = agents.agents.get_mut(agent_id) else {
            return;
        };

        // Capture thread_id for Codex conversation resumption
        if let Some(thread_id) = msg.thread_id.clone() {
            rt.codex_thread_id = Some(thread_id.clone());
            rt.record.codex_thread_id = Some(thread_id);
        }

        for chat in &chat_messages {
            rt.chat.push(chat.clone());
        }

        rt.record.project.clone()
    };

    for chat in chat_messages {
        emit_agent_chat_event(shared, agent_id, &project, chat);
    }
}

fn emit_agent_chat_event(shared: &SharedState, agent_id: &str, project: &str, msg: ChatMessage) {
    let payload = serde_json::to_value(AgentChatEvent {
        agent_id: agent_id.to_owned(),
        project: project.to_owned(),
        message: to_proto_chat_message(msg),
    })
    .unwrap_or(serde_json::Value::Null);

    let id = shared.next_event_id.fetch_add(1, Ordering::Relaxed);
    let _ = shared.events_tx.send(Event {
        r#type: EVT_AGENT_CHAT.to_owned(),
        id: format!("evt-{id}"),
        payload,
    });
}

fn emit_agent_created_event(shared: &SharedState, agent: &murmur_protocol::AgentInfo) {
    let payload = serde_json::to_value(AgentCreatedEvent {
        agent: agent.clone(),
    })
    .unwrap_or(serde_json::Value::Null);

    let id = shared.next_event_id.fetch_add(1, Ordering::Relaxed);
    let _ = shared.events_tx.send(Event {
        r#type: EVT_AGENT_CREATED.to_owned(),
        id: format!("evt-{id}"),
        payload,
    });
}

pub(in crate::daemon) fn emit_agent_deleted_event(
    shared: &SharedState,
    agent_id: &str,
    project: &str,
) {
    let payload = serde_json::to_value(murmur_protocol::AgentDeletedEvent {
        agent_id: agent_id.to_owned(),
        project: project.to_owned(),
    })
    .unwrap_or(serde_json::Value::Null);

    let id = shared.next_event_id.fetch_add(1, Ordering::Relaxed);
    let _ = shared.events_tx.send(Event {
        r#type: murmur_protocol::EVT_AGENT_DELETED.to_owned(),
        id: format!("evt-{id}"),
        payload,
    });
}

fn emit_agent_state_changed_event(
    shared: &SharedState,
    agent_id: &str,
    project: &str,
    state: AgentState,
) {
    let proto_state = match state {
        AgentState::Starting => murmur_protocol::AgentState::Starting,
        AgentState::Running => murmur_protocol::AgentState::Running,
        AgentState::Idle => murmur_protocol::AgentState::Idle,
        AgentState::NeedsResolution => murmur_protocol::AgentState::NeedsResolution,
        AgentState::Exited => murmur_protocol::AgentState::Exited,
        AgentState::Aborted => murmur_protocol::AgentState::Aborted,
    };

    let payload = serde_json::to_value(murmur_protocol::AgentIdleEvent {
        agent_id: agent_id.to_owned(),
        project: project.to_owned(),
        state: proto_state,
    })
    .unwrap_or(serde_json::Value::Null);

    let id = shared.next_event_id.fetch_add(1, Ordering::Relaxed);
    let _ = shared.events_tx.send(Event {
        r#type: murmur_protocol::EVT_AGENT_IDLE.to_owned(),
        id: format!("evt-{id}"),
        payload,
    });
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

async fn persist_agents_runtime(shared: Arc<SharedState>) {
    let agents_json = {
        let agents = shared.agents.lock().await;
        let infos = agents
            .agents
            .values()
            .map(|rt| agent_info_from_record(&rt.record, rt.backend))
            .collect::<Vec<_>>();
        serde_json::to_value(infos).unwrap_or(serde_json::Value::Null)
    };

    if let Err(err) = runtime_store::save_agents(&shared.paths, &agents_json).await {
        tracing::debug!(error = %err, "persist agents runtime failed");
    }
}

/// Restore agents from disk on daemon startup.
/// This allows agents spawned in previous daemon sessions to be recognized
/// so that `mm agent claim` and `mm agent done` work after daemon restarts.
async fn rehydrate_agents(shared: Arc<SharedState>) -> anyhow::Result<()> {
    let infos = runtime_store::load_agents(&shared.paths).await?;

    for info in infos {
        // Mirror fab behavior: only show/live-rehydrate active agents.
        // Completed (exited/aborted) agents should not linger in the UI across sessions.
        if matches!(
            info.state,
            murmur_protocol::AgentState::Exited | murmur_protocol::AgentState::Aborted
        ) {
            continue;
        }

        // Skip if worktree doesn't exist
        let wt_path = std::path::PathBuf::from(&info.worktree_dir);
        if !wt_path.exists() {
            tracing::debug!(agent_id = %info.id, "skipping rehydration - worktree missing");
            continue;
        }

        // Skip if agent is already in memory (shouldn't happen, but be safe)
        {
            let agents = shared.agents.lock().await;
            if agents.agents.contains_key(&info.id) {
                continue;
            }
        }

        // Convert protocol role to core role
        let role = match info.role {
            murmur_protocol::AgentRole::Coding => AgentRole::Coding,
            murmur_protocol::AgentRole::Planner => AgentRole::Planner,
            murmur_protocol::AgentRole::Manager => AgentRole::Manager,
        };

        // Convert backend string to enum
        let backend = match info.backend.as_deref() {
            Some("claude") => AgentBackend::Claude,
            _ => AgentBackend::Codex,
        };

        // Check if process is still running
        let process_alive = info.pid.map(is_process_running).unwrap_or(false);
        if !process_alive {
            continue;
        }

        // Create agent record
        let mut record = AgentRecord::new(
            info.id.clone(),
            info.project.clone(),
            role,
            info.issue_id.clone(),
            info.created_at_ms,
            info.worktree_dir.clone(),
        );

        // Apply events based on process status
        if let Some(pid) = info.pid {
            record = record.apply_event(AgentEvent::Spawned { pid }, info.created_at_ms);
        }

        let (outbound_tx, _) = mpsc::channel::<ChatMessage>(32);
        let (abort_tx, _) = watch::channel(false);

        // Also restore codex_thread_id in the record for persistence
        record.codex_thread_id = info.codex_thread_id.clone();

        let mut agents = shared.agents.lock().await;
        agents.agents.insert(
            info.id.clone(),
            AgentRuntime {
                record,
                backend,
                codex_thread_id: info.codex_thread_id.clone(),
                chat: ChatHistory::new(DEFAULT_CHAT_CAPACITY),
                last_idle_at_ms: None,
                outbound_tx,
                abort_tx,
                tasks: Vec::new(),
            },
        );

        tracing::info!(
            agent_id = %info.id,
            project = %info.project,
            process_alive = %process_alive,
            "rehydrated agent"
        );
    }

    // Prune any terminal/stale agents from disk.
    persist_agents_runtime(shared).await;
    Ok(())
}

fn is_process_running(pid: u32) -> bool {
    // Check if process exists by checking /proc/<pid> on Linux
    let proc_path = format!("/proc/{pid}");
    if !std::path::Path::new(&proc_path).exists() {
        return false;
    }

    // Verify this is actually an agent process (claude or codex) by checking cmdline
    // This prevents PID reuse from causing stale agents to be rehydrated
    let cmdline_path = format!("/proc/{pid}/cmdline");
    match std::fs::read_to_string(&cmdline_path) {
        Ok(cmdline) => {
            let cmdline_lower = cmdline.to_lowercase();
            cmdline_lower.contains("claude") || cmdline_lower.contains("codex")
        }
        Err(_) => false,
    }
}

fn project_dir(paths: &MurmurPaths, name: &str) -> std::path::PathBuf {
    paths.projects_dir.join(name)
}

async fn detect_next_agent_id_seed(paths: &MurmurPaths, git: &Git) -> u64 {
    let mut max_seen = 0u64;

    let mut projects = match tokio::fs::read_dir(&paths.projects_dir).await {
        Ok(v) => v,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return 1;
        }
        Err(_) => return 1,
    };

    loop {
        let entry = match projects.next_entry().await {
            Ok(Some(v)) => v,
            Ok(None) => break,
            Err(_) => break,
        };

        let ty = match entry.file_type().await {
            Ok(v) => v,
            Err(_) => continue,
        };
        if !ty.is_dir() {
            continue;
        }

        let project_dir = entry.path();
        let worktrees_dir = project_dir.join("worktrees");
        if let Ok(mut worktrees) = tokio::fs::read_dir(&worktrees_dir).await {
            loop {
                let wt = match worktrees.next_entry().await {
                    Ok(Some(v)) => v,
                    Ok(None) => break,
                    Err(_) => break,
                };
                let name = wt.file_name();
                let Some(name) = name.to_str() else { continue };
                if let Some(n) = parse_agent_num_from_worktree_dir(name) {
                    max_seen = max_seen.max(n);
                }
            }
        }

        let repo_dir = project_dir.join("repo");
        if !repo_dir.join(".git").exists() {
            continue;
        }

        if let Ok(refs) = git.list_refs_short(&repo_dir, "refs/heads/murmur").await {
            for r in refs {
                if let Some(n) = parse_agent_num_from_branch(&r) {
                    max_seen = max_seen.max(n);
                }
            }
        }
    }

    max_seen.saturating_add(1).max(1)
}

fn parse_agent_num_from_worktree_dir(name: &str) -> Option<u64> {
    let rest = name.strip_prefix("wt-a-")?;
    rest.parse::<u64>().ok()
}

fn parse_agent_num_from_branch(name: &str) -> Option<u64> {
    let rest = name.strip_prefix("murmur/a-")?;
    rest.parse::<u64>().ok()
}

async fn detect_next_plan_id_seed(paths: &MurmurPaths) -> u64 {
    let mut max_seen = 0u64;

    let mut entries = match tokio::fs::read_dir(&paths.plans_dir).await {
        Ok(v) => v,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return 1;
        }
        Err(_) => return 1,
    };

    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(v)) => v,
            Ok(None) => break,
            Err(_) => break,
        };

        let ty = match entry.file_type().await {
            Ok(v) => v,
            Err(_) => continue,
        };
        if !ty.is_file() {
            continue;
        }

        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(n) = parse_plan_num_from_filename(name) {
            max_seen = max_seen.max(n);
        }
    }

    max_seen.saturating_add(1).max(1)
}

fn parse_plan_num_from_filename(name: &str) -> Option<u64> {
    let rest = name.strip_prefix("plan-")?;
    let rest = rest.strip_suffix(".md")?;
    rest.parse::<u64>().ok()
}

fn project_repo_dir(paths: &MurmurPaths, name: &str) -> std::path::PathBuf {
    project_dir(paths, name).join("repo")
}
