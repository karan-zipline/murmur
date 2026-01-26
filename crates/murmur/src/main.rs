use std::env;
use std::fs;
use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context as _};
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use directories::BaseDirs;
use murmur::ipc::jsonl::{read_jsonl, write_jsonl};
use murmur::{client, daemon};
use murmur_core::agent::{ChatMessage, ChatRole};
use murmur_core::paths::{compute_paths, MurmurPaths, PathInputs};
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "mm",
    version,
    about = "Murmur — local-only agent orchestration supervisor"
)]
struct Cli {
    #[arg(long, global = true, value_name = "DIR", env = "MURMUR_DIR")]
    murmur_dir: Option<PathBuf>,

    #[arg(long, global = true, value_name = "PATH", env = "MURMUR_SOCKET_PATH")]
    socket_path: Option<PathBuf>,

    #[arg(long, global = true, env = "MURMUR_LOG", value_name = "LEVEL")]
    log_level: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    #[command(hide = true)]
    Ping,
    #[command(hide = true)]
    Stats {
        #[arg(long)]
        project: Option<String>,
    },
    Status {
        #[arg(short = 'a', long)]
        agents: bool,
    },
    Version,
    Completion {
        #[command(subcommand)]
        command: CompletionCommand,
    },
    Attach {
        projects: Vec<String>,
    },
    Tui,
    Server {
        #[command(subcommand)]
        command: ServerCommand,
    },
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    #[command(
        about = "Commands for managing issues using the configured backend (tk, github, etc.)."
    )]
    Issue(IssueArgs),
    #[command(hide = true)]
    Orchestration {
        #[command(subcommand)]
        command: OrchestrationCommand,
    },
    #[command(hide = true)]
    Claim {
        #[command(subcommand)]
        command: ClaimCommand,
    },
    Claims {
        #[arg(long)]
        project: Option<String>,
    },
    Branch {
        #[command(subcommand)]
        command: BranchCommand,
    },
    #[command(hide = true)]
    Commit {
        #[command(subcommand)]
        command: CommitCommand,
    },
    #[command(hide = true)]
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
    Plan {
        #[command(subcommand)]
        command: PlanCommand,
    },
    Manager {
        #[command(subcommand)]
        command: ManagerCommand,
    },
    #[command(hide = true)]
    Permission {
        #[command(subcommand)]
        command: PermissionCommand,
    },
    #[command(hide = true)]
    Question {
        #[command(subcommand)]
        command: QuestionCommand,
    },
    #[command(hide = true, name = "__internal")]
    Internal {
        #[command(subcommand)]
        command: InternalCommand,
    },
}

#[derive(Subcommand, Debug)]
enum CompletionCommand {
    Bash,
    Fish,
    Powershell,
    Zsh,
}

#[derive(Subcommand, Debug)]
enum ServerCommand {
    Start {
        #[arg(short = 'f', long)]
        foreground: bool,
    },
    Status,
    #[command(alias = "shutdown")]
    Stop,
    Restart {
        #[arg(long)]
        foreground: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ProjectCommand {
    Add {
        input: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        remote_url: Option<String>,
        #[arg(long)]
        max_agents: Option<u16>,
        #[arg(long)]
        autostart: bool,
        #[arg(long)]
        backend: Option<String>,
    },
    List,
    Remove {
        name: String,
        #[arg(long)]
        delete_worktrees: bool,
    },
    Start {
        project: Option<String>,
        #[arg(short = 'a', long)]
        all: bool,
    },
    Stop {
        project: Option<String>,
        #[arg(short = 'a', long)]
        all: bool,
        #[arg(long, alias = "stop-agents")]
        abort_agents: bool,
    },
    Status {
        name: String,
    },
    Config {
        #[command(subcommand)]
        command: ProjectConfigCommand,
    },
}

#[derive(Subcommand, Debug)]
enum ProjectConfigCommand {
    /// Get a single configuration value
    Get { project: String, key: String },
    /// Set a configuration value
    #[command(after_help = "\
KEYS AND VALUES:
  max-agents          Number of concurrent agents (1-10)
  issue-backend       Issue source: tk, github, linear
  agent-backend       AI backend: claude, codex
  planner-backend     Override for planners: claude, codex
  coding-backend      Override for coding agents: claude, codex
  permissions-checker How to handle permissions: manual, llm
  merge-strategy      How to merge completed work: direct, pull-request
  autostart           Start orchestration on daemon start: true, false
  allowed-authors     Filter issues by author (comma-separated)
  linear-team         Linear team UUID (required for linear backend)
  linear-project      Linear project UUID (optional filter)

EXAMPLES:
  mm project config set myproj max-agents 5
  mm project config set myproj issue-backend github
  mm project config set myproj merge-strategy pull-request
  mm project config set myproj allowed-authors alice,bob,charlie
")]
    Set {
        project: String,
        /// Configuration key (see KEYS AND VALUES below)
        key: String,
        /// Value to set
        value: String,
    },
    /// Show all configuration values
    Show { project: String },
}

#[derive(Subcommand, Debug)]
enum AgentCommand {
    #[command(hide = true)]
    Create {
        project: String,
        issue_id: String,
        #[arg(long)]
        backend: Option<String>,
    },
    List {
        #[arg(long)]
        project: Option<String>,
    },
    #[command(hide = true)]
    Delete {
        agent_id: String,
    },
    #[command(
        about = "Abort a running agent",
        long_about = "Abort a running agent.\n\nBy default this requests a graceful shutdown.\nUse --force to kill immediately."
    )]
    Abort {
        agent_id: String,
        #[arg(short = 'f', long)]
        force: bool,
        #[arg(short = 'y', long)]
        yes: bool,
    },
    Plan(AgentPlanArgs),
    #[command(
        about = "Claim a ticket for this agent",
        long_about = "Claim a ticket to prevent other agents from working on it.\n\nUses MURMUR_AGENT_ID from the environment."
    )]
    Claim {
        issue_id: String,
    },
    #[command(
        about = "Set a description for this agent",
        long_about = "Set a human-readable description of what the agent is currently doing.\n\nUses MURMUR_AGENT_ID from the environment."
    )]
    Describe {
        description: String,
    },
    #[command(hide = true)]
    SendMessage {
        agent_id: String,
        message: String,
    },
    #[command(hide = true)]
    Tail {
        agent_id: String,
    },
    #[command(hide = true)]
    ChatHistory {
        agent_id: String,
        #[arg(long)]
        limit: Option<u32>,
    },
    #[command(
        about = "Signal that the agent has completed its task",
        long_about = "Called by agents to signal task completion.\n\nUses MURMUR_AGENT_ID from the environment."
    )]
    Done {
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        error: Option<String>,
    },
}

#[derive(Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
struct AgentPlanArgs {
    #[arg(long, global = true)]
    project: Option<String>,

    #[command(subcommand)]
    command: Option<AgentPlanSubcommand>,

    #[arg(value_name = "PROMPT", trailing_var_arg = true)]
    prompt: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum AgentPlanSubcommand {
    List,
    Stop { plan_id: String },
}

#[derive(Args, Debug)]
struct IssueArgs {
    #[arg(
        short = 'p',
        long,
        global = true,
        value_name = "PROJECT",
        help = "Project name (default: detect from cwd)"
    )]
    project: Option<String>,

    #[command(subcommand)]
    command: IssueCommand,
}

