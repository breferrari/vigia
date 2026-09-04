//! The frame path, behaviourally: I2a.

mod support;

use std::time::{Duration, SystemTime};

use support::{Scratch, committed_link, delta, index_of, materialise, settle, settle_spans};
use vigia_core::{ChangeKind, FileDiff, Frame, Worktree};

/// Small enough to reason about every count, more than one so "all" and "the
/// one that changed" are different numbers.
const FILES: usize = 4;
const LINES: usize = 40;

const FIRST: &str = "src/mod_0.rs";
const SECOND: &str = "src/mod_1.rs";

/// Every diff the frame currently reports, in file order.
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
    for (field, a, b) in [
        ("binary", u32::from(left.binary), u32::from(right.binary)),
        ("added", left.added, right.added),
        ("removed", left.removed, right.removed),
        ("lines", left.lines, right.lines),
    ] {
        if a != b {
            return format!("{field}: frame {a}, fresh {b}");
        }
    }
    if left.bytes != right.bytes {
        return format!("bytes: frame {}, fresh {}", left.bytes, right.bytes);
    }
    if left.first_line != right.first_line {
        return format!(
            "first_line: frame {:?}, fresh {:?}",
            left.first_line, right.first_line
        );
    }

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
fn a_file_written_moments_ago_is_never_reused() {
    // The *producer* half of the racily-clean rule, and the half nothing else here
    // reaches.
    let scratch = Scratch::large_diff("frame-recent", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    scratch.edit_line(FIRST, 0, "fn edited() { let value = 1; }");

    // Frame one: recomputes the edited file because its content changed.
    let before = frame.stats();
    materialise(&mut frame);
    assert_eq!(
        delta(before, frame.stats()).computed,
        1,
        "the edit itself was not picked up"
    );

    // Frame two, with nothing touched in between. The file is unchanged now, and
    // it must *still* be recomputed, because "unchanged" is not yet provable.
    let before = frame.stats();
    materialise(&mut frame);
    let cost = delta(before, frame.stats());
    assert_eq!(
        cost.computed, 1,
        "a file written moments ago was trusted as unchanged, so a same-length \
         rewrite inside its modification-time granule would be served stale"
    );
    assert_eq!(
        cost.reused,
        (FILES - 1) as u64,
        "the other files stopped being reused, so this is measuring the wrong thing"
    );
}

#[test]
fn touching_a_file_costs_one_redundant_diff() {
    // Documenting the trade rather than complaining about it.
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
    // I3 forbids unbounded growth over days, and a monitor watching an agent will see
    // thousands of paths become changed and then clean again as work is staged and
    // committed.
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

/// Total rows over every changed file, from spans alone.
fn total_height(frame: &mut Frame) -> usize {
    frame.height(|_, span| span.lines as usize).expect("height")
}

#[test]
fn a_carried_span_survives_an_edit_only_until_the_file_settles() {
    let scratch = Scratch::large_diff("frame-span-edit", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();

    // Settle without materialising: every span comes from a read, no file has a diff.
    let primed = settle_spans(&mut frame);
    let before = total_height(&mut frame);
    assert_eq!(
        frame.tracked(),
        0,
        "the frame holds {} diffs, so these spans did not come from reads and \
         this test is not about a carried span at all",
        frame.tracked()
    );

    // The premise this test is named for, and without it the whole thing is vacuous.
    assert_eq!(
        primed, FILES as u64,
        "priming measured {primed} of {FILES} files, so there was no walk here \
         to carry anything across"
    );
    let idle = frame.stats();
    frame.advance().expect("advance");
    let unchanged = total_height(&mut frame);
    let idle = delta(idle, frame.stats());
    assert_eq!(
        idle.measured, 0,
        "an idle tick re-measured {} files, so no span is being carried and the \
         assertions below cannot tell a carry from a re-read",
        idle.measured
    );
    assert_eq!(unchanged, before, "an idle tick changed the diff's height");
    assert_eq!(
        idle.deferred, 0,
        "an idle tick kept {} heights waiting, so an unchanged file is being treated \
         as one still being written",
        idle.deferred
    );
    assert_eq!(
        frame.settles_in(SystemTime::now()),
        None,
        "an idle tick armed a settle deadline with nothing waiting"
    );

    // And it proved them with a `stat` each, which nothing else asserts.
    assert_eq!(
        idle.probes, FILES as u64,
        "an idle tick took {} stat calls to re-prove {FILES} carried spans, \
         so the walk is not counting the syscalls it makes",
        idle.probes
    );

    // Twice as many lines in a file the viewport never reached. Inside the margin
    // the file is still being written, so the old height stands, marked as waiting,
    // and nothing is read.
    scratch.rewrite_all(FILES, LINES * 2, 3);
    let moved = frame.stats();
    frame.advance().expect("advance");
    let inside = total_height(&mut frame);
    let moved = delta(moved, frame.stats());
    assert_eq!(
        inside, before,
        "the height moved inside the margin, so a file still being written was read"
    );
    assert_eq!(
        moved.deferred, FILES as u64,
        "the tick kept {} of {FILES} heights waiting, so the walk is not deferring \
         the files that moved",
        moved.deferred
    );
    assert_eq!(
        moved.measured, 0,
        "the tick read {} files inside the margin, which the margin exists to prevent",
        moved.measured
    );
    let due = frame
        .settles_in(SystemTime::now())
        .expect("a tick that kept heights waiting armed no settle deadline");
    assert!(
        due <= Duration::from_secs(2),
        "the settle deadline is {due:?} away, past the two-second margin the wait \
         is measured against"
    );

    // The oracle: a frame with no memory at all, over the same worktree.
    let mut cold = worktree.frame();
    cold.advance().expect("advance");
    let truth = total_height(&mut cold);
    assert!(
        truth > before,
        "the diff doubled and a memoryless frame still counts {before}, so the \
         fixture proves nothing"
    );

    // Once the files settle, one tick reads each of them once and the height is
    // the truth; the tick after reads nothing.
    let re_read = settle_spans(&mut frame);
    let after = total_height(&mut frame);
    assert_eq!(
        after, truth,
        "the frame reports a {after}-row diff after the files settled where a \
         frame with no memory computes {truth}, so a carried span is being \
         trusted past its file"
    );
    assert_eq!(
        re_read, FILES as u64,
        "the settled tick read {re_read} of {FILES} files, so the wait did not \
         end in one read each"
    );
    let again = frame.stats();
    frame.advance().expect("advance");
    total_height(&mut frame);
    let again = delta(again, frame.stats());
    assert_eq!(
        again.measured, 0,
        "the tick after the settled one read {} files again",
        again.measured
    );
    assert_eq!(
        frame.settles_in(SystemTime::now()),
        None,
        "every file settled and a settle deadline is still armed"
    );
}

#[test]
fn a_height_taken_from_a_diff_waits_for_the_margin_like_a_carried_one() {
    // A height that came from a diff earns no exemption: it is asked with a stat,
    // waits while the file is still being written, and is read once when it
    // settles, exactly as a carried one is.
    const REWRITTEN: usize = FILES;
    let scratch = Scratch::large_diff("frame-inhand", REWRITTEN, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    assert_eq!(
        frame.tracked(),
        REWRITTEN,
        "settle left {} diffs for {REWRITTEN} files, so this fixture is not the \
         diff-in-hand case at all",
        frame.tracked()
    );

    let before_height = total_height(&mut frame);

    // Every print moves and every diff doubles, and no diff is recomputed: the
    // frame is asked for the height and nothing else.
    scratch.rewrite_all(REWRITTEN, LINES * 2, 9);

    let before = frame.stats();
    frame.advance().expect("advance");
    let inside = total_height(&mut frame);
    let first = delta(before, frame.stats());
    assert_eq!(
        inside, before_height,
        "the height moved inside the margin, so a file still being written was read"
    );

    // And again, so a per-tick proof cannot be mistaken for a per-frame one.
    let before = frame.stats();
    frame.advance().expect("advance");
    total_height(&mut frame);
    let second = delta(before, frame.stats());

    for (label, cost) in [("the tick after the rewrite", first), ("the next", second)] {
        assert_eq!(
            cost.probes, REWRITTEN as u64,
            "{label} took {} stat calls over {REWRITTEN} files the frame holds diffs \
             for, so a diff in hand is either trusted by presence or asked twice",
            cost.probes
        );
        assert_eq!(
            cost.measured, 0,
            "{label} read {} files that were still being written",
            cost.measured
        );
        assert_eq!(
            cost.deferred, REWRITTEN as u64,
            "{label} kept {} of {REWRITTEN} heights waiting",
            cost.deferred
        );
    }

    // The oracle: a frame with no memory at all, over the same worktree.
    let mut cold = worktree.frame();
    cold.advance().expect("advance");
    let truth = total_height(&mut cold);
    assert!(
        truth > before_height,
        "the diff doubled and a memoryless frame still counts {before_height}"
    );

    // And once they settle, each is read exactly once and the height is the truth.
    let re_read = settle_spans(&mut frame);
    assert_eq!(
        re_read, REWRITTEN as u64,
        "the settled tick read {re_read} of {REWRITTEN} files"
    );
    let after = total_height(&mut frame);
    assert_eq!(
        after, truth,
        "the files settled and the frame still counts {after} where a frame with \
         no memory computes {truth}, so a diff in hand is trusted past its file"
    );

    // And the tick after that reads nothing: the read replaced what the diff in
    // hand said, and nothing is left to disagree with it.
    let again = frame.stats();
    frame.advance().expect("advance");
    total_height(&mut frame);
    let again = delta(again, frame.stats());
    assert_eq!(
        again.measured, 0,
        "the tick after the settled one read {} files again, so the height just taken is not what the next tick asks first",
        again.measured
    );
}

#[test]
fn a_file_the_tick_diffed_is_not_asked_again_by_the_height_walk() {
    // The height goes with the diff it was taken from, proved by the same read, so
    // the walk that follows spends its stats on the other files only.
    let scratch = Scratch::large_diff("frame-diffed-once", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    scratch.edit_line("src/mod_0.rs", 3, "// edited");
    frame.advance().expect("advance");
    let index = index_of(&frame, "src/mod_0.rs");
    let before = frame.stats();
    frame.diff(index).expect("diff");
    let diffed = delta(before, frame.stats());
    assert_eq!(diffed.computed, 1, "the edited file was not recomputed");

    let before = frame.stats();
    total_height(&mut frame);
    let walked = delta(before, frame.stats());
    assert_eq!(
        walked.probes,
        (FILES - 1) as u64,
        "the height walk took {} stat calls after the tick diffed one of {FILES} \
         files, so the file just read is being asked again",
        walked.probes
    );
}

#[test]
fn a_staged_files_height_is_kept_across_ticks_with_no_stat() {
    // A staged change is two blobs and no file on disk, so nothing can go stale
    // between ticks and the walk keeps its height without asking the filesystem.
    let scratch = Scratch::new("frame-staged-height");
    scratch.write(
        "src/staged.rs",
        "one
",
    );
    scratch.commit_all("base");
    scratch.write(
        "src/staged.rs",
        "one
two
three
",
    );
    scratch.git(&["add", "-A"]);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(true);
    frame.advance().expect("advance");
    let first = total_height(&mut frame);
    assert!(first > 0, "the staged change has no height to count");

    let before = frame.stats();
    frame.advance().expect("advance");
    let again = total_height(&mut frame);
    let cost = delta(before, frame.stats());
    assert_eq!(again, first, "a tick changed a staged file's height");
    assert_eq!(
        cost.measured, 0,
        "a tick read a staged file again: {}",
        cost.measured
    );
    assert_eq!(
        cost.probes, 0,
        "a tick took {} stat calls for a change that has no file on disk",
        cost.probes
    );
}

#[test]
fn a_waiting_file_is_counted_once_a_tick_and_a_diff_of_it_reads_it_fresh() {
    // The height walk and the screen keep separate caches of the same file, and a
    // height kept waiting leaves the diff behind it stale. Asking the height twice
    // in one tick counts one wait, and drawing the file recomputes its diff rather
    // than serving the one the wait left behind.
    let scratch = Scratch::large_diff("frame-wait-then-diff", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    scratch.rewrite_all(FILES, LINES * 2, 6);
    frame.advance().expect("advance");
    let before = frame.stats();
    total_height(&mut frame);
    frame
        .rows_of(0, |_, span| span.lines as usize)
        .expect("height");
    let counted = delta(before, frame.stats());
    assert_eq!(
        counted.deferred, FILES as u64,
        "asking the height twice in one tick kept {} heights waiting for {FILES} files",
        counted.deferred
    );

    let before = frame.stats();
    frame.diff(0).expect("diff");
    let drawn = delta(before, frame.stats());
    assert_eq!(
        drawn.computed, 1,
        "drawing a file whose height is waiting served the diff the wait left behind"
    );
    assert_eq!(drawn.reused, 0, "a diff of a file that moved was reused");
}

#[test]
fn a_failed_measure_is_asked_again_rather_than_carried() {
    // The narrowest arm in `fill_span`, and the one where carrying is wrong.
    let scratch = Scratch::new("frame-failed-measure");
    scratch.write(FIRST, support::numbered_lines(40));
    // Different content from FIRST on purpose: identical files share one
    // blob object, and deleting that object below would break both.
    scratch.write(SECOND, support::numbered_lines(25));
    scratch.commit_all("base");

    let blob = scratch.git(&["rev-parse", &format!("HEAD:{FIRST}")]);
    let blob = blob.trim();
    scratch.remove(FIRST);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let whole = total_height(&mut frame);
    assert!(
        whole > 0,
        "the removed file contributes no rows even with its blob present, so \
         this fixture cannot show the difference"
    );

    // Break it, and prove the premise: the walk now reports less.
    let loose = scratch
        .root()
        .join(".git/objects")
        .join(&blob[..2])
        .join(&blob[2..]);
    std::fs::remove_file(&loose).expect("delete the loose object");
    let mut broken_frame = worktree.frame();
    broken_frame.advance().expect("advance");
    let broken = total_height(&mut broken_frame);
    assert!(
        broken < whole,
        "deleting the blob changed nothing about the height ({broken} against \
         {whole}), so the read did not fail and this test proves nothing"
    );

    // Now put the object back, exactly as a finished `git gc` or a filled-in
    // partial clone would, and tick. A frame that carried the failure reports the
    // broken height forever; one that asks again reports the whole diff.
    scratch.write(FIRST, support::numbered_lines(40));
    scratch.git(&["hash-object", "-w", "--", FIRST]);
    scratch.remove(FIRST);
    assert!(loose.exists(), "the loose object was not restored");

    broken_frame.advance().expect("advance");
    let after = total_height(&mut broken_frame);
    assert_eq!(
        after, whole,
        "the blob is readable again and the frame still reports {after} of \
         {whole} rows, so a failed measure was carried instead of retried"
    );
}

#[test]
fn an_attributes_file_rewritten_inside_one_granule_still_drops_the_caches() {
    // The racily-clean case, on the one file whose staleness invalidates every other
    // file's answer.
    let scratch = Scratch::large_diff("frame-attrs-granule", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();

    // Settle the *files* first, so their spans are provable and any re-measure
    // below is the guard's doing rather than the fixture's own youth.
    let primed = settle_spans(&mut frame);
    assert_eq!(
        primed, FILES as u64,
        "the fixture did not measure its files"
    );

    // The attributes arrive, stamped now. This tick drops the caches because a
    // file written a moment ago cannot be proved unchanged, which is the correct
    // and conservative half of the rule.
    let stamp = std::time::SystemTime::now();
    let path = scratch.path_of(".gitattributes");
    stamp_write(
        &path,
        "a.txt binary
",
        stamp,
    );
    frame.advance().expect("advance");
    total_height(&mut frame);

    // Now the collision: different content, the same length, and the same
    // modification time. Nothing a `stat` returns has moved.
    stamp_write(
        &path,
        "b.txt binary
",
        stamp,
    );
    let before = frame.stats();
    frame.advance().expect("advance");
    total_height(&mut frame);
    let cost = delta(before, frame.stats());
    assert!(
        cost.measured >= FILES as u64,
        "the attributes changed inside one granule and only {} files of \
         {FILES} were re-measured, so the guard compared fingerprints, \
         found them equal, and kept every artefact computed under the old \
         rules",
        cost.measured
    );
}

/// Write `contents` and force its modification time, so two writes can be made
/// to collide the way one filesystem granule makes them collide.
fn stamp_write(path: &std::path::Path, contents: &str, at: std::time::SystemTime) {
    std::fs::write(path, contents).expect("write");
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open")
        .set_modified(at)
        .expect("stamp");
}

#[test]
fn a_span_for_a_path_that_stops_changing_is_dropped() {
    let scratch = Scratch::large_diff("frame-span-evict", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    total_height(&mut frame);
    assert_eq!(
        frame.tracked_spans(),
        FILES,
        "the walk left {} spans for {FILES} changed files",
        frame.tracked_spans()
    );

    scratch.git(&["checkout", "--", SECOND]);
    frame.advance().expect("advance");

    assert_eq!(
        frame.files().len(),
        FILES - 1,
        "the reverted file is still reported as changed"
    );
    assert_eq!(
        frame.tracked_spans(),
        FILES - 1,
        "the frame still holds {} spans for {} changed files, so the span cache \
         is bounded by the session rather than by the diff",
        frame.tracked_spans(),
        FILES - 1
    );
}

#[test]
fn staging_a_file_recomputes_the_files_still_changed() {
    // Staging rewrites the index, and the index is the left-hand side of every diff on
    // screen.
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
    // The index is the left-hand side of every diff on screen, and it moves without the
    // working tree moving: `git add`, `git reset` and `git stash` all rewrite entries
    // under files nobody edited.
    let scratch = Scratch::large_diff("frame-index-blob", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let before = diffs(&mut frame);
    let stat_before = std::fs::metadata(scratch.path_of(FIRST)).expect("stat");

    // Point the first file's index entry at some other content entirely.
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

    scratch.corrupt_index();

    let error = frame
        .advance()
        .expect_err("a corrupt index was walked without complaint");

    std::fs::write(scratch.path_of(".git/index"), vec![0xABu8; 20]).expect("truncate the index");
    worktree
        .frame()
        .advance()
        .expect_err("a 20-byte index no longer reports an error, so #13 may have moved");

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

#[test]
fn the_settle_deadline_is_the_last_moved_print_plus_the_margin() {
    let scratch = Scratch::large_diff("frame-settle-last", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle_spans(&mut frame);

    scratch.write(FIRST, "fn first() {}\n".repeat(LINES * 2));
    std::thread::sleep(Duration::from_millis(100));
    scratch.write(SECOND, "fn second() {}\n".repeat(LINES * 2));
    let modified = |rela: &str| {
        std::fs::symlink_metadata(scratch.path_of(rela))
            .and_then(|meta| meta.modified())
            .expect("the fixture's modification time")
    };
    let (earlier, later) = (modified(FIRST), modified(SECOND));
    assert!(
        earlier < later,
        "the two prints share a modification time, so this fixture cannot tell \
         the last print from the first"
    );

    frame.advance().expect("advance");
    total_height(&mut frame);
    let asked = SystemTime::now();
    let due = frame
        .settles_in(asked)
        .expect("two files moved inside the margin and no settle deadline is armed");
    // A wait is the print's modification time plus the margin, so the instant
    // is exact rather than approximate; the margin is `SPEC.md` §6's two seconds.
    assert_eq!(
        asked + due,
        later + Duration::from_secs(2),
        "the settle deadline is {due:?} away, which is the first moved print's \
         margin rather than the last's, so the wake lands while the later file \
         is still inside it"
    );
}

#[test]
fn a_failed_advance_disarms_the_settle_deadline() {
    let scratch = Scratch::large_diff("frame-failure-settle", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle_spans(&mut frame);

    scratch.rewrite_all(FILES, LINES * 2, 3);
    frame.advance().expect("advance");
    total_height(&mut frame);
    assert!(
        frame.settles_in(SystemTime::now()).is_some(),
        "a rewrite inside the margin armed no settle deadline, so there is nothing \
         here for a failed walk to leave armed"
    );

    scratch.corrupt_index();
    frame
        .advance()
        .expect_err("a corrupt index was walked without complaint");
    assert_eq!(
        frame.settles_in(SystemTime::now()),
        None,
        "a failed walk left the settle deadline armed, so the loop that folds it \
         to a zero wait advances, fails and folds it again without ever blocking"
    );
}

#[test]
fn an_advance_on_the_settle_wake_walks_only_once_the_wait_has_run_out() {
    let scratch = Scratch::large_diff("frame-settle-wake", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle_spans(&mut frame);

    scratch.rewrite_all(FILES, LINES * 2, 3);
    frame.advance().expect("advance");
    total_height(&mut frame);
    let asked = SystemTime::now();
    let due = frame
        .settles_in(asked)
        .expect("a rewrite inside the margin armed no settle deadline");

    // Asked while the wait still runs, which is every timeout some other clock
    // caused: nothing is walked and the deadline stands where it was.
    frame.advance_if_settled(asked).expect("advance");
    assert_eq!(
        frame.settles_in(asked),
        Some(due),
        "a timeout before the wait ran out walked status, so every clock the \
         loop owns now costs a tick"
    );

    // Asked at the deadline: the walk runs and the wait is decided again by it.
    frame.advance_if_settled(asked + due).expect("advance");
    assert_eq!(
        frame.settles_in(asked),
        None,
        "the wait ran out and nothing walked, so the total stays where it was \
         until the next event"
    );
}

/// The cache-key gate for `FileDiff::lines`.
#[test]
fn a_same_length_edit_that_changes_the_line_count_is_not_reused() {
    let scratch = Scratch::large_diff("frame-line-count", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let before = diffs(&mut frame);
    let path = scratch.path_of(FIRST);
    let original = std::fs::read(&path).expect("read the fixture");

    // Every newline becomes a space, so the bytes are identical and the file is
    // one line long. Nothing else can produce that pair.
    let flattened: Vec<u8> = original
        .iter()
        .map(|&byte| if byte == b'\n' { b' ' } else { byte })
        .collect();
    assert_eq!(
        flattened.len(),
        original.len(),
        "the rewrite changed the byte length, so this tests the length check \
         rather than the line count"
    );
    scratch.write(FIRST, &flattened);

    let after = diffs(&mut frame);
    assert_same(&after, &fresh(&worktree), "after a same-length reflow");

    let edited = |diffs: &[FileDiff]| -> u32 {
        diffs
            .iter()
            .find(|diff| diff.path == FIRST)
            .expect("the edited file is in the diff")
            .lines
    };
    assert!(
        edited(&before) > 1,
        "the fixture started at {} lines, so flattening it changes nothing",
        edited(&before)
    );
    assert_eq!(
        edited(&after),
        1,
        "the frame still reports {} lines for a file that is now one line long",
        edited(&after)
    );
}

/// A worktree whose link target is ignored, so editing it never enters the
/// changed set.
fn ignored_target_link(name: &str, target: &str) -> Option<Scratch> {
    let scratch = Scratch::new(name);
    scratch.write(".gitignore", "blob/\n");
    scratch.write("blob/a.txt", "AAAA\n");
    scratch.write("blob/b.txt", "BBBB\n");
    committed_link(&scratch, target, "link.txt").then_some(scratch)
}

#[test]
fn a_repointed_symlink_is_not_reused_from_the_targets_fingerprint() {
    // Mutation-sensitive on all three tier-1 targets, and for two different reasons,
    // which is worth recording rather than leaving to be rediscovered.
    let Some(scratch) = ignored_target_link("frame-symlink-repoint", "blob/a.txt") else {
        return;
    };

    // Two spellings of one file, so the followed metadata is byte-identical by
    // construction.
    assert!(scratch.symlink_file("blob/b.txt", "link.txt"));

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    let before = diffs(&mut frame);

    // Stat the target directly rather than through the link.
    let target = || {
        let meta = std::fs::metadata(scratch.path_of("blob/b.txt")).expect("stat the target");
        (meta.len(), meta.modified().expect("mtime"))
    };
    let resolved_before = target();

    assert!(scratch.symlink_file("blob/../blob/b.txt", "link.txt"));

    // The non-vacuity that makes this gate about the fingerprint and nothing else: the
    // file both spellings name did not move.
    assert_eq!(
        resolved_before,
        target(),
        "the two spellings do not name one unchanged file, so a fingerprint that \
         followed the link could notice this repoint and the gate proves nothing"
    );

    let after = diffs(&mut frame);
    assert!(
        before != after,
        "a repointed symlink was reused: the frame is still showing the old \
         target path, because its fingerprint followed the link to a file that \
         did not change"
    );
    assert_same(&after, &fresh(&worktree), "after a symlink was repointed");
}

#[test]
fn editing_a_symlinks_target_does_not_invalidate_the_links_diff() {
    // The other direction, and `SPEC.md` §7 asks for it by name: an invariant whose two
    // failure modes are not symmetrical gets a gate for each.
    let Some(scratch) = ignored_target_link("frame-symlink-target-edit", "blob/a.txt") else {
        return;
    };

    assert!(scratch.symlink_file("blob/b.txt", "link.txt"));

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    assert_eq!(
        frame.files().len(),
        1,
        "the changed set is not the link alone, so `computed` below is a total \
         rather than this file's number"
    );

    // Ignored, so this never joins the changed set. It is also the file the link
    // resolves to, which is the whole point.
    scratch.write("blob/b.txt", "EDITED, AND LONGER THAN BEFORE\n");

    let before = frame.stats();
    materialise(&mut frame);
    let cost = delta(before, frame.stats());

    assert_eq!(
        cost.computed, 0,
        "editing a link's *target* made the frame recompute the link's diff, so \
         the fingerprint is still following the link to a file git reports no \
         change to"
    );
    assert_eq!(
        cost.reused, 1,
        "the frame did not visit the link at all, so nothing above was tested"
    );
}

#[test]
fn a_symlink_read_reports_the_type_probe_it_spent() {
    // The gate over the counting itself, and without it the counting has no failing
    // test of its own.
    let Some(scratch) = ignored_target_link("frame-symlink-probe", "blob/a.txt") else {
        return;
    };
    assert!(scratch.symlink_file("blob/b.txt", "link.txt"));

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(
        frame.files().len(),
        1,
        "the changed set is not the link alone, so the counts below are totals"
    );

    // A cold first diff: one type probe to decide how to read, and one
    // fingerprint to record what was read. The reuse pre-check does not run,
    // because nothing is cached yet.
    let before = frame.stats();
    frame.diff(0).expect("diff the link");
    let cold = delta(before, frame.stats());

    assert_eq!(
        cold.computed, 1,
        "the link was not computed, so no read happened and no probe was due"
    );
    assert_eq!(
        cold.probes, 2,
        "a cold symlink read reported {} stats where two are due: one type probe \
         to choose `read_link` over `read`, and one fingerprint over the link. \
         One means the type probe is taken and not counted, which is what let \
         `maybe_symlink` be deleted with the suite green",
        cold.probes
    );

    // And the same read through the height walk, which reaches
    // `Worktree::measure` rather than `Worktree::diff`: a second call site that
    // has to report its own probe.
    let mut fresh_frame = worktree.frame();
    fresh_frame.advance().expect("advance");
    let before = fresh_frame.stats();
    fresh_frame.height(|_, _| 0).expect("height");
    let walked = delta(before, fresh_frame.stats());

    assert_eq!(
        walked.measured, 1,
        "the walk measured {} files, so it did not read the link",
        walked.measured
    );
    assert_eq!(
        walked.probes, 2,
        "the height walk reported {} stats over one symlink where two are due, \
         so `measure`'s type probe is uncounted even though `diff`'s is",
        walked.probes
    );
}
