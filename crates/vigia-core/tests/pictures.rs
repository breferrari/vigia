//! What a changed image reads as: its format, size and bytes on each side.

mod support;

use std::io::Cursor;

use image::{ImageFormat, RgbaImage};
use support::{Scratch, changes_sorted, encoded, lfs_pointer, png};
use vigia_core::{FileDiff, Format, ImageSide, Origin, Picture};

/// The diff of the one change at `rela`.
fn diff_of(scratch: &Scratch, rela: &str) -> FileDiff {
    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let change = changes
        .iter()
        .find(|c| c.path == rela)
        .unwrap_or_else(|| panic!("{rela} is not listed"));
    worktree.diff(change).expect("diff")
}

fn side(format: Format, width: u32, height: u32, bytes: &[u8]) -> ImageSide {
    ImageSide {
        format,
        width,
        height,
        bytes: bytes.len() as u64,
        stamp: 0,
    }
}

/// A picture with its stamps cleared, for comparing what was read.
fn plain(picture: Option<Picture>) -> Option<Picture> {
    let clear = |side: Option<ImageSide>| side.map(|side| ImageSide { stamp: 0, ..side });
    picture.map(|picture| Picture {
        before: clear(picture.before),
        after: clear(picture.after),
    })
}

/// A modified PNG reads both sides' format, size and bytes.
#[test]
fn image_size_line() {
    let scratch = Scratch::new("picture-modified");
    let old = png(24, 40, 1);
    let new = png(24, 40, 2);
    scratch.write("a.png", &old);
    scratch.commit_all("initial");
    scratch.write("a.png", &new);

    let diff = diff_of(&scratch, "a.png");
    assert!(diff.binary, "a PNG stopped counting as binary");
    assert_eq!(
        plain(diff.picture),
        Some(Picture {
            before: Some(side(Format::Png, 24, 40, &old)),
            after: Some(side(Format::Png, 24, 40, &new)),
        })
    );
}

/// A resized image reads both sizes.
#[test]
fn size_change_line() {
    let scratch = Scratch::new("picture-resized");
    scratch.write("a.png", png(24, 40, 1));
    scratch.commit_all("initial");
    scratch.write("a.png", png(30, 50, 1));

    let picture = diff_of(&scratch, "a.png").picture.expect("a picture");
    let sizes = |side: Option<ImageSide>| side.map(|side| (side.width, side.height));
    assert_eq!(
        (sizes(picture.before), sizes(picture.after)),
        (Some((24, 40)), Some((30, 50)))
    );
}

/// An added image has no old side, and a deleted one no new side.
#[test]
fn added_and_deleted_images() {
    let scratch = Scratch::new("picture-added-deleted");
    let gone = png(8, 8, 3);
    scratch.write("gone.png", &gone);
    scratch.commit_all("initial");
    scratch.remove("gone.png");
    let added = png(16, 12, 4);
    scratch.write("added.png", &added);
    scratch.git(&["add", "-N", "added.png"]);

    assert_eq!(
        plain(diff_of(&scratch, "added.png").picture),
        Some(Picture {
            before: None,
            after: Some(side(Format::Png, 16, 12, &added)),
        })
    );
    assert_eq!(
        plain(diff_of(&scratch, "gone.png").picture),
        Some(Picture {
            before: Some(side(Format::Png, 8, 8, &gone)),
            after: None,
        })
    );
}

