//! I9, held against the shell rather than against the core.

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
const FILES: usize = 100;
const LINES: usize = 500;

/// The file rewritten before every frame, which is the one the view is sitting
/// on.
const EDITED_PATH: &str = "src/mod_0.rs";

/// Timed frames between bulk rewrites in the gate at the bottom of this file.
const REWRITE_EVERY: usize = 50;

/// How long the bulk rewrite may go unrefreshed before the files it wrote start
/// settling underneath the window that is supposed to be measuring them.
const REWRITE_WITHIN: Duration = Duration::from_millis(1_000);

/// Frames discarded before sampling, and frames sampled.
const WARMUP_FRAMES: usize = 50;
const SAMPLED_FRAMES: usize = 250;

/// Sample the history the way `vigia::run` does, `stat` included.
fn sample(history: &mut History, root: &Path, path: &str) {
    sample_all(history, root, &[path.to_owned()]);
}

/// [`sample`] over a whole burst, which is what a bulk rewrite delivers.
fn sample_all(history: &mut History, root: &Path, paths: &[String]) {
    // `vigia::weigh`, which is the one `run` calls.
    history.record_sized(vigia::sized(root, paths), Instant::now());
}

/// Rounds the burst gate times before taking a median.
const SAMPLED_BURSTS: usize = 30;

/// The widest burst a wake can carry, as paths.
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
    // Recorded from an inner clock rather than handed the caller's, because every
    // caller times this differently: some wrap it in `time`, some in `timed`, and the
    // scroll gates wrap a whole motion.
    app.record_frame(began.elapsed());
}

/// Everything a frame does except walk status, which is the half an ageing wake
/// pays and a tick pays on top of.
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
    // The pane comes from the buffer being painted rather than from [`area`].
    let pane = buf.area;
    render(buf, pane, &view, theme, Glyphs::default(), &chrome);
}

/// The screen has to have been full, or a frame that drew two rows is a cheap
/// frame for a reason that is not the code.
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
const RAIL_PANE: Rect = Rect {
    x: 0,
    y: 0,
    width: 200,
    height: 40,
};

/// I9 beside a rail, where the pinned list draws several times the rows it does
/// on the pane every other gate here measures.
#[test]
fn a_frame_beside_a_rail_holds_the_frame_budget() {
    // The rail is asked for since §11.2 B14, and asked for the same way here as in the
    // timed loop below.
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
    // A gate over the gates, and this repo has paid twice for not having one.
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
    // Every tier-1 target has a cheap read, so this asserts unconditionally rather than
    // behind a `cfg`.
    assert!(
        chrome.memory.is_some(),
        "the timed frame read no memory, so the syscall the status bar performs \
         every frame is outside every budget in this file"
    );
}

/// Sizing a whole burst does not measurably change the frame it sits in.
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

    // Interleaved, because a sequential pair under varying load is not a
    // controlled experiment. This repo nearly filed a phantom regression that
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
    // Non-vacuity: both arms have to have done the work, or this compares two numbers
    // neither of which is a frame.
    let recorded = history.stats().recorded;
    assert!(
        recorded >= (HISTORY_PATHS * SAMPLED_BURSTS) as u64,
        "the store recorded {recorded} paths across {SAMPLED_BURSTS} rounds of          both arms, so at least one arm sized a burst the history never took"
    );
    assert!(
        plain > Duration::ZERO,
        "the unsized arm took no time, so this compared nothing"
    );
    // The delta against a fraction of the frame, not a ratio against the whole one.
    let delta = weighed.saturating_sub(plain);
    // An eighth was calibrated on one machine and the cost is host-dependent.
    let allowed = budget(I9_FRAME / 2);
    // No absolute claim here, and the attempt to add one is worth recording.
    assert!(
        delta <= allowed,
        "sizing a {HISTORY_PATHS}-path burst added {delta:?} to the frame          ({plain:?} to {weighed:?}) against {allowed:?}, which is a `stat` that          has become a read or a walk rather than a syscall on metadata the status          walk has already warmed"
    );
}

#[test]
fn the_memory_read_costs_a_fraction_of_the_frame_it_sits_in() {
    // The one *variable* cost the readouts add, and the reason the whole design turns
    // on it: `SPEC.md` §5.1 ships this cell precisely because the read is a syscall on
    // all three tier-1 targets rather than the process spawn `soak.rs` uses, which is
    // 42.8ms median on Windows against this 16ms budget.
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
    frame_budget_at_depth("shell-i9-deep", 500);
}

