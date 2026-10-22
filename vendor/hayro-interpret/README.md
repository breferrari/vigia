# vigia-hayro-interpret

**This is a fork. Use [hayro-interpret](https://crates.io/crates/hayro-interpret) instead.**

`vigia-hayro-interpret` is [hayro-interpret](https://github.com/LaurenzV/hayro) 0.8.0 with no change to its code. It exists so that it builds on `vigia-hayro-syntax`, which carries the patches listed in [`PATCHES.md`](https://github.com/breferrari/vigia/blob/vigia-hayro-0.8.0/vendor/hayro-syntax/PATCHES.md), for [vigia](https://github.com/breferrari/vigia), until a hayro release carries them. It is kept by vigia's maintainer, and it is not affiliated with or endorsed by hayro or its author. Report problems with this fork at <https://github.com/breferrari/vigia/issues>, not to hayro.

hayro-interpret is licensed under Apache-2.0 or MIT, and so is this fork. Upstream's README follows, without its badges, which describe upstream's crate.

---

# hayro-interpret

<!-- cargo-rdme start -->

A crate for interpreting PDF files.

This crate provides an abstraction to interpret the content of a PDF file and render them
into an abstract [`Device`], which clients can implement as needed. This allows you, for
example, to render PDF files to bitmaps (which is what the `hayro` crate does), or other formats
such as SVG.

It should be noted that this crate is still very much in development. Therefore it currently
lacks pretty much any documentation on how to use it. It's current API also only really makes it
useful for rendering to PNG or SVG, though this will be improved upon in the future.

## Examples
See the `examples` folder on the GitHub repository. Apart from that, you can also consult
the implementation of `hayro` and `hayro-svg` to get an idea on how to use this crate.

## Safety
This crate forbids unsafe code via a crate-level attribute.

## Cargo features
This crate has one optional feature:
- `embed-fonts`: PDF processors are required to support 14 predefined fonts that do not need to be
  embedded into a PDF file. If you enable this feature, hayro will embed a (permissively-licensed)
  substitute for each font, so that you don't have to implement your custom font loading logic. This
  will add around ~240KB to your binary.

<!-- cargo-rdme end -->

## License
Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option. 