/// Every format reads as itself, TGA by its name, and a format change reads both.
#[test]
fn every_format_reads() {
    let scratch = Scratch::new("picture-formats");
    let cases = [
        (
            "a.jpg",
            ImageFormat::Jpeg,
            ImageFormat::Jpeg,
            Format::Jpeg,
            Format::Jpeg,
        ),
        (
            "a.gif",
            ImageFormat::Gif,
            ImageFormat::Gif,
            Format::Gif,
            Format::Gif,
        ),
        (
            "a.webp",
            ImageFormat::WebP,
            ImageFormat::WebP,
            Format::Webp,
            Format::Webp,
        ),
        (
            "a.bmp",
            ImageFormat::Bmp,
            ImageFormat::Bmp,
            Format::Bmp,
            Format::Bmp,
        ),
        (
            "a.ico",
            ImageFormat::Ico,
            ImageFormat::Ico,
            Format::Ico,
            Format::Ico,
        ),
        (
            "a.tiff",
            ImageFormat::Tiff,
            ImageFormat::Tiff,
            Format::Tiff,
            Format::Tiff,
        ),
        (
            "a.qoi",
            ImageFormat::Qoi,
            ImageFormat::Qoi,
            Format::Qoi,
            Format::Qoi,
        ),
        (
            "a.TGA",
            ImageFormat::Tga,
            ImageFormat::Tga,
            Format::Tga,
            Format::Tga,
        ),
        (
            "swap.img",
            ImageFormat::Png,
            ImageFormat::Jpeg,
            Format::Png,
            Format::Jpeg,
        ),
    ];
    for (name, old, ..) in cases {
        scratch.write(name, encoded(old, 20, 10, 1));
    }
    scratch.commit_all("initial");
    for (name, _, new, ..) in cases {
        scratch.write(name, encoded(new, 20, 10, 9));
    }

    for (name, _, _, before, after) in cases {
        let picture = diff_of(&scratch, name)
            .picture
            .unwrap_or_else(|| panic!("{name} read no picture"));
        assert_eq!(
            (
                picture.before.map(|side| side.format),
                picture.after.map(|side| side.format)
            ),
            (Some(before), Some(after)),
            "{name}"
        );
    }
}

/// A binary file that is not one of the four formats, or whose header does not
/// hold, reads no picture and stays binary.
#[test]
fn non_images_stay_binary() {
    let scratch = Scratch::new("picture-refused");
    let mut lying = png(24, 40, 1);
    lying.truncate(8);
    lying.extend_from_slice(&[0u8; 64]);
    let mut truncated = png(24, 40, 1);
    truncated.truncate(20);
    let cases: [(&str, Vec<u8>, Vec<u8>); 4] = [
        ("blob.bin", vec![0, 1, 2, 3], vec![0, 1, 2, 4]),
        ("lying.png", png(24, 40, 1), lying),
        ("truncated.png", png(24, 40, 1), truncated),
        ("mixed.png", vec![0, 1, 2, 3], png(24, 40, 1)),
    ];
    for (name, old, _) in &cases {
        scratch.write(name, old);
    }
    scratch.commit_all("initial");
    for (name, _, new) in &cases {
        scratch.write(name, new);
    }

    for (name, _, _) in &cases {
        let diff = diff_of(&scratch, name);
        assert!(diff.binary, "{name} stopped counting as binary");
        assert_eq!(diff.picture, None, "{name} read as a picture");
    }
}

/// A `diff` attribute on a PNG keeps its text diff, and `binary` on one still
/// reads its picture.
#[test]
fn attributes_decide_first() {
    let scratch = Scratch::new("picture-attributes");
    scratch.write(".gitattributes", "text.png diff\nforced.png binary\n");
    for name in ["text.png", "forced.png"] {
        scratch.write(name, png(24, 40, 1));
    }
    scratch.commit_all("initial");
    for name in ["text.png", "forced.png"] {
        scratch.write(name, png(24, 40, 2));
    }

    let text = diff_of(&scratch, "text.png");
    assert!(
        !text.binary && text.picture.is_none(),
        "text.png read as an image"
    );
    let forced = diff_of(&scratch, "forced.png");
    assert!(
        forced.binary && forced.picture.is_some(),
        "forced.png read no picture"
    );
}

/// The staged run reads its picture from the index and `HEAD`.
#[test]
fn staged_image_line() {
    let scratch = Scratch::new("picture-staged");
    scratch.write("a.png", png(24, 40, 1));
    scratch.commit_all("initial");
    scratch.write("a.png", png(24, 40, 2));
    scratch.git(&["add", "a.png"]);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let index = frame
        .files()
        .iter()
        .position(|change| change.origin == Origin::Staged)
        .expect("a staged change");
    let picture = frame.diff(index).expect("diff").1.picture;
    assert!(
        picture.is_some_and(|picture| picture.before.is_some() && picture.after.is_some()),
        "the staged PNG read no picture"
    );
}

