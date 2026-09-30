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
    assert!(
        worktree.measure(change).expect("measure").binary,
        "the height treats a.txt as text while its diff is binary"
    );
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
    assert!(
        !worktree.measure(change).expect("measure").binary,
        "the height treats a.bin as binary while its diff is text"
    );
}

/// A large binary file costs the sniff window to diff, not its size: the bytes
/// after the first NUL-bearing window are never read.
#[test]
fn binary_sniff_bounded() {
    let scratch = Scratch::new("attr-sniff");
    let mut big = vec![0u8; 1 << 20];
    big[0] = b'a';
    scratch.write("big.bin", &big);
    scratch.commit_all("initial");
    big[1] = b'b';
    scratch.write("big.bin", &big);

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let change = changes
        .iter()
        .find(|c| c.path == "big.bin")
        .expect("listed");
    let diff = worktree.diff(change).expect("diff");
    assert!(diff.binary, "a file of NULs was not called binary");
    assert!(
        diff.bytes <= 2 * 8000,
        "diffing a 1 MiB binary file compared {} bytes, where the sniff needs 8000",
        diff.bytes
    );
}

/// A path Git LFS stores reads as binary: the blob is a pointer and the
/// worktree is the content. Other drivers, and LFS paths with no stored
/// pointer, diff as text. No driver is run to find that out.
#[test]
fn lfs_undiffable() {
    let scratch = Scratch::new("attr-driver");
    // Outside the worktree, so a run leaves no untracked file behind.
    let marker = scratch.root().with_extension("ran");
    let _ = std::fs::remove_file(&marker);
    let mark = marker.display().to_string().replace('\\', "/");
    scratch.git(&[
        "config",
        "filter.lfs.clean",
        &format!("cat >/dev/null; echo ran > '{mark}'; echo pointer"),
    ]);
    scratch.git(&["config", "filter.strip.clean", "cat"]);
    scratch.git(&["config", "filter.smudger.smudge", "cat"]);
    scratch.write(
        ".gitattributes",
        "*.csv filter=lfs diff=lfs\nstripped.txt filter=strip\nplain.txt filter=unset-driver\nsmudged.txt filter=smudger\n",
    );
    let names = ["a.csv", "stripped.txt", "plain.txt", "smudged.txt"];
    for name in names {
        scratch.write(name, numbered_lines(5));
    }
    scratch.commit_all("initial");
    for name in names {
        scratch.write(name, numbered_lines(6));
    }
    // New under the LFS pattern: no pointer stored, so nothing to disagree with.
    scratch.write("new.csv", numbered_lines(3));
    // Git ran this clean program to store the pointer, unless an installed
    // git-lfs process took precedence. Either way the blob is a pointer.
    let _ = std::fs::remove_file(&marker);

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let diff_of = |name: &str| {
        let change = changes.iter().find(|c| c.path == name).expect("listed");
        (
            worktree.diff(change).expect("diff"),
            worktree.measure(change).expect("measure"),
        )
    };
    let (diff, span) = diff_of("a.csv");
    assert!(
        diff.binary && diff.hunks.is_empty() && span.binary,
        "a.csv diffed its content against the pointer LFS stored, `diff=lfs` and all: {} hunks",
        diff.hunks.len()
    );
    for (name, added) in [
        ("stripped.txt", 1),
        ("plain.txt", 1),
        ("smudged.txt", 1),
        ("new.csv", 3),
    ] {
        let (diff, span) = diff_of(name);
        assert!(!diff.binary && !span.binary, "{name} read as binary");
        assert_eq!((diff.added, diff.removed), (added, 0), "{name}");
    }
    assert!(
        !marker.exists(),
        "the clean driver ran, which is a process per file per frame"
    );
}

/// LFS as it usually runs, a long-lived process with no clean program.
#[test]
fn lfs_process_only() {
    let scratch = Scratch::new("attr-lfs-process");
    scratch.write(".gitattributes", "*.csv filter=lfs\n");
    scratch.write("a.csv", numbered_lines(5));
    scratch.commit_all("initial");
    // After the commit, so git never has to start it.
    scratch.git(&["config", "filter.lfs.process", "no-such-filter-process"]);
    scratch.write("a.csv", numbered_lines(6));

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let change = changes.iter().find(|c| c.path == "a.csv").expect("listed");
    assert!(
        worktree.diff(change).expect("diff").binary,
        "a process-only LFS path diffed as text"
    );
}

/// Git runs no clean filter on a symlink, so a link under an LFS pattern still
/// diffs its target.
#[test]
fn lfs_keeps_links() {
    let scratch = Scratch::new("attr-driver-link");
    scratch.git(&["config", "filter.lfs.clean", "cat >/dev/null; echo pointer"]);
    scratch.write(".gitattributes", "*.csv filter=lfs\n");
    if !support::committed_link(&scratch, "one.txt", "l.csv") {
        return;
    }
    std::fs::remove_file(scratch.path_of("l.csv")).expect("remove the link");
    assert!(support::made_link(&scratch, "two.txt", "l.csv"), "relink");

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let change = changes.iter().find(|c| c.path == "l.csv").expect("listed");
    let diff = worktree.diff(change).expect("diff");
    assert!(
        !diff.binary,
        "a link under a driver's pattern read as binary"
    );
    assert_eq!((diff.added, diff.removed), (1, 1));
}