#[derive(Subcommand, Debug)]
enum IssueCommand {
    #[command(about = "List issues with optional filters.")]
    List {
        #[arg(short = 's', long, help = "Filter by status (open, closed, blocked)")]
        status: Option<String>,
    },
    #[command(about = "Show detailed information about an issue.")]
    Show { id: String },
    #[command(hide = true)]
    Get { id: String },
    #[command(about = "List open issues with no open dependencies.")]
    Ready,
    #[command(about = "Create a new issue. Use 'mm issue commit' to push changes.")]
    Create {
        title: String,
        #[arg(short = 'd', long, help = "Issue description")]
        description: Option<String>,
        #[arg(
            long = "type",
            value_name = "string",
            default_value = "task",
            help = "Issue type (task, bug, feature, chore)"
        )]
        issue_type: String,
        #[arg(
            long,
            default_value_t = 1,
            help = "Issue priority (0=low, 1=medium, 2=high)"
        )]
        priority: i32,
        #[arg(long, help = "Commit and push changes immediately")]
        commit: bool,
        #[arg(
            long,
            value_name = "strings",
            value_delimiter = ',',
            alias = "dep",
            help = "Issue IDs this issue depends on (comma-separated)"
        )]
        depends_on: Vec<String>,
        #[arg(long, help = "Parent issue ID (creates a sub-issue)")]
        parent: Option<String>,

        #[arg(hide = true, long, value_name = "LABEL")]
        label: Vec<String>,
        #[arg(hide = true, long, value_name = "LINK")]
        link: Vec<String>,
    },
    #[command(
        about = "Update an issue's status, priority, or other fields. Use 'mm issue commit' to push changes."
    )]
    Update {
        id: String,
        #[arg(short = 't', long, help = "New title")]
        title: Option<String>,
        #[arg(short = 's', long, help = "New status (open, closed, blocked)")]
        status: Option<String>,
        #[arg(long, help = "New priority (0=low, 1=medium, 2=high)")]
        priority: Option<i32>,

        #[arg(hide = true, long)]
        description: Option<String>,
        #[arg(hide = true, long, value_name = "TYPE")]
        issue_type: Option<String>,

        #[arg(hide = true, long, value_name = "LABEL")]
        label: Vec<String>,
        #[arg(hide = true, long)]
        clear_labels: bool,

        #[arg(hide = true, long, value_name = "DEP", alias = "depends-on")]
        dep: Vec<String>,
        #[arg(hide = true, long)]
        clear_deps: bool,

        #[arg(hide = true, long, value_name = "LINK")]
        link: Vec<String>,
        #[arg(hide = true, long)]
        clear_links: bool,
    },
    #[command(about = "Mark an issue as closed. Use 'mm issue commit' to push changes.")]
    Close { id: String },
    #[command(
        about = "Add a comment to an issue. The comment body can be provided via --body flag."
    )]
    Comment {
        id: String,
        #[arg(short = 'b', long, help = "Comment body")]
        body: Option<String>,
    },
    #[command(
        about = "Update or create a ## Plan section in the issue body. The plan content can be provided via --body or --file flag."
    )]
    Plan {
        id: String,
        #[arg(short = 'b', long, help = "Plan content")]
        body: Option<String>,
        #[arg(short = 'f', long, help = "Read plan content from file")]
        file: Option<PathBuf>,
    },
    #[command(
        about = "Stage, commit, and push any pending issue changes to the remote repository."
    )]
    Commit,
}

#[derive(Subcommand, Debug)]
enum OrchestrationCommand {
    Start {
        project: Option<String>,
        #[arg(long)]
        all: bool,
    },
    Stop {
        project: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long, alias = "stop-agents")]
        abort_agents: bool,
    },
    Status {
        project: String,
    },
}

#[derive(Subcommand, Debug)]
enum ClaimCommand {
    List {
        #[arg(long)]
        project: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum BranchCommand {
    Cleanup {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        local: bool,
    },
}

#[derive(Subcommand, Debug)]
enum CommitCommand {
    List {
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
}

#[derive(Subcommand, Debug)]
enum HookCommand {
    #[command(name = "PreToolUse", alias = "pre-tool-use")]
    PreToolUse,
    #[command(name = "PermissionRequest", alias = "permission-request")]
    PermissionRequest,
    #[command(name = "Stop", alias = "stop")]
    Stop,
}

#[derive(Subcommand, Debug)]
enum PlanCommand {
    #[command(hide = true)]
    Start {
        #[arg(long)]
        project: Option<String>,
        #[arg(required = true, trailing_var_arg = true)]
        prompt: Vec<String>,
    },
    #[command(hide = true)]
    Stop {
        plan_id: String,
    },
    #[command(name = "list-running")]
    #[command(hide = true)]
    ListRunning {
        #[arg(long)]
        project: Option<String>,
    },
    #[command(alias = "list-stored")]
    List,
    Write,
    #[command(alias = "show")]
    Read {
        plan_id: String,
    },
    #[command(hide = true)]
    SendMessage {
        plan_id: String,
        message: String,
    },
    #[command(hide = true)]
    ChatHistory {
        plan_id: String,
        #[arg(long)]
        limit: Option<u32>,
    },
}

#[derive(Subcommand, Debug)]
enum ManagerCommand {
    Start {
        project: String,
    },
    Stop {
        project: String,
    },
    Status {
        project: String,
    },
    #[command(hide = true)]
    SendMessage {
        project: String,
        message: String,
    },
    #[command(hide = true)]
    ChatHistory {
        project: String,
        #[arg(long)]
        limit: Option<u32>,
    },
    #[command(name = "clear", alias = "clear-history")]
    ClearHistory {
        project: String,
    },
}

#[derive(Subcommand, Debug)]
enum PermissionCommand {
    List,
    Respond {
        request_id: String,
        decision: String,
    },
}

#[derive(Subcommand, Debug)]
enum QuestionCommand {
    List,
    Respond {
        request_id: String,
        response: String,
    },
}

#[derive(Subcommand, Debug)]
enum InternalCommand {
    DummyAgent {
        #[arg(long)]
        agent_id: String,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let paths = resolve_paths(cli.murmur_dir.as_ref(), cli.socket_path.as_ref())?;
    let enable_stderr_logging = !matches!(cli.command, Command::Tui);
    init_logging(&paths, cli.log_level.as_deref(), enable_stderr_logging)?;

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "murmur starting");

    dispatch(cli.command, &paths).await
}

fn resolve_paths(
    murmur_dir_override: Option<&PathBuf>,
    socket_path_override: Option<&PathBuf>,
) -> anyhow::Result<MurmurPaths> {
    let base_dirs = BaseDirs::new().ok_or_else(|| anyhow!("could not determine home directory"))?;
    let home_dir = base_dirs.home_dir().to_path_buf();

    let xdg_config_home = match env::var_os("XDG_CONFIG_HOME") {
        Some(v) => Some(PathBuf::from(v)),
        None => Some(base_dirs.config_dir().to_path_buf()),
    };

    let xdg_runtime_dir = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);

    let murmur_dir_override = murmur_dir_override
        .cloned()
        .or_else(|| env::var_os("MURMUR_DIR").map(PathBuf::from));

    let socket_path_override = socket_path_override
        .cloned()
        .or_else(|| env::var_os("MURMUR_SOCKET_PATH").map(PathBuf::from));

    Ok(compute_paths(PathInputs {
        home_dir,
        xdg_config_home,
        xdg_runtime_dir,
        murmur_dir_override,
        socket_path_override,
    }))
}

fn init_logging(
    paths: &MurmurPaths,
    log_level: Option<&str>,
    enable_stderr_logging: bool,
) -> anyhow::Result<()> {
    let dir_ok = fs::create_dir_all(&paths.murmur_dir).is_ok();

    let env_level = env::var("RUST_LOG").ok();
    let level = log_level
        .map(str::to_owned)
        .or_else(|| env::var("MURMUR_LOG").ok())
        .or(env_level)
        .unwrap_or_else(|| "info".to_owned());

    let filter = EnvFilter::try_new(level).context("parse log level")?;

    let file_layer = if dir_ok {
        tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::NEVER)
            .filename_prefix("murmur")
            .filename_suffix("log")
            .build(&paths.murmur_dir)
            .ok()
            .map(|file_appender| {
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(file_appender)
            })
    } else {
        None
    };

    if enable_stderr_logging {
        let stderr_layer = tracing_subscriber::fmt::layer()
            .with_ansi(true)
            .with_writer(io::stderr);

        tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .with(stderr_layer)
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .init();
    }

    Ok(())
}

async fn dispatch(command: Command, paths: &MurmurPaths) -> anyhow::Result<()> {
    match command {
        Command::Ping => ping(paths).await,
        Command::Stats { project } => stats(paths, project).await,
        Command::Status { agents } => status(paths, agents).await,
        Command::Version => version(),
        Command::Completion { command } => completion(command),
        Command::Attach { projects } => attach(paths, projects).await,
        Command::Tui => tui(paths).await,
        Command::Server { command } => dispatch_server(command, paths).await,
        Command::Project { command } => dispatch_project(command, paths).await,
        Command::Agent { command } => dispatch_agent(command, paths).await,
        Command::Issue(args) => dispatch_issue(args, paths).await,
        Command::Orchestration { command } => dispatch_orchestration(command, paths).await,
        Command::Claim { command } => dispatch_claim(command, paths).await,
        Command::Claims { project } => {
            let resp = client::claim_list(paths, project).await?;
            if resp.claims.is_empty() {
                println!("No active claims");
                return Ok(());
            }
            for c in resp.claims {
                println!("{}\t{}\t{}", c.issue_id, c.agent_id, c.project);
            }
            Ok(())
        }
        Command::Branch { command } => dispatch_branch(command).await,
        Command::Commit { command } => dispatch_commit(command, paths).await,
        Command::Hook { command } => dispatch_hook(command, paths).await,
        Command::Plan { command } => dispatch_plan(command, paths).await,
        Command::Manager { command } => dispatch_manager(command, paths).await,
        Command::Permission { command } => dispatch_permission(command, paths).await,
        Command::Question { command } => dispatch_question(command, paths).await,
        Command::Internal { command } => dispatch_internal(command).await,
    }
}

fn completion(command: CompletionCommand) -> anyhow::Result<()> {
    let shell = match command {
        CompletionCommand::Bash => Shell::Bash,
        CompletionCommand::Fish => Shell::Fish,
        CompletionCommand::Powershell => Shell::PowerShell,
        CompletionCommand::Zsh => Shell::Zsh,
    };

    let mut cmd = Cli::command();
    let mut buf = Vec::new();
    generate(shell, &mut cmd, "mm", &mut buf);
    match io::stdout().write_all(&buf) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(err) => Err(err).context("write completion script to stdout"),
    }
}

