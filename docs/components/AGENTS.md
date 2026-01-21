# Agents

Fugue manages multiple “agents” per project. An agent is a long-lived subprocess running in an isolated git worktree and connected to the daemon via stdin/stdout streams.

Agent roles:
- `coding` — implements an issue and signals completion (`agent done`)
- `planner` — produces a plan artifact under `plans/`
- `manager` — project-scoped interactive coordinator (restricted capabilities)

Code pointers:
- Domain state machine: `crates/fugue-core/src/agent.rs`
- Daemon runtime state: `crates/fugue/src/daemon/state.rs` (`AgentRuntime`)
- Spawn + stream wiring: `crates/fugue/src/daemon/mod.rs`
- Agent RPC: `crates/fugue/src/daemon/rpc/agent.rs`
- Planner RPC: `crates/fugue/src/daemon/rpc/plan.rs`
- Manager RPC: `crates/fugue/src/daemon/rpc/manager.rs`
- Stream parsing: `crates/fugue-core/src/stream/`

---

## Agent Record vs Agent Runtime

Fugue decomplects agent state into:

### `AgentRecord` (domain value; persistable)

`AgentRecord` (`fugue-core`) is an immutable value updated via explicit events:
- identity: `id`, `project`, `role`, `issue_id`
- state: `starting|running|needs_resolution|exited|aborted`
- metadata: timestamps, worktree dir, optional `description`, optional `pid`/exit info

### `AgentRuntime` (imperative shell state)

`AgentRuntime` (`fugue` daemon) contains:
- the current `AgentRecord`
- the selected backend (`claude` or `codex`)
- backend-specific runtime handles (e.g., Codex thread id)
- chat history buffer
- channels for outbound messages and abort signals
- Tokio tasks for stream reading / message delivery

Only the daemon owns `AgentRuntime`.

---

## Worktrees and Branches

Each coding agent runs in its own git worktree:

`projects/<project>/worktrees/wt-<agent-id>/`

Agent branches are named:

`fugue/<agent-id>`

See `docs/components/WORKTREES_AND_MERGE.md`.

---

## Backends

### Claude Code (`claude`)

- Long-lived interactive subprocess (`claude --output-format stream-json --input-format stream-json ...`)
- Supports tool interception via hooks (permissions + AskUserQuestion)
- Fugue injects env vars:
  - `FUGUE_AGENT_ID` and `FAB_AGENT_ID`
  - `FUGUE_PROJECT` and `FAB_PROJECT`
- Fugue injects Claude hook commands into the `--settings` JSON:
  - `fugue hook PreToolUse` (permissions/questions)
  - `fugue hook Stop` (idle notification)

### Codex CLI (`codex`)

- Best-effort “resume” using a thread id (backend dependent)
- Produces a JSONL stream parsed into canonical chat messages
- Tool approvals are handled by Codex itself (Fugue cannot intercept tool execution)

---

## Messaging and Chat History

Fugue normalizes backend outputs into `ChatMessage { role, content, ts_ms }`:
- stored in a bounded in-memory ring buffer (`ChatHistory`)
- exposed via:
  - `agent chat-history <agent-id>` (hidden from `--help`)
  - `agent.chat` events for streaming clients

Sending a message (`agent send-message`) appends a `user` chat entry locally and forwards the message to the backend process.

---

## Completion (`agent done`)

When an agent completes:
- the agent calls `fugue agent done` (uses `FUGUE_AGENT_ID`)
- the daemon:
  - performs merge pipeline (direct merge strategy)
  - closes the issue in the configured backend
  - releases the claim
  - records the merge in the per-project commit log
  - cleans up the agent runtime and worktree (unless conflicts require resolution)

If a merge conflict occurs:
- the agent transitions to `needs_resolution`
- the worktree is kept so conflicts can be resolved manually

---

## Agent-Driven Commands

These commands are designed for the agent process to call (via env vars):

- `fugue agent claim <issue-id>` — uses `FUGUE_AGENT_ID`/`FAB_AGENT_ID`
- `fugue agent claim <issue-id>` — uses `FUGUE_AGENT_ID`
- `fugue agent describe <text>` — uses `FUGUE_AGENT_ID`
- `fugue agent done [--task ...] [--error ...]` — uses `FUGUE_AGENT_ID`

Planner agents similarly write plan artifacts via:
- `fugue plan write` (stdin → `plans/<id>.md`)

---

## Cleanup

Agent cleanup is best-effort and depends on role:
- coding/planner: remove git worktree (`git worktree remove --force`)
- project-less planner: remove its working directory under `~/.fugue/planners/<id>/`

Projects can also be removed with `--delete-worktrees` to delete all worktrees.
