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

/// Paths and their bytes.
pub type Files = Vec<(String, Vec<u8>)>;

/// A refactor of `n` similar `.rs` files of about 3 KB: each one's path and
/// bytes before, then after it moved to another directory with a light edit.
pub fn refactor_files(n: usize) -> (Files, Files) {
    let body = |k: usize| -> String {
        (0..60)
            .map(|l| format!("fn item_{k}_{l}(x: u32) -> u32 {{ x + {l} }} // module {k}\n"))
            .collect()
    };
    let before = (0..n)
        .map(|k| (format!("old/m{k:04}.rs"), body(k).into_bytes()))
        .collect();
    let after = (0..n)
        .map(|k| {
            let edited = body(k).replacen("x + 3 ", "x + 33 ", 1) + "// moved\n";
            (format!("new/n{k:04}.rs"), edited.into_bytes())
        })
        .collect();
    (before, after)
}

/// `length` bytes of noise from `seed`, a NUL first, so each reads as
/// binary and two seeds share no chunk.
pub fn noise(seed: u64, length: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut out: Vec<u8> = (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    out[0] = 0;
    out
}

/// `bytes` with its last `share` of them changed.
pub fn tail_changed(bytes: &[u8], share: f64) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let from = out.len() - (out.len() as f64 * share) as usize;
    for byte in &mut out[from..] {
        *byte ^= 0x5a;
    }
    out
}

/// A PNG of `width` by `height`, and the same with its last pixel changed,
/// as an edit that leaves most of the file's bytes alone.
pub fn edited_png(width: u32, height: u32) -> (Vec<u8>, Vec<u8>) {
    let image = gradient(width, height, 1);
    let mut edited = image.clone();
    edited.put_pixel(width - 1, height - 1, image::Rgba([255, 0, 0, 255]));
    (
        encode_image(&image, image::ImageFormat::Png),
        encode_image(&edited, image::ImageFormat::Png),
    )
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

/// A PDF whose pages are `width` by `height` points, each drawing `content`
/// listed `repeat` times in its `/Contents`, with `resources` as every page's
/// resources and `trailer` added to the trailer dictionary. `extras` follow
/// as objects of their own, numbered from `3 + 2 * pages`.
pub fn pdf_doc(
    pages: &[(u32, u32, &[u8])],
    resources: &str,
    repeat: usize,
    extras: &[&[u8]],
    trailer: &str,
) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    let object = |out: &mut Vec<u8>, body: &[u8], offsets: &mut Vec<usize>| {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend(body);
        out.extend(b"\nendobj\n");
    };
    let kids: Vec<String> = (0..pages.len())
        .map(|page| format!("{} 0 R", 3 + 2 * page))
        .collect();
    object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>", &mut offsets);
    object(
        &mut out,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages.len()
        )
        .as_bytes(),
        &mut offsets,
    );
    for (page, (width, height, content)) in pages.iter().enumerate() {
        let stream = format!("{} 0 R", 4 + 2 * page);
        let contents = if repeat == 1 {
            stream
        } else {
            format!("[{}]", vec![stream; repeat].join(" "))
        };
        object(
            &mut out,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << {resources} >> /Contents {contents} >>"
            )
            .as_bytes(),
            &mut offsets,
        );
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend_from_slice(content);
        stream.extend_from_slice(b"\nendstream");
        object(&mut out, &stream, &mut offsets);
    }
    for extra in extras {
        object(&mut out, extra, &mut offsets);
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for offset in &offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R {trailer}>>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A PDF of `pages`, with `trailer` added to its trailer dictionary.
pub fn pdf_with(pages: &[(u32, u32, &[u8])], trailer: &str) -> Vec<u8> {
    pdf_doc(pages, "", 1, &[], trailer)
}

/// The resources a page needs to set text in Helvetica as `/F1`.
pub const HELVETICA: &str =
    "/Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >>";

/// A 48 by 24 point PDF that sets `label` in Helvetica.
pub fn label_pdf(label: &str) -> Vec<u8> {
    let content = format!("BT /F1 12 Tf 4 8 Td ({label}) Tj ET");
    pdf_doc(&[(48, 24, content.as_bytes())], HELVETICA, 1, &[], "")
}

/// A PDF of `pages`, each `width` by `height` points drawing `content`.
pub fn pdf(pages: &[(u32, u32, &[u8])]) -> Vec<u8> {
    pdf_with(pages, "")
}

/// The drawing of an icon-shaped page: a filled square in a colour from
/// `seed`, and a stroked check mark.
pub fn icon_ops(seed: u8) -> Vec<u8> {
    format!(
        "{:.3} 0.4 0.9 rg 2 2 20 20 re f 1 1 1 RG 2 w 7 12 m 10 15 l 17 8 l S",
        f32::from(seed) / 255.0
    )
    .into_bytes()
}

/// A 24 by 24 point icon PDF, its colour from `seed`.
pub fn icon_pdf(seed: u8) -> Vec<u8> {
    pdf(&[(24, 24, &icon_ops(seed))])
}

/// A PDF that needs a password to open.
pub fn locked_pdf() -> Vec<u8> {
    pdf_with(
        &[(24, 24, &icon_ops(1))],
        "/Encrypt << /Filter /Standard /V 2 /R 3 /O (xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx) /U (yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy) /P -4 /Length 128 >> /ID [(abc)(abc)] ",
    )
}

/// MD5, for the key an encrypted PDF fixture needs.
fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64)
        .map(|i| ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32)
        .collect();
    let mut message = input.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((input.len() as u64) * 8).to_le_bytes());
    let mut state = [0x6745_2301u32, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    for block in message.chunks(64) {
        let words: Vec<u32> = block
            .chunks(4)
            .map(|word| u32::from_le_bytes(word.try_into().expect("four bytes")))
            .collect();
        let [mut a, mut b, mut c, mut d] = state;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let rotated = a
                .wrapping_add(f)
                .wrapping_add(k[i])
                .wrapping_add(words[g])
                .rotate_left(S[i]);
            (a, d, c, b) = (d, c, b, b.wrapping_add(rotated));
        }
        for (held, new) in state.iter_mut().zip([a, b, c, d]) {
            *held = held.wrapping_add(new);
        }
    }
    let mut out = [0u8; 16];
    for (chunk, word) in out.chunks_mut(4).zip(state) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// RC4 of `data` under `key`.
fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut s: Vec<u8> = (0..=255).collect();
    let mut j = 0u8;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, usize::from(j));
    }
    let (mut i, mut j) = (0u8, 0u8);
    data.iter()
        .map(|byte| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(s[usize::from(i)]);
            s.swap(usize::from(i), usize::from(j));
            byte ^ s[usize::from(s[usize::from(i)].wrapping_add(s[usize::from(j)]))]
        })
        .collect()
}

/// A one-page icon PDF encrypted with RC4 and an empty user password, as a
/// document protected only by an owner password is: any viewer opens it.
pub fn open_locked_pdf(seed: u8) -> Vec<u8> {
    const PAD: [u8; 32] = [
        0x28, 0xbf, 0x4e, 0x5e, 0x4e, 0x75, 0x8a, 0x41, 0x64, 0x00, 0x4e, 0x56, 0xff, 0xfa, 0x01,
        0x08, 0x2e, 0x2e, 0x00, 0xb6, 0xd0, 0x68, 0x3e, 0x80, 0x2f, 0x0c, 0xa9, 0xfe, 0x64, 0x53,
        0x69, 0x7a,
    ];
    let (id, permissions) = (b"vigia-fixture-id", -4i32);
    let owner = rc4(&md5(&PAD)[..5], &PAD);
    let mut seeded = PAD.to_vec();
    seeded.extend_from_slice(&owner);
    seeded.extend_from_slice(&permissions.to_le_bytes());
    seeded.extend_from_slice(id);
    let key = md5(&seeded)[..5].to_vec();
    let user = rc4(&key, &PAD);
    // The content stream is object 4, generation 0.
    let mut object_key = key.clone();
    object_key.extend_from_slice(&[4, 0, 0, 0, 0]);
    let content = rc4(&md5(&object_key)[..10], &icon_ops(seed));
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    pdf_with(
        &[(24, 24, &content)],
        &format!(
            "/Encrypt << /Filter /Standard /V 1 /R 2 /O <{}> /U <{}> /P {permissions} >> /ID [<{}> <{}>] ",
            hex(&owner),
            hex(&user),
            hex(id),
            hex(id)
        ),
    )
}

/// A zlib stream that decodes to `length` zero bytes from about one
/// two-hundredth of that: one fixed-Huffman block of a literal and then
/// runs of 258 copied from one back. A decompression bomb, without a
/// compressor.
pub fn zlib_zeros(length: u64) -> Vec<u8> {
    zlib_filled(0, length)
}

