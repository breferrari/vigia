//! The Kitty graphics protocol, as vigia sends it: the transmit, chunked, and
//! the Unicode placeholder cells that place it. `SPEC.md` §11.1.

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use std::num::NonZeroU16;

use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::{Rect, Size};
use ratatui::style::Color;
use ratatui_image::picker::cap_parser::Parser;

/// The character kitty reserves for an image's cells.
const PLACEHOLDER: char = '\u{10EEEE}';

/// The most base64 one chunk carries. Every chunk but the last is a multiple of
/// four, which 3072 bytes of data always encode to.
const CHUNK: usize = 4096;

/// A cell drawn by the terminal's image rather than by its own glyph.
const UNIT_WIDTH: CellDiffOption = CellDiffOption::ForcedWidth(NonZeroU16::MIN);

/// The transmit of `rgba`, `width` by `height` pixels, as image `id` placed
/// virtually over `cells`: one escape a chunk, and under tmux each in its own
/// passthrough, and where each chunk ends.
fn transmit(
    rgba: &[u8],
    (width, height): (u32, u32),
    cells: Size,
    id: u32,
    tmux: bool,
    compress: bool,
) -> (String, Vec<usize>) {
    let deflated;
    let bytes = if compress {
        deflated = zlib(rgba);
        &deflated[..]
    } else {
        rgba
    };
    let (start, escape, end) = Parser::tmux_start_escape_end(tmux);
    let pieces = bytes.chunks(CHUNK / 4 * 3);
    let count = pieces.len();
    let mut out = String::with_capacity(count * (CHUNK + 32) + 96);
    let mut ends = Vec::with_capacity(count);
    for (i, piece) in pieces.enumerate() {
        out.push_str(start);
        let _ = write!(out, "{escape}_Gq=2,");
        if i == 0 {
            let compression = if compress { "o=z," } else { "" };
            let _ = write!(
                out,
                "i={id},a=T,U=1,f=32,{compression}t=d,s={width},v={height},c={},r={},",
                cells.width, cells.height
            );
        }
        let _ = write!(out, "m={};", u8::from(i + 1 < count));
        base64_simd::STANDARD.encode_append(piece, &mut out);
        let _ = write!(out, "{escape}\\");
        out.push_str(end);
        ends.push(out.len());
    }
    (out, ends)
}

/// A string terminator, through tmux if `tmux`.
pub fn string_end(tmux: bool) -> String {
    let (start, escape, end) = Parser::tmux_start_escape_end(tmux);
    format!("{start}{escape}\\{end}")
}

/// The empty last chunk that closes an image left part sent.
pub fn cancel(tmux: bool) -> String {
    let (start, escape, end) = Parser::tmux_start_escape_end(tmux);
    format!("{start}{escape}_Gm=0,q=2;{escape}\\{end}")
}

/// The delete of image `id`. `q=2`: an answer would reach the event reader
/// as keys.
pub fn delete(id: u32, tmux: bool) -> String {
    let (start, escape, end) = Parser::tmux_start_escape_end(tmux);
    format!("{start}{escape}_Ga=d,d=I,i={id},q=2{escape}\\{end}")
}

/// The least of an image [`worth_deflating`] deflates to decide.
const SAMPLE: usize = 64 * 1024;

