# Getting Started

Fugue is a local-only agent orchestration supervisor (daemon + CLI).

For a full guide, see:
- `docs/USAGE.md`
- `docs/CLI.md`

## Prereqs

- Rust toolchain (`cargo`)
- `git`
- One (or both) agent CLIs on your `PATH`:
  - `claude` (Claude Code)
  - `codex` (Codex CLI)

## Quickstart

Terminal A (daemon):

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- server start --foreground`

Terminal B (CLI):

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- agent list`

Add a project (clones into `$FUGUE_DIR/projects/<name>/repo/`):

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project add <path-or-url> [--name myproj]`

Start orchestration:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project start myproj`

Optional: stream daemon events:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- attach`

## Issue Backends

### Local tickets (`tk`)

Tickets live in the project repo clone under `.fugue/tickets/`.

- Create: `FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- issue create --project myproj "Title"`
- Ready list: `FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- issue ready --project myproj`

Format details: `docs/TICKETS.md`.

### GitHub Issues

Requirements:

- The project repo `origin` must be a GitHub remote (for `owner/repo` detection).
- Token via env or config:
  - `GITHUB_TOKEN=...` (or `GH_TOKEN=...`)
  - `[providers.github].token = "..."` (or `api-key`)

Switch backend:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project config set myproj issue-backend github`

### Linear Issues

Requirements:

- API key via env or config:
  - `LINEAR_API_KEY=...`
  - `[providers.linear].api-key = "..."`
- A team id on the project:
  - `FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project config set myproj linear-team <team-uuid>`

Switch backend:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project config set myproj issue-backend linear`

## Planner & Manager

Start a planner (creates `plans/<id>.md`):

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- agent plan --project myproj "Plan the next sprint"`

Show the plan file:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- plan read plan-1`

Start a project manager agent:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- manager start myproj`

## Webhooks (Optional)

Enable in `config.toml` (defaults shown):

```toml
[webhook]
enabled = true
bind-addr = "127.0.0.1:8080"
path-prefix = "/webhooks"
secret = "your-shared-secret"
```

Endpoints:

- `GET /health`
- `POST /webhooks/github?project=<project>`
  - validates `X-Hub-Signature-256` if `secret` is set
- `POST /webhooks/linear?project=<project>`
  - validates `Linear-Signature` if `secret` is set

## Smoke Demo Script

Run from the repo root:

`bash scripts/smoke.sh`
