//! I7, held against the shell rather than against the core.
//!
//! > Startup to first paint is imperceptible. **< 50ms**
//!
//! `crates/vigia-core/tests/budgets.rs` already gates this, and that gate is
//! structurally blind to most of what a first paint costs: it opens the
//! repository, takes the first change and diffs it, and builds no
//! [`Highlighter`] at all. The shipped first paint does build one, and `syntect`
//! compiles a grammar's `fancy_regex` patterns lazily on first use.
//!
//! Measured on the reference machine, release, before this gate existed: a first
//! `App::view` over **two files of two hundred lines** cost **92.62ms** while
//! parsing seventeen lines, and frames two through four cost ~270µs. The same
//! fixtures under an extension `syntect` has no grammar for parsed nothing and
//! showed no step at all. So the cost follows neither the diff nor the lines
//! drawn; it is one compile, and `SPEC.md` §10's 20.37ms never contained it.
//!
//! It is the same shape as `budgets.rs` and `reads.rs`: an invariant the engine
//! can only make *possible* gets a second gate over the caller (`SPEC.md` §7).
//! The axis is new, though, and it is the one this repo keeps rediscovering.
//! §7 already records that a budget measured at one *position* is measured at its
//! cheapest, and that a gate which *settles* first has measured the cheapest
//! state. This is the third: **every wall-clock gate here discards fifty warmup
//! frames, which is correct for a steady-state budget and is exactly where a
//! first-paint cost hides.**

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{App, Body, Theme, body_layout, render};
use vigia_core::{Highlighter, History, Worktree};

use support::{Scratch, budget, exclusively_timed, highlight_delta};

/// I7: startup to first paint.
const I7_STARTUP: Duration = Duration::from_millis(50);

/// The fixture the I4 and I9 gates use, so the numbers are comparable.
const FILES: usize = 100;
const LINES: usize = 500;

/// An ordinary terminal.
fn area() -> Rect {
    Rect::new(0, 0, 80, 24)
}

/// Whether the absolute wall-clock gate should assert.
fn absolute_gates_apply() -> bool {
    if cfg!(debug_assertions) {
        eprintln!(
            "note: the absolute budget gate is skipped in a debug build; \
             run `cargo test --release --test first_paint` to enforce it"
        );
        false
    } else {
        true
    }
}

/// What one cold start cost, and what it drew.
struct FirstPaint {
    /// Discover, advance, build a highlighter, collect and paint.
    first: Duration,
    /// The frame after it, which is the one that pays for the compile.
    second: Duration,
    /// Rows the first frame drew, against the rows its body had.
    rows: usize,
    height: usize,
    /// Lines the first frame parsed, and the second.
    parsed_first: u64,
    parsed_second: u64,
}

/// One cold start, staged exactly as `vigia::run` stages it.
///
/// **A fresh [`Highlighter`] every time, which is what makes this repeatable.**
/// The compile is cached on the `SyntaxSet` a highlighter owns, so a run reusing
/// one would measure the first start and then nothing at all.
///
/// `Session::enter` and the terminal size query are left out, which is the same
/// carve-out `budgets.rs` and `soak.rs` already name: they need a tty. Note that
/// the real `run` takes the alternate screen *before* this work, so what a reader
/// looks at while it happens is a blank screen.
fn cold_start(root: &std::path::Path) -> FirstPaint {
    let began = Instant::now();
    let worktree = Worktree::discover(root).expect("discover");
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::new();
    let mut app = App::new();
    let history = History::new();

    let chrome = app.chrome("fixture", None);
    let body: Body = body_layout(area(), &chrome, frame.files().len());
    let theme = Theme::default();
    let mut buf = Buffer::empty(area());

    let before = highlighter.stats();
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    render(&mut buf, area(), &view, &theme, &chrome);
    let first = began.elapsed();
    let parsed_first = highlight_delta(before, highlighter.stats()).lines;
    let rows = view.rows.len();

    // The frame after it, timed separately. This is where the compile lands once
    // the first frame stops paying for it, and it is reported rather than gated:
    // `SPEC.md` §7 puts a first parse on the cold path that I9 excludes by
    // definition. Reporting it is what stops it going unmeasured again.
    let before = highlighter.stats();
    let began = Instant::now();
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    let chrome = app.chrome("fixture", None);
    render(&mut buf, area(), &view, &theme, &chrome);
    let second = began.elapsed();
    let parsed_second = highlight_delta(before, highlighter.stats()).lines;

    FirstPaint {
        first,
        second,
        rows,
        height: body.diff,
        parsed_first,
        parsed_second,
    }
}

#[test]
fn the_shells_first_paint_holds_the_startup_budget() {
    let scratch = Scratch::large_diff("shell-i7", FILES, LINES);
    let root = scratch.path_of(".");

    // Structural first, so a debug build still checks the shape even though it
    // cannot check the clock.
    let run = cold_start(&root);

    // Non-vacuity, in the direction that matters most here: a frame is not
    // allowed to be fast because it drew less. The whole risk of a plain first
    // paint is that it quietly becomes a *smaller* paint, and then this gate
    // passes while the reader looks at an empty pane.
    assert_eq!(
        run.rows, run.height,
        "the first paint drew {} of {} body rows, so it is fast because it drew \
         less rather than because it deferred the parse",
        run.rows, run.height
    );

    // And it really is the plain frame rather than a lucky one.
    assert_eq!(
        run.parsed_first, 0,
        "the first paint parsed {} lines, so it is still paying for the grammar \
         compile that I7 cannot afford",
        run.parsed_first
    );

    // And the product can still colour at all. Without this the whole gate is
    // satisfied by a build whose highlighter never runs, which is the exact
    // failure `SPEC.md` §7 names one invariant over.
    assert!(
        run.parsed_second > 0,
        "the second frame parsed nothing either, so this gate is timing a shell \
         that never highlights rather than one that defers"
    );

    if !absolute_gates_apply() {
        return;
    }
    let _timed = exclusively_timed();

    // Best of three, the idiom `crates/vigia-core/tests/budgets.rs` uses for its
    // absolute tier: what the code can do rather than what the machine happened
    // to be doing. Each run is a fresh `SyntaxSet`, or the second would measure a
    // compile that has already happened.
    let mut runs = Vec::new();
    for _ in 0..3 {
        runs.push(cold_start(&root));
    }
    let best = runs
        .iter()
        .min_by_key(|run| run.first)
        .expect("at least one run");

    eprintln!(
        "note: first paint {:?} (then {:?} for the frame that compiles the \
         grammar), over {FILES} files x {LINES} lines, {} rows drawn, {} lines \
         parsed on the first frame and {} on the second",
        best.first, best.second, best.rows, best.parsed_first, best.parsed_second
    );

    assert!(
        best.first <= budget(I7_STARTUP),
        "I7: the shell's first paint took {:?}, over the {:?} budget. The frame \
         after it took {:?}, which is where the grammar compile lands",
        best.first,
        budget(I7_STARTUP),
        best.second
    );
}
