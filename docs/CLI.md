# CLI Guide

This document describes Murmur’s CLI surface area and conventions.

Murmur is a daemon-first tool: most commands talk to a running daemon over a Unix socket.

If you’re looking for end-to-end usage, start at `docs/USAGE.md`.

---

## Conventions

### Base directory (`MURMUR_DIR`)

By default, Murmur stores runtime state under `~/.murmur` and config under `~/.config/murmur`.

If you want an isolated environment (tests/demos/CI), set `MURMUR_DIR`:

`MURMUR_DIR=/tmp/murmur-dev mm <command...>`

You can also use the `--murmur-dir` flag (equivalent to setting `MURMUR_DIR`):

`mm --murmur-dir /tmp/murmur-dev <command...>`

If you’re running from a working tree without installing, use:

`cargo run -p murmur --bin mm -- <command...>`

### Output formats

Murmur intentionally uses simple, script-friendly output:
- Many `list` commands print tab-separated rows.
- Many “action” commands print `ok` on success.
- IDs are printed as a single line where possible (e.g., `issue create`, `agent create`).

### Environment variables

Common:
- `MURMUR_DIR` — override base directory (socket/logs/projects/runtime)
- `MURMUR_LOG` — log filter level (e.g., `info`, `debug`)

Agent integration:
- `MURMUR_AGENT_ID` — used by commands intended to be called by agents:
  - `mm agent claim <issue-id>`
  - `mm agent describe <text>`
  - `mm agent done [--task ...] [--error ...]`
  - `mm plan write` (uses agent id as plan id; strips `plan:` prefix)

Backend auth:
- GitHub: `GITHUB_TOKEN` or `GH_TOKEN` (or config `[providers.github].token`)
- Linear: `LINEAR_API_KEY` (or config `[providers.linear].api-key`)

---

## Command Overview

Top-level groups (run `mm --help` for the full tree):

- `server` — daemon lifecycle (`start`, `stop`, `restart`, `status`)
- `tui` — interactive terminal UI (see `docs/TUI.md`)
- `project` — register/configure projects
- `issue` — list/create/update/close/comment issues
- `project start/stop` — start/stop per-project orchestration
- `agent` — list/abort/plan/claim/describe/done
- `claims` — claim inspection
- `plan` — stored plan files (list/read/write)
- `manager` — per-project manager agent
- `attach` — stream daemon events to stdout
- `branch cleanup` — remove merged `murmur/*` branches
- `completion` — generate shell completion scripts

Hidden/internal (available for compatibility, but not listed in `--help`):
- `orchestration` — orchestration control (use `project start/stop` instead)
- `hook` — hook entrypoints used by `claude`
- `permission` / `question` — approvals and AskUserQuestion helpers
- `ping`, `stats`, `commit`, `claim` — debugging/admin utilities

---

## `server`

Foreground daemon:

`mm server start --foreground`

Background daemon:

`mm server start`

Stop / restart:

- `mm server stop`
- `mm server restart`

Status:

`mm server status`

Notes:
- `server stop` has alias `server shutdown`.
- Background start currently “daemonizes” by spawning `server start --foreground` as a child process.

---

## `project`

Add a project:

Preferred:

`mm project add <path-or-url> [--name myproj] [--max-agents N] [--autostart] [--backend claude|codex]`

Legacy-compatible:

`mm project add myproj --remote-url <git-url> [--max-agents N] [--autostart] [--backend ...]`

List:

`mm project list`

Remove:

- `mm project remove myproj`
- `mm project remove myproj --delete-worktrees`

Config:

- `mm project config show myproj`
- `mm project config get myproj max-agents`
- `mm project config set myproj max-agents 5`

Status checks:

`mm project status myproj`

Start orchestration:

- `mm project start myproj`
- `mm project start --all`

Stop orchestration:

- `mm project stop myproj`
- `mm project stop --all`

---

## `issue`

All `issue` commands accept `--project myproj` (or `-p myproj`). If omitted, Murmur attempts to detect the project from the current working directory (repo or agent worktree).

List:

`mm issue list --project myproj`

Show:

`mm issue show <issue-id> --project myproj`

Ready:

`mm issue ready --project myproj`

Create:

`mm issue create "Title" --project myproj [--description "..."] [--type task] [--priority 1] [--depends-on ISSUE-2] [--parent ISSUE-1] [--commit]`

Update:

`mm issue update <issue-id> --project myproj [--title ...] [--status open|blocked|closed] [--priority ...]`

Close:

`mm issue close <issue-id> --project myproj`

Comment:

`mm issue comment <issue-id> --project myproj --body "comment body"`

Upsert plan section:

`mm issue plan <issue-id> --project myproj --file plan.md`

Commit tickets (`tk` only):

`mm issue commit --project myproj`

Details: `docs/components/ISSUE_BACKENDS.md`.

---

## `agent`

Create a coding agent:

`mm agent create myproj <issue-id> [--backend claude|codex]`

List:

- `mm agent list`
- `mm agent list --project myproj`

Lifecycle:

- `mm agent abort <agent-id> [--force] [--yes]`
- `mm agent done [--task <id>] [--error <text>]` (uses `MURMUR_AGENT_ID`)

Notes:
- `agent create/delete/send-message/chat-history/tail` exist for internal use but are hidden from `--help`.

Agent-to-daemon (used by agent processes):

- `mm agent claim <issue-id>` (requires `MURMUR_AGENT_ID`)
- `mm agent describe <text>` (requires `MURMUR_AGENT_ID`)

Planner alias:

- Start: `mm agent plan --project myproj "prompt..."`
- List: `mm agent plan list --project myproj`
- Stop: `mm agent plan stop plan-1`

---

## `plan` (stored plans)

Stored plans live under `plans/<id>.md` in the base directory.

List stored plans:

`mm plan list`

Show contents:

`mm plan read plan-1`

Write from stdin (uses `MURMUR_AGENT_ID`):

`cat plan.md | MURMUR_AGENT_ID=plan:plan-1 mm plan write`

Running planners are controlled via `agent plan ...` (see above).

---

## `manager`

Start/stop:

- `mm manager start myproj`
- `mm manager stop myproj`

Interaction happens via the TUI.

Status/clear:

- `mm manager status myproj`
- `mm manager clear myproj`

---

## `permission` and `question` (hidden)

These commands exist primarily for debugging/automation and are hidden from `--help`.

Permissions:

- List: `mm permission list`
- Respond: `mm permission respond <request-id> allow|deny`

Questions:

- List: `mm question list`
- Respond: `mm question respond <request-id> '{"q1":"answer"}'`

---

## `attach`

Stream daemon events to stdout until Ctrl-C:

- All projects: `mm attach`
- Filtered: `mm attach myproj otherproj`

---

## `branch cleanup`

Clean up merged agent branches (prefix `murmur/`):

- Dry run: `mm branch cleanup --dry-run`
- Delete remote refs: `mm branch cleanup`
- Also delete local refs: `mm branch cleanup --local`

---

## `hook` (internal)

These entrypoints are invoked by `claude` hooks:

- `mm hook PreToolUse`
- `mm hook PermissionRequest` (legacy alias)
- `mm hook Stop`

You generally do not call these manually.

If you see hook execution issues due to a moved/unlinked daemon binary, set `FUGUE_HOOK_EXE` before starting the daemon. See `docs/components/HOOKS.md`.
