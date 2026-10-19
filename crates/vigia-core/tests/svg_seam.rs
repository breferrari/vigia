//! A changed SVG whose root scan panics keeps its text diff. One test, because
//! the seam is process-wide. `SPEC.md` §11.1.

mod support;

use support::{Scratch, changes_sorted};

#[test]
fn svg_header_panic() {
    let root = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">"#;
    let scratch = Scratch::new("svg-seam");
    scratch.write(
        "icon.svg",
        format!("{root}<rect width=\"1\" height=\"1\"/></svg>"),
    );
    scratch.commit_all("initial");
    scratch.write(
        "icon.svg",
        format!("{root}<rect width=\"2\" height=\"1\"/></svg>"),
    );
    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    vigia_core::panic_next(vigia_core::Stage::Header);
    let diff = worktree.diff(&changes[0]).expect("diff");
    assert!(
        diff.picture.is_none() && !diff.hunks.is_empty(),
        "an SVG whose root scan panicked did not keep its text diff"
    );
}
