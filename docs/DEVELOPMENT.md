# Development

Related docs:
- `docs/ARCHITECTURE.md` (design + boundaries)
- `docs/components/` (deep dives)
- `docs/USAGE.md` (how to use Murmur)

## Build & Test

- Build: `cargo build --workspace`
- Test: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt`

## Run (Sprint 3)

Run the foreground daemon:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- server start --foreground`

Ping via IPC (Sprint 2):

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- ping`

Check status (uses ping when the socket exists):

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- server status`

Shutdown via IPC (Sprint 2):

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- server shutdown`

## Project Registry (Sprint 3)

Add a project (clones into `$MURMUR_DIR/projects/<name>/repo/`):

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- project add <path-or-url> [--name myproj]`

List projects:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- project list`

Show a project’s effective config:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- project config show myproj`

Update a config value:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- project config set myproj max-agents 5`

Project status checks:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- project status myproj`

## Agents + Worktrees (Sprint 4)

Create a dummy agent (spawns a worktree under `$MURMUR_DIR/projects/<project>/worktrees/wt-<id>/`):

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- agent create myproj ISSUE-1`

List agents:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- agent list`

Send a message to an agent and view chat history:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- agent send-message a-1 "hello"`

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- agent chat-history a-1 --limit 50`

Abort/delete an agent:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- agent abort a-1`

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- agent delete a-1`

## Issues (`tk`) (Sprint 5)

List issues in `.murmur/tickets/`:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- issue list --project myproj`

Show a single issue:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- issue show issue-abc --project myproj`

List “ready” issues (open issues with no open dependencies):

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- issue ready --project myproj`

Create/update/close:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- issue create --project myproj "Title" --description "..."`

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- issue update issue-abc --project myproj --status blocked --priority 1`

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- issue close issue-abc --project myproj`

Comment and commit:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- issue comment issue-abc --project myproj --body "hello"`

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- issue commit --project myproj`

## Issues (GitHub + Linear) (Sprint 6)

Switch a project’s backend:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- project config set myproj issue-backend github`

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- project config set myproj issue-backend linear`

GitHub auth:

- Env: `GITHUB_TOKEN=...` (or `GH_TOKEN=...`)
- Config: `[providers.github].token = "..."` (or `api-key`)

Linear auth:

- Env: `LINEAR_API_KEY=...`
- Config: `[providers.linear].api-key = "..."`

Linear requires a team id:

`MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- project config set mylinproj linear-team <team-uuid>`

Run `issue ready`:

`MURMUR_DIR=/tmp/murmur-dev GITHUB_TOKEN=... cargo run -p murmur --bin mm -- issue ready --project myghproj`

`MURMUR_DIR=/tmp/murmur-dev LINEAR_API_KEY=... cargo run -p murmur --bin mm -- issue ready --project mylinproj`

## Filesystem Layout

Default base directory is `~/.murmur` (override with `MURMUR_DIR`):

- `murmur.log` — daemon/CLI logs
- `murmur.sock` — daemon IPC socket (Sprint 2)
- `murmur.pid` — daemon pid file (future)

Default config directory is `~/.config/murmur` (overridden to `$MURMUR_DIR/config` when `MURMUR_DIR` is set):

- `config.toml` — global config (Sprint 3)
- `permissions.toml` — global permissions rules (Sprint 10)

## Planner & Manager (Sprint 12)

Planner:

- Start: `MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- agent plan --project myproj "Prompt..."`
- List: `MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- agent plan list --project myproj`
- Show plan file: `MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- plan read plan-1`
- Stop: `MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- agent plan stop plan-1`

Manager:

- Start: `MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- manager start myproj`
- Status: `MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- manager status myproj`
- Clear: `MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- manager clear myproj`
- Stop: `MURMUR_DIR=/tmp/murmur-dev cargo run -p murmur --bin mm -- manager stop myproj`

## Webhooks (Sprint 12)

Enable in `config.toml`:

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
- `POST /webhooks/linear?project=<project>`

## Smoke Script

Run: `bash scripts/smoke.sh`
