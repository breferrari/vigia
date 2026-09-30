//! What decides that a changed file is binary: its attributes first, then its bytes.

mod support;

use support::{Numstat, Scratch, changes_sorted, numbered_lines};

/// `binary` in `.gitattributes` makes a text file binary, the way git reports it.
#[test]
fn binary_attribute_wins() {
    let scratch = Scratch::new("attr-binary");
    scratch.write(".gitattributes", "a.txt binary\n");
    scratch.write("a.txt", numbered_lines(5));
    scratch.commit_all("initial");
    scratch.write("a.txt", numbered_lines(6));
    assert_eq!(
        scratch.git_numstat("a.txt"),
        Numstat::Binary,
        "the fixture is wrong"
    );

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let change = changes.iter().find(|c| c.path == "a.txt").expect("listed");
    let diff = worktree.diff(change).expect("diff");
    assert!(
        diff.binary,
        "git calls a.txt binary and this diffed it as text"
    );
    assert!(diff.hunks.is_empty(), "a binary file drew hunks");
}

/// `diff` set on bytes that sniff as binary diffs them as text, as git does.
#[test]
fn diff_attribute_wins() {
    let scratch = Scratch::new("attr-diff");
    scratch.write(".gitattributes", "*.bin diff\n");
    scratch.write("a.bin", "one\n\u{0}two\n");
    scratch.commit_all("initial");
    scratch.write("a.bin", "one\n\u{0}two\nthree\n");
    assert_eq!(
        scratch.git_numstat("a.bin"),
        Numstat::Lines(1, 0),
        "the fixture is wrong"
    );

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let change = changes.iter().find(|c| c.path == "a.bin").expect("listed");
    let diff = worktree.diff(change).expect("diff");
    assert!(
        !diff.binary,
        "git diffs a.bin as text and this called it binary"
    );
    assert_eq!((diff.added, diff.removed), (1, 0));
}
