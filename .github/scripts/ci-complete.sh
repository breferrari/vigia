#!/bin/sh
# Judges whether every required leg of `ci.yml` ran and passed.
#
# Lives in a file rather than inline in the workflow so it can be driven with
# fabricated results from a test. The shape it has to get right is narrow and
# was got wrong inline: a draft skips every leg by design, and judging those
# skips as failure turns the required check red on every push to a draft.
#
# Usage: ci-complete.sh <draft> <class> <name>=<result>...
#   draft   "true" for a draft pull request. Empty on a push, where the event
#           carries no pull request at all, and anything but "true" means the
#           legs were expected to run.
#   class   what `change-class.sh` printed: "docs" when only the suites that
#           read documents were meant to run, anything else when every leg
#           was. Empty means the class was never computed, and is read as
#           full, so a broken classifier fails the gate rather than passing
#           whatever skipped.
#   leg     one leg's name and result, in workflow order
#
# On a docs pull request the heavy legs skip by design and the docs leg runs.
# On any other the docs leg skips and the heavy legs run. A skip anywhere else
# is a leg that was meant to run and did not.
set -eu

[ "$#" -ge 2 ] || { echo "::error::draft flag and class were not both passed"; exit 1; }
draft="$1"
class="$2"
shift 2

[ "$#" -gt 0 ] || { echo "::error::no leg results were passed"; exit 1; }

# The leg that runs only on a docs pull request. Every other leg is heavy.
docs_leg="docs"

total=0
skipped=0
for leg in "$@"; do
    case "$leg" in
        *=*) ;;
        *) echo "::error::leg '$leg' is not name=result"; exit 1 ;;
    esac
    total=$((total + 1))
    [ "${leg#*=}" = "skipped" ] && skipped=$((skipped + 1))
    echo "leg $total: $leg"
done

# A draft skips its legs on purpose, and the full matrix runs on
# ready_for_review. Anything less than every leg skipped is a partial run.
if [ "$draft" = "true" ] && [ "$skipped" -eq "$total" ]; then
    echo "draft: all $total legs skipped by design, the matrix runs when it is marked ready"
    exit 0
fi

status=0
for leg in "$@"; do
    name="${leg%%=*}"
    result="${leg#*=}"
    case "$result" in
        success) ;;
        skipped)
            if [ "$class" = "docs" ] && [ "$name" != "$docs_leg" ]; then
                echo "$name: skipped, the pull request changes only documents"
            elif [ "$class" != "docs" ] && [ "$name" = "$docs_leg" ]; then
                echo "$name: skipped, the pull request is not documents only"
            else
                echo "::error::$name was meant to run and reported skipped"
                status=1
            fi
            ;;
        *)
            echo "::error::$name reported '$result' rather than success"
            status=1
            ;;
    esac
done

[ "$status" -eq 0 ] || exit "$status"
echo "every leg that was meant to run passed"
