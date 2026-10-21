# Patches to hayro-interpret 0.8.0

Base: `hayro-interpret` 0.8.0 from crates.io, sha256 `ac9df074ff163a63864cec9b0bf640ccbff8abf4e44481df7369ce0ac172ffbe`.

Upstream: `https://github.com/LaurenzV/hayro` at tag `hayro-interpret-v0.8.0`, in `hayro-interpret/`.

Suite: `none`.

This directory is that crate as published, with the patches in `patches/` applied in order, and nothing else. Its code is unchanged: it is forked only so that it depends on `vigia-hayro-syntax`, which leads to the decode limit in `vendor/hayro-syntax`, and so a `cargo install vigia` gets that limit. A crate that kept upstream's dependency would pull a second, unlimited `hayro-syntax` beside the fork.

## `0001-name-the-fork.patch`

The manifest only, with a change notice at its top: the package is named `vigia-hayro-interpret`, its dependency is `vigia-hayro-syntax` under its upstream name, with a path for this workspace that `cargo publish` drops, a description that points at upstream, docs.rs and repository links for the fork, and `Cargo.toml.orig`, `PATCHES.md` and `patches/` left out of the package.

## `0002-fork-notice.patch`

`README.md`, which crates.io shows as the fork's page: a notice at the top that this is a fork kept for vigia, not affiliated with or endorsed by hayro or its author, with a link to upstream and a line saying to use hayro-interpret instead. Upstream's badges are removed. Upstream's text follows unchanged.

## Licence

Upstream is MIT OR Apache-2.0, and so is this fork, whose code is unchanged; upstream's copyright and both licence files are kept.

## Left out

The registry's `.cargo-ok`, `.cargo_vcs_info.json` and `Cargo.lock` are not copied. Every other file is as published.

## Versions

The three forks require each other exactly (`=0.8.0`), so a change to any one is published as a new patch version of all three, in the order `vigia-hayro-syntax`, `vigia-hayro-interpret`, `vigia-hayro`.

## When it goes

When a hayro release carries the decode limit, the workspace takes that release, the three `vendor/hayro*` directories are deleted, and the published `vigia-hayro-syntax`, `vigia-hayro-interpret` and `vigia-hayro` versions are yanked.