/// A repository whose `a.png` is stored as an LFS pointer to `old`, with `new`
/// in the worktree. No `git-lfs` is needed: the pointer is committed as text,
/// and the attribute and filter are set after.
fn lfs_repo(name: &str, old: &[u8], new: &[u8]) -> (Scratch, String) {
    let scratch = Scratch::new(name);
    let (oid, text) = lfs_pointer(old, 0xab);
    scratch.write("a.png", &text);
    scratch.commit_all("initial");
    scratch.write(
        ".gitattributes",
        "*.png filter=lfs diff=lfs merge=lfs -text\n",
    );
    scratch.git(&["add", ".gitattributes"]);
    scratch.git(&["commit", "-q", "-m", "attributes"]);
    scratch.git(&["config", "filter.lfs.process", "no-such-filter-process"]);
    scratch.write("a.png", new);
    (scratch, oid)
}

/// Where git-lfs keeps the object `oid` under the store `root`.
fn object_path(root: &std::path::Path, oid: &str) -> std::path::PathBuf {
    root.join("objects")
        .join(&oid[0..2])
        .join(&oid[2..4])
        .join(oid)
}

fn store(root: &std::path::Path, oid: &str, content: &[u8]) {
    let path = object_path(root, oid);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the store");
    std::fs::write(path, content).expect("write the object");
}

/// A pointer reads the object it names from the local store.
#[test]
fn lfs_pointer_resolves() {
    let old = png(24, 40, 1);
    let new = png(30, 50, 2);
    let (scratch, oid) = lfs_repo("picture-lfs", &old, &new);
    store(&scratch.root().join(".git").join("lfs"), &oid, &old);

    let diff = diff_of(&scratch, "a.png");
    assert!(diff.binary, "a pointer against its content diffed as text");
    assert_eq!(
        plain(diff.picture),
        Some(Picture {
            before: Some(side(Format::Png, 24, 40, &old)),
            after: Some(side(Format::Png, 30, 50, &new)),
        })
    );
}

/// A pointer whose object is not on this machine reads no picture.
#[test]
fn lfs_object_missing() {
    let (scratch, _) = lfs_repo("picture-lfs-missing", &png(24, 40, 1), &png(24, 40, 2));

    let diff = diff_of(&scratch, "a.png");
    assert!(
        diff.binary && diff.picture.is_none(),
        "a missing object read as a picture"
    );
}

/// `lfs.storage` moves the store, relative to the git directory.
#[test]
fn lfs_storage_config() {
    let old = png(24, 40, 1);
    let (scratch, oid) = lfs_repo("picture-lfs-storage", &old, &png(24, 40, 2));
    scratch.git(&["config", "lfs.storage", "elsewhere"]);
    store(&scratch.root().join(".git").join("elsewhere"), &oid, &old);

    let picture = diff_of(&scratch, "a.png").picture;
    assert!(
        picture.is_some_and(|picture| picture.before.is_some()),
        "the store lfs.storage names was not read"
    );
}

/// A large worktree image costs its sniff and its header, never its size.
#[test]
fn worktree_image_reads_header_only() {
    let scratch = Scratch::new("picture-header-only");
    let small = png(8, 8, 1);
    scratch.write("big.png", &small);
    scratch.commit_all("initial");
    let mut big = Cursor::new(Vec::new());
    noise(512)
        .write_to(&mut big, ImageFormat::Png)
        .expect("encode");
    let big = big.into_inner();
    assert!(big.len() > 256 * 1024, "the fixture is too small to tell");
    scratch.write("big.png", &big);

    let diff = diff_of(&scratch, "big.png");
    assert!(diff.picture.is_some(), "big.png read no picture");
    assert!(
        diff.bytes <= small.len() as u64 + 64 * 1024,
        "reading a {} byte PNG's size compared {} bytes",
        big.len(),
        diff.bytes
    );
}

/// A path with a space reads the same.
#[test]
fn spaced_path_reads() {
    let scratch = Scratch::new("picture-spaced");
    scratch.write("my screens/home screen.png", png(24, 40, 1));
    scratch.commit_all("initial");
    scratch.write("my screens/home screen.png", png(24, 40, 2));

    assert!(
        diff_of(&scratch, "my screens/home screen.png")
            .picture
            .is_some(),
        "a spaced path read no picture"
    );
}

/// A link the attribute marks binary reads no picture: git compares the link's
/// target text, not the image it points at.
#[test]
fn link_reads_nothing() {
    let scratch = Scratch::new("picture-link");
    scratch.write(".gitattributes", "*.png binary\n");
    scratch.write("real.png", png(24, 40, 1));
    scratch.commit_all("initial");
    if !support::made_link(&scratch, "real.png", "link.png") {
        return;
    }
    scratch.git(&["add", "-N", "link.png"]);

    let diff = diff_of(&scratch, "link.png");
    assert_eq!(
        diff.picture, None,
        "a link read as the picture it points at"
    );
}

