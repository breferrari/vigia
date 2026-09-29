#!/bin/sh
# Keeps contract phrasing out of release notes.
#
# Usage: title-check.sh <base>   fails when $TITLE or a commit subject in
#                                <base>..HEAD uses a phrase from
#                                .github/public-dialect.txt and no commit body
#                                in that range carries a `Release-note:` line
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

hits=$({ printf '%s\n' "${TITLE:-}"; git log --no-merges --format=%s "${base}..HEAD"; } | matches)
[ -z "$hits" ] && exit 0

if git log --no-merges --format=%b "${base}..HEAD" | grep -q '^Release-note:'; then
    echo "::notice::a commit body carries Release-note:, so these titles do not reach the release notes:"
    printf '%s\n' "$hits" | sed 's/^/::notice::  /'
    exit 0
fi

printf '%s\n' "$hits" | sed 's/^/::error::uses contract phrasing: /'
echo "::error::Reword it, or add 'Release-note: <one plain sentence>' to a commit message body and push. The PR body does not count: it is lost on squash."
exit 1
