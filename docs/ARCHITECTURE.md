# Fugue (Rust) — Architecture

Fugue is a local-only coding-agent supervisor. It manages multiple Claude Code or Codex CLI instances across multiple projects, isolates each agent in its own git worktree, assigns work from pluggable issue backends, and provides a CLI for monitoring + approvals.

This document is the architecture contract for Fugue. It describes the intended components, boundaries, protocols, on-disk layout, and runtime flows. Improvements and redesigns are explicitly out of scope for the initial implementation.

If you’re looking for user docs rather than internals:
- `docs/USAGE.md` (end-to-end)
- `docs/CLI.md` (CLI reference)

If you want deeper internals, see `docs/components/` (daemon, IPC, worktrees, orchestration, backends, permissions).

---

## 1) Goals

**Local-only control plane**
- Run a daemon on the local machine.
- CLI connects via a Unix domain socket.
- No remote / multi-machine mode.

**Multi-project supervision**
- Register multiple projects (by git remote URL).
- Each project has per-project configuration (max agents, issue backend, merge strategy, backends, permissions checker).

**Multi-agent orchestration**
- A per-project orchestrator periodically polls for “ready” issues.
- Spawns coding agents up to `max-agents`.
- Agents operate in isolated git worktrees.

**Issue backends**
- `tk` (file-based) is the default (stored in-repo under `.fugue/tickets/`).
- GitHub Issues backend.
- Linear Issues backend (GraphQL API).

**Agent lifecycle and completion**
- Agents claim issues to avoid duplication.
- Agents work, commit, close issues, and signal completion (`agent done`).
- Orchestrator merges to the project default branch (direct merge strategy).

**Interactive supervision**
- Use the CLI to list agents, view chat history, and inspect recent work.
- Use the CLI to approve/deny permissions and answer AskUserQuestion prompts.

**Permission controls**
- Rule-based allow/deny/pass (`permissions.toml`) for Claude Code tools via hooks.
- Manual approvals via CLI when rules don’t decide.
- Optional LLM-based authorization for permission decisions.

**Planning + manager modes**
- Planner agents for exploring/designing and writing a plan artifact.
- Manager agents for project-scoped coordination (restricted command capabilities).

---

## 2) Non-Goals

To keep the initial implementation focused, the following are explicitly out of scope:

- **No “improvement roadmap” items** (persistence enhancements, recovery, refactors, new assignment strategy, worktree pooling, etc.).
- **No remote service / HTTP API** for client control (local-only IPC).
- **No guaranteed agent preservation across daemon restarts** (restart/recovery is best-effort only).
- **No distributed scheduling** or cluster coordination.
- **No new trackers** beyond `tk`, GitHub, Linear (Jira support is deferred).

We *do* still want clean module boundaries so improvements are possible later, but the initial implementation should match the baseline behavior first.

---

## 3) Architecture Principles (Coding Style)

These are mandatory design constraints for the Rust implementation.

### Functional Core, Imperative Shell

**Core**:
- Pure functions over immutable values.
- No filesystem, network, subprocess, sockets, time, randomness, or logging side effects.
- Core returns **decisions and actions** as data (commands to execute), not effects.

**Shell**:
- Executes I/O (git, process spawning, socket server, HTTP calls, file reads/writes).
- Translates between external protocols (JSON IPC, Claude/Codex JSONL) and core values.
- Emits events and persists/updates runtime state.

### High Cohesion, Low Coupling

- Modules should have one job and a narrow surface area.
- Dependencies flow from unstable/IO-heavy code **toward** stable/pure code (dependency inversion).
- Use small traits at boundaries (“ports”).
- Use DTOs at boundaries; avoid passing giant structs when only a field is needed.

### Values over State / Decomplected Concerns

- Represent domain state as values (`struct State { ... }`) and evolve via explicit events/commands.
- Avoid global singletons.
- Avoid mixing “what” with “when”: the core describes “what should happen”, the shell schedules/executes.

---

## 4) Process Topology (Runtime)

Fugue runs as several cooperating local processes:

1. **Daemon** (`fugue server start`)
   - Owns long-lived state: registered projects, running orchestrators, agents, pending approvals.
   - Exposes a Unix socket IPC API.
   - Broadcasts streaming events to attached clients (`attach`).

