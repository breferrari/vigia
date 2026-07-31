//! I5: correct with zero interaction.
//!
//! > Auto-follows the newest change and scrolls to it, untouched.
//!
//! The invariant that separates a monitor from a viewer, so the load-bearing
//! word in nearly every test here is **untouched**: the view moves with no
//! `Action` applied at all. Where an action does appear it is the subject
//! rather than the setup, because follow mode is defined as much by what
//! disengages it as by what it does.
//!
//! `SPEC.md` §11.1 is the rule, ruled as B1 and B2 on 2026-07-30. Two of its
//! clauses are the ones that would go wrong quietly rather than loudly, and
//! each has a test of its own below: a **resize must not disengage**, because a
//! pane beside an agent is resized constantly and follow mode would evaporate
//! for free; and **`G` must disengage rather than re-engage**, because
//! otherwise a reader cannot look at the newest file without re-arming the
//! view.
//!
//! What is asserted is the **path drawn at the top of the screen**, not the
//! index the position holds. Status order is not the order the fixture writes
//! its files in, so an index assertion would be restating the implementation's
//! own lookup; a path is an oracle the shell cannot satisfy by being
//! consistently wrong.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use vigia::{Action, App, Position, Row, Theme, body_height, render};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, delta};

/// Files in the fixture, enough that following has somewhere to go.
const FILES: usize = 40;

/// A file far enough down the list that landing on it cannot be a coincidence.
const TARGET: usize = 7;

/// A second one, so "moved once" and "moves whenever asked" are different
/// assertions.
const OTHER: usize = 21;

fn body() -> usize {
    // Eighty columns, where the footer is one line whatever the state, so the
    // row count here does not move when I6's two-line footer engages.
    body_height(
        Rect::new(0, 0, 80, 24),
        &App::new().chrome("fixture"),
        FILES,
    )
}

fn fixture(name: &str) -> Scratch {
    Scratch::large_diff(name, FILES, 1)
}

/// The path status reports at `index`.
///
/// Read out of the frame rather than constructed, because the fixture writes
/// `src/mod_0.rs` through `src/mod_39.rs` and status reports them
/// lexicographically, so `mod_10` precedes `mod_2`. A test that assumed
/// otherwise would be asserting against the wrong file and still passing.
fn path_at(frame: &Frame, index: usize) -> String {
    frame.files()[index].path.clone()
}

/// The file whose heading is drawn at the top of the screen.
///
/// The oracle for every assertion in this file. Following is a claim about
/// what the reader sees, so it is checked against what would be drawn rather
/// than against the position that produced it.
fn top_file(
    app: &mut App,
    frame: &mut Frame,
    highlighter: &mut Highlighter,
    history: &History,
) -> String {
    let view = app.view(frame, highlighter, history, body()).expect("view");
    match view.rows.first() {
        Some(Row::File { path, .. }) => path.clone(),
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
    let mut highlighter = Highlighter::new();
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
}

#[test]
fn a_scripted_edit_sequence_draws_the_file_that_changed_last() {
    // I5's proof exactly as `SPEC.md` §3 words it: a scripted edit sequence,
    // snapshot the frame, no input given. `path_at` is not used here because
    // the point is to read the picture, and the picture has to name the file
    // the script touched last without anything in the test saying so.
    let scratch = Scratch::new("shell-follow-scripted");
    scratch.write("README.md", "docs\n");
    scratch.write("src/first.rs", "fn first() {}\n");
    scratch.write("src/second.rs", "fn second() {}\n");
    scratch.commit_all("baseline");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
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
    let height = body_height(area, &app.chrome("fixture"), frame.files().len());
    let view = app
        .view(&mut frame, &mut highlighter, &history, height)
        .expect("view");
    assert_eq!(view.files, 3, "the fixture is not three changed files");

    let theme = Theme::default();
    let chrome = app.chrome("fixture");
    let mut terminal = Terminal::new(TestBackend::new(64, 12)).expect("terminal");
    terminal
        .draw(|f| {
            let area = f.area();
            render(f.buffer_mut(), area, &view, &theme, &chrome);
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
    let mut highlighter = Highlighter::new();
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
    // The half of B1 that is easy to get half right. Re-engaging has to *jump*,
    // not merely arm: `less +F` goes to the end when you ask it to follow, and
    // a reader pressing `f` is asking to see what changed rather than to wait
    // for the next thing that does.
    let scratch = fixture("shell-follow-toggle");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    app.apply(Action::Scroll(1), &mut frame, body())
        .expect("scroll");
    assert!(!app.following());

    // Arrives while disengaged, so it is recorded and not acted on. That is
    // what gives `f` somewhere to jump to a moment later.
    //
    // Checked against the position rather than [`top_file`], because after a
    // one-row scroll the top of the screen is a hunk header rather than a
    // heading. That is the correct picture and the wrong oracle.
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
    let mut highlighter = Highlighter::new();
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
    // B1's rationale as a test. "Jump to the last changed file" and "resume
    // following" are different intents, and overloading `G` with both would
    // leave a reader unable to look at the newest file without re-arming the
    // view. Both ends, because `g` and `G` are the same decision.
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
fn a_change_to_a_file_that_is_not_in_the_diff_leaves_the_view_where_it_was() {
    // Ordinary rather than exceptional: an edit reverted before the tick
    // landed, or a file written back to the bytes the index already holds.
    // There is no newest *change*, so there is nowhere to go, and jumping
    // anywhere would be worse than staying.
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
    // I4 and I2a, held over the path I5 added. Following looks up a name in a
    // list the frame already has, so it must cost nothing: no diff computed,
    // no file read, and not even the `stat` a revalidation would take. The
    // rejected alternative for finding "the newest file" was one `stat` per
    // changed file, which is #19's breach, so this is the gate that stops it
    // creeping back in through the front door.
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
    // Follow mode writes a raw index into the position, and the agent in the
    // other pane can shorten the list underneath it. `Frame::diff` panics on
    // an index past the end deliberately, so this is the crash that would
    // reach a reader who touched nothing at all.
    let scratch = fixture("shell-follow-shrink");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
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
        .view(&mut frame, &mut highlighter, &history, body())
        .expect("a shrunken list must not panic");
    assert!(view.rows.is_empty(), "a clean worktree drew rows");
    assert_eq!(
        app.position(),
        Position::default(),
        "the position was left pointing past the end of an empty list"
    );
}
