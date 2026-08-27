//! I5: correct with zero interaction.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use vigia::{Action, App, Body, Glyphs, Pointing, Position, Row, Theme, body_layout, render};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, delta};

/// Files in the fixture, enough that following has somewhere to go.
const FILES: usize = 40;

/// A file far enough down the list that landing on it cannot be a coincidence.
const TARGET: usize = 7;

/// A second one, so "moved once" and "moves whenever asked" are different
/// assertions.
const OTHER: usize = 21;

/// The shipped split, list included, for the gates that assert about a whole
/// screen rather than about the diff walk under it.
fn layout() -> Body {
    body_layout(
        Rect::new(0, 0, 80, 24),
        &App::new().chrome("fixture", None, Pointing::default(), 0, ""),
        FILES,
        FILES,
    )
}

fn body() -> usize {
    layout().diff
}

fn fixture(name: &str) -> Scratch {
    Scratch::large_diff(name, FILES, 1)
}

/// The path status reports at `index`.
fn path_at(frame: &Frame, index: usize) -> String {
    frame.files()[index].path.clone()
}

/// The file whose heading is drawn at the top of the screen.
fn top_file(
    app: &mut App,
    frame: &mut Frame,
    highlighter: &mut Highlighter,
    history: &History,
) -> String {
    let view = app
        .view(frame, highlighter, history, layout())
        .expect("view");
    match view.rows.first() {
        Some(Row::File(entry)) => entry.path.clone(),
        other => panic!("the top row is {other:?}, not a file heading"),
    }
}

#[test]
fn follow_is_engaged_before_anything_is_touched() {
    // I5 in one line. A monitor that needs a keypress to start showing the
    // current state is not a monitor, so this is the default rather than a
    // setting, and `App` deliberately does not get it from `Default`.
    assert!(
        App::new().following(),
        "a shell starts disengaged, so it shows nothing new until asked"
    );
}

#[test]
fn a_change_moves_the_view_to_the_changed_file_with_no_input_at_all() {
    let scratch = fixture("shell-follow-moves");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let target = path_at(&frame, TARGET);
    let first = path_at(&frame, 0);
    // Non-vacuity, and it is the whole test's premise: the view has to start
    // somewhere other than the file it is about to be moved to, or "it moved"
    // and "it never moved" look identical.
    assert_ne!(
        target, first,
        "the fixture put the follow target where the view already was"
    );
    assert_eq!(
        top_file(&mut app, &mut frame, &mut highlighter, &history),
        first
    );

    app.follow(&target, &frame);

    assert_eq!(
        top_file(&mut app, &mut frame, &mut highlighter, &history),
        target,
        "the view did not move to the file that changed"
    );
    assert_eq!(
        app.position().row,
        0,
        "the view moved to the right file but not to the top of it, so the \
         heading of what just changed is scrolled off"
    );

    // A request answered with "keep the heading" is still answered, and saying
    // otherwise is not harmless: the caller clears the debt on this, so a frame that
    // resolved to row zero and reported nothing leaves a request armed to fire on the
    // next resize.
    app.follow(&target, &frame);
    let view = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("view");
    assert!(
        view.landed,
        "the frame kept the heading and reported no landing, so the request \
         outlives the frame that served it"
    );
}

