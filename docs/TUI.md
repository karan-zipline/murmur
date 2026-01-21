# TUI

`fugue tui` is a single-screen terminal UI that connects to the daemon, shows agents + chat, and supports the core interactive loops: approvals, AskUserQuestion answers, planner start, and abort flows.

## Run

- Start daemon: `fugue server start --foreground`
- Start TUI: `fugue tui`

## Layout

- Header: connection state + counts (agents/commits/perms/questions)
- Left pane: agent list + recent commits
- Right pane: chat view (+ input panel when composing)

## Keybindings

### Normal mode

- `Tab`: switch focus between agent list and chat
- `j/k`: move selection (agent list) / scroll (chat)
- `PgUp/PgDn`: page scroll (chat)
- `Enter`: open input
- `p`: start planner (project picker → prompt)
- `t`: toggle tool events in chat
- `x`: abort/stop confirm (`y/n`)
- `r`: reconnect (only shown when disconnected)
- `q` or `Ctrl+C`: quit

### Input mode

- `Enter`: submit (send message / answer / start planner)
- `Shift+Enter`: newline
- `↑/↓`: input history
- `Esc`: cancel and clear
- `Tab`: exit input (keeps draft)

## Approvals (Permissions)

When a Claude agent requests a tool permission, the chat pane shows a permission overlay:

- `y`: allow
- `n`: deny

Agents with pending permissions are marked with `P` in the agent list.

## Tool Events (Hidden by Default)

By default, Fugue hides tool invocations/results in chat to keep the UI uncluttered.

- Press `t` to toggle tool events on/off.
- When shown, tool lines use the same semantics as the CLI:
  - Invocation line: `[ToolName] <input>`
  - Result line: `-> <summary>` (special-cased for `Read`/`Grep`, full output for error results)

## AskUserQuestion

When a Claude agent asks a question, the chat pane shows a question overlay:

- `j/k`: select option
- `y`: submit selection
- `Other…`: enters input mode for a freeform answer

Agents with pending questions are marked with `Q` in the agent list.

## Reconnect

If the daemon restarts or the event stream drops:

- The header shows `disconnected`.
- Fugue retries automatically with exponential backoff.
- Press `r` to force an immediate reconnect.

On reconnect, Fugue refreshes lists and re-fetches the selected agent’s history.

## Rich Text Rendering

Chat messages render a small Markdown subset:

- fenced code blocks (```` ``` ````)
- inline code (`` `like this` ``)
- bold (`` **like this** ``)
