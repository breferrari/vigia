//! The pictures of a changed image: where they go on the pane, and how they are
//! built off the paint and kept while they are on screen. `SPEC.md` §11.1.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use image::{DynamicImage, ImageFormat, RgbImage, RgbaImage};
use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
use ratatui::widgets::Widget;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::halfblocks::Halfblocks;
use ratatui_image::sliced::SlicedProtocol;
use vigia_core::{Format, ImageSide, Picture, Source, Sources};

use crate::view::Row;

#[cfg(feature = "pdf")]
mod pdf;

/// Without PDF support the core makes no PDF picture, so neither is reached.
#[cfg(not(feature = "pdf"))]
mod pdf {
    use super::{Built, Job, PdfDrawn, undecoded};

    pub(super) fn sized(_: &Job) -> Built {
        undecoded()
    }

    pub(super) fn drawn(_: &Job) -> Option<PdfDrawn> {
        None
    }
}

/// Columns between two panels.
pub const GAP: u16 = 2;
/// The narrowest a panel may be and still be drawn.
pub const MIN_WIDTH: u16 = 8;
/// The shortest a panel may be and still be drawn.
pub const MIN_ROWS: u16 = 6;

/// Which pictures a changed image draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Old, new, and the mask.
    Three,
    /// The mask alone, where three do not fit.
    Mask,
    /// Old and new, for an image whose size changed.
    Pair,
    /// The new picture alone, where a pair does not fit.
    New,
    /// The new picture of an added image, after the added bar.
    Added,
    /// The old picture of a deleted image, dimmed, after the removed bar.
    Deleted,
}

/// Which bar stands in a picture's first column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bar {
    /// An added image's.
    Added,
    /// A deleted image's.
    Removed,
}

/// Which side of the change a panel draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Side {
    Old,
    New,
}

impl Kind {
    /// The bar in the first column, if this kind has one.
    pub fn bar(self) -> Option<Bar> {
        match self {
            Self::Added => Some(Bar::Added),
            Self::Deleted => Some(Bar::Removed),
            _ => None,
        }
    }

    /// Each panel's side and paint, left to right.
    fn paints(self) -> &'static [(Side, Paint)] {
        match self {
            Self::Three => &[
                (Side::Old, Paint::Plain),
                (Side::New, Paint::Plain),
                (Side::New, Paint::Mask),
            ],
            Self::Mask => &[(Side::New, Paint::Mask)],
            Self::Pair => &[(Side::Old, Paint::Plain), (Side::New, Paint::Plain)],
            Self::New | Self::Added => &[(Side::New, Paint::Plain)],
            Self::Deleted => &[(Side::Old, Paint::Dim)],
        }
    }
}

/// One panel's place: columns from the picture's left edge, and its size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Slot {
    /// Columns from the left edge of the picture.
    pub x: u16,
    /// Columns wide.
    pub width: u16,
    /// Rows tall.
    pub rows: u16,
}

/// Where a changed image's pictures go.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shape {
    /// Which pictures.
    pub kind: Kind,
    /// Rows the tallest panel takes, which is what the view reserves.
    pub rows: u16,
    /// The panels, left to right.
    pub slots: Vec<Slot>,
}

/// An image's size in cells when fitted into `width` columns by `rows` rows. A
/// half-block is two pixels in one cell, which is about square on screen.
fn fit(side: &ImageSide, width: u16, rows: u16) -> (u16, u16) {
    if side.width == 0 || side.height == 0 {
        return (0, 0);
    }
    let scale = (f64::from(width) / f64::from(side.width))
        .min(f64::from(rows) * 2.0 / f64::from(side.height));
    let cells = (f64::from(side.width) * scale).floor() as u16;
    let pixels = (f64::from(side.height) * scale).floor() as u16;
    (cells.max(1), pixels.div_ceil(2).max(1))
}

/// Whether a fitted panel is big enough to draw.
fn fits((width, rows): (u16, u16)) -> bool {
    width >= MIN_WIDTH && rows >= MIN_ROWS
}

/// Panels of these fitted sizes laid out from the left, [`GAP`] apart.
fn row_of(kind: Kind, start: u16, sizes: &[(u16, u16)]) -> Shape {
    let mut x = start;
    let slots: Vec<Slot> = sizes
        .iter()
        .map(|&(width, rows)| {
            let slot = Slot { x, width, rows };
            x += width + GAP;
            slot
        })
        .collect();
    Shape {
        kind,
        rows: slots.iter().map(|slot| slot.rows).max().unwrap_or(0),
        slots,
    }
}

/// Where a changed image's pictures go in `width` columns and `rows` rows, or
/// `None` when nothing fits or the terminal cannot draw a half-block's colours.
pub fn shape(picture: &Picture, width: u16, rows: u16, truecolour: bool) -> Option<Shape> {
    if !truecolour {
        return None;
    }
    // A side the worker has yet to size reserves nothing, and still asks it.
    if [picture.before, picture.after]
        .iter()
        .flatten()
        .any(|side| !side.sized())
    {
        return Some(Shape {
            kind: Kind::New,
            rows: 0,
            slots: Vec::new(),
        });
    }
    let third = width.saturating_sub(2 * GAP) / 3;
    let one = |kind: Kind, side: &ImageSide| {
        // The bar takes the slot's first column.
        let size = fit(side, third.saturating_sub(1), rows);
        fits(size).then(|| row_of(kind, 1, &[size]))
    };
    match (&picture.before, &picture.after) {
        (Some(old), Some(new)) if (old.width, old.height) == (new.width, new.height) => {
            let size = fit(new, third, rows);
            if fits(size) {
                return Some(row_of(Kind::Three, 0, &[size, size, size]));
            }
            let size = fit(new, width, rows);
            fits(size).then(|| row_of(Kind::Mask, 0, &[size]))
        }
        (Some(old), Some(new)) => {
            let half = width.saturating_sub(GAP) / 2;
            let (left, right) = (fit(old, half, rows), fit(new, half, rows));
            if fits(left) && fits(right) {
                return Some(row_of(Kind::Pair, 0, &[left, right]));
            }
            let size = fit(new, width, rows);
            fits(size).then(|| row_of(Kind::New, 0, &[size]))
        }
        (None, Some(new)) => one(Kind::Added, new),
        (Some(old), None) => one(Kind::Deleted, old),
        (None, None) => None,
    }
}

/// What names one built picture: the file, its sides' contents, and its shape.
/// A change to any of them is a different picture.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    /// Repository-relative path.
    pub path: String,
    /// The old side's stamp.
    pub before: Option<u64>,
    /// The new side's stamp.
    pub after: Option<u64>,
    /// Where the panels go.
    pub shape: Shape,
}

impl Key {
    /// The key of `picture` at `path` drawn in `shape`.
    pub fn of(path: &str, picture: &Picture, shape: Shape) -> Self {
        Self {
            path: path.to_owned(),
            before: picture.before.map(|side| side.stamp),
            after: picture.after.map(|side| side.stamp),
            shape,
        }
    }
}

/// What the comparison of the two sides found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// One side only, or two of different sizes: nothing to count.
    Uncompared,
    /// This many pixels of this many differ.
    Changed {
        /// Pixels that differ.
        pixels: u64,
        /// Pixels compared.
        total: u64,
    },
    /// The two decode to the same pixels.
    Unchanged,
    /// A side did not decode, so there is nothing to draw.
    Undecoded,
}

/// One panel, encoded for the terminal.
pub enum Panel {
    /// A Kitty image, which this pane encodes itself.
    Kitty(crate::kitty::Image),
    /// Anything else, which `ratatui-image` encodes.
    Sliced(SlicedProtocol),
}

impl Panel {
    /// Whether the terminal paints it over the cells, so an overlay drawn on
    /// top would not hide it.
    pub fn drawn_by_terminal(&self) -> bool {
        !matches!(self, Self::Sliced(SlicedProtocol::Halfblocks(_)))
    }

    /// What a Kitty image of `pixels` holds: its unpacked pixels in the
    /// terminal, or, sent uncompressed, the larger base64 kept for it, four
    /// bytes to three.
    pub fn held(&self, pixels: u64) -> u64 {
        match self {
            Self::Kitty(image) if image.deflated() => pixels * 4,
            Self::Kitty(_) => pixels * 16 / 3,
            Self::Sliced(_) => 0,
        }
    }

    /// Draw it into `area`, its first `above` rows scrolled off the top.
    pub fn render(&self, area: Rect, buf: &mut Buffer, above: u16) {
        match self {
            Self::Kitty(image) => image.render(area, buf, above),
            Self::Sliced(protocol) => {
                let above = i16::try_from(above).unwrap_or(i16::MAX);
                ratatui_image::sliced::SlicedImage::new(protocol, (0, -above).into())
                    .render(area, buf);
            }
        }
    }
}

/// A picture built and encoded, ready to draw.
pub struct Built {
    /// Each panel's place and its encoded cells.
    pub panels: Vec<(Slot, Panel)>,
    /// What the comparison found.
    pub verdict: Verdict,
    /// Whether the two sides carry different colour profiles.
    pub profiles_differ: bool,
    /// The ids of its Kitty images, which the terminal holds until told to drop them.
    pub kitty: Vec<u32>,
    /// Bytes those images hold: their unpacked pixels in the terminal, or, sent
    /// uncompressed, the larger base64 transmit this process keeps for each.
    pub stored: u64,
    /// Which page of a PDF was drawn, and whether the page count changed.
    pub page_note: Option<String>,
    /// The sizes found for sides the frame path left unsized, by their stamp.
    pub sizes: Vec<(u64, u32, u32)>,
    /// A bar the picture draws in place of its shape's: a PDF page one side
    /// added or removed.
    pub bar: Option<Bar>,
}

impl Built {
    /// Whether every Kitty image of it is in the terminal, so its cells place
    /// it.
    pub fn placed(&self) -> bool {
        self.panels.iter().all(|(_, panel)| match panel {
            Panel::Kitty(image) => image.uploaded(),
            Panel::Sliced(_) => true,
        })
    }

    /// What the size line adds once the comparison is done.
    pub fn suffix(&self) -> Option<String> {
        let found = self.found();
        match (&self.page_note, found) {
            (Some(page), Some(found)) => Some(format!("{page} · {found}")),
            (page, found) => page.clone().or(found),
        }
    }

    /// What the comparison found, without the page it was found on.
    fn found(&self) -> Option<String> {
        if self.profiles_differ {
            return Some("colour profile changed".to_owned());
        }
        match self.verdict {
            Verdict::Uncompared => None,
            Verdict::Unchanged => Some("pixels unchanged".to_owned()),
            Verdict::Undecoded => Some("could not be decoded".to_owned()),
            Verdict::Changed { pixels, total } => {
                let share = pixels as f64 * 100.0 / total.max(1) as f64;
                Some(format!("{pixels} px changed ({share:.2}%)"))
            }
        }
    }
}

/// A built picture as a row of the view holds it. Compared by identity, since
/// two builds of one key are the same picture.
#[derive(Clone)]
pub struct Ready(pub Arc<Built>);

impl PartialEq for Ready {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Ready {}

impl std::fmt::Debug for Ready {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ready")
    }
}

/// The colours a picture is built in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inks {
    /// What a transparent pixel shows, and what the mask dims toward: the pane.
    pub canvas: (u8, u8, u8),
    /// A changed pixel in the mask.
    pub changed: (u8, u8, u8),
}

impl Inks {
    /// The pane's colour as the terminal said it is, or as dark or light as it
    /// said, and the theme's picture mask colour.
    pub fn of(theme: &crate::Theme, pane: Option<(u8, u8, u8)>, light: bool) -> Self {
        // Unanswered, the palette says which: a light one washes rows light.
        let washed_light = matches!(
            theme.removed_row.bg,
            Some(ratatui::style::Color::Rgb(r, g, b)) if crate::Background::of((r, g, b)) == crate::Background::Light
        );
        let canvas = pane.unwrap_or(if light || washed_light {
            (255, 255, 255)
        } else {
            (0, 0, 0)
        });
        // A pixel needs a real colour, so a named one falls through.
        let rgb = |colour: Option<ratatui::style::Color>| match colour {
            Some(ratatui::style::Color::Rgb(r, g, b)) => Some((r, g, b)),
            _ => None,
        };
        let changed = rgb(theme.picture_changed.bg)
            .or_else(|| rgb(theme.picture_changed.fg))
            .or_else(|| rgb(theme.removed_bar.bg))
            .unwrap_or((0xf8, 0x51, 0x49));
        Self { canvas, changed }
    }
}

/// Environment variable that overrides the protocol outright.
pub const PICTURES_VAR: &str = "VIGIA_PICTURES";

/// A [`PICTURES_VAR`] this does not understand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PicturesError {
    /// What was found in the variable.
    pub value: String,
}

