# Using Murmur

Murmur is a local-only agent orchestration supervisor (daemon + CLI).

At a high level:
- You run the daemon (`mm server start`).
- You register one or more projects (`mm project add ...`).
- You choose an issue backend per project (`tk`, GitHub, Linear).
- You start orchestration (`mm project start ...`).
- Murmur spawns agents in git worktrees, merges finished work, and closes issues.
- You monitor/interact via the CLI, including approvals and user questions.

For internal design, see `docs/ARCHITECTURE.md`.

---

## Prerequisites

- Rust toolchain (`cargo`)
- `git`
- At least one agent CLI available on `PATH`:
  - `claude` (Claude Code) and/or
  - `codex` (Codex CLI)

Optional (depending on backends):
- GitHub token (`GITHUB_TOKEN` or `GH_TOKEN`)
- Linear API key (`LINEAR_API_KEY`)

---

## Install / Build

From the repo root:

- Install (recommended):

  ```bash
  cargo install --locked --path crates/murmur
  ```

  Verify:

  ```bash
  mm version
  ```

- If you're hacking on Murmur locally and prefer not to install it, replace `mm ...` with:

  ```bash
  cargo run -p murmur --bin mm -- <args...>
  ```

- Build (no install):

  ```bash
  cargo build --workspace
  ./target/debug/mm --help
  ```

---

## Runtime Directories

Murmur is local-only and stores everything under a base directory.

- Default base: `~/.murmur` (runtime state)
- Default config: `~/.config/murmur` (config + permissions)

If you set `MURMUR_DIR` (or pass `--murmur-dir`), Murmur keeps *both* runtime files and config under that directory:
- `<MURMUR_DIR>/config/config.toml`
- `<MURMUR_DIR>/config/permissions.toml`
- `<MURMUR_DIR>/murmur.sock`, `<MURMUR_DIR>/murmur.log`, `<MURMUR_DIR>/projects/`, `<MURMUR_DIR>/plans/`, `<MURMUR_DIR>/runtime/`

Details: `docs/components/STORAGE.md`.

---

## Using a Custom Base Directory (Optional)

The default base directory (`~/.murmur`) is fine for normal usage.

If you want an isolated environment (recommended for tests, demos, and CI):

```bash
export MURMUR_DIR=/tmp/murmur-dev
mm server start --foreground
```

Or pass a one-off override:

```bash
mm --murmur-dir /tmp/murmur-dev server start --foreground
```

---

## Start / Stop the Daemon

Foreground daemon (good for development):

`mm server start --foreground`

Background start:

`mm server start`

Status / ping:

- `mm server status`

Stop:

`mm server stop`

Restart:

`mm server restart`

---

## Add a Project

Murmur registers projects by cloning a remote URL into the base directory.

Recommended (path-or-url; name inferred unless overridden):

`mm project add <path-or-url> [--name myproj]`

Legacy-compatible form (explicit name + remote URL):

`mm project add myproj --remote-url <git-url>`

List projects:

`mm project list`

Inspect project config:

`mm project config show myproj`

Project health checks (remote origin match, repo exists, orchestration running):

`mm project status myproj`

Remove a project (unregisters; repo remains on disk):

- Keep worktrees: `mm project remove myproj`
- Delete worktrees: `mm project remove myproj --delete-worktrees`

Component details:
- `docs/components/CONFIG.md`
- `docs/components/WORKTREES_AND_MERGE.md`

---

## Choose an Issue Backend

Each project has an `issue-backend` setting.

### `tk` (local tickets)

Tickets live inside the project repo clone, under `.murmur/tickets/`.

- Create: `mm issue create --project myproj "Title"`
- List: `mm issue list --project myproj`
- Ready: `mm issue ready --project myproj`

Ticket format: `docs/TICKETS.md`.

### GitHub Issues

Requirements:
- `origin` remote for the project repo must be a GitHub URL (owner/repo detection).
- Token via env or config:
  - `GITHUB_TOKEN=...` (or `GH_TOKEN=...`)
  - `[providers.github].token = "..."` (or `api-key`)

