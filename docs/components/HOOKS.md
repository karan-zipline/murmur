# Claude Hooks

Fugue integrates with Claude Code by passing a `--settings` JSON blob when spawning the `claude` CLI. This settings blob configures **hooks** that invoke Fugue (as a subprocess) during key Claude lifecycle events.

`fugue hook` is an **internal integration surface** used by the Claude hook mechanism; it is intentionally hidden from `fugue --help` and is not meant to be invoked manually.

## Hooks Used

Fugue configures the following Claude hooks:

- `PreToolUse` → runs `fugue hook PreToolUse`
- `PermissionRequest` (legacy compatibility) → runs `fugue hook PermissionRequest` (aliases to `PreToolUse`)
- `Stop` → runs `fugue hook Stop`

## Why This Exists

- `PreToolUse` is the interception point for permission checks and `AskUserQuestion`.
- `PermissionRequest` is maintained for parity/compatibility with environments that still emit this hook event name.
- `Stop` is used to notify the daemon that an agent has gone idle (best-effort).

## How Hook Commands Are Resolved

When Fugue spawns `claude`, it must embed a command string for the hook, e.g.:

```sh
'fugue' 'hook' 'PreToolUse'
```

Fugue resolves the executable prefix in this order:

1. `$FUGUE_HOOK_EXE` (if set)
2. `current_exe()` from the running daemon process
3. Fallback: `fugue` (PATH lookup)

On Linux, `current_exe()` can report a path ending in ` (deleted)` if the running binary has been replaced/unlinked during a rebuild. Fugue strips this suffix before embedding the command.

Hook commands are rendered with POSIX shell escaping so paths containing spaces/parentheses cannot break parsing.

## Troubleshooting

### Symptom: hook command shows `(... (deleted))` and tools fail

Example error:

```text
/home/ubuntu/.cargo/bin/fugue (deleted) hook PreToolUse: /bin/sh: 1: Syntax error: word unexpected
```

Recovery:

- Restart the Fugue daemon (it will then embed a non-deleted path).
- Or set `FUGUE_HOOK_EXE=fugue` (or an absolute path to a stable install) before starting the daemon.

### Symptom: hooks cannot find `fugue`

- Ensure `fugue` is on `PATH` for the daemon process, or set `FUGUE_HOOK_EXE` to an absolute path.