impl std::fmt::Display for PicturesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{PICTURES_VAR}: {:?} is not one of auto, kitty, sixel, iterm2, halfblocks",
            self.value
        )
    }
}

impl std::error::Error for PicturesError {}

/// What the terminal said it can draw, and whether its cell size in pixels is
/// known from the answer or from the terminal's own size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Answer {
    /// It answered the Kitty graphics query.
    pub kitty: bool,
    /// Its device attributes include Sixel.
    pub sixel: bool,
    /// Its cell size in pixels is known.
    pub cell: bool,
    /// It answered with its cell size, so a protocol it did not claim is absent.
    pub heard: bool,
}

/// Which protocol draws the pictures: the override, then the answer, then the
/// table of terminals, then half-blocks.
pub fn protocol_of(
    forced: Option<ProtocolType>,
    answer: Answer,
    lookup: &impl Fn(&str) -> Option<String>,
) -> ProtocolType {
    forced.unwrap_or_else(|| detected(answer, lookup))
}

/// The protocol [`PICTURES_VAR`] names, or `None` for `auto` or unset.
pub fn forced(
    lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<ProtocolType>, PicturesError> {
    let Some((raw, value)) = crate::colour::override_of(lookup, PICTURES_VAR) else {
        return Ok(None);
    };
    match value.as_str() {
        "kitty" => Ok(Some(ProtocolType::Kitty)),
        "sixel" => Ok(Some(ProtocolType::Sixel)),
        "iterm2" => Ok(Some(ProtocolType::Iterm2)),
        "halfblocks" => Ok(Some(ProtocolType::Halfblocks)),
        "auto" => Ok(None),
        _ => Err(PicturesError { value: raw }),
    }
}

/// The protocol the answer and the table give, without the override.
pub fn detected(answer: Answer, lookup: &impl Fn(&str) -> Option<String>) -> ProtocolType {
    if answer.kitty {
        return ProtocolType::Kitty;
    }
    if answer.sixel {
        return ProtocolType::Sixel;
    }
    // A protocol drawn at a guessed cell size spills past its slot.
    if !answer.cell {
        return ProtocolType::Halfblocks;
    }
    match protocol_by_name(lookup) {
        // Kitty and Sixel answer for themselves; only iTerm2 images cannot.
        Some(ProtocolType::Kitty | ProtocolType::Sixel) if answer.heard => ProtocolType::Halfblocks,
        Some(protocol) => protocol,
        None => ProtocolType::Halfblocks,
    }
}

/// Whether pictures are drawn at all: a half-block is a background, which is
/// dropped below truecolour, and a protocol's pixels are not.
pub fn drawn(truecolour: bool, protocol: ProtocolType) -> bool {
    truecolour || protocol != ProtocolType::Halfblocks
}

/// Whether the terminal can be sent `o=z`: Ghostty up to 1.3 inflates with
/// Zig 0.15's flate, which crashes on valid streams. A Ghostty that gives no
/// version is not trusted with it.
fn inflates_safely(lookup: &impl Fn(&str) -> Option<String>) -> bool {
    let program = lookup("TERM_PROGRAM")
        .map(|p| p.trim().to_ascii_lowercase())
        .unwrap_or_default();
    program != "ghostty"
        || lookup("TERM_PROGRAM_VERSION")
            .and_then(|raw| crate::glyphs::version_of(&raw))
            .is_some_and(|version| version > (1, 3))
}

/// The protocol a terminal is known to draw, from its environment.
fn protocol_by_name(lookup: &impl Fn(&str) -> Option<String>) -> Option<ProtocolType> {
    use crate::colour::names;
    let set = |key: &str| lookup(key).is_some_and(|value| !value.trim().is_empty());
    let term = lookup("TERM").unwrap_or_default().to_ascii_lowercase();
    let program = lookup("TERM_PROGRAM")
        .map(|p| p.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if term.starts_with("tmux") || program == "tmux" {
        // Only the hints that outlive the multiplexer: a kitty inside tmux is
        // found by its answer, and its own variables go stale.
        return (set("ITERM_SESSION_ID") || set("WEZTERM_EXECUTABLE"))
            .then_some(ProtocolType::Iterm2);
    }
    if term.starts_with("screen") {
        return None;
    }
    if program == "wezterm"
        || program == "iterm.app"
        || set("WEZTERM_EXECUTABLE")
        || lookup("LC_TERMINAL").is_some_and(|t| t.contains("iTerm"))
    {
        return Some(ProtocolType::Iterm2);
    }
    if program == "ghostty"
        || names(&term, "xterm-ghostty")
        || names(&term, "xterm-kitty")
        || set("KITTY_WINDOW_ID")
    {
        return Some(ProtocolType::Kitty);
    }
    // Inherited by whatever Windows Terminal starts, so only a bare one counts.
    if names(&term, "foot") || (program.is_empty() && set("WT_SESSION")) {
        return Some(ProtocolType::Sixel);
    }
    None
}

/// One picture to build.
struct Job {
    key: Arc<Key>,
    sources: Sources,
    formats: (Option<Format>, Option<Format>),
    /// The cell size in pixels to build at, or `None` for the queried one,
    /// as a job sent before the picker was given carries.
    cell: Option<Cell>,
}

/// A cell's width and height in pixels.
pub type Cell = (u16, u16);

/// A window's size in cells and in pixels, as `crossterm::terminal::window_size`
/// reports it.
pub type Window = ratatui::crossterm::terminal::WindowSize;

/// The cell in `now`, from the cell `queried` in `then`, kept at `current`
/// unless it moved further than the windows' leftover pixels can explain. `None` when either reports no
/// pixels.
pub fn scaled_cell(queried: Cell, current: Cell, then: &Window, now: &Window) -> Option<Cell> {
    let axis = |cell: u16, at: u16, px_then: u16, cells_then: u16, px_now: u16, cells_now: u16| {
        if px_then == 0 || px_now == 0 || cells_then == 0 || cells_now == 0 {
            return None;
        }
        let (cell, at) = (f64::from(cell), f64::from(at));
        let scaled = cell * f64::from(px_now) * f64::from(cells_then)
            / (f64::from(px_then) * f64::from(cells_now));
        // Leftover in the window now can only raise `scaled`, and in the
        // query's window only lower it, by up to a cell each.
        let moved = scaled - at;
        if -at / f64::from(cells_then) <= moved && moved <= at / f64::from(cells_now) {
            return Some(at as u16);
        }
        Some(scaled.round().clamp(1.0, f64::from(u16::MAX)) as u16)
    };
    Some((
        axis(
            queried.0,
            current.0,
            then.width,
            then.columns,
            now.width,
            now.columns,
        )?,
        axis(
            queried.1,
            current.1,
            then.height,
            then.rows,
            now.height,
            now.rows,
        )?,
    ))
}

/// `base` at `cell`.
pub fn picker_at(base: &Picker, cell: Cell) -> Picker {
    // Deprecated in `ratatui-image` `=12.0.0-rc.3`, with no other way to a
    // size. It drops the queried capabilities and background: the worker reads
    // compression from the queried picker, shared memory stays off, Sixel and
    // iTerm2 read only the font size and tmux, and no background is set.
    #[allow(deprecated)]
    let mut picker = Picker::from_fontsize(cell.into());
    picker.set_protocol_type(base.protocol_type());
    picker
}

/// Built pictures, kept while their image is on screen, and the worker that
/// builds them.
pub struct Pictures {
    built: HashMap<Arc<Key>, Ready>,
    pending: HashSet<Arc<Key>>,
    /// What is on screen, shared with the worker so it skips a job scrolled away
    /// before its turn.
    shown: Arc<Mutex<HashSet<Arc<Key>>>>,
    jobs: mpsc::Sender<Job>,
    /// Each finished job, with `None` for one skipped.
    done: mpsc::Receiver<(Arc<Key>, Option<Cell>, Option<Built>)>,
    /// Kitty images whose picture is gone, waiting to be deleted.
    retired: Vec<u32>,
    /// When each kept picture was last on screen, in [`Pictures::keep`] calls.
    seen: HashMap<Arc<Key>, u64>,
    /// How many times [`Pictures::keep`] has run.
    tick: u64,
    /// The frame's walk that kept pictures were last checked against.
    walked: u64,
    /// Whether a delete has to pass through tmux.
    tmux: bool,
    /// Where the picker goes, until it has been given.
    picker: Option<mpsc::Sender<Picker>>,
    /// The cell size pictures are built at now.
    cell: Option<Cell>,
    /// The cell the query found; `None` for half-blocks, whose grid is no cell's.
    queried: Option<Cell>,
    /// The window the cell was queried in, once told.
    queried_in: Option<Window>,
    /// How many images this cache's worker has decoded.
    decoded: Arc<AtomicU64>,
    /// Sizes the worker found since [`Pictures::found_sizes`] last took them.
    found: Vec<(u64, u32, u32)>,
    /// Pairs sent to be scored as renames, each once while the frame's scores
    /// last.
    asked: HashSet<vigia_core::PairKey>,
    /// The frame's [`vigia_core::Frame::pair_epoch`] when `asked` was begun.
    epoch: u64,
    /// Where they go: a thread beside the picture worker, which needs no picker
    /// and skips nothing scrolled away.
    pairs: mpsc::Sender<vigia_core::Candidate>,
    /// Each pair's score, as it lands.
    scores: mpsc::Receiver<(vigia_core::PairKey, u64)>,
    /// Pairs sent whose score has not landed.
    waiting: usize,
}

/// A Kitty image id no other picture, nor another pane on the same terminal,
/// is likely to hold. Never 0, which the protocol does not accept.
fn kitty_id() -> u32 {
    use std::hash::{BuildHasher, Hasher};
    static NEXT: std::sync::OnceLock<std::sync::atomic::AtomicU32> = std::sync::OnceLock::new();
    let next = NEXT.get_or_init(|| {
        let seed = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        std::sync::atomic::AtomicU32::new(seed as u32)
    });
    loop {
        let id = next.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

std::thread_local! {
    /// Decodes on this thread since its worker last handed them to its cache.
    static DECODED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
/// Pictures decoded anywhere but a worker, which is a paint paying for one.
static OFF_WORKER: AtomicU64 = AtomicU64::new(0);
/// The worker thread's name, which is how a decode knows where it runs.
const WORKER: &str = "vigia-pictures";
/// The thread that scores renames.
const PAIRS: &str = "vigia-pairs";

pub use vigia_core::{Stage, panic_next};

/// How many images have been decoded on a thread that is not the worker.
pub fn decodes_off_worker() -> u64 {
    OFF_WORKER.load(Ordering::Relaxed)
}

/// What stands in for a picture that will not be drawn.
fn undecoded() -> Built {
    Built {
        panels: Vec::new(),
        verdict: Verdict::Undecoded,
        profiles_differ: false,
        kitty: Vec::new(),
        stored: 0,
        page_note: None,
        sizes: Vec::new(),
        bar: None,
    }
}

impl Pictures {
    /// A cache whose worker builds in `inks`, encodes for `picker`, and calls
    /// `wake` after each picture.
    pub fn new(inks: Inks, picker: Picker, wake: Box<dyn Fn() + Send>) -> Self {
        let mut pictures = Self::unpicked(inks, wake);
        pictures.pick(picker);
        pictures
    }

    /// A cache whose worker holds every job until [`Pictures::pick`] says how
    /// to encode, so nothing is built twice.
    pub fn unpicked(inks: Inks, wake: Box<dyn Fn() + Send>) -> Self {
        let wake: Arc<Mutex<Box<dyn Fn() + Send>>> = Arc::new(Mutex::new(wake));
        let (pairs, asked) = mpsc::channel::<vigia_core::Candidate>();
        let (scored, scores) = mpsc::channel();
        let woken = Arc::clone(&wake);
        let _ = std::thread::Builder::new()
            .name(PAIRS.to_owned())
            .spawn(move || {
                while let Ok(first) = asked.recv() {
                    // Everything queued, then one wake: each wake walks again.
                    for candidate in std::iter::once(first).chain(asked.try_iter()) {
                        let score = vigia_core::caught(|| vigia_core::candidate_score(&candidate))
                            .unwrap_or(0);
                        vigia_core::armed(Stage::Pairs);
                        if scored.send((candidate.key, score)).is_err() {
                            return;
                        }
                    }
                    if let Ok(wake) = woken.lock() {
                        wake();
                    }
                }
            });
        let (jobs, inbox) = mpsc::channel::<Job>();
        let (outbox, done) = mpsc::channel();
        let (picked, picker) = mpsc::channel::<Picker>();
        let shown = Arc::new(Mutex::new(HashSet::new()));
        let still = Arc::clone(&shown);
        let decoded = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&decoded);
        let trusted = inflates_safely(&|key: &str| std::env::var(key).ok());
        // Detached: it ends when the cache, and with it the sender, is dropped.
        let _ = std::thread::Builder::new()
            .name(WORKER.to_owned())
            .spawn(move || {
                let Ok(base) = picker.recv() else {
                    return;
                };
                let queried = (base.font_size().width, base.font_size().height);
                // Read here: a picker resized for a zoom has lost what the query found.
                let compress = trusted
                    && base
                        .capabilities()
                        .contains(&ratatui_image::picker::Capability::KittyCompression);
                let mut resized: Option<(Cell, Picker)> = None;
                for job in inbox {
                    let picker = match job.cell {
                        Some(cell) if cell != queried => {
                            if resized.as_ref().is_none_or(|(at, _)| *at != cell) {
                                resized = Some((cell, picker_at(&base, cell)));
                            }
                            resized.as_ref().map_or(&base, |(_, picker)| picker)
                        }
                        _ => &base,
                    };
                    let wanted = still.lock().map_or(
                        true,
                        |shown: std::sync::MutexGuard<'_, HashSet<Arc<Key>>>| {
                            shown.contains(&job.key)
                        },
                    );
                    // Everything one picture takes, decode to encode: a file an
                    // agent wrote must not be able to close the pane.
                    let built = wanted.then(|| {
                        vigia_core::caught(|| build(&job, inks, picker, compress))
                            .unwrap_or_else(undecoded)
                    });
                    // Before the send, so a caller that has the picture sees
                    // what it cost.
                    counted.fetch_add(DECODED.take(), Ordering::Relaxed);
                    vigia_core::armed(Stage::Worker);
                    if outbox.send((job.key, job.cell, built)).is_err() {
                        break;
                    }
                    if let Ok(wake) = wake.lock() {
                        wake();
                    }
                }
            });
        Self {
            built: HashMap::new(),
            pending: HashSet::new(),
            shown,
            jobs,
            done,
            retired: Vec::new(),
            seen: HashMap::new(),
            tick: 0,
            walked: 0,
            tmux: false,
            picker: Some(picked),
            cell: None,
            queried: None,
            queried_in: None,
            decoded,
            found: Vec::new(),
            asked: HashSet::new(),
            epoch: 0,
            pairs,
            scores,
            waiting: 0,
        }
    }

    /// Send the pairs `frame` could not afford to score to be scored, each
    /// once.
    pub fn score_pairs(&mut self, frame: &vigia_core::Frame<'_>) {
        // Begun again with the frame's scores, or a score it dropped is
        // never asked for again.
        if self.epoch != frame.pair_epoch() || self.asked.len() >= frame.pairs_kept() {
            self.epoch = frame.pair_epoch();
            self.asked.clear();
        }
        for candidate in frame.unscored_pairs() {
            if self.asked.insert(candidate.key) {
                // The record lists no pair past its size, and every score it
                // drops moves the epoch, so the clear above is only a bound.
                debug_assert!(
                    self.asked.len() <= frame.pairs_kept(),
                    "the worker holds more pairs sent than the record can score"
                );
                if self.pairs.send(candidate).is_err() {
                    return;
                }
                self.waiting += 1;
            }
        }
    }

    /// Pairs sent to be scored whose scores have not come back. A test seam.
    #[doc(hidden)]
    pub fn pairs_waiting(&self) -> usize {
        self.waiting
    }

    /// The rename scores found since the last call, for
    /// [`vigia_core::Frame::learn_pair`].
    pub fn found_pairs(&mut self) -> Vec<(vigia_core::PairKey, u64)> {
        let found: Vec<_> = self.scores.try_iter().collect();
        self.waiting = self.waiting.saturating_sub(found.len());
        found
    }

    /// File `found` with `frame`, and walk once more when one of them pairs, or
    /// when the last of those sent landed and the frame has more to list;
    /// whether it walked.
    ///
    /// # Errors
    ///
    /// The walk fails.
    pub fn pair(
        &mut self,
        frame: &mut vigia_core::Frame<'_>,
        found: Vec<(vigia_core::PairKey, u64)>,
    ) -> vigia_core::Result<bool> {
        let pairs = found.iter().any(|&(_, score)| vigia_core::pairs(score));
        let drained = !found.is_empty() && self.waiting == 0;
        for (key, score) in found {
            frame.learn_pair(key, score);
        }
        // A score that pairs nothing changes nothing a walk would show.
        let walk = pairs || (drained && frame.pairs_unlisted());
        if walk {
            frame.advance()?;
        }
        Ok(walk)
    }

    /// The sizes the worker found for sides the frame path left unsized, by
    /// stamp, since the last call. The frame takes them with
    /// [`vigia_core::Frame::learn_size`].
    pub fn found_sizes(&mut self) -> Vec<(u64, u32, u32)> {
        std::mem::take(&mut self.found)
    }

    /// How many images this cache's worker has decoded, every picture it has
    /// handed back counted.
    pub fn decodes(&self) -> u64 {
        self.decoded.load(Ordering::Relaxed)
    }

    /// Say how pictures are encoded, which starts the held jobs. Once only.
    pub fn pick(&mut self, picker: Picker) {
        if let Some(picked) = self.picker.take() {
            self.tmux = picker.tmux_detected();
            let cell = (picker.font_size().width, picker.font_size().height);
            self.cell = Some(cell);
            let protocol = picker.protocol_type() != ProtocolType::Halfblocks;
            self.queried = protocol.then_some(cell);
            let _ = picked.send(picker);
        }
    }

    /// Whether a window size can move the cell: not for half-blocks.
    pub fn follows_cell(&self) -> bool {
        self.queried.is_some()
    }

    /// Whether the cell changed with the window: the first call after
    /// [`Pictures::pick`] is the window the cell was queried in, and each later
    /// one rebuilds the pictures on screen when the cell it scales to differs.
    /// Always scaled from that first window, so a size gone and come back is the
    /// queried one.
    pub fn window_sized(&mut self, window: Window) -> bool {
        let Some(queried) = self.queried else {
            return false;
        };
        let Some(then) = &self.queried_in else {
            // A window with no pixels is no baseline; the first that has them is.
            if window.width > 0 && window.height > 0 {
                self.queried_in = Some(window);
            }
            return false;
        };
        let current = self.cell.unwrap_or(queried);
        let Some(cell) = scaled_cell(queried, current, then, &window) else {
            return false;
        };
        if Some(cell) == self.cell {
            return false;
        }
        self.cell = Some(cell);
        self.retire_all();
        true
    }

    /// Keep the pictures in `shown`, and the Kitty pictures that left the
    /// screen while the terminal still holds them, so coming back sends nothing.
    /// One whose side is shown under a newer picture is stale, and goes.
    fn keep(&mut self, shown: &HashSet<Arc<Key>>) {
        self.tick += 1;
        for key in shown {
            if self.built.contains_key(key) {
                self.seen.insert(Arc::clone(key), self.tick);
            }
        }
        // A file's staged and unstaged pictures share neither side. An edit
        // keeps the old side, and staging keeps the new.
        let olds: HashSet<(&str, Option<u64>)> = shown
            .iter()
            .map(|key| (key.path.as_str(), key.before))
            .collect();
        let news: HashSet<(&str, Option<u64>)> = shown
            .iter()
            .map(|key| (key.path.as_str(), key.after))
            .collect();
        let doomed: HashSet<Arc<Key>> = self
            .built
            .iter()
            .filter(|(key, ready)| {
                !shown.contains(*key)
                    && (ready.0.kitty.is_empty()
                        // Never on screen, so nothing of it is in the terminal.
                        || !self.seen.contains_key(*key)
                        || olds.contains(&(key.path.as_str(), key.before))
                        || news.contains(&(key.path.as_str(), key.after)))
            })
            .map(|(key, _)| Arc::clone(key))
            .collect();
        if !doomed.is_empty() {
            self.retire_where(|key, _| doomed.contains(key));
        }
        self.evict(shown);
    }

    /// Retire the Kitty pictures off screen that were shown longest ago, until
    /// what the terminal holds for this pane is within [`KITTY_HELD`].
    fn evict(&mut self, shown: &HashSet<Arc<Key>>) {
        let mut held: u64 = self.built.values().map(|ready| ready.0.stored).sum();
        if held <= KITTY_HELD {
            return;
        }
        let mut off: Vec<(u64, u64, Arc<Key>)> = self
            .built
            .iter()
            .filter(|(key, ready)| !shown.contains(*key) && ready.0.stored > 0)
            .map(|(key, ready)| {
                let seen = self.seen.get(key).copied().unwrap_or(0);
                (seen, ready.0.stored, Arc::clone(key))
            })
            .collect();
        off.sort_unstable_by_key(|(seen, _, _)| *seen);
        let mut doomed = HashSet::new();
        for (_, stored, key) in off {
            if held <= KITTY_HELD {
                break;
            }
            held -= stored;
            doomed.insert(key);
        }
        self.retire_where(|key, _| doomed.contains(key));
    }

    /// Drop each picture `doomed` names, and owe the deletes of its Kitty images.
    fn retire_where(&mut self, doomed: impl Fn(&Arc<Key>, &Built) -> bool) {
        let (retired, seen) = (&mut self.retired, &mut self.seen);
        self.built.retain(|key, ready| {
            let dropped = doomed(key, &ready.0);
            if dropped {
                retired.extend(&ready.0.kitty);
                seen.remove(key);
            }
            !dropped
        });
    }

    /// Retire every kept picture, for a pane about to leave the terminal.
    pub fn retire_all(&mut self) {
        self.retire_where(|_, _| true);
    }

    /// Retire every kept Kitty picture, so it is built again and sent under a
    /// new id: a terminal attached since it was sent never received it. Each
    /// call decodes every such picture again.
    pub fn refresh(&mut self) {
        self.retire_where(|_, built| !built.kitty.is_empty());
    }

    /// For a test with no writer: hand out the uploads due and take each as
    /// written whole at once.
    #[doc(hidden)]
    pub fn written_at_once(&mut self) -> Vec<crate::writer::Upload> {
        let (mut uploads, _) = self.uploads();
        for upload in &mut uploads {
            upload.ticket.written();
        }
        uploads
    }

    /// The Kitty deletes owed since the last call, as escapes, for a test to
    /// read. Empty for any other protocol.
    pub fn retired(&mut self) -> String {
        self.retired_ids()
            .into_iter()
            .map(|id| crate::kitty::delete(id, self.tmux))
            .collect()
    }

    /// The Kitty images to delete since the last call, each cancelled first in
    /// case its upload is still going.
    pub fn retired_ids(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.retired)
    }

    /// Whether Kitty escapes pass through tmux.
    pub fn tmux(&self) -> bool {
        self.tmux
    }

    /// The uploads of on-screen Kitty images not yet handed to the writer, and
    /// the images off screen whose upload has to stop.
    pub fn uploads(&mut self) -> (Vec<crate::writer::Upload>, Vec<u32>) {
        let Ok(shown) = self.shown.lock() else {
            return (Vec::new(), Vec::new());
        };
        let (mut uploads, mut stopped) = (Vec::new(), Vec::new());
        for (key, ready) in &self.built {
            for (_, panel) in &ready.0.panels {
                let Panel::Kitty(image) = panel else {
                    continue;
                };
                if shown.contains(key) {
                    uploads.extend(image.upload());
                } else if image.unsent() {
                    // Sent again if it comes back.
                    stopped.push(image.id());
                }
            }
        }
        (uploads, stopped)
    }

    /// Take in what the worker finished since the last call.
    fn collect(&mut self) {
        while let Ok((key, cell, built)) = self.done.try_recv() {
            self.pending.remove(&key);
            // Built at a cell the window has since left: asked for again.
            let at = cell.or(self.queried);
            if at.is_some() && at != self.cell {
                self.retired
                    .extend(built.iter().flat_map(|built| built.kitty.iter()));
                continue;
            }
            if let Some(built) = built {
                self.found.extend(&built.sizes);
                self.built.insert(key, Ready(Arc::new(built)));
            }
        }
    }

    /// The built picture for `key`, if it is ready.
    pub fn get(&self, key: &Key) -> Option<Ready> {
        self.built.get(key).cloned()
    }

    /// How many built pictures are kept.
    pub fn kept(&self) -> usize {
        self.built.len()
    }

    /// Bring `rows`' pictures up to date with the worker. Only images on screen
    /// are ever read past their header.
    pub fn attach(&mut self, rows: &mut [Row], frame: &vigia_core::Frame<'_>) {
        self.collect();
        // One entry per picture: its rows share one key, side by side.
        let mut hung: Vec<(&Arc<Key>, usize, &Picture)> = Vec::new();
        for row in rows.iter() {
            if let Row::Sized {
                key, file, picture, ..
            }
            | Row::Picture {
                key, file, picture, ..
            } = row
                && hung
                    .last()
                    .is_none_or(|(last, _, _)| !Arc::ptr_eq(last, key))
            {
                hung.push((key, *file, picture));
            }
        }
        let shown: HashSet<Arc<Key>> = hung.iter().map(|(key, _, _)| Arc::clone(key)).collect();
        // Once per walk, since this reads every file in the diff.
        let walks = frame.stats().walks;
        if walks != self.walked {
            self.walked = walks;
            let mut away: HashSet<&str> = self
                .built
                .keys()
                .filter(|key| !shown.contains(*key))
                .map(|key| key.path.as_str())
                .collect();
            if !away.is_empty() {
                for file in frame.files() {
                    away.remove(file.path.as_str());
                }
                let gone: HashSet<String> = away.into_iter().map(str::to_owned).collect();
                self.retire_where(|key, _| gone.contains(&key.path));
            }
        }
        self.keep(&shown);
        // Before any job is sent, or the worker can skip one as not on screen.
        if let Ok(mut on_screen) = self.shown.lock() {
            *on_screen = shown;
        }
        for (key, file, picture) in hung {
            if self.built.contains_key(&**key) || self.pending.contains(&**key) {
                continue;
            }
            let job = Job {
                key: Arc::clone(key),
                sources: frame.image_sources(file),
                formats: (
                    picture.before.map(|side| side.format),
                    picture.after.map(|side| side.format),
                ),
                cell: self.cell,
            };
            if self.jobs.send(job).is_ok() {
                self.pending.insert(Arc::clone(key));
            } else {
                // No worker to build it, so it is drawn as one that failed.
                self.built
                    .insert(Arc::clone(key), Ready(Arc::new(undecoded())));
            }
        }
        for row in rows.iter_mut() {
            if let Row::Sized { key, ready, .. } | Row::Picture { key, ready, .. } = row {
                // A Kitty picture draws once the terminal holds all of it.
                *ready = self.get(key).filter(|ready| ready.0.placed());
            }
        }
        // Blank where a picture did not decode, in place: the rows below are
        // indexed by where they stand. The line says why.
        for row in rows.iter_mut() {
            if matches!(row, Row::Picture { ready: Some(ready), .. } if ready.0.panels.is_empty()) {
                *row = Row::Gap;
            }
        }
    }

    /// Wait up to `limit` for every queued picture, for a caller that cannot
    /// take a wake. `false` when some are still being built.
    pub fn settle(&mut self, limit: std::time::Duration) -> bool {
        let until = std::time::Instant::now() + limit;
        loop {
            self.collect();
            let done = self.pending.is_empty();
            if done || std::time::Instant::now() >= until {
                // A picture that left the screen while it was built is not kept.
                let shared = Arc::clone(&self.shown);
                if let Ok(shown) = shared.lock() {
                    self.keep(&shown);
                }
                return done;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

/// The most a pane's kept Kitty pictures may hold, by [`Built::stored`].
/// kitty and Ghostty each store 320 MB of image data per screen and evict the
/// oldest past it, which would leave a kept picture blank.
const KITTY_HELD: u64 = 64 * 1024 * 1024;

/// The most an image may be, in pixels on a side, before its decode is refused.
const MAX_SIDE: u32 = 16_384;
/// The most one decode may allocate.
const MAX_ALLOC: u64 = 512 * 1024 * 1024;

/// Whether an image of `width` by `height` is within what a decode may take.
fn decodable(width: u32, height: u32) -> bool {
    width <= MAX_SIDE && height <= MAX_SIDE && u64::from(width) * u64::from(height) * 4 <= MAX_ALLOC
}

fn image_format(format: Format) -> Option<ImageFormat> {
    Some(match format {
        Format::Png => ImageFormat::Png,
        Format::Jpeg => ImageFormat::Jpeg,
        Format::Gif => ImageFormat::Gif,
        Format::Webp => ImageFormat::WebP,
        Format::Bmp => ImageFormat::Bmp,
        Format::Ico => ImageFormat::Ico,
        Format::Tiff => ImageFormat::Tiff,
        Format::Qoi => ImageFormat::Qoi,
        Format::Tga => ImageFormat::Tga,
        Format::Svg | Format::Pdf => return None,
    })
}

/// The long side an SVG or a PDF page is rendered at, which every panel is
/// shrunk from, unless what it draws costs more than its budget there.
const LONG_SIDE: f32 = 1024.0;
/// The shortest long side one is rendered at before it is refused.
const SHORTEST_SIDE: f32 = 64.0;
/// What one SVG's filters may cost, in kernel cells times pixels. Sixty-four
/// blurs at 256 pixels a side, about 250ms.
const FILTER_BUDGET: f64 = 1024.0 * 256.0 * 256.0;
/// What a filter primitive costs whose work per pixel is fixed, in kernel
/// cells. A blur measured at about twenty.
const PRIMITIVE_CELLS: u64 = 16;
/// What one octave of turbulence costs, in kernel cells, as measured.
const OCTAVE_CELLS: u64 = 16;

/// SVG references to other files refused, since the process started.
static REFUSED: AtomicU64 = AtomicU64::new(0);

/// How many references to other files SVGs have asked for and been refused.
pub fn hrefs_refused() -> u64 {
    REFUSED.load(Ordering::Relaxed)
}

/// The system's fonts, loaded once on the worker for the first SVG with text.
fn fonts() -> std::sync::Arc<resvg::usvg::fontdb::Database> {
    static FONTS: std::sync::OnceLock<std::sync::Arc<resvg::usvg::fontdb::Database>> =
        std::sync::OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut fonts = resvg::usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            fallback_families(&mut fonts);
            std::sync::Arc::new(fonts)
        })
        .clone()
}

/// Stand-ins for the faces usvg's generic families name, metric-compatible
/// first: Times New Roman, Arial and Courier New.
const SERIF: &[&str] = &[
    "Liberation Serif",
    "Tinos",
    "Nimbus Roman",
    "DejaVu Serif",
    "Noto Serif",
];
const SANS: &[&str] = &[
    "Liberation Sans",
    "Arimo",
    "Nimbus Sans",
    "DejaVu Sans",
    "Noto Sans",
];
const MONO: &[&str] = &[
    "Liberation Mono",
    "Cousine",
    "Nimbus Mono PS",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
];

/// Point `serif`, `sans-serif` and `monospace`, where one names no installed
/// face or one that draws no Latin text, at one that does. usvg falls back to
/// `serif`, Times New Roman, so without one a label draws nothing. A cursive
/// or fantasy label falls through to `serif`.
fn fallback_families(fonts: &mut resvg::usvg::fontdb::Database) {
    use resvg::usvg::fontdb::{Family, Query};
    let wanted: [Generic; 3] = [
        (Family::Serif, SERIF, serif_hint, |fonts, name| {
            fonts.set_serif_family(name)
        }),
        (Family::SansSerif, SANS, sans_hint, |fonts, name| {
            fonts.set_sans_serif_family(name)
        }),
        (Family::Monospace, MONO, mono_hint, |fonts, name| {
            fonts.set_monospace_family(name)
        }),
    ];
    let faces = faces_of(fonts);
    // Read once per face: a face is mapped to read it.
    let read = std::cell::RefCell::new(HashMap::new());
    let latin = |id| {
        *read
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| fonts.with_face_data(id, covers_latin).unwrap_or(false))
    };
    let chosen: Vec<_> = wanted
        .into_iter()
        .filter(|(family, ..)| {
            let resolved = fonts.query(&Query {
                families: &[*family],
                ..Query::default()
            });
            !resolved.is_some_and(latin)
        })
        .filter_map(|(_, known, hinted, set)| {
            Some((set, fallback(&faces, known, hinted, |face| latin(face.id))?))
        })
        .collect();
    for (set, name) in chosen {
        set(fonts, name);
    }
}

/// A generic family, its known stand-ins, the faces its name hints at, and
/// how to point it.
type Generic = (
    resvg::usvg::fontdb::Family<'static>,
    &'static [&'static str],
    fn(&FaceInfo) -> bool,
    fn(&mut resvg::usvg::fontdb::Database, String),
);

fn serif_hint(face: &FaceInfo) -> bool {
    let mut words = face.name.split_whitespace();
    words.clone().any(|word| word == "Serif") && !words.any(|word| word == "Sans")
}

fn sans_hint(face: &FaceInfo) -> bool {
    face.name.split_whitespace().any(|word| word == "Sans") && !face.monospaced
}

fn mono_hint(face: &FaceInfo) -> bool {
    face.monospaced
}

/// What choosing a fallback reads of one installed face.
struct FaceInfo {
    id: resvg::usvg::fontdb::ID,
    name: String,
    monospaced: bool,
}

/// Every installed face, sorted by name.
fn faces_of(fonts: &resvg::usvg::fontdb::Database) -> Vec<FaceInfo> {
    let mut faces: Vec<FaceInfo> = fonts
        .faces()
        .filter_map(|face| {
            Some(FaceInfo {
                id: face.id,
                name: face.families.first()?.0.clone(),
                monospaced: face.monospaced,
            })
        })
        .collect();
    faces.sort_by(|a, b| a.name.cmp(&b.name));
    faces
}

/// The face to fall back to: the first of `known` installed, else one that
/// `hinted` picks, else any. Only a face `latin` passes is taken, and it is
/// read only when reached.
fn fallback(
    faces: &[FaceInfo],
    known: &[&str],
    hinted: impl Fn(&FaceInfo) -> bool,
    latin: impl Fn(&FaceInfo) -> bool,
) -> Option<String> {
    let by_name = known
        .iter()
        .flat_map(|name| faces.iter().filter(move |face| face.name == *name));
    by_name
        .chain(faces.iter().filter(|face| hinted(face)))
        .chain(faces.iter())
        .find(|face| latin(face))
        .map(|face| face.name.clone())
}

/// Whether the face at `index` in `data` maps every ASCII letter and digit
/// to a glyph of its own.
fn covers_latin(data: &[u8], index: u32) -> bool {
    use skrifa::MetadataProvider;
    let Ok(font) = skrifa::FontRef::from_index(data, index) else {
        return false;
    };
    let charmap = font.charmap();
    // A symbol map puts its pictures where the ASCII letters would be.
    if charmap.is_symbol() {
        return false;
    }
    ('a'..='z')
        .chain('A'..='Z')
        .chain('0'..='9')
        .all(|ch| charmap.map(ch).is_some())
}

/// The most encoded bytes one SVG's embedded images may hold, every `<use>`
/// copy counted, before the document is refused.
const EMBEDDED_BUDGET: u64 = 32 * 1024 * 1024;

/// An embedded image, as usvg reads it, once its share of `budget` and its
/// size are known to fit. One with no raster size is left out, as usvg leaves
/// out one it cannot read: a nested SVG is, with its own million nodes. Over
/// the budget or the size limits sets `over`.
fn embedded(
    mime: &str,
    data: std::sync::Arc<Vec<u8>>,
    options: &resvg::usvg::Options,
    budget: &AtomicU64,
    over: &std::sync::atomic::AtomicBool,
) -> Option<resvg::usvg::ImageKind> {
    let refuse = || {
        over.store(true, Ordering::Relaxed);
        None
    };
    let size = data.len() as u64;
    if budget
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
            left.checked_sub(size)
        })
        .is_err()
    {
        return refuse();
    }
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(&**data))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    if !decodable(width, height) {
        return refuse();
    }
    (resvg::usvg::ImageHrefResolver::default_data_resolver())(mime, data, options)
}

