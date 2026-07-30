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

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use vigia::{App, body_height};
use vigia_core::{Frame, Highlighter, LineKind, Samples};

use support::{Scratch, budget, highlight_delta, settle};

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

/// Frames discarded before sampling, and frames sampled.
///
/// The same numbers the core's gate uses, and for the same two reasons: I9 is a
/// claim about steady state so the cold path is out of scope by definition, and
/// a percentile needs enough samples to be one.
const WARMUP_FRAMES: usize = 50;
const SAMPLED_FRAMES: usize = 250;

/// An ordinary terminal.
fn area() -> Rect {
    Rect::new(0, 0, 80, 24)
}

fn body(app: &App, files: usize) -> usize {
    body_height(area(), &app.chrome("fixture"), files)
}

/// Whether the absolute wall-clock gate should assert.
///
/// A debug build is several times slower than the one the budgets were set
/// against. Reported rather than silently skipped.
fn absolute_gates_apply() -> bool {
    if cfg!(debug_assertions) {
        eprintln!(
            "note: the absolute budget gate is skipped in a debug build; \
             run `cargo test --release --test budgets` to enforce it"
        );
        false
    } else {
        true
    }
}

fn time(mut work: impl FnMut()) -> Duration {
    let start = Instant::now();
    work();
    start.elapsed()
}

#[test]
fn a_real_frame_with_highlighting_holds_the_frame_budget() {
    let scratch = Scratch::large_diff("shell-i9", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    assert_eq!(frame.files().len(), FILES, "fixture is not {FILES} files");

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let height = body(&app, FILES);

    if !absolute_gates_apply() {
        return;
    }

    // "Under continuous edits", taken literally and the same way the core's gate
    // takes it: one line is rewritten before every frame, so each frame
    // revalidates ninety-nine files, recomputes the one that moved, and
    // re-highlights the one hunk on screen. The edit stands in for the agent in
    // the other pane and is deliberately outside the timed region.
    let mut edits = 0usize;
    let mut marker = String::new();
    let mut next_frame = |frame: &mut Frame, app: &mut App, highlighter: &mut Highlighter| {
        marker = format!("fn edited_{edits}() {{ let value = {edits}; }}");
        scratch.edit_line(EDITED_PATH, 0, &marker);
        edits += 1;
        time(|| {
            frame.advance().expect("advance");
            app.view(frame, highlighter, height).expect("view");
        })
    };

    for _ in 0..WARMUP_FRAMES {
        next_frame(&mut frame, &mut app, &mut highlighter);
    }

    let before = highlighter.stats();
    let mut frames = Samples::new(SAMPLED_FRAMES);
    for _ in 0..SAMPLED_FRAMES {
        frames.push(next_frame(&mut frame, &mut app, &mut highlighter));
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
    let view = app
        .view(&mut frame, &mut highlighter, height)
        .expect("view");
    assert_eq!(
        view.rows.len(),
        height,
        "the body drew {} of {height} rows, so the frames above were not full \
         screens",
        view.rows.len()
    );

    // And the edits have to be still landing. Checked against the frame's diff
    // rather than against the screen, and the difference is the fixture: this
    // one rewrites every line, so a file's hunk is five hundred removals
    // followed by five hundred additions and the newest line sits far below any
    // viewport. A screen assertion here would fail while the code was perfect.
    let last = marker.clone();
    let shared = frame
        .files()
        .iter()
        .position(|change| change.path == EDITED_PATH)
        .expect("the edited file is still a change");
    let (_, diff) = frame.diff(shared).expect("diff");
    assert!(
        diff.hunks.iter().any(|hunk| hunk
            .lines
            .iter()
            .any(|line| line.kind == LineKind::Added && line.text == last)),
        "the diff for {EDITED_PATH} does not contain {last:?}, so the edits \
         stopped reaching it"
    );

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

    // And the cost has to follow the screen. A frame that re-highlighted the
    // whole hunk would parse a thousand lines rather than a screenful, which is
    // the 61ms-per-frame shape `vigia_core::highlight` exists to avoid.
    let per_frame = cost.lines / SAMPLED_FRAMES as u64;
    assert!(
        per_frame <= height as u64,
        "{per_frame} lines were highlighted per frame for a {height}-row body, \
         so a frame is parsing more of the hunk than it draws"
    );

    let p99 = frames.percentile(0.99).expect("samples");
    assert!(
        p99 <= budget(I9_FRAME),
        "I9: a real frame with highlighting over {FILES} files was {p99:?} p99, \
         over the {:?} budget (p50 {:?}, max {:?}, {} hunks parsed, {} reused, \
         {} lines, {} bytes)",
        budget(I9_FRAME),
        frames.percentile(0.50).expect("samples"),
        frames.max().expect("samples"),
        cost.parsed,
        cost.reused,
        cost.lines,
        cost.bytes
    );
}
