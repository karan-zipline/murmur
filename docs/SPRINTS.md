# Fugue — Sprint Plan & Ticket Breakdown

This is the implementation plan for **Fugue**.

- Architecture contract: `docs/ARCHITECTURE.md`
- Scope: local-only, no remote control plane.
- Issue backends: **`tk`** (`.fugue/tickets/`), **GitHub Issues** (GraphQL API), **Linear Issues** (GraphQL API).
- MVP note: **CLI-only**. The TUI is reintroduced post-MVP (Sprints 13–15).

---

## Conventions

### Definition of done (every ticket)

Each ticket is an atomic, mergeable change that includes:

- Production code + any necessary docs.
- Tests where practical:
  - Pure/core logic → unit tests.
  - IO-heavy code → integration tests using `tempfile`, fake binaries on `PATH`, and/or mock HTTP servers.
- A clear validation command (minimum): `cargo test --workspace`.

### Ticket IDs

Ticket IDs are grouped by sprint:

- Sprint 1: `FUGUE-001` … `FUGUE-099`
- Sprint 2: `FUGUE-101` … `FUGUE-199`
- Sprint 3: `FUGUE-201` … `FUGUE-299`
- …

### Demo expectation (every sprint)

Each sprint ends with a **demoable** increment that:

- Runs locally (`cargo run …`).
- Can be validated (tests and/or a scripted smoke check).
- Builds directly on the previous sprint.

---

## Plan Notes (Post Sprint 3)

Implementation discoveries and plan adjustments so far:

- **Workspace shape**: We committed to “Option B” early: `fugue-core` (pure), `fugue-protocol` (serde DTOs), `fugue` (daemon/client shell; also exposes a small library for integration tests).
- **IPC framing**: IPC is **JSONL** over a Unix socket. `attach` is intended to be used on a dedicated connection; responses and events are both JSON objects and can be interleaved (e.g., `detach` response while heartbeats are streaming).
- **Config preservation**: `config.toml` parsing preserves unknown fields via `#[serde(flatten)]` so we don’t lose unmodeled config when rewriting the file.
- **`FUGUE_DIR` behavior**: When `FUGUE_DIR` is set, config lives under `$FUGUE_DIR/config/config.toml` (this makes tests fully isolated).
- **Project naming**: `project add <path-or-url>` infers a name by default (or override with `--name`); legacy `project add <name> --remote-url <url>` is still supported.
- **`project status` semantics**: `project status` is computed by the daemon; `socket_reachable` is always `true` for successful responses.
- **Tracker scope**: targets **Linear**. Jira support is deferred.

---

## Feature Coverage Tracking

By the end of Sprint 12, Fugue should cover the **CLI/daemon** local-only feature set (with `tk` + GitHub + Linear issue backends). Sprints 13–15 add the TUI.

- **Control plane (daemon/CLI/IPC/events)**: Sprints 1–2
- **Project registry + clone-on-add**: Sprint 3
- **Worktrees + agent records**: Sprint 4
- **Issue backends** (`tk`, GitHub, Linear): Sprints 5–6
- **Orchestration + claims + auto-spawn**: Sprint 7 (+ `claim.list`)
- **Real agent backends + streaming + messaging**: Sprint 8
- **Completion pipeline (merge + close) + recent work + stats**: Sprint 9 (+ `stats`)
- **Permissions + hooks + approvals/questions**: Sprint 10
- **Planner/manager/webhooks/docs/smoke**: Sprint 12 (GitHub + Linear webhooks)
- **TUI (agents + chat + approvals + planner/manager + recent work)**: Sprints 13–15

---

## Sprint 1 — Workspace + Skeleton Binary

**Goal**: A buildable Rust workspace with a single `fugue` binary, sane boundaries (`core` vs `shell`), deterministic path resolution, and structured logging.

**Demo**:
- `cargo test --workspace`
- `cargo run -p fugue -- --help`
- `FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- server start --foreground` (starts, logs, clean shutdown via Ctrl-C)

### Tickets

#### FUGUE-001 — Create Cargo workspace + crates
- Outcome: Workspace with `fugue-core` (pure), `fugue-protocol` (DTOs), and `fugue` (binary shell).
- Validation: `cargo test --workspace` (compiles, empty tests OK).

#### FUGUE-002 — Establish baseline linting + formatting rules
- Outcome: `rustfmt` + `clippy` settings and CI-ready `cargo clippy --workspace --all-targets` cleanliness.
- Validation: `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`.

#### FUGUE-003 — Implement path resolution (`FUGUE_DIR`, config dirs)
- Outcome: A single `paths` module producing all filesystem locations from env/config (core computes from values; shell reads env).
- Validation: Unit tests for:
  - default paths
  - `FUGUE_DIR` override
  - safe join rules (no accidental traversal).

#### FUGUE-004 — Add structured logging to `~/.fugue/fugue.log`
- Outcome: `tracing` setup in the shell; log level configurable via CLI flag/env.
- Validation: Integration test that runs a trivial `fugue` subcommand with `FUGUE_DIR` set and asserts the log file is created + contains a known line.

#### FUGUE-005 — CLI command tree skeleton (no behavior yet)
- Outcome: `fugue` accepts subcommands matching architecture: `server`, `project`, `agent`, `issue`, `hook`, `plan`, `manager`, `permission`, `question`.
- Validation: `assert_cmd` test verifies `--help` includes the top-level subcommands.

#### FUGUE-006 — Daemon process skeleton (`server start --foreground`)
- Outcome: A long-running process with clean shutdown handling (Ctrl-C) and a “starting” log line.
- Validation: Integration test spawns `fugue server start --foreground` in the background, waits for a readiness line in logs, then terminates.

#### FUGUE-007 — Document local-only runtime layout (developer-facing)
- Outcome: `docs/ARCHITECTURE.md` references are mirrored in a short `docs/DEVELOPMENT.md` with “how to run” and where files live.
- Validation: Manual: doc exists and matches current code flags (no stale command names).

