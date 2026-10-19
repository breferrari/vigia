//! The worker's half of a changed PDF: which page to draw, and drawing it.

use super::*;

/// Only the first page's size of each side, within [`PDF_DECODED`].
pub(super) fn sized(job: &Job) -> Built {
    hayro::hayro_syntax::with_decode_limit(PDF_DECODED, || sized_within(job))
        .unwrap_or_else(|_| undecoded())
}

/// Both sides' pictures of the first changed page, within [`PDF_DECODED`]:
/// every stream the parse, fingerprint and render decode is held whole, so
/// they share one limit.
pub(super) fn drawn(job: &Job) -> Option<PdfDrawn> {
    hayro::hayro_syntax::with_decode_limit(PDF_DECODED, || drawn_within(job))
        .ok()
        .flatten()
}

/// What one PDF page may cost, in bytes of content stream times pixels on
/// the long side: 1.4 MiB at 1024 pixels. Measured, a page from 3 to 20 MB
/// of content takes 0.29 to 0.51 seconds on the worker.
const PDF_BUDGET: f64 = 1.4 * 1024.0 * 1024.0 * 1024.0;
/// The most objects one page may reach before it is refused: the walk that
/// fingerprints it reads each.
const PDF_OBJECTS: usize = 100_000;
/// The most stream data the worker lets one PDF decode, its two sides
/// together, while it sizes, fingerprints and draws them.
const PDF_DECODED: u64 = 256 * 1024 * 1024;

/// One PDF side, opened.
fn open_pdf(source: &Option<Source>) -> Option<Option<hayro::hayro_syntax::Pdf>> {
    match source {
        Some(source) => {
            let bytes = read_counted(source)?;
            hayro::hayro_syntax::Pdf::new(std::sync::Arc::new(bytes))
                .ok()
                .map(Some)
        }
        None => Some(None),
    }
}

