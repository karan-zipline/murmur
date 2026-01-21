# Permissions and AskUserQuestion

Fugue integrates with Claude Code via hooks to support:
- rule-based allow/deny decisions for tool calls
- interactive manual approvals (CLI)
- AskUserQuestion prompts (agent asks the user; Fugue brokers the response)

Code pointers:
- Hook entrypoints: `crates/fugue/src/hooks.rs`
- Permission rule model + evaluation: `crates/fugue-core/src/permissions.rs`
- Rule loading (global + project): `crates/fugue/src/permissions.rs`
- Permission RPC: `crates/fugue/src/daemon/rpc/permission.rs`
- Question RPC: `crates/fugue/src/daemon/rpc/question.rs`

---

## Where Rules Live

Global rules:
- `~/.config/fugue/permissions.toml`
- or `$FUGUE_DIR/config/permissions.toml`

Project rules (optional):
- `projects/<project>/permissions.toml`

Project rules are applied before global rules.

---

## Rule Model (Core)

Rules are expressed as:
- `tool` (e.g. `Bash`, `Grep`, `WriteFile`)
- `action` (`allow` or `deny`)
- `pattern` (tool-specific matcher)

Rule evaluation is pure and deterministic; the hook uses it to return an immediate decision when possible.

See `crates/fugue-core/src/permissions.rs` for matcher semantics and examples.

---

## Claude `PreToolUse` Hook

When a Claude Code agent attempts to run a tool:
1. Claude invokes `fugue hook PreToolUse` and passes JSON on stdin.
2. Fugue loads rules and evaluates them.
3. If a rule matches, Fugue returns an allow/deny response JSON immediately.
4. If no rule decides, Fugue asks the daemon (`permission.request`).
   - With `permissions-checker = "manual"`, the daemon blocks until the user responds.
   - With `permissions-checker = "llm"`, the daemon uses `[llm_auth]` to auto-decide `allow|deny`. In LLM mode, Fugue is fail-closed: on `unsure` or provider/config errors, the request is denied and is not surfaced for manual approval.

Fugue also configures the legacy `PermissionRequest` hook event name as an alias to `PreToolUse` (see `docs/components/HOOKS.md`).

User response surfaces in:
- CLI:
  - `fugue permission list`
  - `fugue permission respond <id> allow|deny`

---

## AskUserQuestion

AskUserQuestion is a special tool call.

Fugue handles it inside the same `PreToolUse` hook:
1. It parses the AskUserQuestion tool input (questions list).
2. It sends `question.request` to the daemon and blocks.
3. The user answers via CLI.
4. Fugue injects the answers into `tool_input` and returns an allow response to Claude.

CLI:
- `fugue question list`
- `fugue question respond <id> '{"q1":"answer"}'`

---

## Manager Agent Restrictions

Manager agents are intended to coordinate, not implement code.

The global permissions file can contain:

```toml
[manager]
allowed_patterns = ["fugue:*", "git :*"]
```

Fugue translates these patterns into Claude “allow” settings for the manager agent.

Default is conservative: `["fugue:*"]`.

---

## Notes / Limitations

- Codex backend tool approvals are not intercepted by Fugue.
- LLM approvals require `[llm_auth]` configuration and a matching provider API key (`ANTHROPIC_API_KEY` / `OPENAI_API_KEY`, or `[providers.<provider>].api-key`).
 - A reference permissions template ships in the repo as `permissions.toml.default`.
