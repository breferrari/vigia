//! The frame path, behaviourally: I2a.
//!
//! `tests/budgets.rs` gates the *cost* of a frame, which is the half of I2a that
//! can be satisfied by a cache that reuses too much. This file gates the other
//! half. Every test here asks the same question in a different way: is what the
//! frame handed back still true?
//!
//! That asymmetry is the whole risk in this invariant. A frame path that never
//! reuses anything is slow, and the budget gate catches it loudly. A frame path
//! that reuses something it should not is *fast*, passes every budget, and shows
//! the reader a diff that no longer exists. The second failure is the one worth
//! building tests around.

mod support;

use support::{Scratch, delta, materialise, settle};
use vigia_core::{ChangeKind, FileDiff, Frame, Worktree};

/// Small enough to reason about every count, more than one so "all" and "the
/// one that changed" are different numbers.
const FILES: usize = 4;
const LINES: usize = 40;

const FIRST: &str = "src/mod_0.rs";
const SECOND: &str = "src/mod_1.rs";

/// Every diff the frame currently reports, in file order.
///
/// Takes the diffs by value so the frame is free afterwards; a test that wants
/// to compare against a fresh computation needs both at once.
fn diffs(frame: &mut Frame) -> Vec<FileDiff> {
    frame.advance().expect("advance");
    (0..frame.files().len())
        .map(|i| frame.diff(i).expect("diff").1.clone())
        .collect()
}

/// The same diffs, computed from scratch with no reuse anywhere.
fn fresh(worktree: &Worktree) -> Vec<FileDiff> {
    worktree
        .changes()
        .expect("enumerate")
        .map(|change| worktree.diff(&change.expect("change")).expect("diff"))
        .collect()
}

/// Assert two sets of diffs are identical, reporting the first disagreement
/// compactly.
///
/// `assert_eq!` over `Vec<FileDiff>` is correct and unreadable: one wrong line
/// prints both diffs in full, which on these fixtures is tens of thousands of
/// characters and buries the one line that matters. This keeps the strength of
/// full equality and loses the wall of text.
fn assert_same(reused: &[FileDiff], fresh: &[FileDiff], what: &str) {
    assert_eq!(
        reused.len(),
        fresh.len(),
        "{what}: the frame reported {} files, a fresh diff {}",
        reused.len(),
        fresh.len()
    );
    for (from_frame, from_scratch) in reused.iter().zip(fresh) {
        assert_eq!(
            from_frame.path, from_scratch.path,
            "{what}: the frame reported files in a different order"
        );
        assert!(
            from_frame == from_scratch,
            "{what}: {} disagrees with a freshly computed diff\n  \
             frame: +{} -{}, {} bytes, {} hunks\n  \
             fresh: +{} -{}, {} bytes, {} hunks\n  {}",
            from_frame.path,
            from_frame.added,
            from_frame.removed,
            from_frame.bytes,
            from_frame.hunks.len(),
            from_scratch.added,
            from_scratch.removed,
            from_scratch.bytes,
            from_scratch.hunks.len(),
            first_difference(from_frame, from_scratch),
        );
    }
}

/// Where two diffs of one file first disagree, in one line.
fn first_difference(left: &FileDiff, right: &FileDiff) -> String {
    let lines = |diff: &FileDiff| -> Vec<String> {
        diff.hunks
            .iter()
            .flat_map(|hunk| {
                hunk.lines
                    .iter()
                    .map(|line| format!("{:?} {}", line.kind, line.text))
            })
            .collect()
    };
    let (left, right) = (lines(left), lines(right));
    for (i, (a, b)) in left.iter().zip(&right).enumerate() {
        if a != b {
            return format!("first differing line {i}: frame {a:?}, fresh {b:?}");
        }
    }
    format!(
        "no line differs, but the frame has {} lines and a fresh diff {}",
        left.len(),
        right.len()
    )
}

#[test]
fn a_frame_starts_empty_and_advance_fills_it() {
    let scratch = Scratch::large_diff("frame-empty", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();

    // The first frame is the same shape as every later one rather than a
    // special case, which is why this starts empty instead of walking on
    // construction.
    assert!(frame.files().is_empty(), "a new frame reports files");
    assert_eq!(frame.tracked(), 0, "a new frame holds diffs");

    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), FILES);
    assert_eq!(
        frame.tracked(),
        0,
        "advance diffed something; it must only re-read the file list"
    );
}