/// An old side that cannot be read costs the size line, never the diff.
#[test]
fn missing_blob_stays_binary() {
    let scratch = Scratch::new("picture-missing-blob");
    scratch.write("a.png", png(24, 40, 1));
    scratch.commit_all("initial");
    scratch.write("a.png", png(24, 40, 2));
    scratch.point_at_a_missing_blob("a.png");

    let diff = diff_of(&scratch, "a.png");
    assert!(
        diff.binary,
        "a PNG with a missing blob stopped counting as binary"
    );
    assert_eq!(diff.picture, None, "a missing blob read as a picture");
}

/// A 1024 square JPEG with `segments` APP1 blocks of 60000 NULs before its frame
/// header, the way a camera's EXIF sits.
fn jpeg_after(segments: usize) -> Vec<u8> {
    let mut body = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(noise(1024))
        .to_rgb8()
        .write_to(&mut body, ImageFormat::Jpeg)
        .expect("encode");
    let body = body.into_inner();
    let mut jpeg = body[..2].to_vec();
    for _ in 0..segments {
        jpeg.extend_from_slice(&[0xff, 0xe1]);
        jpeg.extend_from_slice(&60_002u16.to_be_bytes());
        jpeg.extend(std::iter::repeat_n(0u8, 60_000));
    }
    jpeg.extend_from_slice(&body[2..]);
    jpeg
}

/// A JPEG with ordinary metadata reads in the first step.
#[test]
fn jpeg_metadata_first() {
    let scratch = Scratch::new("picture-jpeg-metadata");
    let small = encoded(ImageFormat::Jpeg, 8, 8, 1);
    scratch.write("a.jpg", &small);
    scratch.commit_all("initial");
    let big = jpeg_after(1);
    assert!(big.len() > 512 * 1024, "the fixture is too small to tell");
    scratch.write("a.jpg", &big);

    let diff = diff_of(&scratch, "a.jpg");
    let after = diff.picture.and_then(|picture| picture.after);
    assert_eq!(
        after.map(|side| (side.width, side.height)),
        Some((1024, 1024))
    );
    assert!(
        diff.bytes <= small.len() as u64 + 64 * 1024 + 8000,
        "reading a {} byte JPEG's size compared {} bytes",
        big.len(),
        diff.bytes
    );
}

/// A JPEG whose metadata outruns the first step reads in the second.
#[test]
fn jpeg_metadata_long() {
    let scratch = Scratch::new("picture-jpeg-long");
    scratch.write("a.jpg", encoded(ImageFormat::Jpeg, 8, 8, 1));
    scratch.commit_all("initial");
    // 240 KB, and 1020 KB: the second needs the whole of the second step.
    for segments in [4, 17] {
        scratch.write("a.jpg", jpeg_after(segments));
        let after = diff_of(&scratch, "a.jpg")
            .picture
            .and_then(|picture| picture.after);
        assert_eq!(
            after.map(|side| (side.width, side.height)),
            Some((1024, 1024)),
            "a JPEG with {segments} metadata segments did not read"
        );
    }
}

/// A false start no longer than the first step is read once.
#[test]
fn false_start_short() {
    let scratch = Scratch::new("picture-false-start-short");
    // A JPEG's start, because its decoder reads to the end of what it is given.
    let mut junk = vec![0xff, 0xd8, 0xff];
    junk.resize(64 * 1024, 0);
    scratch.write("a.bin", &junk);
    scratch.commit_all("initial");
    junk[3] = 7;
    scratch.write("a.bin", &junk);

    let diff = diff_of(&scratch, "a.bin");
    assert!(diff.picture.is_none(), "the junk read as a picture");
    assert!(
        diff.bytes <= 64 * 1024 + 8000,
        "a 64 KiB false start compared {} bytes",
        diff.bytes
    );
}

/// A JPEG whose size comes after more than a MiB of metadata stays binary.
#[test]
fn jpeg_metadata_limit() {
    let scratch = Scratch::new("picture-jpeg-limit");
    scratch.write("a.jpg", encoded(ImageFormat::Jpeg, 8, 8, 1));
    scratch.commit_all("initial");
    scratch.write("a.jpg", jpeg_after(20));

    let diff = diff_of(&scratch, "a.jpg");
    assert!(
        diff.binary && diff.picture.is_none(),
        "a JPEG past the limit read"
    );
}