async fn dispatch_hook(command: HookCommand, paths: &MurmurPaths) -> anyhow::Result<()> {
    match command {
        HookCommand::PreToolUse | HookCommand::PermissionRequest => {
            murmur::hooks::handle_pre_tool_use(paths).await
        }
        HookCommand::Stop => murmur::hooks::handle_stop(paths).await,
    }
}

async fn dispatch_plan(command: PlanCommand, paths: &MurmurPaths) -> anyhow::Result<()> {
    match command {
        PlanCommand::Start { project, prompt } => {
            let prompt = prompt.join(" ");
            let resp = client::plan_start(paths, project, prompt).await?;
            println!("{}", resp.id);
            Ok(())
        }
        PlanCommand::Stop { plan_id } => {
            client::plan_stop(paths, plan_id).await?;
            println!("ok");
            Ok(())
        }
        PlanCommand::ListRunning { project } => {
            let resp = client::plan_list(paths, project).await?;
            for a in resp.plans {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    a.id,
                    a.project,
                    format_agent_role(a.role),
                    format_agent_state(a.state),
                    a.issue_id
                );
            }
            Ok(())
        }
        PlanCommand::List => plan_list_stored(paths),
        PlanCommand::Write => plan_write(paths),
        PlanCommand::Read { plan_id } => {
            let resp = client::plan_show(paths, plan_id).await?;
            print!("{}", resp.contents);
            Ok(())
        }
        PlanCommand::SendMessage { plan_id, message } => {
            client::plan_send_message(paths, plan_id, message).await?;
            println!("ok");
            Ok(())
        }
        PlanCommand::ChatHistory { plan_id, limit } => {
            let resp = client::plan_chat_history(paths, plan_id, limit).await?;
            for m in resp.messages {
                println!("{}\t{}", format_chat_role(m.role), m.content);
            }
            Ok(())
        }
    }
}

async fn dispatch_manager(command: ManagerCommand, paths: &MurmurPaths) -> anyhow::Result<()> {
    match command {
        ManagerCommand::Start { project } => {
            client::manager_start(paths, project).await?;
            println!("ok");
            Ok(())
        }
        ManagerCommand::Stop { project } => {
            client::manager_stop(paths, project).await?;
            println!("ok");
            Ok(())
        }
        ManagerCommand::Status { project } => {
            let resp = client::manager_status(paths, project).await?;
            match resp.manager {
                Some(mgr) => {
                    println!("id\t{}", mgr.id);
                    println!("project\t{}", mgr.project);
                    println!("role\t{}", format_agent_role(mgr.role));
                    println!("state\t{}", format_agent_state(mgr.state));
                    println!("worktree_dir\t{}", mgr.worktree_dir);
                    if let Some(backend) = mgr.backend.as_deref() {
                        println!("backend\t{}", backend);
                    }
                }
                None => {
                    println!("stopped");
                }
            }
            Ok(())
        }
        ManagerCommand::SendMessage { project, message } => {
            client::manager_send_message(paths, project, message).await?;
            println!("ok");
            Ok(())
        }
        ManagerCommand::ChatHistory { project, limit } => {
            let resp = client::manager_chat_history(paths, project, limit).await?;
            for m in resp.messages {
                println!("{}\t{}", format_chat_role(m.role), m.content);
            }
            Ok(())
        }
        ManagerCommand::ClearHistory { project } => {
            client::manager_clear_history(paths, project).await?;
            println!("ok");
            Ok(())
        }
    }
}

async fn dispatch_permission(
    command: PermissionCommand,
    paths: &MurmurPaths,
) -> anyhow::Result<()> {
    match command {
        PermissionCommand::List => {
            let resp = client::permission_list(paths, None).await?;
            for r in resp.requests {
                let primary =
                    murmur_core::permissions::resolve_primary_field(&r.tool_name, &r.tool_input);
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    r.id, r.agent_id, r.project, r.tool_name, primary
                );
            }
            Ok(())
        }
        PermissionCommand::Respond {
            request_id,
            decision,
        } => {
            let behavior = parse_permission_behavior(&decision)?;
            client::permission_respond(
                paths,
                murmur_protocol::PermissionRespondPayload {
                    id: request_id,
                    behavior,
                    message: None,
                    interrupt: false,
                },
            )
            .await?;
            println!("ok");
            Ok(())
        }
    }
}

async fn dispatch_question(command: QuestionCommand, paths: &MurmurPaths) -> anyhow::Result<()> {
    match command {
        QuestionCommand::List => {
            let resp = client::question_list(paths, None).await?;
            for q in resp.requests {
                println!("{}\t{}\t{}", q.id, q.agent_id, q.project);
                for item in q.questions {
                    println!("  {}\t{}", item.header, item.question);
                    for opt in item.options {
                        println!("    {}\t{}", opt.label, opt.description);
                    }
                }
            }
            Ok(())
        }
        QuestionCommand::Respond {
            request_id,
            response,
        } => {
            let answers = parse_question_answers(&response)?;
            client::question_respond(
                paths,
                murmur_protocol::UserQuestionRespondPayload {
                    id: request_id,
                    answers,
                },
            )
            .await?;
            println!("ok");
            Ok(())
        }
    }
}

fn parse_permission_behavior(s: &str) -> anyhow::Result<murmur_protocol::PermissionBehavior> {
    match s.trim().to_lowercase().as_str() {
        "allow" | "yes" | "y" => Ok(murmur_protocol::PermissionBehavior::Allow),
        "deny" | "no" | "n" => Ok(murmur_protocol::PermissionBehavior::Deny),
        other => Err(anyhow!("invalid decision: {other} (expected allow|deny)")),
    }
}

fn parse_question_answers(s: &str) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    let trimmed = s.trim();
    let parsed: std::collections::BTreeMap<String, String> =
        serde_json::from_str(trimmed).context("parse answers as JSON object")?;
    Ok(parsed)
}

