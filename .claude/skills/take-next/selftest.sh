#!/usr/bin/env sh
# Self-test for step 1's section selection and pre-flight comparison 6.
#
# Why this exists: the selection rule is a jq filter inside a shell script, so
# no cargo test can reach it, and the first two edits to it were verified by
# hand into a GitHub comment. That verification died with the shell it ran in.
# CLAUDE.md's rule is that an invariant without a failing test is a wish, and
# this is the cheapest thing that makes it one.
#
# It is offline and hermetic: every case is a fixture, so it needs no network,
# no `gh`, and no particular state of the tracker. Run it after any edit to
# next.sh, preflight.sh, or the rules beside them in SKILL.md.
#
#   sh .claude/skills/take-next/selftest.sh
#
# Requires `jq` only, which next.sh already requires.

set -u
FAIL=0
HERE="$(dirname "$0")"
SKILL="$HERE/SKILL.md"
NEXT="$HERE/next.sh"
PRE="$HERE/preflight.sh"
FIX="$(mktemp -d)"
trap 'rm -rf "$FIX"' EXIT

ok() { printf '  ok   %s\n' "$1"; }
no() { printf '  FAIL %s\n    expected: %s\n    actual:   %s\n' "$1" "$2" "$3"; FAIL=$((FAIL + 1)); }

# Every case drives next.sh through its fixture seams, so what is tested is what
# a session runs rather than a copy of it. A fixture roadmap is written row by
# row: `## Heading` starts a section, `⬜ N` and `✅ N` are rows naming issue N.
roadmap() { # lines -> roadmap.md
  printf '%s\n' "$@" | awk '
    /^## / { print; print "| | Task | Issue |"; print "|---|---|---|"; next }
    { printf "| %s | task %s | [#%s](https://example.invalid/%s) |\n", $1, $2, $2, $2 }' > "$FIX/roadmap.md"
}
open_issues() { # numbers -> open.json
  printf '%s\n' "$@" | jq -R 'select(length > 0) | {number: tonumber, title: "issue \(.)"}' | jq -s . > "$FIX/open.json"
}
: > "$FIX/tags.txt"
next_on() { # args -> next.sh's output on the fixtures
  NEXT_ROADMAP_FILE="$FIX/roadmap.md" NEXT_ISSUES_FILE="$FIX/open.json" NEXT_TAGS_FILE="$FIX/tags.txt" sh "$NEXT" "$@" 2>&1 | tr -d '\r'
}
take() { # -> the section next.sh names
  next_on | sed -n 's/^section: //p'
}
case_is() { # name, expected
  actual=$(take)
  [ "$actual" = "$2" ] && ok "$1" || no "$1" "$2" "$actual"
}

echo "selection:"

roadmap "## Phase 6 - measured" "⬜ 1" "## Phase 7 - distribution" "⬜ 2"; open_issues 1 2
case_is "file order" "Phase 6 - measured"

roadmap "## Phase 10 - rust" "✅ 1" "## Foundation" "⬜ 2" "## Phase 11 - paint" "⬜ 3"; open_issues 2 3
case_is "non-phase section" "Foundation"

roadmap "## Phase 10 - rust" "✅ 1" "## Foundation, right after 1.2.0" "⬜ 2" "## Phase 11 - paint" "⬜ 3"; open_issues 2 3
printf 'v1.1.0\n' > "$FIX/tags.txt"
case_is "release not tagged" ""
waited=$(next_on | head -n 1)
[ "$waited" = "waiting: Foundation, right after 1.2.0, until its release is tagged" ] && ok "waiting answer" \
  || no "waiting answer" "waiting: Foundation, right after 1.2.0, until its release is tagged" "$waited"
printf 'v1.1.0\nv1.2.0\n' > "$FIX/tags.txt"
case_is "release tagged" "Foundation, right after 1.2.0"
: > "$FIX/tags.txt"

roadmap "## Phase 6 - measured" "✅ 1" "## Phase 7 - distribution" "⬜ 2"; open_issues 1 2
case_is "done section skipped" "Phase 7 - distribution"

