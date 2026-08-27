//! The budgets from `SPEC.md` §3, as gates rather than aspirations.
//!
//! The gates come in two tiers, because absolute wall-clock thresholds are a
//! weak instrument on a shared CI runner and a strong one on a known machine.
//!
//! **Structural gates** compare the engine against itself: bytes read for one
//! file versus all of them, time to diff one versus time to diff every one.
//! They are ratios, so they are hardware-independent, they take no slack, and
//! they are what actually catches the regression that matters. Making the frame
//! path re-diff everything is a 5x wall-clock change that a generous absolute
//! threshold would wave through, and a 100x ratio change that these cannot.
//!
//! **Absolute gates** hold the wall clock to the numbers in `SPEC.md`. They run
//! only in release, because the budgets were set against optimised code, and
//! they accept a slack multiplier so a hosted runner's variance does not read
//! as a code regression. A developer machine runs them with no slack at all.
//!
//! **I10 is the one budget that is not here**, and the pointer is worth a line
//! so nobody concludes it is ungated. Every gate in this file is either a ratio
//! between two fixtures or a duration; I10's budget is a **count** (256 paths,
//! a 120s window) with neither shape, and the fixture that puts it under
//! pressure is ten thousand paths rather than two worktrees. It lives in
//! `tests/history.rs`, named for the invariant, and `SPEC.md` §3's proof column
//! names that file the way I6's names `crates/vigia/tests/legibility.rs`.

mod support;

use std::cell::RefCell;
use std::time::Duration;

use support::{
    Scratch, absolute_gates_apply, budget, delta, highlight_delta, highlight_window, holds_p99,
    holds_p99_rounds, materialise, settle, time, time_cpu,
};
use vigia_core::{
    FileChange, Frame, FrameStats, HighlightStats, Highlighter, LineKind, RETAINED_HUNKS, Samples,
    Worktree,
};

/// I4: first paint on a 100k-line diff.
const I4_FIRST_PAINT: Duration = Duration::from_millis(100);
/// I7: startup to first paint.
const I7_STARTUP: Duration = Duration::from_millis(50);
/// I9: steady-state frame time.
const I9_FRAME: Duration = Duration::from_millis(16);

/// Files in the large fixture, and lines in each.
///
/// 100 x 500, rewritten line for line, is 50k removed plus 50k added: the
/// 100k-line diff I4 is written against.
const FILES: usize = 100;
const LINES: usize = 500;

/// Best of `n`, which measures what the code can do rather than what the
/// machine happened to be doing at the time.
fn best_of(n: usize, mut measure: impl FnMut() -> Duration) -> Duration {
    (0..n)
        .map(|_| measure())
        .min()
        .expect("at least one measurement")
}

fn changes_of(worktree: &Worktree) -> Vec<FileChange> {
    worktree
        .changes()
        .expect("enumerate")
        .map(|c| c.expect("change"))
        .collect()
}

/// Frames discarded before sampling begins.
///
/// I9 is a claim about *steady state*, so the cold path is out of scope by
/// definition: the first frames after a fixture is built pay for a cold page
/// cache and first-touch of every code path. Measured here, the cold frames run
/// to roughly 40ms against a warm p99 of 3ms.
const WARMUP_FRAMES: usize = 50;

/// Frames actually sampled.
///
/// A percentile needs enough samples to be one. At 30, nearest-rank p99 *is*
/// the maximum, so the gate would report the worst frame and call it a tail.
const SAMPLED_FRAMES: usize = 250;

/// Files in the small fixture, which differs from the large one only in count.
const FEW_FILES: usize = 4;

/// The path present in both fixtures, so the content compared is identical.
const SHARED_PATH: &str = "src/mod_0.rs";

/// The one-line edit, identical in both fixtures so the bytes are comparable.
const EDITED_LINE: &str = "fn edited() { let value = 0; }";

/// Settle a frame over `scratch`, edit one line, and report what the next frame
/// cost.
fn cost_of_a_one_line_edit(scratch: &Scratch, files: usize) -> FrameStats {
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    // Guard the fixture. Every assertion below is about a frame over `files`
    // changed files, and would measure nothing if it were not one.
    assert_eq!(
        frame.files().len(),
        files,
        "fixture reported {} changed files, not {files}",
        frame.files().len()
    );
    assert_eq!(
        frame.tracked(),
        files,
        "settled frame holds {} diffs for {files} changed files",
        frame.tracked()
    );

    scratch.edit_line(SHARED_PATH, 0, EDITED_LINE);

    let before = frame.stats();
    materialise(&mut frame);
    delta(before, frame.stats())
}

