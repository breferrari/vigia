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
# and a test reads it.
#
# `CI_FULL=true` in the environment answers full whatever the paths, for a
# pull request that carries the `full-ci` label. A partial run that turns out
# wrong needs a way to be overridden without a code change.
#
# Usage: [CI_FULL=true] change-class.sh < paths
set -eu

paths=$(grep -v '^$' || true)

if [ "${CI_FULL:-}" = "true" ]; then
    echo "::notice::the full-ci label is set, so the class is full"
    echo full
    exit 0
fi

if [ -z "$paths" ]; then
    echo "::notice::no changed paths were listed, so the class is full"
    echo full
    exit 0
fi

code=$(printf '%s\n' "$paths" | grep -Ev \
    -e '^[^/]+\.md$' \
    -e '^docs/[^/]+\.md$' \
    -e '^\.github/PULL_REQUEST_TEMPLATE\.md$' \
    -e '^\.claude/' \
    -e '^LICENSE$' \
    -e '^\.github/ISSUE_TEMPLATE/' \
    -e '^\.github/public-dialect\.txt$' \
    -e '^\.github/release\.yml$' \
    -e '^\.github/FUNDING\.yml$' \
    -e '^\.gitignore$' \
    -e '^assets/[^/]+\.(jpg|jpeg|png|svg|gif)$' \
    || true)

if [ -n "$code" ]; then
    printf '%s\n' "$code" | sed 's/^/::notice::full, because of: /'
    echo full
else
    echo "::notice::docs only: $(printf '%s\n' "$paths" | wc -l | tr -d ' ') path(s)"
    echo docs
fi