/// What walking a page's objects meets, in order.
enum Met<'a, 'b> {
    Name(&'b [u8]),
    /// An object met before, by its number.
    Again(i32, i32),
    Leaf(&'b hayro::hayro_syntax::object::Object<'a>),
    Stream(&'b hayro::hayro_syntax::object::Stream<'a>),
}

/// Every object `value` reaches, each once, but the page tree's parent
/// link. `None` when `meet` says so, or once `seen` holds `cap` objects. A
/// stack of its own, not the thread's: a chain of references can be as long
/// as a file.
fn walk<'a>(
    value: hayro::hayro_syntax::object::MaybeRef<hayro::hayro_syntax::object::Object<'a>>,
    xref: &'a hayro::hayro_syntax::xref::XRef,
    seen: &mut HashSet<(i32, i32)>,
    cap: usize,
    meet: &mut dyn FnMut(Met<'a, '_>) -> Option<()>,
) -> Option<()> {
    use hayro::hayro_syntax::object::{MaybeRef, Name, Object};
    enum Task<'a> {
        Object(Object<'a>),
        Value(MaybeRef<Object<'a>>),
        Name(Name<'a>),
    }
    let mut tasks = vec![Task::Value(value)];
    while let Some(task) = tasks.pop() {
        let object = match task {
            Task::Name(name) => {
                meet(Met::Name(name.as_ref()))?;
                continue;
            }
            Task::Value(MaybeRef::NotRef(object)) | Task::Object(object) => object,
            Task::Value(MaybeRef::Ref(reference)) => {
                let id = (reference.obj_number, reference.gen_number);
                if !seen.insert(id) {
                    meet(Met::Again(id.0, id.1))?;
                    continue;
                }
                // Past it, a change could go unread, so the page is refused.
                if seen.len() > cap {
                    return None;
                }
                match xref.get::<Object<'a>>(reference.into()) {
                    Some(object) => object,
                    None => continue,
                }
            }
        };
        match object {
            Object::Dict(dict) => {
                let entries: Vec<_> = dict
                    .entries()
                    // The page tree, which every page reaches and none owns.
                    .filter(|(name, _)| name.as_ref() != b"Parent")
                    .collect();
                for (name, value) in entries.into_iter().rev() {
                    tasks.push(Task::Value(value));
                    tasks.push(Task::Name(name));
                }
            }
            Object::Array(array) => {
                let values: Vec<_> = array.raw_iter().collect();
                tasks.extend(values.into_iter().rev().map(Task::Value));
            }
            Object::Stream(stream) => {
                meet(Met::Stream(&stream))?;
                tasks.push(Task::Object(Object::Dict(stream.dict().clone())));
            }
            other => meet(Met::Leaf(&other))?,
        }
    }
    Some(())
}

/// What a page draws from, each under its name: its contents and its
/// resources. Resources it inherits are shared with other pages, so of those
/// only the ones its content names count, and a change to one another page
/// draws does not; reading the names decodes the content.
fn page_roots<'a>(
    page: &hayro::hayro_syntax::page::Page<'a>,
) -> Vec<(
    &'static [u8],
    hayro::hayro_syntax::object::MaybeRef<hayro::hayro_syntax::object::Object<'a>>,
)> {
    use hayro::hayro_syntax::object::{MaybeRef, Object};
    let resources = page.resources();
    let named = (!page.raw().contains_key(b"Resources")).then(|| {
        let mut named: HashSet<Vec<u8>> = HashSet::new();
        let mut operations = page.operations();
        while let Some(operation) = operations.next() {
            for operand in operation.operands() {
                if let Object::Name(name) = operand {
                    named.insert(name.as_ref().to_vec());
                }
            }
        }
        named
    });
    let mut roots: Vec<_> = page
        .raw()
        .get::<Object<'a>>(b"Contents")
        .map(|contents| (b"Contents".as_slice(), MaybeRef::NotRef(contents)))
        .into_iter()
        .collect();
    for (key, dict) in [
        (b"ExtGState".as_slice(), &resources.ext_g_states),
        (b"Font", &resources.fonts),
        (b"Properties", &resources.properties),
        (b"ColorSpace", &resources.color_spaces),
        (b"XObject", &resources.x_objects),
        (b"Pattern", &resources.patterns),
        (b"Shading", &resources.shadings),
    ] {
        for (name, value) in dict.entries() {
            if named
                .as_ref()
                .is_none_or(|named| named.contains(name.as_ref()))
            {
                roots.push((key, MaybeRef::NotRef(Object::Name(name))));
                roots.push((key, value));
            }
        }
    }
    roots
}

/// An image's width and height as its dictionary claims them, when it is one.
fn image_size(stream: &hayro::hayro_syntax::object::Stream<'_>) -> Option<(u32, u32)> {
    let dict = stream.dict();
    dict.get::<hayro::hayro_syntax::object::Name<'_>>(b"Subtype")
        .is_some_and(|subtype| subtype.as_ref() == b"Image")
        .then(|| {
            let side = |key: &[u8]| dict.get::<u32>(key).unwrap_or(0);
            (side(b"Width"), side(b"Height"))
        })
}

