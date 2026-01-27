# Issue Backends

Murmur supports multiple issue backends per project:
- `tk` — local markdown files under `.murmur/tickets/` in the repo clone
- GitHub Issues (GraphQL)
- Linear Issues (GraphQL)

The orchestrator always operates on the unified `Issue` model defined in `murmur-core`.

Code pointers:
- Domain model + pure helpers: `crates/murmur-core/src/issue.rs`
- Backend selection: `crates/murmur/src/daemon/issue_backend.rs`
- `tk` backend implementation: `crates/murmur/src/issues.rs`
- GitHub: `crates/murmur/src/github.rs`
- Linear: `crates/murmur/src/linear.rs`
- `issue plan` implementation: `crates/murmur/src/daemon/rpc/issue.rs`

---

## Shared Issue Model

The canonical model is `murmur_core::issue::Issue`:
- `id`, `title`, `description`
- `status` (`open|blocked|closed`)
- `priority`, `type`
- `dependencies` (issue ids)
- `labels`, `links`
- `created_at_ms`

Filtering for `issue list` is handled by `ListFilter` in the core crate.

---

## `tk` Backend (Local Tickets)

### Storage

Tickets are stored in the project repo clone:

`projects/<project>/repo/.murmur/tickets/<id>.md`

Format details: `docs/TICKETS.md`.

### Ready semantics

`issue ready` for `tk`:
- lists open issues
- filters out issues with open dependencies (dependency ids still open)

Implementation uses `murmur_core::issue::compute_ready_issues`.

### Commit behavior

`issue commit` exists for `tk` only:
- stages `.murmur/tickets/`
- commits with provided message
- pushes `HEAD` to `origin`
- uses a `.lock` file in the tickets dir to serialize ticket commits

---

## GitHub Backend

### Requirements

- Project repo `origin` must be a GitHub remote so Murmur can detect `owner/repo`.
- Auth via:
  - env: `GITHUB_TOKEN` or `GH_TOKEN`
  - config: `[providers.github].token` / `api-key`

### Ready semantics

GitHub `ready()` additionally filters:
- blocked issues (blocked-by open issues)
- disallowed authors (if `allowed-authors` is configured; defaults to repo owner when empty)

---

## Linear Backend

### Requirements

- Auth via:
  - env: `LINEAR_API_KEY`
  - config: `[providers.linear].api-key`
- Per-project `linear-team` is required
- `linear-project` is optional (scopes issues)

### Ready semantics

Linear `ready()` uses the shared dependency filter logic from the core crate.

---

## `issue plan` (Upsert Plan Section)

`issue plan` updates a `## Plan` section inside the issue body.

Supported by:
- `tk` (updates the local markdown ticket)
- GitHub and Linear (updates the issue body via API)

The upsert logic is pure and lives in:
- `crates/murmur-core/src/issue.rs` (`upsert_plan_section`)

CLI usage:

`mm issue plan <project> <id> --file plan.md`

or

`mm issue plan <project> <id> --body "..."`.

---

## Comment Polling

The daemon can poll claimed issues for new comments and inject them into the agent's chat.

Supported by:
- **GitHub** — fetches comments via GraphQL, filters by creation time
- **Linear** — fetches comments via GraphQL, filters by creation time
- **`tk`** — not supported (returns error; comments are stored in issue body)

Comments are deduplicated using a shared `DedupStore` to prevent duplicate delivery.

Configuration (in `config.toml`):
```toml
[polling]
comment-polling-enabled = true  # default: true
comment-interval-secs = 10      # default: 10
```

Manual sync: `mm agent sync-comments <agent-id>`
