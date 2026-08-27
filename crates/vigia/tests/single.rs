//! `SPEC.md` §11.2 B16: the diff pinned to one file, on `s`.

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

/// Rows one file's content occupies: its heading, one hunk header, and the
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
fn files_on(view: &View) -> Vec<usize> {
    (view.top.file..view.top.file + view.shown_files()).collect()
}

#[test]
fn the_fixture_is_the_shape_the_rest_of_this_file_assumes() {
    // Every assertion below is arithmetic over SPAN and BLOCK, and the one that
    // matters most is that a file is taller than the body: with a shorter
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
    // Off by default, asserted through what a screen draws rather than
    // through a getter, because the field is private and because the ruling is
    // about the pane: a reader who has pressed nothing scrolls the whole changed
    // set exactly as every version before this one did.
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
    // Both ends, because they are two different pieces of code.
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
    // B16's one argued ruling.
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
    // The last row is content, which is `SPEC.md` §11.1's rule for the bottom
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
    // One gate rather than two, because they are one contract.
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
        // Scrolled off the new file's end before the pin is read, which is what makes
        // the second assertion a claim.
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

    // And the click through the real hit-test, which is what `list.rs` does
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
    // Ruled rather than derived: following is an explicit request to be moved, where
    // the pin is about what a reader's own scrolling reaches.
    let scratch = fixture("shell-single-follow");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    // Pinned without a manual scroll, which is the setup and half the claim.
    let mut app = App::new();
    app.apply(Action::ToggleSingle, &mut frame, body())
        .expect("apply");
    assert!(
        app.following(),
        "`s` disengaged follow, so the pin is a manual scroll and a reader who \
         pins loses the mode the pin is most useful beside"
    );

    // A middle file, not the last, and that is the whole non-vacuity of the second
    // assertion.
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
    // Two claims, and the second is what the first costs.
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
    // The treadmill guard, counted rather than looked at.
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
    // The identity half of "off by default".
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

    // And the same viewport with the flag set differs, which is what makes the
    // assertion above a claim rather than a tautology.
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
    // The panic the pin reaches.
    let scratch = fixture("shell-single-shrink");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    let mut app = pinned(&mut frame);

    // Pinned to the last file, so the shrink below has somewhere to strand it.
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
    // The binding itself, through the real key resolution.
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

    // The total rather than the drawn file count, because the count cannot see the
    // second press.
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
    // The shell drains actions in a batch and paints once at the end of it, so `G` and
    // a held `k` arrive together with no frame between them.
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
    // Where `first` coincides with the unpinned bound, so a walk that ignored the pin
    // entirely would be indistinguishable here on the position alone.
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

    // Inside the diff's own rows, not anywhere on the pane.
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
    // Every other changed file is still in the list, which is the map the pin leaves
    // alone, so the drawn pane naming them is not a failure.
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
    // `tests/list.rs`'s own grid sweeps with the pin off.
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
    // `Page` and `HalfPage` go through `App::step_by` into `App::scroll`, which is the
    // path `scrolling_stops_at_both_ends_of_the_pinned_file` already covers a row at a
    // time.
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
    // The height `G` subtracts is measured before `apply` runs, and `apply` changes it.
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
        // Pinned and moved without a manual scroll, which is the whole setup:
        // `Action::File` would have reached the file and disengaged follow on the way,
        // so the chrome sized below would already have been the one `Bottom` produces
        // and the staleness could not arise.
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
        // Counted after the skips, not before.
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
    // The pairing B16 calls the gesture's best case, and nothing exercised it.
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
    // Where the landing lands is not asserted here, and that is deliberate.
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
    // Every straddle in this file until now was reached with `Action::Scroll`,
    // and that hid a real defect. `View::collect`'s back-up is gated on
    // `anchored || landed_inside`: it fires for a position a reader *scrolled* to
    // and stays quiet for one a jump placed. `Action::ToggleSingle` is not a
    // manual scroll, so it inherits whatever set the position, and `App::diff_to`
    // sets `anchored` false.
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
    // The state that made the pin's first two licences leak.
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

    // Non-vacuity, with no escape clause. The screen has to be genuinely
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
