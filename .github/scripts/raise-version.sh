#!/bin/sh
# Moves the workspace version and the internal dependency's pin to one number,
# and verifies that both moved.
#
# Lives in a file rather than inline in the workflow so it can be driven
# against a manifest from a test. The verification was got wrong inline: it
# counted the new number anywhere in the file, and the manifest also pins the
# release tool, so the first minor to land on the tool's own number counted
# three lines instead of two and refused a release that was correct.
#
# Usage: raise-version.sh <next> [manifest]
#   next      the version to write, three numeric components
#   manifest  the workspace Cargo.toml (default: ./Cargo.toml)
set -eu

[ "$#" -ge 1 ] || { echo "::error::no version was passed"; exit 1; }
next="$1"
manifest="${2:-Cargo.toml}"

# Both strings in one pass, because moving one and not the other is the failure
# the release job is arranged around. Each pattern is anchored to the start of a
# line and matches exactly one line in the manifest. Written through a copy
# rather than `sed -i`, whose argument shape differs between GNU and BSD sed.
raised="${manifest}.raised"
sed -E \
    -e "s|^version = \"[^\"]+\"|version = \"${next}\"|" \
    -e "s|^(vigia-core = \{ path = \"crates/vigia-core\", version = )\"[^\"]+\"|\1\"${next}\"|" \
    "$manifest" > "$raised"
mv "$raised" "$manifest"

# Verify the edit rather than trusting the substitution, because a sed that
# matched nothing exits 0 and leaves the old version in place. Whole lines and
# fixed strings, so a line elsewhere in the file that happens to hold the same
# number is not counted. `|| true`, because `grep -c` exits 1 when it counts
# nothing and `set -e` would then kill the script on the assignment, before the
# diagnostic below could say which line failed to move.
found=$(grep -cxF \
    -e "version = \"${next}\"" \
    -e "vigia-core = { path = \"crates/vigia-core\", version = \"${next}\" }" \
    "$manifest" || true)
if [ "${found:-0}" != "2" ]; then
    echo "::error::expected both version strings at ${next}, found ${found}"
    grep -nE '^version = |^vigia-core = ' "$manifest"
    exit 1
fi
