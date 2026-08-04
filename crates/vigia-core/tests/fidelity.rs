//! Does `gix` read a working tree the way git wrote it?
//!
//! This is the open question `SPEC.md` §10 puts on Phase 1, and it gates the
//! whole stack. Every test here builds its fixture with real `git` and then
//! reads it back through `vigia-core`.

mod support;

use support::{Numstat, Scratch, changes_sorted, committed_link, made_link, numbered_lines};
use vigia_core::{ChangeKind, ChangeOptions, LineKind};

#[test]
fn clean_worktree_yields_no_changes() {
    let scratch = Scratch::new("clean");
    scratch.write("a.txt", "hello\n");
    scratch.commit_all("initial");

    assert_eq!(changes_sorted(&scratch.worktree()), Vec::new());
}

#[test]
fn untracked_file_is_added_and_diffs_as_all_additions() {
    let scratch = Scratch::new("added");
    scratch.write("tracked.txt", "kept\n");
    scratch.commit_all("initial");
    scratch.write("fresh.txt", "one\ntwo\n");

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);

    assert_eq!(changes.len(), 1, "only the new file should be reported");
    assert_eq!(changes[0].path, "fresh.txt");
    assert_eq!(changes[0].kind, ChangeKind::Added);

    let diff = worktree.diff(&changes[0]).expect("diff added file");
    assert_eq!((diff.added, diff.removed), (2, 0));
    assert!(
        diff.hunks[0]
            .lines
            .iter()
            .all(|l| l.kind == LineKind::Added),
        "a new file has no context and nothing removed"
    );
}

#[test]
fn modified_file_reports_only_the_changed_lines() {
    let scratch = Scratch::new("modified");
    scratch.write("a.txt", numbered_lines(20));
    scratch.commit_all("initial");
    scratch.write(
        "a.txt",
        numbered_lines(20).replace("line 10\n", "CHANGED\n"),
    );

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::Modified);

    let diff = worktree.diff(&changes[0]).expect("diff modified file");
    assert_eq!(
        (diff.added, diff.removed),
        (1, 1),
        "one line changed, so exactly one added and one removed"
    );
    assert_eq!(diff.hunks.len(), 1);
}

#[test]
fn deleted_file_reports_every_line_removed() {
    let scratch = Scratch::new("removed");
    scratch.write("gone.txt", "one\ntwo\nthree\n");
    scratch.commit_all("initial");
    scratch.remove("gone.txt");

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, ChangeKind::Removed);

    let diff = worktree.diff(&changes[0]).expect("diff removed file");
    assert_eq!((diff.added, diff.removed), (0, 3));
}

#[test]
fn a_moved_file_is_reported_as_one_rename() {
    let scratch = Scratch::new("renamed");
    scratch.write("old/name.txt", numbered_lines(30));
    scratch.commit_all("initial");
    scratch.git(&["mv", "old/name.txt", "new-name.txt"]);
    // `git mv` stages the move; the monitor watches the worktree, so unstage
    // it and leave the rename sitting in the working tree where we can see it.
    scratch.git(&["reset", "-q"]);

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);

    let renamed = changes
        .iter()
        .find(|c| matches!(c.kind, ChangeKind::Renamed { .. }))
        .unwrap_or_else(|| panic!("expected a rename, got {changes:#?}"));
    assert_eq!(renamed.path, "new-name.txt");
    assert_eq!(
        renamed.kind,
        ChangeKind::Renamed {
            from: "old/name.txt".to_owned()
        }
    );
}

