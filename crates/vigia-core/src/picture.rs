//! What a changed image is on each side: its format, its size and its bytes.

use std::fmt;
use std::io::{BufRead, Read, Seek};

/// The image formats the pane reads. Any other is a binary file like the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Portable Network Graphics.
    Png,
    /// JPEG.
    Jpeg,
    /// GIF.
    Gif,
    /// WebP.
    Webp,
    /// Windows bitmap.
    Bmp,
    /// Windows icon.
    Ico,
    /// TIFF.
    Tiff,
    /// Quite OK Image.
    Qoi,
    /// Truevision TGA, the one format with no signature, so it is known by its
    /// `.tga` name and then by its header.
    Tga,
    /// An SVG document, known by its `.svg` name and its root `<svg>` tag.
    Svg,
    /// A PDF document, known by its `.pdf` name and its `%PDF-` header, and
    /// sized in points.
    Pdf,
}

impl Format {
    fn of(format: image::ImageFormat) -> Option<Self> {
        use image::ImageFormat as F;
        Some(match format {
            F::Png => Self::Png,
            F::Jpeg => Self::Jpeg,
            F::Gif => Self::Gif,
            F::WebP => Self::Webp,
            F::Bmp => Self::Bmp,
            F::Ico => Self::Ico,
            F::Tiff => Self::Tiff,
            F::Qoi => Self::Qoi,
            F::Tga => Self::Tga,
            _ => return None,
        })
    }

    /// The format a path's name stands for when its bytes cannot say: TGA alone.
    pub(crate) fn named(path: &str) -> Option<Self> {
        let (_, extension) = path.rsplit_once('.')?;
        extension.eq_ignore_ascii_case("tga").then_some(Self::Tga)
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::Webp => "WebP",
            Self::Bmp => "BMP",
            Self::Ico => "ICO",
            Self::Tiff => "TIFF",
            Self::Qoi => "QOI",
            Self::Tga => "TGA",
            Self::Svg => "SVG",
            Self::Pdf => "PDF",
        })
    }
}

/// One side of a changed image, read from its header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSide {
    /// What the first bytes say it is.
    pub format: Format,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The whole file's length, read or not.
    pub bytes: u64,
    /// What this side's content is, as far as the pane can tell without hashing
    /// it: its blob id, or its file's length, time and inode. Two sides with one
    /// stamp hold the same image.
    pub stamp: u64,
}

impl ImageSide {
    /// Whether its size is known: a PDF side the frame path left to the
    /// worker is not, until the worker has sized it.
    pub fn sized(&self) -> bool {
        self.width > 0
    }
}

/// Both sides of a changed image. A side is `None` where the change has none:
/// an added image has no old side and a deleted one no new side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picture {
    /// The index side, or `HEAD`'s in the staged run.
    pub before: Option<ImageSide>,
    /// The worktree side, or the index's in the staged run.
    pub after: Option<ImageSide>,
}

/// The most of a PDF the frame path reads, whole, for its size. A PDF's index
/// is at its end, so it is read whole or not at all.
pub(crate) const PDF_LIMIT: u64 = 16 * 1024 * 1024;

/// The most stream data the frame path lets one PDF decode while it sizes
/// it.
#[cfg(feature = "pdf")]
const PDF_DECODED_HERE: u64 = 16 * 1024 * 1024;

/// The largest PDF the frame path reads and sizes itself. A larger one is
/// sized on the worker, and its line names only its bytes until then.
pub(crate) const PDF_SIZED_HERE: u64 = 1024 * 1024;

/// A PDF side the worker sizes: its bytes known, its size not yet.
pub(crate) fn pdf_unsized(bytes: u64) -> ImageSide {
    ImageSide {
        format: Format::Pdf,
        width: 0,
        height: 0,
        bytes,
        stamp: 0,
    }
}

/// How far into a file a PDF's `%PDF-` may start.
pub(crate) const PDF_HEAD: usize = 1024;

