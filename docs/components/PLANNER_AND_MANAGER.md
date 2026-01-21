# Planner and Manager Agents

Murmur has two “non-coding” agent modes:
- planners (produce plan artifacts under `plans/`)
- a per-project manager agent (interactive coordinator)

Code pointers:
- Planner RPC: `crates/murmur/src/daemon/rpc/plan.rs`
- Plan storage commands: `crates/murmur/src/main.rs` (`plan list/read/write`)
- Manager RPC: `crates/murmur/src/daemon/rpc/manager.rs`

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