Enable:

`mm project config set myproj issue-backend github`

### Linear Issues

Requirements:
- API key via env or config:
  - `LINEAR_API_KEY=...`
  - `[providers.linear].api-key = "..."`
- A team id (UUID) on the project:
  - `mm project config set myproj linear-team <team-uuid>`

Enable:

`mm project config set myproj issue-backend linear`

Backend details: `docs/components/ISSUE_BACKENDS.md`.

---

## Start Orchestration

Start orchestration for a project:

`mm project start myproj`

Stop:

`mm project stop myproj`

All projects:

- Start: `mm project start --all`
- Stop: `mm project stop --all`

Claims (what issue is assigned to which agent):

`mm claims --project myproj`

Orchestration details: `docs/components/ORCHESTRATION.md`.

---

## Work With Agents

List running agents:

`mm agent list`

Filter by project:

`mm agent list --project myproj`

Abort:

- `mm agent abort <agent-id>` (prompts)
- `mm agent abort --yes <agent-id>` (no prompt)

Mark done (used by agents; can also be invoked manually):

`MURMUR_AGENT_ID=<agent-id> mm agent done`

Agent internals: `docs/components/AGENTS.md`.

---

## Planner Agents + Stored Plans

Planner agents are “plan mode” agents that produce markdown artifacts under `plans/`.

Start a planner:

`mm agent plan --project myproj "Plan the next sprint"`

List running planners:

`mm agent plan list --project myproj`

Stop a planner:

`mm agent plan stop plan-1`

List stored plan files:

`mm plan list`

Show stored plan contents:

`mm plan read plan-1`

`plan write` is intended to be called by the planning agent (stdin → file). It uses `MURMUR_AGENT_ID` as the plan id:

`cat myplan.md | MURMUR_AGENT_ID=plan:plan-1 mm plan write`

Planner details: `docs/components/PLANNER_AND_MANAGER.md`.

---

## Manager Agent

Manager agents are project-scoped “interactive” agents (e.g., to ask questions about a codebase).

Start:

`mm manager start myproj`

Status:

`mm manager status myproj`

Send message:
Interact with the manager via the TUI: `mm tui`

Stop:

`mm manager stop myproj`

Clear manager state:

`mm manager clear myproj`

---

## Approvals (Permissions) + User Questions

Claude Code agents can request permission to run tools via the `PreToolUse` hook.
Murmur can also surface `AskUserQuestion` prompts as “questions”.

Primary UI: use the TUI to review/respond:

`mm tui`

Hidden CLI fallback (not shown in `--help`):

List pending permission requests:

`mm permission list`

Respond:

- Allow: `mm permission respond <request-id> allow`
- Deny: `mm permission respond <request-id> deny`

List pending user questions:

`mm question list`

Respond (answers must be a JSON object of question-key to response text):

`mm question respond <request-id> '{"q1":"answer"}'`

Permissions internals: `docs/components/PERMISSIONS_AND_QUESTIONS.md`.

---

## Attach (Live Stream)

Stream live events to stdout (Ctrl-C detaches):

- All projects: `mm attach`
- Filtered: `mm attach myproj`

Protocol: `docs/components/IPC.md`.

---

## Branch Cleanup

Delete merged `murmur/*` agent branches (remote by default; add `--local` for local refs):

- Dry run: `mm branch cleanup --dry-run`
- Delete: `mm branch cleanup`

Details: `docs/components/WORKTREES_AND_MERGE.md`.

---

## Webhooks (Optional)

If enabled, Murmur can receive GitHub/Linear webhooks and request orchestration ticks.

Enable (example):

```toml
[webhook]
enabled = true
bind-addr = "127.0.0.1:8080"
path-prefix = "/webhooks"
secret = "your-shared-secret"
```

Details: `docs/components/WEBHOOKS.md`.
