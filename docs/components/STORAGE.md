# Storage Layout and Persistence

Fugue is local-only and stores both:
- runtime state (socket/logs, agent metadata, dedup)
- project clones and worktrees
- plan artifacts

This doc is about *where* data lives and the persistence semantics.

Code pointers:
- Path resolution: `crates/fugue-core/src/paths.rs`
- Config persistence: `crates/fugue/src/config_store.rs`
- Agent runtime persistence: `crates/fugue/src/runtime_store.rs`
- Webhook dedup persistence: `crates/fugue/src/dedup_store.rs`

---

## Base Directory (`~/.fugue`)

Default base directory is `~/.fugue`.

To override everything (recommended for local testing), set:

`FUGUE_DIR=/tmp/fugue-dev`

---

## On-Disk Layout

Under the base directory:

```
<FUGUE_DIR or ~/.fugue>/
  fugue.log
  plans/
    plan-1.md
  runtime/
    agents.json
    dedup.json
  projects/
    <project>/
      repo/
      worktrees/
        wt-a-1/
        wt-plan-1/
      permissions.toml
```

Under the config directory:

```
<~/.config/fugue or $FUGUE_DIR/config>/
  config.toml
  permissions.toml
```

Notes:
- A reference permissions template ships in the repo as `permissions.toml.default`.
- The `tk` backend stores issues inside the project repo clone under `.fugue/tickets/`.
- `project remove` unregisters a project; by default it does *not* delete the repo clone.
- The daemon socket is a Unix domain socket named `fugue.sock`. By default it is placed in `XDG_RUNTIME_DIR` when available; when `FUGUE_DIR` is set, it is placed under `$FUGUE_DIR/fugue.sock`.

---

## Persistence Semantics

### Config (`config.toml`)

- Source of truth for registered projects and global settings.
- Written atomically (write temp file + rename).

### Agent runtime (`runtime/agents.json`)

- Best-effort snapshot of agent runtime metadata.
- Used to resume agents across daemon restarts when possible (backend-dependent).
- Written atomically (write temp file + rename).

### Webhook dedup (`runtime/dedup.json`)

- Recent webhook deliveries (bounded by max age and max entries).
- Prevents repeated tick requests from identical deliveries.
- Written atomically (write temp file + rename).

### Logs (`fugue.log`)

- Structured logs written to the base directory.
- Useful for debugging daemon startup, IPC, agent spawn, merge failures.
