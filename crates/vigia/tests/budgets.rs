//! I9, held against the shell rather than against the core.
//!
//! > Steady-state frame time holds 60fps under continuous edits. **< 16ms** p99.
//!
//! `crates/vigia-core/tests/budgets.rs` already gates this over the frame path,
//! and that gate is now structurally blind to part of the cost: the core does no
//! highlighting, so a frame measured there is a frame with the syntax parser
//! missing. Highlighting happens where the screen is, because it follows the
//! viewport, so the number that matters is measured here.
//!
//! It is the same shape as `reads.rs`: an invariant the engine can only make
//! *possible* gets a second gate over the caller (`SPEC.md` §7). The difference
//! is the tier. `reads.rs` is structural and takes no slack; this is an absolute
//! wall-clock gate, so it runs release-only and accepts `VIGIA_BUDGET_SLACK`.
//!
//! What it costs, measured while it was written: one screenful of Rust is about
//! 1.5ms of `syntect`, against a frame path that was 6.97ms p99 on the same
//! fixture before highlighting existed.
//!
//! **Every gate here paints, and until [#45](https://github.com/breferrari/vigia/issues/45)
//! none of them did.** A frame timed as `Frame::advance` plus `App::view` is the
//! frame the shell has minus the half that writes cells, and that half is where a
//! row's width is decided. A row carrying seven times more line than pane
//! therefore passed a 16ms gate for two phases, because no gate on either tier
//! could see it. `SPEC.md` §7 now says so as a rule; `tests/paint.rs` holds the
//! structural half.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::cell::RefCell;
use std::path::Path;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{
    Action, App, Body, Glyphs, PaintStats, Pointing, Row, Theme, View, WHEEL_ROWS, body_layout,
    render,
};
use vigia_core::{
    CHECKPOINT_STRIDE, Frame, HISTORY_PATHS, HISTORY_SAMPLE, Highlighter, History, LineKind,
    Samples,
};

use support::{
    PROSE_EXT, PROSE_SPANS, Scratch, WIDE_EXT, WIDE_UNPARSED_EXT, absolute_gates_apply, budget,
    delta, exclusively_timed, generated, highlight_delta, holds_p99, holds_p99_rounds,
    prose_generated, settle, settle_spans, time, time_cpu, timed_cpu,
};

/// I9: steady-state frame time.
const I9_FRAME: Duration = Duration::from_millis(16);

/// The fixture the core's own I9 gate uses, so the two numbers are comparable.
///
/// 100 x 500 rewritten line for line is the 100k-line diff the budgets are
/// written against. Every file is one hunk of just over a thousand lines, which
/// is also the worst case for highlighting: a screenful is drawn from the top of
/// a hunk whose content changes on every frame.
const FILES: usize = 100;
const LINES: usize = 500;

/// The file rewritten before every frame, which is the one the view is sitting
/// on.
const EDITED_PATH: &str = "src/mod_0.rs";

/// Timed frames between bulk rewrites in the gate at the bottom of this file.
///
/// The settle margin is a fixed two seconds of wall clock and the frame rate is
/// whatever the machine gives, so "sample 250 frames and they will all be inside
/// the margin" is a race rather than an invariant. Measured: one rewrite for the
/// whole sample ran **1.79s against the 2s margin**, which is 1.12x and is not
/// headroom, so a slower runner would settle part-way through and the tail would
/// stop being the event. Rewriting before *every* frame is the opposite mistake
/// and is recorded at the loop itself.
///
/// That 1.79s was taken before [`exclusively_timed`] existed, so it carries the
/// contention of two neighbouring gates; serialised, the same single-rewrite
/// window is nearer 670ms and about 3x. Quoted as measured rather than adjusted,
/// and the conclusion is unchanged either way: 3x is still a number that depends
/// on the runner, and a CI machine is allowed to be three times slower than this
/// one by `VIGIA_BUDGET_SLACK` alone.
///
/// Fifty puts a chunk at **308ms** measured, so the premise holds with better
/// than six times to spare and the gate stops depending on how fast the runner
/// is, which is the whole reason to prefer a frame count over a duration here.
const REWRITE_EVERY: usize = 50;

/// Frames discarded before sampling, and frames sampled.
///
/// The same numbers the core's gate uses, and for the same two reasons: I9 is a
/// claim about steady state so the cold path is out of scope by definition, and
/// a percentile needs enough samples to be one.
const WARMUP_FRAMES: usize = 50;
const SAMPLED_FRAMES: usize = 250;

/// Sample the history the way `vigia::run` does, `stat` included.
///
/// **The `stat` is part of what a tick costs and therefore part of what these
/// gates measure** ([#232](https://github.com/breferrari/vigia/issues/232)).
/// Weighing a write by the bytes it moved means the run loop reads each changed
/// path's size on the wake that records it, and a gate calling `record` instead
/// would time a frame path the product does not have. That is the failure the
/// comment beside every call site here already names, arriving through a new
/// door: what gets left out gets *cheaper*, so the omission would never fail.
fn sample(history: &mut History, root: &Path, path: &str) {
    sample_all(history, root, &[path.to_owned()]);
}

/// [`sample`] over a whole burst, which is what a bulk rewrite delivers.
///
/// **A hundred paths cost a hundred `stat`s, and the gate has to spend them or
/// it is measuring a tick the product never has.** The watcher coalesces one
/// wake per burst, so the run loop sizes every path in it, and the bulk-rewrite
/// gates below are the only place in this file where a burst is more than one
/// file. Sampling `EDITED_PATH` alone there would have left the widest tick this
/// tool has unmeasured while reporting a number that looked like it covered it.
fn sample_all(history: &mut History, root: &Path, paths: &[String]) {
    // **`vigia::weigh`, which is the one `run` calls.** These three lines used to
    // be copied here, and the copy is the drift surface: what a gate leaves out
    // gets cheaper, so it would go on passing while pricing a tick the product no
    // longer has, which is the failure the comment beside every call site in this
    // file already names.
    history.record_sized(vigia::sized(root, paths), Instant::now());
}

/// Rounds the burst gate times before taking a median.
///
/// Enough that one scheduler hiccup cannot be the answer, which nearest-rank
/// percentiles make a real hazard on a shared runner.
const SAMPLED_BURSTS: usize = 30;

/// The widest burst a wake can carry, as paths.
///
/// **[`HISTORY_PATHS`] rather than [`FILES`]**: `Burst` caps the set it reports
/// at the store's own cap, so this is the widest tick the product has, and a
/// gate built on the hundred-file fixture measured two fifths of the case it was
/// written for. The first `FILES` of these exist on disk and the rest do not,
/// which is the honest shape of a cap being hit rather than a defect in the
/// fixture: `symlink_metadata` fails fast on a missing path, so this is the
/// cheaper half of the range and the gate is measuring a floor on the real cost
/// rather than a ceiling. Stated here rather than discovered from the number.
fn bulk_burst() -> Vec<String> {
    (0..HISTORY_PATHS)
        .map(|f| format!("src/mod_{f}.rs"))
        .collect()
}

/// An ordinary terminal.
fn area() -> Rect {
    Rect::new(0, 0, 80, 24)
}

fn layout(app: &App, files: usize) -> Body {
    layout_of(app, area(), files)
}

/// The same, on a pane that is not the ordinary terminal.
///
/// Split out for the rail's budget gate
/// ([#252](https://github.com/breferrari/vigia/issues/252)), which is the first
/// one here measured on a pane [`area`] does not describe.
fn layout_of(app: &App, pane: Rect, files: usize) -> Body {
    body_layout(
        pane,
        &app.chrome("fixture", None, Pointing::default(), 0, ""),
        files,
        files,
    )
}

fn body(app: &App, files: usize) -> usize {
    layout(app, files).diff
}

/// One frame of the shell, timed whole: diff, collect, paint.
///
/// The paint is the half that was missing. `Shell::draw` does exactly this plus
/// a branch read (one `.git/HEAD`, drawn on every frame since #158) and a
/// terminal size query, so what is timed here is the shipped frame with the tty
/// removed — which is the same carve-out `soak.rs` already names.
///
/// **The two status readouts are inside it, in the order `vigia::run` puts
/// them**, and that is the third thing along the same axis rather than a detail.
/// `SPEC.md` §7's rule is that a stage left outside a gate is a stage nothing
/// can regress you on, and the readouts are the easiest possible instance:
/// `App::sample_memory` performs a syscall and `App::chrome` computes a
/// percentile, so a helper that skipped them would gate a screen the product
/// does not draw while reading as though it gated everything. Sampling before
/// the paint and recording after is what makes each one land inside the frame it
/// reports. `the_timed_frame_draws_the_readouts_it_is_timing` asserts it rather
/// than this comment claiming it.
fn shell_frame(
    frame: &mut Frame,
    app: &mut App,
    highlighter: &mut Highlighter,
    history: &History,
    buf: &mut Buffer,
    theme: &Theme,
    screen: Body,
) {
    let began = Instant::now();
    frame.advance().expect("advance");
    frame_body(frame, app, highlighter, history, buf, theme, screen);
    // Recorded from an inner clock rather than handed the caller's, because
    // every caller times this differently: some wrap it in `time`, some in
    // `timed`, and the scroll gates wrap a whole motion. What the ring needs is
    // one frame's cost, and this is the only place that knows where one starts.
    app.record_frame(began.elapsed());
}

/// Everything a frame does except walk status, which is the half an ageing wake
/// pays and a tick pays on top of.
///
/// **Split out of [`shell_frame`] rather than copied into the one gate that
/// wants it** ([#277](https://github.com/breferrari/vigia/issues/277)). That gate
/// compares a wake that skips `Frame::advance` against one that does not, and a
/// hand-rolled second copy of these four lines is exactly the drift `shell_frame`
/// exists to prevent: the two arms would stop being the same frame the moment
/// either grew a step.
fn frame_body(
    frame: &mut Frame,
    app: &mut App,
    highlighter: &mut Highlighter,
    history: &History,
    buf: &mut Buffer,
    theme: &Theme,
    screen: Body,
) {
    app.sample_memory();
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let view = app.view(frame, highlighter, history, screen).expect("view");
    // **The pane comes from the buffer being painted rather than from
    // [`area`]** ([#252](https://github.com/breferrari/vigia/issues/252)). The two
    // were the same rect arriving twice and had to agree; a caller timing a frame
    // on a *rail* pane would otherwise have had to remember to change both, and
    // the failure mode is the one this file's own doc warns about, since what gets
    // rendered against the wrong rect gets cheaper rather than louder.
    let pane = buf.area;
    render(buf, pane, &view, theme, Glyphs::default(), &chrome);
}

/// The screen has to have been full, or a frame that drew two rows is a cheap
/// frame for a reason that is not the code.
///
/// One helper for the three tick gates rather than three copies, because it is
/// the non-vacuity check every wall-clock assertion in this file rests on and a
/// `height` term drifting out of step with `body(&app, files)` in one copy is
/// invisible in the other two. Returns the view, which two of the callers go on
/// to use.
fn drew_a_full_screen(
    app: &mut App,
    frame: &mut Frame,
    highlighter: &mut Highlighter,
    history: &History,
    screen: Body,
    height: usize,
) -> View {
    let view = app.view(frame, highlighter, history, screen).expect("view");
    assert_eq!(
        view.rows.len(),
        height,
        "the body drew {} of {height} rows, so the frames above were not full \
         screens",
        view.rows.len()
    );
    view
}

/// The edits have to still be landing.
///
/// Checked against the frame's diff rather than against the screen, and the
/// difference is the fixture: these rewrite every line, so a file's hunk is five
/// hundred removals followed by five hundred additions and the newest line sits
/// far below any viewport. A screen assertion here would fail while the code was
/// perfect.
///
/// Shared for the reason the marker itself is not: each gate writes its own
/// format (`fn edited_{n}` here, `fn bulk_edited_{n}` in the bulk gate), so the
/// producer already varies and only the check is common. Two copies of the check
/// would be two places to notice that a producer's format had moved.
fn the_edits_still_land(frame: &mut Frame, path: &str, marker: &str) {
    let at = frame
        .files()
        .iter()
        .position(|change| change.path == path)
        .expect("the edited file is still a change");
    let (_, diff) = frame.diff(at).expect("diff");
    assert!(
        diff.hunks.iter().any(|hunk| hunk
            .lines
            .iter()
            .any(|line| line.kind == LineKind::Added && line.text == marker)),
        "the diff for {path} does not contain {marker:?}, so the edits stopped \
         reaching it"
    );
}

#[test]
fn a_real_frame_with_highlighting_holds_the_frame_budget() {
    frame_budget_at_depth("shell-i9", 0);
}

