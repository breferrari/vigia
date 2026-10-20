#!/bin/sh
# Names the roadmap section the next task is taken from, then lists its open
# issues.
#
# The order is ROADMAP.md's section order on origin/main. The walk runs from the
# first `## Phase` heading to `## Shelf`, and a section is eligible when one of
# its rows is not marked done and names an open issue. Milestones are not read.
#
# A row names its issue in its last cell. Some rows cite a second issue in their
# task prose, so the last link on the line is the one read.
#
# Usage: next.sh [--ranked | --rows]
#   --ranked  print every eligible section in take order, not only the first
#   --rows    print every row on an open issue as `section<TAB>issue<TAB>wait`,
#             in order, where wait is 1 for a section waiting on its release
#
# A section whose heading names a release, as in "right after 1.2.0", waits
# until that release's tag is on origin, and nothing after it is taken first.
#
# Test seams: NEXT_ROADMAP_FILE substitutes a file for the roadmap,
# NEXT_ISSUES_FILE a JSON array of `{number, title}` for the open issues, and
# NEXT_TAGS_FILE one tag per line for origin's tags.
set -eu

REF="${NEXT_REF:-origin/main}"
LIMIT=1000
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if [ -n "${NEXT_ROADMAP_FILE:-}" ]; then cp "$NEXT_ROADMAP_FILE" "$tmp/raw.md"
else
  # A stale ref answers from an order the reader has since changed.
  git fetch -q origin main || exit 2
  git show "$REF:ROADMAP.md" > "$tmp/raw.md" || exit 2
fi
tr -d '\r' < "$tmp/raw.md" > "$tmp/roadmap.md"

if [ -n "${NEXT_ISSUES_FILE:-}" ]; then cp "$NEXT_ISSUES_FILE" "$tmp/open.json"
else gh issue list --state open --limit "$LIMIT" --json number,title > "$tmp/open.json"; fi
# A short fetch would make an open issue look closed and skip its section.
if [ "$(jq 'length' "$tmp/open.json")" -ge "$LIMIT" ]; then
  echo "the open-issue fetch reached its limit of $LIMIT, so the answer would be wrong" >&2
  exit 2
fi
jq -r '.[] | "\(.number)\t\(.title)"' "$tmp/open.json" | tr -d '\r' > "$tmp/open.tsv"

if [ -n "${NEXT_TAGS_FILE:-}" ]; then tr -d '\r' < "$NEXT_TAGS_FILE" > "$tmp/tags.txt"
elif grep -qE '^## .*after [0-9]+\.[0-9]+\.[0-9]+' "$tmp/roadmap.md"; then
  git ls-remote --tags origin > "$tmp/remote.txt" || exit 2
  sed -n 's|.*refs/tags/\([^^]*\).*|\1|p' "$tmp/remote.txt" > "$tmp/tags.txt"
else : > "$tmp/tags.txt"; fi

# One line per eligible row: the section heading, a tab, the issue number.
awk -F'\t' '
  FILENAME == ARGV[1] { tagged[$0] = 1; next }
  FILENAME == ARGV[2] { open[$1] = 1; next }
  /^## Shelf/ { exit }
  /^## Phase / { walking = 1 }
  /^## / {
    section = substr($0, 4)
    waiting = match(section, /after [0-9]+\.[0-9]+\.[0-9]+/) && !(("v" substr(section, RSTART + 6, RLENGTH - 6)) in tagged)
    next
  }
  !walking { next }
  /^\| *(⬜|🔨) *\|.*\[#[0-9]+\]\([^)]*\) *\| *$/ {
    line = $0
    n = ""
    while (match(line, /\[#[0-9]+\]/)) { n = substr(line, RSTART + 2, RLENGTH - 3); line = substr(line, RSTART + RLENGTH) }
    if (n in open) print section "\t" n "\t" (waiting ? 1 : 0)
  }' "$tmp/tags.txt" "$tmp/open.tsv" "$tmp/roadmap.md" > "$tmp/rows.tsv"

case "${1:-}" in
  --rows) cat "$tmp/rows.tsv"; exit 0 ;;
esac
awk -F'\t' '$3 == 1 { exit } { print }' "$tmp/rows.tsv" > "$tmp/ready.tsv"
if [ "${1:-}" = "--ranked" ]; then
  cut -f1 "$tmp/ready.tsv" | awk '!seen[$0]++'
  exit 0
fi

section=$(cut -f1 "$tmp/ready.tsv" | head -n 1)
waiting=$(awk -F'\t' '$3 == 1 { print $1; exit }' "$tmp/rows.tsv")
if [ -z "$section" ] && [ -n "$waiting" ]; then
  printf 'waiting: %s, until its release is tagged\n' "$waiting"
  exit 0
fi
if [ -z "$section" ]; then
  echo "no eligible section: every row above the Shelf is done or names a closed issue. Read which before acting."
  exit 0
fi
printf 'section: %s\n' "$section"
awk -F'\t' -v s="$section" '
  FILENAME == ARGV[1] { title[$1] = $2; next }
  $1 == s { printf "#%s\t%s\n", $2, title[$2] }' "$tmp/open.tsv" "$tmp/ready.tsv"
