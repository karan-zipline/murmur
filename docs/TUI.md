# Terminal UI Guide

The terminal UI (`mm tui`) provides real-time monitoring of agents, approvals, and chat history.

## Quick Start

```bash
# Start the daemon (in one terminal)
mm server start --foreground

# Launch the TUI (in another terminal)
mm tui
```

## Layout

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Murmur │ Connected │ Agents: 3 │ Commits: 5 │ Perms: 1 │ Questions: 0  │
├─────────────────────────────────────────────────────────────────────────┤
│  AGENTS                │  CHAT                                          │
│  ┌───────────────────┐ │  ┌────────────────────────────────────────────┐│
│  │ ▸ a-1 myproj [R]  │ │  │ 14:32:15 Assistant                        ││
│  │   a-2 myproj [R]  │ │  │ I'll implement the user authentication    ││
│  │   plan-1    [P]   │ │  │ system. First, let me read the existing   ││
│  │   manager-myproj  │ │  │ code...                                   ││
│  └───────────────────┘ │  │                                            ││
│                        │  │ 14:32:18 User                              ││
│  RECENT COMMITS        │  │ (tool approved)                            ││
│  ┌───────────────────┐ │  │                                            ││
│  │ abc1234 Fix login │ │  │ 14:32:20 Assistant                        ││
│  │ def5678 Add tests │ │  │ Now I'll create the new authentication    ││
│  └───────────────────┘ │  │ module...                                  ││
│                        │  └────────────────────────────────────────────┘│
│                        │  ┌────────────────────────────────────────────┐│
│                        │  │ > Type a message...                        ││
│                        │  └────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────────────┘
```

### Panes

| Pane | Description |
|------|-------------|
| **Header** | Connection status, counts for agents, commits, permissions, questions |
| **Agent List** | All agents with state indicators; recent commits below |
| **Chat View** | Conversation history for the selected agent |
| **Input** | Message input (appears when composing) |

### Agent State Indicators

| Indicator | Meaning |
|-----------|---------|
| `[R]` | Running |
| `[S]` | Starting |
| `[E]` | Exited |
| `[A]` | Aborted |
| `[!]` | Needs resolution (merge conflict) |
| `[P]` | Pending permission |
| `[Q]` | Pending question |

## Keybindings

### Normal Mode

| Key | Action |
|-----|--------|
| `Tab` | Switch focus between agent list and chat pane |
| `j` / `k` | Move selection (agent list) or scroll (chat) |
| `↑` / `↓` | Same as `j` / `k` |
| `PgUp` / `PgDn` | Page scroll in chat |
| `Home` / `End` | Jump to start/end of chat |
| `Enter` | Open input to compose a message |
| `p` | Start a new planner |
| `t` | Toggle tool event visibility |
| `x` | Abort/stop selected agent (prompts `y`/`n`) |
| `r` | Reconnect (when disconnected) |
| `q` | Quit |
| `Ctrl+C` | Quit |

### Input Mode

| Key | Action |
|-----|--------|
| `Enter` | Submit message |
| `Shift+Enter` | Insert newline |
| `↑` / `↓` | Navigate input history |
| `Esc` | Cancel and clear input |
| `Tab` | Exit input (keeps draft) |

### Permission Prompt

When a permission request is pending:

| Key | Action |
|-----|--------|
| `y` | Allow the tool call |
| `n` | Deny the tool call |

### Question Prompt

When an agent asks a question:

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate options |
| `y` | Submit selected option |
| Select "Other" | Opens input for freeform answer |

## Features

### Permission Approvals

When a Claude agent requests permission to use a tool:

1. The agent appears with `[P]` indicator in the list
2. The chat pane shows a permission overlay with tool details
3. Review the tool name and arguments
4. Press `y` to allow or `n` to deny
5. The agent continues or stops based on your decision

### User Questions

When an agent uses `AskUserQuestion`:

1. The agent appears with `[Q]` indicator
2. The chat pane shows the question with options
3. Navigate with `j`/`k`, select with `y`
4. Choose "Other" for a custom text response

### Tool Event Visibility

By default, tool invocations and results are hidden to keep the chat clean.

Press `t` to toggle tool visibility:

- **Hidden** (default): Only shows assistant and user messages
- **Visible**: Shows tool calls with syntax:
  - Invocation: `[ToolName] <input summary>`
  - Result: `-> <result summary>`

### Planner Quick Start

Press `p` to start a new planner:

1. Select a project (if multiple exist)
2. Enter your planning prompt
3. The planner appears in the agent list

### Reconnection

If the daemon restarts or the connection drops:

1. Header shows "disconnected"
2. Auto-retry with exponential backoff
3. Press `r` to force immediate reconnect
4. On reconnect: lists refresh, selected agent history reloads

### Rich Text Rendering

Chat messages render a subset of Markdown:

- Fenced code blocks (` ``` `)
- Inline code (`` `like this` ``)
- Bold (`**like this**`)

## Tips

### Efficient Monitoring

- Use `Tab` to switch between agent list and chat quickly
- Press `j`/`k` to scan through agents
- The header shows pending approvals/questions at a glance

### Managing Multiple Agents

- Agents from all projects appear in one list
- Sort is by creation time (newest first)
- Select an agent to see its full chat history

### Handling Conflicts

If an agent shows `[!]` (needs resolution):
1. The worktree has merge conflicts
2. Resolve conflicts manually in the worktree
3. The agent can then be retried or aborted

## Troubleshooting

### TUI won't start

- Ensure the daemon is running: `mm server status`
- Check connection: `mm ping`

### No agents appear

- Verify orchestration is running: `mm project status <name>`
- Check for ready issues: `mm issue ready -p <name>`

### Permission prompts not showing

- Only Claude backend supports permission hooks
- Codex handles permissions internally

### Chat not updating

- Check connection status in header
- Press `r` to reconnect if needed
