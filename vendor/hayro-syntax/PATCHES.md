# Patches to hayro-syntax 0.8.0

This directory is `hayro-syntax` 0.8.0 as published on crates.io (MIT OR Apache-2.0), with the changes below. The workspace uses it through `[patch.crates-io]`. Each change is here so it can be dropped when upstream ships a decode limit ([LaurenzV/hayro#1301](https://github.com/LaurenzV/hayro/pull/1301)).

## The decode limit

`src/limit.rs` is new. `with_decode_limit(bytes, work)` runs `work` with a byte budget for everything decoded on this thread, and returns `Err(DecodeLimitExceeded)` when any decode inside it passed the budget. Inside another limit the smaller one holds, and what the inner call decodes is spent from the outer one. Outside any limit nothing is limited, so code that never calls it behaves as upstream does. It needs `std` for its thread-local.

The budget is checked where each decode produces its output:

- `src/object/stream.rs`: `Stream::decoded_image` spends the output of each filter in the chain, and fails with `StreamDecode` past the budget. Every stream decode goes through it.
- `src/filter/lzw_flate.rs`, `run_length.rs`, `ascii_85.rs`: Flate (zlib, raw deflate and the pdf.js fallback), LZW, RunLength and ASCII85 grow their output through `limit::room`, which admits the bytes before the buffer grows, grows capacity in powers of two and never past what is left. A bomb stops at the limit, and the largest buffer a growth copies from is at most half of it when the limit is a power of two. ASCII85 no longer sizes its output from its input up front.
- `src/filter/dct.rs`, `jpx.rs`, `jbig2.rs`, `ccitt.rs`: check the image's dimensions and component count, at the most bytes a sample can take while it decodes, before any pixel is decoded.
- `src/filter/lzw_flate.rs`: a predictor's eight-bit path undoes rows in place. The bit-level path, for TIFF rows at a depth other than eight, writes a second buffer, so the input and output are admitted together.
- `src/lib.rs`: exports `with_decode_limit` and `DecodeLimitExceeded` under `std`, and, hidden, `fallback_capacity`, which reports the capacity the fallback inflater's output reached, for tests of the limit. `src/filter/mod.rs` makes `lzw_flate` visible in the crate for it.

## The predictor row

`src/filter/lzw_flate.rs`: a predictor's row length comes from `/Columns`, which the file claims. It is computed with an overflow check, and a row longer than the data, which yields no row, is never allocated. Before, a large `/Columns` allocated a row of that size and aborted the process when the allocation failed. This one does not depend on the limit.

## Left out

The registry's `.cargo-ok`, `.cargo_vcs_info.json` and `Cargo.lock` are not copied. Every other file is as published.
