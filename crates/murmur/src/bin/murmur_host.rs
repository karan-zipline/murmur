//! Murmur Agent Host
//!
//! A standalone process that manages an agent subprocess independently of the daemon.
//! Communicates with the daemon via Unix socket using the host protocol.
//!
//! ## Usage
//!
//! ```text
//! murmur-host --agent-id a-1 \
//!             --project myproject \
//!             --worktree /path/to/worktree \
//!             --socket-dir /path/to/hosts \
//!             --murmur-dir /path/to/murmur \
//!             --daemon-socket /path/to/daemon.sock
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use clap::Parser;
use murmur::host::{Manager, Server};
use murmur_core::agent::AgentRole;
use murmur_core::config::AgentBackend;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "murmur-host")]
#[command(about = "Murmur Agent Host - manages agent processes independently of the daemon")]
struct Cli {
    /// Agent ID (e.g., "a-1")
    #[arg(long)]
    agent_id: String,

    /// Project name
    #[arg(long)]
    project: String,

    /// Agent role
    #[arg(long, default_value = "coding")]
    role: String,

    /// Backend (claude or codex)
    #[arg(long, default_value = "claude")]
    backend: String,

    /// Worktree directory for the agent
    #[arg(long)]
    worktree: PathBuf,

    /// Directory for host socket files
    #[arg(long)]
    socket_dir: PathBuf,

    /// Murmur data directory
    #[arg(long)]
    murmur_dir: PathBuf,

    /// Daemon socket path (for hooks)
    #[arg(long)]
    daemon_socket: PathBuf,

    /// Issue ID (optional)
    #[arg(long)]
    issue_id: Option<String>,

    /// Initial prompt to send to the agent
    #[arg(long)]
    initial_prompt: Option<String>,

    /// System prompt to append
    #[arg(long)]
    append_system_prompt: Option<String>,

    /// Log file path (default: socket_dir/agent_id.log)
    #[arg(long)]
    log_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let log_file = cli.log_file.unwrap_or_else(|| {
        cli.socket_dir.join(format!("{}.log", cli.agent_id))
    });

    setup_logging(&log_file)?;

    tracing::info!(
        agent_id = %cli.agent_id,
        project = %cli.project,
        backend = %cli.backend,
        worktree = %cli.worktree.display(),
        "starting agent host"
    );

    let role = parse_role(&cli.role)?;
    let backend = parse_backend(&cli.backend)?;

    let socket_path = cli.socket_dir.join(format!("{}.sock", cli.agent_id));

    let config = murmur::host::manager::AgentConfig {
        agent_id: cli.agent_id.clone(),
        project: cli.project,
        role,
        backend,
        worktree: cli.worktree,
        murmur_dir: cli.murmur_dir,
        socket_path: cli.daemon_socket,
        issue_id: cli.issue_id,
        initial_prompt: cli.initial_prompt,
        append_system_prompt: cli.append_system_prompt,
    };

    let (manager, tasks) = Manager::spawn(config)
        .await
        .context("spawn agent")?;

    let server = Server::new(socket_path, Arc::clone(&manager));

    tokio::spawn(async move {
        for task in tasks {
            let _ = task.await;
        }
    });

    tokio::spawn({
        let manager = Arc::clone(&manager);
        async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                if manager.is_exited().await {
                    tracing::info!("agent exited, shutting down host");
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    std::process::exit(0);
                }
            }
        }
    });

    server.run().await?;

    tracing::info!("host shutdown complete");
    Ok(())
}

fn setup_logging(log_file: &PathBuf) -> anyhow::Result<()> {
    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent).context("create log directory")?;
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)
        .context("open log file")?;

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(file)
        .with_ansi(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .context("set tracing subscriber")?;

    Ok(())
}

fn parse_role(s: &str) -> anyhow::Result<AgentRole> {
    match s {
        "coding" => Ok(AgentRole::Coding),
        "planner" => Ok(AgentRole::Planner),
        "manager" => Ok(AgentRole::Manager),
        "director" => Ok(AgentRole::Director),
        _ => anyhow::bail!("unknown role: {}", s),
    }
}

fn parse_backend(s: &str) -> anyhow::Result<AgentBackend> {
    match s {
        "claude" => Ok(AgentBackend::Claude),
        "codex" => Ok(AgentBackend::Codex),
        _ => anyhow::bail!("unknown backend: {}", s),
    }
}