fn require_agent_id_env() -> anyhow::Result<String> {
    env::var("MURMUR_AGENT_ID").map_err(|_| anyhow!("MURMUR_AGENT_ID environment variable not set"))
}

fn confirm_yn(prompt: &str) -> anyhow::Result<bool> {
    eprint!("{prompt} [y/N] ");
    io::stderr().flush().ok();
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("read confirmation")?;
    let answer = input.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

async fn dispatch_server(command: ServerCommand, paths: &MurmurPaths) -> anyhow::Result<()> {
    match command {
        ServerCommand::Start { foreground } => server_start(foreground, paths).await,
        ServerCommand::Status => server_status(paths).await,
        ServerCommand::Stop => server_shutdown(paths).await,
        ServerCommand::Restart { foreground } => {
            let running = client::ping(paths).await.is_ok();
            if running {
                let _ = server_shutdown(paths).await;
                let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
                loop {
                    if tokio::time::Instant::now() > deadline {
                        break;
                    }
                    if client::ping(paths).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                }
            }

            server_start(foreground, paths).await
        }
    }
}

async fn dispatch_project(command: ProjectCommand, paths: &MurmurPaths) -> anyhow::Result<()> {
    match command {
        ProjectCommand::Add {
            input,
            name,
            remote_url,
            max_agents,
            autostart,
            backend,
        } => {
            let (name, remote_url) = if let Some(remote_url) = remote_url {
                if name.is_some() {
                    return Err(anyhow!("cannot use --name with --remote-url"));
                }
                (input, remote_url)
            } else if input_looks_like_git_url(&input) {
                let inferred = name
                    .or_else(|| infer_project_name_from_remote_url(&input))
                    .ok_or_else(|| anyhow!("could not infer project name; pass --name"))?;
                (inferred, input)
            } else {
                let abs = std::fs::canonicalize(&input)
                    .with_context(|| format!("resolve path: {input}"))?;
                let meta = std::fs::metadata(&abs)
                    .with_context(|| format!("stat path: {}", abs.display()))?;
                if !meta.is_dir() {
                    return Err(anyhow!("path is not a directory: {}", abs.display()));
                }

                let git = murmur::git::Git::default();
                let remote_url = git.remote_origin_url(&abs).await?;
                let inferred = name
                    .or_else(|| infer_project_name_from_path(&abs))
                    .or_else(|| infer_project_name_from_remote_url(&remote_url))
                    .ok_or_else(|| anyhow!("could not infer project name; pass --name"))?;
                (inferred, remote_url)
            };

            let _ = client::project_add(
                paths,
                name,
                remote_url,
                max_agents,
                Some(autostart),
                backend,
            )
            .await?;
            println!("ok");
            Ok(())
        }
        ProjectCommand::List => {
            let resp = client::project_list(paths).await?;
            for p in resp.projects {
                println!("{}\t{}", p.name, p.remote_url);
            }
            Ok(())
        }
        ProjectCommand::Start { project, all } => {
            if project.is_none() && !all {
                return Err(anyhow!("specify a project name or use --all"));
            }
            if project.is_some() && all {
                return Err(anyhow!("cannot use both a project name and --all"));
            }

            if all {
                let resp = client::project_list(paths).await?;
                for p in resp.projects {
                    client::orchestration_start(paths, p.name).await?;
                }
                println!("ok");
                return Ok(());
            }

            client::orchestration_start(
                paths,
                project.ok_or_else(|| anyhow!("project name is required"))?,
            )
            .await?;
            println!("ok");
            Ok(())
        }
        ProjectCommand::Stop {
            project,
            all,
            abort_agents,
        } => {
            if project.is_none() && !all {
                return Err(anyhow!("specify a project name or use --all"));
            }
            if project.is_some() && all {
                return Err(anyhow!("cannot use both a project name and --all"));
            }

            let projects = if all {
                let resp = client::project_list(paths).await?;
                resp.projects.into_iter().map(|p| p.name).collect()
            } else {
                vec![project.ok_or_else(|| anyhow!("project name is required"))?]
            };

            for project in &projects {
                let _ = client::orchestration_stop(paths, project.to_owned()).await;
            }

            if !abort_agents {
                println!("ok");
                return Ok(());
            }

            let resp = client::agent_list(paths).await?;
            let mut active_coding_agents = Vec::new();
            for agent in resp.agents {
                if !projects.iter().any(|p| p == &agent.project) {
                    continue;
                }
                if agent.role != murmur_protocol::AgentRole::Coding {
                    continue;
                }
                if matches!(
                    agent.state,
                    murmur_protocol::AgentState::Starting
                        | murmur_protocol::AgentState::Running
                        | murmur_protocol::AgentState::NeedsResolution
                ) {
                    active_coding_agents.push(agent.id);
                }
            }

            for agent_id in active_coding_agents {
                if let Err(err) = client::agent_abort(paths, agent_id, false).await {
                    let msg = err.to_string();
                    if msg.contains("agent not found") {
                        continue;
                    }
                    return Err(err);
                }
            }

            println!("ok");
            Ok(())
        }
        ProjectCommand::Remove {
            name,
            delete_worktrees,
        } => {
            client::project_remove(paths, name, delete_worktrees).await?;
            println!("ok");
            Ok(())
        }
        ProjectCommand::Status { name } => {
            let status = client::project_status(paths, name).await?;
            println!("name\t{}", status.name);
            println!("repo_dir\t{}", status.repo_dir);
            println!("repo_exists\t{}", status.repo_exists);
            println!("remote_url_configured\t{}", status.remote_url_configured);
            println!(
                "remote_url_actual\t{}",
                status.remote_url_actual.unwrap_or_default()
            );
            println!("remote_matches\t{}", status.remote_matches);
            println!("socket_reachable\t{}", status.socket_reachable);
            println!("orchestrator_running\t{}", status.orchestrator_running);
            Ok(())
        }
        ProjectCommand::Config { command } => dispatch_project_config(command, paths).await,
    }
}

async fn dispatch_project_config(
    command: ProjectConfigCommand,
    paths: &MurmurPaths,
) -> anyhow::Result<()> {
    match command {
        ProjectConfigCommand::Get { project, key } => {
            let resp = client::project_config_get(paths, project, key).await?;
            println!("{}", value_to_string(&resp.value));
            Ok(())
        }
        ProjectConfigCommand::Set {
            project,
            key,
            value,
        } => {
            client::project_config_set(paths, project, key, value).await?;
            println!("ok");
            Ok(())
        }
        ProjectConfigCommand::Show { project } => {
            let resp = client::project_config_show(paths, project).await?;
            println!("Project:\t{}", resp.name);
            println!();
            println!("Configuration:");
            for (k, v) in resp.config {
                println!("  {k}:\t{}", value_to_string(&v));
            }
            Ok(())
        }
    }
}

async fn dispatch_agent(command: AgentCommand, paths: &MurmurPaths) -> anyhow::Result<()> {
    match command {
        AgentCommand::Create {
            project,
            issue_id,
            backend,
        } => {
            let resp = client::agent_create(paths, project, issue_id, backend).await?;
            println!("{}", resp.agent.id);
            Ok(())
        }
        AgentCommand::List { project } => {
            let resp = client::agent_list(paths).await?;
            for a in resp.agents {
                if project.as_ref().is_some_and(|p| p != &a.project) {
                    continue;
                }
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    a.id,
                    a.project,
                    format_agent_role(a.role),
                    format_agent_state(a.state),
                    a.issue_id
                );
            }
            Ok(())
        }
        AgentCommand::Delete { agent_id } => {
            client::agent_delete(paths, agent_id).await?;
            println!("ok");
            Ok(())
        }
        AgentCommand::Abort {
            agent_id,
            force,
            yes,
        } => {
            if !yes {
                let prompt = if force {
                    format!("Force abort agent {agent_id}?")
                } else {
                    format!("Abort agent {agent_id}?")
                };
                if !confirm_yn(&prompt)? {
                    println!("canceled");
                    return Ok(());
                }
            }

            client::agent_abort(paths, agent_id, force).await?;
            println!("ok");
            Ok(())
        }
        AgentCommand::Claim { issue_id } => {
            let agent_id = require_agent_id_env()?;
            client::agent_claim(paths, agent_id, issue_id).await?;
            println!("ok");
            Ok(())
        }
        AgentCommand::Describe { description } => {
            let agent_id = require_agent_id_env()?;
            client::agent_describe(paths, agent_id, description).await?;
            println!("ok");
            Ok(())
        }
        AgentCommand::SendMessage { agent_id, message } => {
            client::agent_send_message(paths, agent_id, message).await?;
            println!("ok");
            Ok(())
        }
        AgentCommand::Tail { agent_id } => agent_tail(paths, agent_id).await,
        AgentCommand::ChatHistory { agent_id, limit } => {
            let resp = client::agent_chat_history(paths, agent_id, limit).await?;
            for m in resp.messages {
                println!("{}\t{}", format_chat_role(m.role), m.content);
            }
            Ok(())
        }
        AgentCommand::Done { task, error } => {
            let agent_id = require_agent_id_env()?;
            client::agent_done(paths, agent_id, task, error).await?;
            println!("ok");
            Ok(())
        }
        AgentCommand::Plan(args) => dispatch_agent_plan(args, paths).await,
    }
}

