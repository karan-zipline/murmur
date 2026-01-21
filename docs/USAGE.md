# Usage Guide

This guide covers all Murmur features in depth. For a quick introduction, see [Getting Started](GETTING_STARTED.md).

## Table of Contents

- [Overview](#overview)
- [The Daemon](#the-daemon)
- [Managing Projects](#managing-projects)
- [Issue Backends](#issue-backends)
- [Orchestration](#orchestration)
- [Working with Agents](#working-with-agents)
- [Permissions and Approvals](#permissions-and-approvals)
- [Planner Agents](#planner-agents)
- [Manager Agents](#manager-agents)
- [The Terminal UI](#the-terminal-ui)
- [Webhooks](#webhooks)
- [Configuration Reference](#configuration-reference)
- [Environment Variables](#environment-variables)

---

## Overview

Murmur is a daemon-based orchestrator. The typical workflow is:

```
┌─────────────────────────────────────────────────────────────────┐
│  1. Start daemon          mm server start                       │
│  2. Add projects          mm project add <url>                  │
│  3. Create/import issues  mm issue create / GitHub / Linear    │
│  4. Start orchestration   mm project start <project>            │
│  5. Monitor & approve     mm tui                                │
│  6. Agents complete work  (automatic merge & close)             │
└─────────────────────────────────────────────────────────────────┘
```

All commands communicate with the daemon over a Unix socket. If the daemon isn't running, commands will fail with a connection error.

---

## The Daemon

The daemon is the control plane. It manages projects, spawns agents, handles permissions, and coordinates merges.

### Starting the Daemon

**Foreground** (recommended for development):
```bash
mm server start --foreground
```

**Background**:
```bash
mm server start
```

### Checking Status

```bash
mm server status
```

Output shows whether the daemon is running and basic health info.

### Stopping the Daemon

```bash
mm server stop
```

This gracefully shuts down orchestrators and agents.

### Restarting

```bash
mm server restart
```

### Logs

Daemon logs are written to `~/.murmur/murmur.log` (or `$MURMUR_DIR/murmur.log`).

```bash
tail -f ~/.murmur/murmur.log
```

---

## Managing Projects

Projects are git repositories that Murmur manages. Each project has its own configuration, agents, and worktrees.

### Adding a Project

```bash
# From a URL (clones the repo)
mm project add https://github.com/org/repo.git --name myproj

# With options
mm project add https://github.com/org/repo.git \
  --name myproj \
  --max-agents 5 \
  --backend claude \
  --autostart
```

The repository is cloned to `~/.murmur/projects/myproj/repo/`.

### Listing Projects

```bash
mm project list
```

### Viewing Project Configuration

```bash
mm project config show myproj
```

### Modifying Configuration

```bash
# Get a single value
mm project config get myproj max-agents

# Set a value
mm project config set myproj max-agents 5
mm project config set myproj issue-backend github
mm project config set myproj agent-backend claude
```

### Project Status

```bash
mm project status myproj
```

Shows:
- Whether the repo exists and matches the configured remote
- Whether orchestration is running
- Number of active agents

### Removing a Project

```bash
# Unregister only (keeps files)
mm project remove myproj

# Also delete worktrees
mm project remove myproj --delete-worktrees
```

---

## Issue Backends

Murmur supports three issue backends. Each project uses one backend at a time.

### Local Tickets (`tk`)

The default backend. Issues are stored as Markdown files in the repository under `.murmur/tickets/`.

```bash
# Create an issue
mm issue create --project myproj "Implement user authentication"

# With more details
mm issue create --project myproj "Fix login bug" \
  --description "Users can't log in with special characters in passwords" \
  --type bug \
  --priority 1

# List issues
mm issue list --project myproj

# Show a specific issue
mm issue show ISSUE-1 --project myproj

# List ready issues (open, no open dependencies)
mm issue ready --project myproj

# Update an issue
mm issue update ISSUE-1 --project myproj --status blocked
mm issue update ISSUE-1 --project myproj --priority 2

# Close an issue
mm issue close ISSUE-1 --project myproj

# Add a comment
mm issue comment ISSUE-1 --project myproj --body "Initial investigation complete"

# Commit and push ticket changes
mm issue commit --project myproj
```

See [TICKETS.md](TICKETS.md) for the file format specification.

### GitHub Issues

Use GitHub Issues as your issue backend.

**Setup:**
```bash
# Set your token (or add to config.toml)
export GITHUB_TOKEN=ghp_...

# Switch backend
mm project config set myproj issue-backend github
```

**Optional: Filter by author**
```bash
# Only pick up issues from specific authors
mm project config set myproj allowed-authors '["octocat", "dependabot"]'
```

The `owner/repo` is automatically detected from the project's git remote.

### Linear Issues

Use Linear as your issue backend.

**Setup:**
```bash
# Set your API key
export LINEAR_API_KEY=lin_api_...

# Configure the project
mm project config set myproj issue-backend linear
mm project config set myproj linear-team YOUR_TEAM_UUID

# Optional: scope to a specific Linear project
mm project config set myproj linear-project YOUR_PROJECT_UUID
```

---

## Orchestration

Orchestration is the automatic spawning of agents for ready issues.

### Starting Orchestration

```bash
# Single project
mm project start myproj

# All projects
mm project start --all
```

### Stopping Orchestration

```bash
mm project stop myproj
mm project stop --all
```

### How It Works

The orchestrator runs a loop (approximately every 10 seconds):

1. **Query ready issues** from the configured backend
2. **Filter** issues that are already claimed
3. **Spawn agents** up to `max-agents` for unclaimed issues
4. **Claim** each issue to prevent duplicate work

### Viewing Claims

```bash
mm claims --project myproj
```

Shows which issues are assigned to which agents.

### Auto-start

To start orchestration automatically when the daemon starts:

```bash
mm project config set myproj autostart true
```

---

## Working with Agents

Agents are AI coding assistants (Claude Code or Codex) working in isolated git worktrees.

### Listing Agents

```bash
# All agents
mm agent list

# Filter by project
mm agent list --project myproj
```

### Creating an Agent Manually

Normally the orchestrator creates agents, but you can create one manually:

```bash
mm agent create myproj ISSUE-1
```

### Aborting an Agent

```bash
# Interactive (prompts for confirmation)
mm agent abort a-1

# Force without prompt
mm agent abort a-1 --yes

# Force kill if graceful abort fails
mm agent abort a-1 --force
```

### Agent Lifecycle

1. **Starting** — Agent process is being spawned
2. **Running** — Agent is actively working
3. **Needs Resolution** — A merge conflict or error requires intervention
4. **Exited** — Agent completed successfully
5. **Aborted** — Agent was manually stopped

### Merge on Completion

When an agent completes (`mm agent done`):

1. Murmur rebases the agent branch onto `origin/<default-branch>`
2. Performs a fast-forward merge
3. Pushes to origin
4. Closes the issue
5. Releases the claim
6. Removes the worktree

If a merge conflict occurs, the agent transitions to "needs resolution" and the worktree is preserved for manual intervention.

### Branch Cleanup

After agents complete, their branches may remain. Clean them up:

```bash
# Dry run (see what would be deleted)
mm branch cleanup --dry-run

# Delete remote branches
mm branch cleanup

# Also delete local branches
mm branch cleanup --local
```

---

## Permissions and Approvals

Murmur intercepts Claude Code tool calls and can require approval before execution.

### How It Works

1. Agent attempts to use a tool (e.g., `Bash`, `Write`)
2. Claude invokes `mm hook PreToolUse`
3. Murmur evaluates permission rules
4. If rules don't decide, the request goes to the daemon
5. You approve/deny via TUI or CLI

### Permission Rules

Create `~/.config/murmur/permissions.toml`:

```toml
# Allow reading any file
[[rules]]
tool = "Read"
action = "allow"

# Allow specific bash commands
[[rules]]
tool = "Bash"
action = "allow"
pattern = "cargo *"

[[rules]]
tool = "Bash"
action = "allow"
pattern = "npm *"

[[rules]]
tool = "Bash"
action = "allow"
pattern = "git status"

# Deny dangerous commands
[[rules]]
tool = "Bash"
action = "deny"
pattern = "rm -rf *"

[[rules]]
tool = "Bash"
action = "deny"
pattern = "sudo *"
```

Rules are evaluated in order. First match wins. If no rule matches, the request goes to manual approval.

### Project-Specific Rules

Create `~/.murmur/projects/myproj/permissions.toml` for project-specific rules. These are evaluated before global rules.

### Manual Approval

**Via TUI** (recommended):
```bash
mm tui
# Press y to allow, n to deny when prompted
```

**Via CLI:**
```bash
mm permission list
mm permission respond REQ-123 allow
mm permission respond REQ-123 deny
```

### LLM-Based Approval

Let an LLM decide permissions automatically:

```bash
mm project config set myproj permissions-checker llm
```

Configure the LLM in `config.toml`:

```toml
[llm_auth]
provider = "anthropic"
model = "claude-haiku-4-5"
```

LLM mode is fail-closed: if the LLM is unsure or there's an error, the request is denied.

---

## Planner Agents

Planner agents explore codebases and write planning documents without implementing code.

### Starting a Planner

```bash
mm agent plan --project myproj "Design the authentication system"
```

This creates a plan artifact at `~/.murmur/plans/plan-1.md`.

### Listing Running Planners

```bash
mm agent plan list --project myproj
```

### Viewing Plan Output

```bash
mm plan list        # List all stored plans
mm plan read plan-1 # Show plan contents
```

### Stopping a Planner

```bash
mm agent plan stop plan-1
```

### Project-less Planners

Planners can run without a project for general planning:

```bash
mm agent plan "Compare authentication libraries for Node.js"
```

---

## Manager Agents

Manager agents are interactive coordinators for a project. They can explore the codebase, create issues, and monitor work—but they don't implement code themselves.

### Starting a Manager

```bash
mm manager start myproj
```

### Interacting with the Manager

Use the TUI to send messages:

```bash
mm tui
# Select the manager agent and type your questions
```

### Manager Status

```bash
mm manager status myproj
```

### Stopping the Manager

```bash
mm manager stop myproj
```

### Clearing Manager History

```bash
mm manager clear myproj
```

### Manager Permissions

Managers have restricted tool access by default. Configure in `permissions.toml`:

```toml
[manager]
allowed_patterns = ["murmur:*", "git:*", "Read:*"]
```

---

## The Terminal UI

The TUI (`mm tui`) provides real-time monitoring and interaction.

### Layout

```
┌──────────────────────────────────────────────────────────────────────────┐
│ Header: Connection status, agent count, pending approvals                │
├─────────────────────────────┬────────────────────────────────────────────┤
│ Agent List                  │ Chat View                                  │
│ ┌─────────────────────────┐ │ ┌────────────────────────────────────────┐ │
│ │ a-1 myproj [R]          │ │ │ Assistant: I'll start by...           │ │
│ │ a-2 myproj [R]          │ │ │ User: (tool approved)                 │ │
│ │ plan-1 [P]              │ │ │ Assistant: Now implementing...        │ │
│ └─────────────────────────┘ │ └────────────────────────────────────────┘ │
│ Recent Commits              │ Input (when composing)                     │
│ ┌─────────────────────────┐ │ ┌────────────────────────────────────────┐ │
│ │ abc123 Fix bug          │ │ │ Type your message...                  │ │
│ └─────────────────────────┘ │ └────────────────────────────────────────┘ │
└─────────────────────────────┴────────────────────────────────────────────┘
```

### Keybindings

| Key | Action |
|-----|--------|
| `Tab` | Switch focus between agent list and chat |
| `j/k` | Move selection / scroll |
| `PgUp/PgDn` | Page scroll in chat |
| `Enter` | Open input to send a message |
| `p` | Start a new planner |
| `t` | Toggle tool call visibility |
| `x` | Abort/stop selected agent |
| `y/n` | Allow/deny permission (when prompted) |
| `r` | Reconnect (if disconnected) |
| `q` | Quit |

### Permission Prompts

When an agent needs approval, the chat pane shows the tool call details. Press `y` to allow or `n` to deny.

### User Questions

Agents can ask questions via `AskUserQuestion`. The TUI shows options—navigate with `j/k` and select with `y`, or choose "Other" for a custom answer.

---

## Webhooks

Murmur can receive webhooks from GitHub or Linear to trigger immediate orchestration ticks.

### Enable Webhooks

In `config.toml`:

```toml
[webhook]
enabled = true
bind-addr = "127.0.0.1:8080"
path-prefix = "/webhooks"
secret = "your-shared-secret"
```

### Endpoints

- `GET /health` — Health check
- `POST /webhooks/github?project=<name>` — GitHub webhook
- `POST /webhooks/linear?project=<name>` — Linear webhook

### Signature Verification

- GitHub: Validates `X-Hub-Signature-256`
- Linear: Validates `Linear-Signature`

Both use HMAC-SHA256 with the configured secret.

### Exposing Webhooks

For local development, use a tunnel like ngrok:

```bash
ngrok http 8080
```

Then configure your GitHub/Linear webhook to point to the ngrok URL.

---

## Configuration Reference

### Global Config (`~/.config/murmur/config.toml`)

```toml
log-level = "info"

# Provider credentials
[providers.github]
token = "ghp_..."

[providers.linear]
api-key = "lin_api_..."

[providers.anthropic]
api-key = "sk-ant-..."

[providers.openai]
api-key = "sk-..."

# LLM-based permission decisions
[llm_auth]
provider = "anthropic"
model = "claude-haiku-4-5"

# Webhook server
[webhook]
enabled = true
bind-addr = "127.0.0.1:8080"
path-prefix = "/webhooks"
secret = "your-secret"

# Project definitions
[[projects]]
name = "myproj"
remote-url = "git@github.com:org/repo.git"
max-agents = 3
issue-backend = "github"
agent-backend = "claude"
permissions-checker = "manual"
merge-strategy = "direct"
autostart = true
```

### Project Settings

| Setting | Values | Default | Description |
|---------|--------|---------|-------------|
| `name` | string | — | Project identifier |
| `remote-url` | URL | — | Git remote URL |
| `max-agents` | 1-10 | 3 | Max concurrent coding agents |
| `issue-backend` | `tk`, `github`, `linear` | `tk` | Issue source |
| `agent-backend` | `claude`, `codex` | `claude` | AI backend |
| `coding-backend` | `claude`, `codex` | (inherits) | Override for coding agents |
| `planner-backend` | `claude`, `codex` | (inherits) | Override for planners |
| `permissions-checker` | `manual`, `llm` | `manual` | How to handle permissions |
| `merge-strategy` | `direct`, `pull-request` | `direct` | How to merge completed work |
| `autostart` | bool | false | Auto-start orchestration |
| `allowed-authors` | list | [] | Filter issues by author (GitHub) |
| `linear-team` | UUID | — | Required for Linear backend |
| `linear-project` | UUID | — | Optional Linear project filter |

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `MURMUR_DIR` | Override base directory (default: `~/.murmur`) |
| `MURMUR_LOG` | Log level filter (e.g., `debug`, `info`) |
| `MURMUR_AGENT_ID` | Used by agent commands (`claim`, `done`, etc.) |
| `GITHUB_TOKEN` / `GH_TOKEN` | GitHub API token |
| `LINEAR_API_KEY` | Linear API key |
| `ANTHROPIC_API_KEY` | Anthropic API key (for LLM auth) |
| `OPENAI_API_KEY` | OpenAI API key (for LLM auth) |
| `FUGUE_HOOK_EXE` | Override hook command path |

---

## Further Reading

- **[CLI Reference](CLI.md)** — Complete command documentation
- **[Architecture](ARCHITECTURE.md)** — System design and internals
- **[Component Docs](components/)** — Deep dives into specific subsystems