roadmap "## Phase 6 - measured" "⬜ 1" "## Phase 7 - distribution" "⬜ 2"; open_issues 2
case_is "closed issue skipped" "Phase 7 - distribution"

roadmap "## Phase 6 - measured" "🔨 1" "## Phase 7 - distribution" "⬜ 2"; open_issues 1 2
case_is "in-progress row" "Phase 6 - measured"

roadmap "## Phase 6 - measured" "✅ 1" "## Shelf" "⬜ 2" "## Phase 9 - later" "⬜ 3"; open_issues 2 3
case_is "shelf stops walk" ""

roadmap "## Phase 6 - measured" "✅ 1"; open_issues
case_is "finished roadmap" ""

: > "$FIX/roadmap.md"; open_issues 1
case_is "empty roadmap" ""

roadmap "## Releases" "⬜ 1" "## Phase 6 - measured" "⬜ 2"; open_issues 1 2
case_is "walk starts at phase" "Phase 6 - measured"

printf '%s\n' "## Phase 6 - measured" "| | Task | Issue |" "|---|---|---|" \
  "| ⬜ | Closed by [#1](https://example.invalid/1) | [#2](https://example.invalid/2) |" > "$FIX/roadmap.md"
open_issues 1
case_is "last-cell issue" ""

printf '%s\n' "## Phase 6 - measured" "| | Task | Issue |" "|---|---|---|" \
  "| ⬜ | spaced | [#1](https://example.invalid/1) | " > "$FIX/roadmap.md"
open_issues 1
case_is "trailing space" "Phase 6 - measured"

# --ranked shows the whole order the answer came from.
roadmap "## Phase 6 - measured" "⬜ 1" "## Phase 7 - distribution" "✅ 2" "## Phase 8 - look" "⬜ 3" "⬜ 4"; open_issues 1 3 4
ranked=$(next_on --ranked | paste -sd '|' -)
[ "$ranked" = "Phase 6 - measured|Phase 8 - look" ] && ok "ranked order" \
  || no "ranked order" "Phase 6 - measured|Phase 8 - look" "$ranked"

listed=$(next_on | sed 1d | cut -f1 | paste -sd '|' -)
[ "$listed" = "#1" ] && ok "own issues only" || no "own issues only" "#1" "$listed"

jq -n '[range(0; 1000) | {number: ., title: "x"}]' > "$FIX/open.json"
if NEXT_ROADMAP_FILE="$FIX/roadmap.md" NEXT_ISSUES_FILE="$FIX/open.json" NEXT_TAGS_FILE="$FIX/tags.txt" sh "$NEXT" >/dev/null 2>&1; then
  no "fetch limit refused" "a non-zero exit" "exit 0"
else
  ok "fetch limit refused"
fi

echo "preflight (the board every comparison reads):"

# The tracker fetch every comparison downstream reads, and the roadmap row that
# cites an issue no fetch contains. The two are one section because they are the
# same silence from opposite ends: one is the record arriving short, the other is
# a row pointing outside whatever arrived.
cat > "$FIX/spec.md" <<'EOF'
| **I1** | A thing holds | a budget | a gate |
## 10. Open
## 11. Rulings
EOF

# Every fixture issue is closed, milestoned, named by a roadmap row and, for the
# first, named by the spec, so comparisons 1 to 7 are quiet and each case below
# reads exactly one line moving. The third row cites `#9`, which no `board` call
# produces, and that is the comparison-3 case rather than an oversight.
cat > "$FIX/roadmap.md" <<'EOF'
## Phase 8 - look
| | Task | Issue |
|---|---|---|
| ✅ | one | [#1](https://example.invalid/1) |
| ✅ | two | [#2](https://example.invalid/2) |
| ✅ | absent | [#9](https://example.invalid/9) |
EOF

board() { # count -> issues.json
  jq -n --argjson n "$1" '[range(1; $n + 1) | {
    number: ., state: "CLOSED", milestone: {title: "Phase 8 - look"},
    title: (if . == 1 then "I1: the first" else "issue \(.)" end)
  }]'
}

# One preflight run, filtered to the lines a case is about. `awk '{$1=$1}1'`
# rebuilds the record on single spaces, which is what makes a case pattern
# independent of the column padding `ok` and `hit` align their output on.
runs() { # limit, count, pattern -> the matching lines, space-normalised
  board "$2" > "$FIX/issues.json"
  PREFLIGHT_SPEC_FILE="$FIX/spec.md" \
  PREFLIGHT_ROADMAP_FILE="$FIX/roadmap.md" \
  PREFLIGHT_ISSUES_FILE="$FIX/issues.json" \
  PREFLIGHT_ISSUE_LIMIT="$1" \
    sh "$PRE" 2>&1 | awk -v p="$3" '$0 ~ p { $1 = $1; print }'
}

BOARD='^ +(ok|DRIFT) +(all [0-9]+ issues|the fetch returned)'

line=$(runs 2 2 "$BOARD")
case "$line" in
  "DRIFT the fetch returned 2 issues against a limit of 2"*)
    ok "a board at the fetch limit is reported, not read" ;;
  *) no "a board at the fetch limit is reported, not read" "a DRIFT naming the limit" "$line" ;;
