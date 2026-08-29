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
# Test seams (test-only, never set in a real run): PREFLIGHT_SPEC_FILE,
# PREFLIGHT_ROADMAP_FILE and PREFLIGHT_ISSUES_FILE substitute local files for
# the ref's copies and for the tracker fetch, so a mutation ("delete an
# invariant row", "flip a row's mark", "hand it a board at the cap") can prove
# each comparison fires. A drift check that cannot report "no drift" has not
# been tested, and neither has one that cannot report drift.
set -u
REF="${PREFLIGHT_REF:-origin/main}"
# Five of the seven comparisons read the tracker fetch, so a short one is not
# one defect but five. Overridable the way REF is, and for the same reason:
# selftest.sh proves the guard below fires without pulling a thousand-issue
# fixture through comparison 7, and a drift case there pins this default.
ISSUE_LIMIT="${PREFLIGHT_ISSUE_LIMIT:-1000}"
findings=0
say() { printf '%s\n' "$1"; }
hit() { printf '  DRIFT %s\n' "$1"; findings=$((findings + 1)); }
ok() { printf '  ok    %s\n' "$1"; }

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if [ -z "${PREFLIGHT_SPEC_FILE:-}${PREFLIGHT_ROADMAP_FILE:-}" ]; then
  git fetch -q origin 2>/dev/null || true
fi

if [ -n "${PREFLIGHT_SPEC_FILE:-}" ]; then cp "$PREFLIGHT_SPEC_FILE" "$tmp/spec.md"
else git show "$REF:SPEC.md" > "$tmp/spec.md" || exit 2; fi
if [ -n "${PREFLIGHT_ROADMAP_FILE:-}" ]; then cp "$PREFLIGHT_ROADMAP_FILE" "$tmp/roadmap.md"
else git show "$REF:ROADMAP.md" > "$tmp/roadmap.md" || exit 2; fi

if [ -n "${PREFLIGHT_ISSUES_FILE:-}" ]; then cp "$PREFLIGHT_ISSUES_FILE" "$tmp/issues.json"
else gh issue list --state all --limit "$ISSUE_LIMIT" --json number,title,state,milestone > "$tmp/issues.json" || exit 2; fi
jq -r '.[] | "\(.number)\t\(.state)\t\(.milestone.title // "NONE")\t\(.title)"' "$tmp/issues.json" | tr -d '\r' > "$tmp/issues.tsv"

grep -oE '^\| \*\*I[0-9]+[a-z]?\*\*' "$tmp/spec.md" | grep -oE 'I[0-9]+[a-z]?' | sort -u > "$tmp/spec-invariants.txt"
B='(^|[^A-Za-z0-9])'; A='([^A-Za-z0-9]|$)'

# 0. Machine state. A measurement window in flight on this machine means two
# instruments would measure each other — the soak workflow's own concurrency
# rule, applied locally. A soak log touched in the last ten minutes is the
# cheapest evidence of one; this fires rarely and loudly, and it is advisory
# because it is not tracker drift: the pass decides what to defer (release-tier
# timing runs, above all), the check only makes the claim visible. It exists
# because a 24-hour window was once recorded only in a commit message, and a
# parallel session ruled the same run unauthorised while it was in flight.
# Every worktree's target/ is swept, not this one's: sessions run in per-issue
# worktrees, a soak runs in whichever one launched it, and a check that only
# looked beside itself would miss the one machine-level fact it exists to see.
# The signal is the PROCESS, not a log's mtime. The first version watched for a
# soak log touched in the last ten minutes, and the real soak falsified it
# within the hour: libtest prints its over-60-seconds notice once and then the
# log sits silent for the whole window, so a 24-hour run in flight showed a
# 69-minute-old file while its test process had 1,336 CPU-seconds on the clock.
# A check that can say "ok" while the window runs is worse than none. The test
# binary is named soak-<hash>, which is what both probes match.
say "0. machine state:"
soak_proc=""
if command -v tasklist >/dev/null 2>&1; then
  tasklist 2>/dev/null | grep -qiE '^soak-' && soak_proc="yes"
elif command -v pgrep >/dev/null 2>&1; then
  pgrep -f '[/\\]soak-[0-9a-f]' >/dev/null 2>&1 && soak_proc="yes"
fi
if [ -n "$soak_proc" ]; then
  say "  CLAIMED a soak test process is running on this machine."
  say "          Defer release-tier timing runs; check the window's issue for when it ends."
else
  say "  ok    no measurement window in flight"
fi

# The board every comparison below reads, and whether all of it arrived. A short
# fetch is invisible from inside a comparison: 1 reports drift that is not there,
# and 2, 4 and 7 under-report in silence. This is a precondition rather than an
# eighth comparison, which is why it carries no number: the seven compare two
# records against each other, and this one asks whether one of the records is
# all here. Counted from the JSON rather than from the TSV's line count, because
# a title carrying a newline would make the cheaper form over-count and mask the
# very case this guards.
#
# `gh issue list --limit N` pages internally until N is satisfied, and SKILL.md's
# `--paginate` trap does not reach it: that one is about `gh api --paginate --jq`
# running the filter once per page, and the filter here is a separate jq over the
# finished file. So the ceiling is free to be generous, and what makes it safe is
# this guard rather than its height.
say "the board:"
issues=$(jq 'length' "$tmp/issues.json")
if [ "$issues" -ge "$ISSUE_LIMIT" ]; then
  hit "the fetch returned $issues issues against a limit of $ISSUE_LIMIT, so every comparison below is reading a truncated board"
else
  ok "all $issues issues fetched, under a limit of $ISSUE_LIMIT"
fi

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
  # A row citing an issue the board does not have used to `continue`, which is the
  # same silence #369 was about and not the same cause: truncation is one way to
  # get here, and a deleted issue, a transferred one and a mistyped `#N` are three
  # more that no fetch size would fix. The board guard above cannot see any of
  # them, because it counts what arrived rather than what was asked for.
  if [ -z "$state" ]; then
    printf '  DRIFT row cites #%s, which the tracker does not have\n' "$n"
    continue
  fi
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
  # Shelved milestones are exempt from the section check: a shelf holds no
  # place in the take-order, so it owes the file no `## Phase <n>` section —
  # the Shelf milestone (titled "Phase 5" until 2026-08-06) is the standing case.
  LC_ALL=C sort "$tmp/withwork.txt" > "$tmp/ww.all"
  LC_ALL=C sort "$tmp/shelved.txt" > "$tmp/sh.s"
  LC_ALL=C comm -23 "$tmp/ww.all" "$tmp/sh.s" > "$tmp/ww.s"
  LC_ALL=C sort "$tmp/order.txt" > "$tmp/or.s"
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
