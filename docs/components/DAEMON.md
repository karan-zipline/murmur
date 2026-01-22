# Daemon Internals

This doc describes the Murmur daemon (the long-running supervisor process).

Code pointers:
- Daemon entry: `crates/murmur/src/daemon/mod.rs`
- Socket server: `crates/murmur/src/daemon/server.rs`
- Shared state model: `crates/murmur/src/daemon/state.rs`
- RPC handlers: `crates/murmur/src/daemon/rpc/`
- Webhooks: `crates/murmur/src/daemon/webhook.rs`

See also:
- `docs/components/IPC.md`
- `docs/components/ORCHESTRATION.md`
- `docs/components/AGENTS.md`

---

## Responsibilities

The daemon is the control plane. It owns:

- Loaded configuration (`config.toml`) and project registry.
- Orchestrator lifecycle per project (start/stop, status).
- Running agent processes (coding, planner, manager).
- Claim registry (which agent is assigned to which issue).
- Pending permission requests and user questions.
- Event broadcast stream used by attached clients (`attach`).
- Optional webhook server (tick requests).

The daemon does *not* implement business rules as side-effecting code:
- Pure logic lives in `murmur-core` (e.g. orchestration tick decisions, parsing, plan upserts).
- The daemon is the imperative shell: git, filesystem, sockets, subprocesses, HTTP.

---

## Startup Sequence (High-Level)

1. Resolve `MurmurPaths` (honors `MURMUR_DIR`).
2. Load config: `crates/murmur/src/config_store.rs`.
3. Initialize `SharedState`:
   - `config`, `agents`, `claims`, `orchestrators`, `pending_permissions`, `pending_questions`, etc.
4. **Rehydrate agents** from `runtime/agents.json`:
   - Load persisted agent metadata from disk.
   - Skip agents whose worktrees no longer exist.
   - Check if agent processes are still running (via `/proc/<pid>`).
   - Restore agent runtime entries so that `mm agent claim` and `mm agent done` work for agents from previous daemon sessions.
   - Agents with dead processes are marked as `Exited`.
5. Start the Unix socket server (`murmur.sock`). By default, the socket is placed in `XDG_RUNTIME_DIR` when available; when `MURMUR_DIR` is set, it is placed under `$MURMUR_DIR/murmur.sock`.
6. Start webhook server if enabled.
7. Autostart orchestrators for projects with `autostart = true`.

---

## Concurrency Model

The daemon runs on Tokio and uses:

- `tokio::sync::Mutex` for state partitions (`agents`, `config`, `claims`, ...).
- A `broadcast::Sender<Event>` for event fanout to attached clients.
- Per-agent Tokio tasks for:
  - reading stdout stream and converting to canonical chat/messages
  - handling outbound messages (stdin) to the agent backend
  - handling abort/shutdown signals

Important invariants:
- Merge operations are serialized per project via a per-project mutex (`merge_lock_for_project`).
- Agent runtime cleanup is best-effort; on errors it logs and continues.

---

## Shared State

`SharedState` (`crates/murmur/src/daemon/state.rs`) is the daemon’s in-memory “database”.

Key fields:
- `paths: MurmurPaths` — resolved filesystem layout.
- `config: Mutex<ConfigFile>` — loaded global config (`[[projects]]` etc).
- `agents: Mutex<AgentsState>` — live agent runtimes.
- `claims: Mutex<ClaimRegistry>` — issue claims (prevents duplicate work).
- `orchestrators: Mutex<BTreeMap<String, OrchestratorRuntime>>` — per-project loops.
- `pending_permissions`, `pending_questions` — requests blocked on user response.
- `completed_issues` — per-project set of issue ids completed in this daemon lifetime.
- `commits` — per-project in-memory commit log used for `stats` (and future UIs).

Persistence is intentionally best-effort; see `docs/components/STORAGE.md`.

---

## Shutdown Semantics

`server stop` triggers:
- daemon shutdown signal
- orchestrators stop
- agents are asked to stop/abort (best-effort)
- socket server exits
- webhook server exits (graceful shutdown)

The daemon prefers to shut down cleanly but does not treat partial cleanup as fatal.