#[test]
fn disabling_rename_tracking_splits_the_move_into_two_changes() {
    let scratch = Scratch::new("renames-off");
    scratch.write("old/name.txt", numbered_lines(30));
    scratch.commit_all("initial");
    scratch.git(&["mv", "old/name.txt", "new-name.txt"]);
    scratch.git(&["reset", "-q"]);

    let worktree = scratch.worktree();
    let mut kinds: Vec<ChangeKind> = worktree
        .changes_with(ChangeOptions {
            track_renames: false,
        })
        .expect("enumerate without rename tracking")
        .map(|c| c.expect("change").kind)
        .collect();
    kinds.sort_by_key(|k| format!("{k:?}"));

    assert_eq!(
        kinds,
        vec![ChangeKind::Added, ChangeKind::Removed],
        "without rename tracking a move is a deletion plus an addition"
    );
}

#[test]
fn binary_content_is_flagged_and_not_diffed() {
    let scratch = Scratch::new("binary");
    scratch.write("blob.bin", [0x00, 0x01, 0x02, 0xff, 0x00]);
    scratch.commit_all("initial");
    scratch.write("blob.bin", [0x00, 0x09, 0x02, 0xfe, 0x00, 0x07]);

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    assert_eq!(changes.len(), 1);

    let diff = worktree.diff(&changes[0]).expect("diff binary file");
    assert!(diff.binary, "content with NUL bytes must sniff as binary");
    assert!(diff.hunks.is_empty(), "binary files get no hunks");
    assert_eq!((diff.added, diff.removed), (0, 0));
}

#[test]
fn ignored_files_stay_invisible() {
    let scratch = Scratch::new("ignored");
    scratch.write(".gitignore", "secret.txt\n");
    scratch.commit_all("initial");
    scratch.write("secret.txt", "do not show me\n");
    scratch.write("visible.txt", "show me\n");

    let paths: Vec<String> = changes_sorted(&scratch.worktree())
        .into_iter()
        .map(|c| c.path)
        .collect();

    assert_eq!(paths, vec!["visible.txt".to_owned()]);
}

#[test]
fn nested_paths_use_forward_slashes_on_every_platform() {
    let scratch = Scratch::new("separators");
    scratch.write("keep.txt", "x\n");
    scratch.commit_all("initial");
    scratch.write("src/deep/nested.txt", "y\n");

    let changes = changes_sorted(&scratch.worktree());
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, "src/deep/nested.txt");
}

#[test]
fn hunk_headers_match_what_git_reports() {
    let scratch = Scratch::new("headers");
    scratch.write("a.txt", numbered_lines(40));
    scratch.commit_all("initial");

    // Lines 5 and 8 are close enough that their context windows overlap, so
    // they belong to one hunk. Line 30 is far away and must get its own.
    let edited = numbered_lines(40)
        .replace("line 5\n", "FIVE\n")
        .replace("line 8\n", "EIGHT\n")
        .replace("line 30\n", "THIRTY\n");
    scratch.write("a.txt", &edited);

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let diff = worktree.diff(&changes[0]).expect("diff");

    let ours: Vec<(u32, u32, u32, u32)> = diff
        .hunks
        .iter()
        .map(|h| (h.old_start, h.old_lines, h.new_start, h.new_lines))
        .collect();

    assert_eq!(
        ours,
        scratch.git_hunk_headers("a.txt"),
        "hunk boundaries must agree with git exactly"
    );
    assert_eq!(diff.hunks.len(), 2, "two edits far apart, two hunks");
}