#[test]
fn a_frame_recomputes_only_what_changed() {
    // I2a, and the counts are exact rather than ratios, so this gate is
    // hardware-independent and takes no slack.
    //
    // Measured across two fixtures whose per-file content is identical and
    // whose file counts differ 25-fold, because the tempting form of this gate
    // compares one file against the sum of all files, and that form passes
    // against the exact regression it is written to catch: when every call
    // inflates by N the sum inflates by N too, and the ratio never moves.
    let few = Scratch::large_diff("i2a-few", FEW_FILES, LINES);
    let many = Scratch::large_diff("i2a-many", FILES, LINES);

    let few_cost = cost_of_a_one_line_edit(&few, FEW_FILES);
    let many_cost = cost_of_a_one_line_edit(&many, FILES);

    for (label, files, cost) in [("few", FEW_FILES, few_cost), ("many", FILES, many_cost)] {
        assert_eq!(
            cost.computed, 1,
            "{label}: one line changed in one file, and the frame recomputed \
             {} diffs",
            cost.computed
        );
        assert_eq!(
            cost.reused,
            (files - 1) as u64,
            "{label}: the frame reused {} of the {} diffs it was asked for, so \
             it did not visit every file",
            cost.reused,
            files - 1
        );
    }

    // The claim itself: identical content, edited identically, has to cost
    // identical bytes whether three other files changed or ninety-nine did.
    // There is no shared term here for a uniform inflation to cancel against.
    assert_eq!(
        few_cost.bytes, many_cost.bytes,
        "recomputing {SHARED_PATH} read {} bytes among {FEW_FILES} changed files \
         and {} among {FILES}, so the frame path reads files it was not asked \
         about",
        few_cost.bytes, many_cost.bytes
    );

    // Non-vacuity: a frame that read nothing would satisfy the line above.
    let on_disk = std::fs::metadata(many.path_of(SHARED_PATH))
        .expect("stat the fixture file")
        .len();
    assert!(
        many_cost.bytes >= on_disk,
        "the frame reported {} bytes for a {on_disk}-byte file, so it is not \
         measuring the read at all",
        many_cost.bytes
    );
    assert!(
        many_cost.bytes <= on_disk * 4,
        "recomputing one {on_disk}-byte file compared {} bytes",
        many_cost.bytes
    );

    // What a reuse costs. One `stat` per file visited, and no read: this is the
    // trade the whole design rests on, so it is asserted rather than assumed.
    assert_eq!(
        many_cost.probes,
        (FILES + 1) as u64,
        "visiting {FILES} files took {} stat calls; expected one each plus one \
         to re-fingerprint the file that changed",
        many_cost.probes
    );
    assert_eq!(
        many_cost.evicted, 0,
        "editing a file evicted {} cached diffs",
        many_cost.evicted
    );
}

#[test]
fn diffing_one_file_costs_the_same_however_much_else_changed() {
    // Comparing one call against the sum of all calls would prove nothing: if
    // `diff` inflated uniformly, the sum would inflate with it and the ratio
    // would hold. The claim only has teeth across two fixtures whose per-file
    // content is identical and whose file counts are not.
    let few = Scratch::large_diff("budget-scale-few", FEW_FILES, LINES);
    let many = Scratch::large_diff("budget-scale-many", FILES, LINES);

    let few_worktree = few.worktree();
    let many_worktree = many.worktree();
    let few_changes = changes_of(&few_worktree);
    let many_changes = changes_of(&many_worktree);
    assert_eq!(few_changes.len(), FEW_FILES);
    assert_eq!(many_changes.len(), FILES);

    let pick = |changes: &[FileChange]| {
        changes
            .iter()
            .find(|c| c.path == SHARED_PATH)
            .unwrap_or_else(|| panic!("{SHARED_PATH} missing from fixture"))
            .clone()
    };
    let in_few = pick(&few_changes);
    let in_many = pick(&many_changes);

    let few_diff = few_worktree.diff(&in_few).expect("diff");
    let many_diff = many_worktree.diff(&in_many).expect("diff");

    // Exact, and the strongest form of the claim: identical content has to cost
    // identical bytes whether three other files changed or ninety-nine did.
    assert_eq!(
        few_diff.bytes, many_diff.bytes,
        "diffing {SHARED_PATH} compared {} bytes among {FEW_FILES} changes but {} among {FILES}, \
         so the frame path is reading files it was not asked about",
        few_diff.bytes, many_diff.bytes
    );

    // And bounded by the file itself, which catches inflation that happens to
    // be uniform across both fixtures.
    let on_disk = std::fs::metadata(many.path_of(SHARED_PATH))
        .expect("stat the fixture file")
        .len();
    assert!(
        many_diff.bytes <= on_disk * 4,
        "diffing a {on_disk}-byte file compared {} bytes",
        many_diff.bytes
    );

    // The same claim in time. Loose, because timing on a shared machine is
    // noisy, but a frame path that walked every change would be 25x here.
    let few_time = best_of(7, || {
        time(|| {
            few_worktree.diff(&in_few).expect("diff");
        })
    });
    let many_time = best_of(7, || {
        time(|| {
            many_worktree.diff(&in_many).expect("diff");
        })
    });
    assert!(
        many_time <= few_time * 4,
        "diffing {SHARED_PATH} took {few_time:?} among {FEW_FILES} changes but {many_time:?} \
         among {FILES}"
    );
}

