# Configuration (`config.toml`)

Murmur reads global configuration from `config.toml`.

Paths:
- Default: `~/.config/murmur/config.toml`
- If `MURMUR_DIR` is set: `$MURMUR_DIR/config/config.toml`

Code pointers:
- Schema + validation: `crates/murmur-core/src/config.rs`
- Load/save: `crates/murmur/src/config_store.rs`
- CLI mutations: `crates/murmur/src/main.rs` (`project config ...`)

---

## Global Settings

### Log level

CLI flag: `--log-level <LEVEL>` (also via env `MURMUR_LOG`)

### Providers

Providers are stored as TOML tables under `[providers.*]`.
The daemon also supports environment variable fallbacks for common credentials.

GitHub:
- Config: `[providers.github].token` or `[providers.github].api-key`
- Env: `GITHUB_TOKEN` or `GH_TOKEN`

Linear:
- Config: `[providers.linear].api-key`
- Env: `LINEAR_API_KEY`

Anthropic (LLM auth):
- Config: `[providers.anthropic].api-key`
- Env: `ANTHROPIC_API_KEY`
- Optional endpoint override: `ANTHROPIC_API_URL`

OpenAI (LLM auth):
- Config: `[providers.openai].api-key`
- Env: `OPENAI_API_KEY`
- Optional endpoint override: `OPENAI_API_URL`

### LLM authorization

When a project uses `permissions-checker = "llm"`, Murmur uses `[llm_auth]` to decide tool permissions automatically. In LLM mode, Murmur is fail-closed: if authorization fails or the model is unsure, the request is denied (no manual fallback).

```toml
[llm_auth]
provider = "anthropic" # or "openai"
model = "claude-haiku-4-5"
```

---

## Webhook Settings

Optional:

```toml
[webhook]
enabled = true
bind-addr = "127.0.0.1:8080"
path-prefix = "/webhooks"
secret = "shared-secret"
```

See `docs/components/WEBHOOKS.md`.

---

## Polling Settings

Optional configuration for background polling tasks:

```toml
[polling]
comment-polling-enabled = true  # Enable automatic comment polling (default: true)
comment-interval-secs = 10      # Poll interval in seconds (default: 10)
```

When enabled, the daemon polls claimed issues for new comments and injects them into the corresponding agent's chat. Comments are deduplicated to prevent duplicate delivery.

You can also manually trigger comment sync for a specific agent:
```bash
mm agent sync-comments <agent-id>
```

---

## Orchestration Settings

Optional configuration for orchestration behavior:

```toml
[orchestration]
silence-threshold-secs = 60  # Seconds of user silence before resuming spawning (default: 60)
```

When a user sends a message to any agent in a project (coding agent, manager, or planner), the orchestrator pauses automatic agent spawning for that project until the silence threshold is reached. This prevents the system from spawning new agents while the user is actively working.

Set `silence-threshold-secs = 0` to disable intervention detection (always allow spawning).

Per-project override is available via `silence-threshold-secs` in `[[projects]]`.

---

## Director Configuration (`director.toml`)

The director agent reads its configuration from a separate file:

Path: `~/.murmur/config/director.toml` (or `$MURMUR_DIR/config/director.toml`)

```toml
[director]
allowed_patterns = ["mm:*"]
```

### Allowed Patterns

The `allowed_patterns` list controls which bash commands the director can execute. Pattern format:

| Pattern | Effect |
|---------|--------|
| `mm:*` | Allow any `mm` command |
| `git:*` | Allow any `git` command |
| `ls:*` | Allow `ls` with any arguments |
| `:*` | Allow all commands (not recommended) |
| `mm issue:*` | Allow `mm issue` subcommands |

By default, no patterns are configured (the director cannot run bash commands).

**Example:** Allow the director to manage projects and issues:

```toml
[director]
allowed_patterns = [
    "mm:*",
    "git status:*",
    "git log:*"
]
```

See also: `docs/components/PERMISSIONS_AND_QUESTIONS.md` for the full pattern syntax.

---

## Projects (`[[projects]]`)

Each project stores:
- `name` — project identifier
- `remote-url` — git remote URL to clone
- `max-agents` — max concurrent coding agents (default `3`)
- `autostart` — start orchestration on daemon startup
- `issue-backend` — `tk | github | gh | linear`
- `permissions-checker` — `manual | llm`
- `agent-backend` — `claude | codex` (fallback)
- `planner-backend` / `coding-backend` — optional overrides (fallback to `agent-backend`)
- `allowed-authors` — used by backends that support author filtering (notably GitHub)
- `linear-team` (required for Linear), `linear-project` (optional)
- `merge-strategy` — `direct | pull-request`
- `silence-threshold-secs` — per-project override for intervention detection (0 = use global)

You can inspect and edit via:
- `mm project config show <project>`
- `mm project config get <project> <key>`
- `mm project config set <project> <key> <value>`

Validation rules are enforced by `murmur-core` (`ConfigFile::validate`).