/// Where exactly two edits stop sharing a hunk.
///
/// Sweeping the gap rather than picking one value is deliberate: a single
/// fixture with a comfortable gap passes no matter what the merge threshold
/// is, so it tests nothing. Git is the oracle at every step.
#[test]
fn hunk_grouping_boundary_matches_git_at_every_gap() {
    const TOTAL_LINES: usize = 60;
    const FIRST_EDIT: u32 = 20;

    for gap in 0..=10u32 {
        let scratch = Scratch::new(&format!("gap-{gap}"));
        scratch.write("a.txt", numbered_lines(TOTAL_LINES));
        scratch.commit_all("initial");

        // `gap` unchanged lines sit between the two edits.
        let second_edit = FIRST_EDIT + gap + 1;
        let edited = numbered_lines(TOTAL_LINES)
            .replace(&format!("line {FIRST_EDIT}\n"), "FIRST\n")
            .replace(&format!("line {second_edit}\n"), "SECOND\n");
        scratch.write("a.txt", &edited);

        let worktree = scratch.worktree();
        let changes = changes_sorted(&worktree);
        let diff = worktree.diff(&changes[0]).expect("diff");

        let ours: Vec<(u32, u32, u32, u32)> = diff
            .hunks
            .iter()
            .map(|h| (h.old_start, h.old_lines, h.new_start, h.new_lines))
            .collect();

        assert_eq!(
            ours,
            scratch.git_hunk_headers("a.txt"),
            "disagreed with git when {gap} unchanged lines separate the edits"
        );
    }
}

#[test]
fn rendered_lines_carry_no_line_terminators() {
    let scratch = Scratch::new("terminators");
    scratch.write("crlf.txt", "alpha\r\nbeta\r\ngamma\r\n");
    scratch.commit_all("initial");
    scratch.write("crlf.txt", "alpha\r\nBETA\r\ngamma\r\n");

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let diff = worktree.diff(&changes[0]).expect("diff");

    for line in diff.hunks.iter().flat_map(|h| &h.lines) {
        assert!(
            !line.text.contains('\r') && !line.text.contains('\n'),
            "line text {:?} still carries a terminator",
            line.text
        );
    }
    assert!(
        diff.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|l| l.kind == LineKind::Added && l.text == "BETA")
    );
}

#[test]
fn a_file_without_a_trailing_newline_still_diffs() {
    let scratch = Scratch::new("no-eof-newline");
    scratch.write("a.txt", "one\ntwo");
    scratch.commit_all("initial");
    scratch.write("a.txt", "one\nTWO");

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let diff = worktree.diff(&changes[0]).expect("diff");

    assert_eq!((diff.added, diff.removed), (1, 1));
    assert!(
        diff.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|l| l.kind == LineKind::Added && l.text == "TWO")
    );
}

#[test]
fn one_unreadable_path_does_not_end_the_stream() {
    let scratch = Scratch::new("resilience");
    scratch.write("a.txt", "one\n");
    scratch.write("b.txt", "two\n");
    scratch.commit_all("initial");
    scratch.write("a.txt", "one changed\n");
    scratch.write("b.txt", "two changed\n");

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    assert_eq!(changes.len(), 2);

    // Delete one file after enumeration, which is exactly what the agent in
    // the other pane does. Diffing it must degrade, not fail.
    scratch.remove("a.txt");
    for change in &changes {
        worktree
            .diff(change)
            .unwrap_or_else(|e| panic!("diffing {} after deletion failed: {e}", change.path));
    }
}

/// The line count, against the file on disk rather than against the arithmetic
/// restated.
///
/// `FileDiff::lines` is what the heat strip projects hunk positions across, so
/// getting it wrong misplaces every mark on the strip rather than failing
/// loudly. The oracle is the bytes the fixture wrote: a file's line count is
/// something a test can know independently, which is the whole reason to check
/// it here rather than only beside `compute`.
///
/// Three kinds in one test on purpose. A modification, an addition and a
/// deletion are the three shapes of working-tree side there are, and the wrong
/// implementations (count the index side, count both, count the hunks) each
/// pass for at least one of them alone.
#[test]
fn the_line_count_matches_the_file_on_disk() {
    let scratch = Scratch::new("line-count");
    scratch.write("modified.txt", numbered_lines(40));
    scratch.write("removed.txt", numbered_lines(9));
    scratch.commit_all("initial");

    // Shorter than it was, so counting the index side gives 40 and counting
    // both gives 52.
    let shrunk = numbered_lines(12);
    scratch.write("modified.txt", &shrunk);
    let created = "alpha\nbeta\ngamma\n";
    scratch.write("added.txt", created);
    std::fs::remove_file(scratch.path_of("removed.txt")).expect("delete the fixture file");

    let worktree = scratch.worktree();
    let counted = |text: &str| text.lines().count() as u32;

    for change in changes_sorted(&worktree) {
        let diff = worktree.diff(&change).expect("diff");
        let expected = match diff.path.as_str() {
            "modified.txt" => counted(&shrunk),
            "added.txt" => counted(created),
            // No working-tree side left to locate anything within.
            "removed.txt" => 0,
            other => panic!("unexpected path {other}"),
        };
        assert_eq!(
            diff.lines, expected,
            "{} reported {} lines against {expected} on disk",
            diff.path, diff.lines
        );
    }
}

