# Shared environment for the Android dev scripts (build.sh, install.sh, etc).
#
# This file is committed and path-parametrized: it only sets sensible
# *defaults*. Anything machine-specific belongs in the override layers below,
# in priority order (later wins):
#
#   1. Defaults in this file.
#   2. ~/.config/virtue-dev.env  (untracked, shared across every worktree on
#                                 this machine — see AGENTS.md)
#   3. .env at the repo root     (untracked, this worktree's overrides — e.g.
#                                 a storage-redirected SDK setup; see AGENTS.md
#                                 and .env.example)
#   4. Variables already exported in your shell.
#
# Nothing here is destructive — every assignment respects a value that is
# already set in the environment.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

# ---------------------------------------------------------------------------
# 2 & 3. Machine-wide shared config, then this worktree's override on top of
#    it, sourced before the shell-exported vars below still win.
# ---------------------------------------------------------------------------
VIRTUE_DEV_ENV="${VIRTUE_DEV_ENV:-$HOME/.config/virtue-dev.env}"
if [ -f "$VIRTUE_DEV_ENV" ]; then
  set -a
  # shellcheck disable=SC1090
  . "$VIRTUE_DEV_ENV"
  set +a
fi
if [ -f "$REPO_ROOT/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$REPO_ROOT/.env"
  set +a
fi

# ---------------------------------------------------------------------------
# 1. Defaults (only applied if still unset after the overrides above).
# ---------------------------------------------------------------------------
export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}"
export ANDROID_HOME="${ANDROID_HOME:-$ANDROID_SDK_ROOT}"
export ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-$ANDROID_SDK_ROOT/ndk/26.1.10909125}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_NDK_ROOT}"

# Make sure the SDK tools are reachable.
for _dir in \
  "$ANDROID_SDK_ROOT/cmdline-tools/latest/bin" \
  "$ANDROID_SDK_ROOT/platform-tools" \
  "$ANDROID_SDK_ROOT/emulator"; do
  case ":$PATH:" in
    *":$_dir:"*) ;;
    *) [ -d "$_dir" ] && PATH="$_dir:$PATH" ;;
  esac
done
unset _dir
export PATH

# App + emulator coordinates this project uses.
export VIRTUE_AVD="${VIRTUE_AVD:-virtue_api35}"
export VIRTUE_PACKAGE="${VIRTUE_PACKAGE:-org.virtueinitiative.virtue}"
export VIRTUE_ACTIVITY="${VIRTUE_ACTIVITY:-$VIRTUE_PACKAGE/.MainActivity}"