/// Whether `head`, a file's first bytes, starts a PDF.
pub(crate) fn pdf_headed(head: &[u8]) -> bool {
    head[..head.len().min(PDF_HEAD)]
        .array_windows()
        .any(|w| w == b"%PDF-")
}

/// Whether `path` is named as a PDF, in a build that draws PDFs.
pub(crate) fn pdf_named(path: &str) -> bool {
    cfg!(feature = "pdf") && pdf_path(path)
}

/// Whether `path` is named as a PDF, whether or not this build draws one.
pub(crate) fn pdf_path(path: &str) -> bool {
    named(path, "pdf")
}

/// Whether `path` ends in `.extension`, in any case.
fn named(path: &str, extension: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, found)| found.eq_ignore_ascii_case(extension))
}

/// One PDF side, held whole: its first page's size in points, or `None`
/// for one that is not a PDF, needs a password, or does not parse.
pub(crate) fn pdf_side(bytes: Vec<u8>) -> Option<ImageSide> {
    let length = bytes.len() as u64;
    if length > PDF_LIMIT || !pdf_headed(&bytes) {
        return None;
    }
    // A document an agent wrote must not be able to close the pane.
    let (width, height) = crate::caught(|| pdf_size(bytes)).flatten()?;
    Some(ImageSide {
        format: Format::Pdf,
        width,
        height,
        bytes: length,
        stamp: 0,
    })
}

/// The first page's size in points, rounded up.
#[cfg(feature = "pdf")]
fn pdf_size(bytes: Vec<u8>) -> Option<(u32, u32)> {
    crate::armed(crate::Stage::Header);
    // Parsing decodes object and xref streams, each held whole.
    let (width, height) = hayro_syntax::with_decode_limit(PDF_DECODED_HERE, || {
        let pdf = hayro_syntax::Pdf::new(std::sync::Arc::new(bytes)).ok()?;
        Some(pdf.pages().first()?.render_dimensions())
    })
    .ok()??;
    let side = |length: f32| (length.is_finite() && length > 0.0).then(|| length.ceil() as u32);
    Some((side(width)?, side(height)?))
}

#[cfg(not(feature = "pdf"))]
fn pdf_size(_: Vec<u8>) -> Option<(u32, u32)> {
    None
}

/// How much of a worktree SVG the frame path reads for its size.
pub(crate) const SVG_HEAD: u64 = 64 * 1024;

/// Whether `path` is named as an SVG.
pub(crate) fn svg_named(path: &str) -> bool {
    named(path, "svg")
}

/// One SVG side from the start of its document, which is `bytes` long: its
/// size from the root tag, or `None`.
pub(crate) fn svg_side(head: &[u8], bytes: u64) -> Option<ImageSide> {
    // A blob arrives whole, and reads as far as a worktree side does.
    let head = &head[..head.len().min(SVG_HEAD as usize)];
    // A document an agent wrote must not be able to close the pane.
    let (width, height) = crate::caught(|| svg_size(head)).flatten()?;
    Some(ImageSide {
        format: Format::Svg,
        width,
        height,
        bytes,
        stamp: 0,
    })
}

