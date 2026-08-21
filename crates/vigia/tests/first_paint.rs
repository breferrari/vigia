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
//! state. This is the third: **every steady-state wall-clock gate here discards
//! fifty warmup frames, which is correct for such a budget and is exactly where a
//! first-paint cost hides.**

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{App, Body, Glyphs, Row, Theme, body_layout, render};
use vigia_core::{Highlighter, History, Worktree};

use support::{Scratch, absolute_gates_apply, budget, exclusively_timed, highlight_delta};

/// I7: startup to first paint.
const I7_STARTUP: Duration = Duration::from_millis(50);

/// The fixture the I4 and I9 gates use, so the numbers are comparable.
const FILES: usize = 100;
const LINES: usize = 500;

/// An ordinary terminal.
fn area() -> Rect {
    Rect::new(0, 0, 80, 24)
}

/// What one cold start cost, and what it drew.
struct FirstPaint {
    /// Discover, advance, build a highlighter, collect and paint.
    first: Duration,
    /// The frame after it, which is the one that pays for the compile.
    second: Duration,
    /// Rows the first frame's body had, which is not recoverable from the rows
    /// themselves: it is `Body::diff`, and a frame that drew too few is exactly
    /// what this gate must not accept.
    height: usize,
    /// The two frames' rows with every span stripped.
    ///
    /// The gate's real subject. Deferring colour must change **colour**, and a
    /// row count alone cannot say that: mutating the plain frame to stop drawing
    /// content lines left `rows == height` satisfied, because the walk simply
    /// reached further down the file list and drew more headings instead. Found
    /// by mutation, which is why this field exists.
    plain: Vec<Row>,
    coloured: Vec<Row>,
    /// Lines the first frame parsed, and the second.
    parsed_first: u64,
    parsed_second: u64,
}

/// One cold start, staged as `vigia::run` stages it.
///
/// **Not identically.** `run` also resolves the colour depth and the theme and
/// shortens the worktree name before its first paint, none of which this
/// includes; and `run`'s single `Shell::draw` paints *twice*, where this times
/// the two separately so the second can be reported rather than gated. What is
/// measured is time to the screen the reader first sees, which is what I7 is
/// about.
///
/// **One omission is in the expensive direction and is deliberate: the warmer.**
/// `run` spawns `Highlighter::warm_ahead` just before its draw, so on the real
/// path a thread is compiling grammars while this window is open. Including it
/// here would be worse rather than better, because the best-of-three below
/// would put *three* detached warmers into one measurement where the product
/// has one, and a gate whose noise floor is its own harness cannot see the
/// code. Measured at nil on the reference machine (14.85ms against 14.78ms
/// median over six alternating pairs), and the residual risk is a two-core
/// runner, where `VIGIA_BUDGET_SLACK` is the lever `SPEC.md` §7 already
/// provides.
///
/// **A fresh [`Highlighter`] every time, which is what makes this repeatable.**
/// The compile is cached on the `SyntaxSet` a highlighter owns, so a run reusing
/// one would measure the first start and then nothing at all.
///
/// `Session::enter` and the terminal size query are left out, which is the same
/// carve-out `budgets.rs` and `soak.rs` already name: they need a tty. Note that
/// the real `run` takes the alternate screen *before* this work, so what a reader
/// looks at while it happens is a blank screen.
///
/// **`signal::forward` is left out too, and it is the one carve-out here that
/// does not need a tty.** `run` arms it *before* `Session::enter`, so the safety
/// net covers the takeover itself rather than starting after its fourth step, and
/// a source-scanning gate in `lib.rs` is what holds that order. Either side of the
/// takeover it is on the startup path and not measured here.
///
/// Measured separately rather than assumed: **84.7µs** on the reference machine
/// for the whole arming, dominated by a `thread::spawn` Windows does not perform,
/// against a 50ms budget. Named because this slot is where the next thing armed
/// before first paint would land unmeasured, and a carve-out nobody wrote down is
/// the one that grows.
fn cold_start(root: &std::path::Path) -> FirstPaint {
    let began = Instant::now();
    let worktree = Worktree::discover(root).expect("discover");
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::eager();
    let mut app = App::new();
    let history = History::new();

    let chrome = app.chrome("fixture", None, None, None, None, None);
    let body = body_layout(area(), &chrome, frame.files().len());
    let theme = Theme::default();
    let mut buf = Buffer::empty(area());

    let before = highlighter.stats();
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    render(&mut buf, area(), &view, &theme, Glyphs::default(), &chrome);
    let first = began.elapsed();
    let parsed_first = highlight_delta(before, highlighter.stats()).lines;
    let plain = stripped(&view.rows);

    // The frame after it, timed separately. This is where the compile lands once
    // the first frame stops paying for it, and it is reported rather than gated:
    // `SPEC.md` §7 puts a first parse on the cold path that I9 excludes by
    // definition. Reporting it is what stops it going unmeasured again.
    let before = highlighter.stats();
    let began = Instant::now();
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    let chrome = app.chrome("fixture", None, None, None, None, None);
    render(&mut buf, area(), &view, &theme, Glyphs::default(), &chrome);
    let second = began.elapsed();
    let parsed_second = highlight_delta(before, highlighter.stats()).lines;
    let coloured = stripped(&view.rows);

    FirstPaint {
        first,
        second,
        height: body.diff,
        plain,
        coloured,
        parsed_first,
        parsed_second,
    }
}

