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

/// A pointer under `filter=lfs` that status must read again, because its
/// timestamp moved after the index was written, with a driver that cannot be
/// spawned.
fn racy_pointer(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(
        ".gitattributes",
        "lfs/*.png filter=lfs diff=lfs merge=lfs -text\n",
    );
    let pointer = "version https://git-lfs.github.com/spec/v1\noid sha256:\
        4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393\nsize 12345\n";
    scratch.write("lfs/pointer.png", pointer);
    scratch.write("notes.txt", numbered_lines(3));
    scratch.commit_all("initial");
    scratch.git(&["config", "filter.lfs.process", "no-such-filter-process"]);
    // The same bytes again, so only the timestamp says it may have changed.
    std::thread::sleep(std::time::Duration::from_millis(20));
    scratch.write("lfs/pointer.png", pointer);
    scratch.write("notes.txt", numbered_lines(4));
    scratch
}

#[test]
fn racy_lfs_lists() {
    let scratch = racy_pointer("attr-racy-lfs");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame
        .advance()
        .expect("a racily clean LFS pointer with no driver to run stopped the walk");
    let paths: Vec<&str> = frame.files().iter().map(|c| c.path.as_str()).collect();
    assert_eq!(
        paths,
        ["notes.txt"],
        "the walk did not list exactly the changed file"
    );
}

/// An LFS pointer to `object` committed as `a.png`, with `worktree` written
/// where it was after the index, and `object` in the local store when `stored`.
/// `notes.txt` changes beside it, so a walk always has one change to list.
fn lfs_content(name: &str, object: &[u8], worktree: &[u8], stored: bool) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(
        ".gitattributes",
        "*.png filter=lfs diff=lfs merge=lfs -text\n",
    );
    let (oid, pointer) = support::lfs_pointer(object, 0xab);
    scratch.write("a.png", pointer);
    scratch.write("notes.txt", numbered_lines(3));
    scratch.commit_all("initial");
    scratch.git(&["config", "filter.lfs.process", "no-such-filter-process"]);
    if stored {
        scratch.write(
            &format!(".git/lfs/objects/{}/{}/{oid}", &oid[0..2], &oid[2..4]),
            object,
        );
    }
    std::thread::sleep(std::time::Duration::from_millis(20));
    scratch.write("a.png", worktree);
    scratch.write("notes.txt", numbered_lines(4));
    scratch
}

/// The paths one walk of `scratch` lists.
fn listed(scratch: &Scratch) -> Vec<String> {
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    frame.files().iter().map(|c| c.path.clone()).collect()
}

