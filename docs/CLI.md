# CLI Reference

Complete reference for the `mm` command-line interface.

## Quick Reference

### Essential Commands

| Command | Description |
|---------|-------------|
| `mm server start` | Start the daemon |
| `mm server stop` | Stop the daemon |
| `mm project add <url>` | Register a project |
| `mm project start <name>` | Start orchestration |
| `mm project stop <name>` | Stop orchestration |
| `mm agent list` | List all agents |
| `mm agent abort <id>` | Stop an agent |
| `mm issue list -p <proj>` | List issues |
| `mm issue create -p <proj> "title"` | Create an issue |
| `mm manager start <proj>` | Start project manager |
| `mm director start` | Start global director |
| `mm tui` | Open terminal UI |
| `mm attach` | Stream daemon events |
| `mm host list` | List agent hosts |
| `mm host discover` | Reconnect to orphaned hosts |

### Command Structure

```
mm [OPTIONS] <COMMAND> [ARGS]

Global Options:
  --murmur-dir <DIR>     Override base directory (~/.murmur)
  --socket-path <PATH>   Override daemon socket path
  --log-level <LEVEL>    Set log level [error, warn, info, debug, trace]
  -h, --help             Show help
  -V, --version          Show version
```

### Command Aliases

Many commands have short aliases for convenience:

| Command | Alias |
|---------|-------|
| `mm project list` | `mm project ls` |
| `mm project add` | `mm project new` |
| `mm project remove` | `mm project rm` |
| `mm agent list` | `mm agent ls` |
| `mm agent abort` | `mm agent kill` |
| `mm issue list` | `mm issue ls` |
| `mm issue create` | `mm issue new` |
| `mm plan list` | `mm plan ls` |
| `mm plan read` | `mm plan show` |
| `mm commit list` | `mm commit ls` |

---

## Server Commands

### `mm server start`

Start the Murmur daemon.

```bash
mm server start [OPTIONS]

Options:
  -f, --foreground    Run in foreground (don't daemonize)
```

**Examples:**
```bash
mm server start       # Start in background
mm server start -f    # Start in foreground (for development)
```

### `mm server stop`

Stop the running daemon gracefully.

```bash
mm server stop
```

Alias: `mm server shutdown`

### `mm server status`

Check if the daemon is running.

```bash
mm server status
```

### `mm server restart`

Restart the daemon.

```bash
mm server restart [OPTIONS]

Options:
  -f, --foreground    Run in foreground after restart
```

---

## Project Commands

### `mm project add`

Register a new project.

```bash
mm project add <PATH_OR_URL> [OPTIONS]

Arguments:
  <PATH_OR_URL>    Git URL or local path to clone

Options:
  -n, --name <NAME>         Project name (inferred from URL if omitted)
  -m, --max-agents <N>      Max concurrent agents (default: 3)
  -b, --backend <BACKEND>   AI backend [claude, codex] (default: claude)
      --autostart           Start orchestration when daemon starts
      --remote-url <URL>    Override remote URL
```

Alias: `mm project new`

**Examples:**
```bash
mm project add https://github.com/org/repo.git
mm project add git@github.com:org/repo.git -n myproj
mm project add /path/to/local/repo -m 5
mm project add <url> --autostart -b codex
```

### `mm project list`

List all registered projects.

```bash
mm project list
```

Alias: `mm project ls`

Output: `NAME<tab>REMOTE`

When empty, shows hint: "Add a project with: mm project add <url>"

### `mm project remove`

Remove a project from the registry.

```bash
mm project remove <NAME> [OPTIONS]

Options:
      --delete-worktrees    Also delete agent worktrees
  -y, --yes                 Skip confirmation prompt
```

Alias: `mm project rm`

**Note:** Requires confirmation unless `--yes` is specified.

### `mm project status`

Show project health and orchestration status.

```bash
mm project status <NAME>
```

### `mm project start`

Start orchestration for a project.

```bash
mm project start <NAME>
mm project start -a, --all
```

**Examples:**
```bash
mm project start myproject
mm project start -a    # Start all projects
```

### `mm project stop`

Stop orchestration for a project.