---

## Sprint 2 — IPC (Unix Socket) + Ping

**Goal**: Daemon socket server + CLI client with a stable JSON protocol envelope and a minimal request/response and event stream.

**Demo**:
- Terminal A: `FUGUE_DIR=/tmp/fugue-dev fugue server start --foreground`
- Terminal B: `FUGUE_DIR=/tmp/fugue-dev fugue ping` → `ok`

### Tickets

#### FUGUE-101 — Define IPC envelope + core message DTOs
- Outcome: `fugue-protocol` defines `RequestEnvelope`, `ResponseEnvelope`, `EventEnvelope` with versioning.
- Validation: Unit tests: serde round-trip + unknown-field tolerance.

#### FUGUE-102 — Implement JSONL framing (read/write)
- Outcome: Shared helper for newline-delimited JSON over `UnixStream`.
- Validation: Unit tests using in-memory streams (or `tokio::io::duplex`) for framing edge cases.

#### FUGUE-103 — Daemon: bind Unix socket at `FUGUE_DIR/fugue.sock`
- Outcome: Server accepts connections and dispatches messages by `type`.
- Validation: Integration test ensures socket file appears and accepts a connection.

#### FUGUE-104 — Implement `server.ping` handler + `fugue ping` CLI
- Outcome: `ping` request returns a success response with daemon metadata (version, pid).
- Validation: Integration test: run daemon + `fugue ping` succeeds.

#### FUGUE-105 — Implement `server.shutdown` handler + `fugue server shutdown`
- Outcome: Graceful daemon shutdown via IPC.
- Validation: Integration test: start daemon, call shutdown, ensure process exits within timeout.

#### FUGUE-106 — Event stream: `attach`/`detach` + periodic heartbeat event
- Outcome: A dedicated streaming connection that receives events as JSONL.
- Validation: Integration test: attach, read at least one heartbeat event, detach cleanly.

---

## Sprint 3 — Config + Project Registry (Clone-on-Add)

**Goal**: Persisted config, project add/list/remove, and repo cloning into `~/.fugue/projects/<name>/repo/`.

**Demo**:
- `fugue project add file:///…/bare.git --name myproj` (or legacy `project add myproj --remote-url ...`)
- `fugue project list` shows `myproj`

### Tickets

#### FUGUE-201 — Define `config.toml` schema + validation in core
- Outcome: Pure config model: global + `[[projects]]` (including `issue-backend = tk|github|gh|linear`).
- Validation: Unit tests for valid/invalid configs (unknown enums, missing required fields).

#### FUGUE-202 — Shell: load/save config with atomic writes
- Outcome: `config.toml` read at startup; writes are atomic (write temp + rename).
- Validation: Integration tests validate persistence + parseability; atomicity is provided by `rename` (no crash simulation test yet).

#### FUGUE-203 — Daemon: project registry state + handlers
- Outcome: In-memory registry loaded from config; IPC: `project.list`.
- Validation: Integration test: seed config, start daemon, `project list` returns expected.

#### FUGUE-204 — Implement `project.add` (mkdir + git clone)
- Outcome: Adds project entry, creates dirs, clones remote into `projects/<name>/repo/`.
- Validation: Integration test using a local bare git repo remote.

#### FUGUE-205 — Implement `project.remove` (registry-only)
- Outcome: Removes project from config/daemon; does not delete on-disk repo unless explicitly requested.
- Validation: Integration test verifies config updated and daemon no longer lists project.

#### FUGUE-206 — Implement `project.config get/set/show`
- Outcome: Typed setters for important knobs (`max-agents`, `issue-backend`, `merge-strategy`, Linear fields, etc.).
- Validation: Unit tests for validation; integration test for `set` then `get`.

#### FUGUE-207 — Implement `project.status` (sanity checks)
- Outcome: Reports whether repo exists, remote matches, socket reachable, orchestrator running.
- Validation: Integration test validates repo existence + remote matching (socket reachability is implied by the IPC call).

---

## Sprint 4 — Git Worktrees + Agent Records (Dummy Backend)

**Goal**: Isolated worktrees per agent and a minimal “agent lifecycle” using a deterministic dummy agent backend for early end-to-end testing.

**Demo**:
- `fugue agent create myproj issue-123` (and/or `--backend dummy` if implemented as an override)
- `fugue agent list` shows one running agent

### Tickets

#### FUGUE-301 — Git adapter wrapper (shell)
- Outcome: A small `Git` port implemented via `git` subprocess calls (clone/fetch/worktree/branch/rebase/push primitives).
- Validation: Integration tests run against `tempfile` repos (no network).

#### FUGUE-302 — Worktree manager (`wt-<agentid>` layout)
- Outcome: Create/remove worktrees under `projects/<project>/worktrees/` and track paths.
- Validation: Integration test verifies `git worktree list` contains the new worktree.

#### FUGUE-303 — Default branch detection + branch naming rules
- Outcome: Determine main branch (`main`/`master`) and generate `fugue/<agentid>` branch names.
- Validation: Unit tests for parsing `git remote show origin` output (fixture-based).

#### FUGUE-304 — Agent domain model + in-memory registry (daemon)
- Outcome: Agent records (id, project, role, issue id, state, timestamps, worktree path).
- Validation: Unit tests for pure state transitions (start → running → done/aborted).

#### FUGUE-305 — Runtime persistence (best-effort) for agent metadata
- Outcome: Serialize a non-authoritative `runtime/agents.json` for UI and debugging.
- Validation: Integration test ensures file is written and parseable after agent create/delete.

#### FUGUE-306 — Dummy agent backend (test-only but runnable)
- Outcome: A backend that emits predictable JSONL “chat” output and responds to `send_message` deterministically.
- Validation: Integration test spawns dummy backend and asserts the daemon records expected messages.

#### FUGUE-307 — Implement `agent.create/list/delete/abort`
- Outcome: CLI + IPC handlers; `agent abort` terminates the process + marks state.
- Validation: Integration tests for create/list/abort/delete.