#[test]
fn a_scripted_edit_sequence_draws_the_file_that_changed_last() {
    // I5's proof exactly as `SPEC.md` §3 words it: a scripted edit sequence, snapshot
    // the frame, no input given.
    let scratch = Scratch::new("shell-follow-scripted");
    scratch.write("README.md", "docs\n");
    scratch.write("src/first.rs", "fn first() {}\n");
    scratch.write("src/second.rs", "fn second() {}\n");
    scratch.commit_all("baseline");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // The script. Each step is an edit and the tick it produces, in the order
    // an agent would make them, and nothing else: no key is pressed anywhere
    // in this test.
    for (path, contents) in [
        ("README.md", "docs, revised\n"),
        ("src/first.rs", "fn first() { let a = 1; }\n"),
        ("src/second.rs", "fn second() { let b = 2; }\n"),
    ] {
        scratch.write(path, contents);
        frame.advance().expect("advance");
        app.follow(path, &frame);
    }

    let area = Rect::new(0, 0, 64, 12);
    let height = body_layout(
        area,
        &app.chrome("fixture", None, Pointing::default(), 0, ""),
        frame.files().len(),
        frame.files().len(),
    );
    let view = app
        .view(&mut frame, &mut highlighter, &history, height)
        .expect("view");
    assert_eq!(view.files, 3, "the fixture is not three changed files");

    let theme = Theme::default();
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let mut terminal = Terminal::new(TestBackend::new(64, 12)).expect("terminal");
    terminal
        .draw(|f| {
            let area = f.area();
            render(
                f.buffer_mut(),
                area,
                &view,
                &theme,
                Glyphs::default(),
                &chrome,
            );
        })
        .expect("draw");
    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn scrolling_disengages_follow_and_the_next_change_does_not_move_the_view() {
    let scratch = fixture("shell-follow-scroll-disengages");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let target = path_at(&frame, TARGET);
    let other = path_at(&frame, OTHER);
    app.follow(&target, &frame);
    assert_eq!(
        top_file(&mut app, &mut frame, &mut highlighter, &history),
        target
    );

    app.apply(Action::Scroll(1), &mut frame, body())
        .expect("scroll");
    assert!(
        !app.following(),
        "a manual scroll left follow mode engaged, so the next write yanks the \
         reader off whatever they had scrolled to"
    );

    let parked = app.position();
    app.follow(&other, &frame);
    assert_eq!(
        app.position(),
        parked,
        "a change moved the view although the reader had scrolled away"
    );
}

#[test]
fn f_re_engages_follow_and_jumps_to_the_newest_change() {
    // The half of B1 that is easy to get half right.
    let scratch = fixture("shell-follow-toggle");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    app.apply(Action::Scroll(1), &mut frame, body())
        .expect("scroll");
    assert!(!app.following());

    // Arrives while disengaged, so it is recorded and not acted on. That is
    // what gives `f` somewhere to jump to a moment later.
    let other = path_at(&frame, OTHER);
    let parked = app.position();
    app.follow(&other, &frame);
    assert_eq!(
        app.position(),
        parked,
        "the view followed a change while disengaged"
    );

    app.apply(Action::ToggleFollow, &mut frame, body())
        .expect("toggle");

    assert!(app.following(), "`f` did not re-engage follow mode");
    assert_eq!(
        top_file(&mut app, &mut frame, &mut highlighter, &history),
        other,
        "`f` re-armed the view without moving it, so the reader has to wait for \
         another write to see the change they pressed it for"
    );

    // And it is a toggle rather than a latch.
    app.apply(Action::ToggleFollow, &mut frame, body())
        .expect("toggle");
    assert!(!app.following(), "`f` a second time did not disengage");
}

#[test]
fn a_resize_does_not_disengage_follow() {
    // A pane beside an agent is resized constantly. A resize moves no viewport
    // and expresses no intent, so treating it as a manual scroll would switch
    // I5 off for free and nobody would connect the two.
    let scratch = fixture("shell-follow-resize");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    app.apply(Action::Redraw, &mut frame, body())
        .expect("redraw");
    assert!(app.following(), "a resize disengaged follow mode");

    let target = path_at(&frame, TARGET);
    app.follow(&target, &frame);
    assert_eq!(
        top_file(&mut app, &mut frame, &mut highlighter, &history),
        target,
        "the view stopped following after a resize"
    );
}

#[test]
fn jumping_to_the_last_file_disengages_rather_than_re_engaging() {
    // B1's rationale as a test.
    let scratch = fixture("shell-follow-ends");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    for action in [Action::Bottom, Action::Top] {
        let mut app = App::new();
        assert!(app.following());
        app.apply(action, &mut frame, body()).expect("apply");
        assert!(
            !app.following(),
            "{action:?} left follow mode engaged, so it doubles as a re-arm"
        );
    }
}

#[test]
fn a_file_step_disengages_follow_at_both_ends_of_the_diff() {
    // `n` moves the diff, so it disengages for the reason every other jump does.
    let scratch = fixture("shell-follow-file-step");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    for action in [Action::File(1), Action::File(-1)] {
        let mut app = App::new();
        assert!(app.following());
        assert_eq!(app.position(), Position { file: 0, row: 0 });
        app.apply(action, &mut frame, body()).expect("apply");
        assert!(
            !app.following(),
            "{action:?} left follow mode engaged, so a reader who asked to move \
             gets dragged back on the next write"
        );
    }
}

#[test]
fn a_change_to_a_file_that_is_not_in_the_diff_leaves_the_view_where_it_was() {
    // Ordinary rather than exceptional: an edit reverted before the tick landed, or a
    // file written back to the bytes the index already holds.
    let scratch = fixture("shell-follow-unknown");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();

    let target = path_at(&frame, TARGET);
    app.follow(&target, &frame);
    let settled = app.position();

    let moved = app.follow("src/never_changed.rs", &frame);

    assert!(
        !moved,
        "following a path with no change in it reported a move"
    );
    assert_eq!(
        app.position(),
        settled,
        "an unfollowable path moved the view somewhere arbitrary"
    );
    assert!(
        app.following(),
        "a path that could not be followed switched follow mode off, so one \
         reverted edit stops the monitor being a monitor"
    );
}

#[test]
fn following_a_file_costs_no_diff_and_no_read() {
    // I4 and I2a, held over the path I5 added.
    let scratch = fixture("shell-follow-cost");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();

    // Deliberately measured on an unsettled frame. Waiting out the settle
    // margin first would be the friendlier reading, and it would also mean a
    // `follow` that did read something could hide behind reuse.
    let target = path_at(&frame, TARGET);
    let before = frame.stats();
    let moved = app.follow(&target, &frame);
    let cost = delta(before, frame.stats());

    // Non-vacuity: zero cost is trivially true of a call that did nothing.
    assert!(
        moved,
        "the view did not move, so this measured an early return"
    );
    assert_eq!(
        cost,
        vigia_core::FrameStats::default(),
        "following a file cost {cost:?}, and it is supposed to be a lookup in a \
         list the frame already holds"
    );
}

#[test]
fn a_position_survives_the_file_it_points_at_being_committed() {
    // Follow mode writes a raw index into the position, and the agent in the other pane
    // can shorten the list underneath it.
    let scratch = fixture("shell-follow-shrink");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let last = path_at(&frame, FILES - 1);
    app.follow(&last, &frame);
    assert_eq!(
        top_file(&mut app, &mut frame, &mut highlighter, &history),
        last
    );

    scratch.commit_all("the agent in the other pane commits");
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), 0, "the commit left changes behind");

    let view = app
        .view(&mut frame, &mut highlighter, &history, layout())
        .expect("a shrunken list must not panic");
    assert!(view.rows.is_empty(), "a clean worktree drew rows");
    assert_eq!(
        app.position(),
        Position::default(),
        "the position was left pointing past the end of an empty list"
    );
}