/// An LFS store reached through a link reads, and so does an object that is a
/// link itself.
#[cfg(unix)]
#[test]
fn lfs_store_linked() {
    let old = png(24, 40, 1);
    let (scratch, oid) = lfs_repo("picture-lfs-linked", &old, &png(24, 40, 2));
    let mut name = scratch.root().file_name().expect("a name").to_os_string();
    name.push("-store");
    let elsewhere = scratch.root().with_file_name(name);
    let _ = std::fs::remove_dir_all(&elsewhere);
    store(&elsewhere, &oid, &old);
    let lfs = scratch.root().join(".git").join("lfs");
    // An installed git-lfs makes its store on its own, as it does on CI runners.
    let _ = std::fs::remove_dir_all(&lfs);
    std::os::unix::fs::symlink(&elsewhere, &lfs).expect("link the store");
    let linked_store = diff_of(&scratch, "a.png").picture;

    std::fs::remove_file(&lfs).expect("unlink the store");
    let object = object_path(&lfs, &oid);
    std::fs::create_dir_all(object.parent().expect("a parent")).expect("make the store");
    std::os::unix::fs::symlink(object_path(&elsewhere, &oid), &object).expect("link the object");
    let linked_object = diff_of(&scratch, "a.png").picture;
    let _ = std::fs::remove_dir_all(&elsewhere);

    for (what, picture) in [("store", linked_store), ("object", linked_object)] {
        assert!(
            picture.is_some_and(|picture| picture.before.is_some()),
            "a linked {what} was not read"
        );
    }
}

/// A fifo where an LFS object should be is never opened.
#[cfg(unix)]
#[test]
fn lfs_object_fifo() {
    use std::sync::mpsc;
    use std::time::Duration;

    let (scratch, oid) = lfs_repo("picture-lfs-fifo", &png(24, 40, 1), &png(24, 40, 2));
    let object = object_path(&scratch.root().join(".git").join("lfs"), &oid);
    std::fs::create_dir_all(object.parent().expect("a parent")).expect("make the store");
    let made = std::process::Command::new("mkfifo")
        .arg(&object)
        .status()
        .is_ok_and(|status| status.success());
    if !made {
        return;
    }

    let root = scratch.root().to_path_buf();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let worktree = vigia_core::Worktree::discover(&root).expect("discover");
        let changes = changes_sorted(&worktree);
        let change = changes.iter().find(|c| c.path == "a.png").expect("listed");
        let _ = tx.send(worktree.diff(change).expect("diff").picture);
    });
    // Detached on purpose: a thread parked in `open` never returns.
    let picture = match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(picture) => picture,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("the diff is still inside an open of the fifo")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("the diff panicked; see its output above")
        }
    };
    assert_eq!(picture, None, "a fifo in the store read as a picture");
}

/// Noise, which no encoder can shrink.
fn noise(side: u32) -> RgbaImage {
    RgbaImage::from_fn(side, side, |x, y| {
        let n =
            (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)).wrapping_mul(2_246_822_519);
        image::Rgba(n.to_le_bytes())
    })
}

/// A TIFF whose directory sits after its pixels, past the read limit, reads by
/// seeking there.
#[test]
fn tiff_directory_last() {
    let scratch = Scratch::new("picture-tiff-last");
    let small = encoded(ImageFormat::Tiff, 8, 8, 1);
    scratch.write("a.tiff", &small);
    scratch.commit_all("initial");
    let mut big = Cursor::new(Vec::new());
    noise(1024)
        .write_to(&mut big, ImageFormat::Tiff)
        .expect("encode");
    let big = big.into_inner();
    assert!(
        big.len() > 2 * 1024 * 1024,
        "the fixture is too small to tell"
    );
    scratch.write("a.tiff", &big);

    let diff = diff_of(&scratch, "a.tiff");
    let after = diff.picture.and_then(|picture| picture.after);
    assert_eq!(
        after.map(|side| (side.width, side.height)),
        Some((1024, 1024))
    );
    assert!(
        diff.bytes <= small.len() as u64 + 64 * 1024,
        "reading a {} byte TIFF's size compared {} bytes",
        big.len(),
        diff.bytes
    );
}

