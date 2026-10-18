//! What a changed image is on each side: its format, its size and its bytes.

use std::fmt;
use std::io::{BufRead, Read, Seek};

/// The image formats the pane reads. Any other is a binary file like the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// Portable Network Graphics.
    Png,
    /// JPEG.
    Jpeg,
    /// GIF, of which only the first frame is ever drawn.
    Gif,
    /// WebP.
    Webp,
}

impl Format {
    fn of(format: image::ImageFormat) -> Option<Self> {
        match format {
            image::ImageFormat::Png => Some(Self::Png),
            image::ImageFormat::Jpeg => Some(Self::Jpeg),
            image::ImageFormat::Gif => Some(Self::Gif),
            image::ImageFormat::WebP => Some(Self::Webp),
            _ => None,
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
            Self::Webp => "WebP",
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

/// One side read from `source`, which is `bytes` long: `None` unless the first
/// bytes are one of the four formats and the header after them holds.
///
/// Reads the header and nothing past it.
pub(crate) fn side_of(source: impl BufRead + Seek, bytes: u64) -> Option<ImageSide> {
    let reader = image::ImageReader::new(source).with_guessed_format().ok()?;
    let format = Format::of(reader.format()?)?;
    let (width, height) = reader.into_dimensions().ok()?;
    Some(ImageSide {
        format,
        width,
        height,
        bytes,
    })
}

/// Whether a file starting with `head` can be an image the pane reads.
pub(crate) fn may_be(head: &[u8]) -> bool {
    image::guess_format(head)
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

/// A reader that counts what it hands out, so a header read can report its cost.
pub(crate) struct Counted<R> {
    inner: R,
    pub(crate) read: u64,
}

impl<R> Counted<R> {
    pub(crate) fn new(inner: R) -> Self {
        Self { inner, read: 0 }
    }
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        Ok(n)
    }
}

impl<R: Seek> Seek for Counted<R> {
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
