# Agent Host

The Agent Host system decouples agent processes from the daemon, allowing agents to survive daemon restarts.

Code pointers:
- Host protocol types: `crates/murmur-protocol/src/host.rs`
- Host process entry: `crates/murmur/src/bin/murmur_host.rs`
- Host manager (agent process): `crates/murmur/src/host/manager.rs`
- Host server (socket): `crates/murmur/src/host/server.rs`
- Daemon host manager: `crates/murmur/src/daemon/host_manager.rs`

See also:
- `docs/components/AGENTS.md`
- `docs/components/DAEMON.md`

---

## Overview

Previously, agent subprocesses (Claude Code, Codex) were direct children of the daemon process. If the daemon restarted, all agents would be killed.

The Agent Host architecture introduces an intermediate process layer:

```
Daemon
   │
   └──→ murmur-host (per agent)
           │
           └──→ claude / codex subprocess
```

Each `murmur-host` process:
- Wraps a single agent subprocess
- Exposes a Unix socket for daemon communication
- Can survive daemon restarts
- Buffers recent events for replay

---

## Host Protocol

The daemon and host processes communicate over Unix sockets using a JSONL protocol.

### Socket Location

Host sockets are stored in `~/.murmur/hosts/`:

```
~/.murmur/hosts/
├── agent-abc123.sock
├── agent-def456.sock
└── plan-xyz789.sock
```

### Message Format

**Request:**
```json
{"msg_type": "status", "id": "req-1", "payload": null}
```

**Response:**
```json
{"msg_type": "status", "id": "req-1", "success": true, "payload": {...}}
```

**Stream Event:**
```json
{"event_type": "chat", "agent_id": "abc123", "payload": {...}}
```

### Message Types

| Type | Direction | Description |
|------|-----------|-------------|
| `ping` | Request | Health check |
| `status` | Request | Get agent status |
| `list` | Request | List agents (returns single agent for host) |
| `attach` | Request | Subscribe to event stream |
| `detach` | Request | Unsubscribe from event stream |
| `send` | Request | Send message to agent |
| `stop` | Request | Stop agent gracefully or forcefully |

---

## Host Manager (Host-Side)

The `Manager` struct in `murmur/src/host/manager.rs` manages the agent subprocess:

### Responsibilities

- Spawn and monitor the agent subprocess (Claude or Codex)
- Parse agent output into canonical events
- Buffer recent events in a ring buffer (1000 events)
- Broadcast events to attached clients
- Handle incoming messages (forward to agent stdin)
- Graceful and forced shutdown

### Event Buffer

The host maintains a ring buffer of recent events:

```rust
const EVENT_BUFFER_SIZE: usize = 1000;
```

When a client attaches, it can request events from a specific offset for replay. This enables the daemon to catch up on events that occurred while it was restarting.

---

## Host Manager (Daemon-Side)

The `HostManager` struct in `murmur/src/daemon/host_manager.rs` manages connections to host processes:

### Responsibilities

- Spawn new agent hosts via `murmur-host` binary
- Maintain connections to running hosts
- Send messages to agents
- Stop agents
- Discover and reconnect to orphaned hosts on startup

### Discovery

On daemon startup, the host manager scans the hosts directory for `.sock` files:

```rust
pub async fn discover_and_reconnect(&self) -> anyhow::Result<Vec<HostAgentInfo>>
```

For each socket:
1. Attempt to connect and ping
2. If successful, retrieve agent status
3. Add to managed clients map
4. If socket is stale (no response), remove it

This allows agents to survive daemon restarts and be reconnected.

---

## Spawn Flow

When spawning an agent via host:

```
1. Daemon calls HostManager::spawn_agent(config)
2. HostManager spawns murmur-host process with args:
   --agent-id, --project, --role, --backend, --worktree, etc.
3. murmur-host:
   a. Creates Unix socket at hosts/<agent-id>.sock
   b. Spawns agent subprocess (claude/codex)
   c. Starts socket server
4. Daemon waits for socket to appear (with timeout)
5. Daemon connects to socket
6. Daemon retrieves initial status
7. Agent is ready for work
```

---

## CLI Commands

### `mm host list`

List all connected agent hosts:

```bash
$ mm host list
AGENT_ID     PROJECT    ROLE     STATE
agent-abc123 myproject  coding   running
plan-xyz789  myproject  planner  idle
```

### `mm host status <agent-id>`

Get detailed status for a specific host:

```bash
$ mm host status agent-abc123
Agent ID: agent-abc123
Project: myproject
Role: coding
State: running
Issue: #42
Stream Offset: 1234
Attached Clients: 1
```

### `mm host discover`

Manually trigger discovery of orphaned hosts:

```bash
$ mm host discover
Discovered 2 running hosts
```

---

## Daemon Restart Behavior

When the daemon restarts:

1. `discover_and_reconnect()` is called during startup
2. The hosts directory is scanned for `.sock` files
3. Each socket is probed with a ping/status request
4. Responsive hosts are reconnected and their agents restored
5. Stale sockets (dead processes) are cleaned up

### What is Preserved

- Agent subprocess continues running
- Agent can continue working on its task
- Recent events are buffered for replay

### What is Lost

- Attached clients must reattach
- Some events may be missed if buffer overflows

---

## Protocol Version

The host protocol includes a version number for compatibility:

```rust
pub const HOST_PROTOCOL_VERSION: &str = "0.1.0";
```

The version is returned in ping responses, allowing the daemon to detect incompatible hosts.