/// What one filter primitive costs per pixel.
enum Price {
    Cells(u64),
    /// A window this many root user units across each way, as a morphology
    /// scans, so its cost grows with the render scale.
    Window(f32, f32),
}

/// Every filter primitive `tree` runs, every `<use>` copy counted.
fn prices(tree: &resvg::usvg::Tree) -> Vec<Price> {
    use resvg::usvg::{Group, Node, filter::Kind};
    fn weigh(group: &Group, prices: &mut Vec<Price>) {
        let (sx, sy) = group.abs_transform().get_scale();
        for primitive in group
            .filters()
            .iter()
            .flat_map(|filter| filter.primitives())
        {
            prices.push(match primitive.kind() {
                Kind::ConvolveMatrix(convolve) => {
                    let matrix = convolve.matrix();
                    Price::Cells(u64::from(matrix.columns()) * u64::from(matrix.rows()))
                }
                Kind::Morphology(morphology) => Price::Window(
                    2.0 * morphology.radius_x().get() * sx,
                    2.0 * morphology.radius_y().get() * sy,
                ),
                Kind::Turbulence(turbulence) => {
                    Price::Cells(u64::from(turbulence.num_octaves()).saturating_mul(OCTAVE_CELLS))
                }
                _ => Price::Cells(PRIMITIVE_CELLS),
            });
        }
        for node in group.children() {
            if let Node::Group(child) = node {
                weigh(child, prices);
            }
            node.subroots(|root| weigh(root, prices));
        }
    }
    let mut prices = Vec::new();
    weigh(tree.root(), &mut prices);
    prices
}