/// A file whose length in *bytes* did not change but whose length in *lines*
/// did.
///
/// The pair that separates a line count from a byte count, and the fixture the
/// frame path's cache-key gate uses for the same reason. Nine bytes either way;
/// three lines against one.
#[test]
fn a_line_count_is_not_a_byte_count() {
    let scratch = Scratch::new("line-count-bytes");
    scratch.write("f.txt", "ab\ncd\nef\n");
    scratch.commit_all("initial");
    scratch.write("f.txt", "abcdefgh\n");

    let worktree = scratch.worktree();
    let change = changes_sorted(&worktree)
        .into_iter()
        .find(|c| c.path == "f.txt")
        .expect("the rewritten file is changed");
    let diff = worktree.diff(&change).expect("diff");

    assert_eq!(
        std::fs::metadata(scratch.path_of("f.txt"))
            .expect("stat")
            .len(),
        9,
        "the fixture no longer holds the byte length constant, so it is not \
         testing what it says"
    );
    assert_eq!(diff.lines, 1);
}

/// A recomputed diff does not leave last tick's height behind.
///
/// `Frame::height` fills a span for every changed file, and `Frame::diff` then
/// recomputes whichever file the viewport reaches. Until 2026-08-02 the
/// recompute replaced the cached diff and left the span alone, so a frame that
/// counted before it drew reported the file's **old** height beside its new
/// rows: the scrollbar's total and the content disagreed inside one frame, and
/// a drag resolved through that total landed on the wrong row.
///
/// Free to fix, which is what separates it from [#84]: the fresh diff is
/// already in hand, so the span rebuilds from it without reading a byte. #84 is
/// the other half, a file that changed and has *not* been re-diffed, and that
/// one needs a read to notice.
#[test]
fn a_recomputed_diff_invalidates_its_span() {
    let scratch = Scratch::new("span-invalidation");
    scratch.write("a.txt", "one\ntwo\nthree\n");
    scratch.commit_all("base");
    scratch.write("a.txt", "one\nchanged\nthree\n");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    let rows = |_: &vigia_core::FileChange, span: &vigia_core::FileSpan| {
        1 + span.hunks as usize + span.lines as usize
    };
    let before = frame.height(rows).expect("height");

    // Grow the file, then re-diff it the way a viewport reaching it would.
    scratch.write("a.txt", "one\nchanged\nthree\nfour\nfive\nsix\nseven\n");
    frame.diff(0).expect("diff");

    let after = frame.rows_of(0, rows).expect("rows_of");
    let drawn = {
        let (_, diff) = frame.diff(0).expect("diff");
        1 + diff.hunks.len() + diff.hunks.iter().map(|h| h.lines.len()).sum::<usize>()
    };
    assert_eq!(
        after, drawn,
        "the span survived the recompute: it reports {after} rows where the diff \
         drawn in the same frame is {drawn}"
    );
    assert_ne!(
        before, after,
        "the fixture did not change the file's height, so this proves nothing"
    );
}

// ---------------------------------------------------------------------------
// Symlinks. Git stores one as a mode `120000` blob whose *content is the target
// path*, so every gate below is about reading the link rather than through it:
// [#15](https://github.com/breferrari/vigia/issues/15).
//
// `SPEC.md` §7 names `core.symlinks` as one of the axes every fixture in this
// repository had silently taken a position on. These span it.
// ---------------------------------------------------------------------------

