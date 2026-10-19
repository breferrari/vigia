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

/// Both sides of a changed image. A side is `None` where the change has none:
/// an added image has no old side and a deleted one no new side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picture {
    /// The index side, or `HEAD`'s in the staged run.
    pub before: Option<ImageSide>,
    /// The worktree side, or the index's in the staged run.
    pub after: Option<ImageSide>,
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

/// A file's stamp, from what `stat` says of it.
pub(crate) fn file_stamp(meta: &std::fs::Metadata) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    meta.len().hash(&mut hasher);
    meta.modified().ok().hash(&mut hasher);
    #[cfg(unix)]
    std::os::unix::fs::MetadataExt::ino(meta).hash(&mut hasher);
    hasher.finish()
}

/// One side read from `source`, which is `bytes` long: `None` unless the first
/// bytes are a format the pane reads, or `named` says TGA, and the header after
/// them holds.
///
/// Reads the header and nothing past it.
pub(crate) fn side_of(
    source: impl BufRead + Seek,
    bytes: u64,
    named: Option<Format>,
) -> Option<ImageSide> {
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
