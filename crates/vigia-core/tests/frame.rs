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

use support::{Scratch, committed_link, delta, materialise, settle, settle_spans};
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
///
/// **Every scalar field is checked before the lines are**, and that is not
/// belt-and-braces. `assert_same` compares whole structs, so a field this
/// function does not know about still fails the assertion, and the message would
/// then read "no line differs" and send a reader looking in the wrong place. It
/// happened to `lines` the moment that field was added.
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
    // The *producer* half of the racily-clean rule, and the half nothing else
    // here reaches. The unit tests over `settled` check the arithmetic; this
    // checks that the code recording a fingerprint actually consults it. Hard
    // code the answer to "trusted" and this is the test that goes red.
    //
    // Deterministic, with no race to lose: a filesystem floors the modification
    // time it stamps, so the granule a just-written file was stamped in may still
    // be open, and no fingerprint taken inside it can be trusted however much it
    // matches. The frame therefore has to re-read a recently written file on
    // *every* frame until its granule has provably closed, not merely on the
    // first frame after the write.
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

/// Total rows over every changed file, from spans alone.
///
/// One row per diff line, which is the shape the shell's `rows_of` reduces to
/// once chrome is taken out. It does not have to match what `vigia` draws; it
/// has to be the *same* function on both sides of every comparison below.
fn total_height(frame: &mut Frame) -> usize {
    frame.height(|_, span| span.lines as usize).expect("height")
}

