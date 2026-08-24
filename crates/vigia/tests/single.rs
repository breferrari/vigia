//! `SPEC.md` §11.2 **B16**: the diff pinned to one file, on `s`.
//!
//! Every gate here is one line of [#297](https://github.com/breferrari/vigia/issues/297)'s
//! own gate list, and the list is worth restating because it is what the ruling
//! promised rather than what the code happens to do: off by default; the diff's
//! total is this file's and scrolling clamps at both ends of it; no row of
//! another file is ever drawn; `n`, `p`, a digit and a click still change which
//! file is shown; and toggling it returns the pane to the screen it started
//! from.
//!
//! **The fixture is files taller than the pane**, which is the whole of what
//! separates this from `scroll.rs`. There a file is four rows and the interesting
//! event is crossing a boundary; here a file is twenty-two rows against a body of
//! thirteen, so *scrolling inside one file* and *running out of file* are two
//! distinguishable things and a clamp that fired at the wrong one would be
//! visible.
//!
//! **What is asserted is the drawn rows and the resolved position**, not the
//! request. A pin is enforced in `View::collect`, so a gate reading `App`'s own
//! position would be reading the thing that is allowed to be out of range.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use vigia::{
    Action, App, Body, Position, Row, TRACK_SCALE, View, Viewport, action_for, body_layout,
    diff_height, regions,
};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, materialise};

/// Files in the fixture. Enough that a pinned file has neighbours on both sides,
/// so *stopped at this file* and *stopped at the end of the diff* are different
/// failures.
const FILES: usize = 6;

/// Lines each file rewrites.
const LINES: usize = 10;

/// Rows one file's **content** occupies: its heading, one hunk header, and the
/// removed and added line for each of [`LINES`].
const SPAN: usize = 2 + 2 * LINES;

/// Rows one file's whole block occupies, which is [`SPAN`] plus the blank that
/// closes it for every file but the last.
const BLOCK: usize = SPAN + 1;

/// The file these gates pin. Not the first and not the last, so a walk that
/// silently ran from the top of the diff or back from its end would be caught.
const PINNED: usize = 2;

fn body() -> usize {
    diff_height(
        Rect::new(0, 0, 80, 24),
        &App::new().chrome("fixture", None, None, None, None, None),
        FILES,
    )
}

/// The diff region alone, for the gates about the walk.
///
/// List-free for `scroll.rs`'s reason: these are about how the walk crosses
/// files, or refuses to, and a pinned list would couple their row arithmetic to
/// a cap they are not about.
fn split() -> Body {
    Body::diff_only(body())
}

/// The shipped split, for the gates that need a list to click or address.
fn listed() -> Body {
    body_layout(
        Rect::new(0, 0, 80, 24),
        &App::new().chrome("fixture", None, None, None, None, None),
        FILES,
    )
}

fn fixture(name: &str) -> Scratch {
    Scratch::large_diff(name, FILES, LINES)
}

/// A shell with the diff pinned to [`PINNED`], which is where most gates start.
fn pinned(frame: &mut Frame) -> App {
    let mut app = App::new();
    app.apply(Action::File(PINNED as isize), frame, body())
        .expect("apply");
    app.apply(Action::ToggleSingle, frame, body())
        .expect("apply");
    app
}

fn draw(
    app: &mut App,
    frame: &mut Frame,
    highlighter: &mut Highlighter,
    history: &History,
    at: Body,
) -> View {
    app.view(frame, highlighter, history, at).expect("view")
}

/// Every file index a screen draws a heading for, plus the file its top is in.
///
/// The oracle for *no row of another file is ever drawn*. A heading count alone
/// would miss a screen resting deep inside the wrong file, which draws no
/// heading at all.
fn files_on(view: &View) -> Vec<usize> {
    let mut seen = vec![view.top.file];
    let mut at = view.top.file;
    for row in &view.rows {
        if matches!(row, Row::File(_)) && !view.rows.first().is_some_and(|r| std::ptr::eq(r, row)) {
            at += 1;
            seen.push(at);
        }
    }
    seen.sort_unstable();
    seen.dedup();
    seen
}

