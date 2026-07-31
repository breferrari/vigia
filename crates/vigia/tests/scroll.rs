//! The scroll position, against a real frame.
//!
//! Kept apart from `reads.rs`, which asks what a screen *costs*. This file asks
//! whether it lands in the right place, and the two need different fixtures: a
//! cost measurement wants one file taller than the screen, and scrolling wants
//! many files short enough to move between.
//!
//! The case worth knowing about before reading any of it:
//! [`vigia_core::Frame::diff`] **panics** on an index past the end of the file
//! list, deliberately, and the file list is rebuilt from scratch on every
//! [`vigia_core::Frame::advance`]. A scroll position is exactly the index that
//! outlives that rebuild. So the agent in the other pane committing its work is
//! enough to crash a shell that trusts its own position, with no input on this
//! side at all, and clamping is not defensive programming here but the contract.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use ratatui::layout::Rect;
use vigia::{Action, App, Position, Row, body_height};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, materialise};

/// Files in the scrolling fixture.
const FILES: usize = 40;

/// Rows one file of the fixture occupies: its heading, one hunk header, and the
/// one line it replaced.
const SPAN: usize = 4;

fn body() -> usize {
    // Eighty columns, where the footer is one line whatever the state, so the
    // scroll arithmetic below is not entangled with I6's two-line footer.
    body_height(
        Rect::new(0, 0, 80, 24),
        &App::new().chrome("fixture", None),
        FILES,
    )
}

/// Many files, each a single rewritten line, so scrolling crosses them quickly.
fn fixture(name: &str) -> Scratch {
    Scratch::large_diff(name, FILES, 1)
}

/// Drive one action and report where the next frame would start from.
fn after(
    app: &mut App,
    frame: &mut Frame,
    highlighter: &mut Highlighter,
    history: &History,
    action: Action,
) -> Position {
    app.apply(action, frame, body()).expect("apply");
    app.view(frame, highlighter, history, body())
        .expect("view")
        .top
}

#[test]
fn the_fixture_is_the_shape_the_rest_of_this_file_assumes() {
    // Every assertion below is arithmetic over SPAN, so it is worth one test
    // rather than a comment. Get this wrong and the others pass vacuously.
    let scratch = fixture("shell-scroll-shape");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);

    assert_eq!(frame.files().len(), FILES);
    for index in [0, FILES / 2, FILES - 1] {
        assert_eq!(
            vigia::rows_in(&mut frame, index).expect("rows"),
            SPAN,
            "file {index} is not {SPAN} rows"
        );
    }
}

#[test]
fn scrolling_down_and_back_up_returns_to_where_it_started() {
    // The two directions are different code: down hands the overrun to
    // `View::collect` to carry across files, up has to walk back and ask each
    // file how tall it is. A round trip is the cheapest way to catch either one
    // being off by a row.
    let scratch = fixture("shell-scroll-round");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    for rows in [1, 3, SPAN as isize, 17, (SPAN * 12) as isize] {
        let start = app
            .view(&mut frame, &mut highlighter, &history, body())
            .expect("view")
            .top;
        let moved = after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Scroll(rows),
        );
        assert_ne!(
            moved, start,
            "scrolling {rows} rows from {start:?} moved nowhere"
        );
        let back = after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Scroll(-rows),
        );
        assert_eq!(
            back, start,
            "scrolling {rows} rows down from {start:?} and back up landed on \
             {back:?}"
        );
    }
}

#[test]
fn scrolling_off_the_end_of_a_file_continues_into_the_next_one() {
    // The reason the position is a file plus an offset rather than one row
    // number. Each of these steps crosses a file boundary, and the row it lands
    // on has to be the next file's, not the same file's clamped.
    let scratch = fixture("shell-scroll-cross");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    let mut seen = Vec::new();
    for _ in 0..(SPAN * 3) {
        seen.push(after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Scroll(1),
        ));
    }

    let expected: Vec<Position> = (1..=SPAN * 3)
        .map(|n| Position {
            file: n / SPAN,
            row: n % SPAN,
        })
        .collect();
    assert_eq!(
        seen, expected,
        "one row at a time did not walk file boundaries cleanly"
    );
}

#[test]
fn scrolling_up_walks_file_boundaries_the_same_way_down_does() {
    // The mirror of the test above, and not a duplicate of it. Down hands the
    // overrun to `View::collect` to carry; up walks back and asks each file how
    // tall it is, so the two boundary crossings are different code.
    //
    // A round trip cannot tell them apart, which is the point of doing this
    // separately. Every round trip in this file ends at file zero, and file zero
    // clamps to row zero, so a step that lands one row short at every boundary
    // still arrives at exactly (0, 0). Found by mutation: subtracting one from
    // the previous file's height left the whole suite green.
    let scratch = fixture("shell-scroll-back");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    let start = SPAN * 3;
    let landed = after(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Action::Scroll(start as isize),
    );
    assert_eq!(
        landed,
        Position { file: 3, row: 0 },
        "the walk did not start"
    );

    let mut seen = Vec::new();
    for _ in 0..start {
        seen.push(after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Scroll(-1),
        ));
    }

    // Absolute row `start` counting down, resolved back into a file and an offset.
    let expected: Vec<Position> = (0..start)
        .map(|step| {
            let absolute = start - step - 1;
            Position {
                file: absolute / SPAN,
                row: absolute % SPAN,
            }
        })
        .collect();
    assert_eq!(
        seen, expected,
        "stepping up one row at a time did not walk file boundaries cleanly"
    );
}