/// What `prices` cost over a render `long` pixels on its long side, for a
/// document whose own long side is `size`.
fn filter_cost(prices: &[Price], size: f32, long: f32) -> f64 {
    let scale = long / size;
    let across = |units: f32| (units * scale).ceil().clamp(1.0, long) as f64;
    let cells: f64 = prices
        .iter()
        .map(|price| match *price {
            Price::Cells(cells) => cells as f64,
            Price::Window(x, y) => across(x) * across(y),
        })
        .sum();
    cells * f64::from(long) * f64::from(long)
}

/// The longest side, up to [`LONG_SIDE`], at which every side's filters
/// cost no more than [`FILTER_BUDGET`], or `None` when even
/// [`SHORTEST_SIDE`] costs more.
fn long_side(sides: &[(Vec<Price>, f32)]) -> Option<f32> {
    let fits = |long: f32| {
        sides
            .iter()
            .all(|(prices, size)| filter_cost(prices, *size, long) <= FILTER_BUDGET)
    };
    if fits(LONG_SIDE) {
        return Some(LONG_SIDE);
    }
    if !fits(SHORTEST_SIDE) {
        return None;
    }
    // The cost only grows with the side, so the longest that fits is found by
    // halving.
    let (mut fit, mut over) = (SHORTEST_SIDE as u32, LONG_SIDE as u32);
    while over - fit > 1 {
        let middle = (fit + over) / 2;
        if fits(middle as f32) {
            fit = middle;
        } else {
            over = middle;
        }
    }
    Some(fit as f32)
}