#[test]
fn absolute_budgets_hold_on_a_100k_line_diff() {
    let scratch = Scratch::large_diff("budget-absolute", FILES, LINES);
    let worktree = scratch.worktree();
    let changes = changes_of(&worktree);

    // Guard the fixture itself. If it silently shrank, every gate below would
    // pass while measuring nothing like what the budgets describe.
    let lines: u32 = changes
        .iter()
        .map(|c| {
            let diff = worktree.diff(c).expect("diff");
            diff.added + diff.removed
        })
        .sum();
    assert!(
        lines >= 100_000,
        "fixture is only {lines} changed lines, so it does not exercise the I4 budget"
    );

    if !absolute_gates_apply("cargo test --release -p vigia-core --test budgets") {
        return;
    }

    // I4: the first change has to be available long before the walk finishes.
    let first_change = best_of(5, || {
        time(|| {
            let mut stream = worktree.changes().expect("enumerate");
            stream.next().expect("a change").expect("change");
        })
    });
    assert!(
        first_change <= budget(I4_FIRST_PAINT),
        "I4: first change took {first_change:?}, over the {:?} budget",
        budget(I4_FIRST_PAINT)
    );

    // I7: open the repository, find the first change, and diff it. Process
    // spawn and dynamic loading are outside a test's reach, so this gates the
    // part the code controls; the harness reports the number including them.
    let root = scratch.path_of(".");
    let first_paint = best_of(5, || {
        time(|| {
            let worktree = Worktree::discover(&root).expect("discover");
            let change = worktree
                .changes()
                .expect("enumerate")
                .next()
                .expect("a change")
                .expect("change");
            worktree.diff(&change).expect("diff");
        })
    });
    assert!(
        first_paint <= budget(I7_STARTUP),
        "I7: open plus first paint took {first_paint:?}, over the {:?} budget",
        budget(I7_STARTUP)
    );

    // I9 over the primitives: one enumeration plus the file that changed. This
    // is the floor, not a frame any monitor has, because it has no memory of the
    // frame before it. `a_real_frame_holds_the_frame_budget` gates the shape a
    // monitor actually drives. p99 rather than a mean, because the budget is a
    // tail claim.
    let frame = || {
        time_cpu(|| {
            let change = worktree
                .changes()
                .expect("enumerate")
                .next()
                .expect("a change")
                .expect("change");
            worktree.diff(&change).expect("diff");
        })
    };
    for _ in 0..WARMUP_FRAMES {
        frame();
    }
    let mut frames = Samples::new(SAMPLED_FRAMES);
    for _ in 0..SAMPLED_FRAMES {
        frames.push(frame().0);
    }
    holds_p99(
        "I9: an enumeration plus the file that changed",
        budget(I9_FRAME),
        &frames,
        String::new,
        frame,
    );
}