esac

line=$(runs 4 2 "$BOARD")
case "$line" in
  "ok all 2 issues fetched, under a limit of 4"*) ok "a board under the limit reads clean" ;;
  *) no "a board under the limit reads clean" "ok all 2 issues fetched, under a limit of 4" "$line" ;;
esac

# The other end of the same silence, and the one no fetch size fixes: #9 is cited
# by a roadmap row and is in no board, which is what a deleted issue, a
# transferred one and a mistyped number all look like from here.
line=$(runs 4 2 'row cites #')
case "$line" in
  "DRIFT row cites #9, which the tracker does not have"*)
    ok "a roadmap row citing an issue the board lacks is reported" ;;
  *) no "a roadmap row citing an issue the board lacks is reported" \
       "DRIFT row cites #9, which the tracker does not have" "$line" ;;
esac

# A row's issue is the one in its last cell. Six live rows cite a second issue in
# their task prose (`Revoked by`, `Closed by`, `Deferred by`), and reading the
# first link checked those rows against the wrong issue: five agreed by luck,
# both being closed, and the sixth reported drift on a roadmap that was correct.
#
# Its own fixture, because the shared one above is read by two cases that would
# see this row's second link as a board it lacks.
cat > "$FIX/roadmap.md" <<'ROW'
## Phase 8 - look
| | Task | Issue |
|---|---|---|
| ⬜ | deferred by [#1](https://example.invalid/1) | [#2](https://example.invalid/2) |
ROW
jq -n '[{number: 1, state: "CLOSED", milestone: {title: "Phase 8 - look"}, title: "I1: the first"},
        {number: 2, state: "OPEN", milestone: {title: "Phase 8 - look"}, title: "issue 2"}]'   > "$FIX/issues.json"
# One pre-flight run over the fixture files, at a limit the fixtures stay under.
pre() {
  PREFLIGHT_SPEC_FILE="$FIX/spec.md" PREFLIGHT_ROADMAP_FILE="$FIX/roadmap.md" \
  PREFLIGHT_ISSUES_FILE="$FIX/issues.json" PREFLIGHT_ISSUE_LIMIT=10 sh "$PRE" "$@"
}
line=$(pre 2>&1 | awk '/row (not marked done|marked done|cites)/ { $1 = $1; print }')
case "$line" in
  "") ok "a row is checked against the issue in its own cell" ;;
  *)  no "a row is checked against the issue in its own cell" "no drift" "$line" ;;
esac

# Both directions of a mark that disagrees with its issue, and the mentions
# comparison 7 reads: `#1,#2` names both, `a#3`, `#30` and `#1#3` name no #3.
cat > "$FIX/roadmap.md" <<'ROW'
## Phase 8 - look
| | Task | Issue |
|---|---|---|
| ✅ | done early, see #1,#2 | [#2](https://example.invalid/2) |
| ⬜ | left behind | [#1](https://example.invalid/1) |
a#3, #30 and #1#3
ROW
jq -n '[{number: 1, state: "CLOSED", milestone: {title: "Phase 8 - look"}, title: "I1: the first"},
        {number: 2, state: "OPEN", milestone: {title: "Phase 8 - look"}, title: "issue 2"},
        {number: 3, state: "CLOSED", milestone: {title: "Phase 8 - look"}, title: "issue 3"}]' > "$FIX/issues.json"
