#!/bin/sh
# Checks every fork under `vendor/` that carries a `patches/` directory:
#
#   1. The directory is the crate published on crates.io, at the version and
#      checksum its PATCHES.md names, with its patches applied in order and
#      nothing else changed.
#   2. vigia's shipped graph holds the fork and not the crate it forks, so no
#      type or thread-local exists twice.
#
# Needs the network for the crate and the index. Prints every failure, then
# exits non-zero if there was one.
set -eu

root=$(git rev-parse --show-toplevel)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
graph=$(cd "$root" && cargo tree -p vigia -e normal --target all --all-features --prefix none --format '{p}')

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

failed=0
fail() {
  echo "::error::$1"
  failed=1
  bad=1
}

for dir in "$root"/vendor/*/; do
  dir=${dir%/}
  [ -d "$dir/patches" ] || continue
  bad=0
  name=$(basename "$dir")
  base=$(sed -n 's/^Base: `\([^`]*\)` \([^ ]*\) from crates.io, sha256 `\([0-9a-f]*\)`.*/\1 \2 \3/p' "$dir/PATCHES.md")
  # shellcheck disable=SC2086
  set -- $base
  if [ $# -ne 3 ]; then
    fail "vendor/$name: PATCHES.md has no Base line"
    continue
  fi
  crate=$1 version=$2 pinned=$3

  case $crate in
    ?) prefix=1 ;;
    ??) prefix=2 ;;
    ???) prefix=3/$(printf %.1s "$crate") ;;
    *) prefix=$(printf %.2s "$crate")/$(printf %.4s "$crate" | cut -c3-4) ;;
  esac
  indexed=$(curl -fsSL "https://index.crates.io/$prefix/$crate" | jq -r --arg v "$version" 'select(.vers == $v) | .cksum')
  [ "$indexed" = "$pinned" ] || fail "vendor/$name: PATCHES.md pins $pinned, the index has ${indexed:-nothing} for $crate $version"

  curl -fsSL "https://static.crates.io/crates/$crate/$crate-$version.crate" -o "$work/$name.crate"
  [ "$(sha256 "$work/$name.crate")" = "$pinned" ] || fail "vendor/$name: the downloaded $crate $version does not match its pinned checksum"

  mkdir "$work/$name"
  tar -xzf "$work/$name.crate" -C "$work/$name"
  built="$work/$name/$crate-$version"
  rm -f "$built/.cargo_vcs_info.json" "$built/Cargo.lock"
  for patch in "$dir"/patches/*.patch; do
    (cd "$built" && git apply "$patch") || fail "vendor/$name: $(basename "$patch") does not apply to $crate $version"
  done
  diff -r -x PATCHES.md -x patches -x target -x Cargo.lock "$built" "$dir" >"$work/$name.diff" ||
    fail "vendor/$name is not $crate $version plus its patches: $(head -c 2000 "$work/$name.diff")"

  fork=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$dir/Cargo.toml" | head -1)
  echo "$graph" | grep -q "^$fork v$version" || fail "vigia's graph does not hold $fork $version"
  if echo "$graph" | grep -q "^$crate v"; then
    fail "vigia's graph holds $crate beside its fork $fork"
  fi
  [ "$bad" = 1 ] || echo "vendor/$name: $crate $version plus $(find "$dir/patches" -name '*.patch' | wc -l | tr -d ' ') patches, alone in vigia's graph as $fork"
done

exit "$failed"
