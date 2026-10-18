//! What a changed image reads as: its format, size and bytes on each side.

mod support;

use std::io::Cursor;

use image::{ImageFormat, RgbaImage};
use support::{Scratch, changes_sorted};
use vigia_core::{FileDiff, Format, ImageSide, Origin, Picture};

/// An image of `width` by `height` in `format`, every pixel from `seed`.
fn encoded(format: ImageFormat, width: u32, height: u32, seed: u8) -> Vec<u8> {
    let image = RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([seed, (x % 256) as u8, (y % 256) as u8, 255])
    });
    let mut bytes = Cursor::new(Vec::new());
    match format {
        // JPEG has no alpha channel to encode.
        ImageFormat::Jpeg => image::DynamicImage::ImageRgba8(image)
            .to_rgb8()
            .write_to(&mut bytes, format),
        _ => image.write_to(&mut bytes, format),
    }
    .expect("encode the fixture");
    bytes.into_inner()
}

fn png(width: u32, height: u32, seed: u8) -> Vec<u8> {
    encoded(ImageFormat::Png, width, height, seed)
}

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
    }
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
        diff.picture,
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
        diff_of(&scratch, "added.png").picture,
        Some(Picture {
            before: None,
            after: Some(side(Format::Png, 16, 12, &added)),
        })
    );
    assert_eq!(
        diff_of(&scratch, "gone.png").picture,
        Some(Picture {
            before: Some(side(Format::Png, 8, 8, &gone)),
            after: None,
        })
    );
}

/// JPEG, GIF and WebP read as themselves, and a format change reads both.
#[test]
fn every_format_reads() {
    let scratch = Scratch::new("picture-formats");
    let cases = [
        ("a.jpg", ImageFormat::Jpeg, Format::Jpeg),
        ("a.gif", ImageFormat::Gif, Format::Gif),
        ("a.webp", ImageFormat::WebP, Format::Webp),
        ("swap.img", ImageFormat::Png, Format::Png),
    ];
    for (name, format, _) in cases {
        scratch.write(name, encoded(format, 20, 10, 1));
    }
    scratch.commit_all("initial");
    for (name, format, _) in cases {
        let format = if name == "swap.img" {
            ImageFormat::Jpeg
        } else {
            format
        };
        scratch.write(name, encoded(format, 20, 10, 9));
    }

    for (name, _, read) in cases {
        let picture = diff_of(&scratch, name)
            .picture
            .unwrap_or_else(|| panic!("{name} read no picture"));
        let after = if name == "swap.img" {
            Format::Jpeg
        } else {
            read
        };
        assert_eq!(
            (
                picture.before.map(|side| side.format),
                picture.after.map(|side| side.format)
            ),
            (Some(read), Some(after)),
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

/// The text of an LFS pointer to `content`, under an oid made from `seed`. The
/// pane reads the object at the oid's path and never hashes it, so any 64 hex
/// digits serve.
fn pointer(content: &[u8], seed: u8) -> (String, String) {
    let oid = format!("{seed:02x}").repeat(32);
    let text = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
        content.len()
    );
    (oid, text)
}

/// A repository whose `a.png` is stored as an LFS pointer to `old`, with `new`
/// in the worktree. No `git-lfs` is needed: the pointer is committed as text,
/// and the attribute and filter are set after.
fn lfs_repo(name: &str, old: &[u8], new: &[u8]) -> (Scratch, String) {
    let scratch = Scratch::new(name);
    let (oid, text) = pointer(old, 0xab);
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
        diff.picture,
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
    // Noise, because a smooth image compresses too small to tell.
    let noise = RgbaImage::from_fn(512, 512, |x, y| {
        let n =
            (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)).wrapping_mul(2_246_822_519);
        image::Rgba(n.to_le_bytes())
    });
    let mut big = Cursor::new(Vec::new());
    noise.write_to(&mut big, ImageFormat::Png).expect("encode");
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