out=$(pre 2>&1 | awk '/DRIFT/ { $1 = $1; print }')
want='DRIFT row marked done, issue #2 is open
DRIFT row not marked done, issue #1 is closed
DRIFT #3 has no roadmap mention: issue 3'
if [ "$out" = "$want" ]; then ok "marks and mentions drift both ways"
else no "marks and mentions drift both ways" "$want" "$out"; fi

# An empty file on either side is still read as the side it is.
printf '| ✅ | x | [#1](https://example.invalid/1) |\n' > "$FIX/roadmap.md"
echo '[]' > "$FIX/issues.json"
out=$(pre 2>&1 | awk '/row cites/ { $1 = $1; print }')
[ "$out" = "DRIFT row cites #1, which the tracker does not have" ] && ok "an empty board fails every row" || no "an empty board fails every row" "row cites #1" "$out"
echo '## no rows' > "$FIX/roadmap.md"
jq -n '[{number: 1, state: "CLOSED", milestone: {title: "Phase 8 - look"}, title: "I1: the first"}]' > "$FIX/issues.json"
out=$(pre 2>&1 | awk '/roadmap mention/ { $1 = $1; print }')
[ "$out" = "DRIFT #1 has no roadmap mention: I1: the first" ] && ok "a roadmap with no mentions fails every issue" || no "a roadmap with no mentions fails every issue" "#1 has no roadmap mention" "$out"

# Comparison 8, from the case that filed it: a sibling worktree on `single-297`
# and a plan comment, with `issue-2970`, `issue-1297` and a closing comment beside them.
printf 'worktree C:/Dev/vigia on main\nworktree C:/Dev/vigia.b on single-297\nbranch single-297\nbranch origin/issue-2970-x\nbranch issue-1297-y\n' > "$FIX/refs.txt"
jq -n '{comments: [
  {author: {login: "a"}, createdAt: "t1", body: "Plan (pre-approved for this run): x"},
  {author: {login: "b"}, createdAt: "t2", body: "Shipped in abc. The plan held."},
  {author: {login: "c"}, createdAt: "t3", body: "\n**Approved plan, 2026-09-29.**"}]}' > "$FIX/comments.json"
flight() { # issue -> comparison 8's lines, space-normalised
  PREFLIGHT_SPEC_FILE="$FIX/spec.md" PREFLIGHT_ROADMAP_FILE="$FIX/roadmap.md" PREFLIGHT_ISSUES_FILE="$FIX/issues.json" \
  PREFLIGHT_REFS_FILE="$FIX/refs.txt" PREFLIGHT_COMMENTS_FILE="$FIX/comments.json" PREFLIGHT_ISSUE_LIMIT=10 \
    sh "$PRE" "$1" 2>&1 | awk '/^[0-9]+\./ { on = /^8\./; next } on && /^ / { $1 = $1; print }'
}
out=$(flight 297)
want='DRIFT worktree C:/Dev/vigia.b on single-297
DRIFT branch single-297
DRIFT a plan comment by a, t1
DRIFT a plan comment by c, t3'
[ "$out" = "$want" ] && ok "an issue in flight is reported" || no "an issue in flight is reported" "$want" "$out"
echo '{"comments": []}' > "$FIX/comments.json"
out=$(flight 29)
[ "$out" = "ok no worktree, branch or plan names #29" ] && ok "a free issue reads clean" || no "a free issue reads clean" "ok" "$out"
out=$(pre 2>&1 | grep -c '^8\.')
[ "$out" = 0 ] && ok "no issue, no comparison 8" || no "no issue, no comparison 8" "0" "$out"