/// Sample the frame budget with the viewport `depth` rows into the diff.
fn frame_budget_at_depth(name: &str, depth: usize) {
    frame_budget_on(name, depth, area(), None, false, false);
}

/// The same, on a named pane.
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

    // Asked for before the layout is taken, because it changes the layout.
    if rail {
        let stacked = layout_of(&app, pane, FILES).diff;
        app.apply(vigia::Action::ToggleRail, &mut frame, stacked)
            .expect("toggle the rail");
    }

    // The pin, asked for the same way, and it moves no rows: `SPEC.md` §11.2
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

    // "Under continuous edits", taken literally and the same way the core's gate takes
    // it: one line is rewritten before every frame, so each frame revalidates
    // ninety-nine files, recomputes the one that moved, and re-highlights the one hunk
    // on screen.
    let mut edits = 0usize;
    // A cell rather than a `String`, because the sampler now outlives the reader.
    let marker = RefCell::new(String::new());
    let theme = Theme::default();
    let mut buf = Buffer::empty(pane);
    let mut next_frame =
        |frame: &mut Frame, app: &mut App, highlighter: &mut Highlighter, history: &mut History| {
            *marker.borrow_mut() = format!("fn edited_{edits}() {{ let value = {edits}; }}");
            scratch.edit_line(EDITED_PATH, 0, &marker.borrow());
            edits += 1;
            time_cpu(|| {
                // Inside the timed region on purpose.
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

    // And the edits have to be still landing.
    the_edits_still_land(&mut frame, EDITED_PATH, &marker.borrow());

    // The highlighter has to be re-parsing every frame, which is what says the edits
    // reach *it* and not merely the diff.
    assert_eq!(
        cost.parsed, SAMPLED_FRAMES as u64,
        "{} hunks were re-parsed across {SAMPLED_FRAMES} frames, so the visible \
         hunk is not changing under the highlighter and this is not the steady \
         state I9 describes",
        cost.parsed
    );

    // And the cost has to follow the screen rather than the hunk, at any depth.
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

    // And when a sheet was asked for, one has to have been on the frames that were
    // timed.
    if let Some(rung) = sheet {
        // Inside the sheet's own rect, not over the pane.
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
    // The gate whose absence was the finding.
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
    // A cell rather than a `String`, because the sampler now outlives the reader.
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

    // The non-vacuity that matters here, and it is not the usual one.
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
    // Reports, and deliberately does not assert a wall clock. Everything
    // structural here is asserted and exact; the clock is printed and left to
    // `SPEC.md` §10, and that is the ruling rather than a gap.
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

    // Frames after a rewrite are untimed, and a rewrite never lands inside a timed one.
    const CYCLES: usize = 9;
    const ABSORB: usize = 10;
    const PER_CYCLE: usize = 30;

    let mut in_margin = Samples::new(CYCLES * PER_CYCLE);
    let mut settled_frames = 0usize;
    let mut deferred_in_margin = 0u64;
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
            let was = frame.stats().deferred;
            let (cost, _) = time_cpu(|| {
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
            // Split on zero, not on `FILES`: a frame inside the margin keeps every
            // undrawn height waiting, and a settled one keeps none.
            match frame.stats().deferred - was {
                0 => settled_frames += 1,
                n => {
                    deferred_in_margin += n;
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

    // The premise, and it is a count rather than a clock.
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

    // And each of those frames re-measured nearly the whole changed set, not a handful
    // of it.
    let per_frame = deferred_in_margin / in_margin.len() as u64;
    let floor = (FILES - touchable) as u64;
    assert!(
        per_frame >= floor,
        "an in-margin frame kept {per_frame} heights waiting on average, under \
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
         timed ({settled_frames} settled), {} waiting, {} measured and {} stats \
         per frame across all {drove} driven",
        in_margin.percentile(0.50).expect("samples"),
        in_margin.max().expect("samples"),
        in_margin.len(),
        cost.deferred / drove as u64,
        cost.measured / drove as u64,
        cost.probes / drove as u64,
    );

    // Asserted rather than noted since the read waits for the margin: 22.2ms p99
    // with 94 files read a frame, 7.9ms with none, on the day the wait landed.
    holds_p99(
        &format!(
            "I9: a frame inside the settle margin over {FILES} files the screen does \
             not draw, every height waiting"
        ),
        budget(I9_FRAME),
        &in_margin,
        || {
            format!(
                "({} waiting, {} measured, {} stats per frame)",
                cost.deferred / drove as u64,
                cost.measured / drove as u64,
                cost.probes / drove as u64
            )
        },
        || {
            scratch.rewrite_all(FILES, LINES, CYCLES + 1);
            sample(&mut history, scratch.root(), EDITED_PATH);
            time_cpu(|| {
                shell_frame(
                    &mut frame,
                    &mut app,
                    &mut highlighter,
                    &history,
                    &mut buf,
                    &theme,
                    screen,
                );
            })
        },
    );
}

#[test]
fn the_frame_budget_holds_through_a_bulk_rewrite() {
    // The third position in this gate's input space, after "at the top" and "deep in a
    // hunk".
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
    let before = frame.stats();
    let highlighted = highlighter.stats();
    let mut frames = Samples::new(SAMPLED_FRAMES);
    let mut rewrites = 0;
    let mut rewritten_at = Instant::now();
    for at in 0..SAMPLED_FRAMES {
        if at.is_multiple_of(REWRITE_EVERY) || rewritten_at.elapsed() >= REWRITE_WITHIN {
            rewrites += 1;
            scratch.rewrite_all(FILES, LINES, rewrites);
            rewritten_at = Instant::now();
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

    // Non-vacuity, first in the direction this whole file exists for.
    assert!(
        parsed.lines > 0 && parsed.parsed >= SAMPLED_FRAMES as u64,
        "{} hunks were re-parsed over {} lines across {SAMPLED_FRAMES} frames, so \
         the visible hunk is not changing under the highlighter and this gate is \
         timing the core's frame path with the syntax parser missing",
        parsed.parsed,
        parsed.lines
    );

    // Non-vacuity.
    assert!(
        cost.computed >= SAMPLED_FRAMES as u64,
        "{} diffs were recomputed across {SAMPLED_FRAMES} frames, so frames were \
         reusing and this gate timed settled frames",
        cost.computed
    );

    // And the premise, checked rather than assumed: a file the viewport never drew is
    // still inside its margin now, so it was for the whole window, since settledness
    // only ever increases with time.
    let undrawn = FILES - 1;
    let probed = frame.stats();
    frame.diff(undrawn).expect("diff");
    frame.diff(undrawn).expect("diff");
    let probe = delta(probed, frame.stats());
    let since = rewritten_at.elapsed();
    assert_eq!(
        probe.reused, 0,
        "a file the viewport never drew was reusable {since:?} after the last of \
         {rewrites} rewrites, across {SAMPLED_FRAMES} frames, so the bulk rewrite \
         settled part-way through and the tail of this window was not the event. \
         A gap past {REWRITE_WITHIN:?} means the runner outran the rewrite cadence \
         rather than the shell doing anything wrong"
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

    // Which also settles what the probe above assumed.
    assert!(
        view.top.file + view.read <= undrawn,
        "the viewport drew files {}..{} of {FILES}, which reaches the file the \
         settle probe treats as never drawn",
        view.top.file,
        view.top.file + view.read
    );

    // The re-measure continues the sequence rather than restarting it, which matters
    // more here than on the steady-state gates: what this one measures is a frame
    // *inside the settle margin* after a bulk rewrite, so a second pass that only
    // edited one line would measure a cheaper condition and could mask a real breach.
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
            if at.is_multiple_of(REWRITE_EVERY) || rewritten_at.elapsed() >= REWRITE_WITHIN {
                rewrites += 1;
                scratch.rewrite_all(FILES, LINES, rewrites);
                rewritten_at = Instant::now();
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
const WIDE_FILES: usize = 20;
const WIDE_LINES: usize = 60;

/// The wide fixture at the scale the hundred-file gates use.
const WIDE_MANY_FILES: usize = 100;

/// Display rows one wide file contributes: every line removed and every line
/// added.
const WIDE_HUNK_ROWS: usize = WIDE_LINES * 2;

/// Where the upward scroll starts, in rows from the top of the diff.
const UP_FROM: usize = 1_000;

/// Files that have to sit above the viewport before an upward scroll is one.
const UP_FILES: usize = 4;

/// How the worktree is brought to a settled state before a scroll is timed.
#[derive(Clone, Copy)]
enum Prime {
    /// `settle`: the margin waited out and every file diffed. The steady
    /// state the wide gates are about, and the reason they could never see the
    /// walk.
    Materialised,
    /// `settle_spans`: the margin waited out, nothing diffed. What a reader has
    /// a second after launch.
    Launched,
}

/// One scroll run's setup.
#[derive(Clone, Copy)]
struct Scroll {
    motion: Motion,
    ext: &'static str,
    files: usize,
    /// Frames discarded before sampling. Zero for the gate that exists to
    /// contain the first frames rather than to begin after them.
    warmup: usize,
    prime: Prime,
}

impl Scroll {
    /// The shape every gate had before : twenty files, materialised, warmed.
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
    height: usize,
    painted: PaintStats,
    /// Frames the run drove, warmup included.
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
    Back,
}

impl Motion {
    fn step(self, at: usize) -> isize {
        match self {
            Motion::Down => WHEEL_ROWS,
            Motion::Up => -WHEEL_ROWS,
            Motion::Back if (at / LEG).is_multiple_of(2) => WHEEL_ROWS,
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
    // The direction `SPEC.md` §10 names as the worst case and which nothing had ever
    // run.
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
    // The third suspect, given a number instead of a ranking.
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
        // Resolved by the collect rather than by the scroll: `App` adds the rows to the
        // current file's offset and lets `View::collect` carry the overrun into the
        // files below, which is what keeps a scroll to one diff per file.
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

    // Their sum is what the budget is held against, so nothing is counted twice.
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
        // Accumulated, like the two above it.
        run.painted += painted;
        run.collect.push(collect);
        run.paint.push(paint);

        // The partition, and it is `SPEC.md` §7's carve-out rather than a convenience:
        // a frame that parses a hunk for the first time is on the cold path, which I9
        // excludes by definition.
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
const PROSE_FILES: usize = 10;
const PROSE_LINES: usize = 200;

/// A screenful of ordinary Markdown prose holds the frame budget.
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
    // At least one parsed line per frame, not one across the whole sample.
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
        // A whole run rather than a frame, and the difference is not cosmetic.
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
            // The edit is prose of the same shape, so the rewritten line costs what
            // every other line on screen costs.
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
const SAMPLED_AGEINGS: usize = 50;

#[test]
fn an_ageing_wake_costs_a_fraction_of_the_tick_it_is_not() {
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

    // A window at the path cap, driven on a clock that actually crosses a sample
    // boundary, and both halves are easy to get wrong.
    let paths: Vec<String> = (0..HISTORY_PATHS).map(|n| format!("src/f{n}.rs")).collect();
    let stamped = Instant::now();
    history.record_sized(
        paths.iter().map(|path| (path.as_str(), Some(4_000u64))),
        stamped,
    );

    let (mut ageing, mut ticking) = (Samples::new(SAMPLED_AGEINGS), Samples::new(SAMPLED_AGEINGS));
    for round in 1..=SAMPLED_AGEINGS {
        for walks in [false, true] {
            // One sample per arm, not per round.
            let step = u32::try_from(round * 2 + usize::from(walks)).expect("a round");
            let at = stamped + HISTORY_SAMPLE * step;
            let (wall, _) = time_cpu(|| {
                // The ageing arm is exactly what `Shell::draw` does on a wake that
                // changed nothing on disk: roll the window, then draw.
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

    // Non-vacuity on exactly that: every pass has to have crossed a boundary
    // and walked the projection, or neither arm timed an
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
const SHEET_PANE: Rect = Rect {
    x: 0,
    y: 0,
    width: 120,
    height: 22,
};

/// The size of the sheet `pane` draws, with the sheet up.
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
const ROOMY_PANE: Rect = Rect {
    x: 0,
    y: 0,
    width: 120,
    height: 41,
};

/// I9 with the roomy rung drawn over the frame.
#[test]
fn a_frame_under_the_roomy_sheet_holds_the_frame_budget() {
    assert_eq!(
        sheet_size_on("shell-i9-roomy-shape", ROOMY_PANE),
        (68, 37),
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
#[test]
fn a_pinned_frame_holds_the_frame_budget() {
    frame_budget_on("shell-i9-single", 0, area(), None, false, true);
}

/// What the staged run costs, in the frame it sits in rather than on its own.
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

    // Non-vacuity on what the run did rather than on what the fixture is. A
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
