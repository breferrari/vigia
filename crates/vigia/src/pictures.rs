//! The pictures of a changed image: where they go on the pane, and how they are
//! built off the paint and kept while they are on screen. `SPEC.md` §11.1.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

use image::{DynamicImage, ImageFormat, Rgb, RgbImage, RgbaImage};
use ratatui::layout::Size;
use ratatui_image::protocol::halfblocks::Halfblocks;
use ratatui_image::sliced::SlicedProtocol;
use vigia_core::{Format, ImageSide, Picture, Source, Sources};

use crate::view::Row;

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

/// A picture built and encoded, ready to draw.
pub struct Built {
    /// Each panel's place and its encoded cells.
    pub panels: Vec<(Slot, SlicedProtocol)>,
    /// What the comparison found.
    pub verdict: Verdict,
    /// Whether the two sides carry different colour profiles.
    pub profiles_differ: bool,
}

impl Built {
    /// What the size line adds once the comparison is done.
    pub fn suffix(&self) -> Option<String> {
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

/// One picture to build.
struct Job {
    key: Key,
    sources: Sources,
    formats: (Option<Format>, Option<Format>),
}

/// Built pictures, kept while their image is on screen, and the worker that
/// builds them.
pub struct Pictures {
    built: HashMap<Key, Ready>,
    pending: HashSet<Key>,
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<(Key, Option<Built>)>,
}

/// Pictures decoded since the process started, on any thread.
static DECODES: AtomicU64 = AtomicU64::new(0);
/// Of those, the ones decoded anywhere but the worker, which is a paint paying
/// for one.
static OFF_WORKER: AtomicU64 = AtomicU64::new(0);
/// The worker thread's name, which is how a decode knows where it runs.
const WORKER: &str = "vigia-pictures";

/// How many images have been decoded so far.
pub fn decodes() -> u64 {
    DECODES.load(Ordering::Relaxed)
}

/// How many images have been decoded on a thread that is not the worker.
pub fn decodes_off_worker() -> u64 {
    OFF_WORKER.load(Ordering::Relaxed)
}

impl Pictures {
    /// A cache whose worker builds in `inks` and calls `wake` after each picture.
    pub fn new(inks: Inks, wake: Box<dyn Fn() + Send>) -> Self {
        let (jobs, inbox) = mpsc::channel::<Job>();
        let (outbox, done) = mpsc::channel();
        // Detached: it ends when the cache, and with it the sender, is dropped.
        let _ = std::thread::Builder::new()
            .name(WORKER.to_owned())
            .spawn(move || {
                for job in inbox {
                    let built = build(&job, inks);
                    if outbox.send((job.key, built)).is_err() {
                        break;
                    }
                    wake();
                }
            });
        Self {
            built: HashMap::new(),
            pending: HashSet::new(),
            jobs,
            done,
        }
    }

    /// Take in what the worker finished since the last call.
    pub fn collect(&mut self) {
        while let Ok((key, built)) = self.done.try_recv() {
            self.pending.remove(&key);
            let built = built.unwrap_or(Built {
                panels: Vec::new(),
                verdict: Verdict::Undecoded,
                profiles_differ: false,
            });
            self.built.insert(key, Ready(Arc::new(built)));
        }
    }

    /// Drop every picture whose key is not in `shown`.
    pub fn keep(&mut self, shown: &HashSet<Key>) {
        self.built.retain(|key, _| shown.contains(key));
    }

    /// Whether `key` is neither built, being built, nor known to fail.
    pub fn wants(&self, key: &Key) -> bool {
        !self.built.contains_key(key) && !self.pending.contains(key)
    }

    /// Build `key` from `sources` on the worker.
    pub fn queue(&mut self, key: Key, sources: Sources, picture: &Picture) {
        let formats = (
            picture.before.map(|side| side.format),
            picture.after.map(|side| side.format),
        );
        self.pending.insert(key.clone());
        let _ = self.jobs.send(Job {
            key,
            sources,
            formats,
        });
    }

    /// The built picture for `key`, if it is ready.
    pub fn get(&self, key: &Key) -> Option<Ready> {
        self.built.get(key).cloned()
    }

    /// How many built pictures are kept.
    pub fn kept(&self) -> usize {
        self.built.len()
    }

    /// Take in what the worker built, drop what is no longer on screen, queue
    /// what is on screen and not yet built, and hand the built pictures to their
    /// rows. Only images on screen are ever read past their header.
    pub fn attach(&mut self, rows: &mut [Row], frame: &vigia_core::Frame<'_>) {
        self.collect();
        let shown: HashSet<Key> = rows
            .iter()
            .filter_map(|row| match row {
                Row::Sized { key, .. } | Row::Picture { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();
        self.keep(&shown);
        for row in rows.iter() {
            if let Row::Sized {
                key, file, picture, ..
            }
            | Row::Picture {
                key, file, picture, ..
            } = row
                && self.wants(key)
                && let Ok(sources) = frame.image_sources(*file)
            {
                self.queue(key.clone(), sources, picture);
            }
        }
        for row in rows.iter_mut() {
            if let Row::Sized { key, ready, .. } | Row::Picture { key, ready, .. } = row {
                *ready = self.get(key);
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
    /// take a wake. Whether nothing is still being built.
    pub fn settle(&mut self, limit: std::time::Duration) -> bool {
        let until = std::time::Instant::now() + limit;
        loop {
            self.collect();
            if self.pending.is_empty() {
                return true;
            }
            if std::time::Instant::now() >= until {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

/// The most an image may be, in pixels on a side, before its decode is refused.
const MAX_SIDE: u32 = 16_384;
/// The most one decode may allocate.
const MAX_ALLOC: u64 = 512 * 1024 * 1024;

fn image_format(format: Format) -> ImageFormat {
    match format {
        Format::Png => ImageFormat::Png,
        Format::Jpeg => ImageFormat::Jpeg,
        Format::Gif => ImageFormat::Gif,
        Format::Webp => ImageFormat::WebP,
        Format::Bmp => ImageFormat::Bmp,
        Format::Ico => ImageFormat::Ico,
        Format::Tiff => ImageFormat::Tiff,
        Format::Qoi => ImageFormat::Qoi,
        Format::Tga => ImageFormat::Tga,
    }
}

/// One side decoded: its pixels and its colour profile. The first frame only.
fn decode(source: &Source, format: Format) -> Option<(RgbaImage, Option<Vec<u8>>)> {
    use image::ImageDecoder;
    let bytes = match source {
        Source::Bytes(bytes) => std::borrow::Cow::Borrowed(bytes.as_slice()),
        Source::File(path) => std::borrow::Cow::Owned(std::fs::read(path).ok()?),
    };
    DECODES.fetch_add(1, Ordering::Relaxed);
    if std::thread::current().name() != Some(WORKER) {
        OFF_WORKER.fetch_add(1, Ordering::Relaxed);
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes.as_ref()));
    reader.set_format(image_format(format));
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let profile = decoder.icc_profile().ok().flatten();
    let image = DynamicImage::from_decoder(decoder).ok()?;
    Some((image.to_rgba8(), profile))
}

/// `image` over the pane, so a transparent pixel shows what is behind it.
fn flatten(image: &RgbaImage, canvas: (u8, u8, u8)) -> RgbImage {
    let (r, g, b) = canvas;
    RgbImage::from_fn(image.width(), image.height(), |x, y| {
        let [pr, pg, pb, alpha] = image.get_pixel(x, y).0;
        let over = |p: u8, c: u8| {
            let a = u16::from(alpha);
            ((u16::from(p) * a + u16::from(c) * (255 - a)) / 255) as u8
        };
        Rgb([over(pr, r), over(pg, g), over(pb, b)])
    })
}

/// pixelmatch's colour distance, squared, in YIQ; 35215 is its largest.
fn distance(a: Rgb<u8>, b: Rgb<u8>) -> f32 {
    let yiq = |p: Rgb<u8>| {
        let [r, g, b] = p.0.map(f32::from);
        (
            r * 0.298_895 + g * 0.586_622 + b * 0.114_482,
            r * 0.595_978 - g * 0.274_176 - b * 0.321_802,
            r * 0.211_470 - g * 0.522_617 + b * 0.311_147,
        )
    };
    let ((ya, ia, qa), (yb, ib, qb)) = (yiq(a), yiq(b));
    0.5053 * (ya - yb).powi(2) + 0.299 * (ia - ib).powi(2) + 0.1957 * (qa - qb).powi(2)
}

/// pixelmatch's default threshold of 0.1, on the squared distance.
const THRESHOLD: f32 = 0.1 * 0.1 * 35_215.0;

/// Which pixels differ between two images of one size.
fn changed(old: &RgbImage, new: &RgbImage) -> Vec<bool> {
    old.pixels()
        .zip(new.pixels())
        .map(|(a, b)| distance(*a, *b) > THRESHOLD)
        .collect()
}

/// `image` shrunk to `width` by `height` pixels, each one the average of the
/// pixels under it, with `mask` marking a pixel when any pixel under it is set.
fn pool(image: &RgbImage, mask: Option<&[bool]>, width: u32, height: u32) -> (RgbImage, Vec<bool>) {
    let (iw, ih) = image.dimensions();
    let mut out = RgbImage::new(width, height);
    let mut hit = vec![false; (width * height) as usize];
    for gy in 0..height {
        let (y0, y1) = (
            gy * ih / height,
            ((gy + 1) * ih / height).max(gy * ih / height + 1).min(ih),
        );
        for gx in 0..width {
            let (x0, x1) = (
                gx * iw / width,
                ((gx + 1) * iw / width).max(gx * iw / width + 1).min(iw),
            );
            let mut sum = [0u64; 3];
            let mut any = false;
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = image.get_pixel(x, y).0;
                    for c in 0..3 {
                        sum[c] += u64::from(p[c]);
                    }
                    if let Some(mask) = mask {
                        any |= mask[(y * iw + x) as usize];
                    }
                }
            }
            let n = u64::from((y1 - y0) * (x1 - x0)).max(1);
            out.put_pixel(gx, gy, Rgb(sum.map(|s| (s / n) as u8)));
            hit[(gy * width + gx) as usize] = any;
        }
    }
    (out, hit)
}

/// How a panel's pixels are painted from the pooled picture.
#[derive(Clone, Copy)]
enum Paint {
    /// As they are.
    Plain,
    /// Toward the pane, for a picture that is gone.
    Dim,
    /// Grey toward the pane, with a changed pixel in the changed ink.
    Mask,
}

/// One panel: `image` fitted into `slot` and painted, encoded as half-blocks.
fn panel(
    image: &RgbImage,
    mask: Option<&[bool]>,
    slot: Slot,
    paint: Paint,
    inks: Inks,
) -> Option<SlicedProtocol> {
    let (width, height) = (u32::from(slot.width), u32::from(slot.rows) * 2);
    let (mut out, hit) = pool(image, mask, width, height);
    let (cr, cg, cb) = inks.canvas;
    let canvas = [cr, cg, cb].map(f32::from);
    for (i, pixel) in out.pixels_mut().enumerate() {
        let p = pixel.0.map(f32::from);
        pixel.0 = match paint {
            Paint::Plain => pixel.0,
            Paint::Dim => [0, 1, 2].map(|c| (p[c] * 0.45 + canvas[c] * 0.55) as u8),
            Paint::Mask if hit[i] => {
                let (r, g, b) = inks.changed;
                [r, g, b]
            }
            Paint::Mask => {
                let light = (p[0] * 0.299 + p[1] * 0.587 + p[2] * 0.114) / 255.0;
                // Toward the pane and away from it by its lightness, hard enough that
                // a light screen does not become a slab beside the changed ink.
                let toward = if canvas.iter().sum::<f32>() > 382.0 {
                    -38.0
                } else {
                    38.0
                };
                canvas.map(|c| (c + light * toward).clamp(0.0, 255.0) as u8)
            }
        };
    }
    let halfblocks = Halfblocks::new(
        DynamicImage::ImageRgb8(out),
        Size::new(slot.width, slot.rows),
    )
    .ok()?;
    Some(SlicedProtocol::Halfblocks(halfblocks))
}

/// Build one picture: read and decode both sides, compare them, and encode each
/// panel. `None` when a side does not decode.
fn build(job: &Job, inks: Inks) -> Option<Built> {
    let side = |source: &Option<Source>, format: Option<Format>| match (source, format) {
        (Some(source), Some(format)) => decode(source, format).map(Some),
        (None, None) => Some(None),
        _ => None,
    };
    let old = side(&job.sources.before, job.formats.0)?;
    let new = side(&job.sources.after, job.formats.1)?;
    let profiles_differ = matches!((&old, &new), (Some((_, a)), Some((_, b))) if a != b);
    let old = old.map(|(image, _)| flatten(&image, inks.canvas));
    let new = new.map(|(image, _)| flatten(&image, inks.canvas));
    let mask = match (&old, &new) {
        (Some(old), Some(new)) if old.dimensions() == new.dimensions() => Some(changed(old, new)),
        _ => None,
    };
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
    let shape = &job.key.shape;
    let paints: Vec<(&RgbImage, Paint)> = match shape.kind {
        Kind::Three => vec![
            (old.as_ref()?, Paint::Plain),
            (new.as_ref()?, Paint::Plain),
            (new.as_ref()?, Paint::Mask),
        ],
        Kind::Mask => vec![(new.as_ref()?, Paint::Mask)],
        Kind::Pair => vec![(old.as_ref()?, Paint::Plain), (new.as_ref()?, Paint::Plain)],
        Kind::New | Kind::Added => vec![(new.as_ref()?, Paint::Plain)],
        Kind::Deleted => vec![(old.as_ref()?, Paint::Dim)],
    };
    let panels = shape
        .slots
        .iter()
        .zip(paints)
        .map(|(&slot, (image, paint))| {
            let mask = matches!(paint, Paint::Mask)
                .then_some(())
                .and(mask.as_deref());
            panel(image, mask, slot, paint, inks).map(|protocol| (slot, protocol))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Built {
        panels,
        verdict,
        profiles_differ,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let old = RgbImage::from_pixel(1000, 1000, Rgb([255, 255, 255]));
        let mut new = old.clone();
        new.put_pixel(503, 497, Rgb([0, 0, 0]));
        let mask = changed(&old, &new);
        let (_, hit) = pool(&new, Some(&mask), 20, 20);
        assert_eq!(
            hit.iter().filter(|hit| **hit).count(),
            1,
            "one changed pixel lit no cell, or more than one"
        );
    }

    #[test]
    fn suffix_words() {
        let built = |verdict, profiles_differ| Built {
            panels: Vec::new(),
            verdict,
            profiles_differ,
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
