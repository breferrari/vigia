# Patches to syntect 5.3.0

Base: `syntect` 5.3.0 from crates.io, sha256 `656b45c05d95a5704399aeef6bd0ddec7b2b3531b7c9e900abbf7c4d2190c925`.

Upstream: `https://github.com/trishume/syntect` at tag `v5.3.0`.

Suite: `cargo test --locked --no-default-features --features default-fancy`.

This directory is that crate as published (MIT), with the patches in `patches/` applied in order, and nothing else. It is published to crates.io as `vigia-syntect`, and the workspace takes it under the name `syntect`, so a `cargo install vigia` gets the patches too. `.github/scripts/forks.sh` rebuilds this directory from the published crate and the patches, and fails on any difference.

## `0001-memoise-backref-regexes.patch`

A pattern with a backreference, such as the `\1` that closes a Markdown code fence, is built from the captures of an earlier match. `ParseState::search` compiled a fresh `Regex` for it on every search, at every position, for as long as the context stayed on the stack. The patch keeps those regexes for the rest of the line in the search cache, keyed by an FNV hash of the substituted pattern and checked against it on a hit, so a collision recompiles rather than reusing the wrong regex. Match results on that arm are still not cached, so parsing output is unchanged.

It adds `backrefs_with_different_captures_on_one_line` to syntect's own tests: two fences of different lengths on one line must each close on their own run. A memo that reuses one regex for both fails it.

Measured on an Apple M1 Max, release, with bat's grammars from `two-face` 0.5.2, parse only: a Markdown file with a 24-line Rust fence took 89.6ms median before and 7.8ms after, and a 200-line fence 739.5ms before and 58.9ms after. syntect's suite at the `v5.3.0` tag passes with the patch, with the `default-fancy` features.

## `0002-name-the-fork.patch`

The manifest only: the package is named `vigia-syntect`, with a description that points at upstream, docs.rs and repository links for the fork, and `Cargo.toml.orig`, `PATCHES.md` and `patches/` left out of the package. `cargo package` refuses a source that contains a `Cargo.toml.orig`. It also allows `mismatched_lifetime_syntaxes`, which current rustc raises on 5.3.0's `yaml_load.rs`: a path dependency's warnings are not capped, and CI denies warnings. The library is still named `syntect`.

## `0003-fork-notice.patch`

`Readme.md`, which crates.io shows as the fork's page: a notice at the top that this is a fork of syntect 5.3.0 with these patches, kept for vigia, not affiliated with or endorsed by syntect or its author, with a link to upstream and a line saying to use syntect instead. Upstream's badges are removed, since they describe upstream's crate. Upstream's text follows unchanged.

## Left out

The registry's `.cargo-ok`, `.cargo_vcs_info.json` and `Cargo.lock` are not copied. Every other file is as published.

## Versions

The fork is published as `vigia-syntect` 5.3.0, its base's version. A later change to the fork bumps its patch number; the Base line still names what it is cut from.

## When it goes

When a syntect release carries the memo, the workspace takes that release, this directory is deleted, and the published `vigia-syntect` versions are yanked.
