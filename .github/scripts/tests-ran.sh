#!/bin/sh
# Fails when a `cargo test` log shows that no test ran.
#
# `--exact` with a filter that is not a full test name selects nothing, and
# libtest then exits 0. An ignored test is the same case: listed, never run.
# A failed test counts as run, since that is a measurement.
#
# Usage: tests-ran.sh <log> <target> <filter>
set -eu

[ "$#" -eq 3 ] || { echo "::error::usage: tests-ran.sh <log> <target> <filter>"; exit 1; }
log="$1"
target="$2"
filter="$3"

ran=$(awk '/^test result:/ { n += $4 + $6 } END { print n + 0 }' "$log")

if [ "$ran" -eq 0 ]; then
  echo "::error::filter '$filter' ran no test in target '$target'"
  exit 1
fi
