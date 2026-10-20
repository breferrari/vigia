#!/bin/sh
# Fails when a `cargo test` log shows that no test ran.
#
# `--exact` with a filter that is not a full test name selects nothing, and
# libtest then exits 0. An ignored test is the same case: selected, never run.
# A test that failed, or crashed before its result line, counts as run, since
# that is a measurement.
#
# Usage: tests-ran.sh <log> <target> <filter>
set -eu

[ "$#" -eq 3 ] || { echo "::error::usage: tests-ran.sh <log> <target> <filter>"; exit 1; }
log="$1"
target="$2"
filter="$3"

counts=$(awk '
  /^running [0-9]+ tests?$/ { harnesses++; selected += $2 }
  /^test result:/ { ignored += $8 }
  END { print harnesses + 0, selected - ignored }
' "$log")
harnesses=${counts% *}
ran=${counts#* }

if [ "$harnesses" -eq 0 ]; then
  echo "::error::target '$target' never ran its tests; the log ends:"
  tail -n 20 "$log"
  exit 1
fi
if [ "$ran" -le 0 ]; then
  echo "::error::filter '$filter' ran no test in target '$target'"
  exit 1
fi