2. **CLI** (`fugue ...`)
   - Sends request/response messages to the daemon for almost all commands.
   - Also provides hook commands invoked by Claude Code (permission hooks, idle notifications).

3. **Agent processes** (spawned subprocesses)
   - Coding agents: Claude Code CLI or Codex CLI, one per worktree.
   - Planner agents: Claude/Codex in “plan mode” prompt.
   - Manager agents: Claude/Codex with restricted capabilities (focuses on Claude-style restrictions).

---

## 5) On-Disk Layout

Default base directory: `~/.fugue/` (override with `FUGUE_DIR`).

Suggested layout:

```
~/.fugue/
  fugue.sock
  fugue.pid
  fugue.log
  plans/
    <id>.md
  runtime/
    agents.json        # best-effort metadata (not authoritative)
    dedup.json         # webhook dedup store
  projects/
    <project>/
      repo/            # git clone of remote
      worktrees/
        wt-<agentid>/
        wt-plan-<planid>/
        wt-manager/
      permissions.toml # project-scoped rules (optional)
```

Config locations:

- Global config: `~/.config/fugue/config.toml` (or `$FUGUE_DIR/config/config.toml`)
- Global permissions: `~/.config/fugue/permissions.toml` (or `$FUGUE_DIR/config/permissions.toml`)

`FUGUE_DIR` overrides base paths for local testing/isolation.

---

## 6) Configuration Model

### Global config (`config.toml`)

Top-level concerns:
- log level
- provider API keys (anthropic/openai/github)
- LLM auth settings (provider/model)
- defaults for new projects
- webhook server settings

### Project config (`[[projects]]`)

Each project stores:
- `name`
- `remote-url`
- `max-agents`
- `autostart`
- `issue-backend`: `tk | github | gh | linear`
- `permissions-checker`: `manual | llm`
- `agent-backend` (fallback), plus `planner-backend`, `coding-backend`: `claude | codex`
- `merge-strategy`: `direct | pull-request`
- GitHub-specific fields (e.g., allowed authors list)
- Linear-specific fields (e.g., `linear-team` required, `linear-project` optional)

The daemon is the source of truth for the loaded config; CLI commands mutate config through daemon APIs (the daemon writes to `config.toml`).

---

## 7) Component Model (Conceptual)

### 7.1 Daemon / Supervisor (Control Plane)

Responsibilities:
- IPC request routing (single protocol surface)
- project registry (load/save config)
- orchestrator lifecycle (start/stop per project)
- agent lifecycle (create/delete/abort/send message)
- streaming events to clients (attach/detach + broadcast)
- permission request coordination (pending set + response channels)
- AskUserQuestion coordination
- heartbeat monitoring (stuck agent detection)
- webhook server (optional)

Non-responsibilities:
- doesn’t implement git details (delegates to git/worktree adapter)
- doesn’t implement issue backend logic (delegates to issue adapters)
- doesn’t implement LLM parsing/protocol details (delegates to adapters)

### 7.2 Orchestrator (Per Project)

Responsibilities:
- polling loop (default interval ~10s)
- decides how many agents to spawn
- merges/PRs on `agent done`
- manages in-memory claim registry
- tracks recent merged commits for UI (“recent work”)

### 7.3 Agents

Three kinds:
- **Coding agents**: claim tasks, implement code, commit, close issue, done.
- **Planner agents**: explore + write plans into `plans/<id>.md`.
- **Manager agent**: project-scoped coordinator; restricted commands.

### 7.4 Issue backends

Common interface:
- `get(id)`, `list(filter)`, `ready()`
- `create`, `update`, `close`
- `comment`, `upsert_plan_section` where supported
- `commit()` for `tk` (git add/commit/push), no-op for API backends

Includes:
- `tk`: files in `.fugue/tickets/` within the project repo clone
- GitHub Issues (API-backed)
- Linear Issues (API-backed)

For the canonical `.fugue/tickets/*.md` format, see `docs/TICKETS.md`.

#### GitHub backend (GraphQL API)

Fugue speaks to GitHub Issues via the GraphQL API.

