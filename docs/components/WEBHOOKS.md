# Webhooks

Murmur can optionally run a local webhook server to receive:
- GitHub issue/issue_comment events
- Linear Issue/Comment events

The webhook server is local-only and is intended to trigger orchestration ticks quickly (instead of waiting for the next poll interval).

Code pointers:
- Implementation: `crates/murmur/src/daemon/webhook.rs`
- Dedup store: `crates/murmur/src/dedup_store.rs`

---

## Enable

Add to `config.toml`:

```toml
[webhook]
enabled = true
bind-addr = "127.0.0.1:8080"
path-prefix = "/webhooks"
secret = "shared-secret"
```

Notes:
- If `bind-addr` is `:8080`, Murmur binds `0.0.0.0:8080`.
- The path prefix defaults to `/webhooks`.

---

## Endpoints

Health:
- `GET /health` → `ok`

GitHub:
- `POST <path-prefix>/github?project=<project>`

Linear:
- `POST <path-prefix>/linear?project=<project>`

Project routing:
- Prefer query parameter `project=<name>`
- Otherwise, Murmur checks headers: `X-Murmur-Project`, `X-Project`

---

## Signatures

GitHub:
- Verifies `X-Hub-Signature-256` (HMAC SHA256 over the raw request body).

Linear:
- Verifies `Linear-Signature` (HMAC SHA256 over the raw request body).

If `secret` is empty, signature verification will fail (requests are rejected).

---

## Deduplication

Webhook deliveries are deduped to avoid repeated ticks:
- GitHub: uses `X-GitHub-Delivery` when present (else body hash)
- Linear: uses `Linear-Delivery` when present (else body hash)

Dedup is stored under:

`runtime/dedup.json`

---

## What a Webhook Does

When a relevant webhook arrives:
- Murmur emits an `orchestration.tick_requested` event
- if orchestration is running for the project, Murmur requests an immediate tick

The orchestrator loop still enforces max-agent capacity and claims.
