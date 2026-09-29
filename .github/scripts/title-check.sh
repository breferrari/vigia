#!/bin/sh
# Keeps contract phrasing out of release notes.
#
# Usage: title-check.sh <base>   fails when $TITLE or a commit subject in
#                                <base>..HEAD uses a phrase from
#                                .github/public-dialect.txt without a
#                                `Release-note:` line (see below)
#        title-check.sh --match  prints each stdin line that uses a phrase
#
# Only commit bodies count. Pull requests are squash-merged with their commit
# messages, so a `Release-note:` line in the PR body never reaches the release.
set -eu

list="${DIALECT_LIST:-$(dirname "$0")/../public-dialect.txt}"
[ -f "$list" ] || { echo "::error::${list} does not exist"; exit 1; }

# One ERE over lowercased text. `[^a-z0-9]` stands in for a word boundary,
# because `\b` is GNU-only.
pattern=$(grep -v '^#' "$list" | grep -v '^[[:space:]]*$' | tr 'A-Z' 'a-z' | awk '
    {
        prefix = sub(/\*$/, "")
        alt = "(^|[^a-z0-9])" $0 (prefix ? "" : "([^a-z0-9]|$)")
        out = out (NR > 1 ? "|" : "") alt
    }
    END { print out }
')
[ -n "$pattern" ] || { echo "::error::${list} lists no phrase"; exit 1; }

matches() {
    PATTERN="$pattern" awk '
        { line = tolower($0); gsub(/`[^`]*`/, "", line) }
        line ~ ENVIRON["PATTERN"] { print }
    '
}

if [ "${1:-}" = "--match" ]; then
    matches
    exit 0
fi

[ "$#" -eq 1 ] || { echo "::error::usage: title-check.sh <base> | --match"; exit 1; }
base="$1"

# Each commit stands alone: a trailer on one commit never excuses another's
# subject. The PR title is the note only when no commit carries a trailer.
failed=0
noted=0
for sha in $(git log --no-merges --format=%H "${base}..HEAD"); do
    if git log -1 --format=%b "$sha" | grep -q '^Release-note:'; then
        noted=1
        continue
    fi
    hit=$(git log -1 --format=%s "$sha" | matches)
    if [ -n "$hit" ]; then
        echo "::error::commit ${sha} uses contract phrasing and carries no Release-note: ${hit}"
        failed=1
    fi
done

if [ "$noted" -eq 0 ]; then
    hit=$(printf '%s\n' "${TITLE:-}" | matches)
    if [ -n "$hit" ]; then
        echo "::error::the PR title uses contract phrasing and no commit carries a Release-note: ${hit}"
        failed=1
    fi
fi

if [ "$failed" -ne 0 ]; then
    echo "::error::Reword it, or add 'Release-note: <one plain sentence>' to that commit's message body and push. The PR body does not count: it is lost on squash."
    exit 1
fi
