# Development

Related docs:
- `docs/ARCHITECTURE.md` (design + boundaries)
- `docs/components/` (deep dives)
- `docs/USAGE.md` (how to use Fugue)

## Build & Test

- Build: `cargo build --workspace`
- Test: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt`

## Run (Sprint 3)

Run the foreground daemon:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- server start --foreground`

Ping via IPC (Sprint 2):

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- ping`

Check status (uses ping when the socket exists):

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- server status`

Shutdown via IPC (Sprint 2):

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- server shutdown`

## Project Registry (Sprint 3)

Add a project (clones into `$FUGUE_DIR/projects/<name>/repo/`):

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project add <path-or-url> [--name myproj]`

List projects:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project list`

Show a project’s effective config:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project config show myproj`

Update a config value:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project config set myproj max-agents 5`

Project status checks:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project status myproj`

## Agents + Worktrees (Sprint 4)

Create a dummy agent (spawns a worktree under `$FUGUE_DIR/projects/<project>/worktrees/wt-<id>/`):

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- agent create myproj ISSUE-1`

List agents:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- agent list`

Send a message to an agent and view chat history:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- agent send-message a-1 "hello"`

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- agent chat-history a-1 --limit 50`

Abort/delete an agent:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- agent abort a-1`

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- agent delete a-1`

## Issues (`tk`) (Sprint 5)

List issues in `.fugue/tickets/`:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- issue list --project myproj`

Show a single issue:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- issue show issue-abc --project myproj`

List “ready” issues (open issues with no open dependencies):

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- issue ready --project myproj`

Create/update/close:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- issue create --project myproj "Title" --description "..."`

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- issue update issue-abc --project myproj --status blocked --priority 1`

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- issue close issue-abc --project myproj`

Comment and commit:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- issue comment issue-abc --project myproj --body "hello"`

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- issue commit --project myproj`

## Issues (GitHub + Linear) (Sprint 6)

Switch a project’s backend:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project config set myproj issue-backend github`

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project config set myproj issue-backend linear`

GitHub auth:

- Env: `GITHUB_TOKEN=...` (or `GH_TOKEN=...`)
- Config: `[providers.github].token = "..."` (or `api-key`)

Linear auth:

- Env: `LINEAR_API_KEY=...`
- Config: `[providers.linear].api-key = "..."`

Linear requires a team id:

`FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- project config set mylinproj linear-team <team-uuid>`

Run `issue ready`:

`FUGUE_DIR=/tmp/fugue-dev GITHUB_TOKEN=... cargo run -p fugue -- issue ready --project myghproj`

`FUGUE_DIR=/tmp/fugue-dev LINEAR_API_KEY=... cargo run -p fugue -- issue ready --project mylinproj`

## Filesystem Layout

Default base directory is `~/.fugue` (override with `FUGUE_DIR`):

- `fugue.log` — daemon/CLI logs
- `fugue.sock` — daemon IPC socket (Sprint 2)
- `fugue.pid` — daemon pid file (future)

Default config directory is `~/.config/fugue` (overridden to `$FUGUE_DIR/config` when `FUGUE_DIR` is set):

- `config.toml` — global config (Sprint 3)
- `permissions.toml` — global permissions rules (Sprint 10)

## Planner & Manager (Sprint 12)

Planner:

- Start: `FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- agent plan --project myproj "Prompt..."`
- List: `FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- agent plan list --project myproj`
- Show plan file: `FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- plan read plan-1`
- Stop: `FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- agent plan stop plan-1`

Manager:

- Start: `FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- manager start myproj`
- Status: `FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- manager status myproj`
- Clear: `FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- manager clear myproj`
- Stop: `FUGUE_DIR=/tmp/fugue-dev cargo run -p fugue -- manager stop myproj`

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
