#!/usr/bin/env bash
set -euo pipefail

if [ $# -lt 1 ]; then
  echo "Usage: $0 <last-release-pr-number>" >&2
  echo "Example: $0 345    (or: $0 '#345')" >&2
  exit 1
fi

LAST_PR="${1#\#}"

MERGED_AT=$(gh pr view "$LAST_PR" --json mergedAt --jq .mergedAt)
if [ -z "$MERGED_AT" ] || [ "$MERGED_AT" = "null" ]; then
  echo "PR #$LAST_PR is not merged (no mergedAt timestamp)." >&2
  exit 1
fi

MERGED_DATE="${MERGED_AT%%T*}"

PRS_JSON=$(gh pr list \
  --base staging \
  --state merged \
  --search "merged:>=$MERGED_DATE" \
  --limit 500 \
  --json number,title,url,mergedAt \
  --jq "[.[] | select(.mergedAt > \"$MERGED_AT\")] | sort_by(.mergedAt)")

COUNT=$(echo "$PRS_JSON" | jq 'length')
if [ "$COUNT" -eq 0 ]; then
  echo "No PRs merged to staging since #$LAST_PR ($MERGED_AT)." >&2
  exit 0
fi

echo "$PRS_JSON" | jq -c '.[]' | while read -r pr; do
  number=$(echo "$pr" | jq -r .number)
  title=$(echo "$pr" | jq -r .title)
  url=$(echo "$pr" | jq -r .url)
  printf -- '- [%s](%s)\n' "$title" "$url"
  gh pr view "$number" \
    --json closingIssuesReferences \
    --jq '.closingIssuesReferences[]?.number' | while read -r issue_num; do
      issue=$(gh issue view "$issue_num" --json title,url)
      issue_title=$(echo "$issue" | jq -r .title)
      issue_url=$(echo "$issue" | jq -r .url)
      printf '  - [%s](%s)\n' "$issue_title" "$issue_url"
    done
done
