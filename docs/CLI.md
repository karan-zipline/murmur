# CLI Guide

This document describes Fugue’s CLI surface area and conventions.

Fugue is a daemon-first tool: most commands talk to a running daemon over a Unix socket.

If you’re looking for end-to-end usage, start at `docs/USAGE.md`.

---

## Conventions

### Base directory (`FUGUE_DIR`)

Most examples use `FUGUE_DIR=/tmp/fugue-dev` so everything is isolated:

`FUGUE_DIR=/tmp/fugue-dev fugue <command...>`

You can also use the `--fugue-dir` flag (equivalent to setting `FUGUE_DIR`):

`fugue --fugue-dir /tmp/fugue-dev <command...>`

If you’re running from a working tree without installing, use:

`cargo run -p fugue -- <command...>`

### Output formats

Fugue intentionally uses simple, script-friendly output:
- Many `list` commands print tab-separated rows.
- Many “action” commands print `ok` on success.
- IDs are printed as a single line where possible (e.g., `issue create`, `agent create`).

### Environment variables

Common:
- `FUGUE_DIR` — override base directory (socket/logs/projects/runtime)
- `FUGUE_LOG` — log filter level (e.g., `info`, `debug`)

Agent integration:
- `FUGUE_AGENT_ID` — used by commands intended to be called by agents:
  - `fugue agent claim <issue-id>`
  - `fugue agent describe <text>`
  - `fugue agent done [--task ...] [--error ...]`
  - `fugue plan write` (uses agent id as plan id; strips `plan:` prefix)

Backend auth:
- GitHub: `GITHUB_TOKEN` or `GH_TOKEN` (or config `[providers.github].token`)
- Linear: `LINEAR_API_KEY` (or config `[providers.linear].api-key`)

---

## Command Overview

Top-level groups (run `fugue --help` for the full tree):

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
- `branch cleanup` — remove merged `fugue/*` branches
- `completion` — generate shell completion scripts

Hidden/internal (available for compatibility, but not listed in `--help`):
- `orchestration` — orchestration control (use `project start/stop` instead)
- `hook` — hook entrypoints used by `claude`
- `permission` / `question` — approvals and AskUserQuestion helpers
- `ping`, `stats`, `commit`, `claim` — debugging/admin utilities

---

## `server`

Foreground daemon:

`fugue server start --foreground`

Background daemon:

`fugue server start`

Stop / restart:

- `fugue server stop`
- `fugue server restart`

Status:

`fugue server status`

Notes:
- `server stop` has alias `server shutdown`.
- Background start currently “daemonizes” by spawning `server start --foreground` as a child process.

---

## `project`

Add a project:

Preferred:

`fugue project add <path-or-url> [--name myproj] [--max-agents N] [--autostart] [--backend claude|codex]`

Legacy-compatible:

`fugue project add myproj --remote-url <git-url> [--max-agents N] [--autostart] [--backend ...]`

List:

`fugue project list`

Remove:

- `fugue project remove myproj`
- `fugue project remove myproj --delete-worktrees`

Config:

- `fugue project config show myproj`
- `fugue project config get myproj max-agents`
- `fugue project config set myproj max-agents 5`

Status checks:

`fugue project status myproj`

Start orchestration:

- `fugue project start myproj`
- `fugue project start --all`

Stop orchestration:

- `fugue project stop myproj`
- `fugue project stop --all`

---

## `issue`

All `issue` commands accept `--project myproj` (or `-p myproj`). If omitted, Fugue attempts to detect the project from the current working directory (repo or agent worktree).

List:

`fugue issue list --project myproj`

Show:

`fugue issue show <issue-id> --project myproj`

Ready:

`fugue issue ready --project myproj`

Create:

`fugue issue create "Title" --project myproj [--description "..."] [--type task] [--priority 1] [--depends-on ISSUE-2] [--parent ISSUE-1] [--commit]`

Update:

`fugue issue update <issue-id> --project myproj [--title ...] [--status open|blocked|closed] [--priority ...]`

Close:

`fugue issue close <issue-id> --project myproj`

Comment:

`fugue issue comment <issue-id> --project myproj --body "comment body"`

Upsert plan section:

`fugue issue plan <issue-id> --project myproj --file plan.md`

Commit tickets (`tk` only):

`fugue issue commit --project myproj`

Details: `docs/components/ISSUE_BACKENDS.md`.

---

## `agent`

Create a coding agent:

`fugue agent create myproj <issue-id> [--backend claude|codex]`

List:

- `fugue agent list`
- `fugue agent list --project myproj`

Lifecycle:

- `fugue agent abort <agent-id> [--force] [--yes]`
- `fugue agent done [--task <id>] [--error <text>]` (uses `FUGUE_AGENT_ID`)

Notes:
- `agent create/delete/send-message/chat-history/tail` exist for internal use but are hidden from `--help`.

Agent-to-daemon (used by agent processes):

- `fugue agent claim <issue-id>` (requires `FUGUE_AGENT_ID`)
- `fugue agent describe <text>` (requires `FUGUE_AGENT_ID`)

Planner alias:

- Start: `fugue agent plan --project myproj "prompt..."`
- List: `fugue agent plan list --project myproj`
- Stop: `fugue agent plan stop plan-1`

---

## `plan` (stored plans)

Stored plans live under `plans/<id>.md` in the base directory.

List stored plans:

`fugue plan list`

Show contents:

`fugue plan read plan-1`

Write from stdin (uses `FUGUE_AGENT_ID`):

`cat plan.md | FUGUE_AGENT_ID=plan:plan-1 fugue plan write`

Running planners are controlled via `agent plan ...` (see above).

---

## `manager`

Start/stop:

- `fugue manager start myproj`
- `fugue manager stop myproj`

Interaction happens via the TUI.

Status/clear:

- `fugue manager status myproj`
- `fugue manager clear myproj`

---

## `permission` and `question` (hidden)

These commands exist primarily for debugging/automation and are hidden from `--help`.

Permissions:

- List: `fugue permission list`
- Respond: `fugue permission respond <request-id> allow|deny`

Questions:

- List: `fugue question list`
- Respond: `fugue question respond <request-id> '{"q1":"answer"}'`

---

## `attach`

Stream daemon events to stdout until Ctrl-C:

- All projects: `fugue attach`
- Filtered: `fugue attach myproj otherproj`

---

## `branch cleanup`

Clean up merged agent branches (prefix `fugue/`):

- Dry run: `fugue branch cleanup --dry-run`
- Delete remote refs: `fugue branch cleanup`
- Also delete local refs: `fugue branch cleanup --local`

---

## `hook` (internal)

These entrypoints are invoked by `claude` hooks:

- `fugue hook PreToolUse`
- `fugue hook PermissionRequest` (legacy alias)
- `fugue hook Stop`

You generally do not call these manually.

If you see hook execution issues due to a moved/unlinked daemon binary, set `FUGUE_HOOK_EXE` before starting the daemon. See `docs/components/HOOKS.md`.
