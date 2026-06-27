---
name: work
description: Start a new feature in a fresh git worktree. Creates an auto-named branch and worktree from staging, runs setup.sh, then spawns a fresh Claude Code instance in a new tmux window. Use when the user says /work "some feature description".
---

# Start new feature worktree

Creates a sibling worktree (next to the current one), runs `setup.sh`, then opens a
new tmux window with a fresh Claude Code instance rooted at the worktree.

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

### 4. Compose the initial prompt

Before spawning Claude, expand the user's prompt into a self-contained task description
that gives the new instance enough context to start planning without any prior
conversation. Include:

- What the feature/fix is (paraphrase the user's words)
- The GitHub issue number if one was mentioned (e.g. "See issue #428")
- Any constraint the user stated (e.g. which base branch was used, scope limits)

Do **not** prefix the message with `/work` or any slash command.

### 5. Spawn Claude in a new tmux window

Determine the current tmux session, open a new window rooted at the worktree, start
Claude with plan permissions mode, then send the composed prompt via `send-keys`:

```bash
SESSION=$(tmux display-message -p '#S')
BRANCH=<branch>
PROMPT="<composed prompt>"
tmux new-window -t "$SESSION" -n "$BRANCH" -c "$WORKTREE" "claude --permission-mode plan --dangerously-skip-permissions --remote-control \"$BRANCH\" \"$PROMPT\""
```

After spawning the window, report to the user:

> Worktree ready at `$WORKTREE`. Claude Code is opening in tmux window `$BRANCH`.

Do **not** call `EnterPlanMode` in the current session.