#[test]
fn the_fixture_is_the_shape_the_rest_of_this_file_assumes() {
    // Every assertion below is arithmetic over SPAN and BLOCK, and the one that
    // matters most is that a file is **taller than the body**: with a shorter
    // one, clamping at the file's end and clamping at the diff's end would land
    // in the same place and every gate here would pass against a pin that did
    // nothing.
    let scratch = fixture("shell-single-shape");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    assert_eq!(frame.files().len(), FILES);
    assert!(
        SPAN > body(),
        "a file is {SPAN} rows against a body of {}, so this fixture cannot tell \
         a pinned clamp from an unpinned one",
        body()
    );
    assert_eq!(
        vigia::span_in(&mut frame, PINNED).expect("span"),
        SPAN,
        "the pinned file is not {SPAN} rows of content"
    );
    assert_eq!(
        vigia::rows_in(&mut frame, PINNED).expect("rows"),
        BLOCK,
        "the pinned file's block is not {BLOCK} rows, so its closing blank is gone"
    );
}

#[test]
fn a_shell_starts_unpinned() {
    // **Off by default**, asserted through what a screen draws rather than
    // through a getter, because the field is private and because the ruling is
    // about the pane: a reader who has pressed nothing scrolls the whole changed
    // set exactly as every version before this one did.
    //
    // It is also the non-vacuity every gate below rests on. If a fresh shell were
    // pinned, "the pin draws one file" would be true of the tool with the feature
    // deleted.
    let scratch = fixture("shell-single-default");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    // One row short of the first file's block, so the screen must straddle the
    // boundary into the second file.
    app.apply(Action::Scroll(SPAN as isize - 1), &mut frame, body())
        .expect("apply");
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());

    assert!(
        files_on(&view).len() > 1,
        "an untouched shell drew one file's rows on a screen that spans two, so \
         the diff is pinned before anybody asked"
    );
}

#[test]
fn the_pin_draws_no_row_of_any_other_file() {
    // The ruling's central claim, swept over every offset inside the pinned file
    // rather than sampled: the failure worth catching is a walk that stops at the
    // file's end for most positions and carries on for one of them.
    let scratch = fixture("shell-single-bounds");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = pinned(&mut frame);

    for step in 0..SPAN + 4 {
        let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
        assert_eq!(
            files_on(&view),
            vec![PINNED],
            "at scroll step {step} the pinned pane drew rows of {:?}",
            files_on(&view)
        );
        app.apply(Action::Scroll(1), &mut frame, body())
            .expect("apply");
    }
}

#[test]
fn scrolling_stops_at_both_ends_of_the_pinned_file() {
    // **Both ends, because they are two different pieces of code.** Down is the
    // walk refusing to advance, and is resolved in `View::collect`; up is
    // `App::up` refusing to step into the file above, and is resolved before the
    // walk runs at all. A gate on one direction says nothing about the other.
    let scratch = fixture("shell-single-clamp");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = pinned(&mut frame);

    // Far past the end, in one step and then in many, because a clamp that
    // saturates and a clamp that steps are different bugs.
    for step in [SPAN as isize * 3, 1, 1, 1] {
        app.apply(Action::Scroll(step), &mut frame, body())
            .expect("apply");
        let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
        assert_eq!(
            view.top,
            Position {
                file: PINNED,
                row: SPAN - body(),
            },
            "scrolling past the end of a pinned file did not rest its last row \
             on the bottom"
        );
        assert_eq!(
            view.rows.len(),
            body(),
            "the clamped screen is not full, so rows were lost rather than held"
        );
    }

    // And back up past the start.
    for step in [SPAN as isize * 3, 1, 1, 1] {
        app.apply(Action::Scroll(-step), &mut frame, body())
            .expect("apply");
        let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
        assert_eq!(
            view.top,
            Position {
                file: PINNED,
                row: 0
            },
            "scrolling above a pinned file left it, so the pin holds in one \
             direction only"
        );
    }
}

#[test]
fn the_two_end_keys_reach_the_ends_of_this_file() {
    // B16's one argued ruling. `g` and `G` keep meaning *the ends of what you can
    // scroll to*, and the pin is what decides the ends of what; the failure this
    // covers is either of them reaching for the changed set's ends and taking the
    // pin with it, which is the one thing the gesture removes from everything
    // that is not `n`, `p`, a digit or a click.
    let scratch = fixture("shell-single-ends");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = pinned(&mut frame);

    app.apply(Action::Scroll(5), &mut frame, body())
        .expect("apply");
    app.apply(Action::Top, &mut frame, body()).expect("apply");
    assert_eq!(
        draw(&mut app, &mut frame, &mut highlighter, &history, split()).top,
        Position {
            file: PINNED,
            row: 0
        },
        "`g` under a pin left for the first changed file"
    );

    app.apply(Action::Bottom, &mut frame, body())
        .expect("apply");
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert_eq!(
        view.top,
        Position {
            file: PINNED,
            row: SPAN - body(),
        },
        "`G` under a pin did not reach the pinned file's last screenful"
    );
    // **The last row is content**, which is `SPEC.md` §11.1's rule for the bottom
    // of the diff applied to the bottom of a pinned file: the blank closing the
    // block separates it from a file the pin does not draw, so it is not a row a
    // reader can reach.
    assert!(
        !matches!(view.rows.last(), Some(Row::Gap)),
        "the bottom of a pinned file is a blank rather than content"
    );
}

