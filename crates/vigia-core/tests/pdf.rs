//! What a changed PDF reads as on the frame path: its first page's size, or
//! what it drew before. `SPEC.md` §11.1.

mod support;

use support::{
    Scratch, changes_sorted, icon_ops, icon_pdf, locked_pdf, open_locked_pdf, pdf, pdf_holding,
    pdf_updated, pdf_with, zlib_zeros,
};
use vigia_core::{FileDiff, Format};

fn diff_of(scratch: &Scratch, rela: &str) -> FileDiff {
    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let change = changes
        .iter()
        .find(|c| c.path == rela)
        .unwrap_or_else(|| panic!("{rela} is not listed"));
    worktree.diff(change).expect("diff")
}

/// One PDF changed from `old` to `new`, as its diff.
fn changed(name: &str, old: &[u8], new: &[u8]) -> FileDiff {
    let scratch = Scratch::new(name);
    scratch.write("icon.pdf", old);
    scratch.commit_all("initial");
    scratch.write("icon.pdf", new);
    diff_of(&scratch, "icon.pdf")
}

/// A side's format and size.
type Size = Option<(Format, u32, u32)>;

/// Each side's format and size, or `None` with no picture.
fn sizes(diff: &FileDiff) -> Option<(Size, Size)> {
    let picture = diff.picture?;
    let size = |side: Option<vigia_core::ImageSide>| side.map(|s| (s.format, s.width, s.height));
    Some((size(picture.before), size(picture.after)))
}

#[test]
fn pdf_size_line() {
    let diff = changed("pdf-size", &icon_pdf(1), &icon_pdf(2));
    let side = Some((Format::Pdf, 24, 24));
    assert_eq!(sizes(&diff), Some((side, side)));
    // Points round up, and the first page gives the size.
    let a4 = pdf(&[(595, 842, &icon_ops(1)), (842, 595, &icon_ops(1))]);
    let fraction = pdf(&[(20, 10, &icon_ops(2))]).replace_bytes("[0 0 20 10]", "[0 0 19.2 9.5]");
    let diff = changed("pdf-size-a4", &a4, &fraction);
    assert_eq!(
        sizes(&diff),
        Some((Some((Format::Pdf, 595, 842)), Some((Format::Pdf, 20, 10))))
    );
}

#[test]
fn pdf_added_deleted() {
    // Two repositories: alike as text, the two would pair as a rename.
    let side = Some((Format::Pdf, 24, 24));
    let added = Scratch::new("pdf-added");
    added.write("keep.txt", "x\n");
    added.commit_all("initial");
    added.write("new.pdf", icon_pdf(2));
    assert_eq!(sizes(&diff_of(&added, "new.pdf")), Some((None, side)));
    let deleted = Scratch::new("pdf-deleted");
    deleted.write("gone.pdf", icon_pdf(1));
    deleted.commit_all("initial");
    std::fs::remove_file(deleted.path_of("gone.pdf")).expect("remove");
    assert_eq!(sizes(&diff_of(&deleted, "gone.pdf")), Some((side, None)));
}

#[test]
fn pdf_not_pdf_kept() {
    // Named a PDF, with none of its header: it diffs as it did.
    let diff = changed("pdf-not-pdf", b"plain text\n", b"other text\n");
    assert!(diff.picture.is_none(), "a .pdf that is not one was sized");
    assert!(
        !diff.hunks.is_empty(),
        "a text file named .pdf lost its diff"
    );
}

#[test]
fn pdf_attribute_keeps_diff() {
    let scratch = Scratch::new("pdf-attribute");
    scratch.write(".gitattributes", "*.pdf diff\n");
    scratch.write("icon.pdf", icon_pdf(1));
    scratch.commit_all("initial");
    scratch.write("icon.pdf", icon_pdf(2));
    let diff = diff_of(&scratch, "icon.pdf");
    assert!(
        diff.picture.is_none(),
        "a diff attribute did not keep the text diff"
    );
}

#[test]
fn pdf_cut_binary() {
    let whole = icon_pdf(2);
    // Cut before the page tree ends: nothing to size.
    let cut = &whole[..60];
    let diff = changed("pdf-cut", &icon_pdf(1), cut);
    assert!(
        diff.picture.is_none(),
        "a cut PDF was sized: {:?}",
        sizes(&diff)
    );
}

#[test]
fn pdf_password_binary() {
    let diff = changed("pdf-password", &icon_pdf(1), &locked_pdf());
    assert!(
        diff.picture.is_none(),
        "a PDF that needs a password was sized"
    );
}

#[test]
fn pdf_empty_password_sized() {
    let diff = changed("pdf-open-locked", &icon_pdf(1), &open_locked_pdf(2));
    assert_eq!(
        sizes(&diff).map(|(_, after)| after),
        Some(Some((Format::Pdf, 24, 24))),
        "a PDF that opens with no password was not sized"
    );
}

