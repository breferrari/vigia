#!/usr/bin/env sh
# Self-test for the hooks in this directory. Offline, needs node only.
#
#   sh .claude/scripts/selftest.sh
#
# Each guard reads a tool call as JSON on stdin and exits 2 to block it, so a
# case is one fabricated call and the exit code it must produce. A guard that
# cannot be shown blocking has not been tested, and neither has one that cannot
# be shown letting ordinary work through.
set -u
FAIL=0
HERE="$(dirname "$0")"
FIX="$(mktemp -d)"
trap 'rm -rf "$FIX"' EXIT
# The guards run under node, which on Windows cannot open an MSYS path such as
# /tmp/x, and a guard that cannot read fails open. Every path a guard sees is
# handed over in the form node can use.
W() { cygpath -m "$1" 2>/dev/null || printf '%s' "$1"; }
FIXW=$(W "$FIX")

ok() { printf '  ok   %s\n' "$1"; }
no() { printf '  FAIL %s\n    expected exit %s, got %s\n' "$1" "$2" "$3"; FAIL=$((FAIL + 1)); }

# The JSON a PreToolUse hook receives for a Bash call, with the command escaped
# by jq so quotes and backslashes in a case survive.
bash_call() { jq -n --arg c "$1" '{tool_name: "Bash", tool_input: {command: $c}}'; }

expect() { # guard, expected exit, name, json
  bash_call "$4" | node "$HERE/$1" >/dev/null 2>&1
  got=$?
  [ "$got" -eq "$2" ] && ok "$3" || no "$3" "$2" "$got"
}

says() { # guard, expected exit, name, json, text stderr must carry
  said=$(bash_call "$4" | node "$HERE/$1" 2>&1 >/dev/null)
  got=$?
  if [ "$got" -ne "$2" ]; then no "$3" "$2" "$got"
  elif ! printf '%s' "$said" | grep -q "$5"; then no "$3 (said: $said)" "$2" "$got"
  else ok "$3"; fi
}

echo "scan-guard:"
expect scan-guard.mjs 2 "find rooted at / is blocked" 'find / -name "*.rs"'
expect scan-guard.mjs 2 "find with a flag before / is blocked" 'find -L / -name x'
expect scan-guard.mjs 2 "find / after a separator is blocked" 'cd x && find / -type d'
expect scan-guard.mjs 0 "a bounded find is allowed" 'timeout 10 find / -name x'
expect scan-guard.mjs 0 "a timeout with flags is still a bound" 'timeout -k 5 10 find / -name x'
expect scan-guard.mjs 2 "a timeout on an earlier command bounds nothing here" 'timeout 10 ls; find / -name x'
expect scan-guard.mjs 2 "a timeout before a pipe bounds nothing after it" 'timeout 10 ls | find / -name x'
expect scan-guard.mjs 0 "find under a real path is allowed" 'find /c/Dev/vigia -name "*.rs"'
expect scan-guard.mjs 0 "find in the checkout is allowed" 'find . -name "*.rs"'
expect scan-guard.mjs 0 "a / that is not a start path is allowed" 'ls / && grep -r foo /c/x'