/// The size an SVG's root `<svg>` start tag gives: `width` and `height` in
/// pixels, or else its `viewBox`.
fn svg_size(head: &[u8]) -> Option<(u32, u32)> {
    use svgtypes::{Length, LengthUnit};
    crate::armed(crate::Stage::Header);
    let valid = match std::str::from_utf8(head) {
        Ok(text) => text,
        // Cut mid-character at the read limit.
        Err(error) => std::str::from_utf8(&head[..error.valid_up_to()]).ok()?,
    };
    let attributes = root_attributes(valid.strip_prefix('\u{feff}').unwrap_or(valid))?;
    let value = |name: &str| {
        attributes
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
    };
    let view: Option<svgtypes::ViewBox> = value("viewBox").and_then(|view| view.parse().ok());
    // A value that does not parse counts as absent, as usvg reads it.
    let parsed = |name: &str| value(name).and_then(|value| value.parse::<Length>().ok());
    let (width, height) = (parsed("width"), parsed("height"));
    let font = parsed("font-size")
        .filter(|size| matches!(size.unit, LengthUnit::None | LengthUnit::Px))
        .map_or(12.0, |size| size.number);
    // As usvg converts it, at 96 dpi: a percentage, or a missing side, is a
    // share of the view box.
    let resolve = |length: Option<Length>, of: f64| {
        let Some(Length { number, unit }) = length else {
            return of;
        };
        match unit {
            LengthUnit::None | LengthUnit::Px => number,
            LengthUnit::Em => number * font,
            LengthUnit::Ex => number * font / 2.0,
            LengthUnit::In => number * 96.0,
            LengthUnit::Cm => number * 96.0 / 2.54,
            LengthUnit::Mm => number * 96.0 / 25.4,
            LengthUnit::Pt => number * 96.0 / 72.0,
            LengthUnit::Pc => number * 96.0 / 6.0,
            LengthUnit::Percent => of * number / 100.0,
        }
    };
    let (width, height) = match view {
        Some(view) => {
            let (w, h) = (resolve(width, view.w), resolve(height, view.h));
            // One side given alone takes the other from the view box's aspect.
            match (width, height) {
                (Some(_), None) => (w, w * view.h / view.w),
                (None, Some(_)) => (h * view.w / view.h, h),
                _ => (w, h),
            }
        }
        // Without a view box, only two absolute sides are a size.
        None => {
            let absolute =
                |length: Option<Length>| length.filter(|l| l.unit != LengthUnit::Percent);
            let (width, height) = (absolute(width)?, absolute(height)?);
            (resolve(Some(width), 0.0), resolve(Some(height), 0.0))
        }
    };
    let side = |length: f64| {
        (length.is_finite() && length > 0.0).then(|| length.ceil().min(f64::from(u32::MAX)) as u32)
    };
    Some((side(width)?, side(height)?))
}

/// The attributes of the document's root element when it is `<svg>`, past the
/// prolog, comments and doctype.
fn root_attributes(text: &str) -> Option<Vec<(&str, &str)>> {
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("<?") {
            rest = &after[after.find("?>")? + 2..];
        } else if let Some(after) = rest.strip_prefix("<!--") {
            rest = &after[after.find("-->")? + 3..];
        } else if let Some(after) = rest.strip_prefix("<!") {
            // A doctype, whose internal subset in brackets can hold a `>`.
            let end = after.find('>')?;
            let past = match after.find('[') {
                Some(open) if open < end => open + after[open..].find(']')?,
                _ => 0,
            };
            rest = &after[past + after[past..].find('>')? + 1..];
        } else {
            break;
        }
    }
    let tag = rest.strip_prefix("<svg")?;
    if !tag.starts_with(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/') {
        return None;
    }
    let mut attributes = Vec::new();
    let mut rest = tag;
    loop {
        rest = rest.trim_start();
        if rest.starts_with('>') || rest.starts_with("/>") {
            return Some(attributes);
        }
        let equals = rest.find('=')?;
        let name = rest[..equals].trim();
        // A name that runs into another tag is a start tag that never closed.
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
        {
            return None;
        }
        let after = rest[equals + 1..].trim_start();
        let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let body = &after[1..];
        let end = body.find(quote)?;
        attributes.push((name, &body[..end]));
        rest = &body[end + 1..];
    }
}

impl Picture {
    /// A picture when every side that exists read as an image. The outer `None`
    /// is a side the change does not have; the inner one is a side that is not
    /// an image.
    pub(crate) fn of(
        before: Option<Option<ImageSide>>,
        after: Option<Option<ImageSide>>,
    ) -> Option<Self> {
        if before == Some(None) || after == Some(None) || (before.is_none() && after.is_none()) {
            return None;
        }
        Some(Self {
            before: before.flatten(),
            after: after.flatten(),
        })
    }
}

