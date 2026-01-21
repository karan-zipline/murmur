# Documentation

This folder is the canonical documentation for Murmur (local-only agent orchestration supervisor).

## Start Here

- `docs/GETTING_STARTED.md` — quickstart
- `docs/USAGE.md` — end-to-end user guide
- `docs/CLI.md` — CLI reference and conventions
- `docs/TUI.md` — terminal UI usage and keybindings
- `docs/ARCHITECTURE.md` — architecture/design (how it works internally)
- `docs/DEVELOPMENT.md` — developer workflows (tests, smoke, local runs)

## Domain Docs

- `docs/TICKETS.md` — `.murmur/tickets/` format (tk backend)
- `docs/SPRINTS.md` — implementation plan / parity tracking

## Component Docs

- `docs/components/DAEMON.md` — daemon runtime, state, shutdown
- `docs/components/IPC.md` — Unix socket JSONL protocol + event stream
- `docs/components/CONFIG.md` — `config.toml` + per-project settings
- `docs/components/STORAGE.md` — on-disk layout + persistence semantics
- `docs/components/ISSUE_BACKENDS.md` — `tk`, GitHub, Linear + `issue plan`
- `docs/components/ORCHESTRATION.md` — orchestrator loop, claims, spawn policy
- `docs/components/AGENTS.md` — agent lifecycle, backends, chat/history/events
- `docs/components/WORKTREES_AND_MERGE.md` — git worktrees, branches, merge pipeline, branch cleanup
- `docs/components/PERMISSIONS_AND_QUESTIONS.md` — Claude hooks, rules, approvals, AskUserQuestion
- `docs/components/HOOKS.md` — Claude hook injection, command resolution, troubleshooting
- `docs/components/PLANNER_AND_MANAGER.md` — planner agents + stored plans, manager agent
- `docs/components/WEBHOOKS.md` — webhook server, signatures, dedup, tick requests
