#!/bin/sh
# Names the roadmap section the next task is taken from, then lists its open
# issues.
#
# The order is ROADMAP.md's section order on origin/main, which is versioned and
# reviewed. A section is eligible when one of its rows is not marked done and
# names an open issue, so a finished section is never an answer. The walk stops
# at `## Shelf`: nothing at or below it is taken. Milestones play no part, so a
# renamed or renumbered milestone cannot change which task comes next.
#
# A row names its issue in its last cell. Some rows cite a second issue in their
# task prose, so the last link on the line is the one read.
#
# Usage: next.sh [--ranked]
#   --ranked  print every eligible section in take order, not only the first
#
# Test seams: NEXT_ROADMAP_FILE substitutes a file for the roadmap, and
# NEXT_ISSUES_FILE a JSON array of `{number, title}` for the open issues.
set -eu

REF="${NEXT_REF:-origin/main}"
LIMIT=1000
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if [ -n "${NEXT_ROADMAP_FILE:-}" ]; then tr -d '\r' < "$NEXT_ROADMAP_FILE" > "$tmp/roadmap.md"
else git show "$REF:ROADMAP.md" | tr -d '\r' > "$tmp/roadmap.md"; fi

if [ -n "${NEXT_ISSUES_FILE:-}" ]; then cp "$NEXT_ISSUES_FILE" "$tmp/open.json"
else gh issue list --state open --limit "$LIMIT" --json number,title > "$tmp/open.json"; fi
# A short fetch would make an open issue look closed and skip its section.
if [ "$(jq 'length' "$tmp/open.json")" -ge "$LIMIT" ]; then
  echo "the open-issue fetch reached its limit of $LIMIT, so the answer would be wrong" >&2
  exit 2
fi
jq -r '.[] | "\(.number)\t\(.title)"' "$tmp/open.json" | tr -d '\r' > "$tmp/open.tsv"

# One line per eligible row: the section heading, a tab, the issue number.
awk -F'\t' '
  FILENAME == ARGV[1] { open[$1] = 1; next }
  /^## Shelf/ { exit }
  /^## / { section = substr($0, 4); next }
  section == "" { next }
  /^\| *(⬜|🔨) *\|.*\[#[0-9]+\]\([^)]*\) *\|$/ {
    line = $0
    n = ""
    while (match(line, /\[#[0-9]+\]/)) { n = substr(line, RSTART + 2, RLENGTH - 3); line = substr(line, RSTART + RLENGTH) }
    if (n in open) print section "\t" n
  }' "$tmp/open.tsv" "$tmp/roadmap.md" > "$tmp/rows.tsv"

if [ "${1:-}" = "--ranked" ]; then
  cut -f1 "$tmp/rows.tsv" | awk '!seen[$0]++'
  exit 0
fi

section=$(cut -f1 "$tmp/rows.tsv" | head -n 1)
if [ -z "$section" ]; then
  echo "no eligible section: every row above the Shelf is done or names a closed issue. Read which before acting."
  exit 0
fi
printf 'section: %s\n' "$section"
awk -F'\t' -v s="$section" '
  FILENAME == ARGV[1] { title[$1] = $2; next }
  $1 == s { printf "#%s\t%s\n", $2, title[$2] }' "$tmp/open.tsv" "$tmp/rows.tsv"
