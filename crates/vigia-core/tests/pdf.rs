//! What a changed PDF reads as on the frame path: its first page's size, or
//! what it drew before. `SPEC.md` §11.1.
#![cfg(feature = "pdf")]

mod support;

use support::{
    Scratch, changes_sorted, icon_ops, icon_pdf, locked_pdf, open_locked_pdf, pdf, pdf_doc,
    pdf_holding, pdf_with, pdf_xref_stream, pdf_xref_stream_with, zlib_zeros,
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
fn pdf_named_driver_draws() {
    // As Git for Windows sets for every PDF: no textconv runs here, so the
    // text diff would be the file's bytes.
    let scratch = Scratch::new("pdf-driver");
    scratch.write(".gitattributes", "*.pdf diff=astextplain\n");
    scratch.write("icon.pdf", icon_pdf(1));
    scratch.commit_all("initial");
    scratch.write("icon.pdf", icon_pdf(2));
    let side = Some((Format::Pdf, 24, 24));
    assert_eq!(sizes(&diff_of(&scratch, "icon.pdf")), Some((side, side)));
}

#[test]
fn pdf_named_driver_cut_binary() {
    // A side that gives no size falls back to what an unnamed PDF shows.
    let scratch = Scratch::new("pdf-driver-cut");
    scratch.write(".gitattributes", "*.pdf diff=astextplain\n");
    let whole = pdf_doc(
        &[(24, 24, &icon_ops(1))],
        "",
        1,
        &[b"<< /Length 3 >>\nstream\n\x00\x01\x02\nendstream"],
        "",
    );
    scratch.write("icon.pdf", &whole);
    scratch.commit_all("initial");
    scratch.write("icon.pdf", [&whole[..60], b"\0\0\0"].concat());
    let diff = diff_of(&scratch, "icon.pdf");
    assert!(
        diff.binary && diff.picture.is_none(),
        "a PDF under a driver that gave no size was not binary: {diff:?}"
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

const GIB: u64 = 1024 * 1024 * 1024;

#[test]
fn pdf_xref_stream_bomb() {
    // A cross-reference stream a reader decodes while it loads the file,
    // decoding to a gigabyte from under 1 MiB, past the frame path's limit.
    let zeros = zlib_zeros(140 << 20);
    let bomb = pdf_xref_stream(&zeros);
    assert!(
        bomb.len() < 1 << 20,
        "the bomb is past what the frame reads"
    );
    let before = support::peak_resident();
    let diff = changed("pdf-xref-bomb", &icon_pdf(1), &bomb);
    assert!(
        diff.picture.is_none(),
        "a PDF whose index decodes past the limit was sized"
    );
    if let (Some(before), Some(after)) = (before, support::peak_resident()) {
        // The limit, 16 MiB, and what holding it costs.
        assert!(
            after < before + 64,
            "memory went from {before} MiB to {after} MiB"
        );
    }
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
    // A comment between the name and its value does not hide the image.
    let commented = pdf_holding(
        "/Type /XObject /Subtype % an image\n /Image /Width 8 /Height 8 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode",
        b"\n",
        &zlib_zeros(GIB),
    );
    let diff = changed("pdf-image-commented", &icon_pdf(1), &commented);
    assert!(
        diff.picture.is_some(),
        "a comment hid an image from the frame path"
    );
}

#[test]
fn pdf_predictor_row_frame() {
    // An index stream whose rows claim 128 TiB each: the frame path goes on.
    let bomb = pdf_xref_stream_with(
        &zlib_zeros(64),
        "/DecodeParms << /Predictor 2 /BitsPerComponent 16 /Colors 1 /Columns 70368744177664 >> ",
    );
    let diff = changed("pdf-predictor-frame", &icon_pdf(1), &bomb);
    assert!(diff.binary, "{diff:?}");
}

#[test]
fn pdf_nested_limits() {
    // Three parses of 4 MiB of index each, inside one limit of 10 MiB.
    let doc = pdf_xref_stream(&zlib_zeros(4 << 20));
    let parse = || hayro_syntax::Pdf::new(std::sync::Arc::new(doc.clone())).is_ok();
    let inner = || hayro_syntax::with_decode_limit(64 << 20, parse);
    let outer = hayro_syntax::with_decode_limit(10 << 20, || [inner(), inner(), inner()]);
    assert!(
        outer.is_err(),
        "the inner limits did not count toward the outer one"
    );
    let parses = hayro_syntax::with_decode_limit(64 << 20, || [inner(), inner(), inner()]);
    assert!(
        parses.is_ok_and(|parses| parses.iter().all(Result::is_ok)),
        "the parses fail on their own"
    );
}

#[test]
fn pdf_fallback_capacity() {
    // One byte past a limit of 4096, a literal at a time and in one stored
    // block: the output's capacity never passes the limit.
    for (kind, data) in [
        ("literals", support::deflate_literals(4097)),
        ("stored", support::deflate_stored(4097)),
    ] {
        let (over, capacity) = hayro_syntax::fallback_capacity(4096, &data);
        assert!(over, "{kind}: 4097 bytes did not pass a limit of 4096");
        assert!(capacity <= 4096, "{kind}: the output grew to {capacity}");
    }
}
