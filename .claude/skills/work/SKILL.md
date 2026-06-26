---
name: work
description: Start a new feature in a fresh git worktree. Creates an auto-named branch and worktree from staging, runs setup.sh, then enters plan mode for the given prompt. Use when the user says /work "some feature description".
---

# Start new feature worktree

Creates a sibling worktree (next to the current one), runs `setup.sh`, then enters
plan mode with the user's prompt.

## Steps

### 1. Derive a branch name

From the user's prompt args, create a short kebab-case branch name (3–5 words max).
Examples:

- "add device pairing flow" → `device-pairing-flow`
- "fix crash on empty batch" → `fix-empty-batch-crash`
- "refactor crypto module" → `refactor-crypto`

### 2. Create the worktree

Fetch first, then create the branch off `origin/staging`. Worktrees live as siblings
of the current worktree:

```bash
git fetch origin
REPO_PARENT="$(git rev-parse --show-toplevel)/.."
WORKTREE="$REPO_PARENT/<branch>"
git worktree add "$WORKTREE" -b "<branch>" origin/staging
```

If the branch name is already taken, append a short suffix like `-2`.

### 3. Run setup.sh

```bash
"$WORKTREE/scripts/setup.sh"
```

This installs deps, migrates the local DB, and ensures Caddy is running with the dev
server config. Do NOT run `launch.sh` — the user will do that themselves.

### 4. Enter plan mode

Call `EnterPlanMode` and begin planning the feature described in the user's original
prompt. The plan file goes in the default plans location. Explore the new worktree
for relevant code — all source lives under `$WORKTREE`.

Remind the user at the end of planning that they can start the dev server with:

```
./scripts/launch.sh <branch>
```

from the new worktree directory.