Requirements:
- `owner/repo` is detected from the project repo’s `origin` remote URL (must be a GitHub remote).
- Token is sourced from `[providers.github].token` (or `api-key`) or `GITHUB_TOKEN` / `GH_TOKEN`.
- `allowed-authors` (optional): if empty, defaults to the repository owner; used to filter `ready()`.

#### Linear backend (GraphQL API)

Fugue speaks to Linear via the GraphQL API.

Requirements:
- API key is sourced from `[providers.linear].api-key` or `LINEAR_API_KEY`.
- Per-project `linear-team` is required (team UUID) and `linear-project` is optional (project UUID).

### 7.5 Permissions / Hooks

Claude Code integration:
- Claude invokes `fugue hook PreToolUse` with JSON stdin.
- Hook evaluates rules. If undecided, it asks the daemon and blocks for response.
- Hook returns the decision JSON to Claude Code.

Codex integration:
- Codex uses built-in approval modes; Fugue cannot intercept tool execution.
 
### 7.6 TUI

Fugue ships a single-screen TUI (`fugue tui`) built on top of the daemon’s `attach` event stream. See `docs/TUI.md`.

---

## 8) IPC Protocol (Daemon Socket)

### Transport

- Unix domain socket.
- JSON envelope messages.
- Streaming clients use a dedicated connection (separate socket connection) for live events.

### Envelope

All requests and responses use:

```json
{ "type": "project.list", "id": "req-123", "payload": { } }
```

```json
{ "type": "project.list", "id": "req-123", "success": true, "payload": { } }
```

### Core message categories

- server: `ping`, `shutdown`
- orchestration: `start`, `stop`, `status`
- projects: `project.add`, `project.remove`, `project.list`, `project.config.get/set/show`
- agents: `agent.list`, `agent.create`, `agent.delete`, `agent.abort`, `agent.send_message`, `agent.chat_history`, `agent.describe`, `agent.done`, `agent.claim`, `agent.idle`
- streaming: `attach`, `detach`
- permissions: `permission.request/respond/list`
- questions: `question.request/respond`
- planner: `plan.start/stop/list/send_message/chat_history`
- manager: `manager.start/stop/status/send_message/chat_history/clear_history`
- stats: `stats`, `commit.list`, `claim.list`

### Streaming events

After `attach`, the daemon emits events like:
- agent created/deleted/state/info
- chat entries (assistant/user/tool)
- permission requests and their pending state
- user questions
- planner events
- manager events

Event schema is “append-only messages” for the UI to consume; the UI keeps its own view-model state.

---

## 9) Agent Backends (Claude Code / Codex)

### 9.1 Claude Code backend

Characteristics:
- Long-lived subprocess.
- Multi-turn messages are written to stdin (JSONL).
- Tool execution is interceptable via Claude hooks (permission system).
- Output is `stream-json` JSONL; parse into a canonical internal `StreamMessage` representation.

Command shape (illustrative):
- `claude --output-format stream-json --input-format stream-json --verbose ... --settings <json> --plugin-dir <dir>`

Hooks:
- `PreToolUse` → permission interception
- `Stop` → idle notification (daemon can resume kickstart after idle)

### 9.2 Codex CLI backend

Characteristics:
- Process-per-turn (resume via thread id).
- Output is an event stream (JSONL) that must be converted to canonical `StreamMessage`.
- No external permission hook interception (Codex approval is built-in).

Command shape (illustrative):
- `codex exec --json --full-auto ... "<prompt>"`
- `codex exec resume --json --full-auto <thread-id> "<prompt>"`

Avoid attempting to fix upstream behavioral inconsistencies in this phase.

---

## 10) Worktree & Merge Model

Each coding agent:
- gets a dedicated worktree under `projects/<project>/worktrees/wt-<agentid>/`
- works on a branch `fugue/<agentid>` (naming can be finalized later)

On agent completion (`agent done`):
- **direct** merge strategy:
  - detect default branch (`origin/<default>`)
  - rebase agent worktree on `origin/<default>`
  - fast-forward merge into the default branch
  - push `origin/<default>`
  - on conflicts: agent remains running to resolve
- **pull-request** strategy:
  - reserved for later (config key exists; direct merge is implemented)