#[test]
fn what_a_frame_hands_back_after_an_edit_is_what_a_fresh_diff_computes() {
    // The load-bearing test in this file. Reuse is only correct if it is
    // invisible, so the whole frame is compared against a frame path that has
    // no memory at all.
    let scratch = Scratch::large_diff("frame-fresh", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let before = diffs(&mut frame);
    scratch.edit_line(FIRST, 7, "fn edited() { let value = 999999; }");

    let after = diffs(&mut frame);
    assert_same(&after, &fresh(&worktree), "after a one-line edit");

    // Non-vacuity. If the edit had changed nothing about the diff, the equality
    // above would hold against a cache that ignores the working tree entirely.
    assert!(
        before != after,
        "editing a line changed no diff, so the comparison above proves nothing"
    );
}

#[test]
fn a_same_length_edit_is_noticed() {
    // Half of what makes an in-place edit hard to see: the file's size does not
    // move, so only the modification time separates the two versions.
    let scratch = Scratch::large_diff("frame-same-length", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let before = diffs(&mut frame);
    let len_before = std::fs::metadata(scratch.path_of(FIRST))
        .expect("stat")
        .len();

    scratch.scribble_line(FIRST, 3, '@');

    let len_after = std::fs::metadata(scratch.path_of(FIRST))
        .expect("stat")
        .len();
    assert_eq!(
        len_before, len_after,
        "the edit changed the file's length, so it does not test what it claims"
    );

    let after = diffs(&mut frame);
    assert!(
        before != after,
        "a same-length edit was reused as if nothing had changed"
    );
    assert_same(&after, &fresh(&worktree), "after a same-length edit");
}

#[test]
fn an_idle_frame_recomputes_nothing() {
    let scratch = Scratch::large_diff("frame-idle", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let before = frame.stats();
    materialise(&mut frame);
    let cost = delta(before, frame.stats());

    assert_eq!(
        cost.computed, 0,
        "an idle frame recomputed {} diffs",
        cost.computed
    );
    assert_eq!(cost.bytes, 0, "an idle frame read {} bytes", cost.bytes);
    assert_eq!(
        cost.reused, FILES as u64,
        "an idle frame reused {} of {FILES} diffs, so it did not visit them all",
        cost.reused
    );
}

#[test]
fn touching_a_file_costs_one_redundant_diff() {
    // Documenting the trade rather than complaining about it. The rule is
    // "reuse only what can be proved unchanged", and a new modification time is
    // not proof of anything either way. Erring towards a redundant diff is the
    // side that cannot show a stale frame.
    let scratch = Scratch::large_diff("frame-touch", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let content = std::fs::read(scratch.path_of(FIRST)).expect("read");
    std::fs::write(scratch.path_of(FIRST), &content).expect("rewrite identical content");

    let before = frame.stats();
    materialise(&mut frame);
    let cost = delta(before, frame.stats());

    assert_eq!(
        cost.computed, 1,
        "rewriting one file's own content recomputed {} diffs",
        cost.computed
    );
    assert_eq!(cost.reused, (FILES - 1) as u64);
}

#[test]
fn a_path_that_stops_changing_is_evicted() {
    // I3 forbids unbounded growth over days, and a monitor watching an agent
    // will see thousands of paths become changed and then clean again as work
    // is staged and committed. Holding every diff ever computed is the shape
    // that fails a soak test.
    let scratch = Scratch::large_diff("frame-evict", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    assert_eq!(frame.tracked(), FILES);

    scratch.git(&["checkout", "--", SECOND]);

    let before = frame.stats();
    materialise(&mut frame);
    let cost = delta(before, frame.stats());

    assert_eq!(
        frame.files().len(),
        FILES - 1,
        "the reverted file is still reported as changed"
    );
    assert_eq!(
        frame.tracked(),
        FILES - 1,
        "the frame still holds {} diffs for {} changed files",
        frame.tracked(),
        FILES - 1
    );
    assert_eq!(
        cost.evicted, 1,
        "reverting one file evicted {} diffs",
        cost.evicted
    );
    // And nothing else paid for it: the other files were untouched.
    assert_eq!(
        cost.computed, 0,
        "eviction recomputed {} diffs",
        cost.computed
    );
}

#[test]
fn staging_a_file_recomputes_the_files_still_changed() {
    // Staging rewrites the index, and the index is the left-hand side of every
    // diff on screen. Nothing on disk moved, so a frame path that only watched
    // the working tree would keep showing diffs against a blob that is no
    // longer staged.
    let scratch = Scratch::large_diff("frame-stage", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    scratch.git(&["add", "--", FIRST]);

    let after = diffs(&mut frame);
    assert_eq!(
        after.len(),
        FILES - 1,
        "the staged file is still reported as an unstaged change"
    );
    assert_same(&after, &fresh(&worktree), "after staging one file");
}

#[test]
fn a_new_index_blob_invalidates_a_diff_the_worktree_never_touched() {
    // The index is the left-hand side of every diff on screen, and it moves
    // without the working tree moving: `git add`, `git reset` and `git stash`
    // all rewrite entries under files nobody edited. `update-index` is that
    // operation with nothing else going on, which is what makes this testable
    // at all. Staging normally makes a path clean, and a clean path leaves the
    // frame through a different door entirely (eviction), so it can never
    // exercise this branch.
    let scratch = Scratch::large_diff("frame-index-blob", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let before = diffs(&mut frame);
    let stat_before = std::fs::metadata(scratch.path_of(FIRST)).expect("stat");

    // Point the first file's index entry at some other content entirely. Not
    // another fixture file's blob: every generated file holds identical bytes,
    // so they all share one object id and swapping between them changes
    // nothing at all.
    let other = scratch.hash_object("fn staged_from_somewhere_else() {}\n");
    scratch.git(&[
        "update-index",
        "--cacheinfo",
        &format!("100644,{other},{FIRST}"),
    ]);

    // The working tree really did not move, so a fingerprint alone would have
    // said "reuse this".
    let stat_after = std::fs::metadata(scratch.path_of(FIRST)).expect("stat");
    assert_eq!(stat_before.len(), stat_after.len());
    assert_eq!(
        stat_before.modified().expect("mtime"),
        stat_after.modified().expect("mtime"),
        "rewriting the index touched the file, so this tests the wrong thing"
    );

    let after = diffs(&mut frame);
    assert!(
        before != after,
        "the index moved under an untouched file and the frame reused its diff"
    );
    assert_same(
        &after,
        &fresh(&worktree),
        "after the index entry was rewritten",
    );
}

#[test]
fn a_removed_file_is_diffed_from_the_index_alone_and_reused() {
    // A removal has no working-tree side to fingerprint, so reuse cannot lean
    // on a `stat` at all. It leans on the index blob and the kind instead, and
    // this is what says that path is exercised rather than merely written.
    let scratch = Scratch::large_diff("frame-removed", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    scratch.remove(FIRST);
    materialise(&mut frame);

    let removed = frame
        .files()
        .iter()
        .position(|change| change.path == FIRST)
        .expect("the deleted file is still a change against the index");
    assert_eq!(frame.files()[removed].kind, ChangeKind::Removed);
    let diff = frame.diff(removed).expect("diff").1.clone();
    assert!(
        diff.removed == LINES as u32 && diff.added == 0,
        "deleting a {LINES}-line file reported +{} -{}",
        diff.added,
        diff.removed
    );

    // Idle now, and the removal must be reused without a probe: there is
    // nothing left on disk to probe.
    let before = frame.stats();
    materialise(&mut frame);
    let cost = delta(before, frame.stats());
    assert_eq!(cost.computed, 0, "a removal was recomputed");
    assert_eq!(
        cost.probes,
        (FILES - 1) as u64,
        "{} stat calls for {} files that still exist",
        cost.probes,
        FILES - 1
    );
}

#[test]
fn a_failed_advance_leaves_the_frame_intact() {
    // A monitor that discarded its state because one status walk failed would
    // blank the pane for a reason its reader cannot see. The previous frame is
    // still the best answer available, so it survives.
    let scratch = Scratch::large_diff("frame-failure", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let files = frame.files().to_vec();
    let tracked = frame.tracked();

    // Garbage of a plausible length, and the length is not incidental. `gix`
    // 0.86 reads the index's trailing checksum as `data[data.len() - 20..]`
    // with no lower bound, so an index file shorter than the object hash
    // *panics* instead of returning an error: 19 bytes and below abort, 20 and
    // above report cleanly. That is a `gix` defect rather than something the
    // frame path can catch, since the release profile sets `panic = "abort"`.
    // See ROADMAP.md's deferral shelf. This test gates what vigia controls:
    // given an error, the previous frame survives it.
    std::fs::write(scratch.path_of(".git/index"), vec![0xABu8; 128]).expect("corrupt the index");

    let error = frame
        .advance()
        .expect_err("a corrupt index was walked without complaint");

    assert_eq!(
        frame.files(),
        files.as_slice(),
        "a failed advance changed the file list"
    );
    assert_eq!(
        frame.tracked(),
        tracked,
        "a failed advance dropped cached diffs"
    );
    // The message is what a user sees, so it has to name the problem.
    let message = error.to_string();
    assert!(
        message.contains("status") || message.contains("index"),
        "unhelpful error for a corrupt index: {message}"
    );
}
