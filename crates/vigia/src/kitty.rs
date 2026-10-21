//! The Kitty graphics protocol, as vigia sends it: the transmit, chunked, and
//! the Unicode placeholder cells that place it. `SPEC.md` §11.1.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
use ratatui::style::Color;

/// The character kitty reserves for an image's cells.
const PLACEHOLDER: char = '\u{10EEEE}';

/// The most base64 one chunk carries. Every chunk but the last is a multiple of
/// four, which 3072 bytes of data always encode to.
const CHUNK: usize = 4096;

/// A cell drawn by the terminal's image rather than by its own glyph.
const UNIT_WIDTH: ratatui::buffer::CellDiffOption =
    ratatui::buffer::CellDiffOption::ForcedWidth(std::num::NonZeroU16::MIN);

/// The transmit of `rgba`, `width` by `height` pixels, as image `id` placed
/// virtually over `cells`: one string a chunk, each its own escape, and under
/// tmux each in its own passthrough.
fn transmit(
    rgba: &[u8],
    (width, height): (u32, u32),
    cells: Size,
    id: u32,
    tmux: bool,
    compress: bool,
) -> Vec<String> {
    let deflated;
    let bytes = if compress {
        deflated = zlib(rgba);
        &deflated[..]
    } else {
        rgba
    };
    let (start, escape, end) = if tmux {
        ("\x1bPtmux;", "\x1b\x1b", "\x1b\\")
    } else {
        ("", "\x1b", "")
    };
    let pieces = bytes.chunks(CHUNK / 4 * 3);
    let count = pieces.len();
    pieces
        .enumerate()
        .map(|(i, piece)| {
            let mut chunk = String::with_capacity(CHUNK + 96);
            chunk.push_str(start);
            let _ = write!(chunk, "{escape}_Gq=2,");
            if i == 0 {
                let compression = if compress { "o=z," } else { "" };
                let _ = write!(
                    chunk,
                    "i={id},a=T,U=1,f=32,{compression}t=d,s={width},v={height},c={},r={},",
                    cells.width, cells.height
                );
            }
            let _ = write!(chunk, "m={};", u8::from(i + 1 < count));
            base64_simd::STANDARD.encode_append(piece, &mut chunk);
            let _ = write!(chunk, "{escape}\\");
            chunk.push_str(end);
            chunk
        })
        .collect()
}

/// The most of an image [`worth_deflating`] deflates to decide.
const SAMPLE: usize = 64 * 1024;

/// Whether deflating `rgba` pays: a sample from across the image shrinks at
/// least twice. A photograph shrinks about 1.5 times and costs 40 to 130 ms
/// to deflate whole; the sample costs under half a millisecond.
pub fn worth_deflating(rgba: &[u8]) -> bool {
    if rgba.len() <= SAMPLE {
        return true;
    }
    // Strips from top to bottom, since a screenshot's first rows are its
    // flattest.
    const STRIPS: usize = 16;
    let step = rgba.len() / STRIPS;
    let mut sample = Vec::with_capacity(SAMPLE);
    for i in 0..STRIPS {
        sample.extend_from_slice(&rgba[i * step..i * step + SAMPLE / STRIPS]);
    }
    zlib(&sample).len() * 2 <= sample.len()
}

/// Deflate `raw` into the zlib stream `o=z` names, at the fastest level: the
/// encode is the cost, not the bytes.
fn zlib(raw: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    // Into a `Vec`, which cannot fail.
    encoder.write_all(raw).expect("deflate into a Vec");
    encoder.finish().expect("deflate into a Vec")
}

/// Draw image `id`'s placeholder cells over `area`, an image `size` cells big,
/// its first `skip` rows above the area. `first` goes in front of the first
/// cell, as the transmit does on the frame that sends it.
fn placeholders(
    area: Rect,
    size: Size,
    id: u32,
    skip: u16,
    buf: &mut Buffer,
    mut first: Option<&str>,
) {
    let [high, r, g, b] = id.to_be_bytes();
    let fg = Color::Rgb(r, g, b);
    // A position past the table cannot be encoded, so it is left undrawn
    // rather than drawn at the wrong offset.
    let most = DIACRITICS.len() as u16;
    let width = area.width.min(size.width).min(most);
    let height = area.height.min(size.height).min(most);
    let mut symbol = String::with_capacity(64);
    for y in 0..height {
        for x in 0..width {
            let Some(cell) = buf.cell_mut((area.left() + x, area.top() + y)) else {
                continue;
            };
            symbol.clear();
            if let Some(first) = first.take() {
                symbol.push_str(first);
            }
            symbol.push(PLACEHOLDER);
            if x == 0 {
                symbol.push(diacritic(y + skip));
                symbol.push(diacritic(x));
                symbol.push(diacritic(u16::from(high)));
            }
            cell.set_symbol(&symbol)
                .set_fg(fg)
                .set_diff_option(UNIT_WIDTH);
        }
    }
}

