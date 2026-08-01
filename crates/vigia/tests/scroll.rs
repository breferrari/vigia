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
use vigia::{Action, App, Body, Position, Row, View, body_height};
use vigia_core::{Frame, Highlighter, History};

use support::{Scratch, generated, materialise};

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
    app.view(frame, highlighter, history, Body::diff_only(body()))
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
            .view(
                &mut frame,
                &mut highlighter,
                &history,
                Body::diff_only(body()),
            )
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
    // Scrolling past the end must rest on the last **screenful**, not on the
    // last row. Past it draws an empty pane, which in a monitor is
    // indistinguishable from a broken one.
    //
    // > [!warning] This gate used to assert the defect it was named against
    // >
    // > Until [#57](https://github.com/breferrari/vigia/issues/57) the body of
    // > this test read `assert_eq!(view.rows.len(), 1)`, under this name and
    // > under the comment above it. Both describe the right rule; the assertion
    // > pinned the wrong behaviour in place, and it is the strongest kind of
    // > wrong a test can be, because a defect with a green gate over it is one
    // > nobody goes looking for. It survived being read every time this file was
    // > touched.
    // >
    // > The tell, and it generalises: **an exact small count where the rule is
    // > about a bound.** "Rests on content rather than blank" is a claim about
    // > the screen being full; `== 1` is a claim about it being nearly empty.
    // > `SPEC.md` §7 carries it now.
    let scratch = fixture("shell-scroll-bottom");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    app.apply(
        Action::Scroll((SPAN * FILES * 4) as isize),
        &mut frame,
        body(),
    )
    .expect("scroll");
    let view = drawn(&mut app, &mut frame);
    let (rows, top) = (view.rows.len(), view.top);

    assert_eq!(
        rows,
        body(),
        "scrolling far past the end drew {rows} rows of a {} row body, leaving \
         {} blank under a diff with {} rows to spare",
        body(),
        body() - rows,
        FILES * SPAN
    );
    // The bottom of the diff and nowhere else. Asserted through the position as
    // well as the count, because a screen full of the *wrong* rows satisfies the
    // count on its own.
    assert_eq!(
        top.file * SPAN + top.row + body(),
        FILES * SPAN,
        "the screen is full but ends somewhere other than the end of the diff: {top:?}"
    );
    assert!(
        matches!(
            view.rows.last().expect("a drawn row"),
            Row::Line { .. } | Row::Note { .. }
        ),
        "the bottom row is {:?} rather than content",
        view.rows.last()
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
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            Body::diff_only(body()),
        )
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
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            Body::diff_only(body()),
        )
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
            .view(
                &mut frame,
                &mut highlighter,
                &history,
                Body::diff_only(height),
            )
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
        .view(
            &mut frame,
            &mut highlighter,
            &history,
            Body::diff_only(body()),
        )
        .expect("view");
    assert_eq!(view.rows.len(), body());
    assert_eq!(
        view.top, before,
        "dragging the pane short and back lost the reader's place"
    );
}

#[test]
fn only_the_action_that_reads_the_height_is_given_one() {
    // `Action::needs_height` exists so the shell can skip an uncached
    // terminal-size syscall and a `Chrome` allocation on every action that does
    // not read the answer, which matters because a drained trackpad flick is up
    // to sixty-four actions between two paints.
    //
    // The exhaustive match in `needs_height` catches an action nobody
    // classified. It cannot catch one classified **wrongly**, and that failure
    // is silent: the action is handed a zero height and quietly moves the
    // viewport by the wrong amount. So the claim is checked against `App::apply`
    // itself, by driving each action twice with heights that could not both be
    // right and asserting the answer did not depend on which.
    let scratch = fixture("shell-scroll-height");
    let worktree = scratch.worktree();

    // Every action, so a new variant reaches this list by failing to be in it.
    let actions = [
        Action::Scroll(SPAN as isize * 3),
        Action::Scroll(-(SPAN as isize)),
        Action::Top,
        Action::Bottom,
        Action::Redraw,
        Action::ToggleFollow,
        Action::Page(1),
        Action::Page(-1),
    ];

    for action in actions {
        // Two heights far enough apart that any action reading one would land
        // somewhere different. Started from the same place each time.
        let landed: Vec<Position> = [0usize, body()]
            .into_iter()
            .map(|height| {
                let mut frame = worktree.frame();
                materialise(&mut frame);
                let mut app = App::new();
                let mut highlighter = Highlighter::new();
                let history = History::new();
                app.apply(Action::Scroll(SPAN as isize * 8), &mut frame, body())
                    .expect("seed");
                app.apply(action, &mut frame, height).expect("apply");
                app.view(
                    &mut frame,
                    &mut highlighter,
                    &history,
                    Body::diff_only(body()),
                )
                .expect("view")
                .top
            })
            .collect();

        if action.needs_height() {
            assert_ne!(
                landed[0], landed[1],
                "{action:?} says it needs the height and lands in the same place \
                 without one, so either the claim or the action is wrong"
            );
        } else {
            assert_eq!(
                landed[0], landed[1],
                "{action:?} says it does not need the height and moved when it \
                 changed, so the shell is about to hand it a zero"
            );
        }
    }
}

/// Drive the view once and hand back the whole screenful.
///
/// The four gates below are all assertions that a **full** screen came back, so
/// what they compare against is [`body`] itself. `view.rows.len() == body()`
/// reads as arithmetic where what it means is "no blank rows under content that
/// exists".
///
/// Returns the [`View`] rather than the two numbers most callers want, because
/// one of them wants a third thing and a second helper to fetch it would drive
/// the view twice: `App::view` writes its resolved position back, so a second
/// call is a second resolution of a position the first one already moved.
fn drawn(app: &mut App, frame: &mut Frame) -> View {
    let mut highlighter = Highlighter::new();
    let history = History::new();
    app.view(frame, &mut highlighter, &history, Body::diff_only(body()))
        .expect("view")
}