#[test]
fn racy_driver_not_run() {
    let scratch = Scratch::new("attr-racy-driver");
    let marker = scratch.root().join("ran");
    scratch.write(".gitattributes", "*.csv filter=marker\n");
    scratch.write("a.csv", numbered_lines(3));
    scratch.write("notes.txt", numbered_lines(3));
    scratch.commit_all("initial");
    // After the commit, so only a status read could start it.
    scratch.git(&[
        "config",
        "filter.marker.clean",
        &format!("touch '{}'; cat", marker.display()),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(20));
    scratch.write("a.csv", numbered_lines(3));
    scratch.write("notes.txt", numbered_lines(4));
    assert_eq!(
        listed(&scratch),
        ["notes.txt"],
        "the walk listed the wrong files"
    );
    assert!(!marker.exists(), "the status walk ran the clean driver");
}

#[test]
fn racy_lfs_content_unchanged() {
    let object = b"real image bytes\n".repeat(10);
    let scratch = lfs_content("attr-lfs-same", &object, &object, true);
    assert_eq!(
        listed(&scratch),
        ["notes.txt"],
        "content equal to its LFS object listed as changed"
    );
}

#[test]
fn racy_lfs_content_changed() {
    let object = b"real image bytes\n".repeat(10);
    let mut edited = object.clone();
    edited[5] = b'X';
    let scratch = lfs_content("attr-lfs-edited", &object, &edited, true);
    assert_eq!(
        listed(&scratch),
        ["a.png", "notes.txt"],
        "content that differs from its LFS object did not list"
    );
}

#[test]
fn racy_lfs_no_object() {
    let object = b"real image bytes\n".repeat(10);
    let scratch = lfs_content("attr-lfs-unfetched", &object, &object, false);
    assert_eq!(
        listed(&scratch),
        ["a.png", "notes.txt"],
        "content whose object is not fetched did not list"
    );
}

#[test]
fn racy_lfs_large_listed() {
    let object = vec![7u8; 16 * 1024 * 1024 + 1];
    let scratch = lfs_content("attr-lfs-large", &object, &object, true);
    assert_eq!(
        listed(&scratch),
        ["a.png", "notes.txt"],
        "a file over the compare bound did not list"
    );
}

#[test]
fn racy_lfs_size_first() {
    let object = b"real image bytes\n".repeat(10);
    let longer = b"real image bytes\n".repeat(11);
    let scratch = lfs_content("attr-lfs-longer", &object, &longer, true);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert!(
        frame.files().iter().any(|c| c.path == "a.png"),
        "a length other than the pointer's did not list"
    );
    assert_eq!(
        worktree.lfs_compares(),
        0,
        "a length mismatch still read the object"
    );
}

#[test]
fn racy_lfs_compare_cached() {
    let object = b"real image bytes\n".repeat(10);
    let scratch = lfs_content("attr-lfs-cached", &object, &object, true);
    settle(&scratch, "a.png");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    frame.advance().expect("advance again");
    assert_eq!(
        worktree.lfs_compares(),
        1,
        "an unchanged fingerprint compared the file again"
    );
}

#[test]
fn driver_after_reload() {
    let scratch = Scratch::new("attr-driver-reload");
    let marker = scratch.root().join("ran");
    scratch.write(".gitattributes", "*.csv filter=marker\n");
    scratch.write("a.csv", numbered_lines(3));
    scratch.write("notes.txt", numbered_lines(3));
    scratch.commit_all("initial");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    // Named after the walk opened, so only a reloaded walk could start it.
    scratch.git(&[
        "config",
        "filter.marker.clean",
        &format!("touch '{}'; cat", marker.display()),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(20));
    scratch.write("a.csv", numbered_lines(3));
    scratch.write("notes.txt", numbered_lines(4));
    frame.advance().expect("advance after the config changed");
    let paths: Vec<&str> = frame.files().iter().map(|c| c.path.as_str()).collect();
    assert_eq!(paths, ["notes.txt"], "the walk listed the wrong files");
    assert!(
        !marker.exists(),
        "a driver named after the walk opened ran on the reloaded walk"
    );
}

/// Move `rela`'s modification time well behind the settle margin, which a write
/// a moment ago has not reached.
fn settle(scratch: &Scratch, rela: &str) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(scratch.path_of(rela))
        .expect("open to settle");
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    file.set_modified(past).expect("set the modification time");
}

#[test]
fn racy_lfs_unsettled() {
    let object = b"real image bytes\n".repeat(10);
    let scratch = lfs_content("attr-lfs-unsettled", &object, &object, true);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert!(
        frame.settles_in(std::time::SystemTime::now()).is_some(),
        "a verdict on an unsettled fingerprint armed no wake for when it settles"
    );
    frame.advance().expect("advance again");
    assert_eq!(
        worktree.lfs_compares(),
        2,
        "a verdict taken on a fingerprint inside the settle margin was kept"
    );
}

#[cfg(unix)]
#[test]
fn lfs_mode_change() {
    use std::os::unix::fs::PermissionsExt;
    let object = b"real image bytes\n".repeat(10);
    let scratch = lfs_content("attr-lfs-mode", &object, &object, true);
    std::fs::set_permissions(
        scratch.path_of("a.png"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("make it executable");
    assert_eq!(
        listed(&scratch),
        ["a.png", "notes.txt"],
        "a mode change on an LFS file equal to its object was dropped"
    );
}

#[test]
fn lfs_attribute_removed() {
    let object = b"real image bytes\n".repeat(10);
    let scratch = lfs_content("attr-lfs-unfiltered", &object, &object, true);
    settle(&scratch, "a.png");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert!(
        frame.files().iter().all(|c| c.path != "a.png"),
        "the setup listed the LFS file"
    );
    scratch.write(".gitattributes", "*.txt text\n");
    frame.advance().expect("advance after the attribute went");
    assert!(
        frame.files().iter().any(|c| c.path == "a.png"),
        "a file no longer under filter=lfs stayed hidden by its cached verdict"
    );
}

#[test]
fn lfs_walk_budget() {
    let first = vec![1u8; 10 * 1024 * 1024];
    let second = vec![2u8; 10 * 1024 * 1024];
    let scratch = Scratch::new("attr-lfs-budget");
    scratch.write(
        ".gitattributes",
        "*.png filter=lfs diff=lfs merge=lfs -text\n",
    );
    let objects = [("a.png", &first, 0xa1), ("b.png", &second, 0xb2)];
    for (rela, object, seed) in objects {
        let (_, pointer) = support::lfs_pointer(object, seed);
        scratch.write(rela, pointer);
    }
    scratch.commit_all("initial");
    scratch.git(&["config", "filter.lfs.process", "no-such-filter-process"]);
    for (rela, object, seed) in objects {
        let (oid, _) = support::lfs_pointer(object, seed);
        scratch.write(
            &format!(".git/lfs/objects/{}/{}/{oid}", &oid[0..2], &oid[2..4]),
            object,
        );
        scratch.write(rela, object);
        settle(&scratch, rela);
    }
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(
        frame.files().len(),
        1,
        "one walk compared more than its 16 MiB budget"
    );
    let now = std::time::SystemTime::now();
    assert_eq!(
        frame.settles_in(now),
        Some(std::time::Duration::ZERO),
        "a file the walk could not afford armed no wake to compare it"
    );
    frame.advance_if_settled(now).expect("the wake's walk");
    assert!(
        frame.files().is_empty(),
        "the wake did not compare the file the first walk could not afford"
    );
}

#[test]
fn lfs_verdicts_pruned() {
    let object = b"real image bytes\n".repeat(10);
    let scratch = lfs_content("attr-lfs-pruned", &object, &object, true);
    settle(&scratch, "a.png");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(worktree.lfs_cached(), 1, "the setup kept no verdict");
    // Back to the stored pointer, which the walk no longer flags. Written
    // rather than checked out, since checkout would run the broken driver.
    scratch.write("a.png", support::lfs_pointer(&object, 0xab).1);
    frame.advance().expect("advance after the restore");
    assert_eq!(
        worktree.lfs_cached(),
        0,
        "a verdict outlived the file the walk stopped flagging"
    );
}

#[test]
fn lfs_large_blob_unread() {
    let scratch = Scratch::new("attr-lfs-large-blob");
    scratch.write(
        ".gitattributes",
        "*.png filter=lfs diff=lfs merge=lfs -text\n",
    );
    scratch.write("big.txt", numbered_lines(400));
    scratch.commit_all("initial");
    scratch.git(&["config", "filter.lfs.process", "no-such-filter-process"]);
    scratch.write("big.txt", numbered_lines(401));
    assert_eq!(
        listed(&scratch),
        ["big.txt"],
        "the edited file did not list"
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(
        worktree.lfs_blob_reads(),
        0,
        "a stored blob too large to be an LFS pointer was read to find out"
    );
}

#[test]
fn lfs_wakes_stop_when_decided() {
    let scratch = Scratch::new("attr-lfs-wakes");
    scratch.write(
        ".gitattributes",
        "*.png filter=lfs diff=lfs merge=lfs -text\n",
    );
    let objects: Vec<(String, Vec<u8>, u8)> = (0..3u8)
        .map(|n| (format!("{n}.png"), vec![n; 10 * 1024 * 1024], 0xc0 + n))
        .collect();
    for (rela, object, seed) in &objects {
        let (_, pointer) = support::lfs_pointer(object, *seed);
        scratch.write(rela, pointer);
    }
    scratch.commit_all("initial");
    scratch.git(&["config", "filter.lfs.process", "no-such-filter-process"]);
    for (rela, object, seed) in &objects {
        let (oid, _) = support::lfs_pointer(object, *seed);
        scratch.write(
            &format!(".git/lfs/objects/{}/{}/{oid}", &oid[0..2], &oid[2..4]),
            object,
        );
        scratch.write(rela, object);
        settle(&scratch, rela);
    }
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    // One file a walk, each walk armed by the one before it.
    for walk in 1..objects.len() {
        let now = std::time::SystemTime::now();
        assert!(
            frame.settles_in(now).is_some(),
            "walk {walk} left files undecided and armed no wake"
        );
        frame.advance_if_settled(now).expect("the wake's walk");
    }
    assert!(frame.files().is_empty(), "a file was left undecided");
    assert_eq!(
        frame.settles_in(std::time::SystemTime::now()),
        None,
        "a wake stayed armed once every LFS file was decided"
    );
}