#### FUGUE-308 — Implement `agent.chat_history` + bounded in-memory buffer
- Outcome: Retrieve last N messages; buffer size configurable.
- Validation: Unit tests for buffer trimming; integration test returns dummy messages.

---

## Sprint 5 — `tk` Backend (Local `.fugue/tickets/`) + `issue` CLI

**Goal**: Full local ticket backend parity: parse/format, CRUD, `ready()`, comments, and `commit()` via git.

**Demo**:
- In repo clone: create `.fugue/tickets/issue-abc.md`
- `fugue issue ready --project myproj` lists it
- `fugue issue close --project myproj issue-abc`

### Tickets

#### FUGUE-401 — Define issue domain types + backend trait (core)
- Outcome: `Issue`, `Status` (`open|closed|blocked`), `CreateParams`, `UpdateParams`, `ListFilter`, `IssueBackend` trait.
- Validation: Unit tests for filters and status parsing.

#### FUGUE-402 — Implement tk ticket parser/formatter (pure)
- Outcome: YAML frontmatter + Markdown body format stored under `.fugue/tickets/`.
- Validation: Ported parser/formatter unit tests (fixtures for edge cases).

#### FUGUE-403 — Shell: tk backend adapter reading `.fugue/tickets/*.md`
- Outcome: `get/list/create/update/close/comment/ready` backed by files in the repo clone worktree.
- Validation: Integration tests on a temp git repo with `.fugue/tickets/`.

#### FUGUE-404 — Implement `issue` CLI (tk only, behind config)
- Outcome: `issue list/get/create/update/close/comment/ready` works for tk backend.
- Validation: Integration test: create issue then list/get/close.

#### FUGUE-405 — Implement tk `commit()` (git add/commit/push)
- Outcome: `issue commit` and backend auto-commit where needed.
- Validation: Integration test with a local bare “origin” verifying push occurred.

#### FUGUE-406 — Document tk ticket format and examples
- Outcome: `docs/TICKETS.md` with the canonical tk markdown format and examples.
- Validation: Manual doc review + referenced by `docs/ARCHITECTURE.md`.

---

## Sprint 6 — GitHub + Linear Backends

**Goal**: Support GitHub Issues (GraphQL API) and Linear Issues (GraphQL API) as pluggable backends, selectable per project.

**Demo**:
- GitHub: `GITHUB_TOKEN=… fugue issue ready --project myghproj`
- Linear: `LINEAR_API_KEY=… fugue issue ready --project mylinproj`

### Tickets

#### FUGUE-501 — Extend project config for GitHub + Linear fields
- Outcome: Config keys for GitHub allowed authors + token override; Linear team id + project id.
- Validation: Unit tests: config parse/validate (missing required `linear-team` rejected when `issue-backend=linear`).

#### FUGUE-502 — GitHub backend: detect `owner/repo` from git remote
- Outcome: Robust parsing for SSH/HTTPS remote URLs.
- Validation: Unit tests with fixture remote URLs.

#### FUGUE-503 — GitHub backend: GraphQL client + `list/get/ready`
- Outcome: Query issues, map blocked/labels/state into `Issue` domain.
- Validation: Integration tests with `wiremock` GraphQL server (no real GitHub calls).

#### FUGUE-504 — GitHub backend: `create/update/close/comment`
- Outcome: Mutations for issue lifecycle operations needed by Fugue workflows.
- Validation: `wiremock` tests validate request bodies and response mapping.

#### FUGUE-505 — Linear backend: GraphQL client + auth sources
- Outcome: GraphQL client with API key sourced from `[providers.linear].api-key` or `LINEAR_API_KEY`.
- Validation: `wiremock` tests validate request headers and response mapping.

#### FUGUE-506 — Linear backend: `list/get/ready`
- Outcome: Query issues scoped by `linear-team` (required) and `linear-project` (optional), consistent “ready” semantics.
- Validation: `wiremock` tests validate filter construction and issue mapping.

#### FUGUE-507 — Linear backend: `create/update/close/comment` (+ optional sub-issue)
- Outcome: Mutations for issue lifecycle operations needed by Fugue workflows.
- Validation: `wiremock` tests validate request bodies and response mapping.

#### FUGUE-508 — Backend selection per project (`issue-backend = …`)
- Outcome: Daemon chooses backend implementation per project config; CLI routes through daemon.
- Validation: Integration test with 2 projects configured for different backends (tk + fake linear).

---

## Sprint 7 — Orchestrator Loop + Claims + Auto-Spawn (Dummy Agents)

**Goal**: A working local orchestrator that polls `ready()` issues and spawns agents up to `max-agents`, with deduped claiming.

**Demo**:
- `fugue project start myproj`
- Create 2 tk issues; observe 1 agent spawns (if `max-agents=1`), then second spawns after first completes.

### Tickets

#### FUGUE-601 — Core: orchestrator tick function (pure)
- Outcome: Given (active agents, max-agents, ready issues, claimed set) produce `SpawnPlan`.
- Validation: Unit tests cover “no spawn”, “spawn N”, and “don’t spawn claimed” cases.

#### FUGUE-602 — Core: claim registry model (pure)
- Outcome: Deterministic claim set semantics (claim/release/list), keyed by (project, issue id).
- Validation: Unit tests for claim collisions and release.

#### FUGUE-603 — Daemon: per-project orchestrator runtime
- Outcome: `tokio` loop with configurable interval; start/stop per project.
- Validation: Integration test starts orchestrator and observes spawn attempt events.

#### FUGUE-604 — Spawn pipeline: claim → worktree → agent start → kickoff message
- Outcome: A single “spawn agent” operation producing consistent filesystem layout.
- Validation: Integration test asserts worktree exists and agent registry updated.

#### FUGUE-605 — Implement orchestration IPC + CLI (`start/stop/status`)
- Outcome: `fugue orchestration status` returns running/stopped and counts.
- Validation: Integration tests for start/stop/status.