echo "leak-guard:"
printf 'A clean body.\n\nTwo paragraphs.\n' > "$FIX/clean.md"
printf 'A body.\n\nClaude-Session: https://claude.ai/code/session_01abc\n' > "$FIX/trailer.md"
printf 'See https://claude.ai/code/session_01abc for the trail.\n' > "$FIX/url.md"
expect leak-guard.mjs 0 "a publish with a clean body file is allowed" "gh pr create --title t --body-file $FIXW/clean.md"
expect leak-guard.mjs 2 "a publish whose body carries the trailer is blocked" "gh pr create --title t --body-file $FIXW/trailer.md"
expect leak-guard.mjs 2 "a publish whose body carries a session URL is blocked" "gh issue comment 1 --body-file $FIXW/url.md"
expect leak-guard.mjs 2 "an inline body with a session URL is blocked" 'gh pr comment 1 --body "see https://claude.ai/code/session_01abc"'
expect leak-guard.mjs 0 "a body-file path under the profile is not a leak" 'gh pr create --title t --body-file "C:\Users\someone\AppData\Local\Temp\x\body.md"'
# A release takes its body through --notes-file, which is a different flag and
# not a suffix of --file. Both directions are cases, because the omission broke
# both: the body went unread, and its path was read as a leak.
expect leak-guard.mjs 0 "a release with a clean notes file is allowed" "gh release edit v0.1.0 --notes-file $FIXW/clean.md"
expect leak-guard.mjs 2 "a release whose notes carry a session URL is blocked" "gh release edit v0.1.0 --notes-file $FIXW/url.md"
expect leak-guard.mjs 2 "a new release whose notes carry the trailer is blocked" "gh release create v0.1.0 --notes-file $FIXW/trailer.md"
expect leak-guard.mjs 0 "a notes-file path under the profile is not a leak" 'gh release edit v0.1.0 --notes-file "C:\Users\someone\AppData\Local\Temp\x\notes.md"'
expect leak-guard.mjs 2 "a commit message carrying the trailer is blocked" 'git commit -m "subject" -m "Claude-Session: https://claude.ai/code/session_01abc"'
expect leak-guard.mjs 2 "a commit message file carrying the trailer is blocked" "git commit -F $FIXW/trailer.md"
expect leak-guard.mjs 0 "a clean commit is allowed" 'git commit -m "The version raise counts only the lines it moved"'
expect leak-guard.mjs 0 "a clean commit from a file under the profile is allowed" "git commit -F $FIXW/clean.md"
expect leak-guard.mjs 0 "a command that neither publishes nor commits is ignored" 'echo Claude-Session: x'
says leak-guard.mjs 0 "an unreadable body file is let through, and said" "gh pr create --title t --body-file $FIXW/missing.md" "not scanned"
says leak-guard.mjs 0 "an unexpanded body-file variable is let through, and said" 'P=x.md; gh pr create --body-file "$P"' "not expanded"

echo "record-guard:"
# A Stop call, on a scratch repository whose branch names an issue and whose
# pull request the seam reports merged, against a vault fixture. The marker the
# guard leaves goes into the fixture too, so a rerun starts clean.
REPO="$FIX/repo"; VAULT="$FIX/vault"
REPOW=$(W "$REPO"); VAULTW=$(W "$VAULT")
mkdir -p "$REPO" "$VAULT/projects/vigia/notes"
printf 'vigia\n' > "$REPO/.om-project"
git -C "$REPO" init -q && git -C "$REPO" -c user.name=t -c user.email=t@t commit -q --allow-empty -m init \
  && git -C "$REPO" checkout -q -b issue-42-thing
stop_call() { jq -n --arg cwd "$REPOW" --arg s "$1" '{hook_event_name: "Stop", cwd: $cwd, session_id: $s, stop_hook_active: false}'; }
stop() { # session, expected, name
  stop_call "$1" | RECORD_GUARD_STATE=MERGED CLAUDE_PROJECT_DIR="$REPOW" VIGIL_VAULT="$VAULTW" TMP="$FIXW" TEMP="$FIXW" TMPDIR="$FIXW" node "$HERE/record-guard.mjs" >/dev/null 2>&1
  got=$?
  [ "$got" -eq "$2" ] && ok "$3" || no "$3" "$2" "$got"
}
stop s1 2 "a merged pass with no note is stopped once"
stop s1 0 "the same session is not stopped twice"
printf 'Closed #42 with the evidence.\n' > "$VAULT/projects/vigia/notes/2026-01-01-a-note.md"
stop s2 0 "a note naming the issue satisfies it"
printf 'Closed #420 instead.\n' > "$VAULT/projects/vigia/notes/2026-01-01-a-note.md"
stop s3 2 "a note naming a longer number does not"
git -C "$REPO" checkout -q -b no-issue-here
stop s4 0 "a branch with no issue number is left alone"
git -C "$REPO" checkout -q -b release-2026-09-03
stop s6 0 "a date in a branch name is not an issue"
git -C "$REPO" checkout -q -b 42-thing
stop s7 2 "the older <n>-<slug> branch shape still names its issue"
git -C "$REPO" checkout -q -b issue-7-x
jq -n --arg cwd "$REPOW" '{cwd: $cwd, session_id: "s5", stop_hook_active: true}' \
  | RECORD_GUARD_STATE=MERGED CLAUDE_PROJECT_DIR="$REPOW" VIGIL_VAULT="$VAULTW" TMP="$FIXW" TEMP="$FIXW" node "$HERE/record-guard.mjs" >/dev/null 2>&1
