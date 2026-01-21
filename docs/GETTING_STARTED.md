# Getting Started

Get Murmur running and your first agent working in under 5 minutes.

## Prerequisites

Before you begin, ensure you have:

- **Rust toolchain** — Install via [rustup](https://rustup.rs/)
- **Git** — Version 2.20+ (for worktree support)
- **Agent CLI** — At least one of:
  - `claude` ([Claude Code](https://claude.ai/code))
  - `codex` (Codex CLI)

Verify your setup:

```bash
cargo --version    # Should show 1.75+
git --version      # Should show 2.20+
claude --version   # Or: codex --version
```

## Step 1: Install Murmur

From the repository root:

```bash
cargo install --locked --path crates/murmur
```

Verify the installation:

```bash
mm --version
```

The binary is named `mm` and installs to `~/.cargo/bin/`.

## Step 2: Start the Daemon

Open a terminal and start the daemon in the foreground:

```bash
mm server start --foreground
```

You should see:

```
Murmur daemon starting...
Listening on /run/user/1000/murmur.sock
```

Keep this terminal open. The daemon is the control plane for all Murmur operations.

## Step 3: Add a Project

In a new terminal, add a project:

```bash
mm project add https://github.com/yourorg/yourrepo.git --name myproj
```

This clones the repository into `~/.murmur/projects/myproj/repo/`.

Verify:

```bash
mm project list
```

## Step 4: Create an Issue

Murmur uses the `tk` (local tickets) backend by default. Create an issue:

```bash
mm issue create --project myproj "Add a hello world endpoint"
```

View your issues:

```bash
mm issue list --project myproj
mm issue ready --project myproj   # Shows issues ready for agents
```

## Step 5: Start Orchestration

Start the orchestrator to automatically spawn agents:

```bash
mm project start myproj
```

The orchestrator will:
1. Poll for ready issues every ~10 seconds
2. Spawn agents up to the `max-agents` limit (default: 3)
3. Assign each agent to an issue in an isolated git worktree

## Step 6: Monitor with the TUI

Launch the terminal UI to watch agents work:

```bash
mm tui
```

**TUI Keybindings:**
- `Tab` — Switch between agent list and chat pane
- `j/k` — Navigate / scroll
- `y/n` — Approve / deny permission requests
- `t` — Toggle tool call visibility
- `x` — Abort selected agent
- `q` — Quit

When an agent requests permission to run a tool (like `Bash` or `Write`), you'll see a permission prompt. Press `y` to allow or `n` to deny.

## What Happens Next

1. **Agent works** — The agent reads the issue, explores the codebase, and implements the solution
2. **Agent commits** — When done, the agent commits its changes and calls `mm agent done`
3. **Murmur merges** — The daemon rebases the agent's branch onto `origin/main` and fast-forward merges
4. **Issue closes** — The issue is marked as closed
5. **Claim released** — The orchestrator can now assign the next ready issue

## Next Steps

### Use GitHub Issues Instead of Local Tickets

```bash
# Set your GitHub token
export GITHUB_TOKEN=ghp_...

# Switch the issue backend
mm project config set myproj issue-backend github

# Restart orchestration
mm project stop myproj
mm project start myproj
```

### Use Linear Issues

```bash
# Set your Linear API key
export LINEAR_API_KEY=lin_api_...

# Configure the project
mm project config set myproj issue-backend linear
mm project config set myproj linear-team YOUR_TEAM_UUID

# Restart orchestration
mm project stop myproj
mm project start myproj
```

### Configure Permission Rules

Create `~/.config/murmur/permissions.toml`:

```toml
# Allow all read operations
[[rules]]
tool = "Read"
action = "allow"

# Allow cargo commands
[[rules]]
tool = "Bash"
action = "allow"
pattern = "cargo *"

# Deny destructive commands
[[rules]]
tool = "Bash"
action = "deny"
pattern = "rm -rf *"
```

### Run the Daemon in Background

```bash
mm server start          # Starts in background
mm server status         # Check if running
mm server stop           # Stop the daemon
```

### Isolated Testing Environment

Use `MURMUR_DIR` for a sandboxed environment:

```bash
export MURMUR_DIR=/tmp/murmur-test
mm server start --foreground
```

All state (config, projects, logs) will be under `/tmp/murmur-test/`.

## Troubleshooting

### "Connection refused" or "No such file"

The daemon isn't running. Start it with `mm server start`.

### "Project not found"

Run `mm project list` to see registered projects. Add projects with `mm project add`.

### Agents not spawning

1. Check orchestration is running: `mm project status myproj`
2. Check for ready issues: `mm issue ready --project myproj`
3. Check claims: `mm claims --project myproj` (an issue may already be claimed)
4. Check the daemon logs: `tail -f ~/.murmur/murmur.log`

### Permission prompts not appearing

Make sure you're using the Claude backend (`agent-backend = "claude"`). The Codex backend handles permissions internally.

## Further Reading

- **[Usage Guide](USAGE.md)** — Complete feature documentation
- **[CLI Reference](CLI.md)** — All commands and options
- **[TUI Guide](TUI.md)** — Terminal UI features
- **[Architecture](ARCHITECTURE.md)** — How Murmur works internally
