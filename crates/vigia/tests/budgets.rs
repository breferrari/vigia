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
    Action, App, Body, Glyphs, PaintStats, Row, Theme, View, WHEEL_ROWS, body_layout, render,
};
use vigia_core::{
    CHECKPOINT_STRIDE, Frame, HISTORY_PATHS, Highlighter, History, LineKind, Samples,
};

use support::{
    Scratch, WIDE_EXT, WIDE_UNPARSED_EXT, absolute_gates_apply, budget, delta, exclusively_timed,
    generated, highlight_delta, holds_p99, holds_p99_rounds, settle, settle_spans, time, time_cpu,
    timed_cpu,
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
    body_layout(
        area(),
        &app.chrome("fixture", None, None, None, None, None),
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
    app.sample_memory();
    let chrome = app.chrome("fixture", None, None, None, None, None);
    let view = app.view(frame, highlighter, history, screen).expect("view");
    render(buf, area(), &view, theme, Glyphs::default(), &chrome);
    // Recorded from an inner clock rather than handed the caller's, because
    // every caller times this differently: some wrap it in `time`, some in
    // `timed`, and the scroll gates wrap a whole motion. What the ring needs is
    // one frame's cost, and this is the only place that knows where one starts.
    app.record_frame(began.elapsed());
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
    let mut highlighter = Highlighter::new();
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

    let chrome = app.chrome("fixture", None, None, None, None, None);
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

#[test]
fn sizing_a_whole_burst_costs_a_fraction_of_the_frame_it_sits_in() {
    // **What [#232](https://github.com/breferrari/vigia/issues/232) added to a
    // tick, gated at its widest rather than at its typical.** Weighing a write by
    // the bytes it moved means one `symlink_metadata` per changed path per wake,
    // and the watcher coalesces a bulk rewrite into a single wake, so the widest
    // tick this tool has sizes every file in the burst at once. A gate that sized
    // one path would have measured the case that was never in doubt.
    //
    // The cost is not hypothetical and this repo has already paid it once: an
    // `lstat` before every working-tree read measured **+1.18ms p50** over a
    // hundred undrawn files, which is why `FileChange::maybe_symlink` exists to
    // carry the walk's answer instead. That is the number this gate exists to
    // keep from coming back through a different door.
    //
    // A tenth of the frame, which is looser than the memory read's hundredth
    // above and for the same stated reason: the point is to catch a stat that has
    // become a read or a walk, not to track microseconds on a shared runner.
    if !absolute_gates_apply("cargo test --release -p vigia --test budgets") {
        return;
    }
    let _timed = exclusively_timed();

    let budget = I9_FRAME / 10;
    let scratch = Scratch::large_diff("burst-sizing", FILES, LINES);
    let paths = bulk_burst();
    let mut history = History::new();

    // Warmed, for the memory read's reason one gate up: the first `stat` of a
    // path faults in whatever the platform caches for it, and `SPEC.md` §7's rule
    // about steady state applies to a syscall as much as to a frame.
    for _ in 0..10 {
        sample_all(&mut history, scratch.root(), &paths);
    }

    // **Sampled rather than timed once**, which is the instrument rule the
    // gates below already follow. A single `time` call against a sub-millisecond
    // quantity is one scheduler hiccup away from a red build.
    // **Timed as one block rather than per round, because the thread clock has a
    // quantum.** `GetThreadTimes` reports in 15.625ms steps on Windows, so a
    // sub-millisecond burst measured on its own reads `0ns`, and a budget
    // compared against that is trivially true for every implementation. This gate
    // shipped in exactly that state, and the cap in `vigia::SIZED_PATHS` was
    // *removed* on the reading: zero CPU said the cost was time spent waiting,
    // which `SPEC.md` §7 attributes to the host. Rolling the rounds into one
    // measurement clears the quantum, and the cost is CPU after all.
    let (wall, spent) = time_cpu(|| {
        for _ in 0..SAMPLED_BURSTS {
            sample_all(&mut history, scratch.root(), &paths);
        }
    });
    let rounds = u32::try_from(SAMPLED_BURSTS).expect("a sane round count");
    let (wall, taken) = (wall / rounds, spent / rounds);

    // Non-vacuity, and it is the assertion that matters most: a burst that sized
    // nothing would post a very fast time and pass a budget it never spent.
    assert_eq!(
        paths.len(),
        HISTORY_PATHS,
        "the burst was not the widest a wake can carry, so this timed a narrower \
         tick than the product has"
    );
    assert!(
        history.churn(EDITED_PATH).is_some(),
        "the burst recorded nothing, so this gate timed a walk over paths the \
         store ignored"
    );
    // **Asserted on thread CPU time and reported on both**, which is `SPEC.md`
    // section 7's own rule for this tier ([#212](https://github.com/breferrari/vigia/issues/212)):
    // a wall-clock overshoot spent off-CPU is the host, one spent on-CPU is ours,
    // and contention cannot inflate a CPU clock. It matters here more than
    // anywhere else in this file, because a `stat` is almost entirely waiting. On
    // the reference machine this burst measures about 2.3ms of wall against
    // **0ns** of CPU, and a bound read off the wall number would have capped the
    // feature to buy back time the frame never spent.
    // **Both clocks, and neither may read zero.** The CPU figure is the one that
    // survives contention; the wall figure is the one a reader waits through. The
    // non-vacuity assertion is the half this gate shipped without: with a clock
    // too coarse to see the burst, `taken <= budget` was `0ns <= 1.6ms` and could
    // not fail for any implementation.
    assert!(
        taken > Duration::ZERO,
        "the thread clock reported no time for {SAMPLED_BURSTS} bursts, so it          cannot see this cost and the budget below asserts nothing"
    );
    assert!(
        taken <= budget && wall <= budget,
        "sizing a {HISTORY_PATHS}-path burst spent {taken:?} of thread CPU and          {wall:?} of wall against {budget:?}, a tenth of the {I9_FRAME:?} frame          it shares"
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
    let scratch = Scratch::large_diff(name, FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    assert_eq!(frame.files().len(), FILES, "fixture is not {FILES} files");

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let mut history = History::new();
    let height = body(&app, FILES);
    let screen = layout(&app, FILES);

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
    let mut buf = Buffer::empty(area());
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
        &format!("I9: a real frame with highlighting over {FILES} files"),
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
    let mut highlighter = Highlighter::new();
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
    let mut highlighter = Highlighter::new();
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
    let mut highlighter = Highlighter::new();
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
    let mut highlighter = Highlighter::new();
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
        let chrome = app.chrome("fixture", None, None, None, None, None);
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