The merge path is serialized to avoid concurrent merges stepping on each other.

---

## 11) Key Runtime Flows

### 11.1 Daemon startup

Shell:
1. load config + registry
2. ensure base dirs exist
3. start IPC server
4. start webhook server (if enabled)
5. start orchestrators for `autostart` projects

Core:
- derives initial supervisor state from config.

### 11.2 Add project

Shell:
- persist registry entry
- create `projects/<name>/`
- `git clone <remote-url> projects/<name>/repo/`

Core:
- validates config values and returns “effects” to execute (clone, mkdir).

### 11.3 Orchestrator tick → spawn

Core:
- input: current counts (active agents), ready issue count, claimed set.
- output: `n_to_spawn`.

Shell:
- create worktree(s)
- spawn agent subprocess(es)
- send kickstart prompt

### 11.4 Permission request (Claude)

Shell:
- hook parses stdin
- evaluates rules
- if undecided: IPC to daemon, await response
- returns decision JSON to Claude

Core:
- rule evaluation is pure and deterministic

### 11.5 Agent done → merge

Shell:
- execute merge strategy I/O
- on success: stop/delete agent, release claims, update recent-work log

Core:
- decides state transitions + which side effects to run

---

## 12) Rust Codebase Structure (Implemented)

Fugue is a Cargo workspace with explicit “core vs shell” boundaries:

### `crates/fugue-core/` (functional core)

- Pure domain values and deterministic logic.
- No Tokio runtime; no filesystem/network/subprocess/socket I/O.
- Key modules:
  - `agent.rs` — agent record state machine + chat buffer
  - `issue.rs` — ticket parsing/formatting, `## Plan` upsert, ready computation
  - `orchestration.rs` — pure spawn policy
  - `permissions.rs` — rule evaluation for tool approvals
  - `paths.rs` — deterministic path resolution (inputs passed in)

### `crates/fugue-protocol/` (wire DTOs)

- Serde request/response/event types + message constants.
- Defines IPC payload schemas and event schemas.

### `crates/fugue/` (imperative shell)

- Tokio-based daemon + client + CLI.
- Owns I/O and adapters:
  - git and worktrees (`git.rs`, `worktrees.rs`)
  - HTTP backends (`github.rs`, `linear.rs`)
  - local tickets backend (`issues.rs`)
  - daemon runtime (`daemon/`)
  - CLI entrypoint (`src/main.rs`)

Dependency direction:
- `fugue` depends on `fugue-core` and `fugue-protocol`.
- `fugue-core` and `fugue-protocol` are independent of the shell crate and avoid I/O.

---

## 13) “Ports and Adapters” Interfaces (Key Decoupling Points)

To keep coupling low while still shipping quickly, define minimal traits (“ports”) for:

- `Clock` (shell supplies time; core consumes timestamps)
- `IdGen` (shell supplies ids; core consumes values)
- `Git` (worktree create/reset, merge/rebase/push)
- `Process` (spawn/stop, stdin/stdout streaming)
- `IssueBackend` (tk/github/linear)
- `PermissionsDecider` (manual via daemon/CLI vs LLM)
- `Storage` (config, plans, runtime metadata)

The core should never depend on concrete implementations, only these traits or explicit DTO inputs.

---

## 14) Observability

Requirements:
- structured logs to file (daemon)
- actionable error messages for CLI commands
- enough event emission for `attach` consumers (and future UIs)

Avoid adding new metrics systems or tracing pipelines; keep it simple and local.

---

## 15) Release Checklist (What we must ship first)

- Daemon IPC server + CLI client
- Project registry + clone-on-add
- Worktree creation per agent
- Orchestrator polling + spawning
- Issue backends: `tk` (`.fugue/tickets/`), GitHub Issues, Linear Issues
- In-memory claim registry
- Agent spawn + streaming parse + chat history buffer
- `agent done` path + merge strategy “direct”
- Claude hook command: PreToolUse + Stop idle
- Permissions: global + project rules, manual approvals via CLI
- Plan storage + planner agents
- Manager agents with allowlisted commands (Claude settings allow-list)
- Optional webhook server + dedup store

Anything beyond this is explicitly out of scope for this phase.