#### FUGUE-606 — Implement `agent.done` signal path (CLI → daemon)
- Outcome: Agent can call `fugue agent done` from its worktree; daemon marks done and releases claim.
- Validation: Integration test using dummy agent that triggers `agent done`.

#### FUGUE-607 — Orchestrator: spawn next issue after done
- Outcome: With `max-agents=1`, completing agent causes next ready issue to spawn.
- Validation: Integration test with 2 tk issues and 1-slot cap.

#### FUGUE-608 — Implement `claim.list` (IPC + CLI)
- Outcome: List active claims (project + issue id + agent id).
- Validation: Integration test asserts claim appears after spawn and disappears after `agent done`.

---

## Sprint 8 — Real Agent Backends + Streaming Messages

**Goal**: Replace dummy agents with real Claude/Codex integration + canonical stream parsing, and expose agent messaging + tailing via CLI.

**Demo**:
- `fugue agent create --project myproj --backend codex --issue-id …`
- `fugue agent tail --agent-id …` shows live output
- `fugue agent send-message --agent-id … "…"`.

### Tickets

#### FUGUE-701 — Core: canonical `StreamMessage` + chat history model
- Outcome: Uniform representation for assistant/user/tool events across backends.
- Validation: Unit tests for serialization and ordering.

#### FUGUE-702 — Codex JSONL → `StreamMessage` parser (pure)
- Outcome: Deterministic parsing of Codex output stream into canonical events.
- Validation: Fixture-based unit tests (golden input → expected events).

#### FUGUE-703 — Claude stream-json → `StreamMessage` parser (pure)
- Outcome: Deterministic parsing of Claude output stream into canonical events.
- Validation: Fixture-based unit tests.

#### FUGUE-704 — Shell: process supervisor for long-lived child processes
- Outcome: Spawn, capture stdout/stderr, cancellation/kill, and event forwarding to daemon bus.
- Validation: Integration test with a fake agent binary emitting JSONL.

#### FUGUE-705 — Codex backend adapter (shell)
- Outcome: Start/resume thread id, send prompt, stream output; minimal config knobs.
- Validation: Integration tests with a fake `codex` binary on `PATH`.

#### FUGUE-706 — Claude backend adapter (shell)
- Outcome: Long-lived subprocess with stdin JSONL, stdout stream-json; hook integration paths.
- Validation: Integration tests with a fake `claude` binary on `PATH`.

#### FUGUE-707 — Implement `agent.send_message` + `agent.tail`
- Outcome: CLI commands + IPC; daemon routes messages to correct backend; tail attaches to event stream.
- Validation: Integration test: send message causes fake agent to emit a known response event.

---

## Sprint 9 — Completion Pipeline: Merge + Close

**Goal**: `agent done` triggers merge strategy (`direct` required), closes the issue, and updates “recent work”.

**Demo**:
- Run an agent that commits changes in its worktree.
- `fugue agent done --agent-id …` merges to `main` and closes the issue in tk/GitHub/Linear.

### Tickets

#### FUGUE-801 — Shell: direct merge strategy implementation (git subprocess)
- Outcome: fetch → rebase agent branch on `origin/main` → fast-forward main → push.
- Validation: Integration tests with local bare remote; asserts main advanced.

#### FUGUE-802 — Serialize merges per project
- Outcome: Prevent concurrent merges stepping on each other; queued/locked merges.
- Validation: Concurrency-focused integration test spawns two “done” calls and asserts serialization.

#### FUGUE-803 — Implement `agent.done` handler: merge + close + cleanup
- Outcome: On success: close issue, stop agent, release claim, optionally remove worktree.
- Validation: Integration test with tk issues + dummy/real agent committing changes.

#### FUGUE-804 — Conflict path: keep agent alive for resolution
- Outcome: If rebase fails, mark agent state `needs_resolution` and emit an event; do not close issue.
- Validation: Integration test sets up conflicting commits and asserts state transition.

#### FUGUE-805 — Recent work log + `commit.list`
- Outcome: Store a small recent-commits list per project for UI.
- Validation: Unit tests for log truncation; integration test reads from daemon.

#### FUGUE-806 — Optional: `pull-request` merge strategy (GitHub only)
- Outcome: Push branch + create PR via GraphQL; surface PR URL; skip direct merge.
- Validation: `wiremock` GraphQL test for PR creation request/response.

#### FUGUE-807 — Implement `stats` (IPC + CLI)
- Outcome: `stats` returns commit count (from per-project recent-work) and Claude-only usage stats (best-effort by parsing local `.claude` JSONL).
- Validation: Unit tests for usage parsing with fixtures; integration test asserts `stats` returns commit count for a project after a merge.

---

## Sprint 10 — Permissions + Hooks + Manual Approval (CLI)

**Goal**: Claude permission interception via hooks, rule evaluation, and daemon-mediated approvals (validated via CLI).

**Demo**:
- Run Claude agent with hooks configured.
- Trigger a tool request that rules don’t decide; approve via `fugue permission respond …`.

### Tickets

#### FUGUE-901 — Core: permissions rule schema + matcher (pure)
- Outcome: Rule evaluation (`allow|deny|pass`) for tool requests.
- Validation: Ported unit tests for pattern matching and precedence.

#### FUGUE-902 — Shell: load global + project `permissions.toml` with precedence
- Outcome: Combine global + project rules deterministically.
- Validation: Unit tests for merge precedence and “project overrides global”.

#### FUGUE-903 — Daemon: permission request queue + streaming events
- Outcome: Store pending approvals with correlation ids; emit events to attached clients.
- Validation: Integration test: create request, list pending, respond, verify completion.

#### FUGUE-904 — CLI: `permission list/respond`
- Outcome: Approval path via CLI.
- Validation: Integration tests for list/respond.

