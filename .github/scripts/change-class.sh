#!/bin/sh
# Classifies a pull request by the files it changes, so CI can skip the
# platform matrix and the budgets when nothing they measure has changed.
#
# Paths arrive on stdin, one per line. One word is printed:
#
#   docs   every path is prose, a skill, a template or an image: the suites
#          that read documents run on one platform and nothing else does
#   full   anything else, including an empty list: the whole workflow runs
#
# The list below names what is prose; a path it does not name is code. That
# way a file added later and read by a test runs too much rather than too
# little. `.github/workflows` and `.github/scripts` are code: a change to how
# CI runs is proved by running it. `Cargo.toml`, `Cargo.lock` and
# `assets/syntaxes` are code: the grammar dump is what the coverage suite reads.
# A markdown file under `crates/` is code too: `NOTICE.md` ships in the archive
# and a test reads it. `.claude/scripts` are the guards that run on every
# commit and publish, and they are code. `.claude/take-next` holds the
# take-next rules, which only the skill reads. `docs/research` holds notes and
# their images, and a script kept beside them is code.
#
# The caller lists a renamed file under both names, so moving code into a
# docs path is still code.
#
# Usage: change-class.sh < paths
set -eu

paths=$(grep -v '^$' || true)

if [ -z "$paths" ]; then
    echo "::notice::no changed paths were listed, so the class is full"
    echo full
    exit 0
fi

img='(jpg|jpeg|png|svg|gif)'

# Exit 1 is grep matching nothing, which means every path is prose. Anything
# above it is grep failing, and that must not read as docs.
set +e
code=$(printf '%s\n' "$paths" | grep -Ev \
    -e '^[^/]+\.md$' \
    -e '^docs/[^/]+\.md$' \
    -e '^docs/research/.*\.md$' \
    -e "^docs/research/.*\\.$img\$" \
    -e '^\.github/PULL_REQUEST_TEMPLATE\.md$' \
    -e '^\.claude/skills/' \
    -e '^\.claude/take-next/.*\.md$' \
    -e '^\.claude/settings\.json$' \
    -e '^LICENSE$' \
    -e '^\.github/ISSUE_TEMPLATE/' \
    -e '^\.github/public-dialect\.txt$' \
    -e '^\.github/release\.yml$' \
    -e '^\.github/FUNDING\.yml$' \
    -e '^\.gitignore$' \
    -e "^assets/[^/]+\\.$img\$" \
    -e "^assets/proposals/[^/]+\\.$img\$" \
    -e '^\.techpack/templates/[^/]+\.md$')
status=$?
set -e
if [ "$status" -gt 1 ]; then
    echo "::error::grep failed with status $status, so the class cannot be decided" >&2
    exit "$status"
fi

if [ -n "$code" ]; then
    printf '%s\n' "$code" | sed 's/^/::notice::full, because of: /'
    echo full
else
    echo "::notice::docs only: $(printf '%s\n' "$paths" | wc -l | tr -d ' ') path(s)"
    echo docs
fi