async fn dispatch_agent_plan(args: AgentPlanArgs, paths: &MurmurPaths) -> anyhow::Result<()> {
    match args.command {
        Some(AgentPlanSubcommand::List) => {
            let resp = client::plan_list(paths, args.project).await?;
            for a in resp.plans {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    a.id,
                    a.project,
                    format_agent_role(a.role),
                    format_agent_state(a.state),
                    a.issue_id
                );
            }
            Ok(())
        }
        Some(AgentPlanSubcommand::Stop { plan_id }) => {
            client::plan_stop(paths, plan_id).await?;
            println!("ok");
            Ok(())
        }
        None => {
            if args.prompt.is_empty() {
                return Err(anyhow!(
                    "prompt is required: mm agent plan \"your planning task\""
                ));
            }
            let prompt = args.prompt.join(" ");
            let resp = client::plan_start(paths, args.project, prompt).await?;
            println!("{}", resp.id);
            Ok(())
        }
    }
}

async fn agent_tail(paths: &MurmurPaths, agent_id: String) -> anyhow::Result<()> {
    use tokio::io::{BufReader, BufWriter};
    use tokio::net::UnixStream;

    let stream = UnixStream::connect(&paths.socket_path)
        .await
        .with_context(|| format!("connect: {}", paths.socket_path.display()))?;

    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut writer = BufWriter::new(write_half);

    let attach = murmur_protocol::Request {
        r#type: murmur_protocol::MSG_ATTACH.to_owned(),
        id: format!("tail-{}", now_ms()),
        payload: serde_json::to_value(murmur_protocol::AttachRequest::default())
            .context("serialize attach payload")?,
    };

    write_jsonl(&mut writer, &attach)
        .await
        .context("write attach request")?;

    loop {
        let Some(value) = read_jsonl::<_, serde_json::Value>(&mut reader)
            .await
            .context("read daemon stream")?
        else {
            break;
        };

        if value.get("success").is_some() {
            continue;
        }

        let evt: murmur_protocol::Event = match serde_json::from_value(value) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if evt.r#type != murmur_protocol::EVT_AGENT_CHAT {
            continue;
        }

        let chat: murmur_protocol::AgentChatEvent =
            serde_json::from_value(evt.payload).context("parse agent chat event")?;
        if chat.agent_id != agent_id {
            continue;
        }

        println!(
            "{}\t{}",
            format_chat_role(chat.message.role),
            chat.message.content
        );
    }

    Ok(())
}

fn format_agent_role(role: murmur_protocol::AgentRole) -> &'static str {
    match role {
        murmur_protocol::AgentRole::Coding => "coding",
        murmur_protocol::AgentRole::Planner => "planner",
        murmur_protocol::AgentRole::Manager => "manager",
    }
}

fn format_agent_state(state: murmur_protocol::AgentState) -> &'static str {
    match state {
        murmur_protocol::AgentState::Starting => "starting",
        murmur_protocol::AgentState::Running => "running",
        murmur_protocol::AgentState::NeedsResolution => "needs_resolution",
        murmur_protocol::AgentState::Exited => "exited",
        murmur_protocol::AgentState::Aborted => "aborted",
    }
}

fn format_chat_role(role: murmur_protocol::ChatRole) -> &'static str {
    match role {
        murmur_protocol::ChatRole::User => "user",
        murmur_protocol::ChatRole::Assistant => "assistant",
        murmur_protocol::ChatRole::Tool => "tool",
        murmur_protocol::ChatRole::System => "system",
    }
}

async fn dispatch_orchestration(
    command: OrchestrationCommand,
    paths: &MurmurPaths,
) -> anyhow::Result<()> {
    match command {
        OrchestrationCommand::Start { project, all } => {
            if project.is_none() && !all {
                return Err(anyhow!("specify a project name or use --all"));
            }
            if project.is_some() && all {
                return Err(anyhow!("cannot use both a project name and --all"));
            }

            if all {
                let resp = client::project_list(paths).await?;
                for p in resp.projects {
                    client::orchestration_start(paths, p.name).await?;
                }
                println!("ok");
                return Ok(());
            }

            client::orchestration_start(
                paths,
                project.ok_or_else(|| anyhow!("project name is required"))?,
            )
            .await?;
            println!("ok");
            Ok(())
        }
        OrchestrationCommand::Stop {
            project,
            all,
            abort_agents,
        } => {
            if project.is_none() && !all {
                return Err(anyhow!("specify a project name or use --all"));
            }
            if project.is_some() && all {
                return Err(anyhow!("cannot use both a project name and --all"));
            }

            let projects = if all {
                let resp = client::project_list(paths).await?;
                resp.projects.into_iter().map(|p| p.name).collect()
            } else {
                vec![project.ok_or_else(|| anyhow!("project name is required"))?]
            };

            for project in &projects {
                let _ = client::orchestration_stop(paths, project.to_owned()).await;
            }

            let resp = client::agent_list(paths).await?;
            let mut active = Vec::new();
            for agent in resp.agents {
                if !projects.iter().any(|p| p == &agent.project) {
                    continue;
                }
                if matches!(
                    agent.state,
                    murmur_protocol::AgentState::Starting
                        | murmur_protocol::AgentState::Running
                        | murmur_protocol::AgentState::NeedsResolution
                ) {
                    active.push(agent.id);
                }
            }

            if abort_agents {
                for agent_id in active {
                    if let Err(err) = client::agent_abort(paths, agent_id, true).await {
                        let msg = err.to_string();
                        if msg.contains("agent not found") {
                            continue;
                        }
                        return Err(err);
                    }
                }
            } else if !active.is_empty() {
                eprintln!(
                    "note: {} agent(s) still active; stop them with `mm agent abort <id>` or re-run with `--abort-agents`",
                    active.len()
                );
            }

            println!("ok");
            Ok(())
        }
        OrchestrationCommand::Status { project } => {
            let resp = client::orchestration_status(paths, project).await?;
            println!("project\t{}", resp.project);
            println!("running\t{}", resp.running);
            println!("max_agents\t{}", resp.max_agents);
            println!("active_agents\t{}", resp.active_agents);
            println!("active_claims\t{}", resp.active_claims);
            Ok(())
        }
    }
}

async fn dispatch_claim(command: ClaimCommand, paths: &MurmurPaths) -> anyhow::Result<()> {
    match command {
        ClaimCommand::List { project } => {
            let resp = client::claim_list(paths, project).await?;
            for c in resp.claims {
                println!("{}\t{}\t{}", c.project, c.issue_id, c.agent_id);
            }
            Ok(())
        }
    }
}

