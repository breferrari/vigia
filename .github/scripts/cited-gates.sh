#!/bin/sh
# Fails when a commit message or the PR body cites a gate the branch lacks.
#
# Usage: cited-gates.sh <base>   every `file.rs::name` in the messages of
#                                <base>..HEAD and in $BODY must be a `fn name`
#                                in a tracked file called file.rs
#
# A gate lost before the commit leaves the suite green, since a missing gate is
# the one thing no gate sees. The message still names it, and that is what this
# reads.
set -eu

base="$1"
cited=$( { git log --format=%B "${base}..HEAD"; printf '%s\n' "${BODY:-}"; } \
    | grep -oE '[A-Za-z0-9_]+\.rs::[a-z0-9_]+' | sort -u || true)

missing=0
for cite in $cited; do
    file="${cite%%::*}"
    name="${cite#*::}"
    found=$(git ls-files | grep -E "(^|/)${file}\$" | while IFS= read -r path; do
        grep -lE "fn ${name}([^a-z0-9_]|\$)" "$path" || true
    done)
    if [ -z "$found" ]; then
        echo "::error::${cite} is cited and no ${file} defines fn ${name}"
        missing=$((missing + 1))
    fi
done
[ "$missing" -eq 0 ] || exit 1