/// A file whose diff is several screens tall, with its largest change low down.
fn tall(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(TALL, support::numbered_lines(TALL_LINES));
    scratch.commit_all("baseline");

    // Split from the same helper the baseline was written with, rather than re-spelled.
    let mut lines: Vec<String> = support::numbered_lines(TALL_LINES)
        .lines()
        .map(str::to_owned)
        .collect();
    for at in TWEAKS {
        lines[at] = format!("line {} rewritten", at + 1);
    }
    lines.drain(CUT_AT..CUT_AT + CUT_LINES);
    scratch.write(TALL, format!("{}\n", lines.join("\n")));
    scratch
}

/// The one file in the [`tall`] fixture.
const TALL: &str = "src/deep.rs";

/// How long it is, which only has to be longer than everything cut out of it.
const TALL_LINES: usize = 400;

/// Zero-based lines rewritten above the deletion, more than `2 * CONTEXT + 1`
/// apart so each is a hunk of its own rather than all three sharing one.
const TWEAKS: [usize; 3] = [10, 40, 70];

/// Zero-based first line of the deletion.
const CUT_AT: usize = 200;

/// How many lines it removes. The reported number.
const CUT_LINES: usize = 76;

/// Where the deletion's hunk header sits on the index side.
const CUT_HUNK_START: u32 = CUT_AT as u32 + 1 - vigia_core::CONTEXT;

