# Murmur

**A local multi-agent orchestrator for AI coding assistants**

Murmur is a daemon-based supervisor that manages multiple Claude Code or Codex CLI instances across your projects. It isolates each agent in its own git worktree, assigns work from pluggable issue backends, and provides a CLI and TUI for monitoring and approvals.

```
                    ┌─────────────────────────────────────────────────┐
                    │                    Murmur Daemon                │
                    │  ┌─────────────┐  ┌─────────────┐  ┌─────────┐  │
   Issue Backends   │  │ Orchestrator│  │   Claims    │  │ Webhook │  │
  ┌──────────────┐  │  │  (per proj) │  │  Registry   │  │ Server  │  │
  │ tk (local)   │──│  └──────┬──────┘  └─────────────┘  └─────────┘  │
  │ GitHub Issues│  │         │                                       │
  │ Linear       │  │         ▼                                       │
  └──────────────┘  │  ┌─────────────────────────────────────────┐    │
                    │  │              Agent Manager               │    │
                    │  │  ┌─────────┐ ┌─────────┐ ┌─────────┐    │    │
                    │  │  │ Agent 1 │ │ Agent 2 │ │ Agent N │    │    │
                    │  │  │(worktree)│ │(worktree)│ │(worktree)│    │    │
                    │  │  └─────────┘ └─────────┘ └─────────┘    │    │
                    │  └─────────────────────────────────────────┘    │
                    └─────────────────────────────────────────────────┘
                                          │
                              Unix Socket (IPC)
                                          │
                    ┌─────────────────────────────────────────────────┐
                    │           CLI (mm) / TUI (mm tui)               │
                    └─────────────────────────────────────────────────┘
```

## Key Features

- **Multi-agent orchestration** — Automatically spawn and manage multiple AI agents per project
- **Git worktree isolation** — Each agent works in its own worktree, enabling safe concurrent development
- **Pluggable issue backends** — Use local tickets (`tk`), GitHub Issues, or Linear
- **Permission controls** — Rule-based allow/deny for Claude tools, with manual or LLM-based approval
- **Direct merge pipeline** — Agents complete work, Murmur rebases and merges to your default branch
- **Interactive supervision** — TUI for real-time monitoring, approvals, and agent interaction
- **Planner and Manager agents** — Special agent modes for planning work and coordinating projects

## Quick Start

### Prerequisites

- Rust toolchain (`cargo`)
- Git
- At least one agent CLI: `claude` (Claude Code) or `codex` (Codex CLI)

### Install

```bash
cargo install --locked --path crates/murmur
```

### Run

**Terminal 1** — Start the daemon:
```bash
mm server start --foreground
```

**Terminal 2** — Add a project and start orchestration:
```bash
# Add your project (clones into ~/.murmur/projects/)
mm project add https://github.com/yourorg/yourrepo --name myproj

# Create an issue (using local tk backend)
mm issue create --project myproj "Implement feature X"

# Start orchestration
mm project start myproj

# Watch agents work
mm tui
```

## How It Works

1. **Register projects** — Point Murmur at your git repositories
2. **Configure issue backend** — Choose `tk` (local), GitHub, or Linear
3. **Start orchestration** — Murmur polls for ready issues and spawns agents
4. **Agents work autonomously** — Each in an isolated git worktree
5. **Approve tool usage** — Via TUI or CLI when agents need permissions
6. **Automatic merge** — On completion, Murmur rebases and merges to your default branch

## Documentation

| Document | Description |
|----------|-------------|
| [Getting Started](docs/GETTING_STARTED.md) | 5-minute quickstart guide |
| [Usage Guide](docs/USAGE.md) | Comprehensive usage documentation |
| [CLI Reference](docs/CLI.md) | Complete command reference |
| [TUI Guide](docs/TUI.md) | Terminal UI keybindings and features |
| [Architecture](docs/ARCHITECTURE.md) | System design and internals |
| [Development](docs/DEVELOPMENT.md) | Contributing and local development |

### Component Deep Dives

| Component | Description |
|-----------|-------------|
| [Agents](docs/components/AGENTS.md) | Agent lifecycle, backends, chat history |
| [Orchestration](docs/components/ORCHESTRATION.md) | Spawn policy, claims, tick loop |
| [Issue Backends](docs/components/ISSUE_BACKENDS.md) | tk, GitHub, Linear backends |
| [Permissions](docs/components/PERMISSIONS_AND_QUESTIONS.md) | Rules, hooks, approvals |
| [Worktrees & Merge](docs/components/WORKTREES_AND_MERGE.md) | Git isolation and merge pipeline |
| [Configuration](docs/components/CONFIG.md) | config.toml reference |

## Example Workflows

### Autonomous Bug Fixing

```bash
# Add your project
mm project add git@github.com:myorg/myapp.git --name myapp

# Configure GitHub Issues backend
mm project config set myapp issue-backend github

# Start orchestration (agents will pick up open issues)
mm project start myapp

# Monitor via TUI
mm tui
```

### Planning Mode

```bash
# Start a planner agent to design a feature
mm agent plan --project myapp "Design the authentication system"

# View the generated plan
mm plan read plan-1
```

### Interactive Manager

```bash
# Start a manager agent for exploring the codebase
mm manager start myapp

# Interact via TUI
mm tui
```

## Configuration

Global config lives at `~/.config/murmur/config.toml`:

```toml
log-level = "info"

# API provider credentials
[providers.github]
token = "ghp_..."

[providers.anthropic]
api-key = "sk-ant-..."

# Webhook server (optional)
[webhook]
enabled = true
bind-addr = "127.0.0.1:8080"
secret = "your-webhook-secret"

# Project configuration
[[projects]]
name = "myapp"
remote-url = "git@github.com:myorg/myapp.git"
max-agents = 3
issue-backend = "github"
agent-backend = "claude"
autostart = true
```

Permission rules live at `~/.config/murmur/permissions.toml`:

```toml
[[rules]]
tool = "Bash"
action = "allow"
pattern = "cargo *"

[[rules]]
tool = "Bash"
action = "deny"
pattern = "rm -rf *"

[[rules]]
tool = "Read"
action = "allow"
```

## Architecture

Murmur follows a **Functional Core, Imperative Shell** architecture:

- **`murmur-core`** — Pure domain logic (no I/O)
- **`murmur-protocol`** — IPC message types
- **`murmur`** — Daemon, CLI, and all I/O operations

The daemon owns all state and I/O. The CLI communicates via Unix socket IPC.

## Requirements

- **OS**: Linux, macOS (Unix socket required)
- **Rust**: 1.75+ (for building)
- **Git**: 2.20+ (for worktree support)
- **Agent CLI**: Claude Code (`claude`) or Codex CLI (`codex`)

## License

[License information here]

## Contributing

See [DEVELOPMENT.md](docs/DEVELOPMENT.md) for build instructions and contribution guidelines.