async fn dispatch_branch(command: BranchCommand) -> anyhow::Result<()> {
    match command {
        BranchCommand::Cleanup { dry_run, local } => branch_cleanup(dry_run, local).await,
    }
}

async fn dispatch_commit(command: CommitCommand, paths: &MurmurPaths) -> anyhow::Result<()> {
    match command {
        CommitCommand::List { project, limit } => {
            let resp = client::commit_list(paths, project, limit).await?;
            for c in resp.commits {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    c.project, c.sha, c.branch, c.agent_id, c.issue_id, c.merged_at_ms
                );
            }
            Ok(())
        }
    }
}

async fn branch_cleanup(dry_run: bool, local: bool) -> anyhow::Result<()> {
    let cwd = env::current_dir().context("get working directory")?;

    if !dry_run {
        let _ = run_git_quiet(&cwd, &["fetch", "--prune", "origin"]);
    }

    let base_ref = determine_default_base_ref(&cwd)?;

    let remote_branches = list_branches(&cwd, true)?
        .into_iter()
        .filter(|b| b.starts_with("origin/murmur/"))
        .collect::<Vec<_>>();

    let local_branches = if local {
        list_branches(&cwd, false)?
            .into_iter()
            .filter(|b| b.starts_with("murmur/"))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let mut merged_remote = Vec::new();
    for b in remote_branches {
        if is_branch_merged(&cwd, &base_ref, &b)? {
            merged_remote.push(b);
        }
    }

    let mut merged_local = Vec::new();
    for b in local_branches {
        if is_branch_merged(&cwd, &base_ref, &b)? {
            merged_local.push(b);
        }
    }

    if merged_remote.is_empty() && merged_local.is_empty() {
        println!("No merged murmur/* branches found");
        return Ok(());
    }

    if dry_run {
        println!("Dry run - would delete:");
        for b in &merged_remote {
            println!("[remote]\t{}", b.trim_start_matches("origin/"));
        }
        for b in &merged_local {
            println!("[local]\t{b}");
        }
        return Ok(());
    }

    for b in merged_remote {
        let name = b.trim_start_matches("origin/");
        let _ = run_git_quiet(&cwd, &["push", "origin", "--delete", name]);
    }
    for b in merged_local {
        let _ = run_git_quiet(&cwd, &["branch", "-d", &b]);
    }

    println!("ok");
    Ok(())
}

fn determine_default_base_ref(repo_dir: &std::path::Path) -> anyhow::Result<String> {
    let show = run_git(repo_dir, &["remote", "show", "origin"]).unwrap_or_default();
    if let Some(branch) = murmur::git::parse_default_branch_from_remote_show(&show) {
        let branch = branch.trim();
        if !branch.is_empty() && branch != "(unknown)" {
            return Ok(format!("origin/{branch}"));
        }
    }
    Ok("origin/main".to_owned())
}

fn list_branches(repo_dir: &std::path::Path, remote: bool) -> anyhow::Result<Vec<String>> {
    let mut args = vec!["branch"];
    if remote {
        args.push("-r");
    }
    let out = run_git(repo_dir, &args)?;
    let mut branches = Vec::new();
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.contains("->") {
            continue;
        }
        branches.push(trimmed.trim_start_matches('*').trim().to_owned());
    }
    Ok(branches)
}

fn is_branch_merged(
    repo_dir: &std::path::Path,
    base_ref: &str,
    branch: &str,
) -> anyhow::Result<bool> {
    let merge_base = match run_git(repo_dir, &["merge-base", branch, base_ref]) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };
    let merge_base = merge_base.trim();
    if merge_base.is_empty() {
        return Ok(false);
    }

    let cherry = match run_git(repo_dir, &["cherry", base_ref, branch, merge_base]) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };
    for line in cherry.lines() {
        if line.starts_with("+ ") {
            return Ok(false);
        }
    }
    Ok(true)
}

fn run_git(repo_dir: &std::path::Path, args: &[&str]) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(repo_dir)
        .args(args)
        .output()
        .with_context(|| format!("git {}", args.join(" ")))?;

    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run_git_quiet(repo_dir: &std::path::Path, args: &[&str]) -> anyhow::Result<()> {
    let status = std::process::Command::new("git")
        .current_dir(repo_dir)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("git {}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("git {} failed: {status}", args.join(" ")))
    }
}

async fn dispatch_issue(args: IssueArgs, paths: &MurmurPaths) -> anyhow::Result<()> {
    let project = resolve_issue_project(paths, args.project.as_deref()).await?;

    match args.command {
        IssueCommand::List { status } => {
            let mut resp = client::issue_list(paths, project).await?;
            if let Some(status) = status.as_deref() {
                let status = parse_issue_status(status)?;
                resp.issues.retain(|iss| iss.status == status);
            }
            for iss in resp.issues {
                println!(
                    "{}\t{}\t{}",
                    iss.id,
                    format!("{:?}", iss.status).to_ascii_lowercase(),
                    iss.title
                );
            }
            Ok(())
        }
        IssueCommand::Show { id } | IssueCommand::Get { id } => {
            let resp = client::issue_get(paths, project, id).await?;
            let iss = resp.issue;
            println!("id\t{}", iss.id);
            println!(
                "status\t{}",
                format!("{:?}", iss.status).to_ascii_lowercase()
            );
            println!("priority\t{}", iss.priority);
            println!("type\t{}", iss.issue_type);
            println!("title\t{}", iss.title);
            println!("created_at_ms\t{}", iss.created_at_ms);
            println!("deps\t{}", iss.dependencies.join(","));
            println!("labels\t{}", iss.labels.join(","));
            println!("links\t{}", iss.links.join(","));
            println!();
            if iss.description.trim().is_empty() {
                println!("No description");
            } else {
                println!("{}", iss.description);
            }
            Ok(())
        }
        IssueCommand::Ready => {
            let resp = client::issue_ready(paths, project).await?;
            for iss in resp.issues {
                println!("{}\t{}", iss.id, iss.title);
            }
            Ok(())
        }
        IssueCommand::Create {
            title,
            description,
            issue_type,
            priority,
            commit,
            mut depends_on,
            parent,
            label,
            link,
        } => {
            depends_on.retain(|s| !s.trim().is_empty());

            let parent = parent
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
            let is_tk_backend = if commit || parent.is_some() {
                project_issue_backend_is_tk(paths, &project).await?
            } else {
                false
            };
            if let Some(ref parent) = parent {
                match client::issue_get(paths, project.clone(), parent.clone()).await {
                    Ok(_) => {}
                    Err(err) => {
                        if is_tk_backend {
                            return Err(err).context("validate --parent");
                        }
                    }
                }
            }

            let mut deps = Vec::new();
            if let Some(parent) = parent {
                deps.push(parent);
            }
            deps.extend(depends_on);

            let req = murmur_protocol::IssueCreateRequest {
                project: project.clone(),
                title,
                description,
                issue_type: Some(issue_type),
                priority: Some(priority),
                labels: label,
                dependencies: deps,
                links: link,
            };

            let resp = client::issue_create(paths, req).await?;

            if commit {
                if !is_tk_backend {
                    return Err(anyhow!("--commit is only supported for tk issues"));
                }
                client::issue_commit(paths, project).await?;
            }

            println!("{}", resp.issue.id);
            Ok(())
        }
        IssueCommand::Update {
            id,
            title,
            status,
            priority,
            description,
            issue_type,
            label,
            clear_labels,
            dep,
            clear_deps,
            link,
            clear_links,
        } => {
            let status = status.as_deref().map(parse_issue_status).transpose()?;

            let labels = if clear_labels {
                Some(vec![])
            } else if label.is_empty() {
                None
            } else {
                Some(label)
            };

            let dependencies = if clear_deps {
                Some(vec![])
            } else if dep.is_empty() {
                None
            } else {
                Some(dep)
            };

            let links = if clear_links {
                Some(vec![])
            } else if link.is_empty() {
                None
            } else {
                Some(link)
            };

            let req = murmur_protocol::IssueUpdateRequest {
                project,
                id,
                title,
                description,
                status,
                priority,
                issue_type,
                labels,
                dependencies,
                links,
            };

            let resp = client::issue_update(paths, req).await?;
            println!("{}", resp.issue.id);
            Ok(())
        }
        IssueCommand::Close { id } => {
            client::issue_close(paths, project, id).await?;
            println!("ok");
            Ok(())
        }
        IssueCommand::Comment { id, body } => {
            let body = match body {
                Some(v) => v,
                None => read_stdin_string().context("read comment body from stdin")?,
            };
            if body.trim().is_empty() {
                return Err(anyhow!("comment body is empty"));
            }
            client::issue_comment(paths, project, id, body).await?;
            println!("ok");
            Ok(())
        }
        IssueCommand::Plan { id, body, file } => {
            if body.is_some() && file.is_some() {
                return Err(anyhow!("cannot specify both --body and --file"));
            }
            let plan = if let Some(path) = file {
                fs::read_to_string(&path)
                    .with_context(|| format!("read plan file: {}", path.display()))?
            } else {
                body.ok_or_else(|| anyhow!("plan content is required (use --body or --file)"))?
            };
            client::issue_plan(paths, project, id, plan).await?;
            println!("ok");
            Ok(())
        }
        IssueCommand::Commit => {
            client::issue_commit(paths, project).await?;
            println!("ok");
            Ok(())
        }
    }
}