/// How many index-side lines that hunk covers: what was removed, plus three
/// lines of context on each side.
const CUT_HUNK_LINES: u32 = CUT_LINES as u32 + vigia_core::CONTEXT * 2;

fn tall_layout(app: &App) -> Body {
    body_layout(
        Rect::new(0, 0, 80, 24),
        &app.chrome("fixture", None, Pointing::default(), 0, ""),
        1,
        1,
    )
}

#[test]
fn following_a_tall_file_lands_on_its_busiest_change() {
    // I5 says the viewport goes to what just changed.
    let scratch = tall("shell-follow-tall");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    assert!(app.follow(TALL, &frame), "the view did not move at all");

    let layout = tall_layout(&app);
    let view = app
        .view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");

    // Non-vacuity, and it is this gate's whole premise: the landing row has to
    // be past the bottom of the pane, or a frame that resolved nothing would
    // draw the same rows and pass.
    assert!(
        app.position().row >= layout.diff,
        "the busiest change is at row {} of a {}-row region, so it was already \
         on screen from the heading and this fixture proves nothing",
        app.position().row,
        layout.diff
    );

    match view.rows.first() {
        Some(Row::Hunk {
            old_start,
            old_lines,
            ..
        }) => {
            assert_eq!(
                (*old_start, *old_lines),
                (CUT_HUNK_START, CUT_HUNK_LINES),
                "the top row is a hunk header, but not the deletion's"
            );
        }
        other => panic!("the top row is {other:?}, not the deletion's hunk header"),
    }

    assert!(
        view.rows.iter().any(|row| matches!(
            row,
            Row::Line {
                kind: vigia_core::LineKind::Removed,
                ..
            }
        )),
        "the view landed on the hunk header and drew none of what it removed"
    );

    // And the entry for this file was recorded, which is the other side of the counter
    // the listless-pane gate reads: that one asserts none is built where none can be
    // drawn, and a counter that never counts satisfies it vacuously.
    assert_eq!(
        view.recorded, 1,
        "the file the viewport is inside was not recorded, so the pinned list \
         asks the frame for it a second time"
    );
}

#[test]
fn a_landing_resolves_once_and_the_next_frame_does_not_move_it() {
    // The defect class `SPEC.md` §11.1 keeps ruling against is a row moving under a
    // reader.
    let scratch = tall("shell-follow-once");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    // Past the opening plain frame, so the two frames compared below differ in
    // the landing alone: I7's first frame draws without colour and the next one
    // parses, which would show up here as two different screens for a reason
    // that has nothing to do with follow.
    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    app.follow(TALL, &frame);
    let layout = tall_layout(&app);

    let first = app
        .view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");
    let landed = app.position();
    assert!(first.landed, "the frame drew without resolving the landing");
    assert!(landed.row > 0, "this fixture did not land anywhere");

    let second = app
        .view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");

    assert!(
        !second.landed,
        "the landing was resolved a second time, so the request outlived being served"
    );
    assert_eq!(
        app.position(),
        landed,
        "an untouched second frame moved the viewport"
    );
    assert_eq!(
        second.rows, first.rows,
        "the second frame drew a different screen with no input at all"
    );
}

#[test]
fn landing_on_a_change_costs_no_extra_diff() {
    // I4 over the *resolution*, where `following_a_file_costs_no_diff_and_no_read` is
    // I4 over the jump.
    let scratch = tall("shell-follow-landing-cost");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let layout = tall_layout(&app);

    // Settled first, so both frames below are reuse and the comparison is
    // between two warm frames rather than between a cold one and a warm one.
    support::settle(&mut frame);
    app.view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");

    let before = frame.stats();
    let plain = app
        .view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");
    let without = delta(before, frame.stats());
    assert!(!plain.landed, "the settling frame left a landing owed");

    app.follow(TALL, &frame);
    let before = frame.stats();
    let landing = app
        .view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");
    let with = delta(before, frame.stats());

    assert!(landing.landed, "this measured a frame that landed nowhere");
    assert_eq!(
        with, without,
        "a landing frame cost {with:?} against {without:?} without one"
    );
    assert_eq!(
        landing.read, plain.read,
        "the landing asked the frame for more files than the screen draws"
    );
}