```bash
mm project stop <NAME> [OPTIONS]
mm project stop -a, --all [OPTIONS]

Options:
      --abort-agents    Also abort active coding agents
```

**Examples:**
```bash
mm project stop myproject
mm project stop -a --abort-agents
```

### `mm project config`

View and modify project configuration.

```bash
# Show all config
mm project config show <NAME>

# Get a single value
mm project config get <NAME> <KEY>

# Set a value
mm project config set <NAME> <KEY> <VALUE>
```

**Configuration Keys:**

| Key | Values | Description |
|-----|--------|-------------|
| `max-agents` | 1-10 | Max concurrent coding agents |
| `issue-backend` | `tk`, `github`, `linear` | Issue source |
| `agent-backend` | `claude`, `codex` | AI backend |
| `coding-backend` | `claude`, `codex` | Override for coding agents |
| `planner-backend` | `claude`, `codex` | Override for planners |
| `permissions-checker` | `manual`, `llm` | Permission handling mode |
| `merge-strategy` | `direct`, `pull-request` | Merge mode |
| `autostart` | `true`, `false` | Auto-start on daemon start |
| `allowed-authors` | JSON array | Filter issues by author (GitHub) |
| `linear-team` | UUID | Linear team ID |
| `linear-project` | UUID | Linear project ID |

**Examples:**
```bash
mm project config show myproj
mm project config get myproj max-agents
mm project config set myproj issue-backend github
mm project config set myproj max-agents 5
```

---

## Issue Commands

All issue commands support `-p, --project <NAME>`. If omitted, the project is inferred from the current working directory.

### `mm issue list`

List issues from the configured backend.

```bash
mm issue list [OPTIONS]

Options:
  -p, --project <NAME>     Project name
  -s, --status <STATUS>    Filter by status [open, closed, blocked]
```

Alias: `mm issue ls`

Output: `ID<tab>STATUS<tab>TITLE`

When empty, shows hint: "Create an issue with: mm issue create"

### `mm issue show`

Show details of a specific issue.

```bash
mm issue show <ISSUE_ID> -p <NAME>
```

### `mm issue ready`

List issues that are ready to be worked on (open, no open dependencies).

```bash
mm issue ready -p <NAME>
```

Output: `ID<tab>TITLE`

### `mm issue create`

Create a new issue.

```bash
mm issue create <TITLE> [OPTIONS]

Options:
  -p, --project <NAME>       Project name
  -d, --description <TEXT>   Issue description
      --type <TYPE>          Issue type [task, bug, feature, chore] (default: task)
      --priority <N>         Priority [0=low, 1=medium, 2=high] (default: 1)
      --depends-on <IDS>     Dependencies (comma-separated issue IDs)
      --parent <ID>          Parent issue ID (creates a sub-issue)
      --commit               Immediately commit (tk only)
```

Alias: `mm issue new`

**Examples:**
```bash
mm issue create "Add user authentication" -p myproj
mm issue create "Fix login bug" -p myproj --type bug --priority 2
mm issue create "Refactor API" -p myproj -d "Improve error handling"
```

### `mm issue update`

Update an existing issue.

```bash
mm issue update <ISSUE_ID> [OPTIONS]

Options:
  -p, --project <NAME>    Project name
  -t, --title <TEXT>      New title
  -s, --status <STATUS>   New status [open, closed, blocked]
      --priority <N>      New priority [0, 1, 2]
```

**Examples:**
```bash
mm issue update 42 -s closed
mm issue update 42 -t "New title" --priority 2
```

### `mm issue close`

Close an issue.

```bash
mm issue close <ISSUE_ID> -p <NAME>
```

### `mm issue comment`

Add a comment to an issue.

```bash
mm issue comment <ISSUE_ID> [OPTIONS]

Options:
  -p, --project <NAME>    Project name
  -b, --body <TEXT>       Comment body
```

### `mm issue plan`

Upsert a `## Plan` section in the issue body.

```bash
mm issue plan <ISSUE_ID> [OPTIONS]

Options:
  -p, --project <NAME>    Project name
  -b, --body <TEXT>       Plan content
  -f, --file <PATH>       Read plan from file
```

### `mm issue commit`

Commit and push ticket changes (tk backend only).

