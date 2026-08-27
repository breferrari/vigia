//! I7, held against the shell rather than against the core.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{App, Body, Glyphs, Pointing, Row, Theme, body_layout, render};
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
    plain: Vec<Row>,
    coloured: Vec<Row>,
    /// Lines the first frame parsed, and the second.
    parsed_first: u64,
    parsed_second: u64,
}

/// One cold start, staged as `vigia::run` stages it.
fn cold_start(root: &std::path::Path) -> FirstPaint {
    let began = Instant::now();
    let worktree = Worktree::discover(root).expect("discover");
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::eager();
    let mut app = App::new();
    let history = History::new();

    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(area(), &chrome, frame.files().len(), frame.files().len());
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

    // The frame after it, timed separately, and it is an eager highlighter's second
    // frame rather than the product's.
    let before = highlighter.stats();
    let began = Instant::now();
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("view");
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
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
                emph: Vec::new(),
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

    // Non-vacuity, in the direction that matters most here: a frame is not allowed to
    // be fast because it drew less.
    assert_eq!(
        run.plain.len(),
        run.height,
        "the first paint drew {} of {} body rows, so it is fast because it drew \
         less rather than because it deferred the parse",
        run.plain.len(),
        run.height
    );

    // The assertion the row count cannot make. Deferring colour has to
    // change colour and nothing else, so the two frames must draw the same rows
    // once spans are taken out of the comparison.
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
    // absolute tier: what the code can do rather than what the machine happened to be
    // doing.
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

/// The opening the shipped shell actually has, which the gate above
/// deliberately does not measure.
#[test]
fn the_opening_frames_never_compile_a_grammar_the_warmer_has_not_reached() {
    let scratch = Scratch::large_diff("first-paint-deferred", 4, 40);
    let worktree = Worktree::discover(scratch.root()).expect("discover");
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    // The shipped constructor, not the affordance every other gate here uses.
    let mut highlighter = Highlighter::new();
    let mut app = App::new();
    let history = History::new();
    let theme = Theme::default();
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let body = body_layout(area(), &chrome, frame.files().len(), frame.files().len());
    let mut buf = Buffer::empty(area());

    let mut draw = |app: &mut App, highlighter: &mut Highlighter| {
        let before = highlighter.stats();
        let view = app
            .view(&mut frame, highlighter, &history, body)
            .expect("view");
        render(&mut buf, area(), &view, &theme, Glyphs::default(), &chrome);
        highlight_delta(before, highlighter.stats()).lines
    };

    let first = draw(&mut app, &mut highlighter);
    let second = draw(&mut app, &mut highlighter);

    // Asserted apart, because only one of them is about this change.
    assert_eq!(
        first, 0,
        "the first frame parsed {first} lines, so I7's opening rule is broken \
         before this gate's own subject is even reached"
    );
    // The one the deferral owns.
    assert_eq!(
        second, 0,
        "the second frame parsed {second} lines under a grammar nothing has \
         compiled, so the reader waits on the 74-362ms it costs on a frame they \
         did not ask for"
    );
    assert!(
        !highlighter.wanted().is_empty(),
        "neither opening frame asked for a warm, so the diff on screen would \
         stay plain until the agent in the other pane happened to write again, \
         which on a tree nobody is touching is never"
    );

    // The warm the shell would spawn, served the way the shell serves it.
    let wanted = highlighter.wanted().to_vec();
    highlighter
        .warm_ahead(scratch.root().to_path_buf(), wanted, None)
        .join()
        .expect("the warmer thread");

    assert!(
        draw(&mut app, &mut highlighter) > 0,
        "the frame after the warm still parsed nothing, so the deferral is a \
         screen that never gains its colour rather than one that gains it late"
    );
}
