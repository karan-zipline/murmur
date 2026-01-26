# Murmur: instructions for coding agents

This file provides **repo-wide** instructions for AI coding agents working on Murmur.
Codex will read `AGENTS.md` files from the repo root down to your current working directory; add more-narrow `AGENTS.md` files in subdirectories when needed.

## Build, lint, and unit test (no daemon)

- Build (debug): `cargo build --workspace`
- Build (release): `cargo build --workspace --release`
- Unit tests only: `cargo test --workspace --lib`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt` and `cargo fmt --check`

### Do not start the server for testing

- **Never** run `mm server start` (or `cargo run ... -- server start ...`) as part of testing or validation.
- **Do not** run `scripts/smoke.sh` during agent work (it starts a real daemon and manipulates temp repos).
- Avoid integration tests by default (`crates/murmur/tests`); they may spin up a daemon and require agent CLIs on `PATH` (e.g. `claude`/`codex`).

## Project structure

- `crates/murmur-core/`: **Functional core** (pure domain logic; no I/O)
- `crates/murmur-protocol/`: IPC message types
- `crates/murmur/`: **Imperative shell** (daemon, CLI, and all I/O)

## Rust architecture and design guidelines

Murmur follows **Functional Core, Imperative Shell**:

- Keep `murmur-core` deterministic and easy to test:
  - No filesystem, network, subprocess, sockets, time, randomness, or logging.
  - Return decisions/actions **as data**, not effects.
- Keep all I/O and runtime concerns in `murmur`:
  - Translate external protocols into core values.
  - Execute the core’s decisions and emit events/persist state.
- Prefer high cohesion / low coupling: modules do one job with a narrow surface area.
- Preserve dependency inversion: I/O depends on core, never the reverse.
- Model domain state as values evolved via explicit events.
- Use small DTOs at boundaries (avoid “god structs”).
- Put traits (“ports”) at boundaries for adapters/integration points.

## Feature work checklist

When implementing new features:

1. Put domain logic in `crates/murmur-core/`
2. Put I/O + adapters in `crates/murmur/`
3. Add/update protocol types in `crates/murmur-protocol/` if daemon/CLI IPC needs it
4. Add unit tests (prefer `murmur-core`)
5. Run `cargo fmt --check`, `cargo clippy ...`, and `cargo test --workspace --lib`

References: `docs/ARCHITECTURE.md`, `docs/DEVELOPMENT.md`.
