#!/bin/sh
# Each fork's upstream suite, run at the tag its PATCHES.md names with its
# patches applied: the published crate leaves out the suite's test data. A
# patch that touches only `Cargo.toml` is the published manifest, written for
# the crate rather than the repository, and is skipped.
set -eu

root=$(git rev-parse --show-toplevel)
work=$(mktemp -d)

# The files a patch changes, one a line; a patch it cannot read stops the run.
touched() {
  files=$(sed -n 's#^diff --git a/\([^ ]*\) b/.*#\1#p' "$1")
  if [ -z "$files" ]; then
    echo "::error::$1 names no file in a git diff header" >&2
    exit 1
  fi
  echo "$files"
}
trap 'rm -rf "$work"' EXIT

for dir in "$root"/vendor/*/; do
  dir=${dir%/}
  [ -d "$dir/patches" ] || continue
  name=$(basename "$dir")
  repo=$(sed -n 's/^Upstream: `\([^`]*\)` at tag `\([^`]*\)`.*/\1/p' "$dir/PATCHES.md")
  tag=$(sed -n 's/^Upstream: `\([^`]*\)` at tag `\([^`]*\)`.*/\2/p' "$dir/PATCHES.md")
  suite=$(sed -n 's/^Suite: `\(.*\)`\.$/\1/p' "$dir/PATCHES.md")
  # The crate's directory in the repository, when it is not the root.
  within=$(sed -n 's/^Upstream: .* in `\([^`]*\)\/`\.$/\1/p' "$dir/PATCHES.md")
  if [ -z "$repo" ] || [ -z "$tag" ] || [ -z "$suite" ]; then
    echo "::error::vendor/$name: PATCHES.md needs an Upstream line and a Suite line"
    exit 1
  fi
  if [ "$suite" = none ]; then
    for patch in "$dir"/patches/*.patch; do
      if touched "$patch" | grep -qv -e '^Cargo.toml$' -e '^README.md$' -e '^Readme.md$'; then
        echo "::error::vendor/$name: Suite is none, but $(basename "$patch") changes code"
        exit 1
      fi
    done
    echo "vendor/$name: no code patch, so no suite of its own"
    continue
  fi
  git clone --quiet --depth 1 --branch "$tag" --recurse-submodules --shallow-submodules "$repo" "$work/$name"
  for patch in "$dir"/patches/*.patch; do
    touched "$patch" | grep -qv '^Cargo.toml$' || continue
    git -C "$work/$name" apply ${within:+--directory="$within"} "$patch"
  done
  echo "vendor/$name: $suite"
  # Upstream's manifest lacks the lint allowances our name-the-fork patch adds,
  # and its own warnings are not ours to deny; vendor/ still builds under deny.
  (cd "$work/$name" && env -u CARGO_BUILD_WARNINGS sh -c "$suite")
done