#### FUGUE-905 — CLI hook: `hook PreToolUse` (Claude)
- Outcome: Reads stdin JSON, evaluates rules; if undecided, blocks on daemon approval; returns decision JSON.
- Validation: Integration test runs hook command, then responds via CLI, hook unblocks and exits 0 with correct output.

#### FUGUE-906 — CLI hook: `hook Stop` / idle notification
- Outcome: Hook notifies daemon when agent becomes idle so orchestrator can resume/reprompt if needed.
- Validation: Integration test triggers stop hook and verifies daemon state updated.

#### FUGUE-907 — Daemon: AskUserQuestion queue + `question.request/respond`
- Outcome: Pending question store (correlation id, agent id, prompt) + IPC handlers + streaming events.
- Validation: Integration test creates a synthetic question request, lists pending, responds, and verifies the agent message routing is invoked (via dummy/fake agent backend).

#### FUGUE-908 — CLI: `question list/respond`
- Outcome: Question response path via CLI.
- Validation: Integration tests for list/respond against the daemon queue.

#### FUGUE-909 — Optional: LLM-based permission decider (`permissions-checker = llm`)
- Outcome: A `PermissionsDecider` port with an implementation that calls a configured LLM to decide allow/deny (manual remains default).
- Validation: Unit tests for prompt construction and response parsing; integration tests with a mock HTTP server.

## Sprint 12 — Planner + Manager + Webhooks + End-to-End Docs

**Goal**: Planner/manager modes + webhook trigger path + complete documentation and a repeatable smoke demo.

**Demo**:
- `fugue plan start --project myproj` writes `~/.fugue/plans/<id>.md`
- `fugue manager start --project myproj`
- (Optional) GitHub webhook POST triggers orchestrator tick.

### Tickets

#### FUGUE-1101 — Planner role: plan state + plan file storage
- Outcome: Planner agent writes a plan to `~/.fugue/plans/<id>.md` and is visible in UI/CLI.
- Validation: Integration test asserts plan file created with expected header.

#### FUGUE-1102 — `plan.*` IPC + CLI (`start/stop/list/show/send_message`)
- Outcome: CLI controls planner agents and retrieves their chat history.
- Validation: Integration tests using dummy agent backend.

#### FUGUE-1103 — Manager role: restricted-capability agent backend configuration
- Outcome: Manager agent runs with a restricted command/tool allowlist (Claude-focused).
- Validation: Unit tests for allowlist generation; manual validation with a real agent is documented.

#### FUGUE-1104 — Webhook server skeleton + config
- Outcome: Optional HTTP server that can run alongside daemon (off by default).
- Validation: Integration test starts server on ephemeral port and returns 200 on `/health`.

#### FUGUE-1105 — Webhook dedup store (`runtime/dedup.json`)
- Outcome: Simple dedup keyed by delivery id; persisted best-effort.
- Validation: Unit tests for dedup behavior; integration test for persistence.

#### FUGUE-1106 — GitHub webhook: issue event triggers orchestrator tick
- Outcome: Receiving a configured webhook causes orchestrator to poll immediately (still also polls on interval).
- Validation: Integration test posts a fake webhook payload and asserts an orchestrator “tick requested” event.

#### FUGUE-1107 — Docs: getting started + backend setup (tk/GitHub/Linear)
- Outcome: `docs/GETTING_STARTED.md` covering:
  - daemon/CLI usage
  - tk ticket format and folder placement
  - GitHub token requirements
  - Linear API key requirements (`[providers.linear].api-key` or `LINEAR_API_KEY`)
- Validation: Manual doc review; command examples match CLI.

#### FUGUE-1108 — End-to-end smoke script
- Outcome: `scripts/smoke.sh` (or `justfile`) that:
  - starts daemon in temp `FUGUE_DIR`
  - adds a local project
  - creates a tk ticket
  - starts orchestration with dummy backend
  - verifies agent spawned and done path works.
- Validation: Run script in CI or locally; exits 0 on success.

#### FUGUE-1109 — Linear webhook: issue/comment event triggers orchestrator tick
- Outcome: Receiving a configured Linear webhook causes orchestrator to poll immediately (still also polls on interval).
- Validation: Integration test posts a fake Linear webhook payload with a valid signature and asserts an orchestrator “tick requested” event.

---

## Sprint 13 — TUI v1 Foundations

**Goal**: Reintroduce `fugue tui` and establish a single-screen TUI foundation: a modal state machine and a dedicated `attach` stream connection.

**Reference**: a single view with:
- Header (branding + counts + connection state)
- Left pane: agent list + recent work
- Right pane: chat view (+ overlays for approvals/questions)
- Footer: context-sensitive help bar

**Demo**:
- Terminal A: `fugue server start --foreground`
- Terminal B: `fugue tui` (alt-screen) shows header + panes, fetches agent list, `q` quits cleanly.

### Tickets

#### FUGUE-1201 — CLI: reintroduce `fugue tui` (alt-screen) + file logging
- Outcome: `fugue tui` launches the interactive TUI; logs go to a file (no stderr log spam that corrupts the screen).
- Validation: Integration test updates `--help` assertions; manual run verifies clean enter/exit and log file created.

#### FUGUE-1202 — TUI core: `Model` + message types + pure reducer skeleton
- Outcome: `Model` holds window size, focus, mode, connection status, selected agent, and cached lists; `update(model, msg) -> (model, effects)` is pure.
- Validation: Unit tests cover mode/focus transitions and “no invalid combinations”.

#### FUGUE-1203 — TUI loop: ratatui draw loop + crossterm input pump
- Outcome: Imperative shell runs: (1) poll input, (2) drain daemon events, (3) run effects, (4) draw frame; handles resize; `Ctrl+C` exits.
- Validation: View tests render an empty state with `TestBackend`; manual run.

#### FUGUE-1204 — TUI client trait: daemon RPC adapter + mockable interface
- Outcome: A `TuiClient` trait covers the Sprint 13 needs (`attach` stream + `agent.list`) and is extended in later sprints as UI capabilities grow.
- Validation: Integration tests already cover daemon attach + agent list; TUI view/reducer tests run without a daemon.