/// [`zlib_zeros`] of `byte`, which must be under 144 to take an eight-bit
/// literal code.
pub fn zlib_filled(byte: u8, length: u64) -> Vec<u8> {
    struct Bits {
        out: Vec<u8>,
        held: u64,
        count: u32,
    }
    impl Bits {
        fn push(&mut self, value: u64, bits: u32) {
            self.held |= value << self.count;
            self.count += bits;
            while self.count >= 8 {
                self.out.push(self.held as u8);
                self.held >>= 8;
                self.count -= 8;
            }
        }
        /// A Huffman code, which deflate packs from its top bit.
        fn code(&mut self, code: u64, bits: u32) {
            for bit in (0..bits).rev() {
                self.push((code >> bit) & 1, 1);
            }
        }
    }
    let mut bits = Bits {
        out: vec![0x78, 0x01],
        held: 0,
        count: 0,
    };
    // The last block, with the fixed codes.
    bits.push(1, 1);
    bits.push(1, 2);
    // A literal under 144 is code 0x30 on, in eight bits.
    let literal = 0x30 + u64::from(byte);
    bits.code(literal, 8);
    let mut written = 1u64;
    while written < length {
        let run = (length - written).min(258);
        // Length codes from 257 (seven bits from 0 for 256..279, eight from
        // 0xc0 for 280..287); runs under 258 end the stream with literals.
        if run == 258 {
            bits.code(0xc5, 8);
            bits.code(0, 5);
        } else {
            for _ in 0..run {
                bits.code(literal, 8);
            }
        }
        written += run;
    }
    // End of block.
    bits.code(0, 7);
    if bits.count > 0 {
        bits.push(0, 8 - bits.count);
    }
    // Adler-32 of one byte repeated: the low sum after k bytes is 1 + k*b,
    // and the high sum adds those up.
    let (n, b) = (u128::from(length), u128::from(byte));
    let low = (1 + n * b) % 65_521;
    let high = (n + b * n * (n + 1) / 2) % 65_521;
    let adler = ((high << 16) | low) as u64;
    bits.out.extend_from_slice(&(adler as u32).to_be_bytes());
    bits.out
}

/// `count` zeros as raw deflate, each its own literal: one fixed-Huffman
/// block that grows its output a byte at a time.
pub fn deflate_literals(count: usize) -> Vec<u8> {
    let (mut out, mut held, mut bits) = (Vec::new(), 0u64, 0u32);
    let mut push = |value: u64, width: u32| {
        held |= value << bits;
        bits += width;
        while bits >= 8 {
            out.push(held as u8);
            held >>= 8;
            bits -= 8;
        }
    };
    // The last block, with the fixed codes.
    push(1, 1);
    push(1, 2);
    // A literal 0 is code 0x30 in eight bits, which deflate packs from its
    // top bit; 0x30 reversed is 0x0c.
    for _ in 0..count {
        push(0x0c, 8);
    }
    // End of block, and the last byte's padding.
    push(0, 7);
    push(0, 7);
    out
}

/// `count` zeros, under 65,536, as one stored raw deflate block.
pub fn deflate_stored(count: u16) -> Vec<u8> {
    let mut out = vec![0x01];
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&(!count).to_le_bytes());
    out.extend(std::iter::repeat_n(0, usize::from(count)));
    out
}

/// `length` zeros as zlib that flate2 refuses and a lenient inflater reads: a
/// stored block whose length check is wrong comes before the data.
pub fn zlib_zeros_lenient(length: u64) -> Vec<u8> {
    let whole = zlib_zeros(length);
    [&whole[..2], &[0x00, 0x00, 0x00, 0x01, 0x00], &whole[2..]].concat()
}

