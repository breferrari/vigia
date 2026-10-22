# Patches to hayro-syntax 0.8.0

Base: `hayro-syntax` 0.8.0 from crates.io, sha256 `6cdc818d8d265dcb42081d488f7599df4f73fb79e9d347ecedcd3c23809d2026`.

Upstream: `https://github.com/LaurenzV/hayro` at tag `hayro-syntax-v0.8.0`, in `hayro-syntax/`.

Suite: `cargo test --locked -p hayro-syntax -p hayro-interpret -p hayro -- --skip pdf_version_header --skip pdf_version_catalog`.

The two skipped tests read PDFs from `hayro-tests/downloads/`, a corpus upstream downloads separately and does not commit; they fail the same way on upstream's own tag. The suite runs `hayro-interpret` and `hayro` too, since both build on the patched crate; all three tags and all three published crates are upstream commit `ea9c81dc`, so the clone holds what the forks ship.

This directory is that crate as published, with the patches in `patches/` applied in order, and nothing else. It is published to crates.io as `vigia-hayro-syntax`, and the workspace and `vigia-hayro-interpret` take it under the name `hayro-syntax`, so a `cargo install vigia` gets the decode limit too. `.github/scripts/forks.sh` rebuilds this directory from the published crate and the patches, and fails on any difference.

## `0001-decode-limit.patch`

The decode limit, and the predictor row below it, so it can be dropped when upstream ships a decode limit ([LaurenzV/hayro#1301](https://github.com/LaurenzV/hayro/pull/1301)).

### The decode limit

`src/limit.rs` is new. `with_decode_limit(bytes, work)` runs `work` with a byte budget for everything decoded on this thread, and returns `Err(DecodeLimitExceeded)` when any decode inside it passed the budget. Inside another limit the smaller one holds, and what the inner call decodes is spent from the outer one. Outside any limit nothing is limited, so code that never calls it behaves as upstream does. It needs `std` for its thread-local.

The budget is checked where each decode produces its output:

- `src/object/stream.rs`: `Stream::decoded_image` spends the output of each filter in the chain, and fails with `StreamDecode` past the budget. Every stream decode goes through it.
- `src/filter/lzw_flate.rs`, `run_length.rs`, `ascii_85.rs`: Flate (zlib, raw deflate and the pdf.js fallback), LZW, RunLength and ASCII85 grow their output through `limit::room`, which admits the bytes before the buffer grows, grows capacity in powers of two and never past what is left. A bomb stops at the limit, and the largest buffer a growth copies from is at most half of it when the limit is a power of two. ASCII85 no longer sizes its output from its input up front.
- `src/filter/dct.rs`, `jpx.rs`, `jbig2.rs`, `ccitt.rs`: check the image's dimensions and component count, at the most bytes a sample can take while it decodes, before any pixel is decoded.
- `src/filter/lzw_flate.rs`: a predictor's eight-bit path undoes rows in place. The bit-level path, for TIFF rows at a depth other than eight, writes a second buffer, so the input and output are admitted together.
- `src/lib.rs`: exports `with_decode_limit` and `DecodeLimitExceeded` under `std`, and, hidden, `fallback_capacity`, which reports the capacity the fallback inflater's output reached, for tests of the limit. `src/filter/mod.rs` makes `lzw_flate` visible in the crate for it.

### The predictor row

`src/filter/lzw_flate.rs`: a predictor's row length comes from `/Columns`, which the file claims. It is computed with an overflow check, and a row longer than the data, which yields no row, is never allocated. Before, a large `/Columns` allocated a row of that size and aborted the process when the allocation failed. This one does not depend on the limit.

## `0002-name-the-fork.patch`

The manifest only, with a change notice at its top: the package is named `vigia-hayro-syntax`, with a description that points at upstream, docs.rs and repository links for the fork, and `Cargo.toml.orig`, `PATCHES.md` and `patches/` left out of the package. The library is still named `hayro_syntax`.

## `0003-fork-notice.patch`

`README.md`, which crates.io shows as the fork's page: a notice at the top that this is a fork kept for vigia, not affiliated with or endorsed by hayro or its author, with a link to upstream and a line saying to use hayro-syntax instead. Upstream's badges are removed. Upstream's text follows unchanged.

## `0004-change-notices.patch`

A one-line comment at the top of every file `0001` changes, and of `src/limit.rs`, which it adds, saying the file was changed or added for this fork. It is kept apart from `0001`, so `0001` stays the change that would go upstream.

## Licence

Upstream is MIT OR Apache-2.0, and so is this fork. Upstream's copyright and both licence files are kept, and every file the fork changes carries a notice saying so (`0004`), as Apache-2.0 §4(b) asks.

## Left out

The registry's `.cargo-ok`, `.cargo_vcs_info.json` and `Cargo.lock` are not copied. Every other file is as published.

## Versions

The three forks require each other exactly (`=0.8.0`), so a change to any one is published as a new patch version of all three, in the order `vigia-hayro-syntax`, `vigia-hayro-interpret`, `vigia-hayro`.

## When it goes

When a hayro release carries the decode limit, the workspace takes that release, the three `vendor/hayro*` directories are deleted, and the published `vigia-hayro-syntax`, `vigia-hayro-interpret` and `vigia-hayro` versions are yanked.
