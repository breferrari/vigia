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

mod support;

use std::time::{Duration, Instant};

use support::{Scratch, delta, materialise, settle};
use vigia_core::{FileChange, FrameStats, LineKind, Samples, Worktree};

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

/// Multiplier applied to the absolute budgets, and to nothing else.
///
/// Defaults to 1, so a developer machine is held to `SPEC.md` exactly. CI
/// raises it because hosted runners are shared and their variance is not a
/// property of this code. The structural gates ignore it entirely.
fn slack() -> f64 {
    std::env::var("VIGIA_BUDGET_SLACK")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .filter(|value: &f64| *value >= 1.0)
        .unwrap_or(1.0)
}

fn budget(base: Duration) -> Duration {
    base.mul_f64(slack())
}

/// Whether the absolute wall-clock gates should assert.
///
/// A debug build is several times slower than the one the budgets were set
/// against, so asserting there would fail for a reason that is not a
/// regression. Reported rather than silently skipped.
fn absolute_gates_apply() -> bool {
    if cfg!(debug_assertions) {
        eprintln!(
            "note: absolute budget gates skipped in a debug build; \
             run `cargo test --release --test budgets` to enforce them"
        );
        false
    } else {
        true
    }
}

/// Best of `n`, which measures what the code can do rather than what the
/// machine happened to be doing at the time.
fn best_of(n: usize, mut measure: impl FnMut() -> Duration) -> Duration {
    (0..n)
        .map(|_| measure())
        .min()
        .expect("at least one measurement")
}

fn time(mut work: impl FnMut()) -> Duration {
    let start = Instant::now();
    work();
    start.elapsed()
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

    if !absolute_gates_apply() {
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
        time(|| {
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
        frames.push(frame());
    }
    let p99 = frames.percentile(0.99).expect("samples");
    assert!(
        p99 <= budget(I9_FRAME),
        "I9: frame p99 was {p99:?}, over the {:?} budget (p50 {:?}, max {:?})",
        budget(I9_FRAME),
        frames.percentile(0.50).expect("samples"),
        frames.max().expect("samples")
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

    if !absolute_gates_apply() {
        return;
    }

    let mut edits = 0usize;
    let mut marker = String::new();
    let mut next_frame = |frame: &mut vigia_core::Frame| {
        marker = format!("fn edited_{edits}() {{ }}");
        scratch.edit_line(SHARED_PATH, 0, &marker);
        edits += 1;
        time(|| materialise(frame))
    };

    for _ in 0..WARMUP_FRAMES {
        next_frame(&mut frame);
    }

    let before = frame.stats();
    let mut frames = Samples::new(SAMPLED_FRAMES);
    for _ in 0..SAMPLED_FRAMES {
        frames.push(next_frame(&mut frame));
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
    let last = marker.clone();
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

    let p99 = frames.percentile(0.99).expect("samples");
    assert!(
        p99 <= budget(I9_FRAME),
        "I9: a real frame over {FILES} files was {p99:?} p99, over the {:?} \
         budget (p50 {:?}, max {:?}, {} recomputed, {} reused, {} read)",
        budget(I9_FRAME),
        frames.percentile(0.50).expect("samples"),
        frames.max().expect("samples"),
        cost.computed,
        cost.reused,
        cost.bytes
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