/// A target large enough and binary enough that reading *through* a link to it
/// is unmistakable rather than a subtle count difference.
///
/// Under the defect this file's 4 KiB of NUL bytes sniffs binary, so a diff that
/// read through the link reports `binary: true` and no hunks at all. Under the
/// fix the compared bytes are a twelve-character path. One fixture, three
/// independent assertions: the binary flag, the byte count, and the text.
fn binary_target() -> Vec<u8> {
    vec![0u8; 4096]
}

/// Every line of one kind, in order, so an assertion names texts rather than
/// indices into a hunk.
fn texts(diff: &vigia_core::FileDiff, kind: LineKind) -> Vec<&str> {
    diff.hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .filter(|line| line.kind == kind)
        .map(|line| line.text.as_str())
        .collect()
}

#[test]
fn a_repointed_symlink_diffs_as_its_target_path_not_its_target_contents() {
    let scratch = Scratch::new("symlink-repoint");
    scratch.write("target_a.txt", "AAAAAAAA\n");
    scratch.write("target_b.txt", binary_target());
    if !committed_link(&scratch, "target_a.txt", "link.txt") {
        return;
    }

    assert!(scratch.symlink_file("target_b.txt", "link.txt"));

    // git as the oracle: one line of path replaced by another, not 4 KiB of
    // binary. Asserted before vigia is asked anything, so a fixture that
    // stopped exercising the case fails here rather than passing quietly.
    assert_eq!(
        scratch.git_numstat("link.txt"),
        Numstat::Lines(1, 1),
        "the fixture no longer repoints a link, so nothing below is about #15"
    );

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    assert_eq!(changes.len(), 1, "only the link should be reported");
    assert_eq!(changes[0].path, "link.txt");
    assert_eq!(changes[0].kind, ChangeKind::Modified);

    let diff = worktree.diff(&changes[0]).expect("diff the repointed link");

    assert!(
        !diff.binary,
        "the diff sniffed binary, so it read the 4 KiB target through the link \
         rather than reading the link"
    );
    assert_eq!(
        (diff.added, diff.removed),
        (1, 1),
        "git reports one path replaced by another"
    );
    assert_eq!(texts(&diff, LineKind::Removed), ["target_a.txt"]);
    assert_eq!(
        texts(&diff, LineKind::Added),
        ["target_b.txt"],
        "the added side is the target's contents rather than its path"
    );
    assert_eq!(
        diff.bytes,
        ("target_a.txt".len() + "target_b.txt".len()) as u64,
        "the byte count follows the target file rather than the link text"
    );
}

#[test]
fn a_symlink_to_a_nested_path_reports_forward_slashes() {
    let scratch = Scratch::new("symlink-nested");
    scratch.write("dir/target.txt", "X\n");
    scratch.write("dir/other.txt", "Y\n");
    if !committed_link(&scratch, "dir/target.txt", "nested.txt") {
        return;
    }

    assert!(scratch.symlink_file("dir/other.txt", "nested.txt"));
    assert_eq!(scratch.git_numstat("nested.txt"), Numstat::Lines(1, 1));

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    assert_eq!(changes.len(), 1);

    let diff = worktree.diff(&changes[0]).expect("diff the nested link");

    // The separator gate. A Windows reparse point stores `dir\other.txt` where
    // git stores `dir/other.txt`, so without the conversion in `link_target`
    // this reads as changed on one tier-1 target and unchanged on the other
    // two, which is exactly the class of defect
    // [#65](https://github.com/breferrari/vigia/issues/65) was.
    assert_eq!(texts(&diff, LineKind::Removed), ["dir/target.txt"]);
    assert_eq!(
        texts(&diff, LineKind::Added),
        ["dir/other.txt"],
        "a link target reached the diff with the platform's separator in it"
    );
}