/// The pane the rail is measured on.
///
/// **Wide enough to draw one and tall enough for it to matter.** The point of the
/// gate is not that the pane is wide, it is that the pinned list is no longer
/// capped: beside the diff its rows cost the diff nothing, so a forty-row pane
/// draws every row it has where an eighty-column one draws the settled six. Every
/// one of them is a `Frame::diff`.
const RAIL_PANE: Rect = Rect {
    x: 0,
    y: 0,
    width: 200,
    height: 40,
};

/// I9 beside a rail, where the pinned list draws several times the rows it does
/// on the pane every other gate here measures
/// ([#252](https://github.com/breferrari/vigia/issues/252)).
///
/// **The one thing about this layout only a clock can answer.** That a visible
/// list row costs one `Frame::diff` and no more is structural and
/// `tests/reads.rs` owns it; that the region's height is its own bound is
/// structural too. What neither says is whether a region four times as deep still
/// fits inside sixteen milliseconds, and `SPEC.md` §7's rule is that a budget is a
/// test rather than an aspiration.
///
/// Non-vacuity is asserted rather than assumed: the gate reddens if the pane it
/// names stops drawing a rail, or draws one no deeper than the stacked cap, both
/// of which would leave it measuring the gate above under a different name.
#[test]
fn a_frame_beside_a_rail_holds_the_frame_budget() {
    // **The rail is asked for since §11.2 B14, and asked for the same way here as
    // in the timed loop below.** The first spelling of this built a
    // `Chrome { rail: true, .. }` by hand while `frame_budget_on` reached the same
    // state through `App::apply`, so the shape this gate asserts and the shape it
    // times were produced by two paths that can drift: if `ToggleRail` ever stopped
    // reaching `chrome.rail`, the assertion stayed green while `shell-i9-rail`
    // silently timed a stacked pane, which is exactly the substitution this gate's
    // own docblock says it exists to catch.
    let scratch = Scratch::large_diff("i9-rail-shape", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    let mut app = App::new();
    let stacked = layout(&app, FILES).list;
    app.apply(
        Action::ToggleRail,
        &mut frame,
        layout_of(&app, RAIL_PANE, FILES).diff,
    )
    .expect("ask for the rail");
    let rail = layout_of(&app, RAIL_PANE, FILES);
    assert!(
        rail.rail,
        "the {}x{} pane this gate is named for does not draw a rail",
        RAIL_PANE.width, RAIL_PANE.height
    );
    assert!(
        rail.list > stacked * 3,
        "the rail draws {} pinned rows against the stacked layout's {stacked}, \
         which is not the deeper region this gate exists to time",
        rail.list
    );
    frame_budget_on("shell-i9-rail", 0, RAIL_PANE, None, true, false);
}

#[test]
fn the_timed_frame_draws_the_readouts_it_is_timing() {
    // **A gate over the gates, and this repo has paid twice for not having
    // one.** `SPEC.md` §7 records both: `render` sat outside every budget on
    // both crates for two phases, so a row costing 7.2x its pane passed a 16ms
    // assertion; and a gate that settled before measuring left the one window
    // it was written about unmeasured. Both were invisible from inside the gate,
    // which is exactly the property that makes a comment a bad instrument here.
    //
    // The status readouts are the same shape one more time. `sample_memory` is a
    // syscall and `chrome` sorts a hundred and twenty-eight durations, and if
    // `shell_frame` ever stops calling them, every wall-clock assertion in this
    // file keeps passing while measuring a screen the product does not draw.
    // Nothing else here can catch that, because what is left out gets *cheaper*.
    //
    // Two frames rather than one: the first has no completed frame behind it, so
    // its chrome legitimately carries no frame time. The readout appearing on
    // the second is the shipped behaviour, and asserting it at the first would
    // be asserting the bug.
    let scratch = Scratch::large_diff("readouts-in-the-gate", 4, 20);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let screen = layout(&app, 4);
    let theme = Theme::default();
    let mut buf = Buffer::empty(area());

    for _ in 0..2 {
        shell_frame(
            &mut frame,
            &mut app,
            &mut highlighter,
            &history,
            &mut buf,
            &theme,
            screen,
        );
    }

    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    assert!(
        chrome.frame.is_some(),
        "the timed frame never recorded what it cost, so every wall-clock gate \
         in this file is measuring a screen without the frame readout on it"
    );
    // Every tier-1 target has a cheap read, so this asserts unconditionally
    // rather than behind a `cfg`. A platform outside those three would fail here
    // and should: it means the readout silently stopped being covered, which is
    // the thing this gate exists to notice. `SPEC.md` §5.1 names the three.
    assert!(
        chrome.memory.is_some(),
        "the timed frame read no memory, so the syscall the status bar performs \
         every frame is outside every budget in this file"
    );
}

/// Sizing a whole burst does not measurably change the frame it sits in.
///
/// **Two fixtures rather than one number against a budget, which is the finding
/// this gate exists to keep.** Weighing a write by its size costs one
/// `symlink_metadata` per changed path per wake, and a burst carries up to
/// `HISTORY_PATHS` of them. Timed on its own that is 2.60ms of thread CPU against
/// I9's 16ms, and twice in one session that number was used to cap the feature:
/// once off a wall figure inflated by a loaded machine, once off a `0ns` CPU
/// figure that was the clock's 15.625ms quantum rather than a measurement.
///
/// **Neither reading measured the frame.** `Frame::advance` walks status on the
/// same wake and stats every one of these paths to decide they changed at all, so
/// by the time the sizing runs the metadata is warm and the marginal syscall is
/// free. Interleaved over thirty rounds of a bulk rewrite, the frame measured
/// 18.43ms sizing nothing against 17.93ms sizing all of them: the *unsized* run
/// was the slower of the two.
///
/// So this compares two problem sizes whose per-unit content is identical and
/// whose counts differ, which is the only form of this comparison with no shared
/// term to cancel. It is deliberately loose: it is here to catch a `stat` that
/// has become a read or a walk, three orders of magnitude, not to track
/// microseconds on a shared runner.
#[test]
fn sizing_a_whole_burst_does_not_change_the_frame_it_sits_in() {
    if !absolute_gates_apply("cargo test --release -p vigia --test budgets") {
        return;
    }
    let _timed = exclusively_timed();

    let scratch = Scratch::large_diff("burst-frame", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let mut history = History::new();
    let screen = layout(&app, FILES);
    let theme = Theme::default();
    let mut buf = Buffer::empty(area());
    let paths = bulk_burst();

    // **Interleaved, because a sequential pair under varying load is not a
    // controlled experiment.** This repo nearly filed a phantom regression that
    // way: a 125ms tail landed on the arm the branch did not touch.
    let (mut sized, mut bare) = (Samples::new(SAMPLED_BURSTS), Samples::new(SAMPLED_BURSTS));
    for round in 1..=SAMPLED_BURSTS {
        for weighed in [true, false] {
            scratch.rewrite_all(FILES, LINES, round);
            let (wall, _) = time_cpu(|| {
                if weighed {
                    history.record_sized(vigia::sized(scratch.root(), &paths), Instant::now());
                } else {
                    history.record(paths.iter().map(String::as_str), Instant::now());
                }
                shell_frame(
                    &mut frame,
                    &mut app,
                    &mut highlighter,
                    &history,
                    &mut buf,
                    &theme,
                    screen,
                );
            });
            // Warm rounds only, for the reason every gate in this file warms:
            // the first frames fault in whatever the platform caches.
            if round > SAMPLED_BURSTS / 4 {
                if weighed {
                    sized.push(wall)
                } else {
                    bare.push(wall)
                }
            }
        }
    }

    let weighed = sized.percentile(0.5).expect("a sampled round");
    let plain = bare.percentile(0.5).expect("a sampled round");
    // Non-vacuity: both arms have to have done the work, or this compares two
    // numbers neither of which is a frame.
    // **Non-vacuity on what the run did, not on what the fixture is.** Asserting
    // `paths.len() == HISTORY_PATHS` was true by construction, since `bulk_burst`
    // *is* that range: it kills no mutation and reads as a check.
    let recorded = history.stats().recorded;
    assert!(
        recorded >= (HISTORY_PATHS * SAMPLED_BURSTS) as u64,
        "the store recorded {recorded} paths across {SAMPLED_BURSTS} rounds of          both arms, so at least one arm sized a burst the history never took"
    );
    assert!(
        plain > Duration::ZERO,
        "the unsized arm took no time, so this compared nothing"
    );
    // **The delta against a fraction of the frame, not a ratio against the whole
    // one.** A ratio hides the term it is supposed to expose: with `weigh` swapped
    // for a whole-file read this gate measured 28.98ms against 35.27ms and passed,
    // because both arms grew together and the quotient stayed under two. What this
    // prices is the difference, and the difference is what a `stat` becoming a
    // read would move.
    //
    // Saturating, because the sizing arm is routinely the *faster* of the two: the
    // status walk on the same wake has already warmed this metadata, so the
    // marginal syscall is free and run-to-run noise decides the order.
    let delta = weighed.saturating_sub(plain);
    // **An eighth was calibrated on one machine and the cost is host-dependent.**
    // On the reference machine sizing is free: 18.43ms unsized against 17.93ms
    // sized, the unsized arm slower. On `windows-latest` the same comparison is
    // 6.36ms against 9.41ms, so sizing costs **3.04ms** there. The frame is much
    // cheaper on that host, so the same syscalls are a far larger share of it, and
    // "unmeasurable in situ" turned out to be a fact about one filesystem rather
    // than about the change. CI is what found that, which is the whole reason a
    // gate calibrated locally gets run on three platforms before it is believed.
    //
    // Half the frame, which tolerates the honest 3ms with room for a slower host
    // and still catches what this gate is for: a `stat` that has become a read
    // would be reading five hundred lines from each of two hundred and fifty six
    // files, which is orders of magnitude rather than milliseconds.
    let allowed = budget(I9_FRAME / 2);
    // **No absolute claim here, and the attempt to add one is worth recording.**
    // Asserting this frame holds I9 looked like the product-level statement the
    // ratio never made, and it is measuring the wrong thing: this fixture is a
    // hundred-file bulk rewrite inside the settle margin, which costs 18.43ms
    // unsized on the reference machine, and I9 is the *steady state* budget.
    // `what_a_bulk_rewrite_of_undrawn_files_costs` reports that shape and
    // `the_frame_budget_holds_through_a_bulk_rewrite` gates it, both against the
    // in-margin case rather than against I9. A bound that fails on correct code
    // is worse than no bound, and this gate's job is the comparison.
    assert!(
        delta <= allowed,
        "sizing a {HISTORY_PATHS}-path burst added {delta:?} to the frame          ({plain:?} to {weighed:?}) against {allowed:?}, which is a `stat` that          has become a read or a walk rather than a syscall on metadata the status          walk has already warmed"
    );
}

#[test]
fn the_memory_read_costs_a_fraction_of_the_frame_it_sits_in() {
    // The one *variable* cost the readouts add, and the reason the whole design
    // turns on it: `SPEC.md` §5.1 ships this cell precisely because the read is
    // a syscall on all three tier-1 targets rather than the process spawn
    // `soak.rs` uses, which is **42.8ms median** on Windows against this 16ms
    // budget. That is the claim, so it is gated rather than quoted.
    //
    // One percent, which is loose on purpose. The point is to catch a read that
    // has quietly become a spawn or a whole-directory walk, a change of three
    // orders of magnitude, not to track microseconds on a shared runner. A
    // tighter bound here would fail on contention and teach everyone to ignore
    // it.
    if !absolute_gates_apply("cargo test --release -p vigia --test budgets") {
        return;
    }
    let _timed = exclusively_timed();

    const RUNS: u32 = 1000;
    let budget = I9_FRAME / 100;

    // Warmed, because the first read faults in whatever the platform needs.
    // `SPEC.md` §7's rule about steady state applies to a syscall as much as to
    // a frame.
    for _ in 0..100 {
        vigia::memory::resident();
    }

    let taken = time(|| {
        for _ in 0..RUNS {
            std::hint::black_box(vigia::memory::resident());
        }
    });
    let each = taken / RUNS;

    // Non-vacuity, and it is the assertion that matters most on a platform
    // nobody checked: a `resident()` that returned `None` immediately would post
    // a superb number here and draw nothing at all.
    assert!(
        vigia::memory::resident().is_some(),
        "this platform reads no memory, so the timing below measured an early \
         return rather than a syscall"
    );
    assert!(
        each < budget,
        "one memory read costs {each:?}, over the {budget:?} this gate allows \
         against I9's {I9_FRAME:?}. A read at that cost is a subprocess or a \
         walk rather than a syscall, and SPEC.md §5.1 ships the readout on the \
         strength of it being a syscall"
    );
    eprintln!("note: one memory read is {each:?} against a {I9_FRAME:?} frame");
}

#[test]
fn a_frame_holds_the_budget_however_deep_the_reader_has_scrolled() {
    // The case the gate above is structurally blind to, and it is not exotic:
    // `App::new()` starts at row zero, so measuring only there measures the
    // cheapest position of the shape being tested.
    //
    // Highlighting a hunk is forward-only, so drawing row N needs the N rows
    // above it parsed. That is paid once while the hunk is stable. Under
    // continuous edits it is not stable: the file being written is the file
    // being read, its hunk changes before every frame, and a reader who scrolled
    // in to follow along pays the whole walk on every tick with no input at all.
    // Measured before the rewind existed: 29ms p50 and 53ms p99 here, against a
    // 16ms budget, sustained.
    //
    // The depth is inside the *first* hunk rather than across files. Each file
    // is five hundred rewritten lines, so one hunk is a thousand display rows
    // and there is no file boundary to reset anything.
    frame_budget_at_depth("shell-i9-deep", 500);
}

/// Sample the frame budget with the viewport `depth` rows into the diff.
///
/// One function rather than two tests with a constant swapped, because the two
/// depths have to agree about every other term for the comparison to mean
/// anything.
fn frame_budget_at_depth(name: &str, depth: usize) {
    frame_budget_on(name, depth, area(), None, false, false);
}

/// The same, on a named pane.
///
/// **Parameterised for the rail** ([#252](https://github.com/breferrari/vigia/issues/252)):
/// beside one the pinned list is not capped by [`vigia::LIST_SETTLED`]'s share of
/// the pane, because its rows cost the diff nothing, so a tall pane draws every
/// changed file it has. Each visible list row costs one `Frame::diff`, which
/// `tests/reads.rs` bounds structurally; what only a clock can answer is whether
/// four times as many of them still fit inside I9.
fn frame_budget_on(
    name: &str,
    depth: usize,
    pane: Rect,
    sheet: Option<&str>,
    rail: bool,
    single: bool,
) {
    let scratch = Scratch::large_diff(name, FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    assert_eq!(frame.files().len(), FILES, "fixture is not {FILES} files");

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let mut history = History::new();

    // **Asked for before the layout is taken, because it changes the layout.**
    // The rail is a gesture since `SPEC.md` §11.2 B14
    // ([#295](https://github.com/breferrari/vigia/issues/295)), and unlike the
    // sheet's toggle below it moves rows: taking the height first would time a
    // frame planned for the stacked shape. `ToggleRail` reads no height of its
    // own (`Action::needs_height` is false for it), so the stacked figure is an
    // honest argument here.
    if rail {
        let stacked = layout_of(&app, pane, FILES).diff;
        app.apply(vigia::Action::ToggleRail, &mut frame, stacked)
            .expect("toggle the rail");
    }

    // **The pin, asked for the same way**, and it moves no rows: `SPEC.md` §11.2
    // B16 narrows what the walk may *reach* rather than how tall the body is, so
    // unlike the rail above it needs no re-layout and the height taken below is
    // the same one either way.
    if single {
        app.apply(vigia::Action::ToggleSingle, &mut frame, 0)
            .expect("pin the diff");
    }

    let screen = layout_of(&app, pane, FILES);
    let height = screen.diff;

    if sheet.is_some() {
        // Retained state, so one toggle covers every frame the loop below times.
        app.apply(vigia::Action::ToggleSheet, &mut frame, height)
            .expect("toggle the sheet");
    }

    if depth > 0 {
        // A manual scroll, which disengages follow exactly as a reader's would
        // (`SPEC.md` §11.1). The view then stays where it was put while the
        // edits keep landing, which is the whole point.
        app.apply(
            vigia::Action::Scroll(isize::try_from(depth).expect("a sane depth")),
            &mut frame,
            height,
        )
        .expect("scroll");
        let view = app
            .view(&mut frame, &mut highlighter, &history, screen)
            .expect("view");
        assert_eq!(
            view.top.row, depth,
            "the scroll landed at row {} rather than {depth}, so the fixture \
             does not have one hunk deep enough to measure",
            view.top.row
        );
        assert_eq!(view.top.file, 0, "the scroll crossed into another file");
    }

    if !absolute_gates_apply("cargo test --release -p vigia --test budgets") {
        return;
    }

    let _timed = exclusively_timed();

    // "Under continuous edits", taken literally and the same way the core's gate
    // takes it: one line is rewritten before every frame, so each frame
    // revalidates ninety-nine files, recomputes the one that moved, and
    // re-highlights the one hunk on screen. The edit stands in for the agent in
    // the other pane and is deliberately outside the timed region.
    let mut edits = 0usize;
    // **A cell rather than a `String`, because the sampler now outlives the
    // reader.** `holds_p99` re-measures on a breach, so this closure is still live
    // when `the_edits_still_land` below reads the last marker; a plain `String`
    // would be mutably borrowed by the closure at that point and the read would not
    // compile. Sharing it through a `RefCell` keeps both borrows short.
    let marker = RefCell::new(String::new());
    let theme = Theme::default();
    let mut buf = Buffer::empty(pane);
    let mut next_frame =
        |frame: &mut Frame, app: &mut App, highlighter: &mut Highlighter, history: &mut History| {
            *marker.borrow_mut() = format!("fn edited_{edits}() {{ let value = {edits}; }}");
            scratch.edit_line(EDITED_PATH, 0, &marker.borrow());
            edits += 1;
            time_cpu(|| {
                // Inside the timed region on purpose. `vigia::run` samples the
                // history on the same wake that advances the frame, so a gate that
                // recorded outside `time` would be timing a frame path the product
                // does not have. It is what I10 costs per tick, measured where I9
                // can see it.
                sample(history, scratch.root(), EDITED_PATH);
                shell_frame(frame, app, highlighter, history, &mut buf, &theme, screen);
            })
        };

    for _ in 0..WARMUP_FRAMES {
        next_frame(&mut frame, &mut app, &mut highlighter, &mut history);
    }

    let before = highlighter.stats();
    let mut frames = Samples::new(SAMPLED_FRAMES);
    for _ in 0..SAMPLED_FRAMES {
        frames.push(next_frame(&mut frame, &mut app, &mut highlighter, &mut history).0);
    }
    let cost = highlight_delta(before, highlighter.stats());

    // Non-vacuity, in the three directions this gate can be hollow.
    //
    // Highlighting has to have actually happened, or this measures the frame
    // path the core already gates and calls it a shell number.
    assert!(
        cost.lines > 0,
        "no lines were highlighted across {SAMPLED_FRAMES} frames, so this gate \
         is measuring the core's frame path and nothing else"
    );

    // The screen has to have been full, or a frame that drew two rows would be
    // a cheap frame for a reason that is not the code.
    drew_a_full_screen(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        screen,
        height,
    );

    // And the edits have to be still landing. Checked against the frame's diff
    // rather than against the screen, and the difference is the fixture: this
    // one rewrites every line, so a file's hunk is five hundred removals
    // followed by five hundred additions and the newest line sits far below any
    // viewport. A screen assertion here would fail while the code was perfect.
    the_edits_still_land(&mut frame, EDITED_PATH, &marker.borrow());

    // The highlighter has to be re-parsing every frame, which is what says the
    // edits reach *it* and not merely the diff. One hunk is on screen and its
    // content changes before every frame, so the steady state is exactly one
    // re-parse per frame and no reuse at all.
    assert_eq!(
        cost.parsed, SAMPLED_FRAMES as u64,
        "{} hunks were re-parsed across {SAMPLED_FRAMES} frames, so the visible \
         hunk is not changing under the highlighter and this is not the steady \
         state I9 describes",
        cost.parsed
    );

    // And the cost has to follow the screen rather than the hunk, at any depth.
    //
    // The bound is a screenful **plus one checkpoint stride**, not a screenful:
    // a hunk whose content changed rewinds to the deepest parse position the new
    // content still agrees with, and that position sits at worst a whole stride
    // above the first drawn row. Without the rewind this number was the reader's
    // scroll depth, which is the 53ms-per-frame shape it exists to avoid.
    let per_frame = cost.lines / SAMPLED_FRAMES as u64;
    let bound = (height + CHECKPOINT_STRIDE) as u64;
    assert!(
        per_frame <= bound,
        "{per_frame} lines were highlighted per frame for a {height}-row body at \
         depth {depth}, over the {bound} a rewind to the last checkpoint can \
         cost, so a frame is parsing more of the hunk than it draws"
    );

    holds_p99(
        &format!(
            "I9: a real frame with highlighting over {FILES} files on a {}x{} pane \
             drawing {} pinned rows",
            pane.width, pane.height, screen.list
        ),
        budget(I9_FRAME),
        &frames,
        || {
            format!(
                "({} hunks parsed, {} reused, {} lines, {} bytes)",
                cost.parsed, cost.reused, cost.lines, cost.bytes
            )
        },
        || next_frame(&mut frame, &mut app, &mut highlighter, &mut history),
    );

    // **And when a sheet was asked for, one has to have been on the frames that
    // were timed.** Read out of the buffer the timed loop actually wrote to,
    // rather than off a second `App` built beside it: the non-vacuity check in
    // `a_frame_under_the_sheet_holds_the_frame_budget` names the rung on its own
    // `App`, so deleting the toggle above left that gate green while it timed
    // sheet-free frames. A gate that cannot fail is worse than no gate. `sheet`
    // carries the word that identifies the rung, so two gates share this scaffold
    // and neither can quietly time the other's shape.
    if let Some(rung) = sheet {
        // **Inside the sheet's own rect, not over the pane.** `rung` is an
        // ordinary word (`keyboard`, `moving`), and the fixture's own diff is a
        // hundred files of generated source: a pane-wide search can be satisfied
        // by content the sheet is covering, which would let this pass while
        // timing the wrong rung, or no rung at all. `read_sheet` in
        // `tests/sheet.rs` records the same lesson from the other direction,
        // where the hint bar's `q quit` scored on every frame.
        let laid = vigia::regions(
            pane,
            &app.chrome("fixture", None, Pointing::default(), 0, ""),
            &app.view(&mut frame, &mut highlighter, &history, screen)
                .expect("view"),
        );
        let at = laid
            .sheet
            .map(|s| Rect::new(s.left, s.top, s.width, s.height))
            .expect("this gate asked for a sheet and the pane published none");
        let drawn = (at.top()..at.bottom())
            .map(|y| {
                (at.left()..at.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            drawn.contains("gestures"),
            "this gate asked for the sheet and timed {SAMPLED_FRAMES} frames \
             without one on them, so it measured the gate above under another name"
        );
        assert!(
            drawn.contains(rung),
            "the timed frames carried a sheet but not the {rung:?} rung this gate \
             is named for"
        );
    }
}

#[test]
fn ticking_over_an_undrawn_worktree_holds_the_frame_budget() {
    // **The gate whose absence was the finding**
    // ([#101](https://github.com/breferrari/vigia/issues/101)). Every other
    // wall-clock gate in this file opens with `settle`, and `settle` calls
    // `materialise`, which diffs *every* file. With a `FileDiff` cached for all
    // hundred of them, `Frame::height` rebuilds every span from memory and reads
    // nothing, so the gate's own setup deletes the cost it would otherwise
    // measure. That is `SPEC.md` §7's recurring shape along a fourth axis:
    // cheapest position, cheapest state, warmed-past first frame, and now
    // **cheapest cache population**.
    //
    // The state here is the one a reader is actually in one second after launch:
    // a hundred changed files, of which the viewport has drawn six, ticking as
    // the agent in the other pane writes. Measured before the fix: **16.98ms p50,
    // 18.36ms p99**, with ninety-four files and 3.7 MiB re-read on every tick.
    //
    // `reads.rs::a_tick_re_measures_only_what_changed` is the structural half and
    // is the one that catches this on a machine that is not this one.
    let scratch = Scratch::large_diff("shell-i9-undrawn", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();

    // Waits out the margin exactly as `settle` does, and diffs nothing.
    let primed = settle_spans(&mut frame);
    assert_eq!(frame.files().len(), FILES, "fixture is not {FILES} files");
    assert_eq!(
        primed, FILES as u64,
        "priming measured {primed} of {FILES} files, so this fixture is already \
         materialised and the walk it is meant to time has been deleted"
    );

    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::eager();
    let mut history = History::new();
    let height = body(&app, FILES);
    let screen = layout(&app, FILES);

    if !absolute_gates_apply("cargo test --release -p vigia --test budgets") {
        return;
    }

    let _timed = exclusively_timed();

    let mut edits = 0usize;
    // **A cell rather than a `String`, because the sampler now outlives the
    // reader.** `holds_p99` re-measures on a breach, so this closure is still live
    // when `the_edits_still_land` below reads the last marker; a plain `String`
    // would be mutably borrowed by the closure at that point and the read would not
    // compile. Sharing it through a `RefCell` keeps both borrows short.
    let marker = RefCell::new(String::new());
    let theme = Theme::default();
    let mut buf = Buffer::empty(area());
    let mut next_frame =
        |frame: &mut Frame, app: &mut App, highlighter: &mut Highlighter, history: &mut History| {
            *marker.borrow_mut() = format!("fn edited_{edits}() {{ let value = {edits}; }}");
            scratch.edit_line(EDITED_PATH, 0, &marker.borrow());
            edits += 1;
            time_cpu(|| {
                sample(history, scratch.root(), EDITED_PATH);
                shell_frame(frame, app, highlighter, history, &mut buf, &theme, screen);
            })
        };

    for _ in 0..WARMUP_FRAMES {
        next_frame(&mut frame, &mut app, &mut highlighter, &mut history);
    }

    let before = frame.stats();
    let mut frames = Samples::new(SAMPLED_FRAMES);
    for _ in 0..SAMPLED_FRAMES {
        frames.push(next_frame(&mut frame, &mut app, &mut highlighter, &mut history).0);
    }
    let cost = delta(before, frame.stats());

    // **The non-vacuity that matters here, and it is not the usual one.** Warming
    // fifty frames is right for a p99 and is also the thing that could quietly
    // materialise the worktree behind this gate's back: if anything in the frame
    // path started diffing every file, the walk would go free and this gate would
    // pass for the same reason `settle` made the others pass. So the *undrawn*
    // half is asserted directly.
    //
    // **Bounded by the screen rather than by `< FILES`.** One fewer than a
    // hundred is not "undrawn", it is "ninety-nine drawn", and the bound this
    // gate needs is the one its own comment claims: a frame can touch at most one
    // file per row it draws, list included, so that sum is the honest ceiling and
    // it is derived from the layout rather than written down.
    let touchable = screen.list + screen.diff;
    assert!(
        frame.tracked() <= touchable,
        "the frame holds {} diffs for a screen that can reach {touchable} files \
         at most, so the worktree has been materialised behind this gate and the \
         height walk it exists to time costs nothing",
        frame.tracked()
    );

    // The screen has to have been full, or a frame that drew two rows is cheap
    // for a reason that is not the code.
    drew_a_full_screen(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        screen,
        height,
    );

    // And the edits have to still be landing.
    the_edits_still_land(&mut frame, EDITED_PATH, &marker.borrow());

    holds_p99(
        &format!(
            "I9: a tick over {FILES} changed files of which the viewport has drawn \
             a screenful"
        ),
        budget(I9_FRAME),
        &frames,
        || {
            format!(
                "({} files measured and {} bytes read across {SAMPLED_FRAMES} ticks)",
                cost.measured, cost.bytes
            )
        },
        || next_frame(&mut frame, &mut app, &mut highlighter, &mut history),
    );
}

#[test]
fn what_a_bulk_rewrite_of_undrawn_files_costs() {
    // **Reports, and deliberately does not assert a wall clock.** Everything
    // structural here is asserted and exact; the clock is printed and left to
    // `SPEC.md` §10, and that is the ruling rather than a gap.
    //
    // Three instruments were tried and none separated the fixture from the
    // subject. Rewriting before every timed frame put the write-back inside the
    // timer (29.26ms p99 against 13.13ms p50). Discarding one frame after each
    // rewrite was not enough, because NTFS write-back of 1.7 MiB spans several
    // (16.56, 46.19 and 227.33ms p99 across three of eight runs). Discarding
    // twelve and partitioning on what each frame measured still gives **two
    // passes in eight on a quiet machine**: p50 sits at 13.08-14.33ms across
    // every run and the p99 ranges 15.49ms to 44.70ms. A stable p50 with a tail
    // that moves 3x is the signature `SPEC.md` §7 names, and no threshold
    // separates it from a real regression.
    //
    // So this refuses to assert the clock, exactly as the soak's drift gate
    // refuses below its own window: a gate that cannot say "no regression"
    // without also saying it on a bad disk has not been tested. What it *can*
    // say, and does, is that the corner is entered and how much it costs in
    // files and syscalls. `reads.rs::a_tick_inside_the_settle_margin_stats_each_file_once`
    // holds the same corner as a count, which is the tier that works here.
    //
    // **The cell where two of this repo's own rules intersect, and neither gate
    // covered it.** `SPEC.md` §7 says a gate that settles first has measured the
    // cheapest *state*, and [#101](https://github.com/breferrari/vigia/issues/101)
    // added that a gate whose setup materialises has measured the cheapest *cache
    // population*. `the_frame_budget_holds_through_a_bulk_rewrite` is inside the
    // margin and fully materialised, so its walk is free;
    // `ticking_over_an_undrawn_worktree_holds_the_frame_budget` is undrawn but
    // edits one file at a time. Nothing was undrawn **and** bulk-rewritten.
    //
    // It is the expensive corner by construction: every carried span is unsettled
    // at once, so none can be proved and the walk re-measures the whole changed
    // set for the length of `SETTLE_MARGIN`. That is the pre-#101 cost, paid for
    // two seconds rather than forever, and it is why the fingerprint in
    // `fill_span` is lazy: an unsettled observation is refused without a `stat`,
    // so this corner costs the read it always cost and not a syscall on top of it.
    //
    // The workload is not exotic. An agent running a formatter over a tree is
    // exactly this, and it is the workload `SPEC.md` §2 describes.
    let scratch = Scratch::large_diff("shell-i9-undrawn-bulk", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let primed = settle_spans(&mut frame);
    assert_eq!(frame.files().len(), FILES, "fixture is not {FILES} files");
    assert_eq!(
        primed, FILES as u64,
        "priming measured {primed} of {FILES} files, so this fixture is already \
         materialised and the walk it is meant to time has been deleted"
    );

    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::eager();
    let mut history = History::new();
    let height = body(&app, FILES);
    let screen = layout(&app, FILES);

    if !absolute_gates_apply("cargo test --release -p vigia --test budgets") {
        return;
    }

    let _timed = exclusively_timed();

    let theme = Theme::default();
    let mut buf = Buffer::empty(area());

    // **Frames after a rewrite are untimed, and a rewrite never lands inside a
    // timed one.** Both halves are the instrument rather than the subject, and
    // the first version of this gate had neither: it read **16.56ms, 46.19ms and
    // 227.33ms p99 across three of eight runs** against a p50 that never left
    // 13.2ms, which `SPEC.md` §7 names as the signature of the fixture rather
    // than of the code. NTFS write-back of a 1.7 MiB rewrite arrives over several
    // frames, so discarding one is not enough.
    //
    // **And the sampled frames are partitioned rather than assumed.** "Rewrite,
    // then sample N frames, and they will all still be inside the two-second
    // margin" is a property of the machine and not of the code: CI allows a
    // runner three times slower (`VIGIA_BUDGET_SLACK: "3"`), where the tail of a
    // chunk settles and stops being the event. So every frame is classified by
    // what it actually did. A frame that re-measured the whole changed set is
    // inside the margin and is this gate's subject; one that measured nothing has
    // settled and belongs to the gate above. Only the first kind is asserted on,
    // which is the same partition the scroll gates use for cold and warm frames.
    // **The chunk is short because the margin is a wall clock and the frame rate
    // is not.** A cycle has to finish well inside `SETTLE_MARGIN`, or its tail
    // settles and stops being the event, which is the same race `REWRITE_EVERY`
    // records one gate over. Ninety frames was sized on this machine at ~13ms and
    // failed on a macOS runner at 22ms: 149 of 270 sampled frames had settled by
    // the time they ran. Thirty frames plus ten absorbers is ~880ms there against
    // the two seconds, so the premise holds with better than twice to spare, and
    // nine cycles keep the sample count where a p99 means something.
    const CYCLES: usize = 9;
    const ABSORB: usize = 10;
    const PER_CYCLE: usize = 30;

    let mut in_margin = Samples::new(CYCLES * PER_CYCLE);
    let mut settled_frames = 0usize;
    let mut measured_in_margin = 0u64;
    let before = frame.stats();

    for round in 1..=CYCLES {
        scratch.rewrite_all(FILES, LINES, round);
        for _ in 0..ABSORB {
            sample(&mut history, scratch.root(), EDITED_PATH);
            shell_frame(
                &mut frame,
                &mut app,
                &mut highlighter,
                &history,
                &mut buf,
                &theme,
                screen,
            );
        }
        for _ in 0..PER_CYCLE {
            let was = frame.stats().measured;
            let cost = time(|| {
                sample(&mut history, scratch.root(), EDITED_PATH);
                shell_frame(
                    &mut frame,
                    &mut app,
                    &mut highlighter,
                    &history,
                    &mut buf,
                    &theme,
                    screen,
                );
            });
            // **Split on zero, not on `FILES`.** A frame inside the margin
            // re-measures every changed file *except* the handful the viewport
            // drew, because those have a diff in hand and take source (1) for
            // free. So the in-margin count is a screenful short of a hundred and
            // never equal to it, and a settled frame measures exactly nothing.
            // Zero is the only value that separates the two without hard-coding
            // how many files a screen happens to reach.
            match frame.stats().measured - was {
                0 => settled_frames += 1,
                n => {
                    measured_in_margin += n;
                    in_margin.push(cost);
                }
            }
        }
    }
    let cost = delta(before, frame.stats());
    // Every frame the loop drove, absorbing ones included, because `cost` is a
    // delta across all of them. Dividing by the timed subset alone overstated the
    // per-frame figures by the absorbers' share.
    let drove = CYCLES * (ABSORB + PER_CYCLE);

    // Undrawn, for the reason the gate above gives at more length.
    let touchable = screen.list + screen.diff;
    assert!(
        frame.tracked() <= touchable,
        "the frame holds {} diffs for a screen that can reach {touchable} \
         files at most, so this run is not the undrawn case",
        frame.tracked()
    );

    // **The premise, and it is a count rather than a clock.** A run whose spans
    // all stayed provable measured nothing this gate is about. Two thirds is
    // generous on purpose: it has to survive a runner slow enough that the tail
    // of a chunk settles, while still refusing a run that never entered the
    // corner at all.
    let wanted = CYCLES * PER_CYCLE * 2 / 3;
    let timed = in_margin.len() + settled_frames;
    assert!(
        in_margin.len() >= wanted,
        "only {} of {timed} timed frames re-measured the changed set, under the \
         {wanted} this gate needs, and {settled_frames} had settled. The margin \
         is settling faster than a chunk runs, so shorten the chunk rather than \
         widening the budget",
        in_margin.len()
    );

    // **And each of those frames re-measured nearly the whole changed set**, not
    // a handful of it. Without this, a run where every chunk went settled after
    // three frames would satisfy the count above and measure almost nothing,
    // which is the runner-speed dependence the partition exists to remove.
    let per_frame = measured_in_margin / in_margin.len() as u64;
    let floor = (FILES - touchable) as u64;
    assert!(
        per_frame >= floor,
        "an in-margin frame re-measured {per_frame} files on average, under \
         the {floor} a screen leaves undrawn, so these frames were only \
         part-way into the margin"
    );

    // And the rewrites have to have reached the diff, which no count of measures
    // can see. The line is taken from the generator rather than written out, so
    // it cannot drift from what `rewrite_all` actually wrote.
    let written = generated(LINES, &format!("bulk{CYCLES}"));
    let landed = written
        .lines()
        .nth(LINES / 2)
        .expect("the generator produced that line");
    the_edits_still_land(&mut frame, EDITED_PATH, landed);

    drew_a_full_screen(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        screen,
        height,
    );

    let p99 = in_margin.percentile(0.99).expect("samples");

    // The whole distribution, because a single percentile cannot distinguish a
    // regression from a mis-specified measurement and this one demonstrably
    // cannot. `SPEC.md` §10 carries the numbers this prints.
    eprintln!(
        "note: a bulk rewrite of {FILES} undrawn files, inside the margin: \
         p50 {:?} p99 {p99:?} max {:?} over {} in-margin frames of {timed} \
         timed ({settled_frames} settled), {} measured and {} stats per \
         frame across all {drove} driven",
        in_margin.percentile(0.50).expect("samples"),
        in_margin.max().expect("samples"),
        in_margin.len(),
        cost.measured / drove as u64,
        cost.probes / drove as u64,
    );
}

#[test]
fn the_frame_budget_holds_through_a_bulk_rewrite() {
    // The third position in this gate's input space, after "at the top" and "deep
    // in a hunk". Those two vary *where* the window is; this one varies *when*,
    // and it is the axis `SPEC.md` §7 gained from this test existing: a gate that
    // settles before it measures has measured the cheapest state.
    //
    // The event is a formatter, a branch switch or a multi-file agent edit. Every
    // file changes at once, so for the whole settle margin no file can be proved
    // unchanged and every one the shell asks for is recomputed rather than
    // reused. §10 claimed that breaks I9 for about two seconds. It does over the
    // core frame path, whose fixture materialises all hundred files: 98 of 182
    // frames over budget, 22.34ms p99. The shell recomputes only what it draws,
    // which is why this gate can exist at all, and `reads.rs` holds that half
    // structurally. This is the wall clock agreeing.
    let scratch = Scratch::large_diff("shell-i9-bulk", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    assert_eq!(frame.files().len(), FILES, "fixture is not {FILES} files");

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let mut history = History::new();
    let height = body(&app, FILES);
    let screen = layout(&app, FILES);

    if !absolute_gates_apply("cargo test --release -p vigia --test budgets") {
        return;
    }

    let _timed = exclusively_timed();

    let theme = Theme::default();
    let mut buf = Buffer::empty(area());
    let mut draw =
        |frame: &mut Frame, app: &mut App, highlighter: &mut Highlighter, history: &mut History| {
            time_cpu(|| {
                sample(history, scratch.root(), EDITED_PATH);
                shell_frame(frame, app, highlighter, history, &mut buf, &theme, screen);
            })
        };

    // Warm up *before* the first rewrite, not after. Warming afterwards would
    // spend the margin this gate exists to measure and leave the samples in the
    // settled state every other gate here already covers.
    for _ in 0..WARMUP_FRAMES {
        draw(&mut frame, &mut app, &mut highlighter, &mut history);
    }

    // The event, re-established every `REWRITE_EVERY` frames, and the frame that
    // absorbs its write-back deliberately not timed.
    //
    // Both halves were measured rather than chosen. One rewrite for the whole
    // sample is not enough: the window ran 1.79s against a 2s margin, 1.12x
    // headroom, so a runner any slower would settle part-way and the tail would
    // stop being the event. Rewriting before every frame is worse: `rewrite_all`
    // writes about 1.5 MiB, and while the call sits outside `time` its write-back
    // does not, so thirteen of them took a 2.67ms p50 to a 27ms p99 and this gate
    // measured the fixture. It also starved the two gates above, which share this
    // binary and are timed: all three failed together and the other two passed
    // under `--test-threads=1`.
    //
    // So the rewrite is periodic, and the one frame that pays for the harness's
    // own write-back is spent untimed. `SPEC.md` §7 already puts the cold path
    // outside I9 by definition, and this is that: the cost of a test fixture
    // hitting the disk is not a cost of the shell.
    let before = frame.stats();
    let highlighted = highlighter.stats();
    let mut frames = Samples::new(SAMPLED_FRAMES);
    for at in 0..SAMPLED_FRAMES {
        if at % REWRITE_EVERY == 0 {
            scratch.rewrite_all(FILES, LINES, at / REWRITE_EVERY + 1);
            draw(&mut frame, &mut app, &mut highlighter, &mut history);
        }
        // The drawn file, rewritten before each frame. One file rather than a
        // hundred, the idiom the two gates above already use, and the term that
        // decides frame cost, since only drawn files are fingerprinted at all.
        scratch.edit_line(
            EDITED_PATH,
            0,
            &format!("fn bulk_edited_{at}() {{ let value = {at}; }}"),
        );
        frames.push(draw(&mut frame, &mut app, &mut highlighter, &mut history).0);
    }
    let cost = delta(before, frame.stats());
    let parsed = highlight_delta(highlighted, highlighter.stats());

    // Non-vacuity, first in the direction this whole file exists for. Highlighting
    // has to have actually happened, or this measures the frame path the core
    // already gates and reports it as a shell number. One re-parse per frame is
    // the floor: the drawn file's hunk changes before every frame, so the steady
    // state is exactly that and no reuse at all.
    assert!(
        parsed.lines > 0 && parsed.parsed >= SAMPLED_FRAMES as u64,
        "{} hunks were re-parsed over {} lines across {SAMPLED_FRAMES} frames, so \
         the visible hunk is not changing under the highlighter and this gate is \
         timing the core's frame path with the syntax parser missing",
        parsed.parsed,
        parsed.lines
    );

    // Non-vacuity. A frame that reused rather than recomputed would be a cheap
    // frame for a reason that is not the code, and a percentile diluted with
    // them would pass while saying nothing. One recompute per frame is the floor,
    // and the per-frame edit above is what makes that hold at any frame rate
    // rather than only on a machine fast enough to finish inside the margin.
    assert!(
        cost.computed >= SAMPLED_FRAMES as u64,
        "{} diffs were recomputed across {SAMPLED_FRAMES} frames, so frames were \
         reusing and this gate timed settled frames",
        cost.computed
    );

    // And the premise, checked rather than assumed: a file the viewport never
    // drew is still inside its margin now, so it was for the whole window, since
    // settledness only ever increases with time. Two diffs rather than one,
    // because the first recomputes on any stale fingerprint and only the second
    // can tell "still unsettled" from "settled and reusable". Without this the
    // gate would quietly weaken on a runner slow enough to outrun the margin: the
    // other ninety-nine files would settle, and a shell that fetched ahead would
    // find them reusable and cheap.
    let undrawn = FILES - 1;
    let probed = frame.stats();
    frame.diff(undrawn).expect("diff");
    frame.diff(undrawn).expect("diff");
    let probe = delta(probed, frame.stats());
    assert_eq!(
        probe.reused, 0,
        "a file the viewport never drew was reusable after {SAMPLED_FRAMES} \
         frames, so the bulk rewrite settled part-way through and the tail of \
         this window was not the event"
    );

    // And the screen has to have been full, for the reason the gate above gives.
    let view = drew_a_full_screen(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        screen,
        height,
    );

    // Which also settles what the probe above assumed. It is named `undrawn` and
    // nothing had checked that it was: a fixture whose files were short enough
    // for the viewport to reach index {undrawn} would have fingerprinted it every
    // frame, and the probe would be asking a question about the drawn path while
    // reading as though it asked about the untouched one.
    assert!(
        view.top.file + view.read <= undrawn,
        "the viewport drew files {}..{} of {FILES}, which reaches the file the \
         settle probe treats as never drawn",
        view.top.file,
        view.top.file + view.read
    );

    // **The re-measure continues the sequence rather than restarting it**, which
    // matters more here than on the steady-state gates: what this one measures is a
    // frame *inside the settle margin* after a bulk rewrite, so a second round that
    // only edited one line would be measuring a cheaper condition and could mask a
    // real breach. Carrying `at` forward keeps the periodic rewrite on the same
    // cadence, so round two is the same experiment as round one.
    let mut at = SAMPLED_FRAMES;
    holds_p99(
        &format!(
            "I9: a frame inside the settle margin after every one of {FILES} files \
             was rewritten at once"
        ),
        budget(I9_FRAME),
        &frames,
        || {
            format!(
                "({} diffs recomputed, {} reused, {} bytes)",
                cost.computed, cost.reused, cost.bytes
            )
        },
        || {
            if at % REWRITE_EVERY == 0 {
                scratch.rewrite_all(FILES, LINES, at / REWRITE_EVERY + 1);
                draw(&mut frame, &mut app, &mut highlighter, &mut history);
            }
            scratch.edit_line(
                EDITED_PATH,
                0,
                &format!("fn bulk_edited_{at}() {{ let value = {at}; }}"),
            );
            at += 1;
            draw(&mut frame, &mut app, &mut highlighter, &mut history)
        },
    );
}

/// The wide fixture's shape, and why these two numbers.
///
/// Twenty files of sixty lines, rewritten line for line, so one file is a single
/// hunk of **120 display rows**. Both numbers are chosen against the scroll
/// rather than against the diff:
///
/// * a notch is [`WHEEL_ROWS`] rows, so a file boundary arrives every 40 frames
///   and about **2.4%** of the samples enter a hunk nothing has parsed. At 250
///   samples a nearest-rank p99 is the third-worst frame, so the partition below
///   separates frames the percentile can actually reach rather than a tail
///   nothing occupies;
/// * twenty files is 2440 rows, comfortably more than the 900 a warmup and a
///   sample walk together, so neither direction runs into an end and starts
///   measuring a viewport that cannot move.
const WIDE_FILES: usize = 20;
const WIDE_LINES: usize = 60;

/// The wide fixture at the scale the hundred-file gates use.
///
/// [#101](https://github.com/breferrari/vigia/issues/101)'s first exit criterion
/// is a gate over **both** dimensions at once, and the reason it is a criterion
/// is that nothing crossed them: the scrolling gates run at [`WIDE_FILES`], the
/// hundred-file gates edit in place and never scroll, and every one of them
/// begins by discarding [`WARMUP_FRAMES`].
const WIDE_MANY_FILES: usize = 100;

/// Display rows one wide file contributes: every line removed and every line
/// added.
const WIDE_HUNK_ROWS: usize = WIDE_LINES * 2;

/// Where the upward scroll starts, in rows from the top of the diff.
///
/// Far enough down that 300 frames of three rows never reach the top, since a
/// viewport pinned at row zero stops crossing boundaries and the gate would
/// quietly become a measurement of one file.
const UP_FROM: usize = 1_000;

/// Files that have to sit above the viewport before an upward scroll is one.
const UP_FILES: usize = 4;

/// How the worktree is brought to a settled state before a scroll is timed.
///
/// **The distinction is [#101](https://github.com/breferrari/vigia/issues/101)'s
/// whole finding**, so it is a named type rather than a boolean. Both wait out
/// the engine's settle margin; only one of them also diffs every file, and that
/// one deletes the height walk from every measurement downstream of it.
#[derive(Clone, Copy)]
enum Prime {
    /// `settle`: the margin waited out **and every file diffed**. The steady
    /// state the wide gates are about, and the reason they could never see the
    /// walk.
    Materialised,
    /// `settle_spans`: the margin waited out, nothing diffed. What a reader has
    /// a second after launch.
    Launched,
}

/// One scroll run's setup.
///
/// A struct rather than five positional arguments, because the gates below now
/// differ along three independent axes and `scroll("x", Motion::Down, WIDE_EXT,
/// 100, 0)` says nothing about which number is which.
#[derive(Clone, Copy)]
struct Scroll {
    motion: Motion,
    ext: &'static str,
    files: usize,
    /// Frames discarded before sampling. **Zero** for the gate that exists to
    /// contain the first frames rather than to begin after them.
    warmup: usize,
    prime: Prime,
}

impl Scroll {
    /// The shape every gate had before #101: twenty files, materialised, warmed.
    fn wide(motion: Motion, ext: &'static str) -> Self {
        Self {
            motion,
            ext,
            files: WIDE_FILES,
            warmup: WARMUP_FRAMES,
            prime: Prime::Materialised,
        }
    }
}

/// What one scroll of a wide fixture cost, per stage and per partition.
struct Scrolled {
    /// Frames that reused every hunk they drew: the steady state I9 is about.
    warm: Samples,
    /// The same frames, in thread CPU time rather than wall clock.
    ///
    /// What #178 attributes a wall-clock overshoot with: contention inflates the
    /// ring above and cannot inflate this one.
    warm_cpu: Samples,
    /// Frames that entered a hunk nothing had parsed: `SPEC.md` §7's cold path.
    cold: Samples,
    collect: Samples,
    paint: Samples,
    /// The worst single cold frame's parse, in lines.
    cold_lines: u64,
    /// Lines the whole run highlighted.
    lines: u64,
    /// Hunks the run swept out of the cache, which is the third suspect's own
    /// number: an eviction is only a cost when the reader comes back to it.
    evicted: u64,
    boundaries: usize,
    widest: usize,
    body_rows: usize,
    /// Rows the body had, carried from the run rather than re-derived.
    ///
    /// `hold_the_scroll_budget` writes two assertions in terms of it, and
    /// rebuilding it there from a fresh `App` would be a second source of truth
    /// for a number this run already has.
    height: usize,
    painted: PaintStats,
    /// Frames the run drove, warmup included.
    ///
    /// The number rather than the whole `Scroll` it came from, for the reason
    /// `height` beside it is carried: one assertion is written in terms of it,
    /// and keeping five fields to interpolate one of them reads as though the
    /// others were load-bearing too.
    frames: usize,
}

impl Scrolled {
    fn report(&self, what: &str) {
        eprintln!(
            "{what}: {} warm frames p50 {:?} p99 {:?} max {:?} | \
             {} cold frames p50 {:?} p99 {:?} max {:?} | \
             collect p99 {:?} paint p99 {:?} | \
             {} boundaries, {} lines highlighted, {} hunks evicted, \
             worst cold parse {} lines, \
             {} rows painted from {} characters, widest line {}",
            self.warm.len(),
            self.warm.percentile(0.50).unwrap_or_default(),
            self.warm.percentile(0.99).unwrap_or_default(),
            self.warm.max().unwrap_or_default(),
            self.cold.len(),
            self.cold.percentile(0.50).unwrap_or_default(),
            self.cold.percentile(0.99).unwrap_or_default(),
            self.cold.max().unwrap_or_default(),
            self.collect.percentile(0.99).unwrap_or_default(),
            self.paint.percentile(0.99).unwrap_or_default(),
            self.boundaries,
            self.lines,
            self.evicted,
            self.cold_lines,
            self.painted.rows,
            self.painted.examined,
            self.widest,
        );
    }
}

/// Frames in one leg of [`Motion::Back`], chosen to cross two files each way.
const LEG: usize = 80;

/// How the reader is moving.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    Down,
    /// From [`UP_FROM`] rows in, upwards, into hunks nothing has parsed.
    Up,
    /// Down a couple of files, then back up over the same ground, repeatedly.
    ///
    /// The third suspect's own workload. [`Up`](Self::Up) measures a *first*
    /// entry, which no cache can make cheap; this measures a **second** one,
    /// which is the case `Highlighter::sweep` decides. A reader scrolling back
    /// to something they just passed is the ordinary motion, and if eviction on
    /// hunk exit is a real cost it is here that it shows.
    Back,
}

impl Motion {
    fn step(self, at: usize) -> isize {
        match self {
            Motion::Down => WHEEL_ROWS,
            Motion::Up => -WHEEL_ROWS,
            Motion::Back if (at / LEG) % 2 == 0 => WHEEL_ROWS,
            Motion::Back => -WHEEL_ROWS,
        }
    }
}

#[test]
fn scrolling_down_wide_lines_holds_the_frame_budget() {
    let Some(run) = scroll("wide-down", Scroll::wide(Motion::Down, WIDE_EXT)) else {
        return;
    };
    hold_the_scroll_budget(&run, "scroll down", || {
        scroll("wide-down-again", Scroll::wide(Motion::Down, WIDE_EXT))
    });
}

#[test]
fn scrolling_a_hundred_files_from_the_first_frame_holds_the_frame_budget() {
    // **[#101](https://github.com/breferrari/vigia/issues/101)'s first exit
    // criterion, which no gate crossed.** Two dimensions the suite only ever ran
    // separately, plus the window that every other wall-clock gate here begins
    // *after*: a hundred changed files, wide-character content, scrolled, and
    // sampled from frame zero with no warmup at all.
    //
    // **What it reaches and what it does not, said plainly.** The worktree is
    // primed with `settle_spans`, so the scroll starts in the state a reader is
    // in a second after launch: nothing diffed, the margin waited out. The
    // grammar compile therefore lands inside the measured window and is caught by
    // the cold partition, which is where `SPEC.md` §7 puts a first parse. What it
    // does **not** reach is a repeated height walk, because a scroll takes no
    // `Frame::advance` and the walk is per tick; that one is
    // `ticking_over_an_undrawn_worktree_holds_the_frame_budget` above.
    //
    // Measured before #101's fix and expected to pass: p50 1.61ms, p99 2.78ms
    // over 92 wide files scrolled from frame zero. The prediction in #101's body
    // was that this pair of dimensions was where the cost lived, and it is not.
    // Gated anyway, because "we looked and it was fine" is not something a
    // regression can be held against.
    let Some(run) = scroll(
        "wide-many-first",
        Scroll {
            motion: Motion::Down,
            ext: WIDE_EXT,
            files: WIDE_MANY_FILES,
            warmup: 0,
            prime: Prime::Launched,
        },
    ) else {
        return;
    };
    hold_the_scroll_budget(&run, "scroll down from the first frame", || {
        scroll(
            "wide-many-first-again",
            Scroll {
                motion: Motion::Down,
                ext: WIDE_EXT,
                files: WIDE_MANY_FILES,
                warmup: 0,
                prime: Prime::Launched,
            },
        )
    });
}

#[test]
fn scrolling_up_wide_lines_holds_the_frame_budget() {
    // The direction `SPEC.md` §10 names as the worst case and which nothing had
    // ever run. Scrolling **down** enters a new hunk at its top, where the
    // forward-only parse has nothing above it to pay for; scrolling up enters
    // the same hunk at its **bottom**, so the first frame there parses the whole
    // file in order to draw its last rows.
    let Some(run) = scroll("wide-up", Scroll::wide(Motion::Up, WIDE_EXT)) else {
        return;
    };
    hold_the_scroll_budget(&run, "scroll up", || {
        scroll("wide-up-again", Scroll::wide(Motion::Up, WIDE_EXT))
    });
}

#[test]
fn scrolling_back_over_ground_already_read_holds_the_frame_budget() {
    let Some(run) = scroll("wide-back", Scroll::wide(Motion::Back, WIDE_EXT)) else {
        return;
    };
    hold_the_scroll_budget(&run, "scroll back", || {
        scroll("wide-back-again", Scroll::wide(Motion::Back, WIDE_EXT))
    });
}

#[test]
fn the_parse_is_attributed_by_subtracting_a_grammarless_run() {
    // The third suspect, given a number instead of a ranking. Two runs over
    // byte-identical content, one under a grammar and one under an extension
    // `syntect` has none for, so what separates them is the parse and nothing
    // else. Reported rather than gated: a difference of two wall clocks is
    // evidence, and `SPEC.md` §7 keeps the verdicts on named fixtures.
    //
    // What *is* asserted is the premise, because it is the half that can rot:
    // the grammarless run must really parse nothing.
    let Some(parsed) = scroll("wide-parse", Scroll::wide(Motion::Down, WIDE_EXT)) else {
        return;
    };
    let Some(plain) = scroll("wide-plain", Scroll::wide(Motion::Down, WIDE_UNPARSED_EXT)) else {
        return;
    };
    parsed.report("scroll down, with a grammar");
    plain.report("scroll down, grammarless");

    assert_eq!(
        plain.lines, 0,
        "the grammarless run highlighted {} lines, so `.{WIDE_UNPARSED_EXT}` is \
         no longer grammarless and this subtraction is between two parses",
        plain.lines
    );
    assert!(
        parsed.lines > 0,
        "the run under a grammar highlighted nothing, so there is no parse to \
         attribute"
    );

    let with = parsed.collect.percentile(0.99).unwrap_or_default();
    let without = plain.collect.percentile(0.99).unwrap_or_default();
    eprintln!(
        "the parse is {:?} of a scrolled frame's collect at p99 ({with:?} with a \
         grammar against {without:?} without), over {} lines",
        with.saturating_sub(without),
        parsed.lines,
    );
}

/// Scroll a wide-character fixture one notch a frame and measure it.
///
/// One function rather than several tests with a sign swapped, for the reason
/// [`frame_budget_at_depth`] gives: two runs only mean anything against each
/// other while every other term agrees.
///
/// **No edits, and no `Frame::advance`.** Every other gate in this file drives
/// the frame path with a writer in the other pane, which is I9's own wording.
/// This one drives it with the reader's own thumb, and that is a different frame:
/// `vigia::run` advances the frame on a `Wake::Tick` and *not* on a key or a
/// wheel event, so a scroll frame is collect plus paint. Timing an advance here
/// would add 2.7ms of work the product does not do on a keystroke and bury the
/// term being measured under it.
///
/// Returns `None` when the absolute tier does not apply, after the structural
/// setup has run.
fn scroll(name: &str, setup: Scroll) -> Option<Scrolled> {
    let Scroll {
        motion,
        ext,
        files,
        warmup,
        prime,
    } = setup;
    let scratch = Scratch::wide_lines_as(name, files, WIDE_LINES, ext);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    match prime {
        Prime::Materialised => settle(&mut frame),
        Prime::Launched => {
            let measured = settle_spans(&mut frame);
            assert_eq!(
                measured, files as u64,
                "priming measured {measured} of {files} files, so this run is \
                 already materialised and is not the launch state it claims"
            );
        }
    }
    assert_eq!(frame.files().len(), files, "fixture is not {files} files");

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let height = body(&app, files);
    let screen = layout(&app, files);

    if motion == Motion::Up {
        app.apply(
            Action::Scroll(isize::try_from(UP_FROM).expect("a sane depth")),
            &mut frame,
            height,
        )
        .expect("scroll");
        // Resolved by the collect rather than by the scroll: `App` adds the rows
        // to the current file's offset and lets `View::collect` carry the
        // overrun into the files below, which is what keeps a scroll to one diff
        // per file. So the position is asserted *after* a view, and in files
        // rather than in rows.
        let view = app
            .view(&mut frame, &mut highlighter, &history, screen)
            .expect("view");
        assert!(
            view.top.file >= UP_FILES,
            "{UP_FROM} rows landed on file {} of {files}, so there are fewer \
             than {UP_FILES} files above the viewport and scrolling up will \
             reach the top before it has crossed anything",
            view.top.file
        );
    }

    if !absolute_gates_apply("cargo test --release -p vigia --test budgets") {
        return None;
    }

    let _timed = exclusively_timed();

    let theme = Theme::default();
    let mut buf = Buffer::empty(area());

    // Split rather than timed whole, because "which of the three dominates" is
    // what #45 asks and one total cannot answer it. Their sum is what the budget
    // is held against, so nothing is counted twice.
    let mut run = Scrolled {
        warm: Samples::new(SAMPLED_FRAMES),
        warm_cpu: Samples::new(SAMPLED_FRAMES),
        cold: Samples::new(SAMPLED_FRAMES),
        collect: Samples::new(SAMPLED_FRAMES),
        paint: Samples::new(SAMPLED_FRAMES),
        cold_lines: 0,
        lines: 0,
        evicted: 0,
        boundaries: 0,
        widest: 0,
        body_rows: 0,
        height,
        painted: PaintStats::default(),
        frames: warmup + SAMPLED_FRAMES,
    };
    let mut at_file = usize::MAX;

    for at in 0..(warmup + SAMPLED_FRAMES) {
        app.apply(Action::Scroll(motion.step(at)), &mut frame, height)
            .expect("scroll");

        let before = highlighter.stats();
        // Wall and CPU for both stages, because #178's attribution is per sample:
        // the question is whether the work in *this* round was inside budget, and a
        // total taken around the loop would fold in the untimed scrolling between
        // frames.
        let (screen, collect, collect_cpu) = timed_cpu(|| {
            app.view(&mut frame, &mut highlighter, &history, screen)
                .expect("view")
        });
        let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
        let (painted, paint, paint_cpu) = timed_cpu(|| {
            render(
                &mut buf,
                area(),
                &screen,
                &theme,
                Glyphs::default(),
                &chrome,
            )
        });
        let parsed = highlight_delta(before, highlighter.stats());

        if screen.top.file != at_file {
            at_file = screen.top.file;
            run.boundaries += 1;
        }
        run.body_rows = screen.rows.len();
        run.widest = run.widest.max(
            screen
                .rows
                .iter()
                .filter_map(|row| match row {
                    Row::Line { text, .. } => Some(text.chars().count()),
                    _ => None,
                })
                .max()
                .unwrap_or(0),
        );

        if at < warmup {
            continue;
        }

        run.lines += parsed.lines;
        run.evicted += parsed.evicted;
        // Accumulated, like the two above it and unlike the assignment this used
        // to be. `Scrolled::report` prints all three in one sentence, so a field
        // holding the last frame while its neighbours hold the run was a figure
        // that read as a total and was not one.
        run.painted += painted;
        run.collect.push(collect);
        run.paint.push(paint);

        // The partition, and it is `SPEC.md` §7's carve-out rather than a
        // convenience: a frame that parses a hunk for the first time is on the
        // cold path, which I9 excludes by definition. Diluting the steady state
        // with those would let a regression hide behind them, and dropping them
        // silently would hide the one number #45 exists to surface. So they are
        // separated, both are reported, and only the steady half is asserted.
        if parsed.parsed > 0 {
            run.cold.push(collect + paint);
            run.cold_lines = run.cold_lines.max(parsed.lines);
        } else {
            run.warm.push(collect + paint);
            run.warm_cpu.push(collect_cpu + paint_cpu);
        }
    }

    Some(run)
}

/// The assertions both directions share.
fn hold_the_scroll_budget(run: &Scrolled, what: &str, again: impl FnOnce() -> Option<Scrolled>) {
    run.report(what);

    let height = run.height;

    // Non-vacuity, in four directions.
    //
    // The screen has to have been full, or a frame that drew two rows is a cheap
    // frame for a reason that is not the code.
    assert_eq!(
        run.body_rows, height,
        "the last body drew {} of {height} rows, so these were not full screens",
        run.body_rows
    );

    // Boundaries have to have been crossed, or this measured one file and the
    // cold half of the partition is empty for a reason that is the fixture
    // rather than the code.
    assert!(
        run.boundaries >= UP_FILES,
        "the viewport crossed {} file boundaries in {} frames, so this run never \
         entered a hunk it had not parsed",
        run.boundaries,
        run.frames
    );
    assert!(
        !run.cold.is_empty() && !run.warm.is_empty(),
        "the partition is one-sided: {} warm frames and {} cold",
        run.warm.len(),
        run.cold.len()
    );

    // Highlighting has to have actually happened, which is the direction this
    // whole file exists for and the one the partition above cannot see: a run
    // over a file type nothing recognises has warm frames, cold frames and
    // boundaries, and is the core's frame path with the syntax parser missing.
    assert!(
        run.lines > 0,
        "no lines were highlighted across the sampled frames, so this gate is \
         timing a collect with the parser idle"
    );

    // The lines have to be wider than the pane, or this is `large_diff` with a
    // different name on it and it cannot tell a bounded paint from an unbounded
    // one.
    assert!(
        run.widest > usize::from(area().width),
        "the widest drawn line is {} characters against an {}-column pane, so \
         this fixture never exceeds the pane",
        run.widest,
        area().width
    );

    // And a cold frame has to be bounded by the hunk it entered, which is what
    // says the rewind still holds at this width: one whole new hunk, plus a
    // screenful of the neighbour beside it, plus the stride a changed hunk can
    // rewind past.
    let cold_bound = (WIDE_HUNK_ROWS + height + CHECKPOINT_STRIDE) as u64;
    assert!(
        run.cold_lines <= cold_bound,
        "a frame entering a new hunk parsed {} lines, over the {cold_bound} that \
         one hunk plus a screen can cost, so the parse is not bounded by what \
         the frame entered",
        run.cold_lines
    );

    holds_p99_rounds(
        &format!("I9: {what} through wide lines"),
        budget(I9_FRAME),
        &run.warm,
        || {
            format!(
                "over {} steady frames (collect p99 {:?}, paint p99 {:?}; {} cold \
                 frames at {:?} p99)",
                run.warm.len(),
                run.collect.percentile(0.99).unwrap_or_default(),
                run.paint.percentile(0.99).unwrap_or_default(),
                run.cold.len(),
                run.cold.percentile(0.99).unwrap_or_default(),
            )
        },
        // A whole scripted motion rather than a frame, because that is the unit
        // this gate samples: the run partitions its own frames into the ones that
        // entered a hunk and the ones that did not, and a single extra frame
        // belongs to neither.
        || {
            let again =
                again().expect("the re-measure skipped the absolute tier the first round ran");
            (again.warm, Some(again.warm_cpu))
        },
    );
}

/// Files in the prose fixture, and lines each.
///
/// Smaller than [`FILES`] x [`LINES`] on purpose. This gate's axis is the cost
/// of *one screenful of prose*, which is bounded by the pane, so a hundred files
/// would add revalidation cost that `ticking_over_an_undrawn_worktree_...`
/// already owns and bury the term being measured under it.
const PROSE_FILES: usize = 10;
const PROSE_LINES: usize = 200;

/// A screenful of ordinary Markdown prose holds the frame budget.
///
/// **The gate [#261](https://github.com/breferrari/vigia/issues/261) was missing,
/// and the defect it would have caught shipped for months.** Every I9 gate in
/// this file drew Rust or wide CJK text. Nothing drew the shape a reader
/// watching a documentation change actually has on screen: sentences with
/// identifiers in backticks. Measured on that content before the fix, a worst
/// 24-line screenful of this repository's own `ROADMAP.md` cost **229.48ms of
/// parse against this 16ms budget** on Linux, fully warm, on every frame
/// that redrew it.
///
/// The cause was Markdown's block-start lookahead exploring the inline-content
/// alternation exponentially before concluding that a line with no `|` could not
/// be a table row. `xtask` now guards that pattern; see
/// `crates/vigia-core/tests/coverage.rs` for the gates that hold the rewrite
/// itself sound and present.
///
/// **The grammarless twin is what makes this a measurement rather than an
/// assertion.** Two runs over byte-identical content, one resolving Markdown and
/// one under an extension `syntect` has no grammar for, so what separates them is
/// the parse and nothing else. Without it a breach here reads as "a frame is
/// slow" and points at no layer in particular, which is `SPEC.md` §7's complaint
/// about a gate that goes red without saying what moved.
#[test]
fn a_frame_over_prose_with_code_spans_holds_the_frame_budget() {
    let Some(parsed) = prose_frame(PROSE_EXT) else {
        return;
    };
    let Some(plain) = prose_frame(WIDE_UNPARSED_EXT) else {
        return;
    };

    // The premise, because it is the half that can rot: the control really
    // parses nothing, and the gated arm really parses.
    assert_eq!(
        plain.lines, 0,
        "the grammarless run highlighted {} lines, so `.{WIDE_UNPARSED_EXT}` is \
         no longer grammarless and this subtraction is between two parses",
        plain.lines
    );
    // At least one parsed line **per frame**, not one across the whole sample.
    // `> 0` is satisfied by a single line in two hundred and fifty frames, so if
    // hunk reuse ever started hitting here the gate would keep passing while
    // measuring a frame path with the parser idle, and still call itself a parse
    // measurement. The steady state this gate claims is one re-parse a frame.
    assert!(
        parsed.lines >= SAMPLED_FRAMES as u64,
        "{} lines were highlighted across {SAMPLED_FRAMES} frames, under the one \
         a frame this gate's steady state claims, so the parser is idle for most \
         of the sample and this is not the measurement it reports",
        parsed.lines
    );

    let with = parsed.samples.percentile(0.99).unwrap_or_default();
    let without = plain.samples.percentile(0.99).unwrap_or_default();
    eprintln!(
        "prose with {PROSE_SPANS} code spans a line: p99 {with:?} with a grammar, \
         {without:?} grammarless, so the parse is {:?} of the frame over {} lines",
        with.saturating_sub(without),
        parsed.lines,
    );

    holds_p99_rounds(
        &format!("I9: a frame over Markdown prose carrying {PROSE_SPANS} code spans a line"),
        budget(I9_FRAME),
        &parsed.samples,
        || {
            format!(
                "({} hunks parsed, {} lines; grammarless control p99 {without:?})",
                parsed.parsed, parsed.lines,
            )
        },
        // **A whole run rather than a frame, and the difference is not
        // cosmetic.** `holds_p99` calls its closure once per sample, which is
        // right when a sample is one frame of an already-built fixture. Here a
        // sample cannot be produced alone: `prose_frame` builds a git fixture,
        // settles it and runs 50 warmup frames before the first sampled one.
        // Wired through `holds_p99` this gate re-measured by running that whole
        // sequence 250 times, which is 250 fixtures and over 20 minutes to
        // produce 250 frames that were each the *first* sampled frame of a cold
        // run. Observed, not reasoned about: a breaching round hung for 23
        // minutes before it was killed. `hold_the_scroll_budget` uses this
        // variant for the identical reason.
        || {
            let again = prose_frame(PROSE_EXT)
                .expect("the re-measure skipped the absolute tier the first round ran");
            (again.samples, Some(again.cpu))
        },
    );
}

/// One prose arm: what a frame costs over `ext`, and what the highlighter did.
struct ProseRun {
    samples: Samples,
    /// The same frames in thread CPU time, for the reason [`Scrolled`] carries
    /// one: contention inflates a wall clock and cannot inflate this.
    cpu: Samples,
    parsed: u64,
    lines: u64,
}

/// Drive I9's own shape over the prose fixture and measure it.
///
/// One function taking the extension rather than two near-copies, for the reason
/// [`frame_budget_at_depth`] gives: two runs only mean anything against each
/// other while every other term agrees, and here they agree by construction,
/// because the bytes are identical and only the resolved grammar differs.
///
/// Returns `None` when the absolute tier does not apply, after the structural
/// setup has run.
fn prose_frame(ext: &str) -> Option<ProseRun> {
    let scratch = Scratch::prose_lines_as(&format!("prose-{ext}"), PROSE_FILES, PROSE_LINES, ext);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    assert_eq!(
        frame.files().len(),
        PROSE_FILES,
        "fixture is not {PROSE_FILES} files"
    );

    if !absolute_gates_apply("cargo test --release -p vigia --test budgets") {
        return None;
    }

    let _timed = exclusively_timed();

    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let mut history = History::new();
    let height = body(&app, PROSE_FILES);
    let screen = layout(&app, PROSE_FILES);

    let edited = format!("docs/prose_0.{ext}");

    let mut edits = 0usize;
    let marker = RefCell::new(String::new());
    let theme = Theme::default();
    let mut buf = Buffer::empty(area());
    let mut next_frame =
        |frame: &mut Frame, app: &mut App, highlighter: &mut Highlighter, history: &mut History| {
            // The edit is prose of the same shape, so the rewritten line costs
            // what every other line on screen costs. An edit writing plain text
            // would make the one line the reader is watching the cheapest on the
            // pane, which is the opposite of the case being measured.
            *marker.borrow_mut() = prose_generated(1, &format!("edit{edits}"))
                .trim_end()
                .to_string();
            scratch.edit_line(&edited, 0, &marker.borrow());
            edits += 1;
            time_cpu(|| {
                sample(history, scratch.root(), &edited);
                shell_frame(frame, app, highlighter, history, &mut buf, &theme, screen);
            })
        };

    for _ in 0..WARMUP_FRAMES {
        next_frame(&mut frame, &mut app, &mut highlighter, &mut history);
    }

    let before = highlighter.stats();
    let mut samples = Samples::new(SAMPLED_FRAMES);
    let mut cpu = Samples::new(SAMPLED_FRAMES);
    for _ in 0..SAMPLED_FRAMES {
        let (wall, thread) = next_frame(&mut frame, &mut app, &mut highlighter, &mut history);
        samples.push(wall);
        cpu.push(thread);
    }
    let cost = highlight_delta(before, highlighter.stats());

    // The screen has to have been full, or a frame that drew two rows is cheap
    // for a reason that is not the code.
    drew_a_full_screen(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        screen,
        height,
    );

    Some(ProseRun {
        samples,
        cpu,
        parsed: cost.parsed,
        lines: cost.lines,
    })
}

/// Rounds the ageing comparison samples, warmed the way every gate here warms.
///
/// **Fifty rather than sixty, and the ceiling is the window.** Each round steps
/// the store by two samples, one per arm, so sixty would reach
/// [`HISTORY_SAMPLES`] exactly and drain the fixture's tracks on the last round:
/// the walk being priced is per *track*, so an emptied store makes the final
/// rounds cheap for a reason that is not the code.
const SAMPLED_AGEINGS: usize = 50;

#[test]
fn an_ageing_wake_costs_a_fraction_of_the_tick_it_is_not() {
    // **The number [#243](https://github.com/breferrari/vigia/issues/243)'s
    // ruling rests on, gated so it cannot quietly stop being true.** I1 was
    // amended to let a clock run while the history window holds a sample, and
    // the amendment is affordable because an ageing wake is not a filesystem
    // event: nothing on disk changed, so it does not walk status. If a later
    // edit puts `Frame::advance` back on that path the clock stays correct, the
    // graph still ages, and the cost quietly becomes a tick's.
    //
    // **Two arms rather than one number against a ceiling**, which is this
    // file's rule and the reason it exists: a lone figure has nothing to be
    // compared against and gets compared against a budget instead. Interleaved,
    // because a sequential pair under varying load is not a controlled
    // experiment.
    if !absolute_gates_apply("cargo test --release -p vigia --test budgets") {
        return;
    }
    let _timed = exclusively_timed();

    let scratch = Scratch::large_diff("ageing-wake", 20, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::eager();
    let mut history = History::new();
    let screen = layout(&app, 20);
    let theme = Theme::default();
    let mut buf = Buffer::empty(area());

    // **A window at the path cap, driven on a clock that actually crosses a
    // sample boundary**, and both halves of that were wrong in the first draft.
    // It held twenty paths where `repeak` is priced at the 256-path cap, and it
    // stamped every record with `Instant::now()`, so a hundred rounds of a
    // sub-millisecond frame all landed inside one second: `rolled` was zero every
    // time and the #277 guard skipped the walk in *both* arms. The gate behind
    // the measurement that licensed I1's amendment was timing `Frame::advance`
    // on and off and never once timed an ageing wake.
    let paths: Vec<String> = (0..HISTORY_PATHS).map(|n| format!("src/f{n}.rs")).collect();
    let stamped = Instant::now();
    history.record_sized(
        paths.iter().map(|path| (path.as_str(), Some(4_000u64))),
        stamped,
    );

    let (mut ageing, mut ticking) = (Samples::new(SAMPLED_AGEINGS), Samples::new(SAMPLED_AGEINGS));
    for round in 1..=SAMPLED_AGEINGS {
        for walks in [false, true] {
            // **One sample per arm, not per round.** Both arms sharing a round's
            // instant meant only the first crossed a boundary: the ageing arm
            // paid the projection and the arm it is compared against skipped it,
            // which is the comparison backwards. The store takes the instant as
            // an argument, which is what makes a synthetic clock possible here at
            // all; the frame is still timed against the real one.
            let step = u32::try_from(round * 2 + usize::from(walks)).expect("a round");
            let at = stamped + HISTORY_SAMPLE * step;
            let (wall, _) = time_cpu(|| {
                // The ageing arm is exactly what `Shell::draw` does on a wake
                // that changed nothing on disk: roll the window, then draw. The
                // ticking arm adds the status walk a filesystem event brings with
                // it, and nothing else.
                if walks {
                    frame.advance().expect("advance");
                }
                history.record_sized([], at);
                frame_body(
                    &mut frame,
                    &mut app,
                    &mut highlighter,
                    &history,
                    &mut buf,
                    &theme,
                    screen,
                );
            });
            if round > SAMPLED_AGEINGS / 4 {
                if walks {
                    ticking.push(wall)
                } else {
                    ageing.push(wall)
                }
            }
        }
    }

    let aged = ageing.percentile(0.5).expect("a sampled round");
    let ticked = ticking.percentile(0.5).expect("a sampled round");

    // **Non-vacuity on the thing the first draft got wrong**: every round has to
    // have crossed a boundary and walked the projection, or neither arm timed an
    // ageing wake and the comparison is about `Frame::advance` alone.
    let walked = history.stats().repeaks;
    assert!(
        walked >= (SAMPLED_AGEINGS * 2) as u64,
        "the fixture walked the projection {walked} times over {} arms, so they \
         are not crossing sample boundaries and neither is an ageing wake",
        SAMPLED_AGEINGS * 2
    );
    assert!(
        history.tracked() > 0,
        "the fixture's window drained before the rounds ended, so the later ones \
         priced a walk over nothing"
    );

    // Non-vacuity through the shared helper rather than a copy of it, which is
    // what its own docblock asks for: a `height` term drifting out of step is
    // invisible in a second spelling.
    drew_a_full_screen(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        screen,
        screen.diff,
    );

    assert!(
        aged < ticked,
        "an ageing wake cost {aged:?} against a tick's {ticked:?}, so the path \
         that skips the status walk is no longer cheaper than the one that does \
         it, which is the whole reason I1 could be amended for this clock"
    );
    // And it is a *fraction*, not merely smaller. Stated loosely on purpose: the
    // reference machine measures roughly a third, and a gate pinned near that
    // would be a gate about the runner rather than about the path.
    assert!(
        aged * 2 < ticked,
        "an ageing wake cost {aged:?} against a tick's {ticked:?}, less than half \
         a saving where the reference machine measures 165µs against 529µs"
    );
}

/// The pane the sheet's own budget is measured on.
///
/// **Short and wide on purpose**, because that is the pane the two-column rung
/// arrives on ([#220](https://github.com/breferrari/vigia/issues/220)) and it is
/// the widest the sheet ever draws: 104 columns by 14 rows against the
/// one-column rung's 56 by 19. It is no longer the **largest**, which is a
/// different superlative and belongs to the roomy rung below.
const SHEET_PANE: Rect = Rect {
    x: 0,
    y: 0,
    width: 120,
    height: 21,
};

/// The size of the sheet `pane` draws, with the sheet up.
///
/// **Shared because two gates ask it**, which is `walk_the_ladder`'s own lesson
/// one file over: that scaffold was copied between three gates until #220's audit
/// pointed at it. Each caller still builds its own repository, so the two keep
/// running in parallel.
fn sheet_size_on(name: &str, pane: Rect) -> (u16, u16) {
    let mut app = App::new();
    let scratch = Scratch::large_diff(name, FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    let screen = layout_of(&app, pane, FILES);
    app.apply(vigia::Action::ToggleSheet, &mut frame, screen.diff)
        .expect("toggle the sheet");
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let laid = vigia::regions(pane, &chrome, &{
        let mut highlighter = Highlighter::eager();
        let history = History::new();
        app.view(&mut frame, &mut highlighter, &history, screen)
            .expect("view")
    });
    let drawn = laid
        .sheet
        .expect("the pane this gate is named for draws no sheet");
    (drawn.width, drawn.height)
}

/// I9 with the gestures sheet drawn over the frame.
///
/// **A debt `SPEC.md` §11.2 has named since B12 was ruled**, where it says the
/// frame path sits well inside I9 and *"the sheet **covers** diff rows carrying
/// highlighted spans with about nineteen rows of plain text, so the expectation
/// is that it costs less than what it hides"*, followed by *"that is an
/// expectation and #206 owes the measurement"*. #206 closed without it, and
/// [#220](https://github.com/breferrari/vigia/issues/220) made the debt larger
/// rather than smaller: the sheet's widest rung went from 56 columns to 104, and
/// its drawer moved from bulk string writes to per-cell ones through
/// [`Painter::rule`] and a pipe pass.
///
/// Non-vacuity is asserted rather than assumed, the way the rail's gate does it:
/// this reddens if the pane it names stops drawing a sheet at all, or stops
/// drawing the two-column rung, either of which would leave it timing the gate
/// above under a different name.
#[test]
fn a_frame_under_the_sheet_holds_the_frame_budget() {
    assert_eq!(
        sheet_size_on("shell-i9-sheet-shape", SHEET_PANE),
        (104, 18),
        "the {}x{} pane does not draw the two-column rung, so this gate is not \
         timing the shape it is named for",
        SHEET_PANE.width,
        SHEET_PANE.height
    );

    frame_budget_on(
        "shell-i9-sheet",
        0,
        SHEET_PANE,
        Some("keyboard"),
        false,
        false,
    );
}

/// The pane the roomy rung's own budget is measured on.
///
/// **Tall and wide**, because that is what the roomy rung needs
/// ([#285](https://github.com/breferrari/vigia/issues/285)): a room of
/// sixty-eight columns and a body of thirty-one rows.
const ROOMY_PANE: Rect = Rect {
    x: 0,
    y: 0,
    width: 120,
    height: 40,
};

/// I9 with the **roomy** rung drawn over the frame.
///
/// **The two-column rung is the widest the sheet draws and this one covers the
/// most cells**, which are two different superlatives and only the first was
/// gated: 68 by 31 is 2,108 cells against 104 by 16's 1,664. It is also the rung
/// a full-screen terminal now takes, so it is the shape most readers will
/// actually have on the pane, where the two-column rung is what a short one falls
/// to.
///
/// Its drawer is not the other's either: it writes a label per section and pipes
/// every interior row including the blank ones, where `Shape::Beside` writes one
/// rule carrying two labels. B12's expectation is that the sheet costs less than
/// the highlighted rows it hides, and a rung covering a third more cells than the
/// one that expectation was last checked against is where it would stop being
/// true.
#[test]
fn a_frame_under_the_roomy_sheet_holds_the_frame_budget() {
    assert_eq!(
        sheet_size_on("shell-i9-roomy-shape", ROOMY_PANE),
        (68, 35),
        "the {}x{} pane does not draw the roomy rung, so this gate is not timing \
         the shape it is named for",
        ROOMY_PANE.width,
        ROOMY_PANE.height
    );

    frame_budget_on(
        "shell-i9-roomy",
        0,
        ROOMY_PANE,
        Some("moving"),
        false,
        false,
    );
}

/// I9 with the diff pinned to one file, which is `SPEC.md` §11.2 B16.
///
/// **A gate for the direction nobody measures, and that is why it is here.** The
/// pin is the one gesture on this map that makes a frame do *less*: the walk is
/// bounded to one file and `View::measure` skips `diff_rows`, which is I4's only
/// exception and the only thing in the frame path not bounded by the window. So
/// the expectation is that a pinned frame is cheaper than the gate above it, and
/// an expectation is exactly what §7 says a budget must not be. Until #297's
/// second audit round no budget, no soak and no snapshot ever set `single`, on
/// the one state that changes the walk's range.
///
/// **It is not measured against the unpinned frame**, and that is deliberate: a
/// part compared against the whole it belongs to is the shape §7 records as
/// failing silently in the green direction. This asserts the absolute budget, the
/// same 16ms every other gate in this file asserts, and the *reads* claim is
/// `tests/reads.rs::a_pinned_frame_counts_no_height_at_all`, which is structural
/// and has a non-vacuous control.
///
/// Non-vacuity: the pane is the one every other frame gate uses, so a pin that
/// stopped reaching the walk would leave this timing the gate above under a
/// different name. `a_pinned_frame_counts_no_height_at_all` is what would redden
/// for that, and it is the reason this one does not restate it.
#[test]
fn a_pinned_frame_holds_the_frame_budget() {
    frame_budget_on("shell-i9-single", 0, area(), None, false, true);
}

/// **What the staged run costs, in the frame it sits in rather than on its own.**
///
/// `SPEC.md` §11.2 **B17** ([#313](https://github.com/breferrari/vigia/issues/313)).
/// Reported rather than gated, which is the same tier
/// [`what_a_bulk_rewrite_of_undrawn_files_costs`] sits in and for the same reason:
/// the interesting figure is a *ratio between two arms on one machine*, and a
/// ceiling on either arm alone would be a number about the runner.
///
/// **Interleaved, because a sequential pair under varying load is not a controlled
/// experiment** — the same trap `sizing_a_whole_burst_does_not_change_the_frame_it_sits_in`
/// records nearly filing a phantom regression on. Both arms draw the same worktree
/// through the whole shell frame, and the only difference is whether the frame is
/// also walking `HEAD^{tree}` against the index and diffing what it finds.
///
/// **Why the whole frame and not the walk.** `CLAUDE.md`'s rule, from the episode
/// where a syscall was measured twice in isolation and read two opposite ways: a
/// component timed alone answers a question nobody asked, and what decides whether
/// something is affordable is the thing it sits inside, measured with and without
/// it. In isolation the staged walk is 430µs against the index-worktree walk's
/// 2.15ms on a 200-file fixture; that number priced the design and this one prices
/// the product.
///
/// **Measured 2026-08-25 over a hundred files of five hundred lines, half of them
/// staged: a frame is 5.15ms p50 drawing both runs and 3.10ms drawing one**, which
/// is 32% of I9's 16ms against 19%. The extra two milliseconds are real and they
/// are what a second walk and fifty more diffs cost; what makes them affordable is
/// that the frame has three times that left in headroom and that **nobody pays
/// them unasked**, since the run is off until a reader presses `a`. Quoted with
/// the budget rather than alone, which is `CLAUDE.md`'s rule about citing one: a
/// ceiling without the floor the tool is standing on is a mood.
#[test]
fn what_the_staged_run_costs_the_frame_it_is_drawn_in() {
    if !absolute_gates_apply("cargo test --release -p vigia --test budgets") {
        return;
    }
    let _timed = exclusively_timed();

    let scratch = Scratch::large_diff("staged-frame", FILES, LINES);
    // Half the changed set staged, so both runs are populated and the pane is
    // actually drawing the shape B17 added rather than a second empty walk.
    let staged: Vec<String> = (0..FILES / 2).map(|i| format!("src/mod_{i}.rs")).collect();
    let mut args: Vec<&str> = vec!["add"];
    args.extend(staged.iter().map(String::as_str));
    scratch.git(&args);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let screen = layout(&app, FILES);
    let theme = Theme::default();
    let mut buf = Buffer::empty(area());

    let (mut both, mut one) = (Samples::new(SAMPLED_BURSTS), Samples::new(SAMPLED_BURSTS));
    for round in 1..=SAMPLED_BURSTS {
        for run in [true, false] {
            scratch.rewrite_all(FILES, LINES, round);
            frame.show_staged(run);
            let (wall, _) = time_cpu(|| {
                shell_frame(
                    &mut frame,
                    &mut app,
                    &mut highlighter,
                    &history,
                    &mut buf,
                    &theme,
                    screen,
                );
            });
            // Warm rounds only, for the reason every gate in this file warms.
            if round > SAMPLED_BURSTS / 4 {
                if run { both.push(wall) } else { one.push(wall) }
            }
        }
    }

    let with = both.percentile(0.5).expect("a sampled round");
    let without = one.percentile(0.5).expect("a sampled round");

    // **Non-vacuity on what the run did rather than on what the fixture is.** A
    // fixture assertion would be true by construction; this says the frame really
    // did hold two runs on the arm that claims to.
    frame.show_staged(true);
    frame.advance().expect("advance");
    let staged_files = frame
        .files()
        .iter()
        .filter(|change| change.origin == vigia_core::Origin::Staged)
        .count();
    assert!(
        staged_files > 0 && staged_files < frame.files().len(),
        "the fixture does not hold both runs, so the two arms are the same frame: \
         {staged_files} staged of {}",
        frame.files().len()
    );

    println!(
        "staged run: frame p50 {with:?} with both runs, {without:?} with one, over \
         {FILES} files and {LINES} lines"
    );
}