#[test]
fn a_gesture_in_the_same_batch_settles_an_owed_landing() {
    // A tick and a keystroke coalesce into one batch, so a landing armed by the follow
    // can still be unresolved when a reader's own gesture runs.
    let scratch = tall("shell-follow-settled");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let layout = tall_layout(&App::new());

    for (name, action, expected) in [
        // A jump: `g` is one of the keys §11.1 gives the heading.
        ("g", Action::Top, 0),
        // A scroll: exactly one row was asked for.
        ("a scroll", Action::Scroll(1), 1),
        // A drag of the diff's own bar to the very top, which writes a position
        // of its own rather than going through either of the two above.
        ("a drag", Action::DiffTo(0), 0),
        // `n` at the end of the changed set, which moves nothing and so
        // reaches no jump at all, while still disengaging follow. The fixture is
        // one file, so a step forward from it is always that case.
        ("n at the end", Action::File(1), 0),
    ] {
        let mut app = App::new();
        assert!(app.follow(TALL, &frame), "the follow did not arm anything");
        app.apply(action, &mut frame, layout.diff).expect("apply");

        let view = app
            .view(&mut frame, &mut highlighter, &history, layout)
            .expect("view");

        assert!(
            !view.landed,
            "{name} left the landing owed, so the frame after it resolves one"
        );
        assert_eq!(
            app.position().row,
            expected,
            "{name} was overridden by a landing the follow before it armed"
        );
    }
}

#[test]
fn disengaging_follow_settles_an_owed_landing() {
    // `f` is the reader asking the view to stop moving itself, and a tick can land in
    // the same batch as the keystroke.
    let scratch = tall("shell-follow-disengaged");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let layout = tall_layout(&app);

    assert!(app.follow(TALL, &frame), "the follow did not arm anything");
    app.apply(Action::ToggleFollow, &mut frame, layout.diff)
        .expect("apply");
    assert!(!app.following(), "`f` did not disengage");

    let view = app
        .view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");

    assert!(
        !view.landed,
        "the frame after `f` resolved a landing anyway"
    );
    assert_eq!(
        app.position().row,
        0,
        "the view moved itself once more after the reader asked it to stop"
    );
}

#[test]
fn a_tick_that_follows_nothing_drops_the_landing_the_one_before_it_armed() {
    // Two ticks in one batch, with an advance between them, which is what the drain
    // does: every wake is handled in arrival order and only the paint is shared.
    let scratch = Scratch::new("shell-follow-stale-debt");
    scratch.write(TALL, support::numbered_lines(TALL_LINES));
    scratch.write("src/aaa.rs", "fn a() {}\n");
    scratch.write("src/zzz.rs", support::numbered_lines(TALL_LINES));
    scratch.commit_all("baseline");

    // The tall file is the one the first tick follows, and `src/zzz.rs` is a
    // second long file so that landing in the wrong one is a visible row rather
    // than a clamp back to zero.
    let mut lines: Vec<String> = support::numbered_lines(TALL_LINES)
        .lines()
        .map(str::to_owned)
        .collect();
    for at in TWEAKS {
        lines[at] = format!("line {} rewritten", at + 1);
    }
    lines.drain(CUT_AT..CUT_AT + CUT_LINES);
    let cut = format!("{}\n", lines.join("\n"));
    scratch.write(TALL, &cut);
    scratch.write("src/zzz.rs", &cut);
    scratch.write("src/aaa.rs", "fn a() { let reverted = 1; }\n");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    frame.advance().expect("advance");
    assert!(app.follow(TALL, &frame), "the first tick armed nothing");

    // The agent reverts the file the second tick is about, so the walk stops
    // reporting it and the follow below finds nothing to jump to.
    scratch.write("src/aaa.rs", "fn a() {}\n");
    frame.advance().expect("advance");
    assert!(
        !frame.files().iter().any(|f| f.path == "src/aaa.rs"),
        "the revert did not take the file out of the changed set"
    );
    assert!(
        !app.follow("src/aaa.rs", &frame),
        "a path with no change in it reported a jump"
    );

    let layout = body_layout(
        Rect::new(0, 0, 80, 24),
        &app.chrome("fixture", None, Pointing::default(), 0, ""),
        frame.files().len(),
        frame.files().len(),
    );
    let view = app
        .view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");

    assert!(
        !view.landed,
        "the landing the first tick armed was resolved by a tick that followed \
         nothing, against an index the advance in between renumbered"
    );
    assert_eq!(
        app.position().row,
        0,
        "the viewport landed inside a file no tick ever named"
    );
}