```bash
mm issue commit -p <NAME>
```

---

## Agent Commands

### `mm agent list`

List all running agents.

```bash
mm agent list [OPTIONS]

Options:
  -p, --project <NAME>    Filter by project
```

Alias: `mm agent ls`

Output: `ID<tab>PROJECT<tab>ROLE<tab>STATE<tab>ISSUE`

When empty, shows hint: "Start orchestration to spawn agents"

### `mm agent abort`

Stop an agent.

```bash
mm agent abort <AGENT_ID> [OPTIONS]

Options:
  -f, --force    Force kill immediately (SIGKILL)
  -y, --yes      Skip confirmation prompt
```

Alias: `mm agent kill`

**Note:** Requires confirmation unless `--yes` is specified.

### `mm agent sync-comments`

Manually fetch and inject new comments for an agent.

```bash
mm agent sync-comments <AGENT_ID>
```

This fetches any new comments on the agent's claimed issue and delivers them to the agent.

### Agent-Callable Commands

These commands are used by agent processes (require `MURMUR_AGENT_ID` environment variable):

```bash
mm agent claim <ISSUE_ID>      # Claim an issue
mm agent describe <TEXT>       # Set agent description
mm agent done [--task ID] [--error TEXT]   # Signal completion
```

---

## Planner Commands

### `mm agent plan`

Start a planner agent.

```bash
mm agent plan [OPTIONS] <PROMPT>

Options:
  -p, --project <NAME>    Project (optional, uses ~/.murmur/planners/ if omitted)
```

**Examples:**
```bash
mm agent plan "Add user authentication"
mm agent plan -p myproject "Refactor the API layer"
```

### `mm agent plan list`

List running planners.

```bash
mm agent plan list [-p <NAME>]
```

Alias: `mm agent plan ls`

### `mm agent plan stop`

Stop a running planner.

```bash
mm agent plan stop <PLAN_ID>
```

### `mm plan list`

List stored plan files.

```bash
mm plan list
```

Alias: `mm plan ls`

When empty, shows hint: "Start a planner with: mm agent plan"

### `mm plan read`

Show contents of a stored plan.

```bash
mm plan read <PLAN_ID>
```

Alias: `mm plan show`

---

## Manager Commands

### `mm manager start`

Start a manager agent for a project.

```bash
mm manager start <PROJECT>
```

### `mm manager stop`

Stop the manager agent.

```bash
mm manager stop <PROJECT> [OPTIONS]

Options:
  -y, --yes    Skip confirmation prompt
```

### `mm manager status`

Check manager status.

```bash
mm manager status <PROJECT>
```

### `mm manager clear`

Clear manager chat history.

```bash
mm manager clear <PROJECT>
```

---

## Director Commands

The director is a global agent (not project-scoped) for cross-project coordination.

### `mm director start`

Start the director agent.

```bash
mm director start [OPTIONS]

Options:
  -b, --backend <BACKEND>    AI backend [claude, codex] (default: claude)
```

### `mm director stop`

Stop the director agent.

```bash
mm director stop [OPTIONS]

Options:
  -y, --yes    Skip confirmation prompt
```

### `mm director status`

Check director status.

```bash
mm director status
```

### `mm director clear`

Clear director chat history.

```bash
mm director clear
```

---

## Monitoring Commands

### `mm status`

Show daemon and project status.

```bash
mm status [OPTIONS]

Options:
  -a, --agents    Also display running agents
```

### `mm tui`

Launch the terminal UI.

```bash
mm tui
```

See [TUI.md](TUI.md) for keybindings and features.

### `mm attach`

Stream daemon events to stdout.

```bash
mm attach [PROJECTS...]
```

**Examples:**
```bash
mm attach           # All projects
mm attach myproj    # Single project
mm attach proj1 proj2
```

Press Ctrl-C to detach.

### `mm claims`

Show active issue claims.

```bash
mm claims [OPTIONS]

Options:
  -p, --project <NAME>    Filter by project
```

Output: `ISSUE<tab>AGENT<tab>PROJECT`

---

## Host Commands

Host commands are used to manage and inspect agent host processes. Agent hosts are independent processes that wrap agent subprocesses, allowing agents to survive daemon restarts.

