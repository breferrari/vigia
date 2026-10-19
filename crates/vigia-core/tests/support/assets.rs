//! Images and SVGs generated in code, for the tests and for the scene bed in
//! `xtask`. Depends on `image` alone, so both can include it.

/// `image` encoded as `format`. JPEG has no alpha channel to encode.
pub fn encode_image(image: &image::RgbaImage, format: image::ImageFormat) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    match format {
        image::ImageFormat::Jpeg => image::DynamicImage::ImageRgba8(image.clone())
            .to_rgb8()
            .write_to(&mut bytes, format),
        _ => image.write_to(&mut bytes, format),
    }
    .expect("encode the fixture");
    bytes.into_inner()
}

/// An image of `width` by `height` in `format`, every pixel from `seed`.
pub fn encoded(format: image::ImageFormat, width: u32, height: u32, seed: u8) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([seed, (x % 256) as u8, (y % 256) as u8, 255])
    });
    encode_image(&image, format)
}

/// A phone-sized screenshot as a PNG, its colours from `accent`.
pub fn screen_png(accent: u8) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(1170, 2532, |x, y| {
        image::Rgba([accent, (x % 251) as u8, (y % 241) as u8, 255])
    });
    encode_image(&image, image::ImageFormat::Png)
}

/// An icon-shaped SVG: a gradient circle and a stroked path, its colours
/// moved by `seed`.
pub fn icon_svg(seed: u8) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><defs><linearGradient id="g" x1="0" x2="1"><stop offset="0" stop-color="#{seed:02x}0000"/><stop offset="1" stop-color="#0000ff"/></linearGradient></defs><circle cx="12" cy="12" r="10" fill="url(#g)"/><path d="M7 12l3 3 7-7" stroke="#fff" stroke-width="2" fill="none"/></svg>"##
    )
}

/// A PNG of `width` by `height`, every pixel from `seed`.
pub fn png(width: u32, height: u32, seed: u8) -> Vec<u8> {
    encoded(image::ImageFormat::Png, width, height, seed)
}

/// A PNG whose pixels come from `seed` and whose ICC chunk holds `profile`,
/// so two that differ only in it compare as a profile change.
pub fn profiled_png(width: u32, height: u32, seed: u8, profile: &[u8]) -> Vec<u8> {
    use image::ImageEncoder;
    let image = image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([seed, (x % 256) as u8, (y % 256) as u8, 255])
    });
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    encoder
        .set_icc_profile(profile.to_vec())
        .expect("PNG takes a profile");
    encoder
        .write_image(&image, width, height, image::ExtendedColorType::Rgba8)
        .expect("encode the fixture");
    bytes
}

/// A GIF of `frames`, in order.
pub fn animated_gif(frames: &[image::RgbaImage]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        encoder
            .encode_frames(frames.iter().cloned().map(image::Frame::new))
            .expect("encode the fixture");
    }
    bytes
}

/// A PNG whose header is whole and whose pixel data is not.
pub fn corrupt_png(width: u32, height: u32, seed: u8) -> Vec<u8> {
    let mut bytes = png(width, height, seed);
    // Past the signature and the IHDR chunk.
    for byte in bytes.iter_mut().skip(33) {
        *byte ^= 0x5a;
    }
    bytes
}

/// The text of an LFS pointer to `content`, and its oid, made from `seed`.
/// The pane reads the object at the oid's path and never hashes it, so any 64
/// hex digits serve.
pub fn lfs_pointer(content: &[u8], seed: u8) -> (String, String) {
    let oid = format!("{seed:02x}").repeat(32);
    let text = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
        content.len()
    );
    (oid, text)
}

/// A 64x24 SVG holding one `<text>` label.
pub fn labelled_svg(label: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="24"><rect width="64" height="24" fill="#202830"/><text x="4" y="17" font-size="14" fill="#ffffff">{label}</text></svg>"##
    )
}

/// An SVG whose filter costs more than a picture may take, so it draws
/// `could not be decoded`.
pub fn costly_svg() -> String {
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><filter id="t"><feTurbulence baseFrequency="0.5" numOctaves="100000000"/></filter><rect width="24" height="24" filter="url(#t)"/></svg>"#.to_owned()
}