/// A file whose busiest hunk is near its end and shorter than the pane.
fn tail(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(TAIL, support::numbered_lines(TALL_LINES));
    scratch.commit_all("baseline");

    let mut lines: Vec<String> = support::numbered_lines(TALL_LINES)
        .lines()
        .map(str::to_owned)
        .collect();
    for at in [10, 40, 70, 100] {
        lines[at] = format!("line {} rewritten", at + 1);
    }
    for (at, line) in lines.iter_mut().enumerate().skip(CUT_AT).take(4) {
        *line = format!("line {} rewritten", at + 1);
    }
    scratch.write(TAIL, format!("{}\n", lines.join("\n")));
    scratch
}

/// The one file in the [`tail`] fixture.
const TAIL: &str = "src/shallow.rs";

#[test]
fn a_landing_in_the_last_file_rests_its_tail_on_the_bottom_row() {
    // A jump clears `anchored`, which switches the short-tail back-up off on purpose:
    // follow's claim is about what belongs at the top.
    let scratch = tail("shell-follow-tail");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let layout = tall_layout(&app);

    assert_eq!(frame.files().len(), 1, "this fixture is one file");
    app.follow(TAIL, &frame);

    let view = app
        .view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");

    assert!(view.landed, "this measured a frame that landed nowhere");
    assert!(
        app.position().row > 0,
        "the landing did not fire, so the back-up under test never ran"
    );
    assert_eq!(
        view.rows.len(),
        layout.diff,
        "the landing left {} of {} rows blank under the last file",
        layout.diff.saturating_sub(view.rows.len()),
        layout.diff
    );
    // And the bottom row is the diff's last, which fullness alone does not
    // say: a back-up one row too far also fills the pane, and leaves the final
    // row of the diff undrawn under it.
    assert_eq!(
        view.rows_above + view.rows.len(),
        view.total_rows,
        "the pane is full but the diff's last row is not on it"
    );
    // And the change is still on screen, which is what the back-up must not
    // cost: resting the tail moves the busiest hunk down the pane, never off it.
    assert!(
        view.rows.iter().any(|row| matches!(
            row,
            Row::Line {
                kind: vigia_core::LineKind::Added,
                ..
            }
        )),
        "resting the tail on the bottom row scrolled the change off the top"
    );
}

#[test]
fn an_advance_that_renumbers_the_files_drops_a_landing_armed_before_it() {
    // A tick that names no path never reaches `App::follow`.
    let scratch = Scratch::new("shell-follow-renumbered");
    scratch.write("src/aaa.rs", "fn a() {}\n");
    scratch.write("src/mmm.rs", support::numbered_lines(TALL_LINES));
    scratch.write(TALL, support::numbered_lines(TALL_LINES));
    scratch.commit_all("baseline");

    scratch.write("src/aaa.rs", "fn a() { let staged = 1; }\n");
    let mut lines: Vec<String> = support::numbered_lines(TALL_LINES)
        .lines()
        .map(str::to_owned)
        .collect();
    for at in TWEAKS {
        lines[at] = format!("line {} rewritten", at + 1);
    }
    lines.drain(CUT_AT..CUT_AT + CUT_LINES);
    let cut = format!("{}\n", lines.join("\n"));
    scratch.write("src/mmm.rs", &cut);
    scratch.write(TALL, &cut);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    frame.advance().expect("advance");
    // Status reports lexicographically: aaa, deep, mmm.
    assert_eq!(frame.files().len(), 3, "the fixture is not three files");
    assert!(app.follow(TALL, &frame), "the tick armed nothing");
    assert_eq!(
        app.position().file,
        1,
        "the fixture did not arm on index one"
    );

    // The agent commits the file that sorts first. No path reaches the shell,
    // and `src/mmm.rs` slides into index one behind the followed file's back.
    scratch.git(&["add", "src/aaa.rs"]);
    scratch.git(&["commit", "-m", "the agent in the other pane commits"]);
    frame.advance().expect("advance");
    assert_eq!(
        frame.files()[1].path,
        "src/mmm.rs",
        "the commit did not renumber the list under the position"
    );

    let layout = body_layout(
        Rect::new(0, 0, 80, 24),
        &app.chrome("fixture", None, Pointing::default(), 0, ""),
        frame.files().len(),
        frame.files().len(),
    );
    let view = app
        .view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");

    assert!(
        !view.landed,
        "a landing armed for {TALL} was resolved against src/mmm.rs, which \
         inherited its index when the advance renumbered the list"
    );
    assert_eq!(
        app.position().row,
        0,
        "the viewport landed inside a file no tick ever named"
    );
}

