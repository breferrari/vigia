#!/bin/sh
# Each fork under `vendor/` with a `patches/` directory must be the crate
# published on crates.io, at the checksum its PATCHES.md pins, plus those
# patches and nothing else.
set -eu

root=$(git rev-parse --show-toplevel)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

failed=0
checked=0
for dir in "$root"/vendor/*/; do
  dir=${dir%/}
  [ -d "$dir/patches" ] || continue
  name=$(basename "$dir")
  read -r crate version pinned <<FIELDS
$(sed -n 's/^Base: `\([^`]*\)` \([^ ]*\) from crates.io, sha256 `\([0-9a-f]*\)`.*/\1 \2 \3/p' "$dir/PATCHES.md")
FIELDS
  if [ -z "${pinned:-}" ]; then
    echo "::error::vendor/$name: PATCHES.md has no Base line"
    failed=1
    continue
  fi

  curl -fsSL "https://static.crates.io/crates/$crate/$crate-$version.crate" -o "$work/$name.crate"
  if [ "$(sha256 "$work/$name.crate")" != "$pinned" ]; then
    echo "::error::vendor/$name: $crate $version on crates.io does not match its pinned checksum"
    failed=1
    continue
  fi

  mkdir "$work/$name"
  tar -xzf "$work/$name.crate" -C "$work/$name"
  built="$work/$name/$crate-$version"
  rm -f "$built/.cargo_vcs_info.json" "$built/Cargo.lock"
  for patch in "$dir"/patches/*.patch; do
    (cd "$built" && git apply "$patch")
  done
  # Ours without what the fork adds beside the crate, at its top level only.
  cp -R "$dir" "$work/ours-$name"
  rm -rf "$work/ours-$name/patches" "$work/ours-$name/PATCHES.md" "$work/ours-$name/target" "$work/ours-$name/Cargo.lock"
  checked=$((checked + 1))
  if diff -r "$built" "$work/ours-$name" >"$work/$name.diff"; then
    echo "vendor/$name: $crate $version plus $(find "$dir/patches" -name '*.patch' | wc -l | tr -d ' ') patches"
  else
    echo "::error::vendor/$name is not $crate $version plus its patches"
    head -c 4000 "$work/$name.diff"
    failed=1
  fi
done

if [ "$checked" -eq 0 ]; then
  echo "::error::no fork under vendor/ was checked"
  failed=1
fi
exit "$failed"