async fn resolve_issue_project(
    paths: &MurmurPaths,
    project_override: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(project) = project_override.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(project.to_owned());
    }

    if let Ok(project) = env::var("MURMUR_PROJECT") {
        let project = project.trim();
        if !project.is_empty() {
            return Ok(project.to_owned());
        }
    }

    let cwd = env::current_dir().context("get cwd")?;
    let cwd = cwd.canonicalize().unwrap_or(cwd);

    let resp = client::project_list(paths).await?;
    let mut best: Option<(usize, String)> = None;
    for p in resp.projects {
        let repo_dir = std::path::PathBuf::from(&p.repo_dir);
        let Some(project_dir) = repo_dir.parent() else {
            continue;
        };
        if !cwd.starts_with(project_dir) {
            continue;
        }

        let depth = project_dir.components().count();
        let replace = match best.as_ref() {
            Some((best_depth, _)) => depth > *best_depth,
            None => true,
        };
        if replace {
            best = Some((depth, p.name));
        }
    }

    if let Some((_, project)) = best {
        return Ok(project);
    }

    Err(anyhow!(
        "could not determine project: not in a registered project directory\nUse --project flag or set MURMUR_PROJECT"
    ))
}

async fn project_issue_backend_is_tk(paths: &MurmurPaths, project: &str) -> anyhow::Result<bool> {
    let resp =
        client::project_config_get(paths, project.trim().to_owned(), "issue-backend".to_owned())
            .await?;

    Ok(resp
        .value
        .as_str()
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("tk")))
}

fn read_stdin_string() -> anyhow::Result<String> {
    use std::io::Read as _;

    let mut content = String::new();
    io::stdin().read_to_string(&mut content)?;
    Ok(content)
}

fn parse_issue_status(s: &str) -> anyhow::Result<murmur_protocol::IssueStatus> {
    match s.trim().to_ascii_lowercase().as_str() {
        "open" => Ok(murmur_protocol::IssueStatus::Open),
        "closed" => Ok(murmur_protocol::IssueStatus::Closed),
        "blocked" => Ok(murmur_protocol::IssueStatus::Blocked),
        other => Err(anyhow!("invalid status: {other}")),
    }
}

async fn dispatch_internal(command: InternalCommand) -> anyhow::Result<()> {
    match command {
        InternalCommand::DummyAgent { agent_id } => run_dummy_agent(&agent_id).await,
    }
}

async fn run_dummy_agent(agent_id: &str) -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let mut reader = tokio::io::BufReader::new(stdin);
    let mut writer = tokio::io::BufWriter::new(stdout);

    write_jsonl(
        &mut writer,
        &ChatMessage {
            role: ChatRole::Assistant,
            content: format!("dummy agent {agent_id} ready"),
            tool_name: None,
            tool_input: None,
            tool_use_id: None,
            tool_result: None,
            is_error: false,
            ts_ms: now_ms(),
        },
    )
    .await
    .context("write ready message")?;

    loop {
        let Some(msg) = read_jsonl::<_, ChatMessage>(&mut reader)
            .await
            .context("read message")?
        else {
            break;
        };

        let response = ChatMessage {
            role: ChatRole::Assistant,
            content: format!("(dummy {agent_id}) {}", msg.content),
            tool_name: None,
            tool_input: None,
            tool_use_id: None,
            tool_result: None,
            is_error: false,
            ts_ms: now_ms(),
        };

        write_jsonl(&mut writer, &response)
            .await
            .context("write response")?;
    }

    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "".to_owned(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn plan_path(paths: &MurmurPaths, plan_id: &str) -> anyhow::Result<PathBuf> {
    let id = plan_id.trim().trim_end_matches(".md");
    let filename = format!("{id}.md");
    let path = murmur_core::paths::safe_join(&paths.plans_dir, &filename)
        .map_err(|e| anyhow!("invalid plan id {plan_id:?}: {e}"))?;
    Ok(path)
}

fn plan_write(paths: &MurmurPaths) -> anyhow::Result<()> {
    use std::io::Read as _;

    let agent_id = require_agent_id_env()?;
    let plan_id = agent_id.trim().trim_start_matches("plan:").to_owned();
    if plan_id.trim().is_empty() {
        return Err(anyhow!("plan id is empty"));
    }

    let mut content = String::new();
    io::stdin().read_to_string(&mut content)?;

    fs::create_dir_all(&paths.plans_dir)
        .with_context(|| format!("create plans dir: {}", paths.plans_dir.display()))?;

    let dest = plan_path(paths, &plan_id)?;
    let tmp = plan_path(paths, &format!("{plan_id}.tmp"))?;

    fs::write(&tmp, content).with_context(|| format!("write tmp: {}", tmp.display()))?;
    fs::rename(&tmp, &dest).with_context(|| format!("rename to: {}", dest.display()))?;

    println!("{}", plan_id.trim().trim_end_matches(".md"));
    Ok(())
}

fn plan_list_stored(paths: &MurmurPaths) -> anyhow::Result<()> {
    let entries = match fs::read_dir(&paths.plans_dir) {
        Ok(v) => v,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            println!("No stored plans");
            return Ok(());
        }
        Err(err) => {
            return Err(err).with_context(|| format!("read {}", paths.plans_dir.display()));
        }
    };

    let mut plans = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(v) => v,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(UNIX_EPOCH);
        plans.push((stem.to_owned(), modified));
    }

    if plans.is_empty() {
        println!("No stored plans");
        return Ok(());
    }

    plans.sort_by(|a, b| b.1.cmp(&a.1));

    let fmt = time::format_description::parse("[year]-[month]-[day] [hour]:[minute]").unwrap();
    println!("ID\tMODIFIED");
    for (id, modified) in plans {
        let when: time::OffsetDateTime = modified.into();
        let ts = when.format(&fmt).unwrap_or_else(|_| "-".to_owned());
        println!("{id}\t{ts}");
    }

    Ok(())
}

fn input_looks_like_git_url(input: &str) -> bool {
    let input = input.trim();
    input.contains("://") || input.starts_with("git@")
}

fn infer_project_name_from_path(path: &std::path::Path) -> Option<String> {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_owned())
}

fn infer_project_name_from_remote_url(remote_url: &str) -> Option<String> {
    let trimmed = remote_url.trim().trim_end_matches('/');
    let after_colon = trimmed.rsplit(':').next().unwrap_or(trimmed);
    let last = after_colon.rsplit('/').next().unwrap_or(after_colon);
    let last = last.strip_suffix(".git").unwrap_or(last).trim();
    (!last.is_empty()).then(|| last.to_owned())
}

