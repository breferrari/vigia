#!/bin/sh
# Judges whether every required leg of `ci.yml` ran and passed.
#
# Lives in a file rather than inline in the workflow so it can be driven with
# fabricated results from a test. The shape it has to get right is narrow and
# was got wrong inline: a draft skips every leg by design, and judging those
# skips as failure turns the required check red on every push to a draft.
#
# Usage: ci-complete.sh <draft> <result>...
#   draft   "true" when the event is a draft pull request
#   result  one leg's result, in workflow order
set -eu

draft="${1:?draft flag}"
shift

[ "$#" -gt 0 ] || { echo "::error::no leg results were passed"; exit 1; }

total=0
skipped=0
for result in "$@"; do
    total=$((total + 1))
    [ "$result" = "skipped" ] && skipped=$((skipped + 1))
    echo "leg $total: $result"
done

# A draft skips its legs on purpose, and the full matrix runs on
# `ready_for_review`. Only the whole set counts: a draft that skipped some legs
# and ran others is the partial run this gate exists to catch.
if [ "$draft" = "true" ] && [ "$skipped" -eq "$total" ]; then
    echo "draft: all $total legs skipped by design, the matrix runs when it is marked ready"
    exit 0
fi

for result in "$@"; do
    if [ "$result" != "success" ]; then
        echo "::error::a required leg reported '$result' rather than success"
        exit 1
    fi
done

echo "all $total legs passed"
