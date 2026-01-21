# Ticket Format (`tk` Backend)

The `tk` backend stores issues as Markdown files in your repository under `.murmur/tickets/`. This document specifies the file format.

## Overview

- **Location**: `.murmur/tickets/<issue-id>.md`
- **Format**: YAML frontmatter + Markdown body
- **Commit**: Use `mm issue commit` to push changes

## File Structure

```markdown
---
id: ISSUE-123
status: open
created: 2026-01-20T14:30:00Z
type: task
priority: 1
labels:
  - backend
  - api
deps:
  - ISSUE-100
links:
  - https://example.com/spec
---
# Issue Title

Description goes here. Can be multiple paragraphs
with full Markdown support.

## Comments

**2026-01-20 15:00**: First comment here.

**2026-01-20 16:30**: Another comment.
```

## Frontmatter Fields

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | string | Yes | — | Issue identifier (should match filename) |
| `status` | string | No | `open` | `open`, `closed`, or `blocked` |
| `created` | string | No | — | RFC3339 timestamp |
| `type` | string | No | — | Freeform (e.g., `task`, `bug`, `feature`) |
| `priority` | integer | No | `0` | Higher = more important |
| `labels` | list | No | `[]` | Freeform tags |
| `deps` | list | No | `[]` | Issue IDs this depends on |
| `links` | list | No | `[]` | Related URLs |

Unknown fields are ignored but not preserved on rewrite.

## Body Format

### Title

The first non-empty line after frontmatter may be a `# Title` header. If present, it becomes the issue title. Otherwise, the filename is used.

### Description

Everything after the title (if present) until `## Comments` is the description. Full Markdown is supported.

### Comments

Comments are appended under `## Comments`:

```markdown
## Comments

**YYYY-MM-DD HH:MM**: Comment text here.
```

Use `mm issue comment` to add comments in this format.

## Examples

### Minimal Issue

```markdown
---
id: ISSUE-1
---
# Add user login

Implement basic username/password authentication.
```

### Full Issue

```markdown
---
id: ISSUE-42
status: open
created: 2026-01-15T09:00:00Z
type: feature
priority: 2
labels:
  - auth
  - security
deps:
  - ISSUE-40
  - ISSUE-41
links:
  - https://example.com/auth-spec
  - https://github.com/org/repo/issues/42
---
# Implement OAuth2 login

Add OAuth2 support for Google and GitHub providers.

## Acceptance Criteria

- [ ] Google OAuth working
- [ ] GitHub OAuth working
- [ ] Token refresh implemented

## Comments

**2026-01-15 10:00**: Started investigation.

**2026-01-16 14:30**: Google OAuth prototype complete.
```

### Blocked Issue

```markdown
---
id: ISSUE-50
status: blocked
deps:
  - ISSUE-42
---
# Add social login buttons

Blocked waiting for OAuth implementation.
```

## CLI Commands

### Create

```bash
mm issue create "Title" -p myproj
mm issue create "Title" -p myproj --description "Details" --type bug --priority 1
```

### List and View

```bash
mm issue list -p myproj           # All issues
mm issue ready -p myproj          # Ready issues (open, no open deps)
mm issue show ISSUE-1 -p myproj   # Single issue
```

### Update

```bash
mm issue update ISSUE-1 -p myproj --status blocked
mm issue update ISSUE-1 -p myproj --priority 2
mm issue update ISSUE-1 -p myproj --title "New title"
```

### Close

```bash
mm issue close ISSUE-1 -p myproj
```

### Comment

```bash
mm issue comment ISSUE-1 -p myproj --body "Comment text"
```

### Commit Changes

```bash
mm issue commit -p myproj
```

This stages `.murmur/tickets/`, commits, and pushes.

## Ready Semantics

An issue is "ready" when:

1. Status is `open`
2. All issues in `deps` are `closed`

The orchestrator only spawns agents for ready issues.

## Best Practices

### ID Naming

- Use a consistent prefix: `ISSUE-`, `TASK-`, `BUG-`
- Sequential numbering works well: `ISSUE-1`, `ISSUE-2`
- Keep IDs short but descriptive

### Dependencies

- Use `deps` to block issues until prerequisites are done
- The orchestrator respects dependencies automatically
- Mark dependent issues as `blocked` for clarity

### Labels

- Use labels for categorization: `backend`, `frontend`, `urgent`
- Labels are for human organization; Murmur doesn't filter by them

### Committing

- Commit tickets regularly with `mm issue commit`
- This ensures agents see the latest state
- Tickets are synced via git, so conflicts are possible
