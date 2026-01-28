# Murmur Documentation

Welcome to the Murmur documentation. Murmur is a local multi-agent orchestrator for AI coding assistants like Claude Code and Codex CLI.

## Quick Navigation

| I want to... | Go to... |
|--------------|----------|
| Get started quickly | [Getting Started](GETTING_STARTED.md) |
| Learn all the features | [Usage Guide](USAGE.md) |
| Look up a command | [CLI Reference](CLI.md) |
| Use the terminal UI | [TUI Guide](TUI.md) |
| Understand the architecture | [Architecture](ARCHITECTURE.md) |
| Contribute to Murmur | [Development](DEVELOPMENT.md) |

## Documentation Structure

### User Guides

- **[Getting Started](GETTING_STARTED.md)** — Install Murmur and run your first agent in 5 minutes
- **[Usage Guide](USAGE.md)** — Complete guide to using Murmur day-to-day
- **[CLI Reference](CLI.md)** — Every command, option, and environment variable
- **[TUI Guide](TUI.md)** — Terminal UI keybindings and workflows

### Reference

- **[Architecture](ARCHITECTURE.md)** — Design principles, component model, and runtime flows
- **[Ticket Format](TICKETS.md)** — The `.murmur/tickets/` file format for the `tk` backend
- **[Development](DEVELOPMENT.md)** — Building, testing, and contributing

### Component Deep Dives

Detailed documentation for each major subsystem:

| Component | What it covers |
|-----------|----------------|
| [Agent Host](components/AGENT_HOST.md) | Host process architecture, daemon survival, host protocol |
| [Agents](components/AGENTS.md) | Agent lifecycle, coding/planner/manager roles, backends, chat history |
| [Orchestration](components/ORCHESTRATION.md) | The per-project spawn loop, claim registry, ready issue selection |
| [Issue Backends](components/ISSUE_BACKENDS.md) | `tk` (local files), GitHub Issues, Linear integration |
| [Permissions & Questions](components/PERMISSIONS_AND_QUESTIONS.md) | Tool approval rules, hooks, manual and LLM-based authorization |
| [Worktrees & Merge](components/WORKTREES_AND_MERGE.md) | Git worktree isolation, branch naming, merge pipeline, branch cleanup |
| [Configuration](components/CONFIG.md) | `config.toml` schema, project settings, provider credentials |
| [Storage](components/STORAGE.md) | On-disk layout, persistence semantics, runtime files |
| [Hooks](components/HOOKS.md) | Claude Code hook integration, command resolution |
| [Planner & Manager](components/PLANNER_AND_MANAGER.md) | Non-coding agent modes for planning and coordination |
| [Daemon](components/DAEMON.md) | Daemon internals, startup sequence, shared state |
| [IPC Protocol](components/IPC.md) | Unix socket protocol, message types, event streaming |
| [Webhooks](components/WEBHOOKS.md) | GitHub and Linear webhook integration |

## Concepts Overview

### Core Concepts

- **Daemon** — The long-running supervisor process (`mm server start`)
- **Project** — A registered git repository that Murmur manages
- **Agent** — A Claude Code or Codex subprocess working on an issue
- **Worktree** — An isolated git worktree where an agent operates
- **Claim** — A lock preventing multiple agents from working on the same issue
- **Orchestrator** — The per-project loop that spawns agents for ready issues

### Agent Types

| Type | Purpose | Created via |
|------|---------|-------------|
| Coding | Implements issues, commits code, closes issues | Orchestrator (automatic) or `mm agent create` |
| Planner | Explores codebase, writes plan artifacts | `mm agent plan` |
| Manager | Interactive coordinator for a project | `mm manager start` |

### Issue Backends

| Backend | Source | Notes |
|---------|--------|-------|
| `tk` | `.murmur/tickets/*.md` files in repo | Default, no external auth needed |
| `github` | GitHub Issues API | Requires `GITHUB_TOKEN` |
| `linear` | Linear API | Requires `LINEAR_API_KEY` and team ID |

## Getting Help

- **Command help**: `mm --help` or `mm <command> --help`
- **Report issues**: [GitHub Issues](https://github.com/anthropics/murmur/issues)
- **Source code**: Check the `crates/` directory for implementation details
