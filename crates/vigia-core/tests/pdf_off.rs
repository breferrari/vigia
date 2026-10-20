//! A build without the `pdf` feature, as `cargo install` makes one: a changed
//! PDF is `binary`, as it was before PDFs were drawn. `SPEC.md` §6.
#![cfg(not(feature = "pdf"))]

mod support;

use support::{Scratch, changes_sorted, icon_ops, pdf_doc};

/// An icon PDF holding `length` raw bytes in a stream, as most PDFs do.
fn binary_pdf(seed: u8, length: usize) -> Vec<u8> {
    let mut extra = format!("<< /Length {length} >>\nstream\n").into_bytes();
    extra.extend((0..length).map(|n| n as u8));
    extra.extend_from_slice(b"\nendstream");
    pdf_doc(&[(24, 24, &icon_ops(seed))], "", 1, &[&extra], "")
}

#[test]
fn pdf_feature_off_draws_binary() {
    // One the frame path would size itself, and one it would leave to the
    // worker.
    for length in [3, 2 << 20] {
        let scratch = Scratch::new("pdf-off");
        scratch.write("icon.pdf", binary_pdf(1, length));
        scratch.commit_all("initial");
        scratch.write("icon.pdf", binary_pdf(2, length));
        let worktree = scratch.worktree();
        let change = changes_sorted(&worktree)
            .into_iter()
            .find(|c| c.path == "icon.pdf")
            .expect("icon.pdf is listed");
        let diff = worktree.diff(&change).expect("diff");
        assert!(
            diff.picture.is_none(),
            "a build without PDFs made a picture of {length} bytes"
        );
        assert!(diff.binary, "a PDF in a build without PDFs is not binary");
    }
}

#[test]
fn pdf_feature_off_driver_binary() {
    // Git for Windows names a driver for every PDF. No textconv runs here, so
    // the text diff would be the file's bytes.
    let scratch = Scratch::new("pdf-off-driver");
    scratch.write(".gitattributes", "*.pdf diff=astextplain\n");
    scratch.write("icon.pdf", binary_pdf(1, 3));
    scratch.commit_all("initial");
    scratch.write("icon.pdf", binary_pdf(2, 3));
    let worktree = scratch.worktree();
    let change = changes_sorted(&worktree)
        .into_iter()
        .find(|c| c.path == "icon.pdf")
        .expect("icon.pdf is listed");
    let diff = worktree.diff(&change).expect("diff");
    assert!(diff.binary, "a PDF under a driver diffed as text: {diff:?}");
}