#[test]
fn the_total_is_this_file_and_a_drag_lands_where_the_thumb_says() {
    // **One gate rather than two, because they are one contract.** The bar is
    // drawn from `total_rows` and a drag has to invert the same arithmetic; a
    // drag resolved against the whole diff agrees with the thumb at both ends of
    // the track and nowhere in between, so the ends cannot tell them apart and
    // only the middle can.
    let scratch = fixture("shell-single-bar");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = pinned(&mut frame);

    let measured = Body::diff_only(body());
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, measured);
    assert_eq!(
        view.total_rows, SPAN,
        "the pinned bar is scaled against the changed set rather than the file"
    );
    assert_eq!(
        view.rows_above, 0,
        "the pinned bar counts rows above it that belong to other files"
    );

    // The middle of the track, which is the only place the two mappings differ.
    app.apply(Action::DiffTo(TRACK_SCALE / 2), &mut frame, body())
        .expect("apply");
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, measured);
    assert_eq!(
        view.top,
        Position {
            file: PINNED,
            row: (SPAN - body()) / 2,
        },
        "a drag on a pinned bar resolved against something other than the file \
         the thumb was drawn from"
    );
    assert_eq!(
        view.rows_above,
        (SPAN - body()) / 2,
        "the readout and the gesture disagree about where the drag landed"
    );
}

#[test]
fn n_p_a_digit_and_a_click_still_change_the_file() {
    // The other half of the ruling: the pin takes the file away from *scrolling*
    // and leaves it with the list. A pin that also froze these would be a mode,
    // which is what B4 refuses and what B16 claims not to be.
    let scratch = fixture("shell-single-files");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = pinned(&mut frame);

    for (action, want) in [
        (Action::File(1), PINNED + 1),
        (Action::File(-1), PINNED),
        (Action::ListRow(0), 0),
        (Action::ListRow(3), 3),
    ] {
        app.apply(action, &mut frame, body()).expect("apply");
        let view = draw(&mut app, &mut frame, &mut highlighter, &history, listed());
        assert_eq!(
            view.top,
            Position { file: want, row: 0 },
            "{action:?} did not move the pin to file {want}"
        );
        assert_eq!(
            files_on(&view),
            vec![want],
            "the pin followed the gesture and then drew somebody else's rows"
        );
    }

    // **And the click through the real hit-test**, which is what `list.rs` does
    // for the unpinned case and is the half constructing an `Action` cannot
    // reach: the pin narrows what the *diff* draws, and a hit-test that read the
    // drawn view could have narrowed the region the click lands in with it.
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, listed());
    let area = Rect::new(0, 0, 80, 24);
    let laid = regions(
        area,
        &app.chrome("fixture", None, None, None, None, None),
        &view,
    );
    assert!(
        laid.list.rows > 1,
        "no list region was published to click on, so the pinned pane lost the map"
    );
    let click = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        // Not the bar's column, which is a drag and is resolved first.
        column: 2,
        row: laid.list.top + 1,
        modifiers: KeyModifiers::NONE,
    });
    let action = action_for(&click, laid).expect("a click on a listed row is an action");
    assert_eq!(action, Action::ListRow(1));
    app.apply(action, &mut frame, body()).expect("apply");
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, listed());
    assert_eq!(
        view.top,
        Position { file: 1, row: 0 },
        "a click on a listed file did not move the pinned diff to it"
    );
    assert_eq!(
        files_on(&view),
        vec![1],
        "the click moved the pin and then drew somebody else's rows"
    );
}