/// One SVG side parsed, with only what the document carries.
fn svg_tree(bytes: &[u8]) -> Option<resvg::usvg::Tree> {
    use resvg::usvg::{ImageHrefResolver, Options, Tree};
    let budget = std::sync::Arc::new(AtomicU64::new(EMBEDDED_BUDGET));
    let over = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (spent, refused) = (std::sync::Arc::clone(&budget), std::sync::Arc::clone(&over));
    let mut options = Options {
        // A reference to any other file is refused, so a changed SVG cannot
        // make the pane open one.
        image_href_resolver: ImageHrefResolver {
            resolve_data: Box::new(move |mime, data, options| {
                embedded(mime, data, options, &spent, &refused)
            }),
            resolve_string: Box::new(|_, _| {
                REFUSED.fetch_add(1, Ordering::Relaxed);
                None
            }),
        },
        ..Options::default()
    };
    // `<text>`, or `<svg:text>` under a prefix.
    if bytes
        .array_windows()
        .any(|window| window == b"<text" || window == b":text")
    {
        options.fontdb = fonts();
    }
    let tree = Tree::from_data(bytes, &options).ok()?;
    (!over.load(Ordering::Relaxed)).then_some(tree)
}

/// One SVG side rendered, its long side at `long`.
fn render_svg(tree: &resvg::usvg::Tree, long: f32) -> Option<RgbaImage> {
    let size = tree.size();
    let scale = long / size.width().max(size.height());
    let width = (size.width() * scale).ceil().max(1.0) as u32;
    let height = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    RgbaImage::from_raw(width, height, pixmap.take_demultiplied())
}

/// Each side of a changed SVG rendered at one long side, so the two compare,
/// or `None` when either cannot be drawn.
fn svg_sides(job: &Job) -> Option<(Option<Decoded>, Option<Decoded>)> {
    let tree = |source: &Option<Source>| match source {
        Some(source) => svg_tree(&read_counted(source)?).map(Some),
        None => Some(None),
    };
    let (old, new) = (tree(&job.sources.before)?, tree(&job.sources.after)?);
    let sides: Vec<_> = [&old, &new]
        .into_iter()
        .flatten()
        .map(|tree| (prices(tree), tree.size().width().max(tree.size().height())))
        .collect();
    let long = long_side(&sides)?;
    let render = |tree: Option<resvg::usvg::Tree>| match tree {
        Some(tree) => render_svg(&tree, long).map(|image| Some((image, None))),
        None => Some(None),
    };
    Some((render(old)?, render(new)?))
}

/// What the worker draws of a changed PDF.
struct PdfDrawn {
    pub(crate) old: Option<Decoded>,
    pub(crate) new: Option<Decoded>,
    /// What the line says about the page.
    pub(crate) note: Option<String>,
    /// The bar of a page one side added or removed, drawn alone.
    pub(crate) alone: Option<Bar>,
}

/// A side decoded, and its colour profile.
type Decoded = (RgbaImage, Option<Vec<u8>>);

/// A side's bytes, counted as one decode.
fn read_counted(source: &Source) -> Option<Vec<u8>> {
    let bytes = source.read()?;
    vigia_core::armed(Stage::Decode);
    if std::thread::current().name() == Some(WORKER) {
        DECODED.update(|v| v + 1);
    } else {
        OFF_WORKER.fetch_add(1, Ordering::Relaxed);
    }
    Some(bytes)
}

/// One side decoded, with its colour profile. The first frame only.
fn decode(source: &Source, raster: ImageFormat) -> Option<Decoded> {
    use image::ImageDecoder;
    let bytes = read_counted(source)?;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes));
    reader.set_format(raster);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let profile = decoder.icc_profile().ok().flatten();
    let image = DynamicImage::from_decoder(decoder).ok()?.into_rgba8();
    Some((image, profile))
}

/// `image` laid over the pane in place, so a transparent pixel shows what is
/// behind it.
fn flatten(image: &mut RgbaImage, canvas: (u8, u8, u8)) {
    let (r, g, b) = canvas;
    let under = [r, g, b].map(u16::from);
    for pixel in image.pixels_mut() {
        let alpha = u16::from(pixel.0[3]);
        for (channel, under) in pixel.0.iter_mut().zip(under) {
            *channel = ((u16::from(*channel) * alpha + under * (255 - alpha)) / 255) as u8;
        }
        pixel.0[3] = 255;
    }
}

/// Which pixels differ between two images of one size, alpha included.
fn changed(old: &RgbaImage, new: &RgbaImage) -> Vec<bool> {
    // Two pixels nobody can see are the same whatever colour they store.
    old.pixels()
        .zip(new.pixels())
        .map(|(a, b)| a != b && (a.0[3] != 0 || b.0[3] != 0))
        .collect()
}

/// `mask`, of an image `width` by `height`, shrunk to `to_width` by `to_height`:
/// a pixel is set when any pixel under it is.
fn shrink_mask(
    mask: &[bool],
    (width, height): (u32, u32),
    (to_width, to_height): (u32, u32),
) -> Vec<bool> {
    let mut out = vec![false; (to_width * to_height) as usize];
    for y in 0..height {
        let row = (y * to_height / height) * to_width;
        for x in 0..width {
            if mask[(y * width + x) as usize] {
                out[(row + x * to_width / width) as usize] = true;
            }
        }
    }
    out
}

/// How a panel's pixels are painted from the shrunk picture.
#[derive(Clone, Copy)]
enum Paint {
    /// As they are.
    Plain,
    /// Toward the pane, for a picture that is gone.
    Dim,
    /// Grey toward the pane, with a changed pixel in the changed ink.
    Mask,
}

/// One panel: `image`, already shrunk to the slot's pixels, painted and encoded
/// for `picker`.
fn panel(
    image: RgbImage,
    hit: Option<&Hits>,
    slot: Slot,
    paint: Paint,
    inks: Inks,
    picker: &Picker,
    compress: bool,
) -> Option<(Panel, Option<u32>)> {
    let image = DynamicImage::ImageRgb8(painted(image, hit, paint, inks));
    let size = Size::new(slot.width, slot.rows);
    vigia_core::armed(Stage::Encode);
    // Direct, because the picker's fit would read the grid as font pixels.
    if picker.protocol_type() == ProtocolType::Halfblocks {
        return Some((
            Panel::Sliced(SlicedProtocol::Halfblocks(
                Halfblocks::new(image, size).ok()?,
            )),
            None,
        ));
    }
    if picker.protocol_type() == ProtocolType::Kitty {
        // Built here rather than by the picker, so the id is known for its delete.
        let id = kitty_id();
        let cells = ratatui_image::Resize::natural_size(&image, picker.font_size());
        let rgba = image.to_rgba8();
        let kitty = crate::kitty::Image::new(
            rgba.as_raw(),
            rgba.dimensions(),
            cells,
            id,
            picker.tmux_detected(),
            compress,
        );
        return Some((Panel::Kitty(kitty), Some(id)));
    }
    let sliced =
        SlicedProtocol::new_with_resize(picker, image, size, ratatui_image::Resize::Fit(None))
            .ok()?;
    Some((Panel::Sliced(sliced), None))
}

/// A slot's half-block grid: two pixels to a cell's height.
fn grid_of(slot: Slot) -> (u32, u32) {
    (u32::from(slot.width), u32::from(slot.rows) * 2)
}

/// The pixels a slot is drawn at: its half-block grid, or for a protocol the
/// image fitted into the slot's real pixels. Fitted here rather than by the
/// crate because the mask is painted at these pixels.
fn pixels_of(slot: Slot, (width, height): (u32, u32), picker: &Picker) -> (u32, u32) {
    if picker.protocol_type() == ProtocolType::Halfblocks {
        return grid_of(slot);
    }
    let font = picker.font_size();
    let room = (
        u32::from(slot.width) * u32::from(font.width),
        u32::from(slot.rows) * u32::from(font.height),
    );
    let scaled = |side: u32, to: u32, by: u32, cap: u32| {
        ((u64::from(side) * u64::from(to) / u64::from(by)) as u32).clamp(1, cap)
    };
    // The side that binds is the room exactly, so the crate's own fit sees a
    // whole number of cells and does not resize again.
    if u64::from(room.0) * u64::from(height) <= u64::from(room.1) * u64::from(width) {
        (room.0, scaled(height, room.0, width, room.1))
    } else {
        (scaled(width, room.1, height, room.0), room.1)
    }
}

/// Which cells of a half-block grid `width` by `height` changed.
struct Hits {
    cells: Vec<bool>,
    width: u32,
    height: u32,
}

impl Hits {
    /// Whether the pixel at `i` of an image `width` by `height` lies in a changed
    /// cell, so a change is as wide on a protocol's pixels as on half-blocks.
    fn at(&self, i: usize, (width, height): (u32, u32)) -> bool {
        let (x, y) = (i as u32 % width, i as u32 / width);
        let cell = (y * self.height / height) * self.width + x * self.width / width;
        self.cells[cell as usize]
    }
}

/// `image`'s pixels in `paint`, with `hit` marking the changed ones for a mask.
fn painted(mut image: RgbImage, hit: Option<&Hits>, paint: Paint, inks: Inks) -> RgbImage {
    let dimensions = image.dimensions();
    let (cr, cg, cb) = inks.canvas;
    let canvas = [cr, cg, cb].map(f32::from);
    // Toward the pane and away from it by lightness, hard enough that a light
    // screen does not become a slab beside the changed ink.
    let toward = if crate::Background::of(inks.canvas) == crate::Background::Light {
        -38.0
    } else {
        38.0
    };
    for (i, pixel) in image.pixels_mut().enumerate() {
        let p = pixel.0.map(f32::from);
        pixel.0 = match paint {
            Paint::Plain => pixel.0,
            Paint::Dim => [0, 1, 2].map(|c| (p[c] * 0.45 + canvas[c] * 0.55) as u8),
            Paint::Mask if hit.is_some_and(|hit| hit.at(i, dimensions)) => {
                let (r, g, b) = inks.changed;
                [r, g, b]
            }
            Paint::Mask => {
                let light = (p[0] * 0.299 + p[1] * 0.587 + p[2] * 0.114) / 255.0;
                // Light stays light and dark stays dark on either pane.
                let step = if toward < 0.0 {
                    (1.0 - light) * toward
                } else {
                    light * toward
                };
                canvas.map(|c| (c + step).clamp(0.0, 255.0) as u8)
            }
        };
    }
    image
}

