# Development Guide

This guide covers building, testing, and contributing to Murmur.

## Prerequisites

- **Rust** — 1.75+ (install via [rustup](https://rustup.rs/))
- **Git** — 2.20+ (for worktree support)
- **Agent CLI** — `claude` or `codex` (for integration testing)

## Building

### Quick Build

```bash
cargo build --workspace
```

### Release Build

```bash
cargo build --workspace --release
```

### Install Locally

```bash
cargo install --locked --path crates/murmur
```

The binary installs as `mm` to `~/.cargo/bin/`.

## Testing

### Run All Tests

```bash
cargo test --workspace
```

### Run Specific Test

```bash
cargo test --package murmur test_name
```

### Run with Output

```bash
cargo test --workspace -- --nocapture
```

## Linting

### Clippy

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

### Format Check

```bash
cargo fmt --check
```

### Format

```bash
cargo fmt
```

## Running Locally

### Development Environment

Use `MURMUR_DIR` to isolate development state:

```bash
export MURMUR_DIR=/tmp/murmur-dev
```

### Start Daemon

```bash
# From source
cargo run -p murmur --bin mm -- server start --foreground

# If installed
mm server start --foreground
```

### Common Development Commands

```bash
# Add a test project
mm project add /path/to/repo --name test

# Create an issue
mm issue create "Test issue" -p test

# Start orchestration
mm project start test

# Watch events
mm attach test

# Check status
mm project status test
mm agent list
```

## Project Structure

```
crates/
├── murmur-core/      # Pure domain logic (no I/O)
├── murmur-protocol/  # IPC message types
└── murmur/           # CLI, daemon, and I/O adapters

docs/                 # Documentation
scripts/              # Development scripts
```

### Crate Dependencies

```
murmur (shell)
    ├── murmur-core
    └── murmur-protocol

murmur-core and murmur-protocol have no I/O dependencies
```

## Architecture Notes

Murmur follows **Functional Core, Imperative Shell**:

- **`murmur-core`** — Pure functions, no I/O, easy to test
- **`murmur`** — All I/O: git, processes, sockets, HTTP

When adding features:
1. Put domain logic in `murmur-core`
2. Put I/O and integration in `murmur`
3. Use DTOs at boundaries
4. Keep modules focused

## Integration Tests

Integration tests are in `crates/murmur/tests/`. They:

- Spin up a real daemon
- Use fake git repos
- Mock Claude/Codex with scripts
- Assert on state and messages

### Test Structure

```
tests/
├── orchestration.rs         # Spawn scheduling, merge
├── permissions_and_hooks.rs # Rule evaluation, hooks
├── agents.rs               # Agent lifecycle
├── issues_tk.rs            # Local ticket backend
├── github_backend.rs       # GitHub API (mocked)
├── linear_backend.rs       # Linear API (mocked)
└── ...
```

### Running Integration Tests

```bash
# All integration tests
cargo test --package murmur --test '*'

# Specific test file
cargo test --package murmur --test orchestration
```

## Smoke Test

A quick end-to-end demo:

```bash
bash scripts/smoke.sh
```

## Adding Features

### New CLI Command

1. Add command struct in `src/main.rs`
2. Implement handler
3. Add IPC message type to `murmur-protocol` if needed
4. Add RPC handler in `daemon/rpc/`
5. Add tests
6. Update CLI docs

### New Issue Backend

1. Implement trait in `murmur/src/`
2. Add to backend selection in `daemon/issue_backend.rs`
3. Add config options in `murmur-core/src/config.rs`
4. Add integration tests
5. Update documentation

### New Permission Rule Type

1. Add rule type to `murmur-core/src/permissions.rs`
2. Update rule evaluation logic
3. Update `permissions.toml` parsing
4. Add tests
5. Document in PERMISSIONS_AND_QUESTIONS.md

## Debugging

### Daemon Logs

```bash
tail -f ~/.murmur/murmur.log

# Or with MURMUR_DIR
tail -f $MURMUR_DIR/murmur.log
```

### Verbose Logging

```bash
MURMUR_LOG=debug mm server start --foreground
```

### IPC Inspection

The daemon socket is at:
- `$XDG_RUNTIME_DIR/murmur.sock` (default)
- `$MURMUR_DIR/murmur.sock` (if MURMUR_DIR set)

## Release Checklist

1. Update version in `Cargo.toml`
2. Run full test suite
3. Run clippy with no warnings
4. Update CHANGELOG
5. Tag release

## Documentation

Documentation lives in `docs/`. When updating:

1. Keep user guides in `docs/` root
2. Put deep dives in `docs/components/`
3. Update `docs/README.md` index
4. Keep CLI.md in sync with actual commands

## Getting Help

- **Code questions**: Check inline comments and tests
- **Architecture**: See [ARCHITECTURE.md](ARCHITECTURE.md)
- **Usage**: See [USAGE.md](USAGE.md)
