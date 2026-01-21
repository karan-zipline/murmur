# Architecture

Murmur is a local-only coding-agent supervisor. It manages multiple Claude Code or Codex CLI instances across multiple projects, isolates each agent in its own git worktree, assigns work from pluggable issue backends, and provides a CLI for monitoring and approvals.

This document describes the system design, component model, and key flows. For user-facing documentation, see [Usage Guide](USAGE.md) and [CLI Reference](CLI.md).

## Table of Contents

- [Overview](#overview)
- [Design Principles](#design-principles)
- [System Architecture](#system-architecture)
- [Component Model](#component-model)
- [Crate Structure](#crate-structure)
- [Data Flow](#data-flow)
- [On-Disk Layout](#on-disk-layout)
- [Key Runtime Flows](#key-runtime-flows)

---

## Overview

### Goals

| Goal | Description |
|------|-------------|
| **Local-only control plane** | Daemon on local machine, CLI via Unix socket, no remote API |
| **Multi-project supervision** | Register multiple projects, each with its own config |
| **Multi-agent orchestration** | Per-project orchestrator spawns agents for ready issues |
| **Pluggable issue backends** | `tk` (local files), GitHub Issues, Linear Issues |
| **Git worktree isolation** | Each agent in its own worktree for safe concurrent work |
| **Interactive supervision** | CLI/TUI for approvals, questions, and monitoring |

### Non-Goals

These are explicitly out of scope for the current implementation:

- Remote/HTTP API for client control
- Distributed scheduling or cluster coordination
- Guaranteed agent preservation across daemon restarts
- Additional issue trackers beyond tk, GitHub, Linear

---

## Design Principles

Murmur follows **Functional Core, Imperative Shell** architecture.

### Functional Core (`murmur-core`)

- Pure functions over immutable values
- **No I/O**: No filesystem, network, subprocess, sockets, time, randomness, or logging
- Returns **decisions and actions** as data, not effects
- Deterministic and easy to test

### Imperative Shell (`murmur`)

- Executes all I/O (git, processes, sockets, HTTP, files)
- Translates between external protocols and core values
- Emits events and persists runtime state
- Schedules and executes the core's decisions

### Additional Principles

| Principle | Description |
|-----------|-------------|
| **High cohesion, low coupling** | Modules have one job and narrow surface area |
| **Dependency inversion** | I/O code depends on core, never the reverse |
| **Values over state** | Domain state as values, evolved via explicit events |
| **DTOs at boundaries** | Small data transfer objects, not giant structs |
| **Traits at boundaries** | Minimal "port" interfaces for I/O adapters |

---

## System Architecture

### Process Topology

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              LOCAL MACHINE                               │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│    ┌──────────────────────────────────────────────────────────────┐     │
│    │                      MURMUR DAEMON                            │     │
│    │                     (mm server start)                         │     │
│    │  ┌────────────────────────────────────────────────────────┐  │     │
│    │  │                    Shared State                         │  │     │
│    │  │  • Project registry    • Claim registry                │  │     │
│    │  │  • Agent runtimes      • Pending permissions           │  │     │
│    │  │  • Orchestrators       • Commit logs                   │  │     │
│    │  └────────────────────────────────────────────────────────┘  │     │
│    │                              │                                │     │
│    │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │     │
│    │  │ Orchestrator│  │ Orchestrator│  │   Webhook Server    │  │     │
│    │  │  (proj-a)   │  │  (proj-b)   │  │     (optional)      │  │     │
│    │  └──────┬──────┘  └──────┬──────┘  └─────────────────────┘  │     │
│    │         │                │                                    │     │
│    │         ▼                ▼                                    │     │
│    │  ┌─────────────────────────────────────────────────────────┐ │     │
│    │  │                  AGENT PROCESSES                         │ │     │
│    │  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐    │ │     │
│    │  │  │ Agent   │  │ Agent   │  │ Planner │  │ Manager │    │ │     │
│    │  │  │ (a-1)   │  │ (a-2)   │  │(plan-1) │  │(manager)│    │ │     │
│    │  │  │ wt-a-1/ │  │ wt-a-2/ │  │wt-plan-1│  │wt-manager│    │ │     │
│    │  │  └─────────┘  └─────────┘  └─────────┘  └─────────┘    │ │     │
│    │  └─────────────────────────────────────────────────────────┘ │     │
│    └──────────────────────────────────────────────────────────────┘     │
│                                    │                                     │
│                         Unix Socket (IPC)                                │
│                                    │                                     │
│    ┌──────────────────────────────────────────────────────────────┐     │
│    │              CLI (mm ...)  /  TUI (mm tui)                    │     │
│    └──────────────────────────────────────────────────────────────┘     │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### Component Roles

| Component | Responsibility |
|-----------|----------------|
| **Daemon** | Control plane: state, orchestration, event broadcast, IPC |
| **CLI** | User commands via IPC; also invoked by agent hooks |
| **TUI** | Real-time monitoring UI via attach event stream |
| **Orchestrator** | Per-project loop: poll issues, spawn agents, track claims |
| **Agent** | Claude Code or Codex subprocess in isolated worktree |
| **Webhook Server** | Optional HTTP server for GitHub/Linear webhook triggers |

---

## Component Model

### Daemon (Supervisor)

The daemon owns:

- **Project registry**: Load/save config, manage registered projects
- **Orchestrator lifecycle**: Start/stop per-project spawn loops
- **Agent lifecycle**: Create, delete, abort, send messages
- **Claim registry**: Prevent duplicate work assignments
- **Permission coordination**: Pending requests, response routing
- **Event broadcast**: Stream events to attached clients

The daemon delegates to adapters for:
- Git operations (clone, worktree, merge)
- Issue backend calls (tk, GitHub, Linear)
- LLM parsing and protocol translation

### Orchestrator

Each project has an orchestrator that:

1. Polls for ready issues (~10 second interval)
2. Filters by backend rules (status, authors)
3. Skips already-claimed issues
4. Spawns agents up to `max-agents`
5. Claims issues on spawn
6. Triggers merge on agent completion

### Agents

Three agent types:

| Type | Purpose | Worktree |
|------|---------|----------|
| **Coding** | Implement issues, commit, close | `wt-<agent-id>/` |
| **Planner** | Explore, design, write plans | `wt-plan-<id>/` |
| **Manager** | Interactive coordinator | `wt-manager/` |

Agent state machine:

```
Starting → Running → Exited
              │
              ├─→ NeedsResolution (merge conflict)
              │
              └─→ Aborted (manual stop)
```

### Issue Backends

Common interface:
- `get(id)`, `list(filter)`, `ready()`
- `create`, `update`, `close`
- `comment`, `upsert_plan_section`

Implementations:

| Backend | Storage | Notes |
|---------|---------|-------|
| `tk` | `.murmur/tickets/*.md` | In-repo, commit via `issue commit` |
| `github` | GitHub Issues API | Requires token, detects owner/repo from remote |
| `linear` | Linear API | Requires API key and team UUID |

### Permissions

Claude Code integration via hooks:

```
Agent tool call → PreToolUse hook → mm hook PreToolUse
                                           │
                    ┌──────────────────────┴──────────────────────┐
                    │                                             │
              Rule match?                                   No match
                    │                                             │
           ┌───────┴───────┐                                     │
           ↓               ↓                                     ↓
         Allow           Deny                              Ask daemon
           │               │                                     │
           └───────────────┴─────────────────────────────────────┘
                                           │
                                           ↓
                                    Return to Claude
```

---

## Crate Structure

```
crates/
├── murmur-core/       # Functional core (pure logic)
│   └── src/
│       ├── agent.rs         # Agent state machine, chat buffer
│       ├── claims.rs        # ClaimRegistry
│       ├── config.rs        # Configuration structures
│       ├── issue.rs         # Issue model, parsing, plan upsert
│       ├── orchestration.rs # Pure spawn policy
│       ├── permissions.rs   # Rule evaluation
│       ├── paths.rs         # Path resolution
│       └── stream/          # Agent output parsing
│
├── murmur-protocol/   # IPC message types
│   └── src/lib.rs          # Request/Response/Event DTOs
│
└── murmur/            # Imperative shell (daemon + CLI)
    └── src/
        ├── main.rs          # CLI entrypoint
        ├── client.rs        # IPC client
        ├── daemon/
        │   ├── mod.rs       # Daemon init
        │   ├── server.rs    # Socket server
        │   ├── state.rs     # SharedState
        │   ├── orchestration.rs
        │   ├── merge.rs
        │   ├── claude.rs    # Claude subprocess
        │   ├── webhook.rs
        │   └── rpc/         # Message handlers
        ├── git.rs           # Git operations
        ├── worktrees.rs     # Worktree management
        ├── github.rs        # GitHub API
        ├── linear.rs        # Linear API
        ├── issues.rs        # tk backend
        ├── permissions.rs   # Rule loading
        └── hooks.rs         # Claude hook handlers
```

### Dependency Direction

```
murmur (shell)
    │
    ├──→ murmur-core (pure domain logic)
    │
    └──→ murmur-protocol (wire types)

murmur-core ←─┐
              │ No dependencies on shell or I/O
murmur-protocol ←┘
```

---

## Data Flow

### IPC Protocol

**Transport**: Unix domain socket (`murmur.sock`)

**Message format**: JSONL (one JSON object per line)

**Request envelope**:
```json
{ "type": "project.list", "id": "req-123", "payload": {} }
```

**Response envelope**:
```json
{ "type": "project.list", "id": "req-123", "success": true, "payload": {} }
```

**Event envelope**:
```json
{ "type": "agent.chat", "id": "evt-42", "payload": { ... } }
```

### Message Categories

| Category | Examples |
|----------|----------|
| Server | `ping`, `shutdown` |
| Projects | `project.add`, `project.list`, `project.config.*` |
| Orchestration | `orchestration.start`, `orchestration.stop` |
| Agents | `agent.create`, `agent.list`, `agent.done` |
| Issues | `issue.list`, `issue.ready`, `issue.create` |
| Permissions | `permission.request`, `permission.respond` |
| Plans | `plan.start`, `plan.stop`, `plan.list` |
| Manager | `manager.start`, `manager.stop` |

### Event Streaming

After `attach`, the daemon pushes events:

- `heartbeat` — Periodic health signal
- `agent.chat` — Chat messages from agents
- `permission.requested` — Tool approval needed
- `question.requested` — User question pending
- `agent.idle` — Agent waiting for input

---

## On-Disk Layout

### Default Paths

**Base directory**: `~/.murmur` (or `$MURMUR_DIR`)

**Config directory**: `~/.config/murmur` (or `$MURMUR_DIR/config`)

### Directory Structure

```
~/.murmur/
├── murmur.sock              # Unix domain socket (may be in XDG_RUNTIME_DIR)
├── murmur.pid               # Daemon PID file
├── murmur.log               # Structured logs
│
├── plans/
│   └── plan-1.md            # Stored plan artifacts
│
├── runtime/
│   ├── agents.json          # Agent metadata (best-effort)
│   └── dedup.json           # Webhook deduplication
│
└── projects/
    └── <project-name>/
        ├── repo/            # Git clone of remote
        ├── worktrees/
        │   ├── wt-a-1/      # Coding agent worktree
        │   ├── wt-plan-1/   # Planner worktree
        │   └── wt-manager/  # Manager worktree
        └── permissions.toml # Project-specific rules

~/.config/murmur/
├── config.toml              # Global configuration
└── permissions.toml         # Global permission rules
```

### Persistence Semantics

| File | Purpose | Durability |
|------|---------|------------|
| `config.toml` | Project registry, settings | Atomic writes |
| `agents.json` | Agent metadata | Best-effort snapshot |
| `dedup.json` | Webhook deduplication | Best-effort |
| `murmur.log` | Daemon logs | Append-only |

---

## Key Runtime Flows

### 1. Daemon Startup

```
1. Resolve paths (MurmurPaths)
2. Load config.toml
3. Initialize SharedState
4. Load persisted agent metadata (best-effort recovery)
5. Bind Unix socket
6. Start webhook server (if enabled)
7. Start orchestrators for autostart projects
8. Enter event loop
```

### 2. Add Project

```
CLI: mm project add <url> --name myproj
                    │
                    ▼
            IPC: project.add
                    │
                    ▼
┌─────────────────────────────────────────┐
│ Daemon:                                  │
│  1. Validate config                      │
│  2. git clone <url> → projects/myproj/repo/
│  3. Create worktrees/ directory          │
│  4. Add to config.toml                   │
│  5. Return success                       │
└─────────────────────────────────────────┘
```

### 3. Orchestration Tick

```
┌─────────────────────────────────────────────────────────────────┐
│ Every ~10 seconds (or on webhook trigger):                       │
│                                                                  │
│  1. Query ready issues from backend                              │
│  2. Filter by backend rules (status, authors)                    │
│  3. Filter out claimed issues                                    │
│  4. Compute spawn plan (murmur-core)                            │
│     → available = max_agents - active_agents                    │
│     → spawn up to 'available' unclaimed issues                  │
│  5. For each issue in plan:                                      │
│     a. Create worktree                                           │
│     b. Spawn agent process                                       │
│     c. Record claim                                              │
│     d. Send kickstart prompt                                     │
└─────────────────────────────────────────────────────────────────┘
```

### 4. Permission Request (Claude PreToolUse)

```
Agent calls tool
       │
       ▼
Claude invokes: mm hook PreToolUse
       │
       ▼
┌─────────────────────────────────────────┐
│ 1. Load rules (project + global)         │
│ 2. Evaluate rules (pure)                 │
│    → Match: return allow/deny            │
│    → No match: continue                  │
│ 3. Send permission.request to daemon     │
│ 4. Block waiting for response            │
│ 5. Return decision to Claude             │
└─────────────────────────────────────────┘
       │
       ▼
User approves/denies via TUI or CLI
       │
       ▼
Daemon sends response, hook unblocks
       │
       ▼
Claude proceeds or stops
```

### 5. Agent Done → Merge

```
Agent calls: mm agent done
       │
       ▼
┌─────────────────────────────────────────────────────────────────┐
│ Daemon merge pipeline (direct strategy):                         │
│                                                                  │
│  1. Acquire project merge lock                                   │
│  2. git fetch --prune origin                                     │
│  3. Reset local default branch to origin/<default>               │
│  4. Rebase agent worktree onto origin/<default>                  │
│     → On conflict: mark agent "needs_resolution", stop           │
│  5. Fast-forward merge murmur/<agent-id> into default branch    │
│  6. git push origin <default>                                    │
│  7. Close issue (via backend)                                    │
│  8. Release claim                                                │
│  9. Record commit in log                                         │
│ 10. Remove worktree                                              │
│ 11. Clean up agent runtime                                       │
│ 12. Release merge lock                                           │
└─────────────────────────────────────────────────────────────────┘
```

---

## Further Reading

### Component Deep Dives

- [Agents](components/AGENTS.md) — State machine, backends, chat history
- [Orchestration](components/ORCHESTRATION.md) — Spawn policy, claims
- [Issue Backends](components/ISSUE_BACKENDS.md) — tk, GitHub, Linear
- [Permissions](components/PERMISSIONS_AND_QUESTIONS.md) — Rules, hooks
- [Worktrees & Merge](components/WORKTREES_AND_MERGE.md) — Git isolation
- [Daemon](components/DAEMON.md) — Internals, startup, state
- [IPC](components/IPC.md) — Protocol specification

### Code Pointers

| Concern | Location |
|---------|----------|
| Agent state machine | `murmur-core/src/agent.rs` |
| Spawn policy | `murmur-core/src/orchestration.rs` |
| Rule evaluation | `murmur-core/src/permissions.rs` |
| Daemon entry | `murmur/src/daemon/mod.rs` |
| Socket server | `murmur/src/daemon/server.rs` |
| Shared state | `murmur/src/daemon/state.rs` |
| Merge pipeline | `murmur/src/daemon/merge.rs` |
| Git operations | `murmur/src/git.rs` |