#### FUGUE-1205 — Streaming: dedicated `attach` connection + reconnection state
- Outcome: TUI attaches on startup, converts incoming events into `Msg`s; errors transition to `Disconnected` and show in header/footer.
- Validation: Unit tests for connection-state transitions; manual test by restarting daemon.

#### FUGUE-1206 — Keymap + help bar
- Outcome: Implement the Sprint 13 keybindings (`q`, `Tab`, `j/k`, `g/G`, `Enter`, `Esc`, `r`) and render a context-sensitive footer; add paging/approval/abort/plan bindings as those features land in later sprints.
- Validation: Unit tests for key→action mapping and core transitions; view tests for footer content.

---

## Sprint 14 — TUI v1 Agent UX (Agent List + Chat + Input + Recent Work)

**Goal**: A usable interactive monitor: browse agents, view chat, scroll, and send messages; show recent work and commit count.

**Demo**:
- Start a dummy agent and send it a message.
- `fugue tui` shows the agent, streams chat, and lets you send a reply from the TUI.

### Tickets

#### FUGUE-1301 — TUI: agent list rendering (selection + focus + state icons)
- Outcome: Agent rows show state icon/spinner, agent id, project, backend, and duration; selected row highlights; focus border changes.
- Validation: View tests render a fixed agent list snapshot; unit tests for selection bounds.

#### FUGUE-1302 — TUI: chat view rendering + scroll model
- Outcome: Chat view renders entries with role badges and wraps content; supports scroll up/down and page up/down; “follow tail” behavior when near bottom.
- Validation: Unit tests for follow/scroll math; view tests for wrapped rendering.

#### FUGUE-1303 — TUI: fetch chat history on selection + merge with streaming entries
- Outcome: On agent selection change, fetch history and merge with any newer streaming entries (prevents lost messages when switching agents).
- Validation: Pure unit tests for merge behavior with fixture timestamps/sequence ids.

#### FUGUE-1304 — TUI: input line (multiline, history, modes)
- Outcome: `Enter` enters input mode; `Enter` sends; `Shift+Enter` inserts newline; `Esc` cancels; up/down navigates input history.
- Validation: Unit tests for editor/history behavior; view tests for input box height changes.

#### FUGUE-1305 — TUI: send message (optimistic append + error surface)
- Outcome: Messages sent from the input line are appended immediately; send errors show in the help bar and do not corrupt state.
- Validation: Reducer tests for optimistic append + rollback/marking; manual E2E with daemon + dummy backend.

#### FUGUE-1306 — TUI: recent work + header stats (commit count)
- Outcome: Recent commits render in the left pane; header shows running/total agents and commit count; refreshes periodically.
- Validation: View tests for truncation and empty states; integration test exercises `commit.list` and `stats`.

---

## Sprint 14.5 — Hook Reliability (Claude Code)

**Goal**: No more hook-related tool failures during development or long-running daemon sessions. Claude Code hooks (`PreToolUse`, `Stop`, and legacy `PermissionRequest`) should always invoke Fugue successfully and never emit shell syntax errors due to an invalid executable path.

**Background**: Fugue currently builds Claude hook commands from `std::env::current_exe()` and embeds them into Claude’s `--settings` JSON. On Linux, if the running binary is replaced/unlinked during a rebuild, `/proc/self/exe` is reported as `"... (deleted)"`. Because the hook command is executed via a shell, the resulting unquoted command string breaks parsing and hooks fail, blocking tool use.

**Demo**:
- Start the daemon, create a Claude-backed agent, and run a tool from within the agent (no hook errors).
- While the daemon is running, rebuild/replace the fugue binary, then create another Claude-backed agent and run a tool again (hooks still work; no `(... (deleted))` in hook command).

### Tickets

#### FUGUE-1350 — Centralize hook executable resolution (pure helper)
- Outcome: A small, unit-tested helper resolves the hook executable “prefix” used in hook commands (e.g. `"/abs/path/to/fugue"`, or fallback `"fugue"`).
- Requirements:
  - Strip Linux ` (deleted)` suffix when present.
  - Support an override env var (e.g. `FUGUE_HOOK_EXE`) for pinned installs.
  - Never emit an empty string; always return something runnable.
- Validation: Unit tests covering `(deleted)` stripping, env override, and non-UTF8/empty fallback behavior.

#### FUGUE-1351 — Shell-safe hook command rendering
- Outcome: Hook commands are rendered in a shell-safe form (path with spaces/parentheses cannot break parsing).
- Validation: Unit test builds a temp executable with spaces in its path and confirms the rendered command runs via `sh -c`.

#### FUGUE-1352 — Add `PermissionRequest` hook compatibility
- Outcome: `fugue hook PermissionRequest` exists (aliases to `PreToolUse`) and Claude settings include it alongside `PreToolUse` and `Stop`.
- Validation: CLI help test updated; unit/integration test asserts settings JSON contains all three hooks.

#### FUGUE-1353 — Integration test: daemon spawns Claude with correct `--settings`
- Outcome: A fake `claude` binary captures the `--settings` JSON and asserts:
  - Hook commands do not contain ` (deleted)`.
  - Hook command strings are shell-safe (or at minimum include expected quoting/format).
  - Hooks include `PreToolUse`, `PermissionRequest`, and `Stop` with expected timeouts.
- Validation: New `crates/fugue/tests/...` integration test.

#### FUGUE-1354 — Docs: hook troubleshooting + dev workflow
- Outcome: Document what hooks are used for, how they’re injected, and how to recover when hook execution fails (including when a daemon is running an unlinked binary).
- Validation: Manual doc walkthrough; examples match current CLI (`fugue hook ...`).

---

## Sprint 15 — TUI v1 Interventions (Approvals + Questions + Planner/Manager)

**Goal**: Full interaction loops inside the TUI: approve/deny tool permissions, answer AskUserQuestion prompts, start a planner, and manage abort flows.