/// A page's fingerprint, from its boxes, its rotation and the encoded bytes
/// of every object it draws from, or `None` when an image it holds is larger
/// than any image may decode, or it reaches more than [`PDF_OBJECTS`] new
/// objects. Its content is decoded only when its resources are inherited. An
/// object an earlier page reached, in `seen`, counts by its number alone.
fn page_print(
    page: &hayro::hayro_syntax::page::Page<'_>,
    seen: &mut HashSet<(i32, i32)>,
) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!(
        "{:?} {:?} {:?}",
        page.media_box(),
        page.crop_box(),
        page.rotation()
    )
    .hash(&mut hasher);
    let cap = seen.len() + PDF_OBJECTS;
    for (key, root) in page_roots(page) {
        key.hash(&mut hasher);
        walk(root, page.xref(), seen, cap, &mut |met| {
            match met {
                Met::Name(name) => name.hash(&mut hasher),
                Met::Again(number, generation) => (number, generation).hash(&mut hasher),
                Met::Leaf(leaf) => {
                    // Its text, written into the hash without a string between.
                    struct Hashed<'h>(&'h mut std::collections::hash_map::DefaultHasher);
                    impl std::fmt::Write for Hashed<'_> {
                        fn write_str(&mut self, text: &str) -> std::fmt::Result {
                            self.0.write(text.as_bytes());
                            Ok(())
                        }
                    }
                    let _ = std::fmt::Write::write_fmt(
                        &mut Hashed(&mut hasher),
                        format_args!("{leaf}"),
                    );
                }
                Met::Stream(stream) => {
                    if let Some((width, height)) = image_size(stream)
                        && !decodable(width, height)
                    {
                        return None;
                    }
                    stream.raw_data().hash(&mut hasher);
                }
            }
            Some(())
        })?;
    }
    Some(hasher.finish())
}

/// Which page of a changed PDF is drawn.
#[derive(Clone, Copy)]
enum Chosen {
    /// The same page on both sides.
    Both(usize),
    /// The first page the new side added, alone.
    Added(usize),
    /// The first page the new side removed, alone.
    Removed(usize),
}

/// Which page both sides draw, and what the line says about it, from the
/// first page whose fingerprint differs; `None` when a page reached before it
/// is refused. Pages past that one are not read.
fn chosen_page(
    old: Option<&hayro::hayro_syntax::Pdf>,
    new: Option<&hayro::hayro_syntax::Pdf>,
) -> Option<(Chosen, Option<String>)> {
    let (Some(old), Some(new)) = (old, new) else {
        return Some((Chosen::Both(0), None));
    };
    let (old, new) = (old.pages(), new.pages());
    let (mut old_seen, mut new_seen) = (HashSet::new(), HashSet::new());
    let mut differs = None;
    for (page, (a, b)) in old.iter().zip(new.iter()).enumerate() {
        if page_print(a, &mut old_seen)? != page_print(b, &mut new_seen)? {
            differs = Some(page);
            break;
        }
    }
    let count = (old.len() != new.len()).then(|| format!("{} → {} pages", old.len(), new.len()));
    let (chosen, said) = match differs {
        Some(page) => (
            Chosen::Both(page),
            (page > 0).then(|| format!("page {} of {}", page + 1, new.len())),
        ),
        None if new.len() > old.len() => (
            Chosen::Added(old.len()),
            Some(format!("page {} of {} added", old.len() + 1, new.len())),
        ),
        None if new.len() < old.len() => (
            Chosen::Removed(new.len()),
            Some(format!("page {} of {} removed", new.len() + 1, old.len())),
        ),
        None => (Chosen::Both(0), Some("no page changed".to_owned())),
    };
    let note: Vec<String> = [said, count].into_iter().flatten().collect();
    Some((chosen, (!note.is_empty()).then(|| note.join(" · "))))
}

/// Only the first page's size of each side, in points rounded up, for a
/// picture the frame path left unsized. Nothing is rendered.
fn sized_within(job: &Job) -> Built {
    let size = |source: &Option<Source>, stamp: Option<u64>| -> Option<Option<(u64, u32, u32)>> {
        let Some(pdf) = open_pdf(source)? else {
            return Some(None);
        };
        let (width, height) = pdf.pages().first()?.render_dimensions();
        let side = |length: f32| (length.is_finite() && length > 0.0).then(|| length.ceil() as u32);
        Some(Some((stamp?, side(width)?, side(height)?)))
    };
    // Every side there is must size, or the picture cannot be drawn.
    let (Some(old), Some(new)) = (
        size(&job.sources.before, job.key.before),
        size(&job.sources.after, job.key.after),
    ) else {
        return undecoded();
    };
    Built {
        verdict: Verdict::Uncompared,
        sizes: [old, new].into_iter().flatten().collect(),
        ..undecoded()
    }
}

