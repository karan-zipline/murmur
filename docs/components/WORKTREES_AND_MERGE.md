# Git Worktrees, Branches, and Merge Pipeline

Murmur isolates agent work in git worktrees and uses a direct merge pipeline on completion.

Code pointers:
- Git adapter: `crates/murmur/src/git.rs`
- Worktrees: `crates/murmur/src/worktrees.rs`
- Merge logic: `crates/murmur/src/daemon/merge.rs`
- Completion pipeline: `crates/murmur/src/daemon/rpc/agent.rs` (`agent done`)
- Branch cleanup CLI: `crates/murmur/src/main.rs` (`branch cleanup`)

---

## Repo Clone + Worktree Layout

When you add a project, Murmur clones it into:

`projects/<project>/repo/`

Agents run in worktrees under:

`projects/<project>/worktrees/wt-<agent-id>/`

---

## Default Branch Detection

Murmur determines the default branch by:
1. parsing `git remote show origin` for “HEAD branch”
2. falling back to `main`, then `master`

This is used for:
- worktree base branch selection
- merge pipeline base branch selection
- branch cleanup base ref selection (typically `origin/main`)

---

## Branch Naming

Agent branches are named:

`murmur/<agent-id>`

Remote agent branches are:

`origin/murmur/<agent-id>`

---

## Merge Strategy

On `agent done`, Murmur runs one of two merge strategies (configured per project via `merge-strategy`).

### Direct

With `merge-strategy = "direct"`, Murmur runs a direct merge pipeline:

1. `git fetch --prune origin`
2. checkout and hard reset local default branch to `origin/<default>`
3. rebase the agent worktree onto `origin/<default>`
4. fast-forward merge `murmur/<agent-id>` into the project repo default branch
5. push the default branch back to `origin`

If rebase fails:
- Murmur reports a conflict
- the agent transitions to `needs_resolution`
- the worktree is kept for manual conflict resolution

Merge operations are serialized per project to avoid concurrent merges racing.

### Pull Request (GitHub)

With `merge-strategy = "pull-request"`, Murmur prepares a PR instead of merging into the default branch:

1. `git fetch --prune origin`
2. rebase the agent worktree onto `origin/<default>`
3. force-push the agent branch (`murmur/<agent-id>`) to `origin` (`--force-with-lease`)
4. create a PR via GitHub GraphQL (using the project `remote-url` to determine the repo NWO)
5. stop the agent process but keep the worktree around for follow-ups

If rebase fails:
- Murmur reports a conflict
- the agent transitions to `needs_resolution`
- the worktree is kept for manual conflict resolution

Notes:
- This strategy does **not** update the default branch locally or on `origin`; it only pushes the agent branch and creates the PR.
- A GitHub token is required (`GITHUB_TOKEN`/`GH_TOKEN` or `[providers.github].token`).

---

## Branch Cleanup

`mm branch cleanup` deletes merged `murmur/*` branches.

Detection handles rebased merges:
- uses `git merge-base <branch> <base-ref>`
- uses `git cherry <base-ref> <branch> <merge-base>` and checks for any `+` commits

Behavior:
- by default, deletes remote branches (`origin/murmur/*`)
- `--local` also deletes local branches (`murmur/*`)
- `--dry-run` prints what would be deleted

---

## Worktree Removal

Worktrees are removed with:

`git worktree remove --force <worktree-dir>`

Murmur uses this for:
- agent cleanup (delete)
- planner stop (stop)
- project remove `--delete-worktrees`