/// Whether deflating `rgba`, `width` pixels a row, pays: a sample from across
/// the image shrinks at least twice, which a photograph's does not.
fn worth_deflating(rgba: &[u8], width: u32) -> bool {
    // Four strips from top to bottom, since a screenshot's first rows are its
    // flattest, each three rows deep where 64 KB allows, since a row repeats
    // the one above it far more often than it repeats itself.
    const STRIPS: usize = 4;
    let strip = (3 * 4 * width as usize).clamp(SAMPLE / STRIPS, SAMPLE);
    if rgba.len() <= strip * STRIPS {
        return true;
    }
    let step = rgba.len() / STRIPS;
    let mut sample = Vec::with_capacity(strip * STRIPS);
    for i in 0..STRIPS {
        sample.extend_from_slice(&rgba[i * step..i * step + strip]);
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

/// Draw image `id`'s placeholder cells over all of `room`, its first `skip`
/// rows above it.
fn placeholders(room: Rect, id: u32, skip: u16, buf: &mut Buffer) {
    let [high, r, g, b] = id.to_be_bytes();
    let fg = Color::Rgb(r, g, b);
    let mut symbol = String::with_capacity(64);
    for y in 0..room.height {
        for x in 0..room.width {
            let Some(cell) = buf.cell_mut((room.left() + x, room.top() + y)) else {
                continue;
            };
            symbol.clear();
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
    transmit: Arc<str>,
    ends: Arc<[usize]>,
    tmux: bool,
    deflated: bool,
    /// [`IDLE`], [`QUEUED`] or [`WRITTEN`] in the low bits, and above them
    /// which upload it is, so a report from one cut off marks no later one.
    state: Arc<AtomicU64>,
}

/// Not with the writer.
const IDLE: u64 = 0;
/// Handed to the writer and not yet written whole.
const QUEUED: u64 = 1;
/// Written whole, so its cells may be drawn.
const WRITTEN: u64 = 2;
/// The bits of [`Image::state`] that hold the state.
const STATE: u64 = 3;

/// One upload's hold on its image's state: written, it marks the image held;
/// dropped unwritten, it puts the image back to be sent again. A later upload
/// of the image is a different generation, which neither touches.
pub struct Ticket {
    state: Arc<AtomicU64>,
    queued: u64,
}

impl Ticket {
    /// Its last chunk was written.
    pub fn written(&self) {
        let _ = self.state.compare_exchange(
            self.queued,
            self.queued - QUEUED + WRITTEN,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let _ = self.state.compare_exchange(
            self.queued,
            self.queued - QUEUED + IDLE,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

impl Image {
    /// `rgba`, `width` by `height` pixels, as image `id` over `cells`,
    /// deflated if the terminal `inflates` and deflating pays.
    pub fn new(
        rgba: &[u8],
        pixels: (u32, u32),
        cells: Size,
        id: u32,
        tmux: bool,
        inflates: bool,
    ) -> Self {
        let compress = inflates && worth_deflating(rgba, pixels.0);
        Self::encoded(rgba, pixels, cells, id, tmux, compress)
    }

    /// [`Image::new`], deflated exactly when `compress` says.
    fn encoded(
        rgba: &[u8],
        pixels: (u32, u32),
        cells: Size,
        id: u32,
        tmux: bool,
        compress: bool,
    ) -> Self {
        let (transmit, ends) = transmit(rgba, pixels, cells, id, tmux, compress);
        Self {
            id,
            cells,
            transmit: transmit.into(),
            ends: ends.into(),
            tmux,
            deflated: compress,
            state: Arc::new(AtomicU64::new(IDLE)),
        }
    }

    /// The id the terminal knows it by.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Its transmit, once: none if it is already with the writer or written.
    pub fn upload(&self) -> Option<crate::writer::Upload> {
        let now = self.state.load(Ordering::Acquire);
        if now & STATE != IDLE {
            return None;
        }
        let queued = (now & !STATE) + (STATE + 1) + QUEUED;
        self.state
            .compare_exchange(now, queued, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        Some(crate::writer::Upload {
            id: self.id,
            data: Arc::clone(&self.transmit),
            ends: Arc::clone(&self.ends),
            close: cancel(self.tmux).into(),
            tmux: self.tmux,
            ticket: Ticket {
                state: Arc::clone(&self.state),
                queued,
            },
        })
    }

    /// Whether the terminal has all of it, so its cells place it.
    pub fn uploaded(&self) -> bool {
        self.state.load(Ordering::Acquire) & STATE == WRITTEN
    }

    /// Its upload stops: send it again when next shown. Whether it was still
    /// in flight, so the terminal may hold part of it.
    pub fn unsent(&self) -> bool {
        self.state
            .try_update(Ordering::AcqRel, Ordering::Acquire, |now| {
                (now & STATE == QUEUED).then_some(now - QUEUED + IDLE)
            })
            .is_ok()
    }

    /// Whether its transmit is deflated.
    pub fn deflated(&self) -> bool {
        self.deflated
    }

    /// The cells it covers.
    pub fn size(&self) -> Size {
        self.cells
    }

    /// Draw its cells into `area` with its first `above` rows scrolled above
    /// the area's top. The transmit goes separately, before.
    pub fn render(&self, area: Rect, buf: &mut Buffer, above: u16) {
        // A position past the table cannot be encoded, so it is left undrawn
        // rather than drawn at the wrong offset.
        let most = DIACRITICS.len() as u16;
        let room = Rect {
            width: area.width.min(self.cells.width).min(most),
            height: area
                .height
                .min(self.cells.height.saturating_sub(above))
                .min(most.saturating_sub(above)),
            ..area
        };
        if room.is_empty() {
            return;
        }
        placeholders(room, self.id, above, buf);
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
#[rustfmt::skip]
static DIACRITICS: [char; 297] = [
    '\u{305}', '\u{30D}', '\u{30E}', '\u{310}', '\u{312}', '\u{33D}', '\u{33E}', '\u{33F}',
    '\u{346}', '\u{34A}', '\u{34B}', '\u{34C}', '\u{350}', '\u{351}', '\u{352}', '\u{357}',
    '\u{35B}', '\u{363}', '\u{364}', '\u{365}', '\u{366}', '\u{367}', '\u{368}', '\u{369}',
    '\u{36A}', '\u{36B}', '\u{36C}', '\u{36D}', '\u{36E}', '\u{36F}', '\u{483}', '\u{484}',
    '\u{485}', '\u{486}', '\u{487}', '\u{592}', '\u{593}', '\u{594}', '\u{595}', '\u{597}',
    '\u{598}', '\u{599}', '\u{59C}', '\u{59D}', '\u{59E}', '\u{59F}', '\u{5A0}', '\u{5A1}',
    '\u{5A8}', '\u{5A9}', '\u{5AB}', '\u{5AC}', '\u{5AF}', '\u{5C4}', '\u{610}', '\u{611}',
    '\u{612}', '\u{613}', '\u{614}', '\u{615}', '\u{616}', '\u{617}', '\u{657}', '\u{658}',
    '\u{659}', '\u{65A}', '\u{65B}', '\u{65D}', '\u{65E}', '\u{6D6}', '\u{6D7}', '\u{6D8}',
    '\u{6D9}', '\u{6DA}', '\u{6DB}', '\u{6DC}', '\u{6DF}', '\u{6E0}', '\u{6E1}', '\u{6E2}',
    '\u{6E4}', '\u{6E7}', '\u{6E8}', '\u{6EB}', '\u{6EC}', '\u{730}', '\u{732}', '\u{733}',
    '\u{735}', '\u{736}', '\u{73A}', '\u{73D}', '\u{73F}', '\u{740}', '\u{741}', '\u{743}',
    '\u{745}', '\u{747}', '\u{749}', '\u{74A}', '\u{7EB}', '\u{7EC}', '\u{7ED}', '\u{7EE}',
    '\u{7EF}', '\u{7F0}', '\u{7F1}', '\u{7F3}', '\u{816}', '\u{817}', '\u{818}', '\u{819}',
    '\u{81B}', '\u{81C}', '\u{81D}', '\u{81E}', '\u{81F}', '\u{820}', '\u{821}', '\u{822}',
    '\u{823}', '\u{825}', '\u{826}', '\u{827}', '\u{829}', '\u{82A}', '\u{82B}', '\u{82C}',
    '\u{82D}', '\u{951}', '\u{953}', '\u{954}', '\u{F82}', '\u{F83}', '\u{F86}', '\u{F87}',
    '\u{135D}', '\u{135E}', '\u{135F}', '\u{17DD}', '\u{193A}', '\u{1A17}', '\u{1A75}', '\u{1A76}',
    '\u{1A77}', '\u{1A78}', '\u{1A79}', '\u{1A7A}', '\u{1A7B}', '\u{1A7C}', '\u{1B6B}', '\u{1B6D}',
    '\u{1B6E}', '\u{1B6F}', '\u{1B70}', '\u{1B71}', '\u{1B72}', '\u{1B73}', '\u{1CD0}', '\u{1CD1}',
    '\u{1CD2}', '\u{1CDA}', '\u{1CDB}', '\u{1CE0}', '\u{1DC0}', '\u{1DC1}', '\u{1DC3}', '\u{1DC4}',
    '\u{1DC5}', '\u{1DC6}', '\u{1DC7}', '\u{1DC8}', '\u{1DC9}', '\u{1DCB}', '\u{1DCC}', '\u{1DD1}',
    '\u{1DD2}', '\u{1DD3}', '\u{1DD4}', '\u{1DD5}', '\u{1DD6}', '\u{1DD7}', '\u{1DD8}', '\u{1DD9}',
    '\u{1DDA}', '\u{1DDB}', '\u{1DDC}', '\u{1DDD}', '\u{1DDE}', '\u{1DDF}', '\u{1DE0}', '\u{1DE1}',
    '\u{1DE2}', '\u{1DE3}', '\u{1DE4}', '\u{1DE5}', '\u{1DE6}', '\u{1DFE}', '\u{20D0}', '\u{20D1}',
    '\u{20D4}', '\u{20D5}', '\u{20D6}', '\u{20D7}', '\u{20DB}', '\u{20DC}', '\u{20E1}', '\u{20E7}',
    '\u{20E9}', '\u{20F0}', '\u{2CEF}', '\u{2CF0}', '\u{2CF1}', '\u{2DE0}', '\u{2DE1}', '\u{2DE2}',
    '\u{2DE3}', '\u{2DE4}', '\u{2DE5}', '\u{2DE6}', '\u{2DE7}', '\u{2DE8}', '\u{2DE9}', '\u{2DEA}',
    '\u{2DEB}', '\u{2DEC}', '\u{2DED}', '\u{2DEE}', '\u{2DEF}', '\u{2DF0}', '\u{2DF1}', '\u{2DF2}',
    '\u{2DF3}', '\u{2DF4}', '\u{2DF5}', '\u{2DF6}', '\u{2DF7}', '\u{2DF8}', '\u{2DF9}', '\u{2DFA}',
    '\u{2DFB}', '\u{2DFC}', '\u{2DFD}', '\u{2DFE}', '\u{2DFF}', '\u{A66F}', '\u{A67C}', '\u{A67D}',
    '\u{A6F0}', '\u{A6F1}', '\u{A8E0}', '\u{A8E1}', '\u{A8E2}', '\u{A8E3}', '\u{A8E4}', '\u{A8E5}',
    '\u{A8E6}', '\u{A8E7}', '\u{A8E8}', '\u{A8E9}', '\u{A8EA}', '\u{A8EB}', '\u{A8EC}', '\u{A8ED}',
    '\u{A8EE}', '\u{A8EF}', '\u{A8F0}', '\u{A8F1}', '\u{AAB0}', '\u{AAB2}', '\u{AAB3}', '\u{AAB7}',
    '\u{AAB8}', '\u{AABE}', '\u{AABF}', '\u{AAC1}', '\u{FE20}', '\u{FE21}', '\u{FE22}', '\u{FE23}',
    '\u{FE24}', '\u{FE25}', '\u{FE26}', '\u{10A0F}', '\u{10A38}', '\u{1D185}', '\u{1D186}', '\u{1D187}',
    '\u{1D188}', '\u{1D189}', '\u{1D1AA}', '\u{1D1AB}', '\u{1D1AC}', '\u{1D1AD}', '\u{1D242}', '\u{1D243}',
    '\u{1D244}',
];

/// For a writer test: a ticket of an image just queued, and whether that
/// image is now held.
#[cfg(test)]
pub(crate) fn ticket() -> (Ticket, impl Fn() -> bool) {
    let state = Arc::new(AtomicU64::new((STATE + 1) + QUEUED));
    let read = Arc::clone(&state);
    let ticket = Ticket {
        state,
        queued: (STATE + 1) + QUEUED,
    };
    (ticket, move || {
        read.load(Ordering::Acquire) & STATE == WRITTEN
    })
}

/// Bytes no deflate shrinks, as a photograph's nearly are.
#[cfg(test)]
pub(crate) fn noise(len: usize) -> Vec<u8> {
    let mut seed = 0x2545_f491_u32;
    (0..len)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as u8
        })
        .collect()
}

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
        let mut buf = Buffer::empty(frame(area));
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
        let image = Image::encoded(
            pixels.as_raw(),
            pixels.dimensions(),
            cells,
            id,
            tmux,
            compress,
        );
        let mut buf = Buffer::empty(frame(area));
        image.render(area, &mut buf, skip);
        // Where the crate puts the transmit, so the two compare whole.
        if let Some(cell) = buf.cell_mut((area.x, area.y))
            && cell.symbol().starts_with(PLACEHOLDER)
        {
            let joined = format!("{}{}", image.transmit, cell.symbol());
            cell.set_symbol(&joined);
        }
        buf
    }

    /// A screen with room around `area`, so a cell drawn outside it shows.
    fn frame(area: Rect) -> Rect {
        Rect::new(0, 0, area.right() + 3, area.bottom() + 3)
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
            // Its own size at the origin, clipped narrower at an offset, and
            // inside a larger area, as a panel is drawn into the pane.
            let areas = [
                Rect::new(0, 0, cells.width, cells.height),
                Rect::new(5, 3, cells.width - 1, cells.height),
                Rect::new(2, 1, cells.width + 4, cells.height + 2),
                Rect::new(5, 3, cells.width - 1, cells.height - 1),
            ];
            for ((id, tmux, compress, skip), area) in cases
                .into_iter()
                .flat_map(|case| areas.map(|area| (case, area)))
            {
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
                     {skip} rows above, into {area:?}: the cells differ from ratatui-image's"
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
    fn upload_once() {
        // Handed to the writer once; again only after a cancel; never once
        // written whole. Its cells carry no transmit.
        let pixels = image(40, 40);
        let image = Image::new(
            pixels.as_raw(),
            pixels.dimensions(),
            Size::new(4, 2),
            3,
            false,
            false,
        );
        let upload = image.upload().expect("the first upload");
        assert_eq!(upload.data.len(), *upload.ends.last().expect("a chunk"));
        assert!(
            image.upload().is_none(),
            "the same image was handed over twice"
        );
        image.unsent();
        let again = image
            .upload()
            .expect("a cancelled image was not sent again");
        again.ticket.written();
        image.unsent();
        assert!(
            image.upload().is_none(),
            "an image written whole was sent again"
        );
        let area = Rect::new(0, 0, 4, 2);
        let mut buf = Buffer::empty(area);
        image.render(area, &mut buf, 0);
        assert!(
            buf.content()
                .iter()
                .all(|cell| !cell.symbol().contains("_G")),
            "a cell carried a graphics escape"
        );
    }

    #[test]
    fn late_report() {
        // The report of an upload stopped since marks nothing: not the image
        // left idle, and not the upload sent after it.
        let pixels = image(40, 40);
        let image = Image::new(
            pixels.as_raw(),
            pixels.dimensions(),
            Size::new(4, 2),
            3,
            false,
            false,
        );
        let first = image.upload().expect("an upload");
        image.unsent();
        first.ticket.written();
        assert!(!image.uploaded(), "a stopped upload was taken as held");
        let second = image.upload().expect("sent again");
        let first_again = image.upload();
        assert!(first_again.is_none(), "handed out twice");
        assert!(!image.uploaded(), "held before its own report");
        second.ticket.written();
        assert!(
            image.uploaded(),
            "the upload written whole was not taken as held"
        );
    }

    #[test]
    fn ticket_kept() {
        // An upload dropped unwritten puts its image back to be sent; one
        // written whole is not taken back as stopped.
        let pixels = image(40, 40);
        let image = Image::new(
            pixels.as_raw(),
            pixels.dimensions(),
            Size::new(4, 2),
            3,
            false,
            false,
        );
        drop(image.upload().expect("an upload"));
        let upload = image
            .upload()
            .expect("a dropped upload's image was not sent again");
        upload.ticket.written();
        assert!(
            !image.unsent(),
            "an image written whole was taken as stopped"
        );
        assert!(image.uploaded(), "a stop undid an image written whole");
        drop(upload);
        assert!(image.uploaded(), "dropping a written upload undid it");
    }

    #[test]
    fn chunk_edges() {
        // 3072 bytes of pixels are exactly one full chunk, and 6144 exactly two.
        for (height, count) in [(24, 1), (48, 2)] {
            let pixels = image(32, height);
            let (sent, ends) = transmit(
                pixels.as_raw(),
                pixels.dimensions(),
                Size::new(4, 2),
                5,
                false,
                false,
            );
            let chunks: Vec<&str> = sent.split_inclusive("\x1b\\").collect();
            assert_eq!(ends.len(), chunks.len(), "a chunk end was not recorded");
            assert_eq!(
                chunks.len(),
                count,
                "{} bytes split into {} chunks",
                pixels.len(),
                chunks.len()
            );
            let last = chunks.last().expect("a chunk");
            let data = last.split_once(';').expect("keys then data").1;
            assert_eq!(data.len(), CHUNK + 2, "the last chunk is not a full one");
            assert!(last.contains("m=0"), "the last chunk is not marked last");
        }
    }

    #[test]
    fn far_rows_scrolled() {
        // Scrolled 10 rows, the rows that would be 297 and past are left undrawn
        // rather than drawn as row 0.
        let cells = Size::new(1, 300);
        let pixels = image(10, 3000);
        let image = Image::new(pixels.as_raw(), pixels.dimensions(), cells, 7, false, false);
        let area = Rect::new(0, 0, 1, 290);
        let mut buf = Buffer::empty(area);
        image.render(area, &mut buf, 10);
        let drawn = |y: u16| buf[(0, y)].symbol().contains(PLACEHOLDER);
        assert!(drawn(286), "row 296 was not drawn");
        assert!(
            !drawn(287),
            "row 297, past the table, was drawn as another row"
        );
    }

    #[test]
    fn deflate_sample() {
        // Noise, which no deflate shrinks, goes as it is; a screen of flat
        // colour with text-like stripes is deflated; a small image always is.
        let noise = noise(1000 * 400 * 4);
        let screen = image::RgbaImage::from_fn(1000, 400, |x, y| {
            if y % 20 < 14 && x % 9 < 6 {
                image::Rgba([220, 220, 220, 255])
            } else {
                image::Rgba([30, 30, 46, 255])
            }
        });
        assert!(!worth_deflating(&noise, 1000), "noise was deflated");
        // Four bands of ten rows, each band one row of noise repeated: a row
        // alone does not shrink, and only rows within one band repeat, so the
        // sample sees it only if a strip holds more than one row.
        let rows: Vec<u8> = noise
            .chunks(5000 * 4)
            .take(4)
            .flat_map(|row| row.repeat(10))
            .collect();
        assert!(
            worth_deflating(&rows, 5000),
            "a texture repeated row by row was sent raw"
        );
        assert!(
            worth_deflating(screen.as_raw(), 1000),
            "a flat screen was sent raw"
        );
        assert!(
            worth_deflating(&noise[..SAMPLE], 64),
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
            let (sent, ends) = transmit(
                pixels.as_raw(),
                pixels.dimensions(),
                Size::new(30, 10),
                9,
                false,
                compress,
            );
            let chunks: Vec<&str> = sent.split_inclusive("\x1b\\").collect();
            let mut at = 0;
            for (chunk, end) in chunks.iter().zip(&ends) {
                assert_eq!(
                    &sent[at..*end],
                    *chunk,
                    "a recorded end is not where its chunk ends"
                );
                at = *end;
            }
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