#[test]
fn a_broken_symlink_diffs_as_its_target_path() {
    let scratch = Scratch::new("symlink-broken");
    scratch.write("target_a.txt", "AAAAAAAA\n");
    if !committed_link(&scratch, "target_a.txt", "link.txt") {
        return;
    }

    assert!(scratch.symlink_file("gone.txt", "link.txt"));
    // Non-vacuity: a link that still resolves would make this the ordinary
    // repoint above rather than the dangling case.
    assert!(
        !scratch.path_of("gone.txt").exists(),
        "the target exists, so this link is not broken and proves nothing"
    );
    assert_eq!(scratch.git_numstat("link.txt"), Numstat::Lines(1, 1));

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    assert_eq!(changes.len(), 1);

    // `fs::read` on a dangling link is `NotFound`, which `read_worktree` treats
    // as a deleted file and reports empty. So under the defect this is not a
    // wrong diff but a *missing* one: the whole link reads as removed.
    let diff = worktree.diff(&changes[0]).expect("diff the broken link");
    assert_eq!(texts(&diff, LineKind::Removed), ["target_a.txt"]);
    assert_eq!(
        texts(&diff, LineKind::Added),
        ["gone.txt"],
        "a dangling link read as an empty file rather than as its target path"
    );
}

#[test]
fn an_untracked_symlink_is_added_as_its_target_path() {
    // **The second of `maybe_symlink`'s three arms**, and until this it was
    // unexercised: every other symlink gate here commits the link first, so all
    // of them arrive as `Item::Modification` and are classified off the *index*
    // entry's mode. An untracked link has no index entry at all and is
    // classified off the dirwalk entry's `disk_kind` instead, which is a
    // different `gix` field on a different code path.
    //
    // Not a hypothetical population: a link an agent has just created is exactly
    // what a monitor is pointed at.
    let scratch = Scratch::new("symlink-untracked");
    scratch.write("keep.txt", "tracked\n");
    scratch.write("target_b.txt", binary_target());
    scratch.commit_all("initial");

    if !made_link(&scratch, "target_b.txt", "fresh.txt") {
        return;
    }

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    assert_eq!(
        changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        ["fresh.txt"],
        "the untracked link is not the only change, so the diff below is not it"
    );
    assert_eq!(changes[0].kind, ChangeKind::Added);

    let diff = worktree.diff(&changes[0]).expect("diff the untracked link");
    assert!(
        !diff.binary,
        "the diff sniffed binary, so an untracked link was read through to its \
         4 KiB target: `disk_kind` did not classify it"
    );
    assert_eq!(
        texts(&diff, LineKind::Added),
        ["target_b.txt"],
        "an untracked link was added as its target's contents"
    );

    // The oracle, taken after the measurement so staging cannot affect it: git
    // stores this link as a mode 120000 blob, and its content is the path above.
    scratch.git(&["add", "fresh.txt"]);
    assert_eq!(scratch.index_mode("fresh.txt"), "120000");
}