got=$?
[ "$got" -eq 0 ] && ok "a continuation the hook itself caused is not stopped again" || no "a continuation the hook itself caused is not stopped again" 0 "$got"

echo "mutate.mjs:"
# A repository whose one file carries a CRLF, and a "suite" that is red when
# `beta` is missing.
MUT="$FIX/mut"; MUTW=$(W "$MUT"); MUTATE=$(W "$(cd "$HERE" && pwd)/mutate.mjs")
mkdir -p "$MUT"
printf 'alpha beta\r\ngamma\n' > "$MUT/a.txt"
git -C "$MUT" init -q && git -C "$MUT" config core.autocrlf false && git -C "$MUT" add a.txt && git -C "$MUT" -c user.name=t -c user.email=t@t commit -q -m init
battery() { printf '%s' "$1" > "$FIX/battery.json"; }
mutate() { (cd "$MUT" && node "$MUTATE" "$FIXW/battery.json" -- grep -q beta a.txt 2>&1); }
unchanged() { git -C "$MUT" diff --quiet && [ -z "$(git -C "$MUT" status --porcelain)" ]; }

battery '[{"name":"drop beta","file":"a.txt","old":"beta","new":"BETA"},{"name":"drop gamma","file":"a.txt","old":"gamma","new":"GAMMA"}]'
out=$(mutate); got=$?
if [ "$got" -eq 1 ] && printf '%s' "$out" | grep -q '^KILLED *drop beta' && printf '%s' "$out" | grep -q '^SURVIVED *drop gamma'; then
  ok "a killed and a surviving mutation are both reported"
else no "a killed and a surviving mutation are both reported (said: $out)" 1 "$got"; fi
unchanged && ok "a battery leaves the file byte-identical" || no "a battery leaves the file byte-identical" clean dirty

battery '[{"name":"drop beta","file":"a.txt","old":"beta","new":"BETA"},{"name":"missed","file":"a.txt","old":"delta","new":"x"}]'
out=$(mutate); got=$?
if [ "$got" -eq 2 ] && printf '%s' "$out" | grep -q "occurs 0 times" && unchanged; then ok "a missed anchor aborts before anything runs"
else no "a missed anchor aborts before anything runs (said: $out)" 2 "$got"; fi

battery '[{"name":"twice","file":"a.txt","old":"a","new":"x"}]'
out=$(mutate); got=$?
[ "$got" -eq 2 ] && unchanged && ok "an anchor matching twice aborts" || no "an anchor matching twice aborts (said: $out)" 2 "$got"

printf 'work\n' > "$MUT/b.txt"
battery '[{"name":"drop beta","file":"a.txt","old":"beta","new":"BETA"}]'
out=$(mutate); got=$?
if [ "$got" -eq 2 ] && printf '%s' "$out" | grep -q "not clean" && git -C "$MUT" diff --quiet; then ok "a dirty tree is refused before anything applies"
else no "a dirty tree is refused before anything applies (said: $out)" 2 "$got"; fi
rm -f "$MUT/b.txt"

out=$(cd "$MUT" && node "$MUTATE" "$FIXW/battery.json" -- no-such-command-here 2>&1); got=$?
[ "$got" -eq 2 ] && unchanged && ok "a test command that never ran aborts" || no "a test command that never ran aborts (said: $out)" 2 "$got"

echo
if [ "$FAIL" -eq 0 ]; then echo "all checks passed"; else echo "$FAIL check(s) failed"; fi
exit "$FAIL"