/// Where one side of a changed image can be read from, by a reader on another
/// thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A blob in the repository at `git_dir`. One that is an LFS pointer is read
    /// from `store` instead.
    Blob {
        /// The repository's git directory.
        git_dir: std::path::PathBuf,
        /// The blob.
        id: gix::ObjectId,
        /// The repository's LFS store.
        store: std::path::PathBuf,
    },
    /// A working-tree file, or the LFS object a pointer in it names.
    File(std::path::PathBuf),
}

impl Source {
    /// This side's bytes, read on the calling thread. `None` when they cannot be.
    pub fn read(&self) -> Option<Vec<u8>> {
        match self {
            Self::File(path) => read_file(path),
            Self::Blob { git_dir, id, store } => {
                // Opened per read, so the worker keeps no repository handle of its own.
                let repo = gix::open(git_dir).ok()?;
                let bytes = repo
                    .find_object(*id)
                    .ok()?
                    .try_into_blob()
                    .ok()?
                    .take_data();
                match lfs_oid(&bytes) {
                    Some(oid) => read_file(&lfs_object(store, oid)),
                    None => Some(bytes),
                }
            }
        }
    }
}

/// Where the LFS object `oid` is kept in `store`.
pub(crate) fn lfs_object(store: &std::path::Path, oid: &str) -> std::path::PathBuf {
    store
        .join("objects")
        .join(&oid[0..2])
        .join(&oid[2..4])
        .join(oid)
}

/// A regular file's bytes. Followed, so a link to one reads, and a fifo is never
/// opened.
fn read_file(path: &std::path::Path) -> Option<Vec<u8>> {
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file())
        .then(|| std::fs::read(path).ok())
        .flatten()
}

/// Both sides of a changed image, as [`Source`]s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    /// The index side, or `HEAD`'s in the staged run.
    pub before: Option<Source>,
    /// The worktree side, or the index's in the staged run.
    pub after: Option<Source>,
}

/// A blob's stamp, from its id.
pub(crate) fn blob_stamp(id: &gix::ObjectId) -> u64 {
    let mut head = [0u8; 8];
    head.copy_from_slice(&id.as_bytes()[..8]);
    u64::from_le_bytes(head)
}

/// A worktree file's stamp, from its path and what `stat` says of it.
pub(crate) fn path_stamp(path: &str, meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    let inode = std::os::unix::fs::MetadataExt::ino(meta);
    #[cfg(not(unix))]
    let inode = 0;
    stamp_of(path, meta.len(), meta.modified().ok(), inode)
}

/// The stamp of a worktree file `stat` could not describe: its path alone, so
/// two such files never share one.
pub(crate) fn unread_stamp(path: &str) -> u64 {
    stamp_of(path, 0, None, 0)
}

/// The stamp of a file at `path` that `stat` describes. With the path:
/// without an inode, as on Windows, two files of one size written in one
/// tick share the rest.
fn stamp_of(path: &str, len: u64, modified: Option<std::time::SystemTime>, inode: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    len.hash(&mut hasher);
    modified.hash(&mut hasher);
    inode.hash(&mut hasher);
    hasher.finish()
}

/// One side read from `source`, which is `bytes` long: `None` unless the first
/// bytes are a format the pane reads, or `named` says TGA, and the header after
/// them holds.
///
/// Reads the header and nothing past it. `None` outside when the decoder
/// panicked, which more bytes would only repeat.
pub(crate) fn side_of(
    source: impl BufRead + Seek,
    bytes: u64,
    named: Option<Format>,
) -> Option<Option<ImageSide>> {
    // A header an agent wrote must not be able to close the pane.
    crate::caught(|| header_of(source, bytes, named))
}

