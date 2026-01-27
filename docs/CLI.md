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
| `mm tui` | Open terminal UI |
| `mm attach` | Stream daemon events |

### Command Structure

```
mm [OPTIONS] <COMMAND> [ARGS]

Global Options:
  --murmur-dir <PATH>    Override base directory (~/.murmur)
  --socket-path <PATH>   Override daemon socket path
  --log-level <LEVEL>    Set log level (error, warn, info, debug, trace)
  -h, --help             Show help
  -V, --version          Show version
```

---

## Server Commands

### `mm server start`

Start the Murmur daemon.

```bash
mm server start [OPTIONS]

Options:
  --foreground    Run in foreground (don't daemonize)
```

**Examples:**
```bash
mm server start              # Start in background
mm server start --foreground # Start in foreground (for development)
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
  --foreground    Run in foreground after restart
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
  --name <NAME>         Project name (inferred from URL if omitted)
  --max-agents <N>      Max concurrent agents (default: 3)
  --autostart           Start orchestration when daemon starts
  --backend <BACKEND>   Agent backend: claude, codex (default: claude)
```

**Examples:**
```bash
mm project add https://github.com/org/repo.git
mm project add https://github.com/org/repo.git --name myproj
mm project add git@github.com:org/repo.git --max-agents 5 --autostart
```

### `mm project list`

List all registered projects.

```bash
mm project list
```

Output format: tab-separated (script-friendly).

### `mm project remove`

Remove a project from the registry.

```bash
mm project remove <NAME> [OPTIONS]

Options:
  --delete-worktrees    Also delete agent worktrees
```

### `mm project status`

Show project health and orchestration status.

```bash
mm project status <NAME>
```

### `mm project start`

Start orchestration for a project.

```bash
mm project start <NAME>
mm project start --all
```

### `mm project stop`

Stop orchestration for a project.

```bash
mm project stop <NAME>
mm project stop --all
```

Options:
  --abort-agents    Also abort active coding agents

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
mm project config set myproj allowed-authors '["user1", "user2"]'
```

---

## Issue Commands

All issue commands support `--project <NAME>` (or `-p <NAME>`). If omitted, the project is inferred from the current working directory.

### `mm issue list`

List issues from the configured backend.

```bash
mm issue list --project <NAME>
```

### `mm issue show`

Show details of a specific issue.

```bash
mm issue show <ISSUE_ID> --project <NAME>
```

### `mm issue ready`

List issues that are ready to be worked on (open, no open dependencies).

```bash
mm issue ready --project <NAME>
```

### `mm issue create`

Create a new issue (tk backend).

```bash
mm issue create <TITLE> --project <NAME> [OPTIONS]

Options:
  --description <TEXT>    Issue description
  --type <TYPE>           Issue type (e.g., task, bug, feature)
  --priority <N>          Priority (default: 0)
  --depends-on <ID>       Add dependency on another issue
  --parent <ID>           Set parent issue
  --commit                Immediately commit the new ticket
```

**Examples:**
```bash
mm issue create "Add user authentication" -p myproj
mm issue create "Fix login bug" -p myproj --type bug --priority 1
mm issue create "Refactor API" -p myproj --description "Improve error handling"
```

### `mm issue update`

Update an existing issue.

```bash
mm issue update <ISSUE_ID> --project <NAME> [OPTIONS]

Options:
  --title <TEXT>          New title
  --status <STATUS>       open, blocked, closed
  --priority <N>          New priority
```

### `mm issue close`

Close an issue.

```bash
mm issue close <ISSUE_ID> --project <NAME>
```

### `mm issue comment`

Add a comment to an issue.

```bash
mm issue comment <ISSUE_ID> --project <NAME> --body <TEXT>
```

### `mm issue plan`

Upsert a `## Plan` section in the issue body.

```bash
mm issue plan <ISSUE_ID> --project <NAME> --file <PATH>
mm issue plan <ISSUE_ID> --project <NAME> --body <TEXT>
```

### `mm issue commit`

Commit and push ticket changes (tk backend only).

```bash
mm issue commit --project <NAME>
```

---

## Agent Commands

### `mm agent list`

List all running agents.

```bash
mm agent list [OPTIONS]

Options:
  --project <NAME>    Filter by project
```

Output columns: ID, Project, Issue, State, Backend.

### `mm agent create`

Manually create a coding agent.

```bash
mm agent create <PROJECT> <ISSUE_ID> [OPTIONS]

Options:
  --backend <BACKEND>    claude, codex
```

### `mm agent abort`

Stop an agent.

```bash
mm agent abort <AGENT_ID> [OPTIONS]

Options:
  --force    Force kill if graceful abort fails
  --yes      Skip confirmation prompt
```

### `mm agent done`

Signal agent completion (used by agent processes).

```bash
mm agent done [OPTIONS]

Options:
  --task <ID>      Task/issue ID (optional)
  --error <TEXT>   Error message if failed

Requires: MURMUR_AGENT_ID environment variable
```

### `mm agent claim`

Claim an issue for the current agent (used by agent processes).

```bash
mm agent claim <ISSUE_ID>

Requires: MURMUR_AGENT_ID environment variable
```

### `mm agent describe`

Set a description for the current agent (used by agent processes).

```bash
mm agent describe <TEXT>

Requires: MURMUR_AGENT_ID environment variable
```

### `mm agent sync-comments`

Manually fetch and inject new comments for an agent.

```bash
mm agent sync-comments <AGENT_ID>
```

This fetches any new comments on the agent's claimed issue and delivers them to the agent. Useful for testing or when automatic comment polling is disabled.

**Example:**
```bash
mm agent sync-comments a-1
# Output: Injected 2 comment(s)
```

---

## Planner Commands

### `mm agent plan`

Start a planner agent.

```bash
mm agent plan --project <NAME> <PROMPT>
mm agent plan <PROMPT>    # Project-less planner
```

### `mm agent plan list`

List running planners.

```bash
mm agent plan list --project <NAME>
```

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

### `mm plan read`

Show contents of a stored plan.

```bash
mm plan read <PLAN_ID>
```

### `mm plan write`

Write plan content from stdin (used by planner agents).

```bash
cat plan.md | mm plan write

Requires: MURMUR_AGENT_ID environment variable
```

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
mm manager stop <PROJECT>
```

### `mm manager status`

Check manager status.

```bash
mm manager status <PROJECT>
```

### `mm manager clear`

Clear manager history.

```bash
mm manager clear <PROJECT>
```

---

## Monitoring Commands

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
  --project <NAME>    Filter by project
```

---

## Maintenance Commands

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
mm ping                    # Check daemon connectivity
mm stats                   # Usage statistics
mm commit list             # List recent commits
mm claim list              # Same as `mm claims`
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

- **Lists**: Tab-separated rows
- **Actions**: `ok` on success
- **IDs**: Single line (e.g., `issue create` prints the new issue ID)
- **Errors**: Actionable error messages to stderr

---

## Examples

### Complete Workflow

```bash
# Start daemon
mm server start

# Add a project
mm project add https://github.com/myorg/myapp.git --name myapp

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
mm server start --foreground

# In another terminal
mm project add /path/to/local/repo --name test
mm issue create "Test issue" -p test
mm project start test
mm attach test
```

### Scripting

```bash
#!/bin/bash
# Wait for ready issues and print count

project="myapp"
count=$(mm issue ready -p "$project" | wc -l)
echo "Ready issues: $count"

# List agent IDs
mm agent list --project "$project" | cut -f1
```