/// A Kitty image, encoded, its transmit kept for the frame that first draws it.
pub struct Image {
    id: u32,
    cells: Size,
    transmit: String,
    sent: AtomicBool,
    deflated: bool,
}

impl Image {
    /// `rgba`, `width` by `height` pixels, as image `id` over `cells`.
    pub fn new(
        rgba: &[u8],
        pixels: (u32, u32),
        cells: Size,
        id: u32,
        tmux: bool,
        compress: bool,
    ) -> Self {
        Self {
            id,
            cells,
            transmit: transmit(rgba, pixels, cells, id, tmux, compress).concat(),
            sent: AtomicBool::new(false),
            deflated: compress,
        }
    }

    /// Whether its transmit is deflated.
    pub fn deflated(&self) -> bool {
        self.deflated
    }

    /// The cells it covers.
    pub fn size(&self) -> Size {
        self.cells
    }

    /// Draw it into `area` with its first `above` rows scrolled above the
    /// area's top. The first draw carries the transmit.
    pub fn render(&self, area: Rect, buf: &mut Buffer, above: u16) {
        if area.is_empty() || above >= self.cells.height {
            return;
        }
        let room = Rect {
            width: area.width.min(self.cells.width),
            height: area.height.min(self.cells.height - above),
            ..area
        };
        let first = (!self.sent.swap(true, Ordering::AcqRel)).then_some(self.transmit.as_str());
        placeholders(room, self.cells, self.id, above, buf, first);
    }
}

/// The diacritic that encodes `n`.
fn diacritic(n: u16) -> char {
    DIACRITICS
        .get(usize::from(n))
        .copied()
        .unwrap_or(DIACRITICS[0])
}