/// The same rows with every span removed, so two frames can be compared on
/// everything except the one thing that is meant to differ.
fn stripped(rows: &[Row]) -> Vec<Row> {
    rows.iter()
        .map(|row| match row {
            Row::Line {
                kind, number, text, ..
            } => Row::Line {
                kind: *kind,
                number: *number,
                text: text.clone(),
                spans: Vec::new(),
            },
            other => other.clone(),
        })
        .collect()
}

#[test]
fn the_shells_first_paint_holds_the_startup_budget() {
    let scratch = Scratch::large_diff("shell-i7", FILES, LINES);

    // Structural first, so a debug build still checks the shape even though it
    // cannot check the clock.
    let run = cold_start(scratch.root());

    // Non-vacuity, in the direction that matters most here: a frame is not
    // allowed to be fast because it drew less. The whole risk of a plain first
    // paint is that it quietly becomes a *smaller* paint, and then this gate
    // passes while the reader looks at an empty pane.
    assert_eq!(
        run.plain.len(),
        run.height,
        "the first paint drew {} of {} body rows, so it is fast because it drew \
         less rather than because it deferred the parse",
        run.plain.len(),
        run.height
    );

    // **The assertion the row count cannot make.** Deferring colour has to
    // change colour and nothing else, so the two frames must draw the same rows
    // once spans are taken out of the comparison.
    //
    // **This holds at the `Row` level, which is what it compares, and it is one
    // level short of what a reader sees.** The paint walks a budget per *span*
    // rather than per row, so on decomposed Unicode the plain row and the
    // coloured row can fill a cell differently from identical `Row`s. That is
    // [#106](https://github.com/breferrari/vigia/issues/106), it is pre-existing
    // and independent of highlighting, and comparing rendered cells here is its
    // acceptance rather than this gate's. Named so the gap is a decision rather
    // than an oversight. A row count alone is satisfied
    // by a plain frame that drew different content: mutating `View::collect` to
    // stop pushing content lines left `rows == height` green, because the walk
    // reached further down the file list and drew more headings instead.
    assert_eq!(
        run.plain, run.coloured,
        "the plain first frame and the coloured second one drew different rows, \
         so deferring the parse is changing what the reader sees rather than \
         only when it is coloured"
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

    if !absolute_gates_apply("cargo test --release -p vigia --test first_paint") {
        return;
    }
    let _timed = exclusively_timed();

    // Best of three, the idiom `crates/vigia-core/tests/budgets.rs` uses for its
    // absolute tier: what the code can do rather than what the machine happened
    // to be doing. Each run is a fresh `SyntaxSet`, or the second would measure a
    // compile that has already happened.
    let mut runs = Vec::new();
    for _ in 0..3 {
        runs.push(cold_start(scratch.root()));
    }
    let best = runs
        .iter()
        .min_by_key(|run| run.first)
        .expect("at least one run");

    eprintln!(
        "note: first paint {:?} (then {:?} for the frame that compiles the \
         grammar), over {FILES} files x {LINES} lines, {} rows drawn, {} lines \
         parsed on the first frame and {} on the second",
        best.first,
        best.second,
        best.plain.len(),
        best.parsed_first,
        best.parsed_second
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

/// The opening two frames, as the state machine they are.
///
/// **What this can and cannot reach.** [`App`]'s half is here: which frame
/// parses, and that the plain one leaves a debt exactly once. The other half
/// is `Shell::draw`'s `while self.app.owes_repaint()` loop, and that is not
/// reachable from any test — `Shell` owns a `Session`, which takes the
/// alternate screen and needs a tty, the same carve-out `budgets.rs` and
/// `soak.rs` already name.
///
/// So the loop is held by construction rather than by a gate: it is one
/// expression over a public predicate, where before it was two `draw`
/// statements in a row whose pairing nothing recorded. Deleting the second
/// of those left the entire suite green while the product sat on a
/// permanently uncoloured screen, which is what this state machine exists to
/// make un-deletable. Naming the gap rather than implying coverage.
#[test]
fn the_first_frame_is_plain_and_owes_exactly_one_repaint() {
    let scratch = Scratch::large_diff("app-paint", 2, 8);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let body = Body::diff_only(8);

    let mut app = App::new();
    assert!(
        !app.owes_repaint(),
        "a shell that has drawn nothing is owed nothing; the debt is for a \
         plain frame that is already on screen"
    );

    // The plain one, which is I7's whole fix.
    let before = highlighter.stats();
    app.view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert_eq!(
        highlight_delta(before, highlighter.stats()).lines,
        0,
        "the first frame parsed, so the grammar compile is back on the frame \
         I7 gives fifty milliseconds to"
    );
    assert!(
        app.owes_repaint(),
        "the plain frame left no debt, so nothing makes the shell draw the \
         coloured one and the pane stays grey until something else wakes it"
    );

    // And the one that settles it.
    let before = highlighter.stats();
    app.view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert!(
        highlight_delta(before, highlighter.stats()).lines > 0,
        "the frame that settles the debt parsed nothing, so the shell never \
         colours at all"
    );
    assert!(
        !app.owes_repaint(),
        "the debt survived the frame that paid it, so `Shell::draw` would \
         loop forever"
    );

    // One direction only. A later frame must not re-owe, or a monitor would
    // draw every screen twice for the life of the process.
    app.view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    assert!(!app.owes_repaint());
}

#[test]
fn a_shell_past_its_first_paint_owes_nothing_and_colours_at_once() {
    // The test affordance, asserted rather than assumed: three gates depend
    // on it starting coloured, and if it ever stopped doing that they would
    // silently measure the plain frame instead.
    let scratch = Scratch::large_diff("app-paint-past", 2, 8);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let mut app = App::past_first_paint();
    assert!(!app.owes_repaint());
    let before = highlighter.stats();
    app.view(&mut frame, &mut highlighter, &history, Body::diff_only(8))
        .expect("view");
    assert!(
        highlight_delta(before, highlighter.stats()).lines > 0,
        "`past_first_paint`'s first frame drew plain, so every gate using it \
         to measure a coloured screen is measuring the opposite"
    );
    assert!(!app.owes_repaint());
}
