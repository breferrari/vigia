#!/bin/sh
# take-next pre-flight, as one command instead of six hand-run comparisons.
#
# The comparisons and their WHY live in SKILL.md; this file is the mechanism.
# Every trap documented there is carried here in code: everything reads from
# an explicit git ref (default origin/main) and never the working tree, jq
# output is stripped of CR before any comparison (jq emits CRLF on Windows and
# grep -qxF then fails against every line), word boundaries are explicit
# character classes because \b is backspace inside a jq string, and the
# milestone query uses per_page=100 with no --paginate (the --jq filter runs
# once per page and emits one answer per page).
#
# Exit: 0 clean, 1 any mechanical finding. Comparison 5 (untracked SPEC.md §10
# prerequisites) is judgment, not mechanics: its bullets are printed for
# reading and never counted.
#
# Test seams (test-only, never set in a real run): PREFLIGHT_SPEC_FILE and
# PREFLIGHT_ROADMAP_FILE substitute local files for the ref's copies, so a
# mutation ("delete an invariant row", "flip a row's mark") can prove each
# comparison fires. A drift check that cannot report "no drift" has not been
# tested, and neither has one that cannot report drift.
set -u
REF="${PREFLIGHT_REF:-origin/main}"
findings=0
say() { printf '%s\n' "$1"; }
hit() { printf '  DRIFT %s\n' "$1"; findings=$((findings + 1)); }
ok() { printf '  ok    %s\n' "$1"; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

git fetch -q origin 2>/dev/null || true

if [ -n "${PREFLIGHT_SPEC_FILE:-}" ]; then cp "$PREFLIGHT_SPEC_FILE" "$tmp/spec.md"
else git show "$REF:SPEC.md" > "$tmp/spec.md" || exit 2; fi
if [ -n "${PREFLIGHT_ROADMAP_FILE:-}" ]; then cp "$PREFLIGHT_ROADMAP_FILE" "$tmp/roadmap.md"
else git show "$REF:ROADMAP.md" > "$tmp/roadmap.md" || exit 2; fi

gh issue list --state all --limit 200 --json number,title,state,milestone > "$tmp/issues.json" || exit 2
jq -r '.[] | "\(.number)\t\(.state)\t\(.milestone.title // "NONE")\t\(.title)"' "$tmp/issues.json" | tr -d '\r' > "$tmp/issues.tsv"

grep -oE '^\| \*\*I[0-9]+[a-z]?\*\*' "$tmp/spec.md" | grep -oE 'I[0-9]+[a-z]?' | sort -u > "$tmp/spec-invariants.txt"
B='(^|[^A-Za-z0-9])'; A='([^A-Za-z0-9]|$)'

say "1. untracked — spec invariants no issue title names:"
found=0
while IFS= read -r inv; do
  if ! cut -f4 "$tmp/issues.tsv" | grep -qE "${B}${inv}${A}"; then
    hit "$inv is declared by SPEC.md and no issue title names it"; found=1
  fi
done < "$tmp/spec-invariants.txt"
[ "$found" -eq 0 ] && ok "every declared invariant is named by an issue"

say "2. orphan — issue tokens the spec no longer declares:"
found=0
cut -f4 "$tmp/issues.tsv" | grep -oE "${B}I[0-9]+[a-z]?${A}" | grep -oE 'I[0-9]+[a-z]?' | sort -u > "$tmp/issue-invariants.txt"
while IFS= read -r inv; do
  if ! grep -qxF "$inv" "$tmp/spec-invariants.txt"; then
    hit "an issue names $inv and SPEC.md no longer declares it"; found=1
  fi
done < "$tmp/issue-invariants.txt"
[ "$found" -eq 0 ] && ok "no issue names a retired invariant"

say "3. state — roadmap marks vs issue state:"
found=0
grep -oE '^\| *(✅|🔨|⬜) *\|.*\[#[0-9]+\]' "$tmp/roadmap.md" | while IFS= read -r row; do
  n=$(printf '%s' "$row" | grep -oE '\[#[0-9]+\]' | head -1 | tr -dc '0-9')
  state=$(awk -F'\t' -v n="$n" '$1 == n { print $2 }' "$tmp/issues.tsv")
  [ -z "$state" ] && continue
  case "$row" in
    "| ✅"*) [ "$state" = "OPEN" ] && printf '  DRIFT row marked done, issue #%s is open\n' "$n" ;;
    *)      [ "$state" = "CLOSED" ] && printf '  DRIFT row not marked done, issue #%s is closed\n' "$n" ;;
  esac
