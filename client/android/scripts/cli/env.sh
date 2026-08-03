# Shared environment for the Virtue Android CLI (scripts/cli/va).
#
# This file is committed and path-parametrized: it only sets sensible
# *defaults*. Anything machine-specific belongs in the override layers below,
# in priority order (later wins):
#
#   1. Defaults in this file.
#   2. scripts/cli/env.local.sh            (untracked, your machine overrides —
#                                            e.g. a storage-redirected SDK setup)
#   3. Variables already exported in your shell.
#
# Nothing here is destructive — every assignment respects a value that is
# already set in the environment.

# ---------------------------------------------------------------------------
# 2. Untracked machine-local overrides, sourced first so the values they set
#    (ANDROID_SDK_ROOT, JAVA_HOME, redirected caches, …) win over the defaults
#    below via the ${VAR:-default} guards.
# ---------------------------------------------------------------------------
if [ -f "${VIRTUE_ANDROID_CLI_DIR:-$(dirname "${BASH_SOURCE[0]:-$0}")}/env.local.sh" ]; then
  # shellcheck disable=SC1091
  . "${VIRTUE_ANDROID_CLI_DIR:-$(dirname "${BASH_SOURCE[0]:-$0}")}/env.local.sh"
fi

# ---------------------------------------------------------------------------
# 1. Defaults (only applied if still unset after the override above).
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
