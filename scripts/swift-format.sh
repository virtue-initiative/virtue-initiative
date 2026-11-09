#!/usr/bin/env bash

# Formats every tracked Swift file with swift-format, using the repo-root
# .swift-format config.
#
# Usage: swift-format.sh [--check]
#
#   --check   change nothing; fail if any file isn't formatted or breaks a
#             lint rule (what CI runs)
#
# swift-format ships with Xcode 16+ and every Swift 6 toolchain. CI pins the
# version with the swift:6.1.2 image (.github/workflows/swift-format.yml),
# since its output can shift between releases.

set -euo pipefail

cd "$(dirname "$0")/.."

if command -v swift-format >/dev/null 2>&1; then
  swift_format=(swift-format)
elif command -v xcrun >/dev/null 2>&1 && xcrun --find swift-format >/dev/null 2>&1; then
  swift_format=(xcrun swift-format)
elif command -v swift >/dev/null 2>&1; then
  swift_format=(swift format)
else
  echo "swift-format not found. Install Xcode 16+ or a Swift 6 toolchain." >&2
  exit 1
fi

# Only tracked files, so build output (client/target, .build, DerivedData)
# and checked-out packages like Sparkle are never touched.
if [ "${1:-}" = "--check" ]; then
  if ! git ls-files -z '*.swift' | xargs -0 "${swift_format[@]}" lint --strict --parallel; then
    echo "Swift style check failed. Run \`just swift-format\` to fix formatting." >&2
    exit 1
  fi
else
  git ls-files -z '*.swift' | xargs -0 "${swift_format[@]}" format --in-place --parallel
fi