#[test]
fn follow_still_moves_between_files_while_it_is_pinned() {
    // Ruled rather than derived: following is an explicit request to be moved,
    // where the pin is about what a reader's own scrolling reaches. The pairing
    // is the one the gesture is most useful in, so a pin that froze follow would
    // take the feature away from its own best case.
    let scratch = fixture("shell-single-follow");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    // **Pinned without a manual scroll**, which is the setup and half the claim.
    // `pinned` above reaches its file with `n`, and `n` is a manual scroll that
    // disengages follow by §11.1's own rule, so building this case that way would
    // have tested a shell that was not following at all.
    let mut app = App::new();
    app.apply(Action::ToggleSingle, &mut frame, body())
        .expect("apply");
    assert!(
        app.following(),
        "`s` disengaged follow, so the pin is a manual scroll and a reader who \
         pins loses the mode the pin is most useful beside"
    );

    let target = frame.files()[FILES - 1].path.clone();
    assert!(
        app.follow(&target, &frame),
        "follow did not move the pinned viewport, so `f` is the gesture this \
         ruling took away"
    );
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert_eq!(
        view.top.file,
        FILES - 1,
        "a follow under the pin did not reach the file that changed"
    );
    assert_eq!(
        files_on(&view),
        vec![FILES - 1],
        "the pin let go when follow moved it"
    );
}

#[test]
fn toggling_the_pin_returns_the_screen_it_started_from() {
    // **Two claims, and the second is what the first costs.**
    //
    // From a screen already inside one file, one on-and-off pair is exactly
    // identity: nothing needed clamping, so nothing was rewritten.
    //
    // From a screen straddling two files it cannot be, and the ruling says so out
    // loud rather than leaving it to be discovered. The pin has to rest the
    // file's last row on the bottom, which rewrites the position, so the straddle
    // is gone. What is promised there is that it settles: **every pair after the
    // first is identity**, so the toggle is not a ratchet that walks the reader
    // up the file one press at a time.
    let scratch = fixture("shell-single-toggle");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = App::new();

    // Inside one file: identity on the first pair.
    app.apply(Action::File(PINNED as isize), &mut frame, body())
        .expect("apply");
    app.apply(Action::Scroll(3), &mut frame, body())
        .expect("apply");
    let before = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    app.apply(Action::ToggleSingle, &mut frame, body())
        .expect("apply");
    let _ = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    app.apply(Action::ToggleSingle, &mut frame, body())
        .expect("apply");
    let after = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert_eq!(
        (after.top, after.rows.len()),
        (before.top, before.rows.len()),
        "a pin and an unpin moved a screen that was already inside one file"
    );

    // Straddling two: the first pair settles it, and the rest are identity.
    app.apply(Action::Scroll(SPAN as isize - 4), &mut frame, body())
        .expect("apply");
    let straddle = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert!(
        files_on(&straddle).len() > 1,
        "the straddle case does not straddle, so the settling claim is vacuous"
    );

    app.apply(Action::ToggleSingle, &mut frame, body())
        .expect("apply");
    let _ = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    app.apply(Action::ToggleSingle, &mut frame, body())
        .expect("apply");
    let settled = draw(&mut app, &mut frame, &mut highlighter, &history, split());

    for round in 0..3 {
        app.apply(Action::ToggleSingle, &mut frame, body())
            .expect("apply");
        let _ = draw(&mut app, &mut frame, &mut highlighter, &history, split());
        app.apply(Action::ToggleSingle, &mut frame, body())
            .expect("apply");
        let again = draw(&mut app, &mut frame, &mut highlighter, &history, split());
        assert_eq!(
            (again.top, again.rows.len()),
            (settled.top, settled.rows.len()),
            "toggle pair {round} after the first moved the screen again, so the \
             gesture is a ratchet rather than a toggle"
        );
    }
}