done > "$tmp/state.out"
if [ -s "$tmp/state.out" ]; then cat "$tmp/state.out"; findings=$((findings + $(wc -l < "$tmp/state.out"))); else ok "every roadmap mark agrees with its issue"; fi

say "4. unfiled — open issues with no milestone (invisible to step 1 forever):"
awk -F'\t' '$2 == "OPEN" && $3 == "NONE" { printf "  DRIFT #%s has no milestone: %s\n", $1, $4 }' "$tmp/issues.tsv" > "$tmp/unfiled.out"
if [ -s "$tmp/unfiled.out" ]; then cat "$tmp/unfiled.out"; findings=$((findings + $(wc -l < "$tmp/unfiled.out"))); else ok "every open issue has a milestone"; fi

say "5. judgment — open SPEC.md §10 bullets (read these; ordering language means a blocker):"
sed -n '/^## 10\./,/^## 11\./p' "$tmp/spec.md" | grep -E '^- \[ \]' | cut -c1-160 | sed 's/^/  /'

say "6. milestone drift — step 1's answer vs the roadmap's section order:"
if [ -z "${PREFLIGHT_SPEC_FILE:-}${PREFLIGHT_ROADMAP_FILE:-}" ]; then
  gh api "repos/{owner}/{repo}/milestones?state=open&per_page=100" > "$tmp/ms.json"
  step1=$(jq -r '[ .[] | select(.open_issues > 0) | select((.description // "") | startswith("Shelf:") | not) | { order: (((.title | [scan("^Phase +([0-9]+)")[]] | first) // "9999") | tonumber), title: .title } ] | sort_by(.order, .title) | .[0].title // empty' "$tmp/ms.json" | tr -d '\r')
  grep -oE '^## Phase [0-9]+.*' "$tmp/roadmap.md" | sed 's/^## //' | tr -d '\r' > "$tmp/order.txt"
  jq -r '.[] | select(.open_issues > 0) | .title' "$tmp/ms.json" | tr -d '\r' > "$tmp/withwork.txt"
  jq -r '.[] | select(.open_issues > 0) | select((.description // "") | startswith("Shelf:")) | .title' "$tmp/ms.json" | tr -d '\r' > "$tmp/shelved.txt"
  roadmap_says=""
  while IFS= read -r s; do
    grep -qxF "$s" "$tmp/withwork.txt" || continue
    grep -qxF "$s" "$tmp/shelved.txt" && continue
    roadmap_says="$s"; break
  done < "$tmp/order.txt"
  if [ "$step1" = "$roadmap_says" ]; then ok "step 1 and the roadmap agree: ${step1:-<no eligible work>}"
  else hit "step 1 says '${step1:-<empty>}', roadmap section order says '${roadmap_says:-<empty>}'"; fi
  LC_ALL=C sort "$tmp/withwork.txt" > "$tmp/ww.s"; LC_ALL=C sort "$tmp/order.txt" > "$tmp/or.s"
  LC_ALL=C comm -23 "$tmp/ww.s" "$tmp/or.s" > "$tmp/ms-orphans.out"
  if [ -s "$tmp/ms-orphans.out" ]; then
    sed 's/^/  DRIFT milestone with work and no roadmap section: /' "$tmp/ms-orphans.out"
    findings=$((findings + $(wc -l < "$tmp/ms-orphans.out")))
  else ok "every open milestone with work has a roadmap section"; fi
else
  say "  (skipped under test seams — needs the live tracker)"
fi

say "7. missing row — issues the roadmap never mentions (the direction the 2026-08-03 sweep found four gaps in):"
found=0
cut -f1 "$tmp/issues.tsv" > "$tmp/nums.txt"
while IFS= read -r n; do
  if ! grep -qE "${B}#${n}${A}" "$tmp/roadmap.md"; then
    title=$(awk -F'\t' -v n="$n" '$1 == n { print $4 }' "$tmp/issues.tsv")
    hit "#$n has no roadmap mention: $title"; found=1
  fi
done < "$tmp/nums.txt"
[ "$found" -eq 0 ] && ok "every issue has a roadmap mention"

if [ "$findings" -eq 0 ]; then say "pre-flight clean"; else say "$findings finding(s) — fix in this pass, not a note"; exit 1; fi
