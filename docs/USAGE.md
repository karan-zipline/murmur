# Using Fugue

Fugue is a local-only agent orchestration supervisor (daemon + CLI).

At a high level:
- You run the daemon (`fugue server start`).
- You register one or more projects (`fugue project add ...`).
- You choose an issue backend per project (`tk`, GitHub, Linear).
- You start orchestration (`fugue project start ...`).
- Fugue spawns agents in git worktrees, merges finished work, and closes issues.
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
  cargo install --locked --path crates/fugue
  ```

  Verify:

  ```bash
  fugue version
  ```

- If you're hacking on Fugue locally and prefer not to install it, replace `fugue ...` with:

  ```bash
  cargo run -p fugue -- <args...>
  ```

- Build (no install):

  ```bash
  cargo build --workspace
  ./target/debug/fugue --help
  ```

---

## Runtime Directories

Fugue is local-only and stores everything under a base directory.

- Default base: `~/.fugue` (runtime state)
- Default config: `~/.config/fugue` (config + permissions)

If you set `FUGUE_DIR` (or pass `--fugue-dir`), Fugue keeps *both* runtime files and config under that directory:
- `<FUGUE_DIR>/config/config.toml`
- `<FUGUE_DIR>/config/permissions.toml`
- `<FUGUE_DIR>/fugue.sock`, `<FUGUE_DIR>/fugue.log`, `<FUGUE_DIR>/projects/`, `<FUGUE_DIR>/plans/`, `<FUGUE_DIR>/runtime/`

Details: `docs/components/STORAGE.md`.

---

## Using a Custom Base Directory (Optional)

The default base directory (`~/.fugue`) is fine for normal usage.

If you want an isolated environment (recommended for tests, demos, and CI):

```bash
export FUGUE_DIR=/tmp/fugue-dev
fugue server start --foreground
```

Or pass a one-off override:

```bash
fugue --fugue-dir /tmp/fugue-dev server start --foreground
```

---

## Start / Stop the Daemon

Foreground daemon (good for development):

`fugue server start --foreground`

Background start:

`fugue server start`

Status / ping:

- `fugue server status`

Stop:

`fugue server stop`

Restart:

`fugue server restart`

---

## Add a Project

Fugue registers projects by cloning a remote URL into the base directory.

Recommended (path-or-url; name inferred unless overridden):

`fugue project add <path-or-url> [--name myproj]`

Legacy-compatible form (explicit name + remote URL):

`fugue project add myproj --remote-url <git-url>`

List projects:

`fugue project list`

Inspect project config:

`fugue project config show myproj`

Project health checks (remote origin match, repo exists, orchestration running):

`fugue project status myproj`

Remove a project (unregisters; repo remains on disk):

- Keep worktrees: `fugue project remove myproj`
- Delete worktrees: `fugue project remove myproj --delete-worktrees`

Component details:
- `docs/components/CONFIG.md`
- `docs/components/WORKTREES_AND_MERGE.md`

---

## Choose an Issue Backend

Each project has an `issue-backend` setting.

### `tk` (local tickets)

Tickets live inside the project repo clone, under `.fugue/tickets/`.

- Create: `fugue issue create --project myproj "Title"`
- List: `fugue issue list --project myproj`
- Ready: `fugue issue ready --project myproj`

Ticket format: `docs/TICKETS.md`.

### GitHub Issues

Requirements:
- `origin` remote for the project repo must be a GitHub URL (owner/repo detection).
- Token via env or config:
  - `GITHUB_TOKEN=...` (or `GH_TOKEN=...`)
  - `[providers.github].token = "..."` (or `api-key`)

Enable:

`fugue project config set myproj issue-backend github`

### Linear Issues

Requirements:
- API key via env or config:
  - `LINEAR_API_KEY=...`
  - `[providers.linear].api-key = "..."`
- A team id (UUID) on the project:
  - `fugue project config set myproj linear-team <team-uuid>`

Enable:

`fugue project config set myproj issue-backend linear`

Backend details: `docs/components/ISSUE_BACKENDS.md`.

---

## Start Orchestration

Start orchestration for a project:

`fugue project start myproj`

Stop:

`fugue project stop myproj`

All projects:

- Start: `fugue project start --all`
- Stop: `fugue project stop --all`

Claims (what issue is assigned to which agent):

`fugue claims --project myproj`

Orchestration details: `docs/components/ORCHESTRATION.md`.

---

## Work With Agents

List running agents:

`fugue agent list`

Filter by project:

`fugue agent list --project myproj`

Abort:

- `fugue agent abort <agent-id>` (prompts)
- `fugue agent abort --yes <agent-id>` (no prompt)

Mark done (used by agents; can also be invoked manually):

`FUGUE_AGENT_ID=<agent-id> fugue agent done`

Agent internals: `docs/components/AGENTS.md`.

---

## Planner Agents + Stored Plans

Planner agents are “plan mode” agents that produce markdown artifacts under `plans/`.

Start a planner:

`fugue agent plan --project myproj "Plan the next sprint"`

List running planners:

`fugue agent plan list --project myproj`

Stop a planner:

`fugue agent plan stop plan-1`

List stored plan files:

`fugue plan list`

Show stored plan contents:

`fugue plan read plan-1`

`plan write` is intended to be called by the planning agent (stdin → file). It uses `FUGUE_AGENT_ID` as the plan id:

`cat myplan.md | FUGUE_AGENT_ID=plan:plan-1 fugue plan write`

Planner details: `docs/components/PLANNER_AND_MANAGER.md`.

---

## Manager Agent

Manager agents are project-scoped “interactive” agents (e.g., to ask questions about a codebase).

Start:

`fugue manager start myproj`

Status:

`fugue manager status myproj`

Send message:
Interact with the manager via the TUI: `fugue tui`

Stop:

`fugue manager stop myproj`

Clear manager state:

`fugue manager clear myproj`

---

## Approvals (Permissions) + User Questions

Claude Code agents can request permission to run tools via the `PreToolUse` hook.
Fugue can also surface `AskUserQuestion` prompts as “questions”.

Primary UI: use the TUI to review/respond:

`fugue tui`

Hidden CLI fallback (not shown in `--help`):

List pending permission requests:

`fugue permission list`

Respond:

- Allow: `fugue permission respond <request-id> allow`
- Deny: `fugue permission respond <request-id> deny`

List pending user questions:

`fugue question list`

Respond (answers must be a JSON object of question-key to response text):

`fugue question respond <request-id> '{"q1":"answer"}'`

Permissions internals: `docs/components/PERMISSIONS_AND_QUESTIONS.md`.

---

## Attach (Live Stream)

Stream live events to stdout (Ctrl-C detaches):

- All projects: `fugue attach`
- Filtered: `fugue attach myproj`

Protocol: `docs/components/IPC.md`.

---

## Branch Cleanup

Delete merged `fugue/*` agent branches (remote by default; add `--local` for local refs):

- Dry run: `fugue branch cleanup --dry-run`
- Delete: `fugue branch cleanup`

Details: `docs/components/WORKTREES_AND_MERGE.md`.

---

## Webhooks (Optional)

If enabled, Fugue can receive GitHub/Linear webhooks and request orchestration ticks.

Enable (example):

```toml
[webhook]
enabled = true
bind-addr = "127.0.0.1:8080"
path-prefix = "/webhooks"
secret = "your-shared-secret"
```

Details: `docs/components/WEBHOOKS.md`.