#[test]
fn a_carried_span_does_not_survive_an_edit_the_viewport_never_saw() {
    // **The too-eager direction of [#101](https://github.com/breferrari/vigia/issues/101).**
    // Carrying a span across a tick makes the walk incremental, and the way to
    // get that wrong is to carry one whose file has since been rewritten: the
    // scrollbar is then scaled against a diff that no longer exists, silently,
    // for as long as nothing else touches that file.
    //
    // The edit is to a file **nothing has diffed**, which is the only case where
    // a carried span is the sole source of that file's height. A file on screen
    // is re-diffed anyway and its span is rebuilt from the fresh diff.
    //
    // This is the shape the whole module doc describes: reusing too little is
    // slow and loud, reusing too much is fast and wrong.
    let scratch = Scratch::large_diff("frame-span-edit", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();

    // Settle without materialising: every span comes from a read, no file has a
    // diff. `settle` would defeat this by diffing everything. The shared helper
    // rather than a sleep of its own, because the wait has to exceed the
    // engine's `SETTLE_MARGIN` and a literal here would not move when that does.
    let primed = settle_spans(&mut frame);
    let before = total_height(&mut frame);
    assert_eq!(
        frame.tracked(),
        0,
        "the frame holds {} diffs, so these spans did not come from reads and \
         this test is not about a carried span at all",
        frame.tracked()
    );

    // **The premise this test is named for, and without it the whole thing is
    // vacuous.** Every assertion below is about a span that was *carried*, and a
    // frame that carries nothing satisfies all of them: it re-measures, gets the
    // right answer, and looks identical from the outside. So prove a carry
    // happened first. An idle tick over files nothing touched must measure
    // nothing, which it can only do by trusting what it kept.
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

    // **And it proved them with a `stat` each, which nothing else asserts.**
    // Every other `probes` assertion in the repo covers `Frame::diff` or asserts
    // zero, so deleting `fill_span`'s own `probes` accounting left the suite
    // green — and `a_height_taken_from_a_diff_in_hand_costs_no_stat` is written
    // as `probes == 0`, so it is hollowed out by exactly that deletion. Found by
    // mutation.
    assert_eq!(
        idle.probes, FILES as u64,
        "an idle tick took {} stat calls to re-prove {FILES} carried spans, \
         so the walk is not counting the syscalls it makes",
        idle.probes
    );

    // Twice as many lines in a file the viewport never reached.
    scratch.rewrite_all(FILES, LINES * 2, 3);

    frame.advance().expect("advance");
    let after = total_height(&mut frame);

    // The oracle: a frame with no memory at all, over the same worktree.
    let mut cold = worktree.frame();
    cold.advance().expect("advance");
    let truth = total_height(&mut cold);

    assert!(
        after > before,
        "the diff doubled and the height stayed at {before}, so a span survived \
         the content it was taken from"
    );
    assert_eq!(
        after, truth,
        "the frame reports a {after}-row diff where a frame with no memory \
         computes {truth}, so a carried span is being trusted past its file"
    );
}

#[test]
fn a_height_taken_from_a_diff_in_hand_costs_no_stat() {
    // **The order of `fill_span`'s three sources, held structurally.** A file
    // whose diff the frame already holds needs no evidence: its height is a fold
    // over hunks the frame owns, which is free and is what this cost before
    // [#101](https://github.com/breferrari/vigia/issues/101) existed. Asking for
    // a fingerprint first reads as "cheapest first" and is a syscall bought for
    // nothing.
    //
    // **Written because the wrong order shipped and the wall clock nearly let it
    // through.** With the stat first,
    // `budgets.rs::the_frame_budget_holds_through_a_bulk_rewrite` went from
    // 8.27ms p50 to 11.12ms and from passing 4 of 4 local runs to 2 of 4: a
    // flake, which is the failure mode a percentile gate reports least clearly
    // and which a hosted runner would have blamed on itself. A count cannot be
    // flaky.
    //
    // The bulk rewrite is the shape that makes it visible: every print moves at
    // once, so every carried span mismatches, and the mismatch repeats on every
    // later frame because the span is rebuilt from the same stale diff each time.
    //
    // **In this crate rather than in the shell's `reads.rs`**, where it was first
    // written. It touches no `App`, no `Highlighter`, no viewport and no paint,
    // and its assertions are core counters over a core-private ordering. A gate
    // that pins a `vigia-core` function has to be able to fail in
    // `cargo test -p vigia-core`, or the crate can be reordered green. Its
    // sibling `reads.rs::a_tick_re_measures_only_what_changed` stays there
    // because it genuinely needs a drawn screen to make the drawn/undrawn split.
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

    // Every print moves, and no diff is recomputed: the frame is asked for the
    // height and nothing else.
    scratch.rewrite_all(REWRITTEN, LINES, 9);

    let before = frame.stats();
    frame.advance().expect("advance");
    total_height(&mut frame);
    let first = delta(before, frame.stats());

    // And again, so a per-tick proof cannot be mistaken for a per-frame one.
    let before = frame.stats();
    frame.advance().expect("advance");
    total_height(&mut frame);
    let second = delta(before, frame.stats());

    for (label, cost) in [("the tick after the rewrite", first), ("the next", second)] {
        assert_eq!(
            cost.probes, 0,
            "{label} took {} stat calls to total the height of {REWRITTEN} files \
             the frame already holds diffs for, so the walk is proving what it \
             could have read for free",
            cost.probes
        );
        assert_eq!(
            cost.measured, 0,
            "{label} measured {} files that were already in hand",
            cost.measured
        );
    }
}

#[test]
fn a_failed_measure_is_asked_again_rather_than_carried() {
    // **The narrowest arm in `fill_span`, and the one where carrying is
    // wrong.** A read that failed describes nothing, so its span is zero. Zero is
    // the right answer for *this* tick — a file that vanished has no height — and
    // the wrong thing to inherit.
    //
    // The hazard is specific to a change with **no working-tree side**. For every
    // other kind the evidence refuses itself: `reusable` treats a missing
    // fingerprint as unprovable. A `Removed` diff is computed from the index
    // alone, so `reusable` answers on the kind and the blob and returns **true**,
    // and a zero row-count would then be pinned for the life of the frame with no
    // retry. `Measured::taken` is `None` on this path precisely to stop that.
    //
    // Reaching it needs an object the repository cannot read, which is
    // `Error::MissingBlob`: legitimate during a `git gc` or over a partial clone,
    // and reproducible here by deleting the loose object a removed file's index
    // entry points at.
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
    // **The racily-clean case, on the one file whose staleness invalidates every
    // other file's answer.** Two writes of the same length inside one
    // modification-time granule are indistinguishable by `stat`, which is what
    // [`settled`] exists for, and 13 bytes of `a.txt binary` becoming 13 bytes of
    // `b.txt binary` is exactly that shape. Comparing bare fingerprints calls the
    // attributes unchanged and leaves every cached diff and span computed under
    // rules that moved, permanently, because nothing will touch that file again.
    //
    // **Forced rather than raced.** The gap between two writes here is a few
    // milliseconds and the granule observed on this volume is about one, so a
    // natural attempt misses: the mutation that drops the `settled` term survived
    // forty of forty tries. Stamping both writes with the same modification time
    // makes the collision certain, which is what a one or two second granule
    // (ext3, HFS+, FAT, exFAT) does on its own, and those are the volumes
    // `SETTLE_MARGIN` is sized against.
    //
    // What is still uncovered is a writer that restores an *older* modification
    // time, which is [#16](https://github.com/breferrari/vigia/issues/16) and is a
    // limit of every reuse decision in this file rather than of this one.
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
    // I3's half of #101. Spans cleared whole on every `advance` cannot
    // accumulate; carrying them across ticks makes the map a
    // **fourth** retained cache, and every retained cache in this repo has to be
    // bounded by something and asserted rather than trusted.
    //
    // The bound is the changed set, exactly as it is for the diffs beside it.
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

    // Pin the boundary itself, so a `gix` bump that fixes #13 makes this test
    // say so instead of leaving the comment above quietly wrong. Twenty bytes is
    // the first length that reports rather than aborts, so it must still report.
    // The panicking side cannot be asserted from here: the release profile sets
    // `panic = "abort"`, so a `#[should_panic]` test would take the whole binary
    // with it.
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

/// The cache-key gate for `FileDiff::lines`.
///
/// #39 asked for a line count cached per `(path, blob id)`. It is cached with
/// the **diff** instead, which is the stricter key: a blob id names the index
/// side, and a working-tree edit does not touch it. This is the case that
/// separates the two, and it is the case a byte-length check cannot see.
///
/// The file keeps its length in bytes and changes its length in lines. A
/// fingerprint is `(len, mtime)`, so the length half says nothing here and the
/// whole question is whether the diff was invalidated at all. If it was not, the
/// heat strip would go on projecting hunk positions across a file length that
/// stopped being true.
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

// ---------------------------------------------------------------------------
// Symlinks: the half of [#15](https://github.com/breferrari/vigia/issues/15)
// that lives here rather than in `fidelity.rs`.
//
// The read and the fingerprint had to move together. `Worktree::read_worktree`
// now takes a link's content from `read_link`, so a `fingerprint` that still
// followed the link would fingerprint one file and diff another, and the
// staleness rule would be reading a term that has nothing to do with what it
// governs. Neither direction of that is visible in a diff's *contents* with the
// read already fixed, which is why both gates below read `FrameStats` instead:
// what changed is whether the frame recomputed, not what it computed.
// ---------------------------------------------------------------------------

/// A worktree whose link target is **ignored**, so editing it never enters the
/// changed set.
///
/// That is what makes `computed` an exact per-file number in the gates below
/// rather than a total two files contribute to. Returns `None` when the platform
/// or git declines to hold a symlink, already having said so.
fn ignored_target_link(name: &str, target: &str) -> Option<Scratch> {
    let scratch = Scratch::new(name);
    scratch.write(".gitignore", "blob/\n");
    scratch.write("blob/a.txt", "AAAA\n");
    scratch.write("blob/b.txt", "BBBB\n");
    committed_link(&scratch, target, "link.txt").then_some(scratch)
}

#[test]
fn a_repointed_symlink_is_not_reused_from_the_targets_fingerprint() {
    // **Mutation-sensitive on all three tier-1 targets, and for two different
    // reasons, which is worth recording rather than leaving to be rediscovered.**
    // Restore the following `fs::metadata` and on Linux and macOS this fails at
    // its own assertion: the old fingerprint resolved the link, found an
    // untouched file, and reused a diff that no longer described anything.
    //
    // On **Windows** it fails earlier and louder, inside `settle`, with "the
    // frame was still re-reading after 8 idle frames". That is not this test
    // being flaky, it is a second defect the same line carried: Windows refuses
    // to resolve a symlink whose stored target uses forward slashes, and forward
    // slashes are exactly what git stores. So `fs::metadata` returned `Err`, the
    // fingerprint was `None`, and **every symlink in a Windows worktree was
    // re-diffed on every frame, indefinitely** (I2a). Nothing measured that,
    // because no fixture in this repository held a symlink until now, which is
    // `SPEC.md` §7's fixture-axis rule naming `core.symlinks` and then this being
    // what was behind it.
    let Some(scratch) = ignored_target_link("frame-symlink-repoint", "blob/a.txt") else {
        return;
    };

    // Two spellings of **one file**, so the followed metadata is byte-identical
    // by construction. Two genuinely different targets would differ in mtime by
    // microseconds and a fingerprint that followed the link would invalidate by
    // accident, which makes the gate flaky rather than red: it would pass for
    // the wrong reason on a slow machine and fail to reproduce on a fast one.
    assert!(scratch.symlink_file("blob/b.txt", "link.txt"));

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    let before = diffs(&mut frame);

    // Stat the target **directly** rather than through the link. It is the same
    // number a following fingerprint would have taken, and it is the only
    // spelling of that question that works everywhere: Windows refuses to
    // resolve a symlink whose stored target uses forward slashes at all
    // (`ERROR_INVALID_NAME`), and forward slashes are what git stores.
    let target = || {
        let meta = std::fs::metadata(scratch.path_of("blob/b.txt")).expect("stat the target");
        (meta.len(), meta.modified().expect("mtime"))
    };
    let resolved_before = target();

    assert!(scratch.symlink_file("blob/../blob/b.txt", "link.txt"));

    // The non-vacuity that makes this gate about the fingerprint and nothing
    // else: the file both spellings name did not move. A frame path that still
    // followed has no term left that could tell these two apart, so if the
    // assertion below passes it is because the link's own metadata was read.
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
    // The other direction, and `SPEC.md` §7 asks for it by name: an invariant
    // whose two failure modes are not symmetrical gets a gate for each. Reusing
    // too little is the cheap failure here, and it is still a failure, because a
    // fingerprint that moves when git reports no change is a term that does not
    // mean what the rule reads as. `fidelity.rs` holds the walk's half of this;
    // this is the frame's.
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
    // **The gate over the counting itself, and without it the counting has no
    // failing test of its own.** Round 1 of this branch's audit found that a
    // type probe taken in `Worktree` and counted nowhere made
    // `FileChange::maybe_symlink` deletable with the whole suite green. Round 2
    // found the same criticism one level down: every fixture that asserts on
    // `probes` is built from *regular files*, so the new term is identically
    // zero in all of them and deleting the counting is green too.
    //
    // A symlink is what makes the term non-zero, so this is the only fixture in
    // the repository that can hold it. Delete the `*probes += 1` in
    // `read_worktree`, or the `stats.probes += probes` in either `Frame::diff`
    // or `Frame::fill_span`, and this goes red on its own.
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
