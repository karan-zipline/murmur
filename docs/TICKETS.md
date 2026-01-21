# `tk` Tickets (`.fugue/tickets/`)

Fugue’s `tk` backend stores issues as Markdown files in the project repository under:

`./.fugue/tickets/<issue-id>.md`

These files are intended to be committed and pushed via `fugue issue commit`.

## File Format

Each ticket is a Markdown file with:

1) YAML frontmatter delimited by `---`
2) A Markdown body containing a title header and freeform description

### YAML frontmatter

Supported keys:

- `id` (string): Issue id; should match the filename stem.
- `status` (string): `open` | `closed` | `blocked` (defaults to `open` when absent/empty).
- `created` (string, optional): RFC3339 timestamp (example: `2026-01-20T00:40:26Z`).
- `type` (string, optional): Freeform issue type (examples: `task`, `feature`, `bug`).
- `priority` (int, optional): Defaults to `0`.
- `labels` (list[string], optional)
- `deps` (list[string], optional): Dependencies by issue id. While an issue depends on any *open* issue id, it won’t appear in `fugue issue ready`.
- `links` (list[string], optional): Related URLs.

Unknown keys are ignored by Fugue (but will not be preserved if Fugue rewrites the file).

### Body

Body conventions:

- Optional title: the first non-empty line may be `# <title>`.
- The remainder is the description (freeform Markdown).
- Comments are appended into the description under a `## Comments` section.

`fugue issue comment` writes comments as:

`**YYYY-MM-DD HH:MM**: <text>` (UTC)

## Example

```md
---
id: issue-abc
status: open
created: 2026-01-20T00:40:26Z
type: task
priority: 0
labels:
  - orchestration
deps:
  - issue-aaa
links:
  - https://example.com/spec
---
# Add issue commands

Implement list/get/ready/create/update/close/comment/commit for tk issues.

## Comments

**2026-01-20 00:42**: first pass landed
```

## CLI Workflow

Create an issue:

`fugue issue create "Title" --project <project> --description "..."`

List and view:

- `fugue issue list --project <project>`
- `fugue issue show <id> --project <project>`
- `fugue issue ready --project <project>`

Update / close / comment:

- `fugue issue update <id> --project <project> --title "..." --status open|closed|blocked`
- `fugue issue close <id> --project <project>`
- `fugue issue comment <id> --project <project> --body "message"`

Commit and push `.fugue/tickets/` (tk only):

`fugue issue commit --project <project>`