/// The row, column and id diacritics, in order: kitty's `rowcolumn-diacritics.txt`.
static DIACRITICS: [char; 297] = [
    '\u{305}',
    '\u{30D}',
    '\u{30E}',
    '\u{310}',
    '\u{312}',
    '\u{33D}',
    '\u{33E}',
    '\u{33F}',
    '\u{346}',
    '\u{34A}',
    '\u{34B}',
    '\u{34C}',
    '\u{350}',
    '\u{351}',
    '\u{352}',
    '\u{357}',
    '\u{35B}',
    '\u{363}',
    '\u{364}',
    '\u{365}',
    '\u{366}',
    '\u{367}',
    '\u{368}',
    '\u{369}',
    '\u{36A}',
    '\u{36B}',
    '\u{36C}',
    '\u{36D}',
    '\u{36E}',
    '\u{36F}',
    '\u{483}',
    '\u{484}',
    '\u{485}',
    '\u{486}',
    '\u{487}',
    '\u{592}',
    '\u{593}',
    '\u{594}',
    '\u{595}',
    '\u{597}',
    '\u{598}',
    '\u{599}',
    '\u{59C}',
    '\u{59D}',
    '\u{59E}',
    '\u{59F}',
    '\u{5A0}',
    '\u{5A1}',
    '\u{5A8}',
    '\u{5A9}',
    '\u{5AB}',
    '\u{5AC}',
    '\u{5AF}',
    '\u{5C4}',
    '\u{610}',
    '\u{611}',
    '\u{612}',
    '\u{613}',
    '\u{614}',
    '\u{615}',
    '\u{616}',
    '\u{617}',
    '\u{657}',
    '\u{658}',
    '\u{659}',
    '\u{65A}',
    '\u{65B}',
    '\u{65D}',
    '\u{65E}',
    '\u{6D6}',
    '\u{6D7}',
    '\u{6D8}',
    '\u{6D9}',
    '\u{6DA}',
    '\u{6DB}',
    '\u{6DC}',
    '\u{6DF}',
    '\u{6E0}',
    '\u{6E1}',
    '\u{6E2}',
    '\u{6E4}',
    '\u{6E7}',
    '\u{6E8}',
    '\u{6EB}',
    '\u{6EC}',
    '\u{730}',
    '\u{732}',
    '\u{733}',
    '\u{735}',
    '\u{736}',
    '\u{73A}',
    '\u{73D}',
    '\u{73F}',
    '\u{740}',
    '\u{741}',
    '\u{743}',
    '\u{745}',
    '\u{747}',
    '\u{749}',
    '\u{74A}',
    '\u{7EB}',
    '\u{7EC}',
    '\u{7ED}',
    '\u{7EE}',
    '\u{7EF}',
    '\u{7F0}',
    '\u{7F1}',
    '\u{7F3}',
    '\u{816}',
    '\u{817}',
    '\u{818}',
    '\u{819}',
    '\u{81B}',
    '\u{81C}',
    '\u{81D}',
    '\u{81E}',
    '\u{81F}',
    '\u{820}',
    '\u{821}',
    '\u{822}',
    '\u{823}',
    '\u{825}',
    '\u{826}',
    '\u{827}',
    '\u{829}',
    '\u{82A}',
    '\u{82B}',
    '\u{82C}',
    '\u{82D}',
    '\u{951}',
    '\u{953}',
    '\u{954}',
    '\u{F82}',
    '\u{F83}',
    '\u{F86}',
    '\u{F87}',
    '\u{135D}',
    '\u{135E}',
    '\u{135F}',
    '\u{17DD}',
    '\u{193A}',
    '\u{1A17}',
    '\u{1A75}',
    '\u{1A76}',
    '\u{1A77}',
    '\u{1A78}',
    '\u{1A79}',
    '\u{1A7A}',
    '\u{1A7B}',
    '\u{1A7C}',
    '\u{1B6B}',
    '\u{1B6D}',
    '\u{1B6E}',
    '\u{1B6F}',
    '\u{1B70}',
    '\u{1B71}',
    '\u{1B72}',
    '\u{1B73}',
    '\u{1CD0}',
    '\u{1CD1}',
    '\u{1CD2}',
    '\u{1CDA}',
    '\u{1CDB}',
    '\u{1CE0}',
    '\u{1DC0}',
    '\u{1DC1}',
    '\u{1DC3}',
    '\u{1DC4}',
    '\u{1DC5}',
    '\u{1DC6}',
    '\u{1DC7}',
    '\u{1DC8}',
    '\u{1DC9}',
    '\u{1DCB}',
    '\u{1DCC}',
    '\u{1DD1}',
    '\u{1DD2}',
    '\u{1DD3}',
    '\u{1DD4}',
    '\u{1DD5}',
    '\u{1DD6}',
    '\u{1DD7}',
    '\u{1DD8}',
    '\u{1DD9}',
    '\u{1DDA}',
    '\u{1DDB}',
    '\u{1DDC}',
    '\u{1DDD}',
    '\u{1DDE}',
    '\u{1DDF}',
    '\u{1DE0}',
    '\u{1DE1}',
    '\u{1DE2}',
    '\u{1DE3}',
    '\u{1DE4}',
    '\u{1DE5}',
    '\u{1DE6}',
    '\u{1DFE}',
    '\u{20D0}',
    '\u{20D1}',
    '\u{20D4}',
    '\u{20D5}',
    '\u{20D6}',
    '\u{20D7}',
    '\u{20DB}',
    '\u{20DC}',
    '\u{20E1}',
    '\u{20E7}',
    '\u{20E9}',
    '\u{20F0}',
    '\u{2CEF}',
    '\u{2CF0}',
    '\u{2CF1}',
    '\u{2DE0}',
    '\u{2DE1}',
    '\u{2DE2}',
    '\u{2DE3}',
    '\u{2DE4}',
    '\u{2DE5}',
    '\u{2DE6}',
    '\u{2DE7}',
    '\u{2DE8}',
    '\u{2DE9}',
    '\u{2DEA}',
    '\u{2DEB}',
    '\u{2DEC}',
    '\u{2DED}',
    '\u{2DEE}',
    '\u{2DEF}',
    '\u{2DF0}',
    '\u{2DF1}',
    '\u{2DF2}',
    '\u{2DF3}',
    '\u{2DF4}',
    '\u{2DF5}',
    '\u{2DF6}',
    '\u{2DF7}',
    '\u{2DF8}',
    '\u{2DF9}',
    '\u{2DFA}',
    '\u{2DFB}',
    '\u{2DFC}',
    '\u{2DFD}',
    '\u{2DFE}',
    '\u{2DFF}',
    '\u{A66F}',
    '\u{A67C}',
    '\u{A67D}',
    '\u{A6F0}',
    '\u{A6F1}',
    '\u{A8E0}',
    '\u{A8E1}',
    '\u{A8E2}',
    '\u{A8E3}',
    '\u{A8E4}',
    '\u{A8E5}',
    '\u{A8E6}',
    '\u{A8E7}',
    '\u{A8E8}',
    '\u{A8E9}',
    '\u{A8EA}',
    '\u{A8EB}',
    '\u{A8EC}',
    '\u{A8ED}',
    '\u{A8EE}',
    '\u{A8EF}',
    '\u{A8F0}',
    '\u{A8F1}',
    '\u{AAB0}',
    '\u{AAB2}',
    '\u{AAB3}',
    '\u{AAB7}',
    '\u{AAB8}',
    '\u{AABE}',
    '\u{AABF}',
    '\u{AAC1}',
    '\u{FE20}',
    '\u{FE21}',
    '\u{FE22}',
    '\u{FE23}',
    '\u{FE24}',
    '\u{FE25}',
    '\u{FE26}',
    '\u{10A0F}',
    '\u{10A38}',
    '\u{1D185}',
    '\u{1D186}',
    '\u{1D187}',
    '\u{1D188}',
    '\u{1D189}',
    '\u{1D1AA}',
    '\u{1D1AB}',
    '\u{1D1AC}',
    '\u{1D1AD}',
    '\u{1D242}',
    '\u{1D243}',
    '\u{1D244}',
];

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::widgets::Widget;
    use ratatui_image::protocol::kitty::Kitty;
    use ratatui_image::sliced::{SlicedImage, SlicedProtocol};

    /// An image whose pixels differ, so a chunk boundary or a byte out of place
    /// shows.
    fn image(width: u32, height: u32) -> image::RgbaImage {
        image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x * 7) as u8, (y * 13) as u8, (x ^ y) as u8, 255])
        })
    }

    /// What `ratatui-image`'s own encoder draws into `area`, `skip` rows above it.
    fn theirs(
        pixels: &image::RgbaImage,
        cells: Size,
        id: u32,
        tmux: bool,
        compress: bool,
        area: Rect,
        skip: u16,
    ) -> Buffer {
        let kitty = Kitty::new(
            image::DynamicImage::ImageRgba8(pixels.clone()),
            cells,
            id,
            tmux,
            compress,
            false,
        )
        .expect("encode");
        let mut buf = Buffer::empty(area);
        let above = -i16::try_from(skip).expect("skip");
        SlicedImage::new(&SlicedProtocol::Kitty(kitty), (0, above).into()).render(area, &mut buf);
        buf
    }

    /// What this module draws.
    fn ours(
        pixels: &image::RgbaImage,
        cells: Size,
        id: u32,
        tmux: bool,
        compress: bool,
        area: Rect,
        skip: u16,
    ) -> Buffer {
        let image = Image::new(
            pixels.as_raw(),
            pixels.dimensions(),
            cells,
            id,
            tmux,
            compress,
        );
        let mut buf = Buffer::empty(area);
        image.render(area, &mut buf, skip);
        buf
    }

    #[test]
    fn kitty_oracle() {
        // High id bytes 0, 1 and 255; tmux off and on; deflated and not; whole
        // and scrolled part way; and an image of more than one chunk.
        let cases = [
            (0x0000_0001, false, false, 0),
            (0x01AB_CDEF, false, true, 0),
            (0xFF12_3456, true, false, 3),
            (0xFF12_3456, true, true, 1),
        ];
        for (width, height, cells) in [(40, 40, Size::new(4, 2)), (300, 200, Size::new(30, 10))] {
            let pixels = image(width, height);
            for (id, tmux, compress, skip) in cases {
                let area = Rect::new(0, 0, cells.width, cells.height);
                let (theirs, ours) = (
                    theirs(
                        &pixels,
                        cells,
                        id,
                        tmux,
                        compress,
                        area,
                        skip.min(cells.height - 1),
                    ),
                    ours(
                        &pixels,
                        cells,
                        id,
                        tmux,
                        compress,
                        area,
                        skip.min(cells.height - 1),
                    ),
                );
                assert_eq!(
                    theirs, ours,
                    "{width}x{height} as id {id:#x}, tmux {tmux}, compressed {compress}, \
                     {skip} rows above: the cells differ from ratatui-image's"
                );
            }
        }
    }

    #[test]
    fn kitty_far_rows() {
        // Rows past 255 reach the table's end; past 297 they are left undrawn.
        let cells = Size::new(2, 300);
        let pixels = image(20, 3000);
        let area = Rect::new(0, 0, 2, 300);
        assert_eq!(
            theirs(&pixels, cells, 7, false, false, area, 0),
            ours(&pixels, cells, 7, false, false, area, 0),
            "a picture taller than the diacritics table is drawn otherwise than ratatui-image draws it"
        );
    }

    #[test]
    fn kitty_once() {
        // The transmit rides the first frame that draws the image and no other.
        let pixels = image(40, 40);
        let image = Image::new(
            pixels.as_raw(),
            pixels.dimensions(),
            Size::new(4, 2),
            3,
            false,
            false,
        );
        let area = Rect::new(0, 0, 4, 2);
        let drawn = |image: &Image| {
            let mut buf = Buffer::empty(area);
            image.render(area, &mut buf, 0);
            buf.content()
                .iter()
                .map(|cell| cell.symbol().len())
                .sum::<usize>()
        };
        let (first, second) = (drawn(&image), drawn(&image));
        assert!(
            first > second + 1000 && second > 0,
            "the transmit was not sent exactly once: {first} then {second} bytes of cells"
        );
    }

    #[test]
    fn deflate_sample() {
        // Noise, which no deflate shrinks, goes as it is; a screen of flat
        // colour with text-like stripes is deflated; a small image always is.
        let mut seed = 0x2545_f491_u32;
        let noise: Vec<u8> = (0..1000 * 400 * 4)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let screen = image::RgbaImage::from_fn(1000, 400, |x, y| {
            if y % 20 < 14 && x % 9 < 6 {
                image::Rgba([220, 220, 220, 255])
            } else {
                image::Rgba([30, 30, 46, 255])
            }
        });
        assert!(!worth_deflating(&noise), "noise was deflated");
        assert!(
            worth_deflating(screen.as_raw()),
            "a flat screen was sent raw"
        );
        assert!(
            worth_deflating(&noise[..SAMPLE]),
            "a small image was sent raw"
        );
    }

    #[test]
    fn kitty_chunks() {
        // Every chunk is its own escape of at most 4096 base64 bytes, all but
        // the last a multiple of four and marked `m=1`, and the payloads
        // decoded and inflated are the pixels.
        let pixels = image(300, 200);
        for compress in [false, true] {
            let chunks = transmit(
                pixels.as_raw(),
                pixels.dimensions(),
                Size::new(30, 10),
                9,
                false,
                compress,
            );
            assert!(
                chunks.len() > 1,
                "the image fits one chunk, so this proves nothing"
            );
            let mut payload = Vec::new();
            for (i, chunk) in chunks.iter().enumerate() {
                let body = chunk
                    .strip_prefix("\x1b_G")
                    .and_then(|rest| rest.strip_suffix("\x1b\\"))
                    .expect("one escape a chunk");
                let (keys, data) = body.split_once(';').expect("keys then data");
                let last = i + 1 == chunks.len();
                assert!(
                    data.len() <= CHUNK,
                    "chunk {i} carries {} bytes",
                    data.len()
                );
                assert!(
                    last || data.len() % 4 == 0,
                    "chunk {i} is not a multiple of four"
                );
                assert!(
                    keys.ends_with(if last { "m=0" } else { "m=1" }),
                    "chunk {i} of {} says {keys}",
                    chunks.len()
                );
                payload.extend(base64_simd::STANDARD.decode_to_vec(data).expect("base64"));
            }
            let raw = if compress {
                use std::io::Read as _;
                let mut out = Vec::new();
                flate2::read::ZlibDecoder::new(&payload[..])
                    .read_to_end(&mut out)
                    .expect("inflate");
                out
            } else {
                payload
            };
            assert_eq!(
                raw,
                pixels.as_raw().as_slice(),
                "the chunks do not carry the pixels"
            );
        }
    }
}