#[test]
fn pdf_over_limit_kept() {
    // Past the frame path's whole read: one page whose stream fills 17 MiB.
    let mut ops = Vec::new();
    while ops.len() < 17 << 20 {
        ops.extend_from_slice(b"0.5 0.5 0.5 rg 1 1 3 3 re f\n");
    }
    let diff = changed("pdf-large", &icon_pdf(1), &pdf(&[(24, 24, &ops)]));
    assert!(
        diff.picture.is_none(),
        "a PDF past the read limit was sized"
    );
}

#[test]
fn pdf_counted_binary() {
    let diff = changed("pdf-counted", &icon_pdf(1), &icon_pdf(2));
    assert!(diff.binary, "a drawn PDF is not counted as binary");
    assert_eq!(
        (diff.added, diff.removed),
        (0, 0),
        "a drawn PDF's lines count toward the total"
    );
}

#[test]
fn pdf_rename_drawn() {
    let scratch = Scratch::new("pdf-rename");
    scratch.write("old.pdf", icon_pdf(1));
    scratch.commit_all("initial");
    std::fs::remove_file(scratch.path_of("old.pdf")).expect("remove");
    scratch.write("new.pdf", icon_pdf(1));
    scratch.git(&["add", "-A"]);
    scratch.write("new.pdf", icon_pdf(2));
    let worktree = scratch.worktree();
    for change in changes_sorted(&worktree) {
        let diff = worktree.diff(&change).expect("diff");
        assert!(
            diff.picture.is_some(),
            "{} of a renamed PDF was not drawn",
            change.path
        );
    }
}

#[test]
fn pdf_trailer_extra() {
    // An extra trailer key changes nothing about the size.
    let diff = changed(
        "pdf-trailer",
        &icon_pdf(1),
        &pdf_with(&[(24, 24, &icon_ops(2))], "/Info << >> "),
    );
    assert_eq!(
        sizes(&diff).map(|(_, after)| after),
        Some(Some((Format::Pdf, 24, 24)))
    );
}

/// Byte replacement, for editing a generated PDF in place.
trait ReplaceBytes {
    fn replace_bytes(self, from: &str, to: &str) -> Vec<u8>;
}

impl ReplaceBytes for Vec<u8> {
    fn replace_bytes(self, from: &str, to: &str) -> Vec<u8> {
        let at = self
            .windows(from.len())
            .position(|window| window == from.as_bytes())
            .expect("the text to replace");
        [&self[..at], to.as_bytes(), &self[at + from.len()..]].concat()
    }
}

#[test]
fn zlib_zeros_decode() {
    for length in [1, 258, 259, 1_000, 100_000] {
        assert_eq!(
            vigia_core::inflated_length(&support::zlib_zeros(length), u64::MAX),
            Some(length),
            "{length} zeros"
        );
    }
}

/// This process's resident memory, in MiB, where `ps` can say.
fn resident() -> Option<u64> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let kib: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kib / 1024)
}

/// A PDF whose one object holds a gigabyte of zeros behind `dict`, read on
/// the frame path: it keeps `binary`, and memory stays where it was.
fn bomb_refused(name: &str, bomb: &[u8]) {
    let before = resident();
    let diff = changed(name, &icon_pdf(1), bomb);
    assert!(
        diff.picture.is_none(),
        "{name}: a PDF holding a bomb was sized"
    );
    if let (Some(before), Some(after)) = (before, resident()) {
        assert!(
            after < before + 128,
            "{name}: memory went from {before} MiB to {after} MiB"
        );
    }
}

const GIB: u64 = 1024 * 1024 * 1024;

#[test]
fn pdf_object_stream_bomb() {
    let zeros = zlib_zeros(GIB);
    for (name, dict, eol) in [
        (
            "plain",
            "/Type /ObjStm /N 1 /First 0 /Filter /FlateDecode",
            &b"\n"[..],
        ),
        (
            "escaped",
            "/Type/Obj#53tm /N 1 /First 0 /Filter/FlateDecode",
            b"\n",
        ),
        (
            "commented",
            "/Type % a note\n /ObjStm /N 1 /First 0 /Filter /FlateDecode",
            b"\n",
        ),
        (
            "carriage",
            "/Type /ObjStm /N 1 /First 0 /Filter /FlateDecode",
            b"\r",
        ),
        ("untyped", "/Filter /FlateDecode", b"\r\n"),
    ] {
        bomb_refused(&format!("pdf-bomb-{name}"), &pdf_holding(dict, eol, &zeros));
    }
    let updated = pdf_updated(&icon_pdf(2), "/Type /XRef /Filter /FlateDecode", &zeros);
    bomb_refused("pdf-bomb-update", &updated);
}

#[test]
fn pdf_image_stream_sized() {
    // An image the frame path never decodes: it is not measured there.
    let image = pdf_holding(
        "/Type /XObject /Subtype /Image /Width 8 /Height 8 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode",
        b"\n",
        &zlib_zeros(GIB),
    );
    let diff = changed("pdf-image-sized", &icon_pdf(1), &image);
    assert!(
        diff.picture.is_some(),
        "an image the frame path skips kept the file binary"
    );
}