**Demo**:
- Trigger a permission request; approve/deny from the TUI (`y/n`).
- Trigger an AskUserQuestion request; select an answer (`j/k`, `y`), including “Other” freeform.
- Start a planner from the TUI (`p`) and watch it appear in the agent list.

### Tickets

#### FUGUE-1401 — TUI: pending permissions overlay + `y/n` respond
- Outcome: Permission requests arrive via stream events, mark the agent with an attention indicator, render a pending-permission overlay in chat, and respond via daemon RPC.
- Validation: Integration test uses the daemon permission queue; reducer tests cover attention flagging and “selected agent only” overlay logic.

#### FUGUE-1402 — TUI: AskUserQuestion overlay + option selection + “Other”
- Outcome: Questions render with selectable options; `j/k` navigates; `y` submits; “Other” enters input mode to capture freeform text and submits it.
- Validation: Reducer tests for option navigation and submission payloads; integration test uses the daemon question queue.

#### FUGUE-1403 — TUI: abort flow (`x` → confirm `y/n`)
- Outcome: `x` enters confirm mode; `y` aborts selected agent (or stops a planner); `n` cancels; UI updates on state events.
- Validation: Unit tests for mode transitions; integration test asserts `agent.abort` / `plan.stop` invoked.

#### FUGUE-1404 — TUI: planner start (`p`) with project selection + prompt
- Outcome: `p` opens a project picker, then a prompt entry mode; starting a planner causes it to appear in the agent list (prefixed, like `plan:<id>`).
- Validation: Reducer tests for picker navigation; integration test asserts `plan.start` creates a planner and it can be selected and chatted with.

#### FUGUE-1405 — TUI: manager agent display + chat integration
- Outcome: When a manager is running, it appears as a special agent entry and supports chat-history + send-message.
- Validation: Manual E2E checklist using `fugue manager start`; view tests for special row rendering.

#### FUGUE-1406 — TUI: reconnect policy (auto backoff + manual `r`)
- Outcome: On stream disconnect, TUI shows “disconnected” and retries with exponential backoff up to a limit; `r` forces an immediate retry; on reconnect, refetch list + selected history.
- Validation: Unit tests for backoff schedule bounds; manual test by restarting daemon mid-session.

#### FUGUE-1407 — Docs: re-add `docs/TUI.md`
- Outcome: Document purpose, key bindings, modes, reconnection behavior, and the relationship between TUI and daemon approvals.
- Validation: Manual doc walkthrough; command examples match CLI.

---

## Sprint 16 — CLI Surface Parity

**Goal**: Make Fugue’s *visible* CLI surface match the intended public command tree and semantics while keeping internal RPC and modules intact. Internal-only entrypoints (hooks, permissions/questions, low-level debug commands) should remain available but **not shown** in `fugue --help`.

**Demo**:
- `fugue --help` shows the top-level commands: `agent`, `attach`, `branch`, `claims`, `completion`, `issue`, `manager`, `plan`, `project`, `server`, `status`, `tui`, `version`.
- `fugue completion zsh` prints a working completion script.
- `fugue project start myproj` / `fugue project stop myproj` controls orchestration (and `--all` works).

### Tickets

#### FUGUE-1501 — CLI: add `completion` command (bash/fish/powershell/zsh)
- Outcome: `fugue completion <shell>` generates completions for the supported shells.
- Validation: CLI integration test asserts `fugue completion zsh` outputs non-empty script containing `fugue` and subcommand tokens.

#### FUGUE-1502 — CLI: add `project start/stop` as orchestration control
- Outcome: New `fugue project start [project] --all` and `fugue project stop [project] --all` map to the existing orchestrator start/stop implementation.
- Validation: Integration test starts daemon in temp `FUGUE_DIR`, registers 2 projects, runs `project start --all`, then `project stop --all`, and asserts orchestrator status toggles.

#### FUGUE-1503 — CLI: hide `orchestration` (keep as compat alias)
- Outcome: `fugue orchestration ...` remains supported for backwards compatibility but is hidden from `--help`; docs and examples switch to `project start/stop`.
- Validation: Help snapshot test asserts `orchestration` is not present in `fugue --help`; direct invocation still works.

#### FUGUE-1504 — CLI: hide internal/debug commands from root help
- Outcome: Hide from `fugue --help`: `ping`, `stats`, `commit`, `claim`, `hook`, `permission`, `question` (plus any other debug-only commands).
- Validation: Help snapshot test asserts the visible top-level command set matches the intended list.

#### FUGUE-1505 — CLI: add `--fugue-dir` flag
- Outcome: Add a global `--fugue-dir <path>` flag that overrides the base directory, without breaking `FUGUE_DIR` env usage.
- Validation: Integration test uses `--fugue-dir` to isolate a run and verifies config/socket paths resolve under that directory.

#### FUGUE-1506 — CLI: align `server start` flags (`-f/--foreground`)
- Outcome: Support `-f` as a short flag for `server start --foreground`, and ensure help text reflects the semantics (default daemonizes).
- Validation: CLI parse test (or `assert_cmd`) verifies `fugue server start -f --help` shows the option and `server start -f` runs foreground mode.

---

## Sprint 17 — Agent/Manager/Plan CLI Parity

**Goal**: Align end-user command semantics for agent control and plan storage, while keeping additional admin/debug commands available but hidden.

**Demo**:
- `fugue agent done` works with `FUGUE_AGENT_ID` (no positional agent id), supports `--task` and `--error`.
- `fugue agent plan "<prompt>"`, `fugue agent plan list`, `fugue agent plan stop <id>`.
- `fugue plan list/read/write` manage stored plans only (planner runtime control lives under `agent plan`).
- `fugue manager start/stop/status/clear` (chat interaction happens via TUI).

### Tickets