async fn server_start(foreground: bool, paths: &MurmurPaths) -> anyhow::Result<()> {
    if foreground {
        return daemon::run_foreground(paths).await;
    }

    if paths.socket_path.exists() && client::ping(paths).await.is_ok() {
        println!("running");
        return Ok(());
    }

    let exe = std::env::current_exe().context("get current executable")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["server", "start", "--foreground"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    // Ensure the spawned foreground daemon uses the same socket path as this process, even when
    // the caller used CLI flags instead of env vars.
    cmd.env("MURMUR_SOCKET_PATH", &paths.socket_path);
    if paths.config_dir == paths.murmur_dir.join("config") {
        cmd.env("MURMUR_DIR", &paths.murmur_dir);
    }

    let child = cmd.spawn().context("spawn daemon")?;
    let pid = child.id();

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
    loop {
        if tokio::time::Instant::now() > deadline {
            return Err(anyhow!("timed out waiting for daemon to start (pid {pid})"));
        }
        if client::ping(paths).await.is_ok() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }

    println!("{pid}");
    Ok(())
}

async fn server_status(paths: &MurmurPaths) -> anyhow::Result<()> {
    let running = if !paths.socket_path.exists() {
        false
    } else {
        client::ping(paths).await.is_ok()
    };

    println!("{}", if running { "running" } else { "stopped" });
    tracing::info!(
        running,
        socket = %paths.socket_path.display(),
        "server status"
    );
    Ok(())
}

async fn server_shutdown(paths: &MurmurPaths) -> anyhow::Result<()> {
    client::shutdown(paths).await?;
    println!("ok");
    Ok(())
}

async fn ping(paths: &MurmurPaths) -> anyhow::Result<()> {
    let _ = client::ping(paths).await?;
    println!("ok");
    Ok(())
}

async fn stats(paths: &MurmurPaths, project: Option<String>) -> anyhow::Result<()> {
    let resp = client::stats(paths, project).await?;
    println!("commit_count\t{}", resp.commit_count);
    println!("usage_output_tokens\t{}", resp.usage.output_tokens);
    println!("usage_percent\t{}", resp.usage.percent);
    println!("usage_window_end\t{}", resp.usage.window_end);
    println!("usage_time_left\t{}", resp.usage.time_left);
    println!("usage_plan_limit\t{}", resp.usage.plan_limit);
    println!("usage_plan\t{}", resp.usage.plan);
    Ok(())
}

fn version() -> anyhow::Result<()> {
    println!("{}", env!("CARGO_PKG_VERSION"));
    Ok(())
}

async fn status(paths: &MurmurPaths, show_agents: bool) -> anyhow::Result<()> {
    use std::io::Write as _;

    let ping = match client::ping(paths).await {
        Ok(v) => v,
        Err(_) => {
            println!("stopped");
            return Ok(());
        }
    };

    let uptime = format_duration_ms(ping.uptime_ms);
    println!("running\tpid={}\tuptime={}", ping.pid, uptime);

    let projects = client::project_list(paths).await?.projects;
    if projects.is_empty() {
        println!("No projects registered");
        return Ok(());
    }

    let agents = client::agent_list(paths).await?.agents;

    let mut w = std::io::BufWriter::new(std::io::stdout());
    writeln!(&mut w)?;
    writeln!(&mut w, "PROJECT\tSTATUS\tAGENTS\tREMOTE")?;
    for p in &projects {
        let orch = client::orchestration_status(paths, p.name.clone())
            .await
            .ok();
        let running = orch.as_ref().is_some_and(|s| s.running);
        let active = orch.as_ref().map(|s| s.active_agents).unwrap_or(0);
        let status = if running { "running" } else { "stopped" };
        writeln!(
            &mut w,
            "{}\t{}\t{}/{}\t{}",
            p.name, status, active, p.max_agents, p.remote_url
        )?;
    }

    if show_agents {
        writeln!(&mut w)?;
        writeln!(&mut w, "AGENT\tPROJECT\tROLE\tSTATE\tISSUE\tDESCRIPTION")?;
        for a in agents {
            let desc = a.description.unwrap_or_else(|| "-".to_owned());
            writeln!(
                &mut w,
                "{}\t{}\t{}\t{}\t{}\t{}",
                a.id,
                a.project,
                format_agent_role(a.role),
                format_agent_state(a.state),
                a.issue_id,
                desc
            )?;
        }
    }

    w.flush()?;
    Ok(())
}

fn format_duration_ms(ms: u64) -> String {
    if ms < 1000 {
        return format!("{ms}ms");
    }
    let secs = ms / 1000;
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = hours / 24;
    format!("{days}d")
}

async fn attach(paths: &MurmurPaths, projects: Vec<String>) -> anyhow::Result<()> {
    use tokio::io::{BufReader, BufWriter};
    use tokio::net::UnixStream;

    let stream = UnixStream::connect(&paths.socket_path)
        .await
        .with_context(|| format!("connect: {}", paths.socket_path.display()))?;

    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut writer = BufWriter::new(write_half);

    let attach = murmur_protocol::Request {
        r#type: murmur_protocol::MSG_ATTACH.to_owned(),
        id: format!("attach-{}", now_ms()),
        payload: serde_json::to_value(murmur_protocol::AttachRequest { projects })
            .context("serialize attach payload")?,
    };
    write_jsonl(&mut writer, &attach)
        .await
        .context("write attach request")?;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                let detach = murmur_protocol::Request {
                    r#type: murmur_protocol::MSG_DETACH.to_owned(),
                    id: format!("detach-{}", now_ms()),
                    payload: serde_json::Value::Null,
                };
                let _ = write_jsonl(&mut writer, &detach).await;
                break;
            }
            value = read_jsonl::<_, serde_json::Value>(&mut reader) => {
                let Some(value) = value.context("read daemon stream")? else { break };
                if value.get("success").is_some() {
                    continue;
                }
                let evt: murmur_protocol::Event = match serde_json::from_value(value) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                print_event(evt)?;
            }
        }
    }

    Ok(())
}

async fn tui(paths: &MurmurPaths) -> anyhow::Result<()> {
    murmur::tui::run(paths).await
}

fn print_event(evt: murmur_protocol::Event) -> anyhow::Result<()> {
    match evt.r#type.as_str() {
        murmur_protocol::EVT_AGENT_CHAT => {
            let chat: murmur_protocol::AgentChatEvent =
                serde_json::from_value(evt.payload).context("parse agent chat event")?;
            println!(
                "[{}:{}]\t{}\t{}",
                chat.project,
                chat.agent_id,
                format_chat_role(chat.message.role),
                chat.message.content
            );
        }
        murmur_protocol::EVT_PERMISSION_REQUEST => {
            let req: murmur_protocol::PermissionRequest =
                serde_json::from_value(evt.payload).context("parse permission request")?;
            let primary =
                murmur_core::permissions::resolve_primary_field(&req.tool_name, &req.tool_input);
            println!(
                "[{}:{}]\tpermission\t{}\t{}",
                req.project, req.agent_id, req.tool_name, primary
            );
        }
        murmur_protocol::EVT_USER_QUESTION => {
            let req: murmur_protocol::UserQuestion =
                serde_json::from_value(evt.payload).context("parse user question")?;
            println!("[{}:{}]\tquestion\t{}", req.project, req.agent_id, req.id);
        }
        murmur_protocol::EVT_AGENT_IDLE => {
            let agent_id = evt
                .payload
                .get("agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let project = evt
                .payload
                .get("project")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            println!("[{}:{}]\tidle", project, agent_id);
        }
        murmur_protocol::EVT_ORCHESTRATION_TICK_REQUESTED => {
            let tick: murmur_protocol::OrchestrationTickRequestedEvent =
                serde_json::from_value(evt.payload).context("parse tick event")?;
            println!("[{}]\ttick-requested\tsource={}", tick.project, tick.source);
        }
        _ => {}
    }
    Ok(())
}