#[test]
fn a_pinned_file_shorter_than_the_pane_walks_once() {
    // **The treadmill guard, counted rather than looked at.** `View::collect`
    // backs a short screen up so its last row rests on the bottom, and skips that
    // when the position is already the first one the walk can reach. Unpinned
    // that is `Position::default()`; pinned it is the pinned file's own row zero,
    // and reading it literally makes every frame on a short pinned file restart
    // the walk: three walks and six `Frame::diff` calls against two, forever, on
    // the file an agent is writing to.
    //
    // Nothing on screen can see it. Both walks resolve to the same position and
    // draw the same rows, so the only instrument is the frame's own read count.
    let scratch = Scratch::large_diff("shell-single-treadmill", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = pinned(&mut frame);

    // Anchored, which is what arms the back-up at all: a jump does not want one.
    app.apply(Action::Scroll(1), &mut frame, body())
        .expect("apply");
    app.apply(Action::Scroll(-1), &mut frame, body())
        .expect("apply");

    let short = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert!(
        short.rows.len() < body(),
        "the fixture's file fills the pane, so this gate is not looking at a \
         short screen at all"
    );
    assert_eq!(
        short.read, 1,
        "a pinned file shorter than the pane asked the frame for {} files, so the \
         walk restarted on a position it had already resolved",
        short.read
    );
}

#[test]
fn an_unpinned_frame_is_unchanged_by_the_field_existing() {
    // **The identity half of "off by default".** Every other gate in the suite
    // asserts the unpinned pane's behaviour; this asserts that the *route* is the
    // same one, by collecting the same viewport twice and differing only in the
    // flag that is supposed to change nothing when it is false.
    //
    // It is deliberately not a comparison against a recorded screen: those exist
    // in `tests/render.rs` and they are what would catch a drawn difference. What
    // this catches is a `Viewport::default()` that stopped meaning *the ordinary
    // frame*.
    let scratch = fixture("shell-single-identity");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let asked = Viewport {
        position: Position {
            file: PINNED,
            row: SPAN - 2,
        },
        anchored: true,
        diff_rows: body(),
        measured: true,
        ..Viewport::default()
    };
    assert!(
        !asked.single,
        "`Viewport::default()` pins the diff, so every caller that omits the \
         field asks for a screen it did not mean"
    );
    let plain = View::collect(&mut frame, &mut highlighter, &history, asked).expect("view");
    let again = View::collect(
        &mut frame,
        &mut highlighter,
        &history,
        Viewport {
            single: false,
            ..asked
        },
    )
    .expect("view");
    assert_eq!(
        (plain.top, plain.rows.len(), plain.total_rows),
        (again.top, again.rows.len(), again.total_rows),
        "spelling the flag out changed the unpinned frame"
    );
    assert!(
        plain.total_rows > SPAN,
        "the unpinned total is one file's, so the pin is on when nobody asked"
    );
}

#[test]
fn a_pinned_gesture_survives_the_diff_it_was_made_against() {
    // **The panic the pin reaches that no other gesture does.** `G` and a drag on
    // the diff's bar are the only two gestures that ask the frame how tall a file
    // is *before* `View::collect` has clamped the position against the files that
    // exist, and under a pin both of them do: unpinned, `G` is
    // `jump_to(len - 1)`, which saturates on an empty list and touches no frame,
    // and a drag walks the list it is iterating.
    //
    // Two shapes, and the second is the one that matters most because it is not
    // an edge at all. A **clean worktree** is the state a monitor sits in most of
    // the time, so `s` and then `G` on a pane somebody left open is the ordinary
    // use of this tool, and `Frame::diff` panics on an index into an empty list by
    // design. The first shape is the agent in the other pane committing its work,
    // which renumbers the changed set under a position nobody touched.
    let scratch = fixture("shell-single-shrink");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = pinned(&mut frame);

    // Pinned to the last file, so the shrink below has somewhere to strand it.
    // Stepped rather than jumped, because `G` under a pin is one of the two
    // gestures this gate is about and using it as setup would be assuming the
    // thing under test.
    app.apply(
        Action::File((FILES - 1 - PINNED) as isize),
        &mut frame,
        body(),
    )
    .expect("apply");
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert_eq!(
        view.top.file,
        FILES - 1,
        "the fixture never pinned the last file, so the shrink strands nothing"
    );

    // Half the changes go away, the way an agent committing does it.
    for index in (FILES / 2)..FILES {
        scratch.git(&["checkout", "--", &format!("src/mod_{index}.rs")]);
    }
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), FILES / 2, "the fixture did not shrink");

    for action in [
        Action::Bottom,
        Action::Top,
        Action::DiffTo(TRACK_SCALE / 2),
        Action::Scroll(3),
        Action::Scroll(-3),
    ] {
        app.apply(action, &mut frame, body())
            .unwrap_or_else(|error| panic!("{action:?} after the shrink: {error}"));
        let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
        assert!(
            view.top.file < FILES / 2,
            "{action:?} left the pin on file {}, which is past the {} that exist",
            view.top.file,
            FILES / 2
        );
    }

    // And all the way to nothing, which is the ordinary state rather than the
    // edge: a monitor left open beside an agent that has committed.
    scratch.commit_all("everything");
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), 0, "the worktree is not clean");

    for action in [
        Action::Bottom,
        Action::Top,
        Action::DiffTo(TRACK_SCALE / 2),
        Action::Scroll(1),
        Action::Scroll(-1),
        Action::File(1),
        Action::ListRow(0),
        Action::ToggleSingle,
    ] {
        app.apply(action, &mut frame, body())
            .unwrap_or_else(|error| panic!("{action:?} on a clean worktree: {error}"));
        let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
        assert!(
            view.rows.is_empty(),
            "a clean worktree drew diff rows after {action:?}"
        );
    }
}
