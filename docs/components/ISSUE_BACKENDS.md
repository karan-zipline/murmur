# Issue Backends

Fugue supports multiple issue backends per project:
- `tk` — local markdown files under `.fugue/tickets/` in the repo clone
- GitHub Issues (GraphQL)
- Linear Issues (GraphQL)

The orchestrator always operates on the unified `Issue` model defined in `fugue-core`.

Code pointers:
- Domain model + pure helpers: `crates/fugue-core/src/issue.rs`
- Backend selection: `crates/fugue/src/daemon/issue_backend.rs`
- `tk` backend implementation: `crates/fugue/src/issues.rs`
- GitHub: `crates/fugue/src/github.rs`
- Linear: `crates/fugue/src/linear.rs`
- `issue plan` implementation: `crates/fugue/src/daemon/rpc/issue.rs`

---

## Shared Issue Model

The canonical model is `fugue_core::issue::Issue`:
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

`projects/<project>/repo/.fugue/tickets/<id>.md`

Format details: `docs/TICKETS.md`.

### Ready semantics

`issue ready` for `tk`:
- lists open issues
- filters out issues with open dependencies (dependency ids still open)

Implementation uses `fugue_core::issue::compute_ready_issues`.

### Commit behavior

`issue commit` exists for `tk` only:
- stages `.fugue/tickets/`
- commits with provided message
- pushes `HEAD` to `origin`
- uses a `.lock` file in the tickets dir to serialize ticket commits

---

## GitHub Backend

### Requirements

- Project repo `origin` must be a GitHub remote so Fugue can detect `owner/repo`.
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
- `crates/fugue-core/src/issue.rs` (`upsert_plan_section`)

CLI usage:

`fugue issue plan <project> <id> --file plan.md`

or

`fugue issue plan <project> <id> --body "..."`.
