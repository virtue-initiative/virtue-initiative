# Machine-local Android CLI overrides (untracked).
# Sourced by env.sh before the defaults, so anything set here wins.

# Storage-redirected SDK / NDK / caches / JAVA_HOME for this machine.
if [ -f "$HOME/storage/android-sdk/env.sh" ]; then
  # shellcheck disable=SC1091
  . "$HOME/storage/android-sdk/env.sh"
fi