/// Each side of a changed PDF drawn at its first changed page, fitted inside
/// its first page's shape, both at one long side; with what the line says
/// about the page.
fn drawn_within(job: &Job) -> Option<PdfDrawn> {
    let (old, new) = (
        open_pdf(&job.sources.before)?,
        open_pdf(&job.sources.after)?,
    );
    let (chosen, note) = chosen_page(old.as_ref(), new.as_ref())?;
    let index = match chosen {
        Chosen::Both(page) | Chosen::Added(page) | Chosen::Removed(page) => page,
    };
    // A page one side added or removed is drawn on that side alone.
    let (old, new, alone) = match chosen {
        Chosen::Both(_) => (old, new, None),
        Chosen::Added(_) => (None, new, Some(Bar::Added)),
        Chosen::Removed(_) => (old, None, Some(Bar::Removed)),
    };
    // Past this length a page needs a long side under the floor.
    let refused = (PDF_BUDGET / f64::from(SHORTEST_SIDE)) as usize;
    let mut cost = 0;
    for pdf in [&old, &new].into_iter().flatten() {
        let pages = pdf.pages();
        let Some(page) = pages.get(index).or_else(|| pages.first()) else {
            continue;
        };
        // Decoded once: the render reads the same page's copy.
        let length = page.page_stream().map_or(0, <[u8]>::len);
        if length > refused {
            return None;
        }
        cost = cost.max(length);
    }
    let long = (PDF_BUDGET / cost.max(1) as f64).min(f64::from(LONG_SIDE)) as f32;
    if long < SHORTEST_SIDE {
        return None;
    }
    let render = |pdf: Option<hayro::hayro_syntax::Pdf>| -> Option<Option<Decoded>> {
        let Some(pdf) = pdf else {
            return Some(None);
        };
        let pages = pdf.pages();
        let first = pages.first()?;
        let page = pages.get(index).unwrap_or(first);
        Some(Some((render_page(first, page, long)?, None)))
    };
    Some(PdfDrawn {
        old: render(old)?,
        new: render(new)?,
        note,
        alone,
    })
}

/// `page` rendered and centred in a canvas of `first`'s shape, its long side
/// at `long`, so it fills the rows the first page reserved.
fn render_page(
    first: &hayro::hayro_syntax::page::Page<'_>,
    page: &hayro::hayro_syntax::page::Page<'_>,
    long: f32,
) -> Option<RgbaImage> {
    use hayro::hayro_interpret::InterpreterSettings;
    let (width, height) = first.render_dimensions();
    let canvas_scale = long / width.max(height);
    let canvas = (
        (width * canvas_scale).ceil().max(1.0) as u32,
        (height * canvas_scale).ceil().max(1.0) as u32,
    );
    let (page_width, page_height) = page.render_dimensions();
    let scale = (canvas.0 as f32 / page_width).min(canvas.1 as f32 / page_height);
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let pixmap = hayro::render(
        page,
        &hayro::RenderCache::new(),
        &InterpreterSettings::default(),
        &hayro::RenderSettings::default(),
        &hayro::PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            ..Default::default()
        },
    );
    let (drawn_width, drawn_height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    let drawn = RgbaImage::from_raw(
        drawn_width,
        drawn_height,
        pixmap.take_rgba8(hayro::vello_cpu::peniko::ImageAlphaType::Alpha),
    )?;
    let mut fitted = RgbaImage::new(canvas.0, canvas.1);
    image::imageops::overlay(
        &mut fitted,
        &drawn,
        i64::from(canvas.0.saturating_sub(drawn_width) / 2),
        i64::from(canvas.1.saturating_sub(drawn_height) / 2),
    );
    Some(fitted)
}
