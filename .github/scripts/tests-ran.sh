#!/bin/sh
# Fails when a `cargo test` log shows that no test ran.
#
# `--exact` with a filter that is not a full test name selects nothing, and
# libtest then exits 0, so a flake-rate run would report a clean rate for a
# test that never ran. An ignored test is the same case: listed, never run.
#
# Usage: tests-ran.sh <log> <target> <filter>
#
# Sums passed and failed over every `test result:` line, since one log can
# hold more than one harness. A failed test counts as run: that is a
# measurement, not a broken probe.
set -eu

[ "$#" -eq 3 ] || { echo "::error::usage: tests-ran.sh <log> <target> <filter>"; exit 2; }
log="$1"
target="$2"
filter="$3"

ran=$(sed -n 's/^test result: [A-Za-z]*\. \([0-9][0-9]*\) passed; \([0-9][0-9]*\) failed;.*/\1 \2/p' "$log" |
  awk '{ n += $1 + $2 } END { print n + 0 }')

if [ "$ran" -eq 0 ]; then
  echo "::error::filter '$filter' ran no test in target '$target'"
  exit 2
fi
