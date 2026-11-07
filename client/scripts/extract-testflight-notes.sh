#!/usr/bin/env bash
# Prints the "What to Test (iOS)" section of a PR body read from stdin.
# The section runs from its heading to the next heading of the same or higher
# level. HTML comments are removed, \r is stripped, and leading or trailing
# blank lines are trimmed. Exits 1 when the section is missing or empty.
set -euo pipefail

awk '
BEGIN { inside = 0; level = 0; incomment = 0; n = 0; first = 0; last = 0 }
{
  sub(/\r$/, "")

  # Lines inside a fenced code block are content, never headings.
  fence = ($0 ~ /^[ \t]*(```|~~~)/)

  if (!inside) {
    if (!infence && $0 ~ /^#+[ \t]+What to Test \(iOS\)[ \t]*$/) {
      match($0, /^#+/)
      level = RLENGTH
      inside = 1
    }
    if (fence) infence = !infence
    next
  }

  if (!infence && $0 ~ /^#+[ \t]/) {
    match($0, /^#+/)
    if (RLENGTH <= level) exit
  }
  if (fence) infence = !infence

  # Strip HTML comments, including ones that span lines.
  if (incomment) {
    if (index($0, "-->") == 0) next
    $0 = substr($0, index($0, "-->") + 3)
    incomment = 0
  }
  while (index($0, "<!--") > 0) {
    pre = substr($0, 1, index($0, "<!--") - 1)
    rest = substr($0, index($0, "<!--") + 4)
    if (index(rest, "-->") > 0) {
      $0 = pre substr(rest, index(rest, "-->") + 3)
    } else {
      $0 = pre
      incomment = 1
      break
    }
  }

  n++
  buf[n] = $0
  if ($0 ~ /[^ \t]/) {
    if (first == 0) first = n
    last = n
  }
}
END {
  if (first == 0) exit 1
  # App Store Connect rejects "What to Test" longer than 4000 characters.
  total = 0
  for (i = first; i <= last; i++) total += length(buf[i]) + 1
  if (total > 4000) {
    print "What to Test (iOS) is " total " characters; the limit is 4000." > "/dev/stderr"
    exit 1
  }
  for (i = first; i <= last; i++) print buf[i]
}
'
