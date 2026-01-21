# Orchestration

Orchestration is the per-project loop that:
- polls the configured issue backend for “ready” issues
- spawns coding agents up to `max-agents`
- maintains a claim registry to avoid duplicate work

Code pointers:
- Pure scheduling decision: `crates/murmur-core/src/orchestration.rs`
- Daemon loop: `crates/murmur/src/daemon/orchestration.rs`
- RPC control: `crates/murmur/src/daemon/rpc/orchestration.rs`
- Claims: `crates/murmur-core/src/claims.rs`

---

## The Core Tick Function

The “how many and which issues should we spawn?” logic is pure:

- Inputs:
  - `active_agents`
  - `max_agents`
  - ordered `ready_issue_ids`
  - current `claims`
- Output:
  - `SpawnPlan { issue_ids: Vec<String> }`

This logic lives in `murmur-core` (`orchestrator_tick`).

The daemon provides the inputs and executes the returned plan.

---

## Claims

Claims prevent duplicate work:
- when an agent is spawned for `<project>/<issue-id>`, a claim is recorded
- the orchestrator skips already-claimed issues
- claims are released on:
  - `agent done`
  - `agent abort/delete`
  - `project remove`

Inspect via CLI:
- `murmur claim list --project myproj`
- `mm claims --project myproj`

---

## Spawn Policy

At each tick:
1. Query ready issues from the configured backend.
2. Apply any backend-specific “ready” semantics (blocked status, allowed authors, etc).
3. Compute a spawn plan via `murmur-core`.
4. Spawn up to `available_slots = max_agents - active_agents` new agents.

Agents are created in dedicated worktrees:
- `projects/<project>/worktrees/wt-<agent-id>/`

Details: `docs/components/WORKTREES_AND_MERGE.md`.

---

## Completion and Refill

When an agent completes (`agent done`):
- Murmur merges the agent branch (direct merge strategy).
- Murmur closes the issue (backend-specific).
- The claim is released.
- The orchestrator can spawn the next ready issue on the next tick.

Merge serialization is per project to avoid concurrent merges stepping on each other.

---

## Webhook Tick Requests (Optional)

If the webhook server is enabled, incoming GitHub/Linear events can request a tick:
- daemon emits `orchestration.tick_requested` event
- orchestrator loop observes this and runs a tick sooner than the normal interval

Details: `docs/components/WEBHOOKS.md`.
