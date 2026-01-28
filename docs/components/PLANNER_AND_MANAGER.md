# Planner, Manager, and Director Agents

Murmur has three "non-coding" agent modes:
- planners (produce plan artifacts under `plans/`)
- a per-project manager agent (interactive coordinator)
- a global director agent (cross-project coordinator)

Code pointers:
- Planner RPC: `crates/murmur/src/daemon/rpc/plan.rs`
- Plan storage commands: `crates/murmur/src/main.rs` (`plan list/read/write`)
- Manager RPC: `crates/murmur/src/daemon/rpc/manager.rs`
- Director RPC: `crates/murmur/src/daemon/rpc/director.rs`

---

## Planner Agents

### What a planner is

A planner is an agent with role `planner`:
- visible via the CLI like any other agent
- chat messages are recorded and streamed like any other agent
- expected to write a plan markdown file via `mm plan write`

### Plan IDs and plan files

Planner agents are named `plan-<n>` and stored as:

`plans/plan-<n>.md`

The daemon creates an initial plan file containing prompt metadata.

### Running vs stored plans

- “Running planners” are controlled via IPC (exposed via `mm agent plan ...`).
- “Stored plans” are just markdown files under `plans/` and are managed by:
  - `mm plan list` (stored)
  - `mm plan read <id>`
  - `mm plan write` (stdin → file)

The `agent plan ...` command is a CLI alias that controls running planners.

### Project-less planners

Planners can be started without a project:
- workdir under `~/.murmur/planners/<id>/` (or `$MURMUR_DIR/planners/<id>/`)
- backend defaults to `claude`

This is useful for general planning not tied to a repo.

---

## Manager Agent

### Purpose

The manager agent is project-scoped and intended for:
- exploring and explaining the codebase
- creating issues/tickets
- monitoring orchestration and agents

It is *not* intended to implement code changes itself.

### Identity and worktree

Manager agent id is:

`manager-<project>`

It runs in a project worktree like other agents.

### Startup behavior

On start, the manager is initialized with a project-aware system prompt and then remains idle until you send it a message.

### Restrictions

Manager agents use a conservative tool allow-list:
- loaded from the global permissions file `[manager].allowed_patterns`
- default: `["mm:*"]` — allows the manager to run any `mm` CLI command

**Note:** The CLI binary is `mm`, not `murmur`. The default pattern `mm:*` allows commands like `mm issue create`, `mm project status`, etc.

To customize, add to your `~/.config/murmur/permissions.toml`:

```toml
[manager]
allowed_patterns = ["mm:*", "git :*"]
```

See `docs/components/PERMISSIONS_AND_QUESTIONS.md`.

---

## Director Agent

### Purpose

The director agent is a **global singleton** (not per-project) intended for:
- coordinating work across multiple projects
- monitoring all orchestrators and agents
- making high-level decisions about resource allocation
- providing CTO-level oversight of the system

Unlike managers which are project-scoped, there is only one director for the entire Murmur instance.

### Identity and Working Directory

Director agent id is always:

`director`

It runs in a dedicated working directory:

`~/.murmur/director/` (or `$MURMUR_DIR/director/`)

### Startup Behavior

On start, the director receives a system prompt with:
- list of all registered projects
- each project's status (running/stopped), backend, and max agents
- instructions for cross-project coordination

The director remains idle until you send it a message.

### Backend Support

The director supports both Claude and Codex backends:

```bash
mm director start                    # Uses Claude (default)
mm director start --backend codex    # Uses Codex
```

### Restrictions

Director agents use a conservative tool allow-list:
- loaded from `~/.murmur/config/director.toml` under `[director].allowed_patterns`
- default: empty (no bash commands allowed)

To enable bash commands, create `~/.murmur/config/director.toml`:

```toml
[director]
allowed_patterns = ["mm:*"]
```

Pattern format follows the same rules as manager patterns:
- `mm:*` — allows any `mm` command
- `git:*` — allows any `git` command
- `:*` — allows all commands (not recommended)

### CLI Commands

```bash
mm director start [--backend claude|codex]  # Start the director
mm director stop                             # Stop the director
mm director status                           # Check if director is running
mm director clear                            # Clear chat history
```

### Differences from Manager

| Aspect | Manager | Director |
|--------|---------|----------|
| Scope | Per-project | Global singleton |
| ID | `manager-<project>` | `director` |
| Working dir | Project worktree | `~/.murmur/director/` |
| System prompt | Project-aware | All-projects-aware |
| Config file | `permissions.toml` | `director.toml` |