#[test]
fn swapping_a_symlink_for_a_regular_file_and_back_agrees_with_git() {
    // **The boundary, and it is also the premise `FileChange::maybe_symlink`
    // rests on**, which is why it asserts what git calls each direction rather
    // than only what `vigia` does with it. The hint reads the *index* entry's
    // mode, so it is only sound while a working tree that disagrees with the
    // index about a file's **type** is something git reports as a type change
    // rather than as a modification.
    //
    // Measured here rather than assumed, because the two directions are not
    // symmetric and only one of them was obvious.
    let scratch = Scratch::new("symlink-typechange");
    scratch.write("target.txt", "AAAA\n");
    scratch.write("swap.txt", "plain content\n");
    if !committed_link(&scratch, "target.txt", "link.txt") {
        return;
    }

    // A link becomes a regular file. The index still says `120000`, so the hint
    // says "maybe", the `lstat` finds a plain file, and the read is an ordinary
    // one. git calls this a *modification*, which is the case the hint has to
    // get right by asking rather than by trusting the mode.
    scratch.remove("link.txt");
    scratch.write("link.txt", "now a real file\n");

    // A regular file becomes a link. git calls this a *type change*.
    scratch.remove("swap.txt");
    assert!(scratch.symlink_file("target.txt", "swap.txt"));

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let of = |path: &str| {
        changes
            .iter()
            .find(|change| change.path == path)
            .unwrap_or_else(|| panic!("{path} is not in {changes:#?}"))
    };

    // Direction one: git diffs the file's content against the link text it
    // replaced, so `vigia` has to read the file rather than try to read a link
    // that is no longer there.
    assert_eq!(scratch.git_numstat("link.txt"), Numstat::Lines(1, 1));
    let became_file = of("link.txt");
    assert_eq!(
        became_file.kind,
        ChangeKind::Modified,
        "git reports a link replaced by a regular file as a modification, and the \
         hint's soundness depends on `gix` agreeing"
    );
    let diff = worktree.diff(became_file).expect("diff the replaced link");
    assert_eq!(texts(&diff, LineKind::Removed), ["target.txt"]);
    assert_eq!(
        texts(&diff, LineKind::Added),
        ["now a real file"],
        "the read followed a link that is not there any more"
    );

    // Direction two, and this is the half that closes the hint's only gap. git
    // reports `T` here, so it never reaches a read at all: `is_diffable` is
    // false and `Worktree::diff` returns before consulting the worktree. The
    // index mode being stale therefore cannot mislead anything.
    let became_link = of("swap.txt");
    assert_eq!(
        became_link.kind,
        ChangeKind::TypeChange,
        "a regular file replaced by a symlink is not reported as a type change, \
         so `maybe_symlink` reading the index mode would send this to an ordinary \
         read and diff the link's target through it"
    );
    assert!(
        !became_link.is_diffable(),
        "a type change became diffable, so the early return #15 relies on is gone"
    );
    let diff = worktree.diff(became_link).expect("diff the type change");
    assert!(
        diff.hunks.is_empty(),
        "a type change is a state, not a diff"
    );
    assert_eq!(diff.bytes, 0, "a type change read the worktree");
}

#[test]
fn editing_a_symlinks_target_leaves_the_link_out_of_the_changed_set() {
    // **Named for what it holds rather than for the direction it was written
    // against**, which is `SPEC.md` §7's rule about a gate asserting the defect
    // it is named for. Written as "editing a target does not change the link's
    // diff", it survived the mutation that deletes the `is_symlink` branch in
    // `read_worktree`, because the *status walk* already agreed with git here
    // and the read is never reached: an unchanged path has no diff to be wrong
    // about. So this gates `gix`'s walk, which is what `fidelity.rs` is for, and
    // it is worth keeping for that.
    //
    // The half it looked like it covered is real and lives in `frame.rs`:
    // `editing_a_symlinks_target_does_not_invalidate_the_links_diff` is the
    // fingerprint side, and it is only observable through `FrameStats`.
    let scratch = Scratch::new("symlink-target-edit");
    scratch.write("target_a.txt", "AAAAAAAA\n");
    if !committed_link(&scratch, "target_a.txt", "link.txt") {
        return;
    }

    scratch.write("target_a.txt", "EDITED\n");

    assert_eq!(
        scratch.git_numstat("link.txt"),
        Numstat::Unchanged,
        "git considers the link changed, so this fixture is not the case it names"
    );
    assert_eq!(
        scratch.git_numstat("target_a.txt"),
        Numstat::Lines(1, 1),
        "the edit did not land, so nothing here is being tested"
    );

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    assert_eq!(
        changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        ["target_a.txt"],
        "editing a link's target reported the link as changed, which git does not"
    );
}