fn header_of(source: impl BufRead + Seek, bytes: u64, named: Option<Format>) -> Option<ImageSide> {
    crate::armed(crate::Stage::Header);
    let mut reader = image::ImageReader::new(source).with_guessed_format().ok()?;
    if reader.format().is_none() && named == Some(Format::Tga) {
        reader.set_format(image::ImageFormat::Tga);
    }
    let format = Format::of(reader.format()?)?;
    let (width, height) = reader.into_dimensions().ok()?;
    Some(ImageSide {
        format,
        width,
        height,
        bytes,
        stamp: 0,
    })
}

/// Whether a file starting with `head`, or named as `named` says, can be an
/// image the pane reads.
pub(crate) fn may_be(head: &[u8], named: Option<Format>) -> bool {
    named.is_some()
        || image::guess_format(head)
            .ok()
            .and_then(Format::of)
            .is_some()
}

/// The longest text git-lfs writes as a pointer.
pub(crate) const POINTER_MAX: usize = 1024;

/// The object id an LFS pointer names, or `None` when `bytes` is not one.
pub(crate) fn lfs_oid(bytes: &[u8]) -> Option<&str> {
    if bytes.len() >= POINTER_MAX {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    if lines.next()? != "version https://git-lfs.github.com/spec/v1" {
        return None;
    }
    let oid = lines.find_map(|line| line.strip_prefix("oid sha256:"))?;
    (oid.len() == 64 && oid.bytes().all(|b| b.is_ascii_hexdigit())).then_some(oid)
}

/// The object an LFS pointer names, and the size it records.
pub(crate) fn lfs_pointer(bytes: &[u8]) -> Option<(&str, u64)> {
    let oid = lfs_oid(bytes)?;
    let size = std::str::from_utf8(bytes)
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("size "))?
        .parse()
        .ok()?;
    Some((oid, size))
}

/// The most each step of a header read may read, tried in order. A seek
/// skips without reading, but it drops the reader's buffer, so each one costs a
/// refill. `image`'s JPEG decoder reads to the end of what it is given, so the
/// first step is what one usually costs.
pub(crate) const HEAD_LIMITS: [u64; 2] = [64 * 1024, 1024 * 1024];

/// A reader that ends after `limit` bytes, and totals what it read.
pub(crate) struct Capped<R> {
    inner: R,
    limit: u64,
    pub(crate) total: u64,
}

impl<R> Capped<R> {
    pub(crate) fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            limit,
            total: 0,
        }
    }

    /// Start a step that may read `limit` more, returning what the last one read.
    pub(crate) fn restart(&mut self, limit: u64) -> u64 {
        self.limit = limit;
        std::mem::take(&mut self.total)
    }
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let room = self.limit.saturating_sub(self.total).min(buf.len() as u64) as usize;
        let n = self.inner.read(&mut buf[..room])?;
        self.total += n as u64;
        Ok(n)
    }
}

impl<R: Seek> Seek for Capped<R> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID: &str = "abababababababababababababababababababababababababababababababab";

    #[test]
    fn stamp_names_the_path() {
        // No inode, as on Windows: one size and one time, two paths.
        let when = Some(std::time::SystemTime::UNIX_EPOCH);
        assert_ne!(
            stamp_of("a.png", 10, when, 0),
            stamp_of("b.png", 10, when, 0)
        );
    }

    #[test]
    fn pointer_parses() {
        let text =
            format!("version https://git-lfs.github.com/spec/v1\noid sha256:{OID}\nsize 12\n");
        assert_eq!(lfs_oid(text.as_bytes()), Some(OID));
    }

    #[test]
    fn pointer_refused() {
        for text in [
            String::new(),
            format!("oid sha256:{OID}\n"),
            format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\n",
                &OID[1..]
            ),
            format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}zz\n",
                &OID[2..]
            ),
            format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{OID}\n{}",
                "x".repeat(POINTER_MAX)
            ),
        ] {
            assert_eq!(lfs_oid(text.as_bytes()), None, "{text:?}");
        }
    }
}