#[test]
fn a_refused_landing_is_settled_rather_than_deferred() {
    // The guard is re-read every frame, so refusing a landing and keeping it
    // is not the same as dropping it: the debt fires the moment an index names
    // the followed path again, on a frame no tick armed.
    let scratch = Scratch::new("shell-follow-deferred");
    scratch.write("src/aaa.rs", "fn a() {}\n");
    scratch.write("src/zzz.rs", "fn z() {}\n");
    scratch.write(TALL, support::numbered_lines(TALL_LINES));
    scratch.commit_all("baseline");

    scratch.write("src/aaa.rs", "fn a() { let staged = 1; }\n");
    scratch.write("src/zzz.rs", "fn z() { let staged = 1; }\n");
    let mut lines: Vec<String> = support::numbered_lines(TALL_LINES)
        .lines()
        .map(str::to_owned)
        .collect();
    for at in TWEAKS {
        lines[at] = format!("line {} rewritten", at + 1);
    }
    lines.drain(CUT_AT..CUT_AT + CUT_LINES);
    scratch.write(TALL, format!("{}\n", lines.join("\n")));

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    frame.advance().expect("advance");
    assert!(app.follow(TALL, &frame), "the tick armed nothing");
    assert_eq!(
        app.position().file,
        1,
        "the fixture did not arm on index one"
    );

    scratch.git(&["add", "src/aaa.rs", "src/zzz.rs"]);
    scratch.git(&["commit", "-m", "the agent commits around the followed file"]);
    frame.advance().expect("advance");
    assert_eq!(
        frame.files().len(),
        1,
        "the commit left more than the followed file behind"
    );

    let layout = body_layout(
        Rect::new(0, 0, 80, 24),
        &app.chrome("fixture", None, Pointing::default(), 0, ""),
        frame.files().len(),
        frame.files().len(),
    );
    let refused = app
        .view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");
    assert!(
        !refused.landed,
        "the landing resolved against an index that was out of range"
    );
    // The clamp is what sets the trap: the position now names the followed file
    // again, so a debt kept above would be servable from here.
    assert_eq!(app.position().file, 0, "the position was not clamped back");
    assert_eq!(
        frame.files()[0].path,
        TALL,
        "the clamped position does not name the followed file, so a kept debt \
         would still be refused and this proves nothing"
    );

    app.apply(Action::ToggleSheet, &mut frame, layout.diff)
        .expect("apply");
    let after = app
        .view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");

    assert!(
        !after.landed,
        "the refused landing was kept and resolved on a later frame, so opening \
         the sheet moved the viewport"
    );
    assert_eq!(app.position().row, 0, "the sheet moved the diff");
}