#[test]
fn the_bottom_of_the_diff_is_content_rather_than_blank() {
    // Scrolling past the end must rest on the last row, not past it. Past it
    // draws an empty pane, which in a monitor is indistinguishable from a broken
    // one.
    let scratch = fixture("shell-scroll-bottom");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    let landed = after(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Action::Scroll((SPAN * FILES * 4) as isize),
    );
    assert_eq!(
        landed,
        Position {
            file: FILES - 1,
            row: SPAN - 1
        },
        "scrolling far past the end did not rest on the last row"
    );

    let view = app
        .view(&mut frame, &mut highlighter, &history, body())
        .expect("view");
    assert_eq!(
        view.rows.len(),
        1,
        "the last row of the diff drew {} rows",
        view.rows.len()
    );
    assert!(
        matches!(view.rows[0], Row::Line { .. }),
        "the bottom row is {:?} rather than content",
        view.rows[0]
    );
}

#[test]
fn home_and_end_go_to_the_first_and_last_file() {
    // `End` is the last *file*, from its top, not the last row of the whole
    // diff. Finding that row means adding up every file's height, and every
    // height means a diff, which is the read I4 exists to forbid. Asserted rather
    // than left to a comment because it is a deliberate limit, not an oversight.
    let scratch = fixture("shell-scroll-ends");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    assert_eq!(
        after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Bottom
        ),
        Position {
            file: FILES - 1,
            row: 0
        }
    );
    assert_eq!(
        after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Top
        ),
        Position { file: 0, row: 0 }
    );
}

#[test]
fn a_page_keeps_one_row_of_overlap() {
    // A page that moved a whole screen would leave nothing shared between the
    // two, and a reader loses their place at the seam. One row of overlap is what
    // every pager does and it is worth one assertion.
    let scratch = fixture("shell-scroll-page");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    let rows = body();
    let landed = after(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Action::Page(1),
    );
    let absolute = landed.file * SPAN + landed.row;
    assert_eq!(
        absolute,
        rows - 1,
        "a page of {rows} rows moved {absolute}, so the screens do not overlap"
    );

    assert_eq!(
        after(
            &mut app,
            &mut frame,
            &mut highlighter,
            &history,
            Action::Page(-1)
        ),
        Position { file: 0, row: 0 },
        "paging back did not return to the top"
    );
}

#[test]
fn a_position_survives_the_file_it_pointed_into_disappearing() {
    // The panic this whole clamp exists for. The reader scrolls to the last file,
    // the agent in the other pane commits, and the file list the position was
    // resolved against no longer exists. `Frame::diff` panics on that index by
    // design, so nothing downstream would survive it.
    let scratch = fixture("shell-scroll-shrink");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    let far = after(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Action::Bottom,
    );
    assert_eq!(
        far.file,
        FILES - 1,
        "the fixture never reached its last file"
    );

    // Half the changes go away. The position now names a file past the end.
    for index in (FILES / 2)..FILES {
        scratch.git(&["checkout", "--", &format!("src/mod_{index}.rs")]);
    }
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), FILES / 2, "the fixture did not shrink");

    let view = app
        .view(&mut frame, &mut highlighter, &history, body())
        .expect("view");
    assert_eq!(
        view.top.file,
        FILES / 2 - 1,
        "the position was not pulled back to the last file that still exists"
    );
    assert!(
        !view.rows.is_empty(),
        "the clamp landed somewhere that draws nothing"
    );

    // And all the way to nothing, which is the other end of the same case.
    scratch.commit_all("everything");
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), 0, "the worktree is not clean");

    let view = app
        .view(&mut frame, &mut highlighter, &history, body())
        .expect("view");
    assert_eq!(view.files, 0);
    assert_eq!(view.top, Position::default());
    assert!(view.rows.is_empty());
}

#[test]
fn a_screen_with_no_room_for_a_body_still_resolves() {
    // A pane dragged down to two rows leaves the header and the footer and
    // nothing between them. `body_height` returns zero there, and asking for zero
    // rows has to be an answer rather than a panic or an unclamped position.
    let scratch = fixture("shell-scroll-flat");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();

    // Scroll somewhere first, or the assertion below cannot tell a preserved
    // position from a reset one: they are both (0, 0) at the top of the diff.
    let before = after(
        &mut app,
        &mut frame,
        &mut highlighter,
        &history,
        Action::Scroll((SPAN * 7 + 2) as isize),
    );
    assert_eq!(
        before,
        Position { file: 7, row: 2 },
        "the scroll did not land"
    );

    for height in [0, 1] {
        let view = app
            .view(&mut frame, &mut highlighter, &history, height)
            .expect("view");
        assert_eq!(view.rows.len(), height);
        assert_eq!(view.files, FILES);
        // A frame with no room to draw must not decide where the reader is. It
        // resolved nothing, so it has nothing to say about the position, and
        // reporting one would drag the reader back to the top of the file for as
        // long as the pane stayed short.
        assert_eq!(
            view.top, before,
            "a {height}-row screen moved the reader from {before:?} to {:?}",
            view.top
        );
    }

    // And the position survives being dragged short and back, which is the whole
    // sequence a reader actually performs.
    let view = app
        .view(&mut frame, &mut highlighter, &history, body())
        .expect("view");
    assert_eq!(view.rows.len(), body());
    assert_eq!(
        view.top, before,
        "dragging the pane short and back lost the reader's place"
    );
}
