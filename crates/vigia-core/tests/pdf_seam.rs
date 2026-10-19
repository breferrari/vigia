//! A changed PDF whose size read panics draws what it drew before. One test,
//! because the seam is process-wide. `SPEC.md` §11.1.
#![cfg(feature = "pdf")]

mod support;

use support::{Scratch, changes_sorted, icon_pdf};

#[test]
fn pdf_header_panic() {
    let scratch = Scratch::new("pdf-seam");
    scratch.write("icon.pdf", icon_pdf(1));
    scratch.commit_all("initial");
    scratch.write("icon.pdf", icon_pdf(2));
    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    vigia_core::panic_next(vigia_core::Stage::Header);
    let diff = worktree.diff(&changes[0]).expect("diff");
    assert!(
        diff.picture.is_none(),
        "a PDF whose size read panicked was sized"
    );
}