### `mm host list`

List all connected agent hosts.

```bash
mm host list
```

Alias: `mm host ls`

Output: `AGENT_ID<tab>PROJECT<tab>ROLE<tab>STATE`

### `mm host status`

Get detailed status for a specific agent host.

```bash
mm host status <AGENT_ID>
```

Returns information about the agent running in the host process.

### `mm host discover`

Discover and reconnect to running host processes.

```bash
mm host discover
```

Scans the hosts directory for active socket files and reconnects the daemon to any running hosts that were orphaned (e.g., after a daemon restart).

---

## Utility Commands

### `mm stats`

Show usage statistics.

```bash
mm stats [OPTIONS]

Options:
  -p, --project <NAME>    Filter by project
```

### `mm commit list`

View merge commit history.

```bash
mm commit list [OPTIONS]

Options:
  -p, --project <NAME>    Filter by project
  -n, --limit <N>         Maximum number of commits
```

Alias: `mm commit ls`

### `mm branch cleanup`

Delete merged `murmur/*` branches.

```bash
mm branch cleanup [OPTIONS]

Options:
  --dry-run    Show what would be deleted
  --local      Also delete local branches
```

**Examples:**
```bash
mm branch cleanup --dry-run   # Preview
mm branch cleanup             # Delete remote branches
mm branch cleanup --local     # Delete local and remote
```

### `mm version`

Print version information.

```bash
mm version
```

### `mm completion`

Generate shell completions.

```bash
mm completion bash
mm completion zsh
mm completion fish
mm completion powershell
```

---

## Hidden Commands

These commands exist but are hidden from `--help`. They're primarily for internal use or debugging.

### Permission Commands

```bash
mm permission list
mm permission respond <REQUEST_ID> allow|deny
```

### Question Commands

```bash
mm question list
mm question respond <REQUEST_ID> '{"key": "answer"}'
```

### Hook Commands

Used by Claude Code hooks (not for manual invocation):

```bash
mm hook PreToolUse
mm hook PermissionRequest
mm hook Stop
```

### Debug Commands

```bash
mm ping    # Check daemon connectivity
```

---

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `MURMUR_DIR` | Base directory for all state | `~/.murmur` |
| `MURMUR_SOCKET_PATH` | Override daemon socket path | — |
| `MURMUR_LOG` | Log level filter | `info` |
| `MURMUR_AGENT_ID` | Agent ID (for agent commands) | — |
| `GITHUB_TOKEN` | GitHub API token | — |
| `GH_TOKEN` | GitHub API token (alternative) | — |
| `LINEAR_API_KEY` | Linear API key | — |
| `ANTHROPIC_API_KEY` | Anthropic API key | — |
| `OPENAI_API_KEY` | OpenAI API key | — |
| `FUGUE_HOOK_EXE` | Hook command executable path | — |

---

## Output Formats

Murmur uses simple, script-friendly output:

- **Lists**: Tab-separated rows with headers
- **Actions**: Descriptive messages (e.g., "Removed project.")
- **IDs**: Single line (e.g., `issue create` prints the new issue ID)
- **Empty states**: Helpful hints suggesting next actions
- **Errors**: Actionable error messages to stderr

---

## Examples

### Complete Workflow

```bash
# Start daemon
mm server start

# Add a project
mm project add https://github.com/myorg/myapp.git -n myapp

# Configure GitHub backend
export GITHUB_TOKEN=ghp_...
mm project config set myapp issue-backend github

# Start orchestration
mm project start myapp

# Monitor with TUI
mm tui
```

### Local Development

```bash
# Use isolated directory
export MURMUR_DIR=/tmp/murmur-dev

# Start daemon in foreground
mm server start -f

# In another terminal
mm project add /path/to/local/repo -n test
mm issue create "Test issue" -p test
mm project start test
mm attach test
```

### Scripting

```bash
#!/bin/bash
# Wait for ready issues and print count

project="myapp"
count=$(mm issue ready -p "$project" | tail -n +2 | wc -l)
echo "Ready issues: $count"

# List agent IDs (skip header)
mm agent list -p "$project" | tail -n +2 | cut -f1
```