#[test]
fn a_landing_above_a_hunkless_tail_leaves_no_blank_rows() {
    // The last file is not the only one whose rows can run out.
    let scratch = Scratch::new("shell-follow-hunkless");
    scratch.write(TAIL, support::numbered_lines(TALL_LINES));
    scratch.write("src/zzz.bin", b"\0\0committed\0\0".as_slice());
    scratch.commit_all("baseline");

    let mut lines: Vec<String> = support::numbered_lines(TALL_LINES)
        .lines()
        .map(str::to_owned)
        .collect();
    for at in [10, 40, 70, 100] {
        lines[at] = format!("line {} rewritten", at + 1);
    }
    for (at, line) in lines.iter_mut().enumerate().skip(CUT_AT).take(2) {
        *line = format!("line {} rewritten", at + 1);
    }
    scratch.write(TAIL, format!("{}\n", lines.join("\n")));
    scratch.write("src/zzz.bin", b"\0\0rewritten\0\0".as_slice());

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let layout = body_layout(
        Rect::new(0, 0, 80, 24),
        &app.chrome("fixture", None, Pointing::default(), 0, ""),
        frame.files().len(),
        frame.files().len(),
    );

    assert!(
        frame.files().len() > 1 && frame.files().last().expect("a file").path != TAIL,
        "the followed file is the last one, so this is the last-file case again"
    );
    assert!(app.follow(TAIL, &frame), "the follow did not move the view");

    let view = app
        .view(&mut frame, &mut highlighter, &history, layout)
        .expect("view");

    assert!(view.landed, "this measured a frame that landed nowhere");
    assert!(
        app.position().row > 0,
        "the landing did not fire, so the back-up under test never ran"
    );
    assert_eq!(
        view.rows.len(),
        layout.diff,
        "the landing left {} of {} rows blank above a hunkless tail",
        layout.diff.saturating_sub(view.rows.len()),
        layout.diff
    );
}

#[test]
fn a_landing_survives_a_pane_with_no_diff_region() {
    // The reason the request is cleared on `View::landed` rather than unconditionally.
    let scratch = tall("shell-follow-kept");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    assert!(app.follow(TALL, &frame), "the follow did not arm anything");

    let squeezed = Body {
        diff: 0,
        ..tall_layout(&app)
    };
    let none = app
        .view(&mut frame, &mut highlighter, &history, squeezed)
        .expect("view");
    assert!(
        none.rows.is_empty() && !none.landed,
        "a pane with no diff region drew rows or resolved a landing"
    );

    let view = app
        .view(&mut frame, &mut highlighter, &history, tall_layout(&app))
        .expect("view");

    assert!(
        view.landed,
        "the request was forgotten by the frame that could not serve it, so the \
         reader stays on the heading until the agent writes again"
    );
    assert!(app.position().row > 0, "the kept request landed nowhere");
}

#[test]
fn a_pane_with_no_list_builds_no_entry_it_cannot_draw() {
    // The one guard here that nothing else can see.
    let scratch = tall("shell-follow-listless");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    assert!(app.follow(TALL, &frame), "the follow did not arm anything");
    let listless = Body {
        list: 0,
        ..tall_layout(&app)
    };
    let view = app
        .view(&mut frame, &mut highlighter, &history, listless)
        .expect("view");

    assert!(view.landed, "this measured a frame that landed nowhere");
    assert!(
        app.position().row > 0,
        "the landing did not fire, so the heading is drawn and its entry is the \
         row rather than a record"
    );
    assert_eq!(
        view.recorded, 0,
        "a pane with no list recorded {} entries, and nothing on it can read one",
        view.recorded
    );
}

#[test]
fn the_landing_turns_on_the_diff_regions_own_height() {
    // What the unit battery cannot see.
    let scratch = tall("shell-follow-heights");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    for (diff, expected, why) in [
        // Below the floor: the change is four rows under the landing, so a
        // four-row region draws the `@@` and none of it. Keep the heading.
        (
            4usize,
            0usize,
            "a region too short to draw the change from the landing",
        ),
        (
            5,
            28,
            "one row taller, and the landing is worth the heading",
        ),
        // Below the ceiling: the change is at row 32 of the block, so a 32-row
        // region stops one short of it and the landing is still needed.
        (
            32,
            28,
            "a region one row short of drawing the change from the heading",
        ),
        (
            33,
            0,
            "one row taller, and the change is drawn without moving at all",
        ),
    ] {
        let mut app = App::new();
        assert!(app.follow(TALL, &frame), "the follow did not arm anything");
        let body = Body {
            diff,
            ..tall_layout(&app)
        };
        app.view(&mut frame, &mut highlighter, &history, body)
            .expect("view");

        assert_eq!(
            app.position().row,
            expected,
            "a {diff}-row diff region landed on row {}, and it is {why}",
            app.position().row
        );
    }
}
