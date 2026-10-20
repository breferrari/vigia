# vigia-hayro

**This is a fork. Use [hayro](https://crates.io/crates/hayro) instead.**

`vigia-hayro` is [hayro](https://github.com/LaurenzV/hayro) 0.8.0 with no change to its code. It exists so that it builds on `vigia-hayro-interpret`, which builds on `vigia-hayro-syntax`, which carries the patches listed in [`PATCHES.md`](https://github.com/breferrari/vigia/blob/vigia-hayro-0.8.0/vendor/hayro-syntax/PATCHES.md), for [vigia](https://github.com/breferrari/vigia), until a hayro release carries them. It is kept by vigia's maintainer, and it is not affiliated with or endorsed by hayro or its author. Report problems with this fork at <https://github.com/breferrari/vigia/issues>, not to hayro.

hayro is licensed under Apache-2.0 or MIT, and so is this fork. Upstream's README follows, without its badges, which describe upstream's crate.

---

# hayro

<!-- cargo-rdme start -->

A crate for rendering PDF files.

This crate allows you to render pages of a PDF file into bitmaps. It is supposed to be relatively
lightweight, since we do not have any dependencies on the GPU. All the rendering happens on the CPU.

The ultimate goal of this crate is to be a *feature-complete* and *performant* PDF rasterizer.
With that said, we are currently still very far away from reaching that goal: So far, no effort
has been put into performance optimizations, as we are still working on implementing missing features.
However, this crate is currently the most comprehensive and feature-complete
implementation of a PDF rasterizer in pure Rust. This claim is supported by the fact that we currently
include over 1000 PDF files in our regression test suite. The majority of those have been scraped
from the `pdf.js` and `PDFBOX` test suites and therefore represent a very large and diverse sample
of PDF files.

As mentioned, there are still some serious limitations, including lack of support for
encrypted/password-protected PDF files, blending and isolation, knockout groups as well as a range
of smaller features such as color key masking. But you should be able to render the vast majority
of PDF files without too many issues.

### Safety
This crate forbids unsafe code via a crate-level attribute.

### Examples
For usage examples, see the [example](https://github.com/LaurenzV/hayro/tree/master/hayro/examples) in
the GitHub repository.

### Cargo features
This crate has one optional feature:
- `embed-fonts`: See the description of [`hayro-interpret`](https://docs.rs/hayro-interpret/latest/hayro_interpret/#cargo-features) for more information.

<!-- cargo-rdme end -->

## License
Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option. 
