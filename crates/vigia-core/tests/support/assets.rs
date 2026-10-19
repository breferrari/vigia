//! Images and SVGs generated in code, and the git config a fixture repository
//! takes, for the tests and for the scene bed in `xtask`. Depends on `image`
//! alone, so both can include it.

/// The config every fixture repository sets. Each one stops a developer's
/// global config from changing what is measured: autocrlf in particular is
/// commonly true on Windows and would rewrite line endings. **The fsmonitor
/// pair is for `crates/vigia/tests/writes.rs`**, which asserts that nothing
/// under the worktree moves while `vigia` runs: with `core.fsmonitor` on, a
/// `git add` spawns a `git-fsmonitor--daemon` that outlives the command and
/// keeps writing inside `.git`.
pub const GIT_CONFIG: [(&str, &str); 7] = [
    ("core.autocrlf", "false"),
    ("core.safecrlf", "false"),
    ("user.email", "test@example.invalid"),
    ("user.name", "vigia tests"),
    ("commit.gpgsign", "false"),
    ("core.fsmonitor", "false"),
    ("core.untrackedCache", "false"),
];

/// The CRC that closes a PNG chunk of `kind` holding `data`.
fn chunk_crc(kind: &[u8], data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in kind.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

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

/// A `width` by `height` image whose red is `seed` and whose green and blue
/// step with the column and the row.
pub fn gradient(width: u32, height: u32, seed: u8) -> image::RgbaImage {
    image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([seed, (x % 256) as u8, (y % 256) as u8, 255])
    })
}

/// An image of `width` by `height` in `format`, every pixel from `seed`.
pub fn encoded(format: image::ImageFormat, width: u32, height: u32, seed: u8) -> Vec<u8> {
    encode_image(&gradient(width, height, seed), format)
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
    let image = gradient(width, height, seed);
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

/// A PNG whose chunks are whole, checksums included, and whose compressed
/// pixel data is garbled, so its header reads and its pixels do not. Only a
/// size whose pixels fit one data chunk is taken.
pub fn corrupt_png(width: u32, height: u32, seed: u8) -> Vec<u8> {
    let mut bytes = png(width, height, seed);
    let at = bytes
        .windows(4)
        .position(|window| window == b"IDAT")
        .expect("a PNG has pixel data");
    assert_eq!(
        bytes.windows(4).filter(|window| *window == b"IDAT").count(),
        1,
        "a {width}x{height} PNG spans several data chunks, and only one is garbled"
    );
    let length = u32::from_be_bytes(bytes[at - 4..at].try_into().expect("four bytes")) as usize;
    for byte in &mut bytes[at + 4..at + 4 + length] {
        *byte ^= 0x5a;
    }
    let crc = chunk_crc(b"IDAT", &bytes[at + 4..at + 4 + length]);
    bytes[at + 4 + length..at + 8 + length].copy_from_slice(&crc.to_be_bytes());
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
pub fn costly_svg() -> &'static str {
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24"><filter id="t"><feTurbulence baseFrequency="0.5" numOctaves="100000000"/></filter><rect width="24" height="24" filter="url(#t)"/></svg>"#
}

/// A PNG whose header claims `width` by `height` and whose pixel data is a
/// few bytes, so it sizes as large as it says without being that large.
pub fn claimed_png(width: u32, height: u32) -> Vec<u8> {
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&chunk_crc(kind, data).to_be_bytes());
        out
    }
    let mut header = width.to_be_bytes().to_vec();
    header.extend_from_slice(&height.to_be_bytes());
    // Eight bits, RGBA, the only compression, filter and interlace methods.
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(chunk(b"IHDR", &header));
    bytes.extend(chunk(
        b"IDAT",
        &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
    ));
    bytes.extend(chunk(b"IEND", &[]));
    bytes
}
