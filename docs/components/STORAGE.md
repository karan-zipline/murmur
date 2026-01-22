# Storage Layout and Persistence

Murmur is local-only and stores both:
- runtime state (socket/logs, agent metadata, dedup)
- project clones and worktrees
- plan artifacts

This doc is about *where* data lives and the persistence semantics.

Code pointers:
- Path resolution: `crates/murmur-core/src/paths.rs`
- Config persistence: `crates/murmur/src/config_store.rs`
- Agent runtime persistence: `crates/murmur/src/runtime_store.rs`
- Webhook dedup persistence: `crates/murmur/src/dedup_store.rs`

---

## Base Directory (`~/.murmur`)

Default base directory is `~/.murmur`.

To override everything (recommended for local testing), set:

`MURMUR_DIR=/tmp/murmur-dev`

---

## On-Disk Layout

Under the base directory:

```
<MURMUR_DIR or ~/.murmur>/
  murmur.log
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
<~/.config/murmur or $MURMUR_DIR/config>/
  config.toml
  permissions.toml
```

Notes:
- A reference permissions template ships in the repo as `permissions.toml.default`.
- The `tk` backend stores issues inside the project repo clone under `.murmur/tickets/`.
- `project remove` unregisters a project; by default it does *not* delete the repo clone.
- The daemon socket is a Unix domain socket named `murmur.sock`. By default it is placed under `~/.murmur/murmur.sock` (or `$MURMUR_DIR/murmur.sock` when `MURMUR_DIR` is set). When `MURMUR_SOCKET_PATH` is set, it overrides the socket path.

---

## Persistence Semantics

### Config (`config.toml`)

- Source of truth for registered projects and global settings.
- Written atomically (write temp file + rename).

### Agent runtime (`runtime/agents.json`)

- Best-effort snapshot of agent runtime metadata.
- Written atomically (write temp file + rename) after agent spawn and state changes.
- **On daemon restart**, agents are rehydrated from this file:
  - Agents whose worktrees still exist are restored to the in-memory registry.
  - Process liveness is checked via `/proc/<pid>`.
  - Live processes are marked `Running`; dead processes are marked `Exited`.
  - This allows `mm agent claim` and `mm agent done` to work for agents spawned in previous daemon sessions.
  - Note: Chat history and Codex thread IDs are lost on restart; only agent metadata is preserved.

### Webhook dedup (`runtime/dedup.json`)

- Recent webhook deliveries (bounded by max age and max entries).
- Prevents repeated tick requests from identical deliveries.
- Written atomically (write temp file + rename).

### Logs (`murmur.log`)

- Structured logs written to the base directory.
- Useful for debugging daemon startup, IPC, agent spawn, merge failures.
