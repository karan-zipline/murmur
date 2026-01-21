# IPC Protocol (Unix Socket + JSONL)

Fugue uses a local-only IPC protocol:
- Transport: Unix domain socket (`fugue.sock`). By default, the socket is placed in `XDG_RUNTIME_DIR` when available; when `FUGUE_DIR` is set, the socket is placed under `$FUGUE_DIR/fugue.sock`.
- Framing: JSONL (one JSON object per line)
- Message types: request/response + out-of-band events

Code pointers:
- Protocol types/constants: `crates/fugue-protocol/src/lib.rs`
- JSONL framing: `crates/fugue/src/ipc/jsonl.rs`
- Server router: `crates/fugue/src/daemon/server.rs`

---

## Message Envelopes

Requests:

```json
{ "type": "project.list", "id": "req-123", "payload": {} }
```

Responses:

```json
{ "type": "project.list", "id": "req-123", "success": true, "payload": {} }
```

Events:

```json
{ "type": "agent.chat", "id": "evt-42", "payload": { "agent_id": "a-1", "project": "demo", "message": { "role": "assistant", "content": "..." } } }
```

Conventions:
- `id` is used to correlate request/response pairs.
- Extra/unknown fields are ignored for forwards compatibility.

---

## Attach / Detach (Event Streaming)

`attach` puts the connection into “stream mode”:
- the daemon continues to send events over the same socket connection
- the client may still send requests (e.g. `detach`) and receive responses

Attach request payload:

```json
{ "projects": ["myproj"] }
```

If `projects` is empty, all projects are streamed.

Detach stops event delivery on that connection.

---

## Major Request Types (Stable Names)

Server:
- `ping`
- `shutdown`

Streaming:
- `attach`
- `detach`

Projects:
- `project.add`
- `project.list`
- `project.remove`
- `project.status`
- `project.config.show`
- `project.config.get`
- `project.config.set`

Orchestration:
- `orchestration.start`
- `orchestration.stop`
- `orchestration.status`

Agents:
- `agent.create`
- `agent.list`
- `agent.delete`
- `agent.abort`
- `agent.send_message`
- `agent.chat_history`
- `agent.done`
- `agent.idle`
- `agent.claim`
- `agent.describe`

Issues:
- `issue.list`
- `issue.get`
- `issue.ready`
- `issue.create`
- `issue.update`
- `issue.close`
- `issue.comment`
- `issue.commit`
- `issue.plan`

Plans (running planners):
- `plan.start`
- `plan.stop`
- `plan.list`
- `plan.send_message`
- `plan.chat_history`
- `plan.show`

Manager:
- `manager.start`
- `manager.stop`
- `manager.status`
- `manager.send_message`
- `manager.chat_history`
- `manager.clear_history`

Permissions + questions:
- `permission.request`
- `permission.respond`
- `permission.list`
- `question.request`
- `question.respond`
- `question.list`

Stats:
- `stats`
- `commit.list`
- `claim.list`

---

## Major Event Types

The daemon emits:
- `heartbeat`
- `agent.chat` (canonical chat messages for agents/planners/manager)
- `permission.requested`
- `question.requested`
- `agent.idle`
- `orchestration.tick_requested` (from webhooks or internal triggers)

Event payloads are defined in `crates/fugue-protocol/src/lib.rs`.