#### FUGUE-1601 — CLI: make `agent done` env-based + flags
- Outcome: `fugue agent done` no longer accepts a positional agent id; it uses `FUGUE_AGENT_ID` and supports `--task` and `--error`.
- Validation: Integration test sets env var, invokes `agent done --task ...`, and asserts daemon processes completion pipeline as expected (dummy backend acceptable).

#### FUGUE-1602 — CLI: clarify `agent describe` help + error messages
- Outcome: Help text and error messages explicitly reference `FUGUE_AGENT_ID`.
- Validation: `assert_cmd` tests for help output and for the “env var not set” error.

#### FUGUE-1603 — CLI: align `agent abort` UX (`--force`, `--yes`)
- Outcome: Add `--force` to kill immediately and `--yes` to skip confirmation.
- Validation: Integration test uses a dummy agent process, runs `agent abort --yes`, and asserts agent is removed; unit test covers flag parsing and confirmation gating.

#### FUGUE-1604 — CLI: hide admin/debug agent subcommands
- Outcome: Hide `agent create/delete/send-message/tail/chat-history` from help; keep them available for internal use (tests/TUI) either as hidden subcommands or moved under `__internal`.
- Validation: Help snapshot test asserts `fugue agent --help` lists only `list/abort/claim/describe/done/plan` (and `help`).

#### FUGUE-1605 — CLI: split “planner runtime” from “plan storage”
- Outcome:
  - `fugue agent plan ...` remains the only visible interface for planning agents.
  - `fugue plan ...` only manages stored plan files: `list`, `read`, `write`.
  - Keep backwards-compatible hidden aliases for `fugue plan start/stop/...` if needed.
- Validation: CLI help tests + integration test that starts a planner via `agent plan` and writes a stored plan via `plan write`.

#### FUGUE-1606 — CLI: align manager surface (hide chat methods)
- Outcome: Hide `manager send-message/chat-history`; keep `start/stop/status/clear` visible.
- Validation: Help snapshot test + manual validation note (TUI provides interaction).

---

## Sprint 18 — Issue CLI Parity + Project Detection

**Goal**: Make `fugue issue` support: `--project` flag with cwd detection, `show` naming, status filtering, and compatible flags (`--depends-on`, `--parent`, `--commit`).

**Demo**:
- In a project repo/worktree, run `fugue issue create "Title"` (no explicit project) and get an id.
- `fugue issue list --status open` filters correctly.
- `fugue issue show <id>` works (and `get` remains as hidden alias).
- `fugue issue create --parent <id>` creates a child issue by prepending the parent id to dependencies.
- `fugue issue create --commit` commits and pushes immediately (tk only).

### Tickets

#### FUGUE-1701 — Core/shell: detect project from cwd (repo or worktree)
- Outcome: Resolve `--project` default by mapping the current directory to a registered project (repo clone path or known worktree prefix).
- Validation: Unit tests using `tempfile` with synthetic repo/worktree layouts and config.

#### FUGUE-1702 — CLI: make `issue show` primary (keep `get` hidden alias)
- Outcome: Rename visible command to `issue show`; keep `issue get` as hidden alias for backwards compatibility.
- Validation: Help snapshot test verifies `show` is present and `get` is hidden; integration test verifies both work.

#### FUGUE-1703 — CLI: add `issue list --status` filter
- Outcome: `issue list` supports `--status open|closed|blocked`.
- Validation: Unit tests for filter mapping; integration test with tk issues covering all statuses.

#### FUGUE-1704 — CLI: align `issue create` flags (`--commit`, `--depends-on`, `--parent`, `--type`, `--priority`)
- Outcome:
  - Signature becomes `issue create <title>` with `--project` optional.
  - `--depends-on` accepts comma-separated values.
  - `--parent` prepends parent to deps and verifies parent exists (for tk; best-effort for remote backends).
  - `--priority` uses a 0/1/2 convention with default 1; `--type` defaults to `"task"`.
  - `--commit` optionally runs `issue commit` automatically (tk only).
- Validation: Integration test for tk backend verifying deps/parent/priority defaults and commit behavior.

#### FUGUE-1705 — CLI: align `issue update`/`close`/`comment` to use `--project` default
- Outcome: All issue commands accept id without explicit project when cwd detection succeeds; flags align (`--status`, `--priority`, `--title`).
- Validation: Integration test runs commands from inside a worktree and asserts correct project inference.

#### FUGUE-1706 — CLI: align `issue commit` semantics
- Outcome: `issue commit` uses `--project` default detection and has no required args; stages/commits/pushes tk ticket changes.
- Validation: Integration test with local bare remote asserts a commit and push occurred.

---

## Sprint 19 — Remaining Behavioral Parity

**Goal**: Close the remaining feature gaps that are currently reserved/partial in Fugue.

**Demo**:
- `merge-strategy = "pull-request"` creates a PR (GitHub only) instead of merging directly when a coding agent completes.
- `permissions-checker = "llm"` auto-decides tool permissions (with fallback to manual/TUI on “unsure” or errors).

### Tickets

#### FUGUE-1801 — Merge strategy: implement `pull-request` (GitHub)
- Outcome: On `agent done` with `merge-strategy = pull-request`, Fugue pushes the branch, creates a PR, records PR URL, and keeps the worktree around for follow-ups.
- Validation: Integration tests with `wiremock` for GitHub GraphQL PR creation; local git repo assertions for push behavior.

#### FUGUE-1802 — Permissions: implement `permissions-checker = llm` (`[llm_auth]`)
- Outcome: Implement LLM authorizer behavior using `[llm_auth]` config (Anthropic/OpenAI), returning `allow|deny|unsure` and falling back to manual approval on `unsure`.
- Validation: Unit tests for prompt construction + response parsing; `wiremock` integration tests for provider calls.

#### FUGUE-1803 — CLI/docs: make hooks internal-but-supported
- Outcome: `fugue hook` remains supported for Claude integration but is hidden from root help; docs clarify it’s an internal integration surface.
- Validation: Help snapshot test confirms hidden; integration test confirms hooks still run end-to-end.
