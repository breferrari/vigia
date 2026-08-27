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

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use vigia::{
    Action, App, Body, Glyphs, Pointing, Position, Regions, Row, TRACK_SCALE, Theme, View,
    Viewport, action_for, body_layout, diff_height, regions, render,
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
        &App::new().chrome("fixture", None, Pointing::default(), 0, ""),
        FILES,
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
        &App::new().chrome("fixture", None, Pointing::default(), 0, ""),
        FILES,
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

/// Every file index a screen draws rows of.
///
/// The oracle for *no row of another file is ever drawn*. Counting headings
/// alone would miss a screen resting deep inside the wrong file, which draws no
/// heading at all, so the file the top is in is counted whether or not its
/// heading is on screen.
///
/// **[`View::shown_files`] is that rule and this reads it** rather than counting
/// headings again here. It is `pub`, exported, and documented for exactly this
/// question, and until now it had no caller outside `view.rs` and no gate at
/// all: a second copy of the rule here would have been the one under test while
/// the shipped one stayed unexercised. The files a screen draws are contiguous
/// from its top, which is what makes the count an index range.
fn files_on(view: &View) -> Vec<usize> {
    (view.top.file..view.top.file + view.shown_files()).collect()
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

    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
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
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
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
        // **Scrolled off the new file's end before the pin is read**, which is
        // what makes the second assertion a claim. Every one of these gestures
        // lands on row zero of a file taller than the body, so a screen drawn
        // from there shows one file whether or not anything is pinned, so
        // asserting that alone is green with the feature deleted.
        app.apply(Action::Scroll(SPAN as isize), &mut frame, body())
            .expect("apply");
        let view = draw(&mut app, &mut frame, &mut highlighter, &history, listed());
        assert_eq!(
            files_on(&view),
            vec![want],
            "the pin followed {action:?} to file {want} and then drew somebody \
             else's rows"
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
        &app.chrome("fixture", None, Pointing::default(), 0, ""),
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

    // **A middle file, not the last**, and that is the whole non-vacuity of the
    // second assertion. Following the last file draws one file whether or not
    // anything is pinned, because there is nothing after it to spill into, so a
    // gate on the last file is green with the feature deleted.
    const FOLLOWED: usize = 1;
    // In a `const` block, which is this repo's idiom for a claim about constants:
    // a bad one is then a build that will not compile rather than a suite that
    // goes red, and it is what `render.rs` already does for `SECTIONS`' bounds.
    const _: () = assert!(
        FOLLOWED + 1 < FILES,
        "the followed file is the last, so `files_on` proves nothing"
    );
    let target = frame.files()[FOLLOWED].path.clone();
    assert!(
        app.follow(&target, &frame),
        "follow did not move the pinned viewport, so `f` is the gesture this \
         ruling took away"
    );
    // Scrolled to the followed file's end, so an unpinned walk would be showing
    // the file below it by the time `files_on` is read.
    app.apply(Action::Scroll(SPAN as isize), &mut frame, body())
        .expect("apply");
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert_eq!(
        view.top.file, FOLLOWED,
        "a follow under the pin did not reach the file that changed, or scrolling \
         off its end left it"
    );
    assert_eq!(
        files_on(&view),
        vec![FOLLOWED],
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
    // **The middle draw is observed rather than discarded**, and until #297's
    // audit it was `let _ = draw(...)`. That made the whole gate green with the
    // feature deleted: it asserted only that doing nothing twice does nothing.
    // What has to be true is that the pin was *on* in between and drew the pinned
    // screen, and that the unpin then gave the other files back.
    let pinned = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert_eq!(
        files_on(&pinned),
        vec![PINNED],
        "the middle of the toggle pair drew more than the pinned file, so the \
         identity below is between two unpinned screens"
    );
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
    let clamped = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert_eq!(
        files_on(&clamped),
        vec![PINNED],
        "the pin did not take the straddle down to one file"
    );
    assert_eq!(
        clamped.top,
        Position {
            file: PINNED,
            row: SPAN - body(),
        },
        "the pin did not rest the straddled file's last row on the bottom"
    );
    app.apply(Action::ToggleSingle, &mut frame, body())
        .expect("apply");
    let settled = draw(&mut app, &mut frame, &mut highlighter, &history, split());

    for round in 0..3 {
        app.apply(Action::ToggleSingle, &mut frame, body())
            .expect("apply");
        let between = draw(&mut app, &mut frame, &mut highlighter, &history, split());
        assert_eq!(
            files_on(&between),
            vec![PINNED],
            "toggle pair {round} did not pin anything, so the identity below is \
             between two unpinned screens"
        );
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
        wrap: false,
        width: 0,
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
    assert!(
        plain.total_rows > SPAN,
        "the unpinned total is one file's, so the pin is on when nobody asked"
    );

    // **And the same viewport with the flag set differs**, which is what makes
    // the assertion above a claim rather than a tautology. Collecting
    // `Viewport { single: false, ..asked }` and comparing it with `asked`
    // compares one value with itself: it asserts that `collect` is
    // deterministic and would have stayed green with the flag ignored entirely.
    let pinned = View::collect(
        &mut frame,
        &mut highlighter,
        &history,
        Viewport {
            single: true,
            ..asked
        },
    )
    .expect("view");
    assert_eq!(
        pinned.total_rows, SPAN,
        "the pinned total is not the pinned file's, so the flag reached nothing"
    );
    assert_ne!(
        (plain.top, plain.total_rows),
        (pinned.top, pinned.total_rows),
        "one viewport collected two ways gave one answer, so the flag is inert"
    );
}

#[test]
fn a_pinned_gesture_survives_the_diff_it_was_made_against() {
    // **The panic the pin reaches.** `G` and a drag on the diff's bar are two of
    // the three gestures that ask the frame how tall a file is *before*
    // `View::collect` has clamped the position against the files that exist, and
    // under a pin both of them do. **The third is `App::up`'s walk back**, which
    // had the same latent panic and is gated by
    // `tests/scroll.rs::a_walk_back_survives_the_file_it_pointed_into_disappearing`.
    // Saying *the only two* here is the claim that keeps the third unfound.
    // Unpinned, `G` is
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

#[test]
fn s_is_what_asks_for_one_file_and_s_is_what_gives_the_diff_back() {
    // **The binding itself, through the real key resolution.** Every other gate
    // in this file constructs `Action::ToggleSingle`, so deleting the
    // `KeyCode::Char('s')` arm left the entire workspace green: B16 could have
    // shipped with a gesture nothing could reach from a keyboard. #295 closed
    // exactly this hole for `r` and it reopened here, which is why the check is
    // the action's *resolution* rather than its existence.
    //
    // `Regions::default()` deliberately: a key is a key, so the hit-test that
    // resolves a pointer has nothing to say about it.
    use ratatui::crossterm::event::{KeyCode, KeyEvent};

    let press = |key: char| Event::Key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
    assert_eq!(
        action_for(&press('s'), Regions::default()),
        Some(Action::ToggleSingle),
        "`s` resolves to no action, so nothing on a keyboard reaches the pin"
    );

    // And it toggles rather than latching, read off the screen rather than off
    // the state, because the state is private and the screen is the promise.
    let scratch = fixture("shell-single-key");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = App::new();

    app.apply(Action::File(PINNED as isize), &mut frame, body())
        .expect("apply");
    app.apply(Action::Scroll(SPAN as isize - 4), &mut frame, body())
        .expect("apply");
    let straddle = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert!(
        files_on(&straddle).len() > 1,
        "the fixture does not straddle, so the press below cannot be seen to work"
    );

    // **The total rather than the drawn file count, because the count cannot see
    // the second press.** The first `s` rests the straddled file's last row on
    // the bottom, and from there the file has exactly a screenful left, so an
    // *unpinned* walk from the same position also draws one file. What separates
    // them is what the bar is scaled against, which is the whole changed set
    // again the moment the pin comes off.
    let action = action_for(&press('s'), Regions::default()).expect("an action");
    app.apply(action, &mut frame, body()).expect("apply");
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert_eq!(
        files_on(&view),
        vec![PINNED],
        "the first `s` did not pin the diff to one file"
    );
    assert_eq!(
        view.total_rows, SPAN,
        "the first `s` left the bar scaled against the changed set"
    );

    let action = action_for(&press('s'), Regions::default()).expect("an action");
    app.apply(action, &mut frame, body()).expect("apply");
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert!(
        view.total_rows > SPAN,
        "the second `s` did not give the rest of the diff back: the bar still \
         measures {} rows against the pinned file's {SPAN}",
        view.total_rows
    );
}

#[test]
fn a_pinned_end_key_and_a_scroll_in_one_wake_both_move() {
    // **The shell drains actions in a batch and paints once at the end of it**,
    // so `G` and a held `k` arrive together with no frame between them. `G` under
    // a pin writing the pinned file's whole height and letting `View::collect`
    // clamp it on the way to the screen draws the right rows and leaves a
    // *position* nothing can move from: every `k` in the same batch walked the
    // row down from `span` and every one of them still clamped to the same
    // screen. Nine keystrokes swallowed on this fixture at this pane.
    //
    // Unpinned the case cannot arise, because `G` there is `jump_to`, which
    // resolves to row zero. So this is the pin's own defect and it needs the
    // pin's own gate.
    let scratch = fixture("shell-single-batch");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = pinned(&mut frame);

    // One batch: no `App::view` between the two, which is what production does.
    app.apply(Action::Bottom, &mut frame, body())
        .expect("apply");
    app.apply(Action::Scroll(-1), &mut frame, body())
        .expect("apply");
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());

    assert_eq!(
        view.top,
        Position {
            file: PINNED,
            row: SPAN - body() - 1,
        },
        "a `k` batched with `G` moved nothing, so the reader presses a key up to \
         {} times before the screen answers",
        SPAN - body()
    );
}

#[test]
fn the_first_file_pins_like_any_other() {
    // **Where `first` coincides with the unpinned bound**, so a walk that ignored
    // the pin entirely would be indistinguishable here on the position alone.
    // What separates them is the file below: unpinned, a screen scrolled to the
    // end of file zero spills into file one.
    let scratch = fixture("shell-single-first");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = App::new();

    app.apply(Action::ToggleSingle, &mut frame, body())
        .expect("apply");
    app.apply(Action::Scroll(SPAN as isize * 2), &mut frame, body())
        .expect("apply");
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());

    assert_eq!(files_on(&view), vec![0], "the pin on file zero let go");
    assert_eq!(
        view.top,
        Position {
            file: 0,
            row: SPAN - body(),
        },
        "scrolling past the end of a pinned first file did not clamp to it"
    );
}

#[test]
fn a_pinned_pane_draws_and_its_bar_is_the_files() {
    // **Nothing in the suite drew a pinned view until #297's audit asked.** Every
    // other gate here reads a `View`, and a `View` is not a screen: the diff's
    // scrollbar, the row wash, the caret and the rail are all the painter's, and
    // a pin that produced a correct `View` and a broken screen would have been
    // invisible to all of them.
    let scratch = fixture("shell-single-drawn");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = pinned(&mut frame);

    let at = Rect::new(0, 0, 80, 24);
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let laid = body_layout(at, &chrome, FILES, FILES);
    let view = app
        .view(&mut frame, &mut highlighter, &history, laid)
        .expect("view");
    let mut buf = Buffer::empty(at);
    render(
        &mut buf,
        at,
        &view,
        &Theme::default(),
        Glyphs::default(),
        &chrome,
    );

    // **Inside the diff's own rows, not anywhere on the pane.** The first
    // spelling asserted `drawn.contains(&pinned_path)` over the whole buffer, and
    // the pinned path is also in the *list*, which the same pane draws: deleting
    // every diff row from the painter left it green. The region is what makes it
    // a claim about the diff.
    let laid_regions = regions(at, &chrome, &view);
    let diff_rows: String = (laid_regions.diff.top..laid_regions.diff.top + laid_regions.diff.rows)
        .map(|row| {
            (0..at.width)
                .map(|col| buf[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let pinned_path = frame.files()[PINNED].path.clone();
    assert!(
        laid_regions.diff.rows > 0,
        "the pane published no diff region, so there is nothing to read"
    );
    assert!(
        diff_rows.contains(&pinned_path),
        "the pinned file's heading is not among the diff's own rows:\n{diff_rows}"
    );
    for (index, file) in frame.files().iter().enumerate() {
        if index != PINNED {
            assert!(
                !diff_rows.contains(&file.path),
                "the drawn diff carries {:?}, which is not the pinned file",
                file.path
            );
        }
    }
    // Every other changed file is still in the **list**, which is the map the pin
    // leaves alone, so the drawn pane naming them is not a failure. What must not
    // be there is another file's *diff heading*, and the view is the oracle for
    // that.
    assert_eq!(
        files_on(&view),
        vec![PINNED],
        "the drawn pane's diff reached past the pinned file"
    );
    assert!(
        view.list.len() > 1,
        "the pinned pane drew no map, so the reader has no way to change file"
    );
    assert_eq!(
        view.total_rows, SPAN,
        "the drawn bar is scaled against something other than the pinned file"
    );
}

#[test]
fn the_pin_and_the_rail_do_not_fight() {
    // Two view gestures that both change what the body is made of. `r` moves the
    // map beside the diff and `s` narrows what the diff may reach, so neither
    // should touch the other; nothing in the suite had asked.
    let scratch = fixture("shell-single-rail");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = pinned(&mut frame);

    // A pane wide enough to honour the rail, which is 134 columns since #252.
    let at = Rect::new(0, 0, 160, 30);
    app.apply(Action::ToggleRail, &mut frame, body())
        .expect("apply");
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let laid = body_layout(at, &chrome, FILES, FILES);
    assert!(
        laid.rail,
        "the pane did not take the rail, so this gate is vacuous"
    );

    let view = app
        .view(&mut frame, &mut highlighter, &history, laid)
        .expect("view");
    assert_eq!(
        files_on(&view),
        vec![PINNED],
        "the rail let the pinned diff reach another file"
    );
    assert!(
        view.list.len() > 1,
        "the rail drew no map beside the pinned diff"
    );
}

#[test]
fn a_pane_with_no_room_pins_without_panicking() {
    // Degenerate shapes, which `View::collect` is public for and which
    // `tests/list.rs`'s own grid sweeps with the pin off. A body of zero rows
    // resolves nothing and must still answer; a body of one row is the floor
    // `Body::split` gives the diff, and it is where an off-by-one in the pinned
    // range would land.
    let scratch = fixture("shell-single-degenerate");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    for diff_rows in [0usize, 1, 2] {
        for list_rows in [0usize, 1, 4] {
            let view = View::collect(
                &mut frame,
                &mut highlighter,
                &history,
                Viewport {
                    position: Position {
                        file: PINNED,
                        row: SPAN * 2,
                    },
                    anchored: true,
                    diff_rows,
                    list_rows,
                    measured: true,
                    single: true,
                    ..Viewport::default()
                },
            )
            .expect("a degenerate pinned viewport still collects");

            assert!(
                view.rows.len() <= diff_rows,
                "a {diff_rows}-row body drew {} rows",
                view.rows.len()
            );
            assert!(
                view.list.len() <= list_rows,
                "a {list_rows}-row list drew {} rows",
                view.list.len()
            );
            if diff_rows > 0 {
                assert_eq!(
                    view.top.file, PINNED,
                    "a {diff_rows}-row pinned body resolved onto another file"
                );
                assert_eq!(
                    view.total_rows, SPAN,
                    "a {diff_rows}-row pinned body measured something other than \
                     the pinned file"
                );
            }
        }
    }
}

#[test]
fn a_page_and_a_half_page_clamp_at_the_pinned_files_ends() {
    // `Page` and `HalfPage` go through `App::step_by` into `App::scroll`, which
    // is the path `scrolling_stops_at_both_ends_of_the_pinned_file` already
    // covers a row at a time. They are gated separately because the step is a
    // *screen* rather than a row, so a clamp that happened to be right for one
    // row could still overshoot for a page, and because the two keys are what a
    // reader actually holds.
    let scratch = fixture("shell-single-pages");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = pinned(&mut frame);

    for down in [Action::Page(1), Action::HalfPage(1)] {
        for _ in 0..6 {
            app.apply(down, &mut frame, body()).expect("apply");
        }
        let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
        assert_eq!(
            view.top,
            Position {
                file: PINNED,
                row: SPAN - body(),
            },
            "{down:?} past the end of a pinned file did not clamp to it"
        );
        assert_eq!(
            files_on(&view),
            vec![PINNED],
            "{down:?} left the pinned file"
        );
    }

    for up in [Action::Page(-1), Action::HalfPage(-1)] {
        for _ in 0..6 {
            app.apply(up, &mut frame, body()).expect("apply");
        }
        let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
        assert_eq!(
            view.top,
            Position {
                file: PINNED,
                row: 0
            },
            "{up:?} above a pinned file left it"
        );
    }
}

#[test]
fn a_pinned_end_key_rests_on_the_bottom_at_every_width() {
    // **The height `G` subtracts is measured before `apply` runs, and `apply`
    // changes it.** `Shell::diff_rows_for` builds a `Chrome` to size the body,
    // then `App::apply` turns follow off because `Bottom` is a manual scroll, and
    // `Footer::plan` sizes its rungs from `Chrome::following`: `follow ▶  N/M` is
    // thirteen columns against `N/M`'s three. On a pane narrow enough for that to
    // decide between a one-line and a two-line footer, the region drawn is taller
    // than the region `span - height` was taken against, so the file's last row
    // rests above the bottom with a blank under it.
    //
    // **And it persists**, which is what makes it worth a sweep rather than a
    // note: `App::view` writes the resolved position back every frame, so the
    // screen stays wrong until the reader scrolls. That is
    // [#57](https://github.com/breferrari/vigia/issues/57)'s symptom on the arm
    // written to avoid it.
    //
    // Swept rather than pinned at the one width that flips, because which width
    // that is falls out of the footer's own ladder and would be a number this
    // gate had to keep in step with `Footer::plan`. What is asserted is the
    // property at every width the sweep covers: after `G`, a pinned file taller
    // than the body fills it.
    let scratch = fixture("shell-single-rest");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();

    let mut narrow = 0;
    for width in 30..=90u16 {
        let at = Rect::new(0, 0, width, 24);
        let mut app = App::new();
        // Following, which is the shipped default and the whole of the
        // staleness: `App::new` engages it and `Bottom` is what turns it off.
        assert!(app.following(), "the fixture is not following");
        // **Pinned and moved without a manual scroll**, which is the whole setup:
        // `Action::File` would have reached the file and disengaged follow on the
        // way, so the chrome sized below would already have been the one `Bottom`
        // produces and the staleness could not arise. `s` is not a manual scroll
        // and a follow is a request to be moved, so both leave follow engaged.
        app.apply(Action::ToggleSingle, &mut frame, 0)
            .expect("apply");
        let target = frame.files()[PINNED].path.clone();
        assert!(
            app.follow(&target, &frame),
            "the follow did not move the pin"
        );
        assert!(
            app.following(),
            "the setup disengaged follow before `G` could"
        );

        // Exactly what the shell does: size the body from the chrome as it
        // stands, apply, then lay out and draw from the chrome as it ends up.
        let before = app.chrome("fixture", None, Pointing::default(), 0, "");
        let height = body_layout(at, &before, FILES, FILES).diff;
        app.apply(Action::Bottom, &mut frame, height)
            .expect("apply");

        let after = app.chrome("fixture", None, Pointing::default(), 0, "");
        let laid = body_layout(at, &after, FILES, FILES);
        if laid.diff == 0 || laid.diff >= SPAN {
            continue;
        }
        // **Counted after the skips, not before.** Counting first lets a width
        // that the assertion never reaches satisfy the non-vacuity guard below,
        // which is a guard certifying a case the gate may have skipped. Neither
        // skip fires on this fixture today; the ordering is what keeps that from
        // being load-bearing.
        if laid.diff != height {
            narrow += 1;
        }
        let view = app
            .view(&mut frame, &mut highlighter, &history, laid)
            .expect("view");
        assert_eq!(
            view.rows.len(),
            laid.diff,
            "at {width} columns the pinned end key left {} of {} rows drawn, so \
             the file's last row is not on the bottom",
            view.rows.len(),
            laid.diff
        );
        assert_eq!(
            files_on(&view),
            vec![PINNED],
            "at {width} columns the end key left the pinned file"
        );
    }

    assert!(
        narrow > 0,
        "no width in the sweep changed its body height when follow went off, so \
         this gate never reaches the case it is named for"
    );
}

#[test]
fn a_landing_owed_to_follow_resolves_inside_the_pinned_file() {
    // **The pairing B16 calls the gesture's best case, and nothing exercised
    // it.** Follow chooses the file an agent is writing and the pin keeps the diff
    // on it, so a landing owed by `App::follow` resolving *inside* a pinned walk
    // is the ordinary state of this feature, not an edge, and it is easily left
    // with no test anywhere setting `single` with a landing owed:
    // `follow_still_moves_between_files_while_it_is_pinned` clears it with a
    // scroll before it draws, `tests/follow.rs` never pins, and `tests/list.rs`'s
    // degenerate grid pins `single: false` by name.
    //
    // Two things have to hold, and they are on opposite sides of the walk.
    // `landing_of` has to place the viewport at the busiest hunk of the pinned
    // file ([#257](https://github.com/breferrari/vigia/issues/257)), and the
    // `landed_inside` restart has to back a short screen up against the *pinned*
    // floor rather than the diff's, which is the bound whose earlier reading was
    // this ruling's recorded defect.
    //
    // **A sparse fixture rather than this file's usual one**, and that is what
    // makes the landing a landing. `landing_of` keeps the heading whenever the
    // busiest hunk is already drawn from it, which is the right answer and is
    // what the ordinary fixture gets: one hunk starting two rows down, visible
    // from row zero. A file edited every fortieth line has hunks all the way
    // down, so the busiest is below the fold and the landing has somewhere to go.
    let scratch = Scratch::sparse_edits("shell-single-landing", FILES, 200, 40);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = App::new();

    app.apply(Action::ToggleSingle, &mut frame, body())
        .expect("apply");
    assert!(
        app.following(),
        "`s` disengaged follow, so nothing owes a landing"
    );

    // A middle file, so the landing has files on both sides to spill into if the
    // pin let go.
    const FOLLOWED: usize = 2;
    const _: () = assert!(FOLLOWED + 1 < FILES, "the followed file is the last");
    let target = frame.files()[FOLLOWED].path.clone();
    assert!(
        app.follow(&target, &frame),
        "the follow did not move the viewport"
    );

    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    let span = vigia::span_in(&mut frame, FOLLOWED).expect("span");
    assert!(
        span > body(),
        "the followed file is not taller than the body, so a landing and a \
         heading are the same row"
    );
    assert_eq!(
        files_on(&view),
        vec![FOLLOWED],
        "a landing under the pin drew rows of another file"
    );
    assert!(
        view.landed,
        "the landing was never resolved, so this gate is about a plain jump"
    );
    // **Where the landing lands is not asserted here, and that is deliberate.**
    // `landing_of` keeps the heading whenever the busiest hunk is already drawn
    // from it and moves off it only when it is not, which is
    // [#257](https://github.com/breferrari/vigia/issues/257)'s rule and
    // `tests/follow.rs`'s gate. Two fixtures were tried here before that was
    // clear: one hunk starting two rows down is visible from row zero, and hunks
    // of equal size tie to the earliest, which is also visible. Both keep the
    // heading, correctly.
    //
    // What this gate owns is the half `follow.rs` cannot see: that the request
    // was **resolved by a pinned walk at all**, that the walk did not leave the
    // file to do it, and that the restart it can trigger backs up against the
    // pinned floor rather than the diff's. `View::landed` above is the first,
    // `files_on` the second, and the full screen and the stable next frame are
    // the third.
    assert_eq!(
        view.rows.len(),
        body(),
        "the landed screen is short, so the back-up did not fill it"
    );
    assert_eq!(
        view.total_rows, span,
        "the bar under a landed pin measures the changed set"
    );

    // And the frame after it, which is where a landing that was not cleared, or a
    // restart that resolved against the diff's floor instead of the pin's, would
    // show up as the screen moving on its own.
    let settled = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert_eq!(
        (settled.top, settled.rows.len()),
        (view.top, view.rows.len()),
        "the frame after a landed pin moved with no input at all"
    );
    assert_eq!(
        files_on(&settled),
        vec![FOLLOWED],
        "the frame after a landed pin left the pinned file"
    );
}

#[test]
fn a_straddle_reached_by_a_drag_pins_to_the_bottom_too() {
    // **Every straddle in this file until now was reached with `Action::Scroll`,
    // and that hid a real defect.** `View::collect`'s back-up is gated on
    // `anchored || landed_inside`: it fires for a position a reader *scrolled* to
    // and stays quiet for one a jump placed. `Action::ToggleSingle` is not a
    // manual scroll, so it inherits whatever set the position, and `App::diff_to`
    // sets `anchored` **false**.
    //
    // So a reader who dragged the diff's bar into the middle of a tall file and
    // then pressed `s` got a short screen with trailing blanks, which jumped
    // upward on the next `j`. `SPEC.md` §11.2 B16 said the opposite in as many
    // words, and no gate could see it because every gate arrived by scrolling.
    //
    // The gate is the drag, not the scroll, and that is the whole of it.
    let scratch = fixture("shell-single-drag-pin");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = App::new();

    // Unpinned, dragged to the middle of the whole diff, which lands inside a
    // file rather than on a heading and leaves `anchored` false.
    app.apply(Action::DiffTo(TRACK_SCALE / 2), &mut frame, body())
        .expect("apply");
    let straddle = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert!(
        straddle.top.row > 0,
        "the drag landed on a heading, so the pin below has nothing to clamp"
    );
    assert_eq!(
        straddle.rows.len(),
        body(),
        "the unpinned drag already drew a short screen, so this gate would pass \
         against a pin that did nothing"
    );

    let file = straddle.top.file;
    app.apply(Action::ToggleSingle, &mut frame, body())
        .expect("apply");
    let view = draw(&mut app, &mut frame, &mut highlighter, &history, split());

    assert_eq!(
        files_on(&view),
        vec![file],
        "the pin after a drag reached another file"
    );
    assert_eq!(
        view.rows.len(),
        body(),
        "the pin after a drag left {} of {} rows drawn, so the file's last row is \
         not on the bottom and the screen has blanks under it",
        view.rows.len(),
        body()
    );
    assert_eq!(
        view.top,
        Position {
            file,
            row: SPAN - body(),
        },
        "the pin after a drag did not rest the file's last row on the bottom"
    );

    // And the next frame does not move, which is what the reader would have seen
    // as a jump upward on their first `j`.
    let settled = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert_eq!(
        (settled.top, settled.rows.len()),
        (view.top, view.rows.len()),
        "the frame after a dragged pin moved with no input"
    );
}

#[test]
fn a_jump_onto_a_short_tail_survives_being_pinned_and_unpinned() {
    // **The state that made the pin's first two licences leak.** `SPEC.md` §11.1
    // keeps a deliberate exception for a jump onto a tail shorter than the pane
    // ([#59](https://github.com/breferrari/vigia/issues/59)): the file the jump
    // was for keeps the top row and the blanks under it stay, because a jump is a
    // claim about the top and backing up would move the file off it.
    //
    // Two attempts at licensing the pin's own back-up went through `anchored`,
    // and `anchored` outlives the pin. So `n` onto a short tail, then `s` and `s`,
    // left an **unpinned** frame anchored, `View::collect` found it short, and the
    // reader was pulled out of the file they had asked for with no input at all.
    // Licensing from `single` has no state to leak, and this is the gate that
    // says so.
    //
    // Every straddle elsewhere in this file is reached with `Scroll` or `DiffTo`,
    // both of which already anchor, which is exactly why nothing saw it.
    // **A fixture of one-line files, not this file's usual tall one.** A short
    // tail means a *file shorter than the pane*, and every file in `fixture` is
    // twenty-two rows against a thirteen-row body, so a jump to the last one
    // fills the screen and there is no tail to protect. The first version of this
    // gate used it and was vacuous: the mutation that re-adds the leak survived.
    let scratch = Scratch::large_diff("shell-single-short-tail", FILES, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = App::new();

    // The last file, whose remaining rows cannot fill the pane: `SPAN` is the
    // whole file and the diff ends there, so a jump to it leaves blanks.
    app.apply(Action::Bottom, &mut frame, body())
        .expect("apply");
    let jumped = draw(&mut app, &mut frame, &mut highlighter, &history, split());
    assert_eq!(
        jumped.top,
        Position {
            file: FILES - 1,
            row: 0
        },
        "the jump did not land on the last file's heading"
    );

    // **Non-vacuity, with no escape clause.** The screen has to be genuinely
    // short or the back-up has nothing to fire on and this gate passes against
    // every licence, which is what the first version did: it carried an
    // `|| tail == body()` alternative that was true on its own fixture and made
    // the assertion unfalsifiable.
    assert!(
        jumped.rows.len() < body(),
        "the jump filled the pane with {} rows, so there is no short tail to \
         protect and this gate cannot fail",
        jumped.rows.len()
    );

    for round in 0..3 {
        app.apply(Action::ToggleSingle, &mut frame, body())
            .expect("apply");
        let pinned = draw(&mut app, &mut frame, &mut highlighter, &history, split());
        assert_eq!(
            pinned.top, jumped.top,
            "pass {round}: pinning a jump onto a short tail moved it off the top row"
        );

        app.apply(Action::ToggleSingle, &mut frame, body())
            .expect("apply");
        let after = draw(&mut app, &mut frame, &mut highlighter, &history, split());
        assert_eq!(
            after.top, jumped.top,
            "pass {round}: unpinning backed the reader out of the file the jump \
             was for, which is #59's exception undone with no input"
        );
        assert_eq!(
            after.rows.len(),
            jumped.rows.len(),
            "pass {round}: the unpinned screen changed shape"
        );
    }
}
