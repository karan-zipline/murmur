# Getting Started

Murmur is a local-only agent orchestration supervisor (daemon + CLI).

For a full guide, see:
- `docs/USAGE.md`
- `docs/CLI.md`

## Prereqs

- Rust toolchain (`cargo`)
- `git`
- One (or both) agent CLIs on your `PATH`:
  - `claude` (Claude Code)
  - `codex` (Codex CLI)

## Install from Source (Recommended)

From the repo root:

```bash
cargo install --locked --path crates/murmur
```

This installs `mm` into Cargo’s bin dir (usually `~/.cargo/bin`).

Verify:

```bash
command -v mm
mm version
```

Upgrade after pulling new commits:

```bash
cargo install --locked --path crates/murmur --force
```

## Quickstart (2 Terminals)

Terminal A (daemon):

```bash
mm server start --foreground
```

Terminal B (CLI):

```bash
mm agent list
```

Add a project (clones into the base directory under `~/.murmur/projects/<name>/repo/` by default):

```bash
mm project add <path-or-url> --name myproj
```

Start orchestration:

```bash
mm project start myproj
```

Optional: stream daemon events:

```bash
mm attach
```

## Using a Custom Base Directory (Optional)

By default, Murmur stores its local state under `~/.murmur` and config under `~/.config/murmur`.

If you want an isolated environment for testing, set `MURMUR_DIR` (or pass `--murmur-dir`):

```bash
export MURMUR_DIR=/tmp/murmur-dev
mm server start --foreground
```

Or:

```bash
mm --murmur-dir /tmp/murmur-dev server start --foreground
```

## Issue Backends

### Local tickets (`tk`)

Tickets live in the project repo clone under `.murmur/tickets/`.

- Create: `mm issue create --project myproj "Title"`
- Ready list: `mm issue ready --project myproj`

Format details: `docs/TICKETS.md`.

### GitHub Issues

Requirements:

- The project repo `origin` must be a GitHub remote (for `owner/repo` detection).
- Token via env or config:
  - `GITHUB_TOKEN=...` (or `GH_TOKEN=...`)
  - `[providers.github].token = "..."` (or `api-key`)

Switch backend:

```bash
mm project config set myproj issue-backend github
```

### Linear Issues

Requirements:

- API key via env or config:
  - `LINEAR_API_KEY=...`
  - `[providers.linear].api-key = "..."`
- A team id on the project:
  - `mm project config set myproj linear-team <team-uuid>`

Switch backend:

```bash
mm project config set myproj issue-backend linear
```

## Planner & Manager

Start a planner (creates `plans/<id>.md`):

```bash
mm agent plan --project myproj "Plan the next sprint"
```

Show the plan file:

```bash
mm plan read plan-1
```

Start a project manager agent:

```bash
mm manager start myproj
```

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
