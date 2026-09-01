---
name: team-branches
description: >
  How named agents share a git repository without trampling each other: wip/
  branches, worktrees, joining a teammate's task, merging, and deleting the
  leftover branch. Load before editing a repo another agent also works in.
---

# Team git: isolate, talk, finish, delete

Several agents on one checkout is how work disappears. The shared tree is for
the default branch (deploy, status). Your edits live in a worktree on a branch
you own. When the task is done the branch is merged and deleted. Leftover
`wip/` branches are garbage.

## Branch strategy

- **Default** (never commit here as a named agent): `main`, `master`, or `dev`
  — whichever `origin/HEAD` points at. Detect; do not guess.
- **Your work**: `wip/<you>/<task>` (lowercase, hyphens). Example:
  `wip/razvan/fake-detection`.
- **Protected**: `main` `master` `dev` `develop` `production` `prod` `staging`.
  Never commit, never force-push, never `reset --hard` these.
- **No long-lived feature branches.** A `wip/` lives for one task. When the
  task is done it is gone.

Prefer the tools: `repo_status`, `repo_start`, `repo_sync`, `repo_finish`,
`repo_save`. If you must use git directly, the commands below are the same
policy.

## Start (before you edit)

```
repo_start   # or, by hand:
git fetch origin
git worktree add -b wip/<you>/<task> <repo>/.worktrees/<you>-<task> origin/<default>
```

Work only inside the path `repo_start` printed. `cd` there for every command.
Tell the team, once, with `agent_send`: branch name and the files you will
touch.

If `repo_status` already shows a teammate's `wip/` covering those files: do
**not** open a second branch. `agent_send` them, join their worktree, or wait.
Two rewrites of the same file is how one of you gets deleted.

## During

- `repo_save` often. Uncommitted work in a worktree dies with the machine.
- `repo_sync` after a teammate merges, or when you have been away: fetch and
  rebase onto the default. Conflicts are yours to fix, not to force through.
- Never `git checkout main` in the shared tree "just to ship". If the shared
  tree is dirty it belongs to someone else.

## Finish (the task is actually done)

```
repo_finish          # push, merge if the main checkout is free, delete worktree + branch
repo_finish merge=false   # push and drop the worktree; leave the remote for the lead
```

A leftover worktree or `wip/` branch after "done" is a bug. Delete it.

If the main checkout is dirty or not on the default branch, `repo_finish`
pushes and removes *your* worktree, then tells the lead to merge. Do not
"fix" that by checking out the default in the dirty tree.

## Join a teammate

1. `repo_status` — read who is on what.
2. `agent_send` — "I need `app/Foo.php`; you're on `wip/ada/design`. Can I
   take it / should I wait / should I edit in your worktree?"
3. Only start `wip/<you>/...` if they say the area is free.

## GitOps in one line

Default branch is production-shaped. You never push to it directly. You push
`wip/<you>/<task>`, someone (you, if the tree is free; otherwise the lead)
merges, then the branch is deleted. That is the whole strategy.
