#!/bin/sh
# Runs each fork's own upstream test suite against its patches: the upstream
# repository at the tag its PATCHES.md names, with the code patches applied.
# The published crate leaves out the test data the suite needs, which is why
# this reads the repository rather than `vendor/`. A patch named
# `*-name-the-fork.patch` changes only the published manifest and is skipped.
set -eu

root=$(git rev-parse --show-toplevel)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

for dir in "$root"/vendor/*/; do
  dir=${dir%/}
  [ -d "$dir/patches" ] || continue
  name=$(basename "$dir")
  repo=$(sed -n 's/^Upstream: `\([^`]*\)` at tag `\([^`]*\)`.*/\1/p' "$dir/PATCHES.md")
  tag=$(sed -n 's/^Upstream: `\([^`]*\)` at tag `\([^`]*\)`.*/\2/p' "$dir/PATCHES.md")
  suite=$(sed -n 's/^Suite: `\(.*\)`\.$/\1/p' "$dir/PATCHES.md")
  if [ -z "$repo" ] || [ -z "$tag" ] || [ -z "$suite" ]; then
    echo "::error::vendor/$name: PATCHES.md needs an Upstream line and a Suite line"
    exit 1
  fi
  git clone --quiet --depth 1 --branch "$tag" --recurse-submodules --shallow-submodules "$repo" "$work/$name"
  for patch in "$dir"/patches/*.patch; do
    case $patch in *-name-the-fork.patch) continue ;; esac
    git -C "$work/$name" apply "$patch"
  done
  echo "vendor/$name: $suite"
  (cd "$work/$name" && sh -c "$suite")
done