# Comparison 6 in both directions.
milestones() { # json, open issue numbers -> the comparison-6 lines
  printf '%s' "$1" > "$FIX/ms.json"
  shift
  board 3 | jq --arg open "$*" '($open | split(" ") | map(tonumber)) as $o
    | map(if (.number | IN($o[])) then .state = "OPEN" else . end)' > "$FIX/issues.json"
  PREFLIGHT_SPEC_FILE="$FIX/spec.md" \
  PREFLIGHT_ROADMAP_FILE="$FIX/roadmap6.md" \
  PREFLIGHT_ISSUES_FILE="$FIX/issues.json" \
  PREFLIGHT_MILESTONES_FILE="$FIX/ms.json" \
  PREFLIGHT_ISSUE_LIMIT=10 \
    sh "$PRE" 2>&1 | awk '/^6\./ { on = 1; next } /^[0-9]+\./ { on = 0 } on && /DRIFT|ok/ { $1 = $1; print }'
}
printf '%s\n' "## Phase 8 - look" "| | Task | Issue |" "|---|---|---|" "| ✅ | one | [#1](https://example.invalid/1) |" > "$FIX/roadmap6.md"

line=$(milestones '[{"title":"Phase 8 - look","state":"open","open_issues":0}]')
[ "$line" = "ok milestones and phases agree, and every open issue has a section that offers it" ] \
  && ok "records agree" \
  || no "records agree" "ok" "$line"

line=$(milestones '[{"title":"Phase 8 - look","state":"open","open_issues":0},{"title":"Phase 9 - pictures","state":"open","open_issues":2}]')
[ "$line" = "DRIFT milestone with work and no roadmap section: Phase 9 - pictures" ] \
  && ok "milestone without heading" \
  || no "milestone without heading" "DRIFT milestone with work and no roadmap section: Phase 9 - pictures" "$line"

printf '%s\n' "| ⬜ | three | [#3](https://example.invalid/3) |" "## Phase 12 - edges" "| | Task | Issue |" "|---|---|---|" "| ⬜ | two | [#2](https://example.invalid/2) |" >> "$FIX/roadmap6.md"
line=$(milestones '[{"title":"Phase 8 - look","state":"open","open_issues":1}]' 2)
[ "$line" = "DRIFT roadmap phase with open rows and no open milestone: Phase 12 - edges" ] \
  && ok "heading without milestone" \
  || no "heading without milestone" "DRIFT roadmap phase with open rows and no open milestone: Phase 12 - edges" "$line"

line=$(milestones '[{"title":"Phase 8 - look","state":"open","open_issues":1},{"title":"Phase 12 - edges","state":"open","open_issues":1}]' 1)
[ "$line" = "DRIFT open issue no section offers: #1" ] \
  && ok "issue no section offers" \
  || no "issue no section offers" "DRIFT open issue no section offers: #1" "$line"

# A failing chooser must not read as agreement. A board at next.sh's own limit
# makes it refuse.
printf '%s' '[]' > "$FIX/ms.json"
jq -n '[range(1; 1001) | {number: ., state: "OPEN", milestone: {title: "Phase 8 - look"}, title: "x"}]' > "$FIX/issues.json"
line=$(PREFLIGHT_SPEC_FILE="$FIX/spec.md" PREFLIGHT_ROADMAP_FILE="$FIX/roadmap6.md" \
  PREFLIGHT_ISSUES_FILE="$FIX/issues.json" PREFLIGHT_MILESTONES_FILE="$FIX/ms.json" PREFLIGHT_ISSUE_LIMIT=5000 \
  sh "$PRE" 2>&1 | grep -c 'next.sh failed')
[ "$line" = 1 ] && ok "chooser failure reported" || no "chooser failure reported" "1" "$line"

echo "drift:"

present() { # needle, file, name
  grep -qF "$1" "$2" 2>/dev/null && ok "$3" || no "$3" "$1" "absent"
}

present 'sh .claude/skills/take-next/next.sh' "$SKILL" "SKILL.md sends a session to next.sh"
present 'next.sh' "$PRE" "preflight.sh reports the section next.sh takes"

# The limit is the one thing the cases above cannot check about themselves: they
# run at a small override, so nothing there would notice the shipped default
# dropping back under the board.
present 'PREFLIGHT_ISSUE_LIMIT:-1000' "$PRE" "preflight.sh still fetches to a limit of 1000"

echo
if [ "$FAIL" -eq 0 ]; then
  echo "all checks passed"
else
  echo "$FAIL check(s) failed"
fi
exit "$FAIL"