/// A large file that only starts like an image costs what its decoder read
/// before it gave up, not the read limit.
#[test]
fn false_start_cost() {
    let scratch = Scratch::new("picture-false-start");
    let mut junk = b"BM".to_vec();
    junk.extend(std::iter::repeat_n(0u8, 4 * 1024 * 1024));
    scratch.write("a.bin", &junk);
    scratch.commit_all("initial");
    junk[3] = 7;
    scratch.write("a.bin", &junk);

    let diff = diff_of(&scratch, "a.bin");
    assert!(
        diff.binary && diff.picture.is_none(),
        "the junk read as a picture"
    );
    assert!(
        diff.bytes <= 64 * 1024,
        "a false start compared {} bytes",
        diff.bytes
    );
}

/// A worktree pointer with extension lines before its oid, longer than one read
/// buffer, still resolves. The worktree holds pointers where git-lfs is not
/// installed.
#[test]
fn lfs_long_pointer() {
    let old = png(24, 40, 1);
    let new = png(30, 50, 2);
    let (scratch, old_oid) = lfs_repo("picture-lfs-long", &old, b"placeholder");
    let lfs = scratch.root().join(".git").join("lfs");
    store(&lfs, &old_oid, &old);
    let new_oid = "cd".repeat(32);
    store(&lfs, &new_oid, &new);
    let extensions: String = (0..6)
        .map(|n| format!("ext-{n}-name sha256:{}\n", "ef".repeat(32)))
        .collect();
    let text = format!(
        "version https://git-lfs.github.com/spec/v1\n{extensions}oid sha256:{new_oid}\nsize {}\n",
        new.len()
    );
    assert!(
        text.len() > 512 && text.len() < 1024,
        "the fixture is the wrong length"
    );
    scratch.write("a.png", &text);

    let after = diff_of(&scratch, "a.png")
        .picture
        .and_then(|picture| picture.after);
    assert_eq!(
        after.map(|side| (side.width, side.height)),
        Some((30, 50)),
        "a long worktree pointer did not resolve"
    );
}

/// A side's stamp moves with its content and stands still without it.
#[test]
fn stamps_follow_content() {
    let scratch = Scratch::new("picture-stamps");
    scratch.write("a.png", png(24, 40, 1));
    scratch.commit_all("initial");
    scratch.write("a.png", png(24, 40, 2));
    let first = diff_of(&scratch, "a.png").picture.expect("a picture");
    let again = diff_of(&scratch, "a.png").picture.expect("a picture");
    // A second apart, so the file's time moves even where it counts in seconds.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    scratch.write("a.png", png(24, 40, 3));
    let moved = diff_of(&scratch, "a.png").picture.expect("a picture");

    let stamps = |picture: Picture| {
        picture
            .before
            .zip(picture.after)
            .map(|(b, a)| (b.stamp, a.stamp))
    };
    let (old, new) = stamps(first).expect("both sides");
    assert_ne!(old, 0, "the blob side has no stamp");
    assert_eq!(
        stamps(again),
        Some((old, new)),
        "a stamp moved without its content"
    );
    let (old_again, newer) = stamps(moved).expect("both sides");
    assert_eq!(old_again, old, "the blob side's stamp moved");
    assert_ne!(
        newer, new,
        "the worktree side's stamp stood still while it changed"
    );
}

/// The sources of a changed image read back what each side holds: a blob, a
/// worktree file, and an LFS pointer's object.
#[test]
fn image_sources() {
    let old = png(24, 40, 1);
    let new = png(24, 40, 2);
    let (scratch, oid) = lfs_repo("picture-sources", &old, &new);
    store(&scratch.root().join(".git").join("lfs"), &oid, &old);
    scratch.write("plain.img", png(8, 8, 1));
    scratch.git(&["add", "plain.img"]);
    scratch.git(&["commit", "-q", "-m", "plain"]);
    scratch.write("plain.img", png(8, 8, 2));

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let read = |path: &str| {
        let change = changes.iter().find(|c| c.path == path).expect("listed");
        let sources = worktree.image_sources(change);
        (
            sources.before.and_then(|source| source.read()),
            sources.after.and_then(|source| source.read()),
        )
    };
    assert_eq!(
        read("a.png"),
        (Some(old), Some(new)),
        "the LFS pair read back wrong"
    );
    assert_eq!(
        read("plain.img"),
        (Some(png(8, 8, 1)), Some(png(8, 8, 2))),
        "the plain pair read back wrong"
    );
}
