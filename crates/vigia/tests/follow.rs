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
        &App::new().chrome("fixture", None, Pointing::default(), 0),
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
    // **And this is the other half of #257's rule**, on the fixture that has
    // it: every block here is five rows against a thirteen-row region, so the
    // busiest hunk is already drawn from the heading and the heading stays. A
    // landing that fired unconditionally would cost the path, the counts, the
    // sigil and the heat strip to show the reader rows they could already see,
    // and it would turn this assertion red. Both sides of the edge it turns on
    // are gated exactly in `view.rs`'s
    // `a_busiest_hunk_already_on_screen_keeps_the_heading`.
    assert_eq!(
        app.position().row,
        0,
        "the view moved to the right file but not to the top of it, so the \
         heading of what just changed is scrolled off"
    );

    // **A request answered with "keep the heading" is still answered**, and
    // saying otherwise is not harmless: the caller clears the debt on this, so a
    // frame that resolved to row zero and reported nothing leaves a request
    // armed to fire on the next resize. Every other gate here lands on a row
    // above zero, so this is the only place the distinction is visible.
    //
    // Re-armed, because the assertion above went through `top_file`, which draws
    // a frame and so has already served the first request.
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
        &app.chrome("fixture", None, Pointing::default(), 0),
        frame.files().len(),
        frame.files().len(),
    );
    let view = app
        .view(&mut frame, &mut highlighter, &history, height)
        .expect("view");
    assert_eq!(view.files, 3, "the fixture is not three changed files");

    let theme = Theme::default();
    let chrome = app.chrome("fixture", None, Pointing::default(), 0);
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
    // The half of B1 that is easy to get half right. Re-engaging has to *jump*,
    // not merely arm: `less +F` goes to the end when you ask it to follow, and
    // a reader pressing `f` is asking to see what changed rather than to wait
    // for the next thing that does.
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
fn a_file_step_disengages_follow_at_both_ends_of_the_diff() {
    // `n` moves the diff, so it disengages for the reason every other jump does.
    // The half worth asserting is the **second** one: `p` at the first file moves
    // nothing at all, and it still disengages, because on this map follow yields
    // to a reader's intent rather than to whether the arithmetic landed
    // somewhere new. `Action::Top` at the top already behaves this way and the
    // test one row up pins it; a file step that quietly took the other rule would
    // leave a reader who asked to go somewhere being dragged back on the next
    // write, with nothing on screen to say why.
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
///
/// The shape [#257](https://github.com/breferrari/vigia/issues/257) was reported
/// against: a Swift test file carrying a 76-line deletion that the reader could
/// not see, because follow put the heading on the top row and the deletion was
/// below the bottom one. Three small edits above it are what push it there; a
/// two-hunk file puts its second header ten rows down, which fits on any pane
/// and would make this gate pass against the old behaviour.
///
/// Written out rather than built from [`Scratch::sparse_edits`] because the
/// hunks here are deliberately **unequal**: that fixture edits every `every`th
/// line, so every hunk holds exactly one change and no hunk is the busiest.
fn tall(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(TALL, support::numbered_lines(TALL_LINES));
    scratch.commit_all("baseline");

    // Split from the same helper the baseline was written with, rather than
    // re-spelled. Two definitions of one format sitting a line apart is one
    // change to `numbered_lines` away from making this a whole-file rewrite,
    // which would still be a diff and would no longer be this fixture.
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
///
/// One-based, and three lines of context above the first line removed:
/// `CUT_AT` is zero-based, so the first line gone is 201 and the hunk opens at
/// 198.
const CUT_HUNK_START: u32 = CUT_AT as u32 + 1 - vigia_core::CONTEXT;

/// How many index-side lines that hunk covers: what was removed, plus three
/// lines of context on each side.
const CUT_HUNK_LINES: u32 = CUT_LINES as u32 + vigia_core::CONTEXT * 2;

fn tall_layout(app: &App) -> Body {
    body_layout(
        Rect::new(0, 0, 80, 24),
        &app.chrome("fixture", None, Pointing::default(), 0),
        1,
        1,
    )
}

#[test]
fn following_a_tall_file_lands_on_its_busiest_change() {
    // I5 says the viewport goes to what just changed. On a file whose diff is
    // one screenful the heading and the change are the same place and the
    // promise is kept by accident of size; on this one they are twenty-odd rows
    // apart, and landing on the heading shows the reader a filename and three
    // one-line tweaks instead of the 76 lines that just went.
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

    // **And the entry for this file was recorded**, which is the other side of
    // the counter the listless-pane gate reads: that one asserts none is built
    // where none can be drawn, and a counter that never counts satisfies it
    // vacuously. One, because the walk reaches one file and its heading is above
    // the window.
    assert_eq!(
        view.recorded, 1,
        "the file the viewport is inside was not recorded, so the pinned list \
         asks the frame for it a second time"
    );
}

#[test]
fn a_landing_resolves_once_and_the_next_frame_does_not_move_it() {
    // The defect class `SPEC.md` §11.1 keeps ruling against is a row moving
    // under a reader. A landing is resolved by the frame that draws it, so the
    // second frame has nothing left to resolve and must draw the same screen: a
    // rule that re-derived the row every frame would walk the viewport down a
    // file as an agent's hunks grew.
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
    // I4 over the *resolution*, where `following_a_file_costs_no_diff_and_no_read`
    // is I4 over the jump. The landing is arithmetic on a diff the walk has
    // already fetched, so a frame that lands must cost exactly what the same
    // frame costs without one. The version that is wrong here is the readable
    // one: asking `Frame::diff` for the file a second call earlier, which
    // re-reads any file written in the last two seconds and so would put a
    // second whole-file read on the one file that is always inside that margin.
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
    // **A tick and a keystroke coalesce into one batch**, so a landing armed by
    // the follow can still be unresolved when a reader's own gesture runs. The
    // request is settled by every gesture `Action::is_manual_scroll` calls one,
    // or the frame after it draws over the row the reader just asked for, and a
    // request that outlived its jump would be inherited by the next one:
    // `SPEC.md` §11.1 rules that `G`, the digits and `n`/`p` land on a heading,
    // and a debt left armed makes them land mid-file instead. `n` at an end
    // writes no position at all and still settles it, which is why the predicate
    // is the rule rather than the write.
    //
    // Driven with no view between the follow and the gesture, which is the state
    // the drain produces and the only one where the debt is still outstanding.
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
        // A scroll: the reader asked for exactly one row.
        ("a scroll", Action::Scroll(1), 1),
        // A drag of the diff's own bar to the very top, which writes a position
        // of its own rather than going through either of the two above.
        ("a drag", Action::DiffTo(0), 0),
        // **`n` at the end of the changed set**, which moves nothing and so
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
    // `f` is the reader asking the view to stop moving itself, and a tick can
    // land in the same batch as the keystroke. A request the frame has not
    // resolved yet would move the viewport once more after that, which is the
    // one thing the key was pressed to stop.
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
    // **Two ticks in one batch, with an advance between them**, which is what
    // the drain does: every wake is handled in arrival order and only the paint
    // is shared. The second names a path the walk no longer reports, an edit
    // reverted before the tick landed, so it writes no position at all. A debt
    // left over from the first would then resolve against an *index*, and the
    // advance has just renumbered every one of them.
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
        &app.chrome("fixture", None, Pointing::default(), 0),
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

/// A file whose busiest hunk is **near its end and shorter than the pane**.
///
/// Deliberately not [`tall`], which cannot show this: there the busiest hunk is
/// a 76-line deletion, so landing on it fills any pane from its own rows and no
/// tail is left over. This one is four one-line tweaks and then a four-line
/// rewrite low down, so the busiest hunk is fifteen rows against an eighteen-row
/// region and the rows under it run out. A block ends at its last hunk, so what
/// is left below a landing is that hunk and nothing else, however long the file
/// is.
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
    // A jump clears `anchored`, which switches the short-tail back-up off on
    // purpose: follow's claim is about what belongs at the top. The claim is met
    // by any row the change is visible from, though, so honouring it past the
    // end of the diff draws a handful of rows over a block of blanks. Every
    // other file has the next one's rows under it and cannot do this.
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
    // **And the bottom row is the diff's last**, which fullness alone does not
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
    // **A tick that names no path never reaches `App::follow`.** The drain
    // advances the frame on every tick and follows only when the burst carried
    // one, and a `.git/index` write carries none: an agent running `git add` or
    // `git commit` beside the pane produces exactly that. The advance renumbers
    // every index, and a landing armed by the tick before it holds nothing but
    // an index, so resolving it puts the viewport deep inside whichever file
    // inherited the number.
    //
    // **The renumbered index has to still name a file**, which is the whole
    // subject of the guard: a first draft of this committed everything, so the
    // list was empty, `View::collect` returned at its own `files == 0` branch,
    // and the gate passed with the guard deleted. Here `src/aaa.rs` is committed
    // and the third file slides into its place.
    //
    // Driven the way the loop drives it, with no `follow` call for the second
    // tick, because that is the state the defect needs.
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
        &app.chrome("fixture", None, Pointing::default(), 0),
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
    // **The guard is re-read every frame**, so refusing a landing and keeping it
    // is not the same as dropping it: the debt fires the moment an index names
    // the followed path again, on a frame no tick armed.
    //
    // The fixture is what makes that reachable, and the renumbering gate above
    // cannot do it: there the followed file keeps an index no later frame points
    // at, so the guard refuses forever and a kept debt is indistinguishable from
    // a dropped one. Here the two files *around* the followed one are committed,
    // so the position is out of range on the frame that refuses, and
    // `View::collect` then clamps it back onto the followed file. The next frame
    // is the one that would fire.
    //
    // Opening the gestures sheet is where a reader would meet it: `ToggleSheet`
    // moves no viewport at all, and its own ruling is that a reader who opens it
    // and closes it is looking at the screen they left.
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
        &app.chrome("fixture", None, Pointing::default(), 0),
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
    // The last file is not the only one whose rows can run out. A file followed
    // only by hunkless ones has a one-row block under it and cannot fill the
    // pane either, which is why the first fix here, a clamp on the last file,
    // covered one case of the class rather than the class. A binary file is the
    // cheapest hunkless block the default view can hold: a heading and one line
    // saying why. (A rename is cheaper still and unreachable here, because
    // `git mv` stages it and the default view is the unstaged one.)
    //
    // Built here rather than on top of [`tail`], which has already committed its
    // baseline: a second `commit_all` would take the tall file's own diff with
    // it and leave the fixture with nothing to follow.
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
        &app.chrome("fixture", None, Pointing::default(), 0),
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
    // **The reason the request is cleared on `View::landed` rather than
    // unconditionally.** A pane dragged below the floor draws no diff at all, so
    // that frame resolves nothing, and forgetting the request there would leave
    // the reader on the heading for good: the tick that armed it has been spent
    // and no other will re-arm it until the agent writes again.
    //
    // A resize is `SPEC.md` §11.1's "no state change", so this is the same
    // ruling the follow mode paragraph already makes about disengaging.
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
    // **The one guard here that nothing else can see.** The walk records an
    // entry for the file the viewport is inside so the pinned list does not ask
    // the frame for it a second time, and on a pane too short for a list there
    // is no list to serve: the record is dropped unread, and building it is the
    // heat projection over that file's whole diff, every frame.
    //
    // No counter in `FrameStats` moves for it, because building an entry reads
    // nothing: it walks lines the frame already holds. So `View::recorded` is
    // what this asserts on, and it exists for this. Mutating the guard to `true`
    // survived every other gate in the suite, which is what asked for it.
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
    // **What the unit battery cannot see.** `landing_of`'s own tests pin both
    // edges of the rule as a function of the `height` they hand it, and never go
    // through `View::collect`, so they say nothing about *which* number the walk
    // passes. Every other gate here sits far from both edges, so the call site
    // could add or subtract a row and the whole suite would stay green while a
    // reader on a pane one row either side of an edge got the wrong screen.
    //
    // The `tall` fixture puts the busiest hunk's header at row 28 and its first
    // removal at row 32, so the two edges are four rows apart at the bottom and
    // at 32 at the top, and driving the region to each side of both is what pins
    // the argument.
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