#[test]
fn a_real_frame_holds_the_frame_budget() {
    // I9 through the type a monitor drives, which the gate above does not touch:
    // it times the primitives, and a frame path with no memory of the previous
    // frame is not a frame shape that exists once I2a is in.
    //
    // "Under continuous edits" is taken literally. One line is rewritten before
    // every frame, so each frame revalidates ninety-nine files and recomputes
    // the one that moved, which is the steady state the budget describes. The
    // edit is deliberately outside the timed region: it stands in for the agent
    // in the other pane, and its cost is not ours.
    let scratch = Scratch::large_diff("i9-frame", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    assert_eq!(frame.files().len(), FILES, "fixture is not {FILES} files");

    if !absolute_gates_apply("cargo test --release -p vigia-core --test budgets") {
        return;
    }

    let mut edits = 0usize;
    // A cell rather than a `String`, because `holds_p99` re-measures on a breach,
    // so this closure is still live where the marker is read below. See the same
    // note in the shell's `budgets.rs`.
    let marker = RefCell::new(String::new());
    let mut next_frame = |frame: &mut vigia_core::Frame| {
        *marker.borrow_mut() = format!("fn edited_{edits}() {{ }}");
        scratch.edit_line(SHARED_PATH, 0, &marker.borrow());
        edits += 1;
        time_cpu(|| materialise(frame))
    };

    for _ in 0..WARMUP_FRAMES {
        next_frame(&mut frame);
    }

    let before = frame.stats();
    let mut frames = Samples::new(SAMPLED_FRAMES);
    for _ in 0..SAMPLED_FRAMES {
        frames.push(next_frame(&mut frame).0);
    }
    let cost = delta(before, frame.stats());

    // Non-vacuity, and it deliberately does not count recomputes. A recompute
    // count cannot show that the edits are landing: one write keeps its file
    // unsettled for the whole settle margin, so every frame inside that window
    // recomputes it whether or not anything is still being edited. Counting
    // would pass against a test that edited once and then sat still.
    //
    // What cannot be manufactured by the margin is the *content*: the frame has
    // to be handing back the last edit actually made.
    let last = marker.borrow().clone();
    let shared = frame
        .files()
        .iter()
        .position(|change| change.path == SHARED_PATH)
        .expect("the edited file is still a change");
    let (_, diff) = frame.diff(shared).expect("diff");
    assert!(
        diff.hunks.iter().any(|hunk| {
            hunk.lines
                .iter()
                .any(|line| line.kind == LineKind::Added && line.text == last)
        }),
        "the frame's diff for {SHARED_PATH} does not contain {last:?}, so the \
         edits stopped reaching it"
    );
    assert!(
        cost.computed <= (SAMPLED_FRAMES * 2) as u64,
        "{} recomputes over {SAMPLED_FRAMES} frames, so a frame is recomputing \
         files nobody edited",
        cost.computed
    );

    holds_p99(
        &format!("I9: a real frame over {FILES} files"),
        budget(I9_FRAME),
        &frames,
        || {
            format!(
                "({} recomputed, {} reused, {} read)",
                cost.computed, cost.reused, cost.bytes
            )
        },
        || next_frame(&mut frame),
    );
}

/// Lines between one change and the next in the I2b fixtures.
///
/// Comfortably over twice `CONTEXT`, so each change gets a hunk of its own and
/// the gates below can name one. `Scratch::sparse_edits` asserts the same thing
/// rather than trusting this constant to stay right.
const HUNK_SPACING: usize = 20;

/// Hunks one screenful of the sparse fixture shows.
///
/// Each hunk here is three context lines, a removal, an addition and three more
/// context, under a header: nine rows. Three of them is an ordinary terminal.
const WINDOW_HUNKS: usize = 3;

/// The two file sizes I2b's claim is measured across.
///
/// Ten-fold, and identical wherever they overlap, because `generated` writes the
/// same line at the same index whatever the file's length. That is what makes
/// the comparison like for like.
const SMALL_FILE: usize = 500;
const LARGE_FILE: usize = 5_000;

/// Bytes of one hunk's lines, which is what highlighting that hunk has to cost.
fn hunk_bytes(frame: &mut Frame, path: &str, ordinal: usize) -> u64 {
    let index = frame
        .files()
        .iter()
        .position(|change| change.path == path)
        .unwrap_or_else(|| panic!("{path} is not a changed file"));
    let (_, diff) = frame.diff(index).expect("diff");
    diff.hunks[ordinal]
        .lines
        .iter()
        .map(|line| line.text.len() as u64)
        .sum()
}

/// What re-highlighting cost after one line changed, and what it should have.
struct Rehighlight {
    /// What the **first** window cost, with nothing to reuse.
    ///
    /// Carried because the edit lands in hunk 1 of both fixtures, at the same
    /// offset from the top of each, so a design that parsed forward from the
    /// start of the file would cost the same in both and [`Rehighlight::cost`]
    /// alone could not tell. A cold window is where a cost that follows the
    /// file rather than the screen becomes visible.
    first: HighlightStats,
    cost: HighlightStats,
    /// Bytes of the single hunk the edit landed in.
    hunk: u64,
    /// Lines the window drew, so an empty window cannot pass a gate.
    drawn: usize,
}

/// Settle, draw a window, edit one line inside its middle hunk, and draw again.
///
/// The edit lands at `HUNK_SPACING * 2`, which the fixture puts in hunk 1: its
/// n-th change sits at line `n * HUNK_SPACING`, so hunk 0 holds the first and
/// hunk 1 the second. Hunks 0 and 2 are untouched and are what "reused" counts.
fn cost_of_rehighlighting_a_one_line_edit(scratch: &Scratch) -> Rehighlight {
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut highlighter = Highlighter::eager();
    let drawn = highlight_window(&mut frame, &mut highlighter, SHARED_PATH, 0, WINDOW_HUNKS).rows;
    assert_eq!(
        highlighter.stats().parsed,
        WINDOW_HUNKS as u64,
        "the first window parsed {} hunks rather than {WINDOW_HUNKS}, so the \
         measurement below starts from the wrong state",
        highlighter.stats().parsed
    );

    let first = highlighter.stats();
    scratch.edit_line(SHARED_PATH, HUNK_SPACING * 2, EDITED_LINE);

    let before = highlighter.stats();
    frame.advance().expect("advance");
    highlight_window(&mut frame, &mut highlighter, SHARED_PATH, 0, WINDOW_HUNKS);

    Rehighlight {
        first,
        cost: highlight_delta(before, highlighter.stats()),
        hunk: hunk_bytes(&mut frame, SHARED_PATH, 1),
        drawn,
    }
}

#[test]
fn only_the_hunk_that_changed_is_reparsed() {
    // I2b, and the counts are exact rather than ratios, so this gate is
    // hardware-independent and takes no slack.
    //
    // The fixture matters as much as the assertions. `large_diff` rewrites every
    // line and so produces exactly **one** hunk per file, where reusing nothing
    // and reusing everything are indistinguishable. `sparse_edits` gives a file
    // many small hunks, which is the only shape in which "only the one that
    // changed" is a claim at all.
    let scratch = Scratch::sparse_edits("i2b-hunks", 1, SMALL_FILE, HUNK_SPACING);
    let after = cost_of_rehighlighting_a_one_line_edit(&scratch);

    assert!(
        after.drawn > 0,
        "the window drew no lines, so nothing was highlighted either way"
    );
    assert_eq!(
        after.cost.parsed, 1,
        "one line changed in one hunk, and the window re-parsed {} hunks",
        after.cost.parsed
    );
    assert_eq!(
        after.cost.reused,
        (WINDOW_HUNKS - 1) as u64,
        "the window reused {} of the {} hunks it drew, so the hunks nobody \
         touched were thrown away",
        after.cost.reused,
        WINDOW_HUNKS - 1
    );

    // The strongest form: not merely "one hunk" but *that* hunk's bytes and no
    // others'. A re-parse that quietly took the whole window would still report
    // one hunk parsed.
    assert_eq!(
        after.cost.bytes, after.hunk,
        "re-highlighting read {} bytes for a hunk of {}",
        after.cost.bytes, after.hunk
    );
    assert!(
        after.hunk > 0,
        "the hunk the edit landed in is empty, so the equality above is two \
         zeroes agreeing"
    );
}

#[test]
fn reparsing_after_an_edit_costs_the_same_however_large_the_file() {
    // I2b's budget: re-parse follows the edit, not the file. One fixture cannot
    // say that, for the reason `SPEC.md` §3 gives on both I2a's row and this
    // one. The two differ ten-fold in length and are byte-identical wherever
    // they overlap, so the hunk the edit lands in holds the same content in
    // both and there is no shared term for a uniform inflation to cancel
    // against.
    let small = Scratch::sparse_edits("i2b-small", 1, SMALL_FILE, HUNK_SPACING);
    let large = Scratch::sparse_edits("i2b-large", 1, LARGE_FILE, HUNK_SPACING);

    let in_small = cost_of_rehighlighting_a_one_line_edit(&small);
    let in_large = cost_of_rehighlighting_a_one_line_edit(&large);

    // Guard the fixtures first, or the equalities below are two zeroes agreeing.
    assert!(in_small.cost.bytes > 0 && in_small.cost.lines > 0);
    assert_eq!(
        in_small.hunk, in_large.hunk,
        "the edited hunk is {} bytes in the {SMALL_FILE}-line file and {} in \
         the {LARGE_FILE}-line one, so the two fixtures do not overlap and \
         nothing below compares like with like",
        in_small.hunk, in_large.hunk
    );

    assert_eq!(
        in_small.cost.bytes, in_large.cost.bytes,
        "one line changed cost {} bytes of re-parsing in a {SMALL_FILE}-line \
         file and {} in a {LARGE_FILE}-line one, so the cost follows the file \
         rather than the edit",
        in_small.cost.bytes, in_large.cost.bytes
    );
    assert_eq!(
        in_small.cost.lines, in_large.cost.lines,
        "{} lines re-parsed in the small file against {} in the large one",
        in_small.cost.lines, in_large.cost.lines
    );
    assert_eq!(in_small.cost.parsed, in_large.cost.parsed);

    // And the same claim about the **cold** window, which is the half the
    // re-highlight cost cannot make. The edit lands in hunk 1 of both fixtures,
    // at the same offset from the top of each, so a highlighter that parsed
    // forward from the start of the file would re-parse identical amounts in
    // both and every equality above would hold. What such a design cannot hide
    // is the first window, where a cost that follows the file rather than the
    // screen shows up at full size.
    assert_eq!(
        in_small.drawn, in_large.drawn,
        "the first window drew {} lines in the {SMALL_FILE}-line file and {} in \
         the {LARGE_FILE}-line one",
        in_small.drawn, in_large.drawn
    );
    assert_eq!(
        in_small.first.bytes, in_large.first.bytes,
        "a cold window parsed {} bytes in the {SMALL_FILE}-line file and {} in \
         the {LARGE_FILE}-line one, so highlighting reaches past what it draws",
        in_small.first.bytes, in_large.first.bytes
    );
    assert_eq!(
        in_small.first.lines, in_large.first.lines,
        "a cold window parsed {} lines against {}",
        in_small.first.lines, in_large.first.lines
    );
}

#[test]
fn a_redraw_inside_the_settle_margin_reparses_nothing() {
    // The gate that decides how a hunk is identified, and the reason the cache
    // is keyed on content rather than on a counter the frame path bumps.
    //
    // For two seconds after a write the frame path cannot prove the file
    // unchanged, so it re-diffs it on **every** frame by design. A highlighter
    // keyed on "did the frame recompute this diff" would therefore re-parse
    // every hunk of an edited file, every frame, for the whole margin. Content
    // keying costs a hash and is exact.
    let scratch = Scratch::sparse_edits("i2b-margin", 1, SMALL_FILE, HUNK_SPACING);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut highlighter = Highlighter::eager();
    highlight_window(&mut frame, &mut highlighter, SHARED_PATH, 0, WINDOW_HUNKS);

    // Land an edit, and stay inside its margin for the rest of the test.
    scratch.edit_line(SHARED_PATH, HUNK_SPACING * 2, EDITED_LINE);
    frame.advance().expect("advance");
    highlight_window(&mut frame, &mut highlighter, SHARED_PATH, 0, WINDOW_HUNKS);

    let frame_before = frame.stats();
    let highlight_before = highlighter.stats();
    frame.advance().expect("advance");
    highlight_window(&mut frame, &mut highlighter, SHARED_PATH, 0, WINDOW_HUNKS);
    let frame_cost = delta(frame_before, frame.stats());
    let cost = highlight_delta(highlight_before, highlighter.stats());

    // Non-vacuity, and it is the whole point: the frame really did recompute
    // the diff, which is the condition a counter-keyed cache would trip over.
    assert!(
        frame_cost.computed > 0,
        "the frame reused its diff, so this test never reached the settle \
         margin it exists for"
    );
    assert_eq!(
        cost.parsed, 0,
        "{} hunks were re-parsed for a diff that was recomputed but identical",
        cost.parsed
    );
    assert_eq!(cost.lines, 0, "{} lines were re-parsed", cost.lines);
    assert_eq!(cost.bytes, 0, "{} bytes were re-parsed", cost.bytes);
    assert_eq!(
        cost.reused, WINDOW_HUNKS as u64,
        "the window drew {WINDOW_HUNKS} hunks and reused {}",
        cost.reused
    );
}

#[test]
fn the_highlight_cache_is_bounded_by_the_viewport() {
    // I3's shape, held against the one structure this change adds. The frame
    // path's own map is bounded by the current diff; this is bounded by the
    // screen, which is the stronger claim and the one that survives a reader
    // scrolling through a large file for a day.
    const STEPS: usize = 30;

    let scratch = Scratch::sparse_edits("i2b-bounded", 1, LARGE_FILE, HUNK_SPACING);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut highlighter = Highlighter::eager();
    for first in 0..STEPS {
        highlight_window(
            &mut frame,
            &mut highlighter,
            SHARED_PATH,
            first,
            WINDOW_HUNKS,
        );
        // The window, plus the hunks a reader may scroll back to. The second
        // term is a constant rather than a function of how far the loop has
        // gone, which is what keeps this a claim about the screen: at step 29
        // the reader has read thirty-two hunks and the cache holds seven.
        assert!(
            highlighter.tracked() <= WINDOW_HUNKS + RETAINED_HUNKS,
            "after scrolling to hunk {first} the cache holds {} hunks for a \
             window of {WINDOW_HUNKS} and a retired queue of {RETAINED_HUNKS}, \
             so it grows with what has been read rather than with what is on \
             screen",
            highlighter.tracked()
        );
    }

    // Non-vacuity: a cache that never stored anything would satisfy the bound,
    // and one that never *retired* anything would satisfy it for the wrong
    // reason. Both ends are pinned to equality, so the gate is pressed rather
    // than merely true.
    assert_eq!(highlighter.tracked(), WINDOW_HUNKS + RETAINED_HUNKS);

    // Each step retires the one hunk that left the window, and the queue holds
    // the last `RETAINED_HUNKS` of them. Everything before that is gone.
    assert_eq!(
        highlighter.stats().evicted,
        (STEPS - 1 - RETAINED_HUNKS) as u64,
        "scrolling {STEPS} hunks evicted {}, so hunks are being kept after they \
         leave the screen and the retired queue is not turning over",
        highlighter.stats().evicted
    );
}

#[test]
fn a_hunk_scrolled_back_to_is_not_re_parsed() {
    // The invariant `RETAINED_HUNKS` exists for, and the one the viewport bound
    // above cannot express: it says the cache does not grow, and a cache of size
    // zero satisfies that perfectly.
    //
    // Highlighting is forward-only, so a hunk re-entered from *below* is parsed
    // from its first line down to the visible row. Sweeping on exit meant a
    // reader who scrolled back over what they had just read paid that walk
    // again, with the answer having been in memory one frame earlier: measured
    // over 120-row hunks of Japanese, **26.39ms** against a 16ms budget, once
    // per file ([#45](https://github.com/breferrari/vigia/issues/45)).
    let scratch = Scratch::sparse_edits("i2b-scrollback", 1, LARGE_FILE, HUNK_SPACING);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut highlighter = Highlighter::eager();

    // Read forward over twice the window, so the hunks left behind are genuinely
    // off screen rather than still half drawn.
    for first in 0..=WINDOW_HUNKS {
        highlight_window(
            &mut frame,
            &mut highlighter,
            SHARED_PATH,
            first,
            WINDOW_HUNKS,
        );
    }

    // The premise, checked before the return rather than inferred from it: the
    // three hunks really did leave the screen. Held but not drawn is exactly
    // what the queue is, and without that this would be measuring a window that
    // never moved.
    assert_eq!(
        highlighter.tracked(),
        WINDOW_HUNKS * 2,
        "the cache holds {} hunks after reading {} windows of {WINDOW_HUNKS}, \
         so the hunks about to be scrolled back to are not sitting in the \
         retired queue and this proves nothing",
        highlighter.tracked(),
        WINDOW_HUNKS + 1
    );

    // Then back to where the reader started, which is the motion a wheel flick
    // and its reversal produce.
    let before = highlighter.stats();
    let drawn = highlight_window(&mut frame, &mut highlighter, SHARED_PATH, 0, WINDOW_HUNKS).rows;
    let cost = highlight_delta(before, highlighter.stats());

    assert!(drawn > 0, "the window back at the top drew nothing");
    assert_eq!(
        cost.parsed, 0,
        "scrolling back over {WINDOW_HUNKS} hunks already read re-parsed {} of \
         them, so the retired queue is not being consulted",
        cost.parsed
    );
    assert_eq!(
        cost.lines, 0,
        "scrolling back re-parsed {} lines, so a hunk came out of the queue and \
         was rebuilt rather than reused",
        cost.lines
    );
    assert_eq!(
        cost.reused, WINDOW_HUNKS as u64,
        "the window back at the top reused {} of {WINDOW_HUNKS} hunks",
        cost.reused
    );
}

/// Print the whole frame-time distribution instead of asserting on it.
///
/// Ignored by default because it is a diagnostic rather than a gate. It exists
/// because the I9 gate above was once wrong in two ways at once, sampling too
/// few frames to have a p99 and sampling them cold, and a single failing number
/// could not distinguish that from a real regression. Run it before believing
/// the gate is what is broken:
///
/// ```text
/// cargo test --release --test budgets -- --ignored --nocapture distribution
/// ```
#[test]
#[ignore = "diagnostic, not a gate"]
fn frame_time_distribution() {
    let scratch = Scratch::large_diff("budget-distribution", FILES, LINES);
    let worktree = scratch.worktree();

    let mut warm = Samples::new(SAMPLED_FRAMES);
    let mut over_budget = 0usize;
    for i in 0..(WARMUP_FRAMES + SAMPLED_FRAMES) {
        let elapsed = time(|| {
            let change = worktree
                .changes()
                .expect("enumerate")
                .next()
                .expect("a change")
                .expect("change");
            worktree.diff(&change).expect("diff");
        });
        if i >= WARMUP_FRAMES {
            warm.push(elapsed);
            if elapsed > I9_FRAME {
                over_budget += 1;
            }
        }
    }

    for quantile in [0.50, 0.90, 0.99, 0.999] {
        println!(
            "p{:<6} {:?}",
            quantile * 100.0,
            warm.percentile(quantile).expect("samples")
        );
    }
    println!("max     {:?}", warm.max().expect("samples"));
    println!("over {I9_FRAME:?}: {over_budget}/{SAMPLED_FRAMES} warm frames");
}

#[test]
fn a_hunk_edited_while_off_screen_is_re_parsed_when_it_comes_back() {
    // The other half of `a_hunk_scrolled_back_to_is_not_re_parsed`, and the one
    // that decides whether retention is *correct* rather than merely fast.
    //
    // This is the monitor's whole workload: the agent in the other pane is
    // writing while the reader scrolls. So a retained parse is a parse of
    // content that may no longer exist, and handing it back unchecked would show
    // colours for a version of the file nobody has. Reuse must lose to the
    // digest, exactly as it does for a hunk that never left the screen.
    //
    // The failure this rules out is silent and permanent: nothing on screen says
    // the colours are stale, and the entry stays wrong until it is evicted.
    let scratch = Scratch::sparse_edits("i2b-scrollback-edit", 1, LARGE_FILE, HUNK_SPACING);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut highlighter = Highlighter::eager();

    // Read the top of the file, then move past it so it is retired rather than
    // live. Off screen and still held is precisely the state under test.
    highlight_window(&mut frame, &mut highlighter, SHARED_PATH, 0, WINDOW_HUNKS);
    for first in 1..=WINDOW_HUNKS {
        highlight_window(
            &mut frame,
            &mut highlighter,
            SHARED_PATH,
            first,
            WINDOW_HUNKS,
        );
    }
    assert_eq!(
        highlighter.tracked(),
        WINDOW_HUNKS * 2,
        "hunk 0 is not being held off screen, so this measures a cache that was \
         never asked to give back something stale"
    );

    // The agent writes to the part the reader has scrolled away from.
    scratch.edit_line(SHARED_PATH, HUNK_SPACING, EDITED_LINE);
    frame.advance().expect("advance");

    // And the reader scrolls back into it.
    let before = highlighter.stats();
    highlight_window(&mut frame, &mut highlighter, SHARED_PATH, 0, WINDOW_HUNKS);
    let cost = highlight_delta(before, highlighter.stats());

    assert_eq!(
        cost.parsed, 1,
        "coming back to a window whose first hunk was rewritten while it was \
         retired re-parsed {} hunks: 0 means a stale parse was handed back, and \
         more than 1 means the hunks that did not change were thrown away with \
         it",
        cost.parsed
    );
    assert_eq!(
        cost.reused,
        (WINDOW_HUNKS - 1) as u64,
        "the untouched hunks in the window were re-parsed rather than recovered",
    );
    assert!(
        cost.lines > 0,
        "the changed hunk was reported as parsed and cost no lines, so nothing \
         was actually re-read"
    );
}

/// A distribution of `slow` samples among `n`, the rest at 1ms.
///
/// Ten samples, because nearest-rank p99 over ten *is* the maximum, so one slow
/// sample moves the tail and leaves the median alone. That is the shape the whole
/// retry exists for, in the smallest fixture that can express it.
fn spiked(n: usize, slow: usize) -> Samples {
    let mut samples = Samples::new(n);
    for at in 0..n {
        samples.push(if at < n - slow {
            Duration::from_millis(1)
        } else {
            Duration::from_millis(500)
        });
    }
    samples
}

/// Every sample slow on the wall clock, and `cpu` of CPU time each.
///
/// The pair is the whole subject of the gates below: a stall is a wall clock nobody
/// can explain from the work that was done.
fn every_sample(wall: u64, cpu: u64) -> impl FnMut() -> (Duration, Duration) {
    move || (Duration::from_millis(wall), Duration::from_millis(cpu))
}

#[test]
fn a_breached_p99_is_re_measured_before_it_is_believed() {
    // **The instrument gets its own gates**, because #178's fix is a claim about
    // the measurement rather than about the code, and prose about a measurement
    // proves nothing. One stalled sample in the first round, a clean second round:
    // the gate holds, and the log says it had to look twice.
    holds_p99(
        "a probe that stalled once",
        Duration::from_millis(10),
        &spiked(10, 1),
        String::new,
        every_sample(1, 1),
    );
}

#[test]
fn a_wall_clock_breach_the_cpu_clock_acquits_is_reported_and_not_failed() {
    // **The attribution, and it is why the CPU clock was worth two test-only
    // dependencies.** Every sample in the second round is 500ms of wall clock and
    // 1ms of work: the process was not running, and no frame path getting slower
    // could produce that shape. This is what #178's three occurrences were, and it
    // is the only weakening in the whole ruling.
    holds_p99(
        "a probe on a runner that took the CPU away",
        Duration::from_millis(10),
        &spiked(10, 10),
        String::new,
        every_sample(500, 1),
    );
}

#[test]
#[should_panic(expected = "work done")]
fn a_p99_that_breaches_twice_on_work_done_still_fails() {
    // The half that keeps this a gate rather than a retry loop, and #178's own
    // acceptance: the unchanged code must still fail when the budget is genuinely
    // breached. Wall and CPU both out, so the frame path is what moved, and the
    // message says which.
    holds_p99(
        "a probe that is simply slow",
        Duration::from_millis(10),
        &spiked(10, 10),
        String::new,
        every_sample(500, 500),
    );
}

#[test]
#[should_panic(expected = "no CPU clock")]
fn a_breach_with_no_cpu_clock_is_treated_as_ours() {
    // The fallback, asserted rather than assumed: where there is no thread clock an
    // overshoot cannot be attributed, and an unattributable breach is ours. Failing
    // closed is the only safe direction for a gate.
    holds_p99_rounds(
        "a probe on a platform with no clock",
        Duration::from_millis(10),
        &spiked(10, 10),
        String::new,
        || (spiked(10, 10), None),
    );
}

#[test]
fn the_cpu_clock_tells_waiting_from_working() {
    // **Non-vacuity for the two gates above, and the only thing here that touches
    // the platform.** A clock stuck at zero would make every breach look like
    // somebody else's load; a clock that returned the wall clock would make every
    // stall look like a regression. Both directions are checked, and the second is
    // a sleep, because a sleep is exactly the case the attribution rests on.
    // **Bounded by a deadline rather than by an iteration count**, and that is a
    // correction: eighty million multiplications is a closed form, so release folded
    // the whole loop and the gate measured two hundred nanoseconds of nothing. A
    // wall-clock deadline with an opaque accumulator is busy for as long as it says
    // whatever the optimiser does.
    //
    // Eighty milliseconds because of the coarsest clock in play: `GetThreadTimes` is
    // quantized to the scheduler tick, about 15.6ms, so a few milliseconds of work
    // reports zero CPU on Windows and would say nothing about the clock.
    let busy = Duration::from_millis(80);
    let (wall, cpu) = time_cpu(|| {
        let deadline = std::time::Instant::now() + busy;
        let mut sum = 0u64;
        while std::time::Instant::now() < deadline {
            for at in 0..10_000u64 {
                sum = sum.wrapping_add(std::hint::black_box(at).wrapping_mul(2_654_435_761));
            }
        }
        std::hint::black_box(sum);
    });
    assert!(
        cpu >= busy / 2,
        "the CPU clock reported {cpu:?} for {wall:?} of work that never waited for \
         anything, so a real overshoot would read as somebody else's load"
    );

    let (slept_wall, slept_cpu) = time_cpu(|| std::thread::sleep(Duration::from_millis(60)));
    assert!(
        slept_wall >= Duration::from_millis(50),
        "a sixty millisecond sleep measured {slept_wall:?} of wall clock"
    );
    assert!(
        slept_cpu < Duration::from_millis(20),
        "a sixty millisecond sleep measured {slept_cpu:?} of CPU time, so this clock \
         cannot tell waiting from working and the whole attribution is unsound"
    );
}

/// `n` samples of `ms` each, with no spike at all.
///
/// The shape the three gates above never build: they vary *what* a sample costs
/// and hold the count at ten. The defect [#269](https://github.com/breferrari/vigia/issues/269)
/// records rides on the **count**, so a fixture that cannot vary it cannot see it.
fn flat(n: usize, ms: u64) -> Samples {
    let mut samples = Samples::new(n);
    for _ in 0..n {
        samples.push(Duration::from_millis(ms));
    }
    samples
}

#[test]
#[should_panic(expected = "work done")]
fn a_long_round_slightly_over_budget_on_work_is_not_excused_by_accumulated_noise() {
    // **The gate this instrument was missing, and the defect it now catches was
    // live.** The acquittal compares the round's off-CPU deficit against what the
    // round spent over budget. Both are sums, so both grow with the sample count
    // and the comparison is stable. Comparing that sum against a *single*
    // frame's overshoot, which does not grow, lets ordinary per-frame
    // scheduling
    // noise out-accumulated the thing it had to explain and every long round
    // drifted toward acquittal regardless of what the code did.
    //
    // Here: 250 samples of 12ms wall against a 10ms budget, each carrying 0.1ms of
    // perfectly ordinary off-CPU time. The deficit sums to 25ms, which is more than
    // the 2ms a single sample went over by, and **nothing here is a stall** — the
    // process was running for 11.9ms of every 12ms. Under the old comparison this
    // acquitted. It is the exact shape #261's prose gate hit at 250 samples, where
    // a 108ms deficit excused 21.76s of genuine parse.
    holds_p99_rounds(
        "a long round that is simply slow",
        Duration::from_millis(10),
        &flat(250, 12),
        String::new,
        || {
            let mut cpu = Samples::new(250);
            for _ in 0..250 {
                cpu.push(Duration::from_micros(11_900));
            }
            (flat(250, 12), Some(cpu))
        },
    );
}

#[test]
fn a_long_round_that_is_genuinely_stalled_is_still_excused() {
    // The other side of the same fixture, and the reason the fix is a unit
    // correction rather than a shape rule. A first attempt required the p50 to sit
    // inside budget before the host could be blamed, on the reasoning that a stall
    // moves the tail rather than the median. **That is false**, and this is the
    // counter-example: a runner that takes the CPU away for a whole round produces
    // a breach that is uniform, not tail-shaped, with a median fifty times the
    // budget. `a_wall_clock_breach_the_cpu_clock_acquits_is_reported_and_not_failed`
    // is the same claim at ten samples; this is it at two hundred and fifty, so the
    // count cannot quietly become the thing that decides.
    holds_p99_rounds(
        "a long round on a runner that took the CPU away",
        Duration::from_millis(10),
        &flat(250, 500),
        String::new,
        || {
            let mut cpu = Samples::new(250);
            for _ in 0..250 {
                cpu.push(Duration::from_millis(1));
            }
            (flat(250, 500), Some(cpu))
        },
    );
}

#[test]
fn the_excess_a_round_spends_over_budget_counts_only_the_samples_that_exceeded_it() {
    // `Samples::excess_over` is what makes the two sides of that comparison the
    // same kind of number, so its own arithmetic is asserted rather than inferred
    // from the gates that consume it.
    let budget = Duration::from_millis(10);

    assert_eq!(
        flat(100, 5).excess_over(budget),
        Duration::ZERO,
        "a round entirely inside budget spent nothing over it"
    );
    assert_eq!(
        flat(100, 10).excess_over(budget),
        Duration::ZERO,
        "a sample exactly at budget has not exceeded it"
    );
    assert_eq!(
        flat(100, 12).excess_over(budget),
        Duration::from_millis(200),
        "a hundred samples 2ms over is 200ms over"
    );

    // The single-spike case, which is what the totals form was protecting and
    // which this must not break: one slow sample contributes its own excess and
    // the ninety-nine fast ones contribute nothing.
    assert_eq!(
        spiked(100, 1).excess_over(budget),
        Duration::from_millis(490),
        "one 500ms sample against a 10ms budget is 490ms, and the rest are under"
    );
}
