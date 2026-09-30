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

/// A path whose clean filter is an external program, as Git LFS configures it,
/// reads as binary: the blob is what the program wrote and the worktree is what
/// it read. The program is never run to find that out.
#[test]
fn skipped_driver_undiffable() {
    let scratch = Scratch::new("attr-driver");
    // Outside the worktree, so a run leaves no untracked file behind.
    let marker = scratch.root().with_extension("ran");
    let _ = std::fs::remove_file(&marker);
    let mark = marker.display().to_string().replace('\\', "/");
    scratch.git(&[
        "config",
        "filter.ptr.clean",
        &format!("cat >/dev/null; echo ran > '{mark}'; echo pointer"),
    ]);
    scratch.git(&["config", "filter.smudger.smudge", "cat"]);
    scratch.write(
        ".gitattributes",
        "a.csv filter=ptr\nb.csv filter=proc\nplain.txt filter=unset-driver\nsmudged.txt filter=smudger\n",
    );
    for name in ["a.csv", "b.csv", "plain.txt", "smudged.txt"] {
        scratch.write(name, numbered_lines(5));
    }
    scratch.commit_all("initial");
    // A process driver alone, the way LFS runs, configured after the commit so
    // git never has to start it.
    scratch.git(&["config", "filter.proc.process", "no-such-filter-process"]);
    for name in ["a.csv", "b.csv", "plain.txt", "smudged.txt"] {
        scratch.write(name, numbered_lines(6));
    }
    std::fs::remove_file(&marker).expect("git ran the driver to store the pointer");

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let diff_of = |name: &str| {
        let change = changes.iter().find(|c| c.path == name).expect("listed");
        (
            worktree.diff(change).expect("diff"),
            worktree.measure(change).expect("measure"),
        )
    };
    for name in ["a.csv", "b.csv"] {
        let (diff, span) = diff_of(name);
        assert!(
            diff.binary && diff.hunks.is_empty() && span.binary,
            "{name} diffed its content against what its driver stored: {} hunks",
            diff.hunks.len()
        );
    }
    // A driver with no clean or process program is a no-op in git.
    for name in ["plain.txt", "smudged.txt"] {
        let (diff, span) = diff_of(name);
        assert!(!diff.binary && !span.binary, "{name} read as binary");
        assert_eq!((diff.added, diff.removed), (1, 0), "{name}");
    }
    assert!(
        !marker.exists(),
        "the clean driver ran, which is a process per file per frame"
    );
}