/// `length` zeros, rounded up to whole runs, as LZW with the early change:
/// each code after the first repeats the last one and one zero more, until
/// the table is full, and then, with `repeat`, its longest entry repeats, or
/// without, the table clears and fills again.
fn lzw_built(length: u64, repeat: bool) -> Vec<u8> {
    let (mut out, mut held, mut count) = (Vec::new(), 0u64, 0u32);
    let mut push = |code: u64, bits: u32| {
        held = (held << bits) | code;
        count += bits;
        while count >= 8 {
            count -= 8;
            out.push((held >> count) as u8);
        }
    };
    let width = |size: u64| match size + 1 {
        ..512 => 9,
        512..1024 => 10,
        1024..2048 => 11,
        _ => 12,
    };
    let mut written = 0;
    let mut last = 9;
    while written < length {
        push(256, last);
        push(0, 9);
        written += 1;
        let mut size = 258;
        while size < 4096 && written < length {
            push(size, width(size));
            written += size - 256;
            size += 1;
        }
        last = width(size);
        // The table is full: code 4095 holds 3839 zeros.
        while repeat && written < length {
            push(4095, 12);
            written += 3839;
        }
    }
    push(257, last);
    // Enough padding to flush what is left of the end code.
    push(0, 7);
    out
}

/// [`lzw_built`], the table filled again and again.
pub fn lzw_zeros(length: u64) -> Vec<u8> {
    lzw_built(length, false)
}

/// [`lzw_built`], one full table's longest entry repeated.
pub fn lzw_repeats(length: u64) -> Vec<u8> {
    lzw_built(length, true)
}

/// `length` zeros, rounded up to whole runs, as `RunLength`: 128 a run.
pub fn run_length_zeros(length: u64) -> Vec<u8> {
    let mut out = [129u8, 0].repeat(length.div_ceil(128) as usize);
    out.push(128);
    out
}

/// `rows` all-white rows in CCITT Group 4, one bit each.
pub fn ccitt_white(rows: usize) -> Vec<u8> {
    vec![0xff; rows.div_ceil(8)]
}

/// An icon PDF that also holds, as its own object, a stream whose dictionary
/// is `dict` and whose data, after `stream` and `eol`, is `data`.
pub fn pdf_holding(dict: &str, eol: &[u8], data: &[u8]) -> Vec<u8> {
    let mut object = format!("<< {dict} /Length {} >>\nstream", data.len()).into_bytes();
    object.extend_from_slice(eol);
    object.extend_from_slice(data);
    object.extend_from_slice(b"\nendstream");
    pdf_doc(&[(24, 24, &icon_ops(2))], "", 1, &[&object], "")
}

/// `base` with an incremental update appended: one more object, a stream
/// whose dictionary is `dict` and whose data is `data`, in a second xref
/// section whose trailer points back at the first.
pub fn pdf_updated(base: &[u8], dict: &str, data: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(base);
    let previous: usize = text
        .rsplit("startxref\n")
        .next()
        .and_then(|tail| tail.lines().next())
        .and_then(|line| line.trim().parse().ok())
        .expect("a startxref");
    let size: usize = text
        .split("/Size ")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .and_then(|size| size.parse().ok())
        .expect("a trailer size");
    let mut out = base.to_vec();
    let offset = out.len();
    out.extend(
        format!(
            "{size} 0 obj\n<< {dict} /Length {} >>\nstream\n",
            data.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = out.len();
    out.extend(
        format!(
            "xref\n{size} 1\n{offset:010} 00000 n \ntrailer\n<< /Size {} /Root 1 0 R /Prev {previous} >>\nstartxref\n{xref}\n%%EOF\n",
            size + 1
        )
        .as_bytes(),
    );
    out
}

/// An icon PDF whose cross-reference is a stream, compressed as `data`, as
/// PDF 1.5 writes one: a reader decodes it before anything else.
pub fn pdf_xref_stream(data: &[u8]) -> Vec<u8> {
    pdf_xref_stream_with(data, "")
}

/// [`pdf_xref_stream`] with `extra` in the stream's dictionary.
pub fn pdf_xref_stream_with(data: &[u8], extra: &str) -> Vec<u8> {
    let doc = pdf(&[(24, 24, &icon_ops(2))]);
    // Everything before the table, which the stream takes the place of.
    let body = &doc[..memchr_xref(&doc)];
    let mut out = body.to_vec();
    let offset = out.len();
    out.extend(
        format!(
            "5 0 obj\n<< /Type /XRef /Size 6 /W [1 4 2] /Root 1 0 R /Filter /FlateDecode {extra}/Length {} >>\nstream\n",
            data.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(data);
    out.extend(format!("\nendstream\nendobj\nstartxref\n{offset}\n%%EOF\n").as_bytes());
    out
}

fn memchr_xref(doc: &[u8]) -> usize {
    doc.windows(5)
        .rposition(|window| window == b"xref\n")
        .expect("a cross-reference table")
}