/// Build one picture: read and decode both sides, compare them, and encode each
/// panel. A side that does not decode builds nothing to draw.
fn build(job: &Job, inks: Inks, picker: &Picker, compress: bool) -> Built {
    let side = |source: &Option<Source>, format: Option<Format>| match (source, format) {
        (Some(source), Some(format)) => decode(source, image_format(format)?).map(Some),
        (None, None) => Some(None),
        _ => None,
    };
    let vector = |format| job.formats.0 == Some(format) || job.formats.1 == Some(format);
    if vector(Format::Pdf) && job.key.shape.slots.is_empty() {
        return pdf::sized(job);
    }
    let (mut page, mut alone) = (None, None);
    let sides = if vector(Format::Svg) {
        svg_sides(job)
    } else if vector(Format::Pdf) {
        pdf::drawn(job).map(|drawn| {
            (page, alone) = (drawn.note, drawn.alone);
            (drawn.old, drawn.new)
        })
    } else {
        side(&job.sources.before, job.formats.0).zip(side(&job.sources.after, job.formats.1))
    };
    let Some((old, new)) = sides else {
        return undecoded();
    };
    let profiles_differ = matches!((&old, &new), (Some((_, a)), Some((_, b))) if a != b);
    let (mut old, mut new) = (old.map(|(image, _)| image), new.map(|(image, _)| image));
    let mask = match (&old, &new) {
        (Some(old), Some(new)) if old.dimensions() == new.dimensions() => Some(changed(old, new)),
        _ => None,
    };
    for image in [old.as_mut(), new.as_mut()].into_iter().flatten() {
        flatten(image, inks.canvas);
    }
    let verdict = match &mask {
        Some(mask) => match mask.iter().filter(|hit| **hit).count() as u64 {
            0 => Verdict::Unchanged,
            pixels => Verdict::Changed {
                pixels,
                total: mask.len() as u64,
            },
        },
        None => Verdict::Uncompared,
    };
    // Each side shrunk once per panel size, so the new picture and its mask
    // share one pass.
    let mut shrunk: HashMap<(Side, u32, u32), RgbImage> = HashMap::new();
    let mut panels = Vec::new();
    let mut kitty = Vec::new();
    let mut stored = 0;
    // A page drawn alone takes the first slot, after its bar, as an added or
    // deleted image does.
    let (slots, paints): (Vec<Slot>, &[(Side, Paint)]) = match (alone, job.key.shape.slots.first())
    {
        (Some(bar), Some(first)) => (
            vec![Slot {
                x: first.x + 1,
                width: first.width.saturating_sub(1),
                rows: first.rows,
            }],
            match bar {
                Bar::Added => Kind::Added.paints(),
                Bar::Removed => Kind::Deleted.paints(),
            },
        ),
        _ => (job.key.shape.slots.clone(), job.key.shape.kind.paints()),
    };
    for (&slot, &(side, paint)) in slots.iter().zip(paints) {
        let image = match side {
            Side::Old => old.as_ref(),
            Side::New => new.as_ref(),
        };
        let Some(image) = image else {
            return undecoded();
        };
        let to = pixels_of(slot, image.dimensions(), picker);
        let small = shrunk
            .entry((side, to.0, to.1))
            .or_insert_with(|| {
                // Averaged on the way down; kept sharp on the way up, so an icon
                // smaller than its panel stays a picture of pixels.
                let filter = if to.0 < image.width() {
                    image::imageops::FilterType::Triangle
                } else {
                    image::imageops::FilterType::Nearest
                };
                DynamicImage::ImageRgba8(image::imageops::resize(image, to.0, to.1, filter))
                    .to_rgb8()
            })
            .clone();
        let hit = match (paint, &mask) {
            (Paint::Mask, Some(mask)) => {
                let grid = grid_of(slot);
                Some(Hits {
                    cells: shrink_mask(mask, image.dimensions(), grid),
                    width: grid.0,
                    height: grid.1,
                })
            }
            _ => None,
        };
        let pixels = u64::from(small.width()) * u64::from(small.height());
        let Some((protocol, id)) = panel(small, hit.as_ref(), slot, paint, inks, picker, compress)
        else {
            return undecoded();
        };
        if let Some(id) = id {
            kitty.push(id);
            stored += protocol.held(pixels);
        }
        panels.push((slot, protocol));
    }
    Built {
        panels,
        verdict,
        profiles_differ,
        kitty,
        stored,
        page_note: page,
        sizes: Vec::new(),
        bar: alone,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitty_sent_compressed() {
        #[allow(deprecated)]
        let mut picker = Picker::from_fontsize((10, 20).into());
        picker.set_protocol_type(ProtocolType::Kitty);
        let inks = Inks {
            canvas: (0, 0, 0),
            changed: (255, 0, 0),
        };
        let slot = Slot {
            x: 0,
            width: 4,
            rows: 2,
        };
        let sent = |compress| {
            let (protocol, _) = panel(
                RgbImage::new(40, 40),
                None,
                slot,
                Paint::Plain,
                inks,
                &picker,
                compress,
            )
            .expect("a panel");
            let Panel::Kitty(image) = &protocol else {
                panic!("a Kitty picker built another panel");
            };
            image.upload().expect("an upload").data.to_string()
        };
        assert!(sent(true).contains("o=z"), "asked to compress, it sent raw");
        assert!(!sent(false).contains("o=z"), "it compressed unasked");
    }

    #[test]
    fn photo_sent_raw() {
        // A picture no deflate shrinks goes uncompressed even to a terminal that
        // takes compression, and is counted as the base64 kept for it.
        #[allow(deprecated)]
        let mut picker = Picker::from_fontsize((10, 20).into());
        picker.set_protocol_type(ProtocolType::Kitty);
        let inks = Inks {
            canvas: (0, 0, 0),
            changed: (255, 0, 0),
        };
        let slot = Slot {
            x: 0,
            width: 100,
            rows: 50,
        };
        let noise = RgbImage::from_raw(1000, 1000, crate::kitty::noise(1000 * 1000 * 3))
            .expect("a whole image");
        let (panel, _) =
            panel(noise, None, slot, Paint::Plain, inks, &picker, true).expect("a panel");
        assert!(
            matches!(panel, Panel::Kitty(_)),
            "a Kitty picker built another panel"
        );
        assert_eq!(
            panel.held(1000 * 1000),
            1000 * 1000 * 16 / 3,
            "noise was deflated though deflating it saves nothing, or not counted as its base64"
        );
    }

    #[test]
    fn kitty_unsent_dropped() {
        let mut pictures = Pictures::unpicked(
            Inks {
                canvas: (0, 0, 0),
                changed: (255, 0, 0),
            },
            Box::new(|| {}),
        );
        let key = Arc::new(Key {
            path: "a.png".to_owned(),
            before: Some(1),
            after: Some(2),
            shape: Shape {
                kind: Kind::Three,
                rows: 1,
                slots: Vec::new(),
            },
        });
        // Built after it scrolled away, so never on screen.
        let built = Built {
            kitty: vec![7],
            stored: 1,
            ..undecoded()
        };
        pictures.built.insert(key, Ready(Arc::new(built)));
        pictures.keep(&HashSet::new());
        assert_eq!(pictures.kept(), 0, "a picture never on screen was kept");
    }

    #[test]
    fn kitty_stale_side() {
        let mut pictures = Pictures::unpicked(
            Inks {
                canvas: (0, 0, 0),
                changed: (255, 0, 0),
            },
            Box::new(|| {}),
        );
        let key = |before, after| {
            Arc::new(Key {
                path: "a.png".to_owned(),
                before: Some(before),
                after: Some(after),
                shape: Shape {
                    kind: Kind::Three,
                    rows: 1,
                    slots: Vec::new(),
                },
            })
        };
        let (staged, unstaged, edited) = (key(1, 2), key(2, 3), key(2, 4));
        for (id, key) in [(1, &staged), (2, &unstaged), (3, &edited)] {
            let built = Built {
                kitty: vec![id],
                stored: 1,
                ..undecoded()
            };
            pictures
                .built
                .insert(Arc::clone(key), Ready(Arc::new(built)));
        }
        pictures.keep(&HashSet::from([Arc::clone(&staged), Arc::clone(&unstaged)]));
        pictures.keep(&HashSet::from([Arc::clone(&unstaged)]));
        assert!(
            pictures.get(&staged).is_some(),
            "the staged picture of a shown path went"
        );
        pictures.keep(&HashSet::from([Arc::clone(&edited)]));
        assert!(
            pictures.get(&unstaged).is_none(),
            "the picture an edit replaced was kept"
        );
        assert!(
            pictures.get(&staged).is_some(),
            "an edit to the worktree dropped the staged picture"
        );
        // Staged: the edit's new side is now the staged picture's.
        let added = key(1, 4);
        for (id, key) in [(3, &edited), (4, &added)] {
            let built = Built {
                kitty: vec![id],
                stored: 1,
                ..undecoded()
            };
            pictures
                .built
                .insert(Arc::clone(key), Ready(Arc::new(built)));
        }
        pictures.keep(&HashSet::from([Arc::clone(&edited), Arc::clone(&added)]));
        assert!(pictures.get(&edited).is_some(), "a shown picture went");
        pictures.keep(&HashSet::from([Arc::clone(&added)]));
        assert!(
            pictures.get(&edited).is_none(),
            "the picture staging replaced was kept"
        );
    }

    #[test]
    fn kitty_evicted_oldest() {
        let mut pictures = Pictures::unpicked(
            Inks {
                canvas: (0, 0, 0),
                changed: (255, 0, 0),
            },
            Box::new(|| {}),
        );
        let key = |n: u64| {
            Arc::new(Key {
                path: format!("{n}.png"),
                before: Some(n),
                after: Some(n + 1),
                shape: Shape {
                    kind: Kind::Three,
                    rows: 1,
                    slots: Vec::new(),
                },
            })
        };
        // Three of 30 MiB each, shown one after another: 90 MiB off screen.
        for n in 0..3 {
            let built = Built {
                kitty: vec![u32::try_from(n + 1).expect("small")],
                stored: 30 * 1024 * 1024,
                ..undecoded()
            };
            pictures.built.insert(key(n), Ready(Arc::new(built)));
            pictures.keep(&HashSet::from([key(n)]));
        }
        assert_eq!(
            pictures.retired(),
            "\x1b_Ga=d,d=I,i=1,q=2\x1b\\",
            "not exactly the oldest went"
        );
        assert_eq!(pictures.kept(), 2, "more than the bound requires went");
        // Past the bound with only what is on screen: none of it goes.
        pictures.built.insert(
            key(9),
            Ready(Arc::new(Built {
                kitty: vec![9],
                stored: 70 * 1024 * 1024,
                ..undecoded()
            })),
        );
        pictures.keep(&HashSet::from([key(9)]));
        assert!(
            pictures.get(&key(9)).is_some(),
            "a picture on screen was evicted"
        );
    }

    #[test]
    fn scaled_cell_slack() {
        let window = |columns, rows, width, height| Window {
            rows,
            columns,
            width,
            height,
        };
        let then = window(100, 30, 1000, 600);
        let at = |now: &Window| scaled_cell((10, 20), (10, 20), &then, now);
        assert_eq!(at(&then), Some((10, 20)));
        // A zoom of one pixel a cell, and of twice.
        assert_eq!(at(&window(90, 27, 1000, 600)), Some((11, 22)));
        assert_eq!(at(&window(50, 15, 1000, 600)), Some((20, 40)));
        // A pane cut to fewer cells, or left with a partial row or column,
        // keeps its cell.
        assert_eq!(at(&window(20, 10, 208, 219)), Some((10, 20)));
        assert_eq!(
            scaled_cell(
                (10, 20),
                (10, 20),
                &window(100, 30, 1040, 619),
                &window(20, 10, 208, 200)
            ),
            Some((10, 20))
        );
        // A queried cell in another unit than the window's pixels.
        assert_eq!(
            scaled_cell(
                (10, 20),
                (10, 20),
                &window(100, 30, 500, 300),
                &window(50, 15, 250, 150)
            ),
            Some((10, 20))
        );
        // Rounded, not cut.
        assert_eq!(at(&window(85, 30, 1000, 600)), Some((12, 20)));
        // Gone and come back through windows with leftover pixels.
        let gone = scaled_cell(
            (10, 20),
            (10, 20),
            &window(100, 10, 1000, 200),
            &window(100, 40, 1000, 920),
        );
        assert_eq!(gone, Some((10, 23)));
        assert_eq!(
            scaled_cell(
                (10, 20),
                (10, 23),
                &window(100, 10, 1000, 200),
                &window(100, 10, 1000, 200)
            ),
            Some((10, 20)),
            "the query's own window kept the zoomed cell"
        );
        // Zoomed to twice, then widened: the query window's leftover lowers the
        // scaled cell, and the current one stays.
        assert_eq!(
            scaled_cell(
                (10, 20),
                (20, 40),
                &window(10, 30, 109, 600),
                &window(40, 15, 800, 600)
            ),
            Some((20, 40))
        );
        assert_eq!(at(&window(100, 30, 0, 0)), None);
        assert_eq!(
            scaled_cell((10, 20), (10, 20), &window(100, 30, 0, 600), &then),
            None
        );
        assert_eq!(at(&window(100, 30, 1, 1)), Some((1, 1)));
    }

    #[test]
    fn sixel_font_change() {
        // A Sixel picture built after the cell doubles is drawn at the new
        // cell's pixels, through the picker rebuilt at that size.
        let mut base = Picker::halfblocks();
        base.set_protocol_type(ProtocolType::Sixel);
        let inks = Inks {
            canvas: (0, 0, 0),
            changed: (255, 0, 0),
        };
        let slot = Slot {
            x: 0,
            width: 4,
            rows: 2,
        };
        let width = |cell: Cell| {
            let picker = picker_at(&base, cell);
            assert_eq!(
                picker.tmux_detected(),
                base.tmux_detected(),
                "the rebuild lost tmux"
            );
            let pixels = pixels_of(slot, (100, 100), &picker);
            let (panel, _) = panel(
                RgbImage::new(pixels.0, pixels.1),
                None,
                slot,
                Paint::Plain,
                inks,
                &picker,
                false,
            )
            .expect("a panel");
            let area = Rect::new(0, 0, 4, 2);
            let mut buf = Buffer::empty(area);
            panel.render(area, &mut buf, 0);
            let sent: String = buf.content().iter().map(|cell| cell.symbol()).collect();
            // The raster attributes: aspect, aspect, width, height.
            let raster = sent.split("\"1;1;").nth(1).expect("a Sixel image");
            raster
                .split(';')
                .next()
                .expect("a width")
                .parse::<u32>()
                .expect("a number")
        };
        assert_eq!(
            (width((10, 20)), width((20, 40))),
            (40, 80),
            "a Sixel picture was not drawn at the new cell's pixels"
        );
    }

    #[test]
    fn picker_parity() {
        // If `ratatui-image` drops `from_fontsize`, this and the build fail.
        for protocol in [
            ProtocolType::Kitty,
            ProtocolType::Sixel,
            ProtocolType::Iterm2,
        ] {
            let mut base = Picker::halfblocks();
            base.set_protocol_type(protocol);
            let at = picker_at(&base, (17, 33));
            assert_eq!(at.protocol_type(), base.protocol_type());
            assert_eq!(at.tmux_detected(), base.tmux_detected());
            assert_eq!((at.font_size().width, at.font_size().height), (17, 33));
        }
    }
    use image::Rgb;

    #[test]
    fn missing_family_falls_back() {
        use resvg::usvg::fontdb::{Database, Family, Query};
        let mut fonts = Database::new();
        fonts.load_system_fonts();
        if fonts.is_empty() {
            eprintln!("note: this system has no fonts, so the fallback test proves nothing here");
            return;
        }
        // As on a system without Times New Roman.
        fonts.set_serif_family("No Such Family");
        fallback_families(&mut fonts);
        let found = fonts.query(&Query {
            families: &[Family::Serif],
            ..Query::default()
        });
        assert!(
            found.is_some(),
            "serif names no installed face, so a label draws nothing"
        );
    }

    fn face(name: &str, monospaced: bool) -> FaceInfo {
        FaceInfo {
            id: resvg::usvg::fontdb::ID::dummy(),
            name: name.to_owned(),
            monospaced,
        }
    }

    #[test]
    fn fallback_draws_latin() {
        let faces = [
            face("Abyssinica SIL", false),
            face("Noto Sans Symbols", false),
            face("Ubuntu Sans", false),
        ];
        let latin =
            |face: &FaceInfo| face.name != "Noto Sans Symbols" && face.name != "Abyssinica SIL";
        assert_eq!(
            fallback(&faces, SANS, sans_hint, latin).as_deref(),
            Some("Ubuntu Sans"),
            "a face with no Latin glyphs was chosen, so a label draws nothing"
        );
        assert_eq!(
            fallback(&faces[..2], SANS, sans_hint, latin),
            None,
            "a face with no Latin glyphs was chosen as the last resort"
        );
    }

    #[test]
    fn fallback_prefers_known() {
        let faces = [face("DejaVu Serif", false), face("Liberation Serif", false)];
        assert_eq!(
            fallback(&faces, SERIF, serif_hint, |_| true).as_deref(),
            Some("Liberation Serif"),
            "Times New Roman's metric stand-in was passed over"
        );
    }

    #[test]
    fn fallback_follows_hints() {
        assert!(
            !sans_hint(&face("Sansita Swashed", false)),
            "a display face was hinted sans-serif for the letters in its name"
        );
        assert!(
            !serif_hint(&face("Serifa", false)),
            "a face was hinted serif for the letters in its name"
        );
        let serif = [face("Arimo", false), face("Gentium Serif", false)];
        assert_eq!(
            fallback(&serif, &[], serif_hint, |_| true).as_deref(),
            Some("Gentium Serif"),
            "serif fell back to a face its name does not call serif"
        );
        let sans = [face("Fira Sans Mono", true), face("Fira Sans", false)];
        assert_eq!(
            fallback(&sans, &[], sans_hint, |_| true).as_deref(),
            Some("Fira Sans"),
            "sans-serif fell back to a monospaced face"
        );
        let mono = [face("Hack", true), face("Monoton", false)];
        assert_eq!(
            fallback(&mono[1..], &[], mono_hint, |_| true).as_deref(),
            Some("Monoton"),
            "with no monospaced face, monospace found nothing"
        );
        assert_eq!(
            fallback(
                &[face("Monoton", false), face("Hack", true)],
                &[],
                mono_hint,
                |_| true
            )
            .as_deref(),
            Some("Hack"),
            "monospace fell back to a face that is not monospaced"
        );
    }

    #[test]
    fn latin_coverage_read() {
        assert!(
            !covers_latin(b"not a font", 0),
            "garbage read as a Latin face"
        );
        let mut fonts = resvg::usvg::fontdb::Database::new();
        fonts.load_system_fonts();
        if fonts.is_empty() {
            eprintln!("note: this system has no fonts, so the coverage test proves less here");
            return;
        }
        let read = |face: &FaceInfo| fonts.with_face_data(face.id, covers_latin);
        let faces = faces_of(&fonts);
        assert!(
            faces.iter().any(|face| read(face) == Some(true)),
            "no installed face reads as covering Latin"
        );
        // These emoji faces map digits but no letters (Segoe UI Emoji maps
        // both); symbol faces map pictures
        // onto the letters.
        let named = |names: &[&str]| -> Vec<&FaceInfo> {
            faces
                .iter()
                .filter(|face| names.contains(&face.name.as_str()))
                .collect()
        };
        for (kind, names) in [
            ("emoji", &["Apple Color Emoji", "Noto Color Emoji"][..]),
            (
                "symbol",
                &["Wingdings", "Webdings", "Symbol", "OpenSymbol", "D050000L"][..],
            ),
        ] {
            let found = named(names);
            if found.is_empty() {
                eprintln!("note: no {kind} face here");
            }
            for face in found {
                assert_eq!(
                    read(face),
                    Some(false),
                    "{} reads as covering Latin",
                    face.name
                );
            }
        }
    }

    #[test]
    fn generics_fall_back() {
        use resvg::usvg::fontdb::{Database, Family, Query};
        let mut fonts = Database::new();
        fonts.load_system_fonts();
        if fonts.is_empty() {
            eprintln!("note: this system has no fonts, so the fallback test proves nothing here");
            return;
        }
        // As on a system without the defaults, and with serif on a face that
        // draws no Latin.
        let emoji = faces_of(&fonts)
            .into_iter()
            .find(|face| face.name.contains("Emoji"))
            .map_or("No Such Family".to_owned(), |face| face.name);
        fonts.set_serif_family(emoji);
        fonts.set_sans_serif_family("No Such Family");
        fonts.set_monospace_family("No Such Family");
        fallback_families(&mut fonts);
        for (family, monospaced) in [
            (Family::Serif, None),
            (Family::SansSerif, Some(false)),
            (Family::Monospace, Some(true)),
        ] {
            let id = fonts
                .query(&Query {
                    families: &[family],
                    ..Query::default()
                })
                .unwrap_or_else(|| panic!("{family:?} names no installed face"));
            assert_eq!(
                fonts.with_face_data(id, covers_latin),
                Some(true),
                "{family:?} names a face that draws no Latin"
            );
            if let Some(monospaced) = monospaced {
                let face = fonts.face(id).expect("face");
                assert_eq!(
                    face.monospaced, monospaced,
                    "{family:?} points at {}",
                    face.families[0].0
                );
            }
        }
    }

    fn side(width: u32, height: u32) -> ImageSide {
        ImageSide {
            format: Format::Png,
            width,
            height,
            bytes: 0,
            stamp: 0,
        }
    }

    fn both(width: u32, height: u32) -> Picture {
        Picture {
            before: Some(side(width, height)),
            after: Some(side(width, height)),
        }
    }

    #[test]
    fn three_panels() {
        let shape = shape(&both(1242, 2688), 76, 19, true).expect("a shape");
        assert_eq!(shape.kind, Kind::Three);
        assert_eq!(shape.slots.len(), 3);
        let first = shape.slots[0];
        assert_eq!(
            shape.slots[1].x,
            first.width + GAP,
            "panels are not two columns apart"
        );
        assert!(first.width >= MIN_WIDTH && first.rows >= MIN_ROWS);
    }

    #[test]
    fn mask_alone_narrow() {
        let shape = shape(&both(2000, 800), 60, 19, true).expect("a shape");
        assert_eq!(shape.kind, Kind::Mask);
        assert_eq!(shape.slots.len(), 1);
    }

    #[test]
    fn no_picture_short() {
        assert_eq!(shape(&both(1242, 2688), 76, 5, true), None);
    }

    #[test]
    fn no_picture_below_truecolour() {
        assert_eq!(shape(&both(1242, 2688), 120, 40, false), None);
    }

    #[test]
    fn added_and_deleted_one_panel() {
        let added = Picture {
            before: None,
            after: Some(side(1242, 2688)),
        };
        let shape_added = shape(&added, 76, 19, true).expect("a shape");
        assert_eq!(
            (shape_added.kind, shape_added.slots.len()),
            (Kind::Added, 1)
        );
        assert_eq!(
            shape_added.slots[0].x, 1,
            "the bar does not have the first column"
        );
        let three = shape(&both(1242, 2688), 76, 19, true).expect("a shape");
        assert!(
            shape_added.slots[0].width <= three.slots[0].width,
            "an added picture is wider than a three-panel one"
        );
        let deleted = Picture {
            before: Some(side(1242, 2688)),
            after: None,
        };
        assert_eq!(
            shape(&deleted, 76, 19, true).map(|shape| shape.kind),
            Some(Kind::Deleted)
        );
    }

    #[test]
    fn resized_pair() {
        let picture = Picture {
            before: Some(side(1170, 2532)),
            after: Some(side(1179, 2556)),
        };
        assert_eq!(
            shape(&picture, 76, 19, true).map(|shape| shape.kind),
            Some(Kind::Pair)
        );
    }

    #[test]
    fn one_pixel_lights() {
        let old = RgbaImage::from_pixel(1000, 1000, image::Rgba([255, 255, 255, 255]));
        let mut new = old.clone();
        new.put_pixel(503, 497, image::Rgba([0, 0, 0, 255]));
        let mask = changed(&old, &new);
        let hit = shrink_mask(&mask, new.dimensions(), (20, 20));
        assert_eq!(
            hit.iter().filter(|hit| **hit).count(),
            1,
            "one changed pixel lit no cell, or more than one"
        );
    }

    #[test]
    fn every_change_counts() {
        let old = RgbaImage::from_pixel(3, 1, image::Rgba([100, 100, 100, 255]));
        let mut new = old.clone();
        new.put_pixel(0, 0, image::Rgba([101, 100, 100, 255]));
        new.put_pixel(1, 0, image::Rgba([100, 100, 100, 0]));
        assert_eq!(
            changed(&old, &new),
            [true, true, false],
            "a one-step shade or an alpha-only change went uncounted"
        );
    }

    #[test]
    fn invisible_pixels_equal() {
        let old = RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 0]));
        let new = RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0]));
        assert_eq!(
            changed(&old, &new),
            [false],
            "a re-encoded transparent pixel counted"
        );
    }

    #[test]
    fn mask_ink_defaults() {
        let changed = |theme: &crate::Theme| Inks::of(theme, None, false).changed;
        let colours = crate::terminal::Colours {
            background: (0x10, 0x10, 0x10),
            foreground: None,
            ansi: [Some((0xaa, 0x33, 0x33)); 16],
        };
        // Pinned by value, so a fallback that moved shows here.
        for (name, theme, today) in [
            ("ansi", crate::Theme::ansi(), (0xf8, 0x51, 0x49)),
            ("dark", crate::Theme::dark(), (0xf8, 0x51, 0x49)),
            ("light", crate::Theme::light(), (0xcf, 0x22, 0x2e)),
            ("system", crate::Theme::system(None), (0xf8, 0x51, 0x49)),
            (
                "system answered",
                crate::Theme::system(Some(&colours)),
                (0xf8, 0x51, 0x49),
            ),
        ] {
            assert_eq!(changed(&theme), today, "{name}'s mask changed colour");
            for depth in [
                crate::colour::Depth::Ansi256,
                crate::colour::Depth::Ansi16,
                crate::colour::Depth::None,
            ] {
                assert_eq!(
                    changed(&theme.resolve(depth)),
                    (0xf8, 0x51, 0x49),
                    "{name}'s mask changed colour at {depth:?}"
                );
            }
        }
    }

    #[test]
    fn mask_ink_key() {
        let changed = |source: &str| {
            let theme = crate::theme::parse(source).expect("theme parses");
            (Inks::of(&theme, None, false).changed, theme.removed_bar)
        };
        let dark = crate::Theme::dark().removed_bar;
        assert_eq!(
            changed("base = dark\npicture_changed = on #00ff00"),
            ((0x00, 0xff, 0x00), dark),
            "picture_changed did not move the mask alone"
        );
        assert_eq!(
            changed("base = dark\nremoved_bar = on #123456").0,
            (0x12, 0x34, 0x56),
            "an unset picture_changed did not follow the theme's removed_bar"
        );
        assert_eq!(
            changed("base = dark\nremoved_bar = on #123456\npicture_changed = on red").0,
            (0x12, 0x34, 0x56),
            "a named picture_changed hid the theme's removed_bar"
        );
        assert_eq!(
            changed("base = dark\npicture_changed = #00ff00").0,
            (0x00, 0xff, 0x00),
            "a foreground picture_changed was ignored"
        );
        let resolved = crate::theme::parse("base = dark\npicture_changed = on #00ff00")
            .expect("theme parses")
            .resolve(crate::colour::Depth::Ansi256);
        assert_eq!(
            Inks::of(&resolved, None, false).changed,
            (0x00, 0xff, 0x00),
            "picture_changed was dropped below truecolour"
        );
        let foreground = crate::theme::parse("base = dark\npicture_changed = #00ff00")
            .expect("theme parses")
            .resolve(crate::colour::Depth::None);
        assert_eq!(
            Inks::of(&foreground, None, false).changed,
            (0x00, 0xff, 0x00),
            "a foreground picture_changed was dropped without colour"
        );
    }

    #[test]
    fn mask_keeps_polarity() {
        let light = Inks {
            canvas: (255, 255, 255),
            changed: (0xcf, 0x22, 0x2e),
        };
        let image = RgbImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgb([255, 255, 255])
            } else {
                Rgb([0, 0, 0])
            }
        });
        let mask = painted(image, None, Paint::Mask, light);
        let (white, black) = (mask.get_pixel(0, 0).0[0], mask.get_pixel(1, 0).0[0]);
        assert!(
            white > black,
            "on a light pane the mask drew white as {white} and black as {black}"
        );
    }

    #[test]
    fn dim_and_mask_paints() {
        let inks = Inks {
            canvas: (0x0d, 0x11, 0x17),
            changed: (0xf8, 0x51, 0x49),
        };
        let image = RgbImage::from_pixel(2, 1, Rgb([200, 200, 200]));
        let dim = painted(image.clone(), None, Paint::Dim, inks);
        assert!(
            dim.get_pixel(0, 0).0.iter().all(|c| *c < 200),
            "a deleted picture was not dimmed toward the pane"
        );
        let hits = Hits {
            cells: vec![true, false],
            width: 2,
            height: 1,
        };
        let mask = painted(image, Some(&hits), Paint::Mask, inks);
        assert_eq!(
            mask.get_pixel(0, 0).0,
            [0xf8, 0x51, 0x49],
            "a changed pixel is not in the changed ink"
        );
        let quiet = mask.get_pixel(1, 0).0;
        assert!(
            quiet
                .iter()
                .zip([0x0d, 0x11, 0x17])
                .all(|(c, pane)| c.abs_diff(pane) <= 38),
            "an unchanged pixel of the mask is {quiet:?}, not near the pane"
        );
    }

    #[test]
    fn inflate_gate() {
        for (pairs, deflate) in [
            (
                &[
                    ("TERM_PROGRAM", "ghostty"),
                    ("TERM_PROGRAM_VERSION", "1.3.1"),
                ][..],
                false,
            ),
            (
                &[
                    ("TERM_PROGRAM", "ghostty"),
                    ("TERM_PROGRAM_VERSION", "1.3.0"),
                ][..],
                false,
            ),
            (
                &[
                    ("TERM_PROGRAM", "ghostty"),
                    ("TERM_PROGRAM_VERSION", "1.4.0"),
                ][..],
                true,
            ),
            (&[("TERM_PROGRAM", "ghostty")][..], false),
            (
                &[
                    ("TERM_PROGRAM", "WezTerm"),
                    ("TERM_PROGRAM_VERSION", "1.3.1"),
                ][..],
                true,
            ),
            (
                &[("TERM_PROGRAM", "tmux"), ("TERM_PROGRAM_VERSION", "3.5a")][..],
                true,
            ),
            (&[][..], true),
        ] {
            assert_eq!(
                inflates_safely(&env(pairs)),
                deflate,
                "{pairs:?} is sent o=z: {}",
                !deflate
            );
        }
    }

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    /// The override then the answer, composed as `run` composes them.
    fn chosen(
        answer: Answer,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<ProtocolType, PicturesError> {
        Ok(protocol_of(forced(&lookup)?, answer, &lookup))
    }

    const KNOWN: Answer = Answer {
        kitty: false,
        sixel: false,
        cell: true,
        heard: false,
    };

    #[test]
    fn override_wins() {
        let answer = Answer {
            kitty: true,
            ..KNOWN
        };
        for (value, want) in [
            (" Sixel ", ProtocolType::Sixel),
            ("halfblocks", ProtocolType::Halfblocks),
            ("iterm2", ProtocolType::Iterm2),
            ("KITTY", ProtocolType::Kitty),
        ] {
            assert_eq!(
                chosen(answer, env(&[(PICTURES_VAR, value)])),
                Ok(want),
                "{PICTURES_VAR}={value:?}"
            );
        }
        assert_eq!(
            chosen(answer, env(&[(PICTURES_VAR, "auto")])),
            Ok(ProtocolType::Kitty),
            "auto did not fall through to the answer"
        );
        assert_eq!(
            chosen(answer, env(&[(PICTURES_VAR, "  ")])),
            Ok(ProtocolType::Kitty),
            "a blank override was not taken as unset"
        );
    }

    #[test]
    fn override_refused() {
        assert_eq!(
            chosen(KNOWN, env(&[(PICTURES_VAR, "png")])),
            Err(PicturesError {
                value: "png".to_owned()
            })
        );
    }

    #[test]
    fn answer_wins() {
        let wezterm = env(&[("TERM_PROGRAM", "WezTerm")]);
        let kitty = Answer {
            kitty: true,
            sixel: true,
            cell: true,
            heard: true,
        };
        assert_eq!(chosen(kitty, &wezterm), Ok(ProtocolType::Kitty));
        let sixel = Answer {
            sixel: true,
            ..KNOWN
        };
        assert_eq!(chosen(sixel, &wezterm), Ok(ProtocolType::Sixel));
    }

    #[test]
    fn env_table() {
        for (pairs, want) in [
            (&[("TERM_PROGRAM", "WezTerm")][..], ProtocolType::Iterm2),
            (&[("TERM_PROGRAM", "iTerm.app")], ProtocolType::Iterm2),
            (&[("LC_TERMINAL", "iTerm2")], ProtocolType::Iterm2),
            (&[("TERM_PROGRAM", "ghostty")], ProtocolType::Kitty),
            (&[("TERM", "xterm-kitty")], ProtocolType::Kitty),
            (&[("TERM", "foot")], ProtocolType::Sixel),
            (&[("WT_SESSION", "abc")], ProtocolType::Sixel),
            (
                &[("TERM_PROGRAM", "WarpTerminal")],
                ProtocolType::Halfblocks,
            ),
            (&[("TERM_PROGRAM", "vscode")], ProtocolType::Halfblocks),
            (
                &[("TERM_PROGRAM", "Apple_Terminal")],
                ProtocolType::Halfblocks,
            ),
            (&[("TERM", "alacritty")], ProtocolType::Halfblocks),
            (
                &[("TERM", "screen-256color"), ("KITTY_WINDOW_ID", "1")],
                ProtocolType::Halfblocks,
            ),
        ] {
            assert_eq!(chosen(KNOWN, env(pairs)), Ok(want), "{pairs:?}");
        }
    }

    #[test]
    fn answer_denies_table() {
        let heard = Answer {
            heard: true,
            ..KNOWN
        };
        assert_eq!(
            chosen(heard, env(&[("WT_SESSION", "abc")])),
            Ok(ProtocolType::Halfblocks),
            "the table drew Sixel for a terminal that answered without it"
        );
        assert_eq!(
            chosen(heard, env(&[("TERM_PROGRAM", "WezTerm")])),
            Ok(ProtocolType::Iterm2),
            "an answer hid iTerm2 images, which no terminal reports"
        );
        assert_eq!(
            chosen(
                KNOWN,
                env(&[("WT_SESSION", "abc"), ("TERM_PROGRAM", "vscode")])
            ),
            Ok(ProtocolType::Halfblocks),
            "an inherited WT_SESSION drew Sixel in VS Code"
        );
    }

    #[test]
    fn tmux_outer_hint() {
        let stale_kitty = env(&[("TERM", "tmux-256color"), ("KITTY_WINDOW_ID", "3")]);
        assert_eq!(
            chosen(KNOWN, stale_kitty),
            Ok(ProtocolType::Halfblocks),
            "a kitty variable inside tmux was believed"
        );
        let iterm = env(&[("TERM_PROGRAM", "tmux"), ("ITERM_SESSION_ID", "w0")]);
        assert_eq!(chosen(KNOWN, iterm), Ok(ProtocolType::Iterm2));
    }

    #[test]
    fn unknown_cell_halfblocks() {
        let unknown = Answer {
            cell: false,
            ..KNOWN
        };
        assert_eq!(
            chosen(unknown, env(&[("TERM_PROGRAM", "WezTerm")])),
            Ok(ProtocolType::Halfblocks),
            "a protocol was picked with no cell size to fit it to"
        );
    }

    #[test]
    fn protocol_any_depth() {
        assert!(drawn(true, ProtocolType::Halfblocks));
        assert!(!drawn(false, ProtocolType::Halfblocks));
        for protocol in [
            ProtocolType::Kitty,
            ProtocolType::Sixel,
            ProtocolType::Iterm2,
        ] {
            assert!(
                drawn(false, protocol),
                "{protocol:?} was not drawn below truecolour"
            );
        }
    }

    #[test]
    fn protocol_pixels() {
        let mut picker = Picker::halfblocks();
        let slot = Slot {
            x: 0,
            width: 10,
            rows: 6,
        };
        assert_eq!(pixels_of(slot, (100, 200), &picker), (10, 12));
        picker.set_protocol_type(ProtocolType::Kitty);
        // 100x120 pixels of room at 10x20 cells; a 1:2 image is fitted by height.
        assert_eq!(pixels_of(slot, (100, 200), &picker), (60, 120));
        let tall = Slot { rows: 20, ..slot };
        assert_eq!(
            pixels_of(tall, (100, 300), &picker),
            (100, 300),
            "a width-bound panel was cut to its width in height"
        );
        assert_eq!(
            pixels_of(slot, (4, 4), &picker),
            (100, 100),
            "a small image was not fitted up to its slot"
        );
    }

    #[test]
    fn hits_span_cells() {
        let hits = Hits {
            cells: vec![false, true, false, false],
            width: 2,
            height: 2,
        };
        let (width, height) = (20u32, 40u32);
        let lit: Vec<(u32, u32)> = (0..width * height)
            .filter(|i| hits.at(*i as usize, (width, height)))
            .map(|i| (i % width, i / width))
            .collect();
        assert_eq!(lit.len(), 200, "one changed cell lit {} pixels", lit.len());
        assert!(
            lit.iter().all(|(x, y)| *x >= 10 && *y < 20),
            "a lit pixel is outside its cell"
        );
    }

    #[test]
    fn suffix_words() {
        let built = |verdict, profiles_differ| Built {
            panels: Vec::new(),
            verdict,
            profiles_differ,
            kitty: Vec::new(),
            stored: 0,
            page_note: None,
            sizes: Vec::new(),
            bar: None,
        };
        assert_eq!(
            built(
                Verdict::Changed {
                    pixels: 1409,
                    total: 3_338_496
                },
                false
            )
            .suffix()
            .as_deref(),
            Some("1409 px changed (0.04%)")
        );
        assert_eq!(
            built(Verdict::Unchanged, false).suffix().as_deref(),
            Some("pixels unchanged")
        );
        assert_eq!(
            built(Verdict::Unchanged, true).suffix().as_deref(),
            Some("colour profile changed")
        );
        assert_eq!(built(Verdict::Uncompared, false).suffix(), None);
    }
}
