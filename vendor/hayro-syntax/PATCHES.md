# Patches to hayro-syntax 0.8.0

This directory is `hayro-syntax` 0.8.0 as published on crates.io (MIT OR Apache-2.0), with the changes below. The workspace uses it through `[patch.crates-io]`. Each change is here so it can be dropped when upstream ships a decode limit ([LaurenzV/hayro#1301](https://github.com/LaurenzV/hayro/pull/1301)).

## The decode limit

`src/limit.rs` is new. `with_decode_limit(bytes, work)` runs `work` with a byte budget for everything decoded on this thread, and returns `Err(DecodeLimitExceeded)` when any decode inside it passed the budget. Outside it nothing is limited, so code that never calls it behaves as upstream does. It needs `std` for its thread-local.

The budget is checked where each decode produces its output:

- `src/object/stream.rs`: `Stream::decoded_image` spends the output of each filter in the chain, and fails with `StreamDecode` past the budget. Every stream decode goes through it.
- `src/filter/lzw_flate.rs`: Flate reads at most one byte past what is left, for zlib, raw deflate and the fallback decoder, so a bomb stops early rather than after it is whole. A zlib stream that passed the budget does not fall back to raw deflate. LZW checks its output as it grows.
- `src/filter/run_length.rs`: checks its output as it grows.
- `src/filter/dct.rs`, `jpx.rs`, `jbig2.rs`, `ccitt.rs`: check the image's dimensions, at the most bytes a pixel each can produce, before the pixels are decoded.
- `src/lib.rs`: exports `with_decode_limit` and `DecodeLimitExceeded` under `std`.

Nothing else differs from the published crate. `diff -ru` against the registry copy shows exactly these files.