#[test]
fn a_diff_that_shrank_under_the_viewport_still_fills_the_screen() {
    // **The half that was reported**, and the one no fixture in this suite could
    // have produced: every other gate here holds the file list still and moves
    // the window. This moves the file list.
    //
    // The sequence is the one that found it, run literally. A worktree of forty
    // changed files, scrolled deep into, then `git reset --hard` with two
    // untracked files left behind — an agent in the other pane reverting its own
    // work, which is an ordinary event on the pane this tool exists for and
    // exactly when someone looks over. The retained position was reasonable when
    // it was taken and names a row the new diff does not have.
    //
    // What made it monitor-class rather than cosmetic: the header went on
    // truthfully saying `watching · 2 files` over a blank body, so one thing on
    // screen contradicted the other two. `SPEC.md` §11.1 ruled the empty state
    // into existence to stop "nothing changed" and "this has stopped working"
    // looking alike, and this reintroduced that ambiguity by another route.
    // **Tall files, not the shared forty-by-four fixture, and this is the part
    // that took a mutation run to get right.** What survives the shrink is the
    // row *offset within a file*, because the file index is clamped to the new
    // list and the offset is not. Over four-row files the deepest offset
    // reachable is three, which is smaller than anything the surviving file
    // holds, so the walk never reaches the overshoot branch at all and the gate
    // passes against the unfixed code. Sixty-two row files give the offset
    // somewhere to be large.
    //
    // Recorded rather than quietly corrected: this gate asserted the right
    // outcome and reproduced the wrong situation, which is the same shape as the
    // gate #57 was found under and would have been just as green.
    let scratch = Scratch::large_diff("shell-scroll-shrunk", FILES, 30);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    // To the bottom of the old diff, and disengaged from follow the way a
    // reader's own scroll disengages it. A following viewport is dragged back by
    // I5 on the next tick and would never see this.
    app.apply(Action::Scroll(10_000), &mut frame, body())
        .expect("scroll");
    let before = drawn(&mut app, &mut frame).top;
    assert!(
        before.file > 1 && before.row > 25,
        "the fixture did not leave a deep enough offset to be shrunk out from \
         under: {before:?}. The offset is what survives, so a small one makes \
         this gate pass against the defect it is written for"
    );

    // Twenty-five lines, so the surviving file is taller than the screen on its
    // own. That is the common shape and the one that needs no walk backwards,
    // which is the shape that was reported.
    scratch.git(&["reset", "--hard"]);
    scratch.write("kept_one.md", generated(25, "kept-one"));
    scratch.write("kept_two.md", generated(25, "kept-two"));
    materialise(&mut frame);
    assert_eq!(frame.files().len(), 2, "the fixture is not two files");

    let view = drawn(&mut app, &mut frame);
    let (rows, top) = (view.rows.len(), view.top);
    assert_eq!(
        rows,
        body(),
        "the diff shrank under the viewport and the screen came back with {rows} \
         of {} rows drawn, so {} were blank while the header said two files had \
         changed. Top: {top:?}",
        body(),
        body() - rows
    );
}

#[test]
fn a_last_file_shorter_than_the_screen_is_filled_from_the_ones_above_it() {
    // The case a subtraction inside the last file cannot reach, and the reason
    // `last_screenful` walks. When the final file has fewer rows than the body,
    // backing off within it still leaves the screen short: the top has to land
    // in an earlier file, which is a question the forward walk has no way to
    // ask, because its borrow of the current file is live until that file is
    // drawn.
    //
    // Four-row files against a twenty-two row body means six of them fill it, so
    // this exercises a real walk rather than the degenerate one.
    let scratch = fixture("shell-scroll-short-tail");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    app.apply(Action::Scroll(10_000), &mut frame, body())
        .expect("scroll");
    let view = drawn(&mut app, &mut frame);
    let (rows, top) = (view.rows.len(), view.top);

    assert!(
        top.file < FILES - 1,
        "the top stayed in the last file, which is {SPAN} rows and cannot fill a \
         {} row body on its own: {top:?}",
        body()
    );
    assert_eq!(rows, body(), "the walk back left the screen short");
}

#[test]
fn a_diff_shorter_than_the_screen_starts_at_the_top() {
    // The other end of the same rule, and the one that must **not** be "fill the
    // screen": a diff with fewer rows than the body has nothing to fill it with,
    // so the honest picture is every row it has, from the first.
    //
    // Without a floor, `last_screenful` walking back until it has a screenful is
    // a loop that runs off the front of the file list, and the obvious
    // off-by-one puts the top in a file that does not exist —
    // `vigia_core::Frame::diff` **panics** on an index past the end, which this
    // file's own header warns about.
    let scratch = Scratch::large_diff("shell-scroll-short-diff", 2, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    materialise(&mut frame);
    let mut app = App::new();

    app.apply(Action::Scroll(10_000), &mut frame, body())
        .expect("scroll");
    let view = drawn(&mut app, &mut frame);
    let (rows, top) = (view.rows.len(), view.top);

    assert_eq!(
        top,
        Position::default(),
        "a diff shorter than the screen did not start at the top"
    );
    assert_eq!(
        rows,
        2 * SPAN,
        "a two file diff drew {rows} rows rather than the {} it has",
        2 * SPAN
    );
}
